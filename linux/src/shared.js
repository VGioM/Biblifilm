// shared.js — la logique de Biblifilm qui ne touche ni au disque ni au réseau :
// titres affichés, tuiles (un film, ou une saison), reprise de lecture et
// épisodes vus, sections de la barre latérale, tri. Chargé par main.js et par
// l'interface. Traduction de MediaItem.swift, LibraryEntry.swift,
// WatchStore.swift et des parties pures de LibraryModel.swift.
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.Shared = factory();
}(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const pad2 = (n) => String(n).padStart(2, '0');
  const pad3 = (n) => String(n).padStart(3, '0');
  const collator = new Intl.Collator('fr', { numeric: true, sensitivity: 'base' });
  const cmp = (a, b) => collator.compare(a, b);
  const contains = (hay, q) => String(hay).toLocaleLowerCase('fr').includes(q.toLocaleLowerCase('fr'));

  const KIND = {
    movie: { name: 'Film', symbol: 'film' },
    episode: { name: 'Série', symbol: 'tv' },
    audio: { name: 'Audio', symbol: 'music' },
    other: { name: 'Autre', symbol: 'question' },
  };
  const SPECIAL = {
    film: { bucket: 'Films et hors-séries', row: 'Film' },
    oav: { bucket: 'Films et hors-séries', row: 'OAV' },
    special: { bucket: 'Films et hors-séries', row: 'Spécial' },
    extra: { bucket: 'Bonus', row: 'Bonus' },
  };

  // ── Une fiche ────────────────────────────────────────────────────────────

  const matched = (it) => !!(it.metadata && it.metadata.matched);
  const code = (m) => (m && m.season != null && m.episode != null ? `S${pad2(m.season)}E${pad2(m.episode)}` : null);

  function displayTitle(it) {
    const m = it.metadata;
    if (matched(it)) {
      if (it.parsed.kind === 'episode' && m.episodeTitle) return m.episodeTitle;
      if (m.seriesTitle) return m.seriesTitle;
    }
    return it.parsed.title;
  }

  function displaySubtitle(it) {
    const parts = [];
    const m = it.metadata, p = it.parsed;
    if (p.kind === 'episode') {
      if (matched(it) && m.seriesTitle) parts.push(m.seriesTitle);
      if (code(m)) parts.push(code(m));
      else if (p.absoluteEpisode != null) parts.push(`Ép. ${pad3(p.absoluteEpisode)}`);
      else if (p.season != null && p.episode != null) parts.push(`S${pad2(p.season)}E${pad2(p.episode)}`);
    } else {
      const y = (m && m.year) || p.year;
      if (y) parts.push(String(y));
      if (m && m.genres && m.genres[0]) parts.push(m.genres[0]);
    }
    if (!parts.length) parts.push(it.ext.toUpperCase());
    return parts.join(' · ');
  }

  const seasonNumber = (it) => (it.metadata && it.metadata.season != null ? it.metadata.season : it.parsed.season);
  function episodeNumber(it) {
    if (it.metadata && it.metadata.episode != null) return it.metadata.episode;
    if (it.parsed.episode != null) return it.parsed.episode;
    return it.parsed.absoluteEpisode;
  }

  function episodeRowTitle(it) {
    if (it.parsed.specialTitle) return it.parsed.specialTitle;
    if (matched(it) && it.metadata.episodeTitle) return it.metadata.episodeTitle;
    return it.filename.replace(/\.[^.]+$/, '');
  }

  function playerTitle(it) {
    if (it.parsed.specialTitle) return it.parsed.specialTitle;
    const m = it.metadata;
    if (it.parsed.kind === 'episode' && matched(it) && m.episodeTitle) return m.episodeTitle;
    if (matched(it) && m.seriesTitle) return m.seriesTitle;
    return it.parsed.title;
  }

  function playerSubtitle(it) {
    const parts = [];
    const m = it.metadata;
    if (it.parsed.kind === 'episode') {
      if (matched(it) && m.seriesTitle) parts.push(m.seriesTitle);
      if (code(m)) parts.push(code(m));
      else if (it.parsed.absoluteEpisode != null) parts.push(`Ép. ${pad3(it.parsed.absoluteEpisode)}`);
      if (it.parsed.special) parts.push(SPECIAL[it.parsed.special].row);
    } else {
      const y = (m && m.year) || it.parsed.year;
      if (y) parts.push(String(y));
      if (m && m.genres && m.genres[0]) parts.push(m.genres[0]);
    }
    return parts.join(' · ');
  }

  function groupingTitle(it) {
    if (it.parsed.kind === 'episode') {
      if (matched(it) && it.metadata.seriesTitle) return it.metadata.seriesTitle;
      return it.parsed.title;
    }
    return displayTitle(it);
  }

  // ── Tuiles : un film, ou une saison ──────────────────────────────────────

  function sortEpisodes(items) {
    const n = (it) => {
      const v = it.parsed.special ? it.parsed.specialNumber : episodeNumber(it);
      return v == null ? Number.MAX_SAFE_INTEGER : v;
    };
    return items.slice().sort((a, b) => (n(a) - n(b)) || (a.filename < b.filename ? -1 : a.filename > b.filename ? 1 : 0));
  }

  function group(seriesTitle, season, special, episodes) {
    const firstMeta = (f) => { for (const e of episodes) { const v = e.metadata && f(e.metadata); if (v) return v; } return null; };
    const n = episodes.length;
    const seasonLabel = special ? SPECIAL[special].bucket : season == null ? 'Épisodes' : `Saison ${season}`;
    return {
      type: 'season',
      id: special ? `${seriesTitle}|x${SPECIAL[special].bucket}` : `${seriesTitle}|s${season == null ? '?' : season}`,
      seriesTitle, season, special, episodes,
      posterURL: firstMeta((m) => m.posterURL),
      year: firstMeta((m) => m.year),
      genres: firstMeta((m) => (m.genres && m.genres.length ? m.genres : null)) || [],
      overview: firstMeta((m) => m.overview),
      isIdentified: episodes.some(matched),
      totalSize: episodes.reduce((s, e) => s + e.size, 0),
      latestModified: Math.max(...episodes.map((e) => e.modified)),
      seasonLabel,
      subtitle: special ? `${seasonLabel} · ${n} titre${n > 1 ? 's' : ''}` : `${seasonLabel} · ${n} épisode${n > 1 ? 's' : ''}`,
    };
  }

  /** Regroupe les épisodes par série et saison ; le reste reste seul. */
  function build(items) {
    const singles = [];
    const buckets = new Map();
    for (const it of items) {
      if (it.parsed.kind !== 'episode') { singles.push(single(it)); continue; }
      const gt = groupingTitle(it);
      const key = it.parsed.special ? `${gt}|x${SPECIAL[it.parsed.special].bucket}` : `${gt}|s${seasonNumber(it) == null ? '?' : seasonNumber(it)}`;
      if (!buckets.has(key)) buckets.set(key, { gt, season: it.parsed.special ? null : seasonNumber(it), special: it.parsed.special, eps: [] });
      buckets.get(key).eps.push(it);
    }
    const out = singles;
    for (const b of buckets.values()) out.push(group(b.gt, b.season, b.special, sortEpisodes(b.eps)));
    return out;
  }

  function single(it) {
    return {
      type: 'single', id: 'f:' + it.id, item: it, seriesTitle: groupingTitle(it), posterURL: it.metadata && it.metadata.posterURL,
      totalSize: it.size, latestModified: it.modified, isIdentified: matched(it),
    };
  }

  const entryId = (e) => (e.type === 'season' ? 's:' + e.id : e.id);
  const entryKind = (e) => (e.type === 'season' ? 'episode' : e.item.parsed.kind);
  const entrySeasonNumber = (e) => {
    if (e.type === 'single') return 0;
    if (!e.special) return e.season == null ? 9000 : e.season;
    return e.special === 'extra' ? 10001 : 10000;
  };
  const entrySubtitle = (e) => (e.type === 'season' ? e.subtitle : displaySubtitle(e.item));

  // ── Reprise et épisodes vus ──────────────────────────────────────────────

  const COMPLETION = 0.92;            // au-delà, l'épisode est vu : le reste, c'est le générique
  const MIN_RESUME = 5;               // en dessous, « reprendre » voudrait dire « recommencer »

  const progressOf = (r) => (r && r.duration > 0 ? Math.min(Math.max(r.position / r.duration, 0), 1) : 0);
  const isResumable = (r) => !!r && r.position > MIN_RESUME && progressOf(r) < COMPLETION;

  function formatTime(sec) {
    const t = Math.floor(sec || 0);
    const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
    return h > 0 ? `${h}:${pad2(m)}:${pad2(s)}` : `${m}:${pad2(s)}`;
  }

  function Watch(records) {
    const rec = (it) => records[it.id];
    const watched = (it) => !!(rec(it) && rec(it).watched);
    const progress = (it) => progressOf(rec(it));
    const resume = (it) => (isResumable(rec(it)) ? rec(it).position : null);
    const watchedCount = (g) => g.episodes.reduce((n, e) => n + (watched(e) ? 1 : 0), 0);
    const seasonProgress = (g) => (g.episodes.length
      ? g.episodes.reduce((s, e) => s + (watched(e) ? 1 : progress(e)), 0) / g.episodes.length : 0);

    /** L'épisode à proposer : celui joué en dernier s'il est en cours, sinon le suivant non vu. */
    function nextEpisode(g) {
      const eps = g.episodes;
      let last = -1, lastDate = -Infinity;
      eps.forEach((e, i) => { const r = rec(e); if (r && r.lastPlayed > lastDate) { last = i; lastDate = r.lastPlayed; } });
      if (last >= 0) {
        if (resume(eps[last]) != null) return eps[last];
        const next = eps.slice(last).find((e) => !watched(e));
        if (next) return next;
      }
      return eps.find((e) => !watched(e)) || null;
    }
    const lastPlayedIn = (g) => {
      const d = g.episodes.map((e) => rec(e) && rec(e).lastPlayed).filter(Boolean);
      return d.length ? Math.max(...d) : null;
    };
    const seasonInProgress = (g) => lastPlayedIn(g) != null && nextEpisode(g) != null;

    function entryProgress(e) { return e.type === 'single' ? (watched(e.item) ? 1 : progress(e.item)) : seasonProgress(e); }
    function entryWatched(e) { return e.type === 'single' ? watched(e.item) : (e.episodes.length > 0 && watchedCount(e) === e.episodes.length); }

    return { rec, watched, progress, resume, watchedCount, seasonProgress, nextEpisode, lastPlayedIn, seasonInProgress, entryProgress, entryWatched };
  }

  // ── Sections, recherche, tri ─────────────────────────────────────────────

  const SECTIONS = [
    { id: 'all', title: 'Tout', icon: 'grid' },
    { id: 'continueWatching', title: 'Reprendre', icon: 'playCircle' },
    { id: 'movies', title: 'Films', icon: 'film' },
    { id: 'shows', title: 'Séries', icon: 'tv' },
    { id: 'audio', title: 'Audio', icon: 'music' },
    { id: 'recent', title: 'Ajouts récents', icon: 'clock' },
    { id: 'unmatched', title: 'Non identifiés', icon: 'question' },
  ];
  const SORTS = [
    { id: 'title', label: 'Titre' },
    { id: 'dateAdded', label: "Date d'ajout" },
    { id: 'size', label: 'Taille' },
    { id: 'kind', label: 'Type' },
  ];

  function sectionTitle(sel) {
    if (sel.startsWith('series:')) return sel.slice(7);
    return (SECTIONS.find((s) => s.id === sel) || SECTIONS[0]).title;
  }

  function seriesNames(items) {
    return [...new Set(items.filter((i) => i.parsed.kind === 'episode').map(groupingTitle))].sort(cmp);
  }

  /** Les tuiles « Reprendre », la plus récente d'abord, avec la saison entière. */
  function continueEntries(items, W) {
    const dated = [];
    for (const e of build(items)) {
      if (e.type === 'single') {
        const r = W.rec(e.item);
        if (W.resume(e.item) != null && r) dated.push([e, r.lastPlayed]);
      } else if (W.seasonInProgress(e)) {
        dated.push([e, W.lastPlayedIn(e)]);
      }
    }
    return dated.sort((a, b) => b[1] - a[1]).map((x) => x[0]);
  }

  function counts(items, W) {
    return {
      all: items.length,
      movies: items.filter((i) => i.parsed.kind === 'movie').length,
      shows: items.filter((i) => i.parsed.kind === 'episode').length,
      audio: items.filter((i) => i.parsed.kind === 'audio').length,
      unmatched: items.filter((i) => !matched(i)).length,
      continueWatching: continueEntries(items, W).length,
    };
  }

  function visibleItems(items, sel, search, sort) {
    let r;
    switch (sel) {
      case 'all': r = items.slice(); break;
      case 'movies': r = items.filter((i) => i.parsed.kind === 'movie'); break;
      case 'shows': r = items.filter((i) => i.parsed.kind === 'episode'); break;
      case 'audio': r = items.filter((i) => i.parsed.kind === 'audio'); break;
      case 'unmatched': r = items.filter((i) => !matched(i)); break;
      case 'recent': return items.slice().sort((a, b) => b.modified - a.modified).slice(0, 60);
      default:
        if (sel.startsWith('series:')) {
          const name = sel.slice(7);
          return items.filter((i) => i.parsed.kind === 'episode' && groupingTitle(i) === name);
        }
        r = items.slice();
    }
    const q = search.trim();
    if (q) r = r.filter((i) => contains(displayTitle(i), q) || contains(groupingTitle(i), q) || contains(i.filename, q));
    return r;
  }

  function sortEntries(entries, sort) {
    const byTitle = (a, b) => cmp(a.seriesTitle, b.seriesTitle) || (entrySeasonNumber(a) - entrySeasonNumber(b));
    switch (sort) {
      case 'dateAdded': return entries.sort((a, b) => b.latestModified - a.latestModified);
      case 'size': return entries.sort((a, b) => b.totalSize - a.totalSize);
      case 'kind': return entries.sort((a, b) => (entryKind(a) === entryKind(b) ? byTitle(a, b) : entryKind(a) < entryKind(b) ? -1 : 1));
      default: return entries.sort(byTitle);
    }
  }

  function visibleEntries(items, sel, search, sort, W) {
    if (sel === 'continueWatching') {
      const all = continueEntries(items, W);
      const q = search.trim();
      return q ? all.filter((e) => contains(e.seriesTitle, q)) : all;
    }
    const entries = build(visibleItems(items, sel, search, sort));
    if (sel === 'recent') return entries.sort((a, b) => b.latestModified - a.latestModified);
    return sortEntries(entries, sort);
  }

  /** La saison entière qui contient cet épisode, dans l'ordre de lecture. */
  function seasonOf(items, it) {
    if (it.parsed.kind !== 'episode') return null;
    return build(items).find((e) => e.type === 'season' && e.episodes.some((x) => x.id === it.id)) || null;
  }

  return {
    KIND, SPECIAL, SECTIONS, SORTS, COMPLETION, MIN_RESUME, matched, displayTitle, displaySubtitle, seasonNumber, episodeNumber,
    episodeRowTitle, playerTitle, playerSubtitle, groupingTitle, build, entryId, entryKind, entrySubtitle, progressOf,
    isResumable, formatTime, Watch, sectionTitle, seriesNames, continueEntries, counts, visibleItems, visibleEntries, seasonOf, cmp,
  };
}));
