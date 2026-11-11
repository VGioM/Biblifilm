// parser.js — transforme un nom de fichier de release en titre propre, avec
// saison et épisode. Traduction directe de FilenameParser.swift.
//
//   [DragonMax] Nicky Larson - 001.mkv        -> épisode, numéro absolu 1
//   Breaking.Bad.S01E02.1080p.WEB-DL.mkv      -> épisode S1E2
//   The Wire 3x04 - Amsterdam.avi             -> épisode S3E4
//   Inception.2010.1080p.BluRay.x264.mkv      -> film, 2010
'use strict';

const path = require('path');

const videoExtensions = new Set([
  'mkv', 'mp4', 'm4v', 'avi', 'mov', 'wmv', 'flv', 'webm', 'mpg', 'mpeg',
  'mpe', 'm2v', 'm2ts', 'mts', 'ts', 'vob', 'ogv', 'ogm', '3gp', '3g2',
  'divx', 'xvid', 'rmvb', 'rm', 'asf', 'f4v', 'mxf', 'nsv', 'iso', 'dv',
  'amv', 'mp2', 'mpv', 'qt', 'yuv', 'm4p',
]);

const audioExtensions = new Set([
  'mp3', 'flac', 'm4a', 'aac', 'wav', 'aiff', 'aif', 'ogg', 'oga', 'opus',
  'wma', 'alac', 'ape', 'wv', 'mka', 'dsf', 'dff', 'mid', 'midi', 'it',
  'mod', 's3m', 'xm', 'spx', 'tta', 'voc', 'au',
]);

/** Ce qui décrit la release, jamais le titre. Retiré du nom. */
const junkTokens = new Set([
  '480p', '576p', '720p', '1080p', '1080i', '2160p', '4k', 'uhd', 'hd', 'sd',
  'bluray', 'blu-ray', 'brrip', 'bdrip', 'bdremux', 'remux', 'webrip', 'web-dl',
  'webdl', 'web', 'hdtv', 'pdtv', 'dvdrip', 'dvdscr', 'dvd', 'hdrip', 'camrip',
  'cam', 'ts', 'tc', 'r5', 'vhsrip', 'satrip', 'dsr', 'hdcam',
  'x264', 'x265', 'h264', 'h265', 'h', 'avc', 'hevc', 'xvid', 'divx', '10bit',
  '8bit', '10bits', 'hi10p', 'hdr', 'hdr10', 'dovi', 'dv', 'sdr',
  'aac', 'aac2', 'ac3', 'eac3', 'dts', 'dtshd', 'truehd', 'atmos', 'flac',
  'mp3', 'opus', 'ddp', 'ddp5', 'dd5', 'dd', '2ch', '6ch',
  'multi', 'multi3', 'vostfr', 'vost', 'vf', 'vff', 'vfq', 'vfi', 'vo',
  'french', 'truefrench', 'english', 'eng', 'fre', 'fr', 'jap', 'jpn',
  'subfrench', 'subbed', 'dubbed', 'dual', 'audio',
  'repack', 'proper', 'rerip', 'internal', 'limited', 'unrated', 'extended',
  'remastered', 'directors', 'cut', 'theatrical', 'complete', 'integrale',
  'final', 'uncensored', 'bd', 'raw', 'batch', 'nf', 'amzn', 'dsnp', 'atvp',
  'hmax', 'hulu', 'itunes', 'criterion', 'version', 'longue', 'vlong',
  'subforced', 'subs', 'sub', 'hdlight', 'light', 'uncut', '3d', 'sbs',
  'readnfo', 'hardcoded',
]);

const episodePatterns = [
  /[sS](\d{1,2})[\s._-]*[eE](\d{1,3})/,                              // S01E02, s1 e2
  /(?<![0-9a-zA-Z])(\d{1,2})[xX](\d{1,3})(?![0-9])/,                 // 3x04
  /[sS]aison[\s._-]*(\d{1,2})[\s._-]*[eEéÉ](?:p(?:isode)?)?[\s._-]*(\d{1,3})/, // Saison 2 Episode 5
  /[sS]eason[\s._-]*(\d{1,2})[\s._-]*[eE](?:p(?:isode)?)?[\s._-]*(\d{1,3})/,
];

// Un hors-série n'est reconnu qu'après un tiret séparateur : sinon « The Movie
// Critic » ou « Film Noir Collection » passeraient pour des hors-séries.
const separator = '[\\s._]+[-–][\\s._]*';
const strongMarkers = [
  ['t[ée]l[ée]films?', 'film'],
  ['oavs?|ovas?', 'oav'],
  ['ncop|nced|openings?|endings?|g[ée]n[ée]riques?', 'extra'],
  ['bonus|making[\\s._-]?of', 'extra'],
];
const numberedMarkers = [
  ['films?', 'film'],
  ['movies?', 'film'],
  ['sp[ée]ciaux|sp[ée]ciales?|specials?', 'special'],
];
const specialMarkers = [
  ...strongMarkers.map(([w, k]) => [new RegExp(`${separator}(?:${w})(?![a-z])`, 'i'), k]),
  ...numberedMarkers.map(([w, k]) => [new RegExp(`${separator}(?:${w})(?![a-z])(?=[\\s._-]*\\d)`, 'i'), k]),
];

/** Numéro d'épisode seul, sans saison : « - 001 », « Ep 12 », « E12 », « #5 ». */
const absolutePatterns = [
  /[\s._-]-[\s._]*(\d{1,4})(?:v\d)?(?=[\s._[(]|$)/,
  /(?<![a-zA-Z0-9])[eEéÉ](?:[pP](?:isode|is)?)?[\s._-]*(\d{1,4})(?![0-9])/,
  /(?<![a-zA-Z0-9])#[\s._]*(\d{1,4})(?![0-9])/,
];

function match(s, re) {
  const m = re.exec(s);
  if (!m) return null;
  return { start: m.index, end: m.index + m[0].length, groups: m.slice(1) };
}

const replaceAll = (s, re, repl) => s.replace(new RegExp(re.source, re.flags.includes('g') ? re.flags : re.flags + 'g'), repl);

/** Vrai quand un titre candidat n'est que du vocabulaire de saison/épisode. */
function isNoiseOnly(title) {
  const stripped = replaceAll(title.toLowerCase(),
    /\b(saison|season|episode|episodes|ep|epis|serie|series|s|e|vol|volume|part|partie|disc|disque|livre|book|cd|dvd)\b|\d+/, ' ');
  return stripped.trim() === '';
}

const trimChars = (s, chars) => {
  let a = 0, b = s.length;
  while (a < b && chars.includes(s[a])) a++;
  while (b > a && chars.includes(s[b - 1])) b--;
  return s.slice(a, b);
};

/** Séparateurs normalisés, déchets de release retirés. */
function cleanTitle(input) {
  let s = input;
  // Dispositions de canaux (5.1, 7.1) : du bruit, mais pas les chiffres seuls.
  s = replaceAll(s, /(?<![0-9])[0-9]\.[0-9](?![0-9])/, ' ');
  // Un aparté entre parenthèses est un titre traduit, pas le nom cherché.
  s = replaceAll(s, /\([^)]*\)/, ' ');
  s = s.replace(/_/g, ' ');
  s = replaceAll(s, /(?<=[a-zA-Z0-9])\.(?=[a-zA-Z0-9])/, ' ');
  s = s.replace(/\./g, ' ');
  s = replaceAll(s, /[()[\]{}]/, ' ');
  // Un « -GROUPE » final seulement s'il est le dernier mot.
  s = replaceAll(s, /\s-\s*[A-Za-z0-9]{2,15}\s*$/, ' ');
  const kept = [];
  for (const t of s.split(/[ -]/)) {
    const low = t.toLowerCase().replace(/^[\p{P}]+|[\p{P}]+$/gu, '');
    if (!low) continue;
    if (junkTokens.has(low)) continue;
    kept.push(t);
  }
  let out = kept.join(' ');
  out = replaceAll(out, /\s{2,}/, ' ');
  return trimChars(out, ' -–—_.,:;');
}

/**
 * @param file chemin du fichier
 * @param folderHint nom du dossier parent, titre de secours quand le nom du
 *   fichier n'en porte aucun (`Kaamelott/Saison 2 Episode 5.mkv`).
 */
function parse(file, folderHint) {
  const ext = path.extname(file).slice(1).toLowerCase();
  const raw = path.basename(file, path.extname(file)).normalize('NFC');
  let work = raw;
  let releaseGroup = null;

  // 1. Groupes de release entre crochets : [DragonMax]
  const g = /^\[([^\]]{1,30})\]/.exec(work);
  if (g) releaseGroup = g[1];
  work = replaceAll(work, /\[[^\]]*\]/, ' ');
  work = replaceAll(work, /\{[^}]*\}/, ' ');

  // 2. Année. Entre parenthèses, elle fait foi (« Blade Runner 2049 (2017) »).
  let year = null, yearRange = null;
  let m = match(work, /\((19\d{2}|20\d{2})\)/) || match(work, /(?<![0-9])(19\d{2}|20\d{2})(?![0-9])/);
  if (m) { year = parseInt(m.groups[0], 10); yearRange = m; }

  // 3. Films, OAV et bonus : séparés de la numérotation des épisodes.
  let special = null, specialTitle = null, specialNumber = null, specialRange = null;
  for (const [re, kind] of specialMarkers) {
    const sm = match(work, re);
    if (!sm) continue;
    special = kind;
    specialRange = sm;
    const tail = work.slice(sm.end);
    const num = match(tail, /^[\s._-]*(\d{1,3})(?![0-9])/);
    if (num) {
      specialNumber = parseInt(num.groups[0], 10);
      specialTitle = cleanTitle(tail.slice(num.end));
    } else if (kind === 'extra') {
      specialTitle = cleanTitle(work.slice(sm.start));
    } else {
      specialTitle = cleanTitle(tail);
    }
    if (specialTitle === '') specialTitle = null;
    break;
  }

  // 4. Saison/épisode.
  let season = null, episode = null, seRange = null;
  if (!special) {
    for (const re of episodePatterns) {
      const em = match(work, re);
      if (em) { season = parseInt(em.groups[0], 10); episode = parseInt(em.groups[1], 10); seRange = em; break; }
    }
  }

  // 5. Numérotation absolue, seulement sans S/E.
  let absolute = null, absRange = null;
  if (season == null && !special) {
    for (const re of absolutePatterns) {
      const am = match(work, re);
      if (am) {
        const n = parseInt(am.groups[0], 10);
        if (!(n >= 1900 && n <= 2099)) { absolute = n; absRange = am; break; }
      }
    }
  }

  // 6. Le titre est ce qui précède le marqueur d'épisode ou l'année.
  let titlePart = work;
  if (specialRange) titlePart = work.slice(0, specialRange.start);
  else if (seRange || absRange) titlePart = work.slice(0, (seRange || absRange).start);
  else if (yearRange) titlePart = work.slice(0, yearRange.start);

  // « Monster.2004.S01E61 » : une année finale appartient à la série.
  if (season != null || absolute != null) {
    titlePart = titlePart.replace(/[\s._-]*(?<![0-9])(19\d{2}|20\d{2})(?![0-9])[\s._-]*$/, ' ');
  }

  let title = cleanTitle(titlePart);
  if ((title === '' || isNoiseOnly(title)) && folderHint) {
    const hinted = cleanTitle(folderHint.normalize('NFC'));
    if (hinted && !isNoiseOnly(hinted)) title = hinted;
  }
  if (!title) title = cleanTitle(work);
  if (!title) title = raw;

  let kind;
  if (audioExtensions.has(ext)) kind = 'audio';
  else if (special || season != null || absolute != null) kind = 'episode';
  else if (videoExtensions.has(ext)) kind = 'movie';
  else kind = 'other';

  return { title, kind, year, season, episode, absoluteEpisode: absolute, releaseGroup, special, specialTitle, specialNumber };
}

/**
 * Nettoie un titre tapé à la main dans la feuille de correction : seule une
 * année finale est retirée (et rendue à part pour affiner la recherche).
 */
function normalizeUserTitle(input) {
  const composed = String(input).normalize('NFC').trim();
  const trailingYear = /[\s([]*(?<![0-9])(19\d{2}|20\d{2})(?![0-9])[\s)\]]*$/;
  const ym = trailingYear.exec(composed);
  const year = ym ? parseInt(ym[1], 10) : null;
  let s = composed.replace(trailingYear, ' ');
  s = replaceAll(s, /\s{2,}/, ' ');
  s = trimChars(s, ' -–—_.,:;()[]');
  return { title: s || composed, year };
}

function showKey(p) {
  let k = p.title.toLowerCase();
  if (p.year != null) k += `|${p.year}`;
  return `${k}|${p.kind}`;
}

module.exports = { parse, normalizeUserTitle, showKey, videoExtensions, audioExtensions };
