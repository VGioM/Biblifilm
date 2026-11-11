'use strict';
// selfupdate.js charge Electron ; on n'en teste ici que les fonctions pures,
// avec un faux module « electron ».
const test = require('node:test');
const assert = require('node:assert');
const Module = require('module');

const realLoad = Module._load;
Module._load = function (request, ...rest) {
  if (request === 'electron') return { app: {}, dialog: {}, shell: {}, BrowserWindow: {} };
  return realLoad.call(this, request, ...rest);
};
const U = require('../src/selfupdate');
Module._load = realLoad;

test('isNewer compare numériquement', () => {
  assert.strictEqual(U.isNewer('1.10.0', '1.9.9'), true);
  assert.strictEqual(U.isNewer('v1.3.2', '1.3.2'), false);
  assert.strictEqual(U.isNewer('1.3', '1.3.0'), false);
  assert.strictEqual(U.isNewer('2', '1.99'), true);
});

test("seule l'AppImage Linux est prise, jamais le zip macOS", () => {
  const assets = [
    { name: 'App-1.4.0-macOS-arm64.zip' },
    { name: 'App-1.4.0-linux-x86_64.AppImage' },
    { name: 'App-1.4.0-windows-x64.zip' },
  ];
  assert.strictEqual(U.linuxAsset(assets).name, 'App-1.4.0-linux-x86_64.AppImage');
  assert.strictEqual(U.linuxAsset([{ name: 'App-1.4.0-macOS-arm64.zip' }]), null);
});

test('notes lisibles : sans Markdown ni Installation', () => {
  const n = U.readableNotes('## Nouveautés\n- **Un** `deux`\n\n## Installation\n1. truc');
  assert.strictEqual(n, 'Nouveautés\n• Un deux');
});
