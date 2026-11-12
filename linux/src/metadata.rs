//! Identification en ligne : TMDB (avec clé), puis TVmaze et Wikidata (sans
//! clé, séries seulement), et le cache disque qui rend une nouvelle analyse
//! instantanée. Traduction de MetadataService.swift, TMDBProvider.swift et
//! TVmazeProvider.swift.

use crate::common::store;
use crate::model::Metadata;
use crate::parser::{Kind, Parsed};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use unicode_normalization::UnicodeNormalization;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(20))).user_agent("Biblifilm/1.0").http_status_as_error(false).build().into()
}

fn get_json(agent: &ureq::Agent, url: &str, bearer: Option<&str>) -> Result<Value, u16> {
    let mut req = agent.get(url);
    if let Some(b) = bearer { req = req.header("Authorization", &format!("Bearer {b}")); }
    let mut resp = req.call().map_err(|_| 0u16)?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) { return Err(status); }
    resp.body_mut().read_json::<Value>().map_err(|_| 0)
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { out.push(b as char) } else { out.push_str(&format!("%{b:02X}")) }
    }
    out
}

pub fn normalize(s: &str) -> String {
    let f: String = s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase();
    let f: String = f.chars().map(|c| if c.is_ascii_alphanumeric() || c == ' ' { c } else { ' ' }).collect();
    f.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn s(v: &Value, k: &str) -> Option<String> { v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(String::from) }
fn year_of(d: Option<String>) -> Option<i32> { d.filter(|d| d.len() >= 4).and_then(|d| d[..4].parse().ok()) }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Candidate {
    pub provider_id: i64,
    pub is_series: bool,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<i32>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
}

impl Candidate {
    pub fn subtitle(&self) -> String {
        let mut p = vec![];
        if let Some(y) = self.year { p.push(y.to_string()); }
        if let Some(o) = self.original_title.as_ref().filter(|o| **o != self.title) { p.push(o.clone()); }
        p.join(" · ")
    }
}

// ── TMDB ──────────────────────────────────────────────────────────────────

pub struct Tmdb { key: String, agent: ureq::Agent }

impl Tmdb {
    pub fn new(key: &str) -> Self { Tmdb { key: key.trim().to_string(), agent: agent() } }
    /// La courte « API Key » (v3) ou le long « Read Access Token » (v4, un JWT).
    fn bearer(&self) -> bool { self.key.starts_with("eyJ") && self.key.contains('.') }

    fn get(&self, path: &str, q: &[(&str, String)]) -> Result<Value, u16> {
        let mut url = format!("https://api.themoviedb.org/3{path}?");
        if !self.bearer() { url += &format!("api_key={}&", enc(&self.key)); }
        url += &q.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&");
        get_json(&self.agent, &url, if self.bearer() { Some(&self.key) } else { None })
    }

    pub fn validate(key: &str) -> Option<String> {
        let t = Tmdb::new(key);
        if t.key.is_empty() { return Some("Aucune clé saisie.".into()); }
        match t.get("/configuration", &[]) {
            Ok(_) => None,
            Err(401) => Some(if t.bearer() { "Jeton refusé (401). Vérifiez le « API Read Access Token ».".into() }
                else { "Clé refusée (401). Copiez la « API Key » (32 caractères) ou le jeton de lecture.".into() }),
            Err(0) => Some("Connexion impossible.".into()),
            Err(c) => Some(format!("TMDB a répondu HTTP {c}.")),
        }
    }

    fn image(p: Option<String>, size: &str) -> Option<String> { p.map(|p| format!("https://image.tmdb.org/t/p/{size}{p}")) }

    /// L'ordre de TMDB ne vaut rien pour un titre exact : on reclasse sur le titre et l'année.
    fn score(title: Option<&str>, original: Option<&str>, date: Option<String>, popularity: f64, want: &Parsed) -> f64 {
        let mut sc = 0.0;
        let w = normalize(&want.title);
        let (t, o) = (normalize(title.unwrap_or("")), normalize(original.unwrap_or("")));
        if !w.is_empty() {
            if t == w || o == w { sc += 100.0 } else if t.starts_with(&w) || o.starts_with(&w) { sc += 25.0 }
        }
        if let (Some(wy), Some(gy)) = (want.year, year_of(date)) {
            let gap = (gy - wy).abs();
            sc += if gap == 0 { 60.0 } else if gap <= 1 { 30.0 } else { -20.0 };
        }
        sc + (popularity / 10.0).min(5.0)
    }

    fn agrees(title: Option<&str>, original: Option<&str>, want: &Parsed) -> bool {
        let w = normalize(&want.title);
        if w.is_empty() { return false; }
        let (t, o) = (normalize(title.unwrap_or("")), normalize(original.unwrap_or("")));
        t == w || o == w || t.starts_with(&w) || o.starts_with(&w)
    }

    fn search(&self, path: &str, title: &str, year: Option<i32>, year_key: &str, lang: &str) -> Vec<Value> {
        let mut q = vec![("query", title.to_string()), ("language", lang.to_string()), ("include_adult", "false".into())];
        if let Some(y) = year { q.push((year_key, y.to_string())); }
        let mut r = self.get(path, &q).ok().and_then(|v| v["results"].as_array().cloned()).unwrap_or_default();
        if r.is_empty() && year.is_some() {
            q.pop();
            r = self.get(path, &q).ok().and_then(|v| v["results"].as_array().cloned()).unwrap_or_default();
        }
        r
    }

    fn best<'a>(r: &'a [Value], want: &Parsed, tk: &str, ok: &str, dk: &str) -> Option<&'a Value> {
        r.iter().max_by(|a, b| {
            let sa = Self::score(a[tk].as_str(), a[ok].as_str(), s(a, dk), a["popularity"].as_f64().unwrap_or(0.0), want);
            let sb = Self::score(b[tk].as_str(), b[ok].as_str(), s(b, dk), b["popularity"].as_f64().unwrap_or(0.0), want);
            sa.partial_cmp(&sb).unwrap()
        })
    }

    pub fn lookup(&self, p: &Parsed, lang: &str) -> Option<Metadata> {
        match p.kind {
            Kind::Episode => {
                let r = self.search("/search/tv", &p.title, p.year, "first_air_date_year", lang);
                let show = Self::best(&r, p, "name", "original_name", "first_air_date")?.clone();
                self.tv(show["id"].as_i64()?, p, lang, Some(&show))
            }
            Kind::Audio => None,
            _ => {
                let r = self.search("/search/movie", &p.title, p.year, "year", lang);
                let mut best = Self::best(&r, p, "title", "original_title", "release_date").cloned();
                // « Rocky I » : TMDB range le premier film sous « Rocky ». Seulement en repli.
                let variant = fancy_regex::Regex::new(r"\s+(?:I|1)$").unwrap().replace(&p.title, "").to_string();
                let agrees = best.as_ref().map(|b| Self::agrees(b["title"].as_str(), b["original_title"].as_str(), p)).unwrap_or(false);
                if !agrees && variant != p.title && variant.chars().count() >= 2 {
                    let retry = Parsed { title: variant.clone(), ..p.clone() };
                    let alt = self.search("/search/movie", &variant, p.year, "year", lang);
                    if let Some(c) = Self::best(&alt, &retry, "title", "original_title", "release_date") {
                        if Self::agrees(c["title"].as_str(), c["original_title"].as_str(), &retry) { best = Some(c.clone()); }
                    }
                }
                let b = best?;
                self.movie(b["id"].as_i64()?, lang, Some(&b))
            }
        }
    }

    fn movie(&self, id: i64, lang: &str, fb: Option<&Value>) -> Option<Metadata> {
        let d = self.get(&format!("/movie/{id}"), &[("language", lang.into())]).ok();
        if d.is_none() && fb.is_none() { return None; }
        let null = Value::Null;
        let (d, f) = (d.as_ref().unwrap_or(&null), fb.unwrap_or(&null));
        Some(Metadata {
            series_title: s(d, "title").or(s(f, "title")).or(s(d, "original_title")).or(s(f, "original_title")),
            overview: s(d, "overview").or(s(f, "overview")),
            year: year_of(s(d, "release_date").or(s(f, "release_date"))),
            rating: d["vote_average"].as_f64().or(f["vote_average"].as_f64()),
            genres: d["genres"].as_array().map(|g| g.iter().filter_map(|x| s(x, "name")).collect()).unwrap_or_default(),
            poster_url: Self::image(s(d, "poster_path").or(s(f, "poster_path")), "w500"),
            source: Some("TMDB".into()), matched: true, ..Default::default()
        })
    }

    fn tv(&self, id: i64, p: &Parsed, lang: &str, fb: Option<&Value>) -> Option<Metadata> {
        let d = self.get(&format!("/tv/{id}"), &[("language", lang.into())]).ok();
        if d.is_none() && fb.is_none() { return None; }
        let null = Value::Null;
        let (d, f) = (d.as_ref().unwrap_or(&null), fb.unwrap_or(&null));
        let (mut season, mut episode) = (p.season, p.episode);
        if season.is_none() {
            if let Some(abs) = p.absolute_episode {
                let seasons: Vec<(i64, i64)> = d["seasons"].as_array().map(|a| a.iter().map(|x| (x["season_number"].as_i64().unwrap_or(0), x["episode_count"].as_i64().unwrap_or(0))).collect()).unwrap_or_default();
                let (s2, e2) = resolve_absolute(abs as i64, &seasons);
                season = Some(s2 as i32); episode = Some(e2 as i32);
            }
        }
        let mut m = Metadata {
            series_title: s(d, "name").or(s(f, "name")).or(s(f, "original_name")),
            overview: s(d, "overview").or(s(f, "overview")),
            year: year_of(s(d, "first_air_date").or(s(f, "first_air_date"))),
            rating: d["vote_average"].as_f64().or(f["vote_average"].as_f64()),
            genres: d["genres"].as_array().map(|g| g.iter().filter_map(|x| s(x, "name")).collect()).unwrap_or_default(),
            poster_url: Self::image(s(d, "poster_path").or(s(f, "poster_path")), "w500"),
            source: Some("TMDB".into()), matched: true, ..Default::default()
        };
        let (Some(sn), Some(en)) = (season, episode) else { return Some(m) };
        let ep = self.get(&format!("/tv/{id}/season/{sn}/episode/{en}"), &[("language", lang.into())]).ok();
        let ep = ep.as_ref().unwrap_or(&null);
        m.episode_title = s(ep, "name");
        if let Some(o) = s(ep, "overview") { m.overview = Some(o); }
        m.season = ep["season_number"].as_i64().map(|x| x as i32).or(Some(sn));
        m.episode = ep["episode_number"].as_i64().map(|x| x as i32).or(Some(en));
        if let Some(r) = ep["vote_average"].as_f64() { m.rating = Some(r); }
        m.still_url = Self::image(s(ep, "still_path"), "w780");
        Some(m)
    }

    pub fn candidates(&self, p: &Parsed, lang: &str) -> Vec<Candidate> {
        if p.kind == Kind::Episode {
            let mut r = self.search("/search/tv", &p.title, p.year, "first_air_date_year", lang);
            r.sort_by(|a, b| Self::score(b["name"].as_str(), b["original_name"].as_str(), s(b, "first_air_date"), b["popularity"].as_f64().unwrap_or(0.0), p)
                .partial_cmp(&Self::score(a["name"].as_str(), a["original_name"].as_str(), s(a, "first_air_date"), a["popularity"].as_f64().unwrap_or(0.0), p)).unwrap());
            return r.iter().take(12).map(|x| Candidate { provider_id: x["id"].as_i64().unwrap_or(0), is_series: true,
                title: s(x, "name").or(s(x, "original_name")).unwrap_or(p.title.clone()), original_title: s(x, "original_name"),
                year: year_of(s(x, "first_air_date")), overview: s(x, "overview"), poster_url: Self::image(s(x, "poster_path"), "w342") }).collect();
        }
        let mut q = vec![("query", p.title.clone()), ("language", lang.to_string()), ("include_adult", "false".into())];
        if let Some(y) = p.year { q.push(("year", y.to_string())); }
        let mut r = self.get("/search/movie", &q).ok().and_then(|v| v["results"].as_array().cloned()).unwrap_or_default();
        // L'année est un filtre, pas un fait : un fichier mal étiqueté mérite un filet plus large.
        if r.len() < 2 && p.year.is_some() {
            q.pop();
            let known: Vec<i64> = r.iter().filter_map(|x| x["id"].as_i64()).collect();
            if let Some(more) = self.get("/search/movie", &q).ok().and_then(|v| v["results"].as_array().cloned()) {
                r.extend(more.into_iter().filter(|x| !known.contains(&x["id"].as_i64().unwrap_or(-1))));
            }
        }
        r.sort_by(|a, b| Self::score(b["title"].as_str(), b["original_title"].as_str(), s(b, "release_date"), b["popularity"].as_f64().unwrap_or(0.0), p)
            .partial_cmp(&Self::score(a["title"].as_str(), a["original_title"].as_str(), s(a, "release_date"), a["popularity"].as_f64().unwrap_or(0.0), p)).unwrap());
        r.iter().take(12).map(|x| Candidate { provider_id: x["id"].as_i64().unwrap_or(0), is_series: false,
            title: s(x, "title").or(s(x, "original_title")).unwrap_or(p.title.clone()), original_title: s(x, "original_title"),
            year: year_of(s(x, "release_date")), overview: s(x, "overview"), poster_url: Self::image(s(x, "poster_path"), "w342") }).collect()
    }

    pub fn for_candidate(&self, c: &Candidate, p: &Parsed, lang: &str) -> Option<Metadata> {
        if c.is_series { self.tv(c.provider_id, p, lang, None) } else { self.movie(c.provider_id, lang, None) }
    }
}

/// Numéro absolu (anime) → saison/épisode, en parcourant la longueur des saisons.
pub fn resolve_absolute(abs: i64, seasons: &[(i64, i64)]) -> (i64, i64) {
    let mut ss: Vec<&(i64, i64)> = seasons.iter().filter(|s| s.0 > 0).collect();
    if ss.is_empty() { return (1, abs); }
    ss.sort_by_key(|s| s.0);
    let mut rest = abs;
    for (n, count) in ss {
        if rest <= *count { return (*n, rest); }
        rest -= count;
    }
    (1, abs)
}

// ── TVmaze + Wikidata ─────────────────────────────────────────────────────

fn strip_html(h: Option<String>) -> Option<String> {
    let h = h?;
    let t = fancy_regex::Regex::new("<[^>]+>").unwrap().replace_all(&h, "").replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ");
    let t = t.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

fn tvmaze(p: &Parsed) -> Option<Metadata> {
    // Seulement les séries : chercher un film dans TVmaze rend des absurdités sûres d'elles.
    if p.kind != Kind::Episode { return None; }
    let a = agent();
    let search = |title: &str| -> Option<Value> {
        let hits = get_json(&a, &format!("https://api.tvmaze.com/search/shows?q={}", enc(title)), None).ok()?;
        let hits: Vec<Value> = hits.as_array()?.iter().filter(|h| h["score"].as_f64().unwrap_or(0.0) >= 0.6).cloned().collect();
        let rank = |h: &Value| {
            let mut sc = h["score"].as_f64().unwrap_or(0.0);
            let y = s(&h["show"], "premiered").and_then(|d| d.get(..4).and_then(|x| x.parse::<i32>().ok()));
            if let (Some(w), Some(y)) = (p.year, y) { let g = (y - w).abs(); sc += if g == 0 { 1.0 } else if g <= 1 { 0.5 } else if g >= 10 { -0.3 } else { 0.0 }; }
            let animated = h["show"]["type"].as_str().map(|t| t.eq_ignore_ascii_case("animation")).unwrap_or(false)
                || h["show"]["genres"].as_array().map(|g| g.iter().any(|x| x.as_str().map(|s| s.eq_ignore_ascii_case("anime")).unwrap_or(false))).unwrap_or(false);
            if p.absolute_episode.is_some() && animated { sc += 0.4; }
            sc
        };
        hits.into_iter().max_by(|x, y| rank(x).partial_cmp(&rank(y)).unwrap()).map(|h| h["show"].clone())
    };
    let mut show = search(&p.title);
    if show.is_none() {
        // Un titre traduit (« Nicky Larson ») : Wikidata donne le nom que TVmaze connaît.
        let w = get_json(&a, &format!("https://www.wikidata.org/w/api.php?action=wbsearchentities&search={}&language=fr&uselang=en&format=json&limit=3&type=item", enc(&p.title)), None).ok();
        if let Some(label) = w.and_then(|w| s(&w["search"][0], "label")) {
            if label.to_lowercase() != p.title.to_lowercase() { show = search(&label); }
        }
    }
    let show = show?;
    let mut m = Metadata {
        series_title: s(&show, "name"), overview: strip_html(s(&show, "summary")),
        year: s(&show, "premiered").and_then(|d| d.get(..4).and_then(|x| x.parse().ok())),
        rating: show["rating"]["average"].as_f64(),
        genres: show["genres"].as_array().map(|g| g.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        poster_url: s(&show["image"], "original").or(s(&show["image"], "medium")), source: Some("TVmaze".into()), matched: true, ..Default::default()
    };
    let id = show["id"].as_i64()?;
    let ep = if let (Some(sn), Some(en)) = (p.season, p.episode) {
        get_json(&a, &format!("https://api.tvmaze.com/shows/{id}/episodebynumber?season={sn}&number={en}"), None).ok()
    } else if let Some(abs) = p.absolute_episode {
        get_json(&a, &format!("https://api.tvmaze.com/shows/{id}/episodes"), None).ok()
            .and_then(|all| all.as_array().and_then(|v| if abs >= 1 && (abs as usize) <= v.len() { Some(v[abs as usize - 1].clone()) } else { None }))
    } else { None };
    if let Some(ep) = ep {
        m.episode_title = s(&ep, "name");
        m.season = ep["season"].as_i64().map(|x| x as i32);
        m.episode = ep["number"].as_i64().map(|x| x as i32);
        if let Some(o) = strip_html(s(&ep, "summary")) { m.overview = Some(o); }
        m.still_url = s(&ep["image"], "original").or(s(&ep["image"], "medium"));
    }
    Some(m)
}

// ── Service et cache ──────────────────────────────────────────────────────

pub struct Service {
    file: PathBuf,
    cache: Mutex<(HashMap<String, Metadata>, bool)>,
}

impl Service {
    pub fn new(file: PathBuf) -> Self {
        let cache = store::read_json::<HashMap<String, Metadata>>(&file).unwrap_or_default();
        Service { file, cache: Mutex::new((cache, false)) }
    }

    /// La clé inclut l'épisode, pour que chacun garde son titre.
    pub fn key(p: &Parsed, lang: &str) -> String {
        let mut k = format!("{}|{lang}", p.show_key());
        if let (Some(s), Some(e)) = (p.season, p.episode) { k += &format!("|s{s}e{e}"); }
        else if let Some(a) = p.absolute_episode { k += &format!("|a{a}"); }
        k
    }

    pub fn cached(&self, p: &Parsed, lang: &str) -> Option<Metadata> { self.cache.lock().unwrap().0.get(&Self::key(p, lang)).cloned() }

    /// Cherche en ligne (TMDB, puis TVmaze) ; les échecs sont notés aussi.
    pub fn lookup(&self, p: &Parsed, key: Option<&str>, lang: &str) -> Metadata {
        if let Some(m) = self.cached(p, lang) { return m; }
        let found = key.and_then(|k| Tmdb::new(k).lookup(p, lang)).or_else(|| tvmaze(p));
        let m = found.unwrap_or_else(Metadata::miss);
        let mut c = self.cache.lock().unwrap();
        c.0.insert(Self::key(p, lang), m.clone());
        c.1 = true;
        m
    }

    pub fn flush(&self) {
        let mut c = self.cache.lock().unwrap();
        if !c.1 { return; }
        let _ = store::write_json(&self.file, &c.0, false);
        c.1 = false;
    }

    pub fn store(&self, m: Metadata, p: &Parsed, lang: &str) {
        self.cache.lock().unwrap().0.insert(Self::key(p, lang), m);
        self.cache.lock().unwrap().1 = true;
        self.flush();
    }
    pub fn forget(&self, p: &Parsed, lang: &str) {
        let removed = self.cache.lock().unwrap().0.remove(&Self::key(p, lang)).is_some();
        if removed { self.cache.lock().unwrap().1 = true; self.flush(); }
    }
    pub fn forget_unmatched(&self) {
        let mut c = self.cache.lock().unwrap();
        let before = c.0.len();
        c.0.retain(|_, v| v.matched);
        if c.0.len() != before { c.1 = true; }
        drop(c);
        self.flush();
    }
    pub fn clear(&self) { let mut c = self.cache.lock().unwrap(); c.0.clear(); c.1 = true; drop(c); self.flush(); }
    pub fn count(&self) -> usize { self.cache.lock().unwrap().0.len() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absolute_numbering() {
        assert_eq!(resolve_absolute(1, &[(1, 12), (2, 10)]), (1, 1));
        assert_eq!(resolve_absolute(15, &[(0, 3), (1, 12), (2, 10)]), (2, 3));
        assert_eq!(resolve_absolute(40, &[(1, 12)]), (1, 40));
    }
    #[test]
    fn titles_compare_without_accents() {
        assert_eq!(normalize("Amélie : Le Fabuleux!"), "amelie le fabuleux");
    }
}
