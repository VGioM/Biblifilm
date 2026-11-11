// metadata.js — identification en ligne : TMDB (avec clé), puis TVmaze et
// Wikidata (sans clé, séries seulement), et le cache disque qui rend une
// nouvelle analyse instantanée. Traduction de MetadataService.swift,
// TMDBProvider.swift et TVmazeProvider.swift.
'use strict';

const fs = require('fs');
const { showKey } = require('./parser');
const { writeJSON } = require('./store');

async function json(url, headers = {}) {
  const res = await fetch(url, { headers: { 'User-Agent': 'Biblifilm/1.0', ...headers }, signal: AbortSignal.timeout(20000) });
  if (!res.ok) { const e = new Error(`Réponse du service : HTTP ${res.status}`); e.status = res.status; throw e; }
  return res.json();
}

const yearOf = (d) => (d && d.length >= 4 ? parseInt(d.slice(0, 4), 10) || null : null);
const normalize = (s) => String(s || '').normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase()
  .replace(/[^a-z0-9 ]/g, ' ').replace(/\s+/g, ' ').trim();

// ── TMDB ───────────────────────────────────────────────────────────────────

class TMDB {
  constructor(apiKey) { this.apiKey = apiKey.trim(); this.name = 'TMDB'; }
  // Les deux identifiants que TMDB propose : la courte « API Key » (v3) ou le
  // long « Read Access Token » (v4, un JWT).
  get bearer() { return this.apiKey.startsWith('eyJ') && this.apiKey.includes('.'); }

  get(p, query = {}) {
    const u = new URL('https://api.themoviedb.org/3' + p);
    if (!this.bearer) u.searchParams.set('api_key', this.apiKey);
    for (const [k, v] of Object.entries(query)) u.searchParams.set(k, v);
    return json(u.toString(), this.bearer ? { Authorization: `Bearer ${this.apiKey}` } : {});
  }

  static async validate(key) {
    const probe = new TMDB(key || '');
    if (!probe.apiKey) return 'Aucune clé saisie.';
    try { await probe.get('/configuration'); return null; } catch (e) {
      if (e.status === 401) {
        return probe.bearer ? 'Jeton refusé (401). Vérifiez le « API Read Access Token ».'
          : 'Clé refusée (401). Copiez la « API Key » (32 caractères) ou le jeton de lecture.';
      }
      if (e.status) return `TMDB a répondu HTTP ${e.status}.`;
      return `Connexion impossible : ${e.message}`;
    }
  }

  image(p, size) { return p ? `https://image.tmdb.org/t/p/${size}${p}` : null; }

  /** L'ordre de TMDB ne vaut rien pour un titre exact : on reclasse sur le titre et l'année. */
  score(title, original, date, popularity, want) {
    let s = 0;
    const w = normalize(want.title);
    if (w) {
      if (normalize(title) === w || normalize(original) === w) s += 100;
      else if (normalize(title).startsWith(w) || normalize(original).startsWith(w)) s += 25;
    }
    const got = yearOf(date);
    if (want.year && got) {
      const gap = Math.abs(got - want.year);
      s += gap === 0 ? 60 : gap <= 1 ? 30 : -20;
    }
    return s + Math.min((popularity || 0) / 10, 5);
  }

  titleAgrees(title, original, want) {
    const w = normalize(want.title);
    if (!w) return false;
    return normalize(title) === w || normalize(original) === w || normalize(title).startsWith(w) || normalize(original).startsWith(w);
  }

  async lookup(p, language) {
    if (p.kind === 'episode') return this.lookupTV(p, language);
    if (p.kind === 'audio') return null;
    return this.lookupMovie(p, language);
  }

  async searchMovies(title, year, language) {
    const q = { query: title, language, include_adult: 'false' };
    if (year) q.year = String(year);
    let r = (await this.get('/search/movie', q)).results || [];
    if (!r.length && year) { delete q.year; r = (await this.get('/search/movie', q)).results || []; }
    return r;
  }

  pickMovie(results, want) {
    let best = null, bs = -Infinity;
    for (const r of results) {
      const s = this.score(r.title, r.original_title, r.release_date, r.popularity, want);
      if (s > bs) { bs = s; best = r; }
    }
    return best;
  }

  async lookupMovie(p, language) {
    let best = this.pickMovie(await this.searchMovies(p.title, p.year, language), p);
    // « Rocky I » : TMDB range le premier film sous « Rocky ». Seulement en repli.
    const variant = p.title.replace(/\s+(?:I|1)$/, '');
    if ((!best || !this.titleAgrees(best.title, best.original_title, p)) && variant !== p.title && variant.length >= 2) {
      const retry = { ...p, title: variant };
      const cand = this.pickMovie(await this.searchMovies(variant, p.year, language), retry);
      if (cand && this.titleAgrees(cand.title, cand.original_title, retry)) best = cand;
    }
    if (!best) return null;
    return this.movieMetadata(best.id, language, best);
  }

  async movieMetadata(id, language, fallback) {
    let d = null;
    try { d = await this.get(`/movie/${id}`, { language }); } catch { /* repli sur le résultat de recherche */ }
    if (!d && !fallback) return null;
    const f = fallback || {};
    d = d || {};
    return {
      seriesTitle: d.title || f.title || d.original_title || f.original_title || null,
      episodeTitle: null,
      overview: [d.overview, f.overview].find((x) => x) || null,
      year: yearOf(d.release_date || f.release_date),
      season: null, episode: null,
      rating: d.vote_average != null ? d.vote_average : f.vote_average,
      genres: (d.genres || []).map((g) => g.name),
      posterURL: this.image(d.poster_path || f.poster_path, 'w500'),
      stillURL: null, source: 'TMDB', matched: true,
    };
  }

  async searchTV(title, year, language) {
    const q = { query: title, language, include_adult: 'false' };
    if (year) q.first_air_date_year = String(year);
    let r = (await this.get('/search/tv', q)).results || [];
    if (!r.length && year) { delete q.first_air_date_year; r = (await this.get('/search/tv', q)).results || []; }
    return r;
  }

  async lookupTV(p, language) {
    const results = await this.searchTV(p.title, p.year, language);
    let show = null, bs = -Infinity;
    for (const r of results) {
      const s = this.score(r.name, r.original_name, r.first_air_date, r.popularity, p);
      if (s > bs) { bs = s; show = r; }
    }
    if (!show) return null;
    return this.tvMetadata(show.id, p, language, show);
  }

  async tvMetadata(showID, p, language, fallback) {
    let d = null;
    try { d = await this.get(`/tv/${showID}`, { language }); } catch { /* repli */ }
    if (!d && !fallback) return null;
    const f = fallback || {};
    d = d || {};
    const seriesTitle = d.name || f.name || f.original_name || null;
    const poster = this.image(d.poster_path || f.poster_path, 'w500');
    let season = p.season, episode = p.episode;
    if (season == null && p.absoluteEpisode != null) [season, episode] = resolveAbsolute(p.absoluteEpisode, d.seasons);
    const base = {
      seriesTitle, episodeTitle: null, overview: d.overview || f.overview || null,
      year: yearOf(d.first_air_date || f.first_air_date), season: null, episode: null,
      rating: d.vote_average != null ? d.vote_average : f.vote_average,
      genres: (d.genres || []).map((g) => g.name), posterURL: poster, stillURL: null, source: 'TMDB', matched: true,
    };
    if (season == null || episode == null) return base;
    let ep = null;
    try { ep = await this.get(`/tv/${showID}/season/${season}/episode/${episode}`, { language }); } catch { /* inconnu */ }
    return {
      ...base,
      episodeTitle: ep && ep.name || null,
      overview: (ep && ep.overview) || base.overview,
      season: ep && ep.season_number != null ? ep.season_number : season,
      episode: ep && ep.episode_number != null ? ep.episode_number : episode,
      rating: ep && ep.vote_average != null ? ep.vote_average : base.rating,
      stillURL: this.image(ep && ep.still_path, 'w780'),
    };
  }

  /** Toutes les correspondances plausibles, la meilleure d'abord, pour la feuille « Corriger ». */
  async candidates(p, language) {
    if (p.kind === 'episode') {
      const r = await this.searchTV(p.title, p.year, language);
      return r.map((x) => [x, this.score(x.name, x.original_name, x.first_air_date, x.popularity, p)])
        .sort((a, b) => b[1] - a[1]).slice(0, 12).map(([x]) => ({
          providerID: x.id, isSeries: true, title: x.name || x.original_name || p.title, originalTitle: x.original_name || null,
          year: yearOf(x.first_air_date), overview: x.overview || null, posterURL: this.image(x.poster_path, 'w342'),
        }));
    }
    const q = { query: p.title, language, include_adult: 'false' };
    if (p.year) q.year = String(p.year);
    let r = (await this.get('/search/movie', q)).results || [];
    // L'année est un filtre, pas un fait : un fichier mal étiqueté mérite un filet plus large.
    if (r.length < 2 && p.year) {
      delete q.year;
      const known = new Set(r.map((x) => x.id));
      r = r.concat(((await this.get('/search/movie', q)).results || []).filter((x) => !known.has(x.id)));
    }
    return r.map((x) => [x, this.score(x.title, x.original_title, x.release_date, x.popularity, p)])
      .sort((a, b) => b[1] - a[1]).slice(0, 12).map(([x]) => ({
        providerID: x.id, isSeries: false, title: x.title || x.original_title || p.title, originalTitle: x.original_title || null,
        year: yearOf(x.release_date), overview: x.overview || null, posterURL: this.image(x.poster_path, 'w342'),
      }));
  }

  forCandidate(c, p, language) {
    return c.isSeries ? this.tvMetadata(c.providerID, p, language, null) : this.movieMetadata(c.providerID, language, null);
  }
}

/** Numéro absolu (anime) → saison/épisode, en parcourant la longueur des saisons. */
function resolveAbsolute(abs, seasons) {
  if (!seasons || !seasons.length) return [1, abs];
  let rest = abs;
  for (const s of seasons.filter((x) => x.season_number > 0).sort((a, b) => a.season_number - b.season_number)) {
    if (rest <= s.episode_count) return [s.season_number, rest];
    rest -= s.episode_count;
  }
  return [1, abs];
}

// ── TVmaze + Wikidata ──────────────────────────────────────────────────────

const stripHTML = (h) => {
  if (!h) return null;
  const t = h.replace(/<[^>]+>/g, '').replace(/&amp;/g, '&').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&nbsp;/g, ' ').trim();
  return t || null;
};

const TVmaze = {
  name: 'TVmaze',
  // Seulement les séries : chercher un film dans TVmaze rend des absurdités sûres d'elles.
  async lookup(p) {
    if (p.kind !== 'episode') return null;
    let show = await this.searchShow(p.title, p);
    if (!show) {
      let canonical = null;
      try { canonical = await this.canonicalTitle(p.title); } catch { /* Wikidata injoignable */ }
      if (canonical && canonical.toLowerCase() !== p.title.toLowerCase()) show = await this.searchShow(canonical, p);
    }
    if (!show) return null;
    const meta = {
      seriesTitle: show.name, episodeTitle: null, overview: stripHTML(show.summary),
      year: show.premiered ? parseInt(show.premiered.slice(0, 4), 10) : null, season: null, episode: null,
      rating: show.rating && show.rating.average, genres: show.genres || [],
      posterURL: show.image && (show.image.original || show.image.medium) || null, stillURL: null, source: 'TVmaze', matched: true,
    };
    try {
      const ep = await this.episode(show.id, p);
      if (ep) {
        meta.episodeTitle = ep.name || null;
        meta.season = ep.season; meta.episode = ep.number;
        const s = stripHTML(ep.summary);
        if (s) meta.overview = s;
        meta.stillURL = ep.image && (ep.image.original || ep.image.medium) || null;
      }
    } catch { /* série seule */ }
    return meta;
  },
  async searchShow(title, p) {
    const hits = (await json(`https://api.tvmaze.com/search/shows?q=${encodeURIComponent(title)}`)).filter((h) => h.score >= 0.6);
    if (!hits.length) return null;
    const preferAnimated = p.absoluteEpisode != null;
    const rank = (h) => {
      let s = h.score;
      const y = h.show.premiered ? parseInt(h.show.premiered.slice(0, 4), 10) : null;
      if (p.year && y) { const gap = Math.abs(y - p.year); s += gap === 0 ? 1 : gap <= 1 ? 0.5 : gap >= 10 ? -0.3 : 0; }
      const animated = /animation/i.test(h.show.type || '') || (h.show.genres || []).some((g) => /anime/i.test(g));
      if (preferAnimated && animated) s += 0.4;
      return s;
    };
    return hits.reduce((a, b) => (rank(b) > rank(a) ? b : a)).show;
  },
  async canonicalTitle(title) {
    const r = await json(`https://www.wikidata.org/w/api.php?action=wbsearchentities&search=${encodeURIComponent(title)}&language=fr&uselang=en&format=json&limit=3&type=item`);
    return r.search && r.search[0] ? r.search[0].label : null;
  },
  async episode(id, p) {
    if (p.season != null && p.episode != null) {
      try { return await json(`https://api.tvmaze.com/shows/${id}/episodebynumber?season=${p.season}&number=${p.episode}`); } catch { return null; }
    }
    if (p.absoluteEpisode != null) {
      const all = await json(`https://api.tvmaze.com/shows/${id}/episodes`);
      return p.absoluteEpisode >= 1 && p.absoluteEpisode <= all.length ? all[p.absoluteEpisode - 1] : null;
    }
    return null;
  },
};

// ── Service et cache ───────────────────────────────────────────────────────

class MetadataService {
  constructor(file) {
    this.file = file;
    try { this.cache = JSON.parse(fs.readFileSync(file, 'utf8')); } catch { this.cache = {}; }
    this.inFlight = new Map();
    this.dirty = false;
  }

  /** La clé inclut l'épisode, pour que chacun garde son titre. */
  key(p, language) {
    let k = `${showKey(p)}|${language}`;
    if (p.season != null && p.episode != null) k += `|s${p.season}e${p.episode}`;
    else if (p.absoluteEpisode != null) k += `|a${p.absoluteEpisode}`;
    return k;
  }

  cached(p, language) { return this.cache[this.key(p, language)] || null; }

  async metadata(p, settings) {
    const k = this.key(p, settings.language);
    if (this.cache[k]) return this.cache[k].matched ? this.cache[k] : null;
    if (this.inFlight.has(k)) return this.inFlight.get(k);
    const task = (async () => {
      const providers = [];
      if (settings.tmdbKey) providers.push(new TMDB(settings.tmdbKey));
      providers.push(TVmaze);
      for (const pr of providers) {
        try { const r = await pr.lookup(p, settings.language); if (r) return r; } catch { /* fournisseur suivant */ }
      }
      return null;
    })();
    this.inFlight.set(k, task);
    try {
      const result = await task;
      // Les échecs sont notés aussi, pour ne pas harceler le réseau à chaque lancement.
      this.cache[k] = result || { matched: false };
      this.dirty = true;
      return result;
    } finally {
      this.inFlight.delete(k);
    }
  }

  flush() {
    if (!this.dirty) return;
    writeJSON(this.file, this.cache);
    this.dirty = false;
  }

  store(meta, p, language) { this.cache[this.key(p, language)] = meta; this.dirty = true; this.flush(); }
  forget(p, language) { if (delete this.cache[this.key(p, language)]) { this.dirty = true; this.flush(); } }
  forgetUnmatched() {
    const before = Object.keys(this.cache).length;
    for (const [k, v] of Object.entries(this.cache)) if (!v.matched) delete this.cache[k];
    if (Object.keys(this.cache).length !== before) { this.dirty = true; this.flush(); }
  }
  clear() { this.cache = {}; this.dirty = true; this.flush(); }
  get count() { return Object.keys(this.cache).length; }

  async candidates(p, settings) {
    if (!settings.tmdbKey) return [];
    return new TMDB(settings.tmdbKey).candidates(p, settings.language);
  }

  async forCandidate(c, p, settings) {
    if (!settings.tmdbKey) return null;
    return new TMDB(settings.tmdbKey).forCandidate(c, p, settings.language);
  }
}

module.exports = { MetadataService, TMDB, TVmaze, resolveAbsolute, normalize };
