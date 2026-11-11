// player.js — la lecture par mpv, pilotée par son socket IPC (l'équivalent
// Linux de PlayerModel.swift, qui embarque VLCKit sur Mac).
//
// mpv lit tout (MKV, x265 10 bits, EAC3, sous-titres ASS…), tient l'écran
// allumé pendant la lecture et gère clavier et plein écran. Biblifilm lui
// passe la file d'attente (la suite de la saison), la position de reprise, le
// titre de chaque épisode, et lit en retour la position toutes les 2 secondes
// pour la reprise et les épisodes vus. Les pistes audio et sous-titres choisies
// sont reconnues à leur libellé et réappliquées à l'épisode suivant, mpv
// renumérotant les pistes à chaque fichier.
//
// mpv installé normalement (paquet) ou en Flatpak (io.mpv.Mpv) : dans ce cas le
// socket vit dans $XDG_RUNTIME_DIR/app/io.mpv.Mpv/, le seul dossier que le bac à
// sable partage avec l'hôte, et le dossier des vidéos lui est ouvert en lecture.
'use strict';

const { spawn, execFileSync } = require('child_process');
const fs = require('fs');
const net = require('net');
const os = require('os');
const path = require('path');

function which(cmd) {
  for (const dir of (process.env.PATH || '').split(':')) {
    const p = path.join(dir, cmd);
    try { fs.accessSync(p, fs.constants.X_OK); return p; } catch { /* suivant */ }
  }
  return null;
}

function hasFlatpak(id) {
  if (!which('flatpak')) return false;
  try { execFileSync('flatpak', ['info', id], { stdio: 'ignore', timeout: 5000 }); return true; } catch { return false; }
}

let cache = null;
/** Les lecteurs présents : { mpv: {...}|null, vlc: {...}|null }. */
function detect(force) {
  if (cache && !force) return cache;
  const mpvBin = which('mpv');
  const vlcBin = which('vlc');
  cache = {
    mpv: mpvBin ? { kind: 'native', cmd: mpvBin, label: 'mpv' }
      : hasFlatpak('io.mpv.Mpv') ? { kind: 'flatpak', id: 'io.mpv.Mpv', label: 'mpv (Flatpak)' } : null,
    vlc: vlcBin ? { kind: 'native', cmd: vlcBin, label: 'VLC' }
      : hasFlatpak('org.videolan.VLC') ? { kind: 'flatpak', id: 'org.videolan.VLC', label: 'VLC (Flatpak)' } : null,
  };
  return cache;
}

function commonDir(paths) {
  if (!paths.length) return '/';
  let parts = path.dirname(paths[0]).split('/');
  for (const p of paths.slice(1)) {
    const q = path.dirname(p).split('/');
    let i = 0;
    while (i < parts.length && i < q.length && parts[i] === q[i]) i++;
    parts = parts.slice(0, i);
  }
  return parts.join('/') || '/';
}

/** Ouvre des fichiers dans VLC (ou l'app par défaut) sans suivi : la porte de sortie. */
function openInVLC(files, vlc) {
  if (!vlc) return false;
  const args = vlc.kind === 'flatpak'
    ? ['run', `--filesystem=${commonDir(files)}:ro`, vlc.id, ...files]
    : files;
  const child = spawn(vlc.kind === 'flatpak' ? 'flatpak' : vlc.cmd, args, { detached: true, stdio: 'ignore' });
  child.on('error', () => {});
  child.unref();
  return true;
}

const trackLabel = (t) => (t ? { title: t.title || '', lang: t.lang || '' } : null);

/**
 * Une séance de lecture : un mpv, une file d'épisodes.
 * queue : [{ id, path, title }] ; start : position du premier, en secondes.
 * hooks : update(id, position, duration, routine), ended(), tracks(kind, label)
 */
class MpvSession {
  constructor(mpv, queue, start, opts, hooks) {
    this.mpv = mpv; this.queue = queue; this.start = start || 0; this.opts = opts; this.hooks = hooks;
    this.index = 0; this.pos = 0; this.dur = 0; this.played = 0; this.lastSaved = -1; this.lastTick = 0;
    this.loaded = false; this.applying = false; this.closed = false; this.buf = ''; this.reqId = 100; this.pending = new Map();
  }

  socketPath() {
    const run = process.env.XDG_RUNTIME_DIR || os.tmpdir();
    const name = `biblifilm-${process.pid}-${Date.now()}.sock`;
    if (this.mpv.kind === 'flatpak') {
      const dir = path.join(run, 'app', this.mpv.id);
      fs.mkdirSync(dir, { recursive: true });
      return path.join(dir, name);
    }
    return path.join(run, name);
  }

  launch() {
    this.sock = this.socketPath();
    const mpvArgs = [
      '--force-window=yes', '--idle=no', '--keep-open=no', `--input-ipc-server=${this.sock}`,
      '--save-position-on-quit=no', '--osd-on-seek=msg-bar', `--title=\${media-title} — Biblifilm`,
      `--alang=${this.opts.alang || 'fr,fre,fra,en,eng'}`,
    ];
    if (this.start > 0) mpvArgs.push(`--start=${Math.floor(this.start)}`);
    if (this.opts.fullscreen) mpvArgs.push('--fullscreen');
    mpvArgs.push('--', ...this.queue.map((q) => q.path));
    const [cmd, args] = this.mpv.kind === 'flatpak'
      ? ['flatpak', ['run', `--filesystem=${commonDir(this.queue.map((q) => q.path))}:ro`, this.mpv.id, ...mpvArgs]]
      : [this.mpv.cmd, mpvArgs];
    this.proc = spawn(cmd, args, { stdio: 'ignore' });
    this.proc.on('error', (e) => this.finish(e));
    this.proc.on('exit', () => this.finish());
    this.connect(0);
  }

  connect(attempt) {
    if (this.closed) return;
    const c = net.createConnection(this.sock);
    c.on('connect', () => { this.conn = c; this.onConnected(); });
    c.on('data', (d) => this.onData(d));
    c.on('error', () => {
      c.destroy();
      if (!this.conn && attempt < 150) setTimeout(() => this.connect(attempt + 1), 100);
    });
  }

  send(command) {
    if (!this.conn || this.closed) return Promise.resolve(null);
    const id = ++this.reqId;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.conn.write(JSON.stringify({ command, request_id: id }) + '\n');
      setTimeout(() => { if (this.pending.has(id)) { this.pending.delete(id); resolve(null); } }, 3000);
    });
  }

  onConnected() {
    const props = ['time-pos', 'duration', 'playlist-pos', 'current-tracks/audio', 'current-tracks/sub'];
    props.forEach((p, i) => this.conn.write(JSON.stringify({ command: ['observe_property', i + 1, p] }) + '\n'));
    this.timer = setInterval(() => this.save(true), 2000);
  }

  onData(d) {
    this.buf += d.toString('utf8');
    let nl;
    while ((nl = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, nl); this.buf = this.buf.slice(nl + 1);
      if (!line.trim()) continue;
      let msg; try { msg = JSON.parse(line); } catch { continue; }
      if (msg.request_id && this.pending.has(msg.request_id)) {
        this.pending.get(msg.request_id)(msg.error === 'success' ? msg.data : null);
        this.pending.delete(msg.request_id);
      } else if (msg.event) this.onEvent(msg);
    }
  }

  onEvent(msg) {
    if (msg.event === 'property-change') {
      switch (msg.name) {
        case 'time-pos':
          if (typeof msg.data === 'number') {
            const step = msg.data - this.pos;
            if (step > 0 && step < 2.5) this.played += step;   // ce qui a vraiment été joué
            this.pos = msg.data;
          }
          break;
        case 'duration': if (typeof msg.data === 'number') this.dur = msg.data; break;
        case 'playlist-pos':
          if (typeof msg.data === 'number' && msg.data >= 0 && msg.data !== this.index) {
            this.save(false);
            this.index = msg.data; this.pos = 0; this.dur = 0; this.played = 0; this.lastSaved = -1; this.loaded = false;
          }
          break;
        case 'current-tracks/audio':
        case 'current-tracks/sub':
          // Un choix fait par l'utilisateur, pas par nous en chargeant le fichier.
          // Sans piste audio, c'est un changement de fichier, jamais un choix : mpv vide
          // les pistes entre deux épisodes.
          if (this.loaded && !this.applying && (msg.data || msg.name.endsWith('sub'))) {
            this.hooks.tracks(msg.name.endsWith('audio') ? 'audio' : 'sub', msg.data ? trackLabel(msg.data) : 'none');
          }
          break;
        default:
      }
    } else if (msg.event === 'file-loaded') {
      this.onFileLoaded();
    } else if (msg.event === 'end-file') {
      // Ce qui suit (pistes vidées, fichier suivant) n'est plus un choix de l'utilisateur.
      this.loaded = false;
      if (msg.reason !== 'eof') return;
      // Fin réelle du fichier : l'épisode est vu.
      if (this.dur > 0 && this.played > 3) { this.pos = this.dur; this.save(false); }
    }
  }

  async onFileLoaded() {
    const item = this.queue[this.index];
    this.applying = true;
    // --start ne vaut que pour le premier fichier de la file.
    if (this.index > 0 || this.startCleared) { /* déjà fait */ } else {
      setTimeout(() => this.send(['set_property', 'options/start', 'none']), 500);
      this.startCleared = true;
    }
    if (item) this.send(['set_property', 'force-media-title', item.title]);
    const prefs = this.opts.trackPrefs || {};
    const tracks = (await this.send(['get_property', 'track-list'])) || [];
    for (const kind of ['audio', 'sub']) {
      const want = prefs[kind];
      if (!want) continue;
      if (want === 'none') { if (kind === 'sub') await this.send(['set_property', 'sid', 'no']); continue; }
      const list = tracks.filter((t) => t.type === kind);
      const hit = list.find((t) => (t.title || '') === want.title && (t.lang || '') === want.lang)
        || (want.title && list.find((t) => (t.title || '') === want.title))
        || (want.lang && list.find((t) => (t.lang || '') === want.lang));
      if (hit) await this.send(['set_property', kind === 'audio' ? 'aid' : 'sid', hit.id]);
    }
    setTimeout(() => { this.applying = false; this.loaded = true; }, 800);
  }

  /** Écrit la position, seulement si elle a bougé, et seulement après 3 s vraiment jouées. */
  save(routine) {
    const item = this.queue[this.index];
    if (!item || this.played <= 3 || this.dur <= 0) return;
    const p = Math.round(this.pos * 10) / 10;
    if (routine && p === this.lastSaved) return;
    this.lastSaved = p;
    this.hooks.update(item.id, this.pos, this.dur, routine);
  }

  finish(err) {
    if (this.closed) return;
    this.save(false);
    this.closed = true;
    clearInterval(this.timer);
    if (this.conn) this.conn.destroy();
    try { fs.unlinkSync(this.sock); } catch { /* déjà parti */ }
    this.hooks.ended(err);
  }

  stop() {
    if (this.closed) return;
    this.send(['quit']);
    setTimeout(() => { try { this.proc.kill(); } catch { /* fini */ } }, 1500);
  }
}

/**
 * Installe mpv depuis Flathub, pour l'utilisateur : pas de mot de passe à
 * demander. Rend null si c'est fait, sinon la raison de l'échec.
 */
function installMpv(onProgress) {
  return new Promise((resolve) => {
    if (!which('flatpak')) { resolve('Flatpak est introuvable : installez mpv avec le gestionnaire de paquets de votre distribution.'); return; }
    const run = (args, next) => {
      const p = spawn('flatpak', args, { env: { ...process.env, LC_ALL: 'C' } });
      let out = '';
      const take = (d) => {
        out += d;
        const m = /(\d+)%/.exec(d.toString());
        if (m && onProgress) onProgress(Number(m[1]));
      };
      p.stdout.on('data', take); p.stderr.on('data', take);
      p.on('error', (e) => next(e.message));
      p.on('close', (code) => next(code === 0 ? null : (out.trim().split('\n').pop() || `code ${code}`)));
    };
    // Flathub pour l'utilisateur, ajouté s'il manque (c'est le dépôt officiel de Flatpak).
    run(['remote-add', '--user', '--if-not-exists', 'flathub', 'https://dl.flathub.org/repo/flathub.flatpakrepo'], () => {
      run(['install', '-y', '--noninteractive', '--user', 'flathub', 'io.mpv.Mpv'], (err) => {
        detect(true);
        resolve(err && !detect().mpv ? `L'installation de mpv a échoué : ${err}` : null);
      });
    });
  });
}

module.exports = { detect, MpvSession, openInVLC, commonDir, installMpv };
