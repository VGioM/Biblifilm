// selfupdate.js — met l'app à jour depuis ses releases GitHub (version Linux).
//
// Le même fichier vit dans Biblifilm, Biblijeu et Recomps (branche linux) :
// une correction faite ici se reporte dans les deux autres. C'est l'équivalent
// de SelfUpdate.swift côté Mac.
//
// Au lancement (au plus une fois toutes les 20 h, sauf si la recherche
// automatique est désactivée), l'app lit la dernière release de son dépôt. Une
// même release porte un fichier par plateforme : seul
// `<App>-<version>-linux-x86_64.AppImage` est pris ici, jamais le zip macOS.
// Si la version est plus récente, l'app propose de l'installer : l'AppImage est
// téléchargée à côté de celle en place, vérifiée (empreinte SHA-256 publiée par
// GitHub quand elle existe, en-tête ELF, taille), rendue exécutable, puis
// échangée avec l'ancienne par un simple renommage, et l'app se relance.
//
// Hors AppImage (lancement depuis les sources), rien n'est remplacé : la page
// de la release est proposée à la place.
'use strict';

const { app, dialog, shell, BrowserWindow } = require('electron');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { spawn } = require('child_process');

const ASSET_SUFFIX = '-linux-x86_64.appimage';

let repo = '';
let store = null;      // { get(key, def), set(key, value) }
let busy = false;

const KEYS = {
  automatic: 'SelfUpdate.automatic',
  lastCheck: 'SelfUpdate.lastCheck',
  skipped: 'SelfUpdate.skippedVersion',
};

function clean(tag) {
  let t = String(tag || '').trim();
  if (t.toLowerCase().startsWith('v')) t = t.slice(1);
  return t;
}

function parts(v) {
  return clean(v).split('.').map((p) => parseInt(p, 10) || 0);
}

/** Vrai quand `a` est strictement plus récente que `b`. */
function isNewer(a, b) {
  const x = parts(a), y = parts(b);
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const p = x[i] || 0, q = y[i] || 0;
    if (p !== q) return p > q;
  }
  return false;
}

/** L'AppImage Linux de la release, et elle seule. */
function linuxAsset(assets) {
  return (assets || []).find((a) => a.name.toLowerCase().endsWith(ASSET_SUFFIX))
    || (assets || []).find((a) => a.name.toLowerCase().endsWith('.appimage') && /x86_64|amd64|x64/i.test(a.name))
    || null;
}

/** Les notes telles qu'on les lit, sans le Markdown ni la partie « Installation ». */
function readableNotes(body) {
  let text = String(body || '').replace(/\r\n/g, '\n');
  const cut = text.indexOf('## Installation');
  if (cut >= 0) text = text.slice(0, cut);
  return text.split('\n').map((l) => {
    let s = l.replace(/^#+/, '');
    if (s.startsWith('- ')) s = '•' + s.slice(1);
    return s.replace(/\*\*/g, '').replace(/`/g, '').trim();
  }).join('\n').trim();
}

const automatic = () => store.get(KEYS.automatic, true);
function setAutomatic(on) { store.set(KEYS.automatic, !!on); }

async function latest(feed) {
  const url = feed || `https://api.github.com/repos/${repo}/releases/latest`;
  let res;
  try {
    res = await fetch(url, {
      headers: { Accept: 'application/vnd.github+json', 'User-Agent': `${app.getName()}-updater` },
      signal: AbortSignal.timeout(15000),
    });
  } catch (e) { throw new Error(`GitHub n'a pas répondu correctement (${e.message}).`); }
  if (res.status !== 200) throw new Error(`GitHub n'a pas répondu correctement (code ${res.status}).`);
  const rel = await res.json();
  const asset = linuxAsset(rel.assets);
  if (!asset) throw new Error("La dernière release ne contient pas encore de version Linux.");
  return {
    version: clean(rel.tag_name),
    notes: readableNotes(rel.body),
    page: rel.html_url || `https://github.com/${repo}/releases`,
    download: asset.browser_download_url,
    size: asset.size || 0,
    // GitHub publie l'empreinte de chaque fichier (« sha256:… ») ; les
    // anciennes releases n'en ont pas.
    digest: typeof asset.digest === 'string' && asset.digest.startsWith('sha256:') ? asset.digest.slice(7) : null,
  };
}

function humanSize(n) {
  if (!n) return '';
  const u = ['o', 'Ko', 'Mo', 'Go'];
  let i = 0; let v = n;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
  return `${v.toFixed(v < 10 && i ? 1 : 0).replace('.', ',')} ${u[i]}`;
}

function parentWindow() {
  return BrowserWindow.getFocusedWindow() || BrowserWindow.getAllWindows()[0] || null;
}

async function tell(title, detail) {
  await dialog.showMessageBox(parentWindow(), { type: 'info', message: title, detail, buttons: ['OK'] });
}

async function check(interactive) {
  if (busy || !repo) return;
  busy = true;
  try {
    let release;
    try {
      release = await latest();
      store.set(KEYS.lastCheck, Date.now());
    } catch (e) {
      if (interactive) await tell('Impossible de vérifier les mises à jour', e.message);
      return;
    }
    const current = app.getVersion();
    if (!isNewer(release.version, current)) {
      if (interactive) await tell(`${app.getName()} est à jour`, `Vous avez la dernière version (${current}).`);
      return;
    }
    if (!interactive && store.get(KEYS.skipped, null) === release.version) return;

    const size = release.size ? ` (${humanSize(release.size)})` : '';
    const canReplace = !!process.env.APPIMAGE;
    const { response } = await dialog.showMessageBox(parentWindow(), {
      type: 'info',
      message: `${app.getName()} ${release.version} est disponible`,
      detail: `Vous avez la version ${current}. `
        + (canReplace
          ? `La mise à jour télécharge la nouvelle version${size}, remplace l'AppImage et relance l'app.`
          : "L'app n'est pas lancée depuis son AppImage : la page de la release va s'ouvrir.")
        + (release.notes ? `\n\n${release.notes}` : ''),
      buttons: [canReplace ? 'Mettre à jour' : 'Ouvrir la page', 'Plus tard', 'Ignorer cette version'],
      defaultId: 0, cancelId: 1,
    });
    if (response === 2) { store.set(KEYS.skipped, release.version); return; }
    if (response !== 0) return;
    if (!canReplace) { shell.openExternal(release.page); return; }
    try {
      await install(release, process.env.APPIMAGE);
      relaunch(process.env.APPIMAGE);
    } catch (e) {
      const r = await dialog.showMessageBox(parentWindow(), {
        type: 'warning',
        message: "La mise à jour n'a pas pu être installée",
        detail: `${e.message}\n\nVous pouvez la télécharger à la main depuis la page de la release.`,
        buttons: ['Ouvrir la page', 'Fermer'],
      });
      if (r.response === 0) shell.openExternal(release.page);
    }
  } finally {
    busy = false;
  }
}

/** Télécharge, vérifie et met la nouvelle AppImage à la place de `target`. */
async function install(release, target) {
  const dir = path.dirname(target);
  try { fs.accessSync(dir, fs.constants.W_OK); fs.accessSync(target, fs.constants.W_OK); }
  catch { throw new Error("Impossible de remplacer l'app à son emplacement actuel (droits insuffisants)."); }

  // Dans le même dossier : le remplacement final est un simple renommage.
  const staging = path.join(dir, `.${path.basename(target)}.update-${process.pid}`);
  const win = parentWindow();
  try {
    const res = await fetch(release.download, { headers: { 'User-Agent': `${app.getName()}-updater` } });
    if (res.status !== 200) throw new Error(`GitHub n'a pas répondu correctement (code ${res.status}).`);
    const total = Number(res.headers.get('content-length')) || release.size || 0;
    const out = fs.createWriteStream(staging, { mode: 0o755 });
    const hash = crypto.createHash('sha256');
    let done = 0;
    for await (const chunk of res.body) {
      hash.update(chunk);
      done += chunk.length;
      if (!out.write(chunk)) await new Promise((r) => out.once('drain', r));
      if (win && total) win.setProgressBar(done / total);
    }
    await new Promise((resolve, reject) => out.end((err) => (err ? reject(err) : resolve())));
    if (win) win.setProgressBar(-1);

    if (release.size && done !== release.size) throw new Error('Le fichier téléchargé est incomplet.');
    if (release.digest && hash.digest('hex') !== release.digest.toLowerCase()) {
      throw new Error("L'empreinte du fichier téléchargé ne correspond pas à celle publiée sur GitHub.");
    }
    const head = Buffer.alloc(4);
    const fd = fs.openSync(staging, 'r');
    fs.readSync(fd, head, 0, 4, 0);
    fs.closeSync(fd);
    if (!head.equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) {
      throw new Error("Le fichier téléchargé n'est pas une AppImage.");
    }
    fs.chmodSync(staging, 0o755);
    fs.renameSync(staging, target);
  } catch (e) {
    if (win) win.setProgressBar(-1);
    try { fs.unlinkSync(staging); } catch { /* déjà parti */ }
    throw e;
  }
}

/** Relance l'app une fois ce processus terminé. */
function relaunch(appImage) {
  const child = spawn('/bin/sh', ['-c', `while kill -0 ${process.pid} 2>/dev/null; do sleep 0.2; done; exec "$0"`, appImage], {
    detached: true, stdio: 'ignore',
  });
  child.unref();
  setTimeout(() => app.quit(), 0);
}

/** À appeler une fois au lancement, avec le dépôt GitHub (« VGioM/Recomps »). */
function start(repoName, settingsStore) {
  repo = repoName;
  store = settingsStore;
  // Laisse la fenêtre s'ouvrir avant de solliciter le réseau.
  setTimeout(() => {
    if (!automatic()) return;
    const last = store.get(KEYS.lastCheck, 0);
    if (last && Date.now() - last < 20 * 3600 * 1000) return;
    check(false);
  }, 4000);
}

const checkNow = () => check(true);

module.exports = {
  start, checkNow, automatic, setAutomatic, isNewer, linuxAsset, readableNotes, clean,
  menuTitle: 'Rechercher les mises à jour…',
  automaticTitle: 'Rechercher automatiquement les mises à jour',
};
