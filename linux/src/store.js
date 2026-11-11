// store.js — réglages et petits fichiers JSON dans ~/.config/<App>/ (même
// fichier dans les trois apps). Écriture atomique : un fichier temporaire
// renommé, pour qu'un arrêt brutal ne laisse jamais un JSON coupé en deux.
'use strict';

const fs = require('fs');
const path = require('path');

function readJSON(file, fallback) {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')); } catch { return fallback; }
}

function writeJSON(file, value, { backup = false } = {}) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  if (backup && fs.existsSync(file)) {
    const bak = file.replace(/\.json$/, '.backup.json');
    try { fs.copyFileSync(file, bak); } catch { /* sans gravité */ }
  }
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(value));
  fs.renameSync(tmp, file);
}

/** Réglages clé → valeur, gardés en mémoire et écrits à chaque changement. */
class Settings {
  constructor(file, defaults = {}) {
    this.file = file;
    this.defaults = defaults;
    this.data = readJSON(file, {});
  }
  get(key, def) {
    if (key in this.data) return this.data[key];
    if (key in this.defaults) return this.defaults[key];
    return def;
  }
  set(key, value) {
    if (value === undefined || value === null) delete this.data[key];
    else this.data[key] = value;
    writeJSON(this.file, this.data);
  }
  all() { return { ...this.defaults, ...this.data }; }
}

module.exports = { readJSON, writeJSON, Settings };
