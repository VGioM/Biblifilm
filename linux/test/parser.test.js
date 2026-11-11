'use strict';
// Les mêmes cas que Tests/BiblifilmTests/FilenameParserTests.swift.
const test = require('node:test');
const assert = require('node:assert');
const P = require('../src/parser');

const parse = (name, folder) => P.parse('/media/' + name, folder);

test('épisodes SxxEyy, 3x04, Saison/Episode', () => {
  for (const [file, title, s, e] of [
    ['Breaking.Bad.S01E02.1080p.WEB-DL.mkv', 'Breaking Bad', 1, 2],
    ['The Wire 3x04 - Amsterdam.avi', 'The Wire', 3, 4],
    ['Sakamoto Days S01E20 MULTi 1080p WEB AV1 E-AC-3 -Tsundere-Raws.mkv', 'Sakamoto Days', 1, 20],
    ['Kaamelott Saison 2 Episode 5.mkv', 'Kaamelott', 2, 5],
    ['Show.Name.s02.e10.mkv', 'Show Name', 2, 10],
  ]) {
    const p = parse(file);
    assert.deepStrictEqual([p.kind, p.title, p.season, p.episode], ['episode', title, s, e], file);
  }
});

test('films avec année', () => {
  for (const [file, title, year] of [
    ['Inception.2010.1080p.BluRay.x264.mkv', 'Inception', 2010],
    ["Le Fabuleux Destin d'Amélie Poulain (2001).mp4", "Le Fabuleux Destin d'Amélie Poulain", 2001],
    ['Blade Runner 2049 (2017).mkv', 'Blade Runner 2049', 2017],
    ['Marionnette.2020.MULTI.1080p.WEB.H264-EXTREME.mkv', 'Marionnette', 2020],
    ['Buddy.2026.VOSTFR.1080p.WEBRip.10.bits.EAC3.5.1.x265-GL0P.mkv', 'Buddy', 2026],
    ['Il est de retour.2015.MULTi.1080p.BluRay.HEVC.DTS.HDMA.5.1-AKLHD.mkv', 'Il est de retour', 2015],
  ]) {
    const p = parse(file);
    assert.deepStrictEqual([p.kind, p.title, p.year], ['movie', title, year], file);
  }
});

test('numérotation absolue et groupe de release', () => {
  const p = parse('[DragonMax] Nicky Larson - 001.mkv');
  assert.strictEqual(p.kind, 'episode');
  assert.strictEqual(p.title, 'Nicky Larson');
  assert.strictEqual(p.absoluteEpisode, 1);
  assert.strictEqual(p.season, null);
  assert.strictEqual(p.releaseGroup, 'DragonMax');
});

test("l'année avant l'épisode appartient à la série", () => {
  const p = parse('Monster.2004.S01E61.mkv');
  assert.deepStrictEqual([p.title, p.year, p.season, p.episode], ['Monster', 2004, 1, 61]);
  assert.strictEqual(parse('Some Show - 2004.mkv').absoluteEpisode, null);
});

test('hors-séries', () => {
  const oav = parse('Nicky Larson - OAV - 01 - Bay City Wars.mkv');
  assert.deepStrictEqual([oav.special, oav.specialNumber, oav.specialTitle, oav.absoluteEpisode, oav.title],
    ['oav', 1, 'Bay City Wars', null, 'Nicky Larson']);
  assert.strictEqual(parse('The Movie Critic.mkv').special, null);
  assert.strictEqual(parse('Film Noir Collection.mkv').special, null);
  assert.strictEqual(parse('Nicky Larson - Téléfilm - Goodbye.mkv'.normalize('NFD')).special, 'film');
});

test('titre pris dans le dossier', () => {
  const p = parse('Saison 2 Episode 5.mkv', 'Kaamelott');
  assert.deepStrictEqual([p.title, p.season, p.episode], ['Kaamelott', 2, 5]);
});

test('types', () => {
  assert.strictEqual(parse('Song.flac').kind, 'audio');
  assert.strictEqual(parse('Notes.txt').kind, 'other');
  assert.strictEqual(parse('Some Film.mkv').kind, 'movie');
  assert.strictEqual(P.showKey(parse('Breaking.Bad.S01E02.mkv')), P.showKey(parse('Breaking Bad S03E10 720p.mkv')));
});

test('titre saisi à la main', () => {
  assert.deepStrictEqual(P.normalizeUserTitle('Pusher II (2004)'), { title: 'Pusher II', year: 2004 });
  assert.deepStrictEqual(P.normalizeUserTitle('The Cut'), { title: 'The Cut', year: null });
  assert.deepStrictEqual(P.normalizeUserTitle('  Alien 1979 '), { title: 'Alien', year: 1979 });
});
