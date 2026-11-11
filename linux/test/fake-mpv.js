#!/usr/bin/env node
// Un faux mpv pour les tests : ouvre le socket IPC donné, répond aux commandes
// et joue la file à grande vitesse (1 s de film = 20 ms), comme le ferait mpv.
'use strict';
const net = require('net');
const args = process.argv.slice(2);
const sock = args.find((a) => a.startsWith('--input-ipc-server=')).split('=')[1];
const start = Number((args.find((a) => a.startsWith('--start=')) || '=0').split('=')[1]);
const files = args.slice(args.indexOf('--') + 1);
const DUR = 60;
const log = (o) => process.env.FAKE_MPV_LOG && require('fs').appendFileSync(process.env.FAKE_MPV_LOG, JSON.stringify(o) + '\n');
log({ args });
const server = net.createServer((c) => {
  const send = (o) => c.write(JSON.stringify(o) + '\n');
  let buf = '';
  c.on('data', (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const m = JSON.parse(buf.slice(0, i)); buf = buf.slice(i + 1);
      log({ cmd: m.command });
      if (m.command[0] === 'get_property' && m.command[1] === 'track-list') {
        send({ request_id: m.request_id, error: 'success', data: [{ id: 1, type: 'audio', title: 'Anglais', lang: 'eng' }, { id: 2, type: 'audio', title: 'Français VFF', lang: 'fre' }] });
      } else if (m.request_id) send({ request_id: m.request_id, error: 'success', data: null });
    }
  });
  (async () => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    await sleep(200);
    for (let f = 0; f < files.length; f++) {
      send({ event: 'property-change', name: 'playlist-pos', data: f });
      send({ event: 'file-loaded' });
      send({ event: 'property-change', name: 'duration', data: DUR });
      // L'utilisateur choisit la piste française sur le premier fichier.
      if (f === 0) { await sleep(1000); send({ event: 'property-change', name: 'current-tracks/audio', data: { id: 2, title: 'Français VFF', lang: 'fre' } }); }
      const from = f === 0 ? start : 0;
      // Le second fichier est quitté au milieu.
      const until = f === files.length - 1 ? DUR / 2 : DUR;
      for (let t = from; t <= until; t += 0.5) { send({ event: 'property-change', name: 'time-pos', data: t }); await sleep(10); }
      if (until === DUR) send({ event: 'end-file', reason: 'eof' });
      // Comme mpv : les pistes se vident entre deux fichiers.
      send({ event: 'property-change', name: 'current-tracks/audio', data: null });
    }
    await sleep(2300);
    process.exit(0);
  })();
});
server.listen(sock);
