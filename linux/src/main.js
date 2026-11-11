// Biblifilm pour Linux : analyse un dossier, identifie chaque fichier en ligne
// (titre complet, épisode, synopsis, affiche) et le lit avec mpv ou le lecteur
// intégré, en suivant la reprise de lecture et les épisodes vus.
'use strict';

const { app, BrowserWindow, ipcMain, dialog, shell, Menu, clipboard, powerSaveBlocker, session } = require('electron');
const fs = require('fs');
const fsp = require('fs/promises');
const path = require('path');
const Parser = require('./parser');
const S = require('./shared');
const { MetadataService, TMDB } = require('./metadata');
const Player = require('./player');
const { Settings, readJSON, writeJSON } = require('./store');
const SelfUpdate = require('./selfupdate');

app.setName('Biblifilm');
const REPO = 'VGioM/Biblifilm';
// Les tests de bout en bout pointent ailleurs, pour ne jamais toucher l'historique réel.
if (process.env.BIBLIFILM_SUPPORT_DIR) app.setPath('userData', process.env.BIBLIFILM_SUPPORT_DIR);
const supportDir = app.getPath('userData');
fs.mkdirSync(supportDir, { recursive: true });

const settings = new Settings(path.join(supportDir, 'settings.json'), {
  tmdbAPIKey: '', libraryPath: null, preferredLanguage: 'fr-FR', includeAudio: true, skipSmallFiles: true,
  autoFetchMetadata: true, watchFolder: true, player: 'auto', fullscreen: true, lastSelection: 'all',
  sortOrder: 'title', inspectorVisible: true, seriesExpanded: true, trackPrefs: {},
});
const metadata = new MetadataService(path.join(supportDir, 'metadata-cache.json'));

let win = null;

const M = {
  items: [], isScanning: false, scanProgress: '', isFetching: false, metadataProgress: 0, errorMessage: null,
};

// ── Envoi de l'état à l'interface, par morceaux ────────────────────────────

const dirty = new Set();
let pushTimer = null;
function push(...parts) {
  for (const p of parts.length ? parts : ['items', 'records', 'status']) dirty.add(p);
  if (pushTimer) return;
  pushTimer = setTimeout(() => {
    pushTimer = null;
    if (!win || win.isDestroyed()) { dirty.clear(); return; }
    const msg = {};
    if (dirty.has('items')) msg.items = M.items;
    if (dirty.has('records')) msg.records = Watch.records;
    if (dirty.has('status') || dirty.has('items')) {
      const players = Player.detect();
      msg.status = {
        isScanning: M.isScanning, scanProgress: M.scanProgress, isFetching: M.isFetching, metadataProgress: M.metadataProgress,
        errorMessage: M.errorMessage, settings: settings.all(), hasTMDBKey: !!settings.get('tmdbAPIKey').trim(),
        players: { mpv: players.mpv && players.mpv.label, vlc: players.vlc && players.vlc.label },
        cacheCount: metadata.count, autoUpdate: SelfUpdate.automatic(), version: app.getVersion(),
      };
    }
    dirty.clear();
    win.webContents.send('state', msg);
  }, 80);
}

// ── Historique de visionnage ───────────────────────────────────────────────

const Watch = {
  file: path.join(supportDir, 'watch-state.json'),
  records: {},
  saveTimer: null,
  lastQuietPush: 0,
  load() { this.records = readJSON(this.file, {}); },

  /** Appelé pendant la lecture. `routine` : la sauvegarde périodique d'un film qui continue. */
  update(id, position, duration, routine) {
    const before = this.records[id];
    const r = { position: 0, duration: 0, watched: false, lastPlayed: Date.now(), ...(before || {}) };
    r.position = position;
    if (duration > 0) r.duration = duration;
    r.lastPlayed = Date.now();
    if (S.progressOf(r) >= S.COMPLETION) r.watched = true;
    this.records[id] = r;
    // Les vues n'entendent parler d'une sauvegarde de routine que toutes les 15 s,
    // ou tout de suite quand ce qu'elles montrent change (repris, vu, nouveau).
    const visible = !before || before.watched !== r.watched || S.isResumable(before) !== S.isResumable(r);
    if (!routine || visible || Date.now() - this.lastQuietPush > 15000) { this.lastQuietPush = Date.now(); push('records'); }
    this.scheduleSave();
  },

  setWatched(ids, watched) {
    for (const id of ids) {
      const r = { position: 0, duration: 0, ...(this.records[id] || {}) };
      r.watched = watched;
      r.lastPlayed = Date.now();
      // Non vu efface la reprise ; vu la met à la fin.
      r.position = watched ? r.duration : 0;
      this.records[id] = r;
    }
    push('records');
    this.scheduleSave();
  },

  scheduleSave() {
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.saveNow(), 1500);
  },

  saveNow() {
    clearTimeout(this.saveTimer);
    // L'ancienne version est gardée en sauvegarde : l'historique est irremplaçable.
    writeJSON(this.file, this.records, { backup: Object.keys(this.records).length > 0 });
  },

  restore() {
    const data = readJSON(this.file.replace(/\.json$/, '.backup.json'), null);
    if (!data) return 0;
    this.records = data;
    this.saveNow();
    push('records');
    return Object.keys(data).length;
  },
};
Watch.load();

// ── Analyse du dossier ─────────────────────────────────────────────────────

const skippedDirectories = new Set(['.Trash', '.Trash-1000', '$RECYCLE.BIN', 'node_modules', '.git', '.svn', 'lost+found',
  '@eaDir', 'Extras', 'Sample', 'Samples', 'Featurettes']);
const MIN_VIDEO = 20 * 1024 * 1024;   // en dessous : un extrait ou une bande-annonce

async function scanFolder(root, opts, progress) {
  const items = [];
  async function walk(dir) {
    let entries;
    try { entries = await fsp.readdir(dir, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      if (e.name.startsWith('.')) continue;
      const p = path.join(dir, e.name);
      if (e.isDirectory()) { if (!skippedDirectories.has(e.name)) await walk(p); continue; }
      if (!e.isFile()) continue;
      const ext = path.extname(e.name).slice(1).toLowerCase();
      const isVideo = Parser.videoExtensions.has(ext), isAudio = Parser.audioExtensions.has(ext);
      if (!isVideo && !(isAudio && opts.includeAudio)) continue;
      let st;
      try { st = await fsp.stat(p); } catch { continue; }
      if (opts.skipSmallFiles && isVideo && st.size < MIN_VIDEO) continue;
      // Le dossier parent est le meilleur indice quand le nom ne porte pas la série.
      const parent = path.basename(dir);
      const hint = dir === root ? null : parent;
      items.push({ id: p, path: p, filename: e.name, ext, parsed: Parser.parse(p, hint), size: st.size, modified: st.mtimeMs, metadata: null });
      if (items.length % 25 === 0 && progress) progress(items.length);
    }
  }
  await walk(root);
  return items.sort((a, b) => S.cmp(S.displayTitle(a), S.displayTitle(b)));
}

let scanToken = 0;
async function rescan() {
  const root = settings.get('libraryPath');
  if (!root) return;
  const token = ++scanToken;
  stopMetadata();
  M.isScanning = true; M.scanProgress = 'Analyse…';
  push('status');
  const found = await scanFolder(root, settings.all(), (n) => { M.scanProgress = `${n} fichiers trouvés…`; push('status'); });
  if (token !== scanToken) return;
  M.items = found;
  M.isScanning = false; M.scanProgress = '';
  applyCached();
  push();
  if (settings.get('autoFetchMetadata')) fetchMetadata();
  startWatching();
}

/** Relit le dossier et fusionne, en gardant les fiches déjà résolues. */
async function refreshIncrementally() {
  const root = settings.get('libraryPath');
  if (!root || M.isScanning) return;
  const found = await scanFolder(root, settings.all());
  const prev = new Map(M.items.map((i) => [i.id, i]));
  let added = 0;
  for (const it of found) {
    const old = prev.get(it.id);
    if (old) it.metadata = old.metadata; else added++;
  }
  if (!added && found.length === M.items.length) return;
  M.items = found;
  applyCached();
  push('items');
  if (settings.get('autoFetchMetadata')) fetchMetadata();
}

function applyCached() {
  const lang = settings.get('preferredLanguage');
  for (const it of M.items) {
    if (it.metadata) continue;
    const hit = metadata.cached(it.parsed, lang);
    if (hit) it.metadata = hit;
  }
}

let watcher = null, watchTimer = null;
function startWatching() {
  if (watcher) { watcher.close(); watcher = null; }
  const root = settings.get('libraryPath');
  if (!settings.get('watchFolder') || !root) return;
  try {
    watcher = fs.watch(root, { recursive: true }, () => {
      // Les événements sont regroupés sur 4 secondes : un téléchargement en
      // cours ne doit pas déclencher dix analyses.
      clearTimeout(watchTimer);
      watchTimer = setTimeout(refreshIncrementally, 4000);
    });
    watcher.on('error', () => {});
  } catch { watcher = null; }
}

// ── Métadonnées ────────────────────────────────────────────────────────────

const metaSettings = () => {
  const key = settings.get('tmdbAPIKey').trim();
  return { tmdbKey: key || null, language: settings.get('preferredLanguage') };
};

let metaRun = 0;
function stopMetadata() { metaRun++; M.isFetching = false; M.metadataProgress = 0; }

/** Cherche les fiches manquantes, quatre à la fois. */
async function fetchMetadata(force) {
  const run = ++metaRun;
  const snap = metaSettings();
  if (force) { metadata.clear(); for (const it of M.items) it.metadata = null; }
  const targets = M.items.filter((i) => force || !i.metadata);
  if (!targets.length) { M.isFetching = false; push('status'); return; }
  M.isFetching = true; M.metadataProgress = 0;
  push('status');
  let next = 0, done = 0;
  const worker = async () => {
    while (next < targets.length && run === metaRun) {
      const it = targets[next++];
      let meta = null;
      try { meta = await metadata.metadata(it.parsed, snap); } catch { meta = null; }
      if (run !== metaRun) return;
      it.metadata = meta || { matched: false };
      done++;
      M.metadataProgress = done / targets.length;
      // Enregistré au fil de l'eau : quitter pendant la première analyse ne perd pas le travail fait.
      if (done % 25 === 0) metadata.flush();
      push('items', 'status');
    }
  };
  await Promise.all([worker(), worker(), worker(), worker()]);
  metadata.flush();
  if (run === metaRun) { M.isFetching = false; M.metadataProgress = 0; push('status'); }
}

function retryUnmatched() {
  metadata.forgetUnmatched();
  for (const it of M.items) if (!S.matched(it)) it.metadata = null;
  fetchMetadata();
}

/** Ce qu'on cherche : le nom du fichier, ou la correction tapée, nettoyée de son année. */
function searchName(it, override) {
  const p = { ...it.parsed };
  if (override && override.trim()) {
    const c = Parser.normalizeUserTitle(override);
    p.title = c.title;
    if (c.year) p.year = c.year;
  }
  return p;
}

async function refetch(id, override) {
  const it = M.items.find((i) => i.id === id);
  if (!it) return;
  const snap = metaSettings();
  const query = searchName(it, override);
  it.metadata = null;
  push('items');
  // Repartir de zéro, sinon la réponse en cache (peut-être fausse) revient tout droit.
  metadata.forget(it.parsed, snap.language);
  metadata.forget(query, snap.language);
  let meta = null;
  try { meta = await metadata.metadata(query, snap); } catch { /* raté */ }
  // Rangé sous la clé du fichier, pas du titre tapé, pour survivre au relancement.
  metadata.store(meta || { matched: false }, it.parsed, snap.language);
  it.metadata = meta || { matched: false };
  push('items');
}

async function candidates(id, override) {
  const it = M.items.find((i) => i.id === id);
  if (!it) return [];
  try { return await metadata.candidates(searchName(it, override), metaSettings()); } catch { return []; }
}

/** Le choix de l'utilisateur, appliqué à tous les épisodes de la même série. */
async function applyCandidate(id, cand, override) {
  const it = M.items.find((i) => i.id === id);
  if (!it) return;
  const snap = metaSettings();
  const query = searchName(it, override);
  const key = Parser.showKey(it.parsed);
  const targets = [it, ...(it.parsed.kind === 'episode' ? M.items.filter((x) => x.id !== it.id && Parser.showKey(x.parsed) === key) : [])];
  for (const t of targets) {
    const ask = { ...t.parsed, title: query.title };
    if (query.year) ask.year = query.year;
    let meta = null;
    try { meta = await metadata.forCandidate(cand, ask, snap); } catch { /* suivant */ }
    if (!meta) continue;
    metadata.store(meta, t.parsed, snap.language);
    t.metadata = meta;
    push('items');
  }
  metadata.flush();
}

// ── Lecture ────────────────────────────────────────────────────────────────

let playing = null;
let blocker = null;

function chooseMpv() {
  const pref = settings.get('player');
  if (pref === 'builtin') return null;
  return Player.detect().mpv;
}

/** Lit un fichier ; un épisode met en file la suite de sa saison. */
function play(id, fromStart, followers) {
  const it = M.items.find((i) => i.id === id);
  if (!it) return;
  if (!fs.existsSync(it.path)) { M.errorMessage = `Le fichier est introuvable :\n${it.path}`; push('status'); return; }
  let rest = followers;
  if (!rest) {
    const g = S.seasonOf(M.items, it);
    rest = g ? g.episodes.slice(g.episodes.findIndex((e) => e.id === it.id) + 1) : [];
  }
  const rec = Watch.records[it.id];
  const start = !fromStart && S.isResumable(rec) ? rec.position : 0;
  const queue = [it, ...rest].map((x) => ({ id: x.id, path: x.path, title: S.playerTitle(x), subtitle: S.playerSubtitle(x), ext: x.ext }));
  const mpv = chooseMpv();
  if (mpv) {
    if (playing) playing.stop();
    const prefs = settings.get('trackPrefs') || {};
    const s = new Player.MpvSession(mpv, queue, start, { fullscreen: settings.get('fullscreen'), trackPrefs: prefs }, {
      update: (vid, pos, dur, routine) => Watch.update(vid, pos, dur, routine),
      tracks: (kind, label) => { const p = { ...(settings.get('trackPrefs') || {}) }; p[kind] = label; settings.set('trackPrefs', p); s.opts.trackPrefs = p; },
      ended: (err) => {
        if (playing === s) playing = null;
        Watch.saveNow();
        push('records');
        if (err) { M.errorMessage = `mpv n'a pas pu démarrer : ${err.message}`; push('status'); }
      },
    });
    playing = s;
    s.launch();
    return;
  }
  // Sans mpv : le lecteur intégré, dans la fenêtre.
  if (win) win.webContents.send('playBuiltin', { queue, start });
}

function playEntry(entryId) {
  const e = S.build(M.items).find((x) => S.entryId(x) === entryId);
  if (!e) return;
  if (e.type === 'single') return play(e.item.id);
  const W = S.Watch(Watch.records);
  const start = W.nextEpisode(e) || e.episodes[0];
  if (start) play(start.id);
}

function playAll(ids) {
  const list = ids.map((id) => M.items.find((i) => i.id === id)).filter(Boolean);
  if (list.length) play(list[0].id, false, list.slice(1));
}

function openExternally(ids) {
  const files = ids.map((id) => M.items.find((i) => i.id === id)).filter(Boolean).map((i) => i.path).filter((p) => fs.existsSync(p));
  if (!files.length) return;
  if (!Player.openInVLC(files, Player.detect().vlc)) shell.openPath(files[0]);
}

// ── Canaux de l'interface ──────────────────────────────────────────────────

let keyTimer = null;
let lastKnownKey = settings.get('tmdbAPIKey').trim();

const calls = {
  ready: () => push(),
  chooseFolder: async () => {
    const r = await dialog.showOpenDialog(win, {
      title: 'Choisissez le dossier contenant vos films et séries', buttonLabel: 'Choisir',
      defaultPath: settings.get('libraryPath') || app.getPath('videos'), properties: ['openDirectory'],
    });
    if (r.canceled || !r.filePaths[0]) return;
    settings.set('libraryPath', r.filePaths[0]);
    M.items = [];
    rescan();
  },
  rescan: () => rescan(),
  fetchMetadata: (force) => fetchMetadata(!!force),
  refetch: (id, override) => refetch(id, override),
  refetchMany: (ids) => { for (const id of ids) refetch(id); },
  candidates: (id, override) => candidates(id, override),
  applyCandidate: (id, cand, override) => applyCandidate(id, cand, override),
  play: (id, fromStart) => play(id, !!fromStart),
  playEntry: (entryId) => playEntry(entryId),
  playAll: (ids) => playAll(ids),
  openExternally: (ids) => openExternally(ids),
  reveal: (id) => { const it = M.items.find((i) => i.id === id); if (it) shell.showItemInFolder(it.path); },
  copyPath: (id) => { const it = M.items.find((i) => i.id === id); if (it) clipboard.writeText(it.path); },
  setWatched: (ids, watched) => Watch.setWatched(ids, !!watched),
  watchUpdate: (id, pos, dur, routine) => Watch.update(id, pos, dur, routine),
  saveWatch: () => Watch.saveNow(),
  restoreWatch: () => Watch.restore(),
  inhibit: (on) => {
    // L'écran reste allumé tant qu'un film est vraiment en lecture, pas en pause.
    if (on && blocker == null) blocker = powerSaveBlocker.start('prevent-display-sleep');
    if (!on && blocker != null) { powerSaveBlocker.stop(blocker); blocker = null; }
  },
  setSetting: (key, value) => {
    const before = settings.get(key);
    settings.set(key, value);
    if (key === 'watchFolder') startWatching();
    if ((key === 'includeAudio' || key === 'skipSmallFiles') && before !== value) rescan();
    if (key === 'preferredLanguage' && before !== value) { for (const it of M.items) it.metadata = null; applyCached(); fetchMetadata(); }
    if (key === 'tmdbAPIKey') {
      // Coller une clé relance tout ce qui avait échoué sans elle, une fois la clé entièrement tapée.
      clearTimeout(keyTimer);
      keyTimer = setTimeout(() => {
        const k = String(value || '').trim();
        if (k !== lastKnownKey && k.length >= 20) { lastKnownKey = k; retryUnmatched(); }
      }, 2000);
    }
    push('status');
  },
  testKey: async () => {
    const problem = await TMDB.validate(settings.get('tmdbAPIKey'));
    if (!problem) fetchMetadata(true);
    return problem;
  },
  clearCache: async () => {
    metadata.clear();
    await session.defaultSession.clearCache();
    push('status');
  },
  cacheInfo: async () => ({ count: metadata.count, bytes: await session.defaultSession.getCacheSize() }),
  detectPlayers: () => { Player.detect(true); push('status'); },
  setAutoUpdate: (on) => { SelfUpdate.setAutomatic(on); buildMenu(); push('status'); },
  checkUpdates: () => SelfUpdate.checkNow(),
  openExternal: (url) => { if (/^https:\/\//.test(url)) shell.openExternal(url); },
  clearError: () => { M.errorMessage = null; push('status'); },
};

ipcMain.handle('call', async (_e, channel, ...args) => {
  const fn = calls[channel];
  if (!fn) throw new Error(`canal inconnu : ${channel}`);
  return fn(...args);
});

// ── Fenêtre et menus ───────────────────────────────────────────────────────

const send = (ev, ...a) => win && win.webContents.send(ev, ...a);

function buildMenu() {
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    { label: 'Fichier', submenu: [
      { label: 'Choisir un dossier…', accelerator: 'CmdOrCtrl+O', click: () => calls.chooseFolder() },
      { label: 'Analyser à nouveau', accelerator: 'CmdOrCtrl+R', click: () => rescan() },
      { label: 'Rechercher les infos manquantes', click: () => fetchMetadata() },
      { type: 'separator' },
      { label: 'Réglages…', accelerator: 'CmdOrCtrl+,', click: () => send('showSettings') },
      { type: 'separator' },
      { role: 'quit', label: 'Quitter' },
    ] },
    { label: 'Édition', submenu: [
      { role: 'undo', label: 'Annuler' }, { role: 'redo', label: 'Rétablir' }, { type: 'separator' },
      { role: 'cut', label: 'Couper' }, { role: 'copy', label: 'Copier' }, { role: 'paste', label: 'Coller' },
      { role: 'selectAll', label: 'Tout sélectionner' }, { type: 'separator' },
      { label: 'Rechercher', accelerator: 'CmdOrCtrl+F', click: () => send('focusSearch') },
    ] },
    { label: 'Affichage', submenu: [
      { label: 'Retour à la bibliothèque', accelerator: 'Alt+Left', click: () => send('back') },
      { label: 'Afficher/masquer les informations', accelerator: 'CmdOrCtrl+I', click: () => send('toggleInspector') },
      { type: 'separator' },
      { role: 'resetZoom', label: 'Taille réelle' }, { role: 'zoomIn', label: 'Agrandir' }, { role: 'zoomOut', label: 'Réduire' },
      { type: 'separator' }, { role: 'togglefullscreen', label: 'Plein écran' }, { role: 'toggleDevTools', label: 'Outils de développement' },
    ] },
    { label: 'Aide', submenu: [
      { label: SelfUpdate.menuTitle, click: () => SelfUpdate.checkNow() },
      { label: SelfUpdate.automaticTitle, type: 'checkbox', checked: SelfUpdate.automatic(), click: (i) => { SelfUpdate.setAutomatic(i.checked); push('status'); } },
      { type: 'separator' },
      { label: 'Page du projet', click: () => shell.openExternal(`https://github.com/${REPO}`) },
      { label: `À propos de Biblifilm ${app.getVersion()}`, click: () => dialog.showMessageBox(win, { message: `Biblifilm ${app.getVersion()}`, detail: 'Bibliothèque de films et séries, identifiés en ligne (TMDB, TVmaze, Wikidata), lus avec mpv.' }) },
    ] },
  ]));
}

function createWindow() {
  win = new BrowserWindow({
    width: 1280, height: 820, minWidth: 940, minHeight: 580, title: 'Biblifilm', backgroundColor: '#1c1c1e', autoHideMenuBar: true,
    icon: path.join(__dirname, '..', 'resources', 'icon.png'),
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true, nodeIntegration: false, enableBlinkFeatures: 'AudioVideoTracks' },
  });
  win.loadFile(path.join(__dirname, 'renderer', 'index.html'));
  win.webContents.setWindowOpenHandler(({ url }) => { if (/^https:\/\//.test(url)) shell.openExternal(url); return { action: 'deny' }; });
  win.webContents.on('will-navigate', (e) => e.preventDefault());
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => { if (win) { if (win.isMinimized()) win.restore(); win.focus(); } });
  app.whenReady().then(() => {
    SelfUpdate.start(REPO, settings);
    buildMenu();
    createWindow();
    Player.detect();
    rescan();
  });
  // Une dernière écriture en quittant, sinon une fermeture en pleine lecture
  // perd jusqu'à deux secondes — assez pour effacer une reprise en début d'épisode.
  app.on('before-quit', () => {
    if (playing) playing.save(false);
    Watch.saveNow();
    metadata.flush();
  });
  app.on('window-all-closed', () => app.quit());
}
