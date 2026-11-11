'use strict';
const test = require('node:test');
const assert = require('node:assert');
const S = require('../src/shared');
const P = require('../src/parser');

const item = (name, extra = {}) => {
  const p = '/media/' + name;
  return { id: p, path: p, filename: name.split('/').pop(), ext: 'mkv', parsed: P.parse(p, name.includes('/') ? name.split('/')[0] : null), size: 1, modified: 0, metadata: null, ...extra };
};

test('les épisodes forment une tuile par saison, les hors-séries à part', () => {
  const items = [item('Show.S01E02.mkv'), item('Show.S01E01.mkv'), item('Show.S02E01.mkv'), item('Show - OAV - 01 - Bonus Film.mkv'), item('Film.2010.mkv')];
  const entries = S.build(items);
  const seasons = entries.filter((e) => e.type === 'season');
  assert.strictEqual(seasons.length, 3);
  const s1 = seasons.find((s) => s.season === 1);
  assert.deepStrictEqual(s1.episodes.map((e) => e.parsed.episode), [1, 2]);
  assert.ok(seasons.some((s) => s.special === 'oav' && s.seasonLabel === 'Films et hors-séries'));
  assert.strictEqual(entries.filter((e) => e.type === 'single').length, 1);
});

test('reprise : seulement après 5 s et avant 92 %', () => {
  assert.strictEqual(S.isResumable({ position: 4, duration: 100 }), false);
  assert.strictEqual(S.isResumable({ position: 50, duration: 100 }), true);
  assert.strictEqual(S.isResumable({ position: 95, duration: 100 }), false);
});

test("l'épisode à proposer : celui joué en dernier s'il est en cours, sinon le suivant non vu", () => {
  const items = [item('Show.S01E01.mkv'), item('Show.S01E02.mkv'), item('Show.S01E03.mkv'), item('Show.S01E04.mkv')];
  const g = S.build(items)[0];
  const id = (n) => items[n - 1].id;
  // Épisode 1 laissé au générique, puis 2 vu, puis 3 commencé.
  let W = S.Watch({ [id(1)]: { position: 90, duration: 100, watched: false, lastPlayed: 1 },
    [id(2)]: { position: 100, duration: 100, watched: true, lastPlayed: 2 },
    [id(3)]: { position: 30, duration: 100, watched: false, lastPlayed: 3 } });
  assert.strictEqual(W.nextEpisode(g).id, id(3));
  W = S.Watch({ [id(2)]: { position: 100, duration: 100, watched: true, lastPlayed: 2 } });
  assert.strictEqual(W.nextEpisode(g).id, id(3));
  assert.strictEqual(W.watchedCount(g), 1);
  assert.strictEqual(W.seasonInProgress(g), true);
});

test('Reprendre : la saison entière, la plus récente d\'abord', () => {
  const items = [item('A.S01E01.mkv'), item('A.S01E02.mkv'), item('Film.2010.mkv')];
  const W = S.Watch({ [items[0].id]: { position: 100, duration: 100, watched: true, lastPlayed: 5 },
    [items[2].id]: { position: 40, duration: 100, watched: false, lastPlayed: 9 } });
  const list = S.continueEntries(items, W);
  assert.deepStrictEqual(list.map((e) => e.type), ['single', 'season']);
  assert.strictEqual(list[1].episodes.length, 2);
});

test('tri par titre : la série, jamais le titre de l\'épisode', () => {
  const items = [item('Zed.S01E01.mkv', { metadata: { matched: true, seriesTitle: 'Zed', episodeTitle: 'Aaa' } }), item('Beta.2001.mkv')];
  const entries = S.visibleEntries(items, 'all', '', 'title', S.Watch({}));
  assert.deepStrictEqual(entries.map((e) => e.seriesTitle), ['Beta', 'Zed']);
});
