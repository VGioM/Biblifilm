'use strict';
// La séance mpv contre un faux mpv (test/fake-mpv.js) qui parle le même protocole IPC.
const test = require('node:test');
const assert = require('node:assert');
const path = require('path');
const fs = require('fs');
const os = require('os');
const { MpvSession } = require('../src/player');

test('reprise, épisode vu, suite de la saison, piste audio retenue', async () => {
  const log = path.join(os.tmpdir(), `fake-mpv-${process.pid}.log`);
  process.env.FAKE_MPV_LOG = log;
  const updates = [];
  const tracks = [];
  const queue = [{ id: 'e1', path: '/v/e1.mkv', title: 'Un' }, { id: 'e2', path: '/v/e2.mkv', title: 'Deux' }];
  await new Promise((resolve) => {
    const s = new MpvSession({ kind: 'native', cmd: path.join(__dirname, 'fake-mpv.js') }, queue, 12, { trackPrefs: { audio: { title: 'Français VFF', lang: 'fre' } } }, {
      update: (id, pos, dur, routine) => updates.push({ id, pos, dur, routine }),
      tracks: (kind, label) => tracks.push([kind, label]),
      ended: resolve,
    });
    s.launch();
  });
  const cmds = fs.readFileSync(log, 'utf8').trim().split('\n').map(JSON.parse);
  fs.unlinkSync(log);
  assert.ok(cmds[0].args.includes('--start=12'), 'la reprise est passée à mpv');
  // La piste mémorisée est réappliquée à chaque fichier, par son libellé.
  assert.strictEqual(cmds.filter((c) => c.cmd && c.cmd[0] === 'set_property' && c.cmd[1] === 'aid' && c.cmd[2] === 2).length, 2);
  assert.ok(cmds.some((c) => c.cmd && c.cmd[1] === 'force-media-title' && c.cmd[2] === 'Deux'));
  const last1 = updates.filter((u) => u.id === 'e1').pop();
  assert.strictEqual(last1.pos, 60, 'fin du fichier : épisode 1 vu');
  const last2 = updates.filter((u) => u.id === 'e2').pop();
  assert.ok(last2.pos >= 29 && last2.pos <= 30, `épisode 2 quitté au milieu (${last2.pos})`);
  assert.strictEqual(last2.routine, false, 'une dernière écriture en quittant');
  // Le choix de la piste française est retenu ; les pistes vidées entre deux fichiers, non.
  assert.deepStrictEqual(tracks, [['audio', { title: 'Français VFF', lang: 'fre' }]]);
});
