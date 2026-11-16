//! Les fiches, les tuiles (un film, ou une saison), la reprise de lecture et
//! les épisodes vus, les sections de la barre latérale et le tri. Traduction de
//! MediaItem.swift, LibraryEntry.swift, WatchStore.swift et des parties pures
//! de LibraryModel.swift.

use crate::parser::{Kind, Parsed, Special};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Metadata {
    pub series_title: Option<String>,
    pub episode_title: Option<String>,
    pub overview: Option<String>,
    pub year: Option<i32>,
    pub season: Option<i32>,
    pub episode: Option<i32>,
    pub rating: Option<f64>,
    pub genres: Vec<String>,
    #[serde(rename = "posterURL")]
    pub poster_url: Option<String>,
    #[serde(rename = "stillURL")]
    pub still_url: Option<String>,
    pub source: Option<String>,
    /// Faux : un échec noté, pour ne pas réessayer à chaque lancement.
    pub matched: bool,
}

impl Metadata {
    pub fn miss() -> Self { Metadata { matched: false, ..Default::default() } }
    pub fn code(&self) -> Option<String> { Some(format!("S{:02}E{:02}", self.season?, self.episode?)) }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub path: PathBuf,
    pub filename: String,
    pub ext: String,
    pub parsed: Parsed,
    pub size: u64,
    pub modified: i64,
    pub metadata: Option<Metadata>,
}

impl Item {
    pub fn matched(&self) -> Option<&Metadata> { self.metadata.as_ref().filter(|m| m.matched) }
    pub fn kind(&self) -> Kind { self.parsed.kind }

    pub fn display_title(&self) -> String {
        if let Some(m) = self.matched() {
            if self.kind() == Kind::Episode { if let Some(e) = m.episode_title.as_ref().filter(|s| !s.is_empty()) { return e.clone(); } }
            if let Some(s) = m.series_title.as_ref().filter(|s| !s.is_empty()) { return s.clone(); }
        }
        self.parsed.title.clone()
    }

    pub fn display_subtitle(&self) -> String {
        let mut parts = vec![];
        let m = self.metadata.as_ref();
        if self.kind() == Kind::Episode {
            if let Some(s) = self.matched().and_then(|m| m.series_title.clone()) { parts.push(s); }
            if let Some(c) = m.and_then(|m| m.code()) { parts.push(c); }
            else if let Some(a) = self.parsed.absolute_episode { parts.push(format!("Ép. {a:03}")); }
            else if let (Some(s), Some(e)) = (self.parsed.season, self.parsed.episode) { parts.push(format!("S{s:02}E{e:02}")); }
        } else {
            if let Some(y) = m.and_then(|m| m.year).or(self.parsed.year) { parts.push(y.to_string()); }
            if let Some(g) = m.and_then(|m| m.genres.first().cloned()) { parts.push(g); }
        }
        if parts.is_empty() { parts.push(self.ext.to_uppercase()); }
        parts.join(" · ")
    }

    pub fn season_number(&self) -> Option<i32> { self.metadata.as_ref().and_then(|m| m.season).or(self.parsed.season) }
    pub fn episode_number(&self) -> Option<i32> {
        self.metadata.as_ref().and_then(|m| m.episode).or(self.parsed.episode).or(self.parsed.absolute_episode)
    }

    pub fn row_title(&self) -> String {
        if let Some(s) = self.parsed.special_title.as_ref() { return s.clone(); }
        if let Some(e) = self.matched().and_then(|m| m.episode_title.clone()).filter(|s| !s.is_empty()) { return e; }
        self.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    }

    pub fn player_title(&self) -> String {
        if let Some(s) = self.parsed.special_title.as_ref() { return s.clone(); }
        if let Some(m) = self.matched() {
            if self.kind() == Kind::Episode { if let Some(e) = m.episode_title.clone().filter(|s| !s.is_empty()) { return e; } }
            if let Some(s) = m.series_title.clone().filter(|s| !s.is_empty()) { return s; }
        }
        self.parsed.title.clone()
    }

    pub fn player_subtitle(&self) -> String {
        let mut parts = vec![];
        let m = self.metadata.as_ref();
        if self.kind() == Kind::Episode {
            if let Some(s) = self.matched().and_then(|m| m.series_title.clone()) { parts.push(s); }
            if let Some(c) = m.and_then(|m| m.code()) { parts.push(c); }
            else if let Some(a) = self.parsed.absolute_episode { parts.push(format!("Ép. {a:03}")); }
            if let Some(s) = self.parsed.special { parts.push(s.row().into()); }
        } else {
            if let Some(y) = m.and_then(|m| m.year).or(self.parsed.year) { parts.push(y.to_string()); }
            if let Some(g) = m.and_then(|m| m.genres.first().cloned()) { parts.push(g); }
        }
        parts.join(" · ")
    }

    pub fn grouping_title(&self) -> String {
        if self.kind() == Kind::Episode {
            if let Some(s) = self.matched().and_then(|m| m.series_title.clone()).filter(|s| !s.is_empty()) { return s; }
            return self.parsed.title.clone();
        }
        self.display_title()
    }
}

// ── Tuiles ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Season {
    pub id: String,
    pub series_title: String,
    pub season: Option<i32>,
    pub special: Option<Special>,
    pub episodes: Vec<usize>,
}

#[derive(Clone, Debug)]
pub enum Entry { Single(usize), Season(Season) }

pub fn collator(a: &str, b: &str) -> Ordering {
    // Tri « naturel » insensible à la casse et aux accents : « Épisode 10 » après « Épisode 9 ».
    fn key(s: &str) -> Vec<(u8, String, u64)> {
        let folded: String = unicode_normalization::UnicodeNormalization::nfd(s).filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase();
        let mut out = vec![];
        let mut chars = folded.chars().peekable();
        while let Some(c) = chars.next() {
            if c.is_ascii_digit() {
                let mut n = c.to_string();
                while let Some(d) = chars.peek().copied().filter(|d| d.is_ascii_digit()) { n.push(d); chars.next(); }
                out.push((0, String::new(), n.parse().unwrap_or(u64::MAX)));
            } else {
                out.push((1, c.to_string(), 0));
            }
        }
        out
    }
    key(a).cmp(&key(b))
}

fn sort_episodes(items: &[Item], eps: &mut [usize]) {
    eps.sort_by(|&a, &b| {
        let n = |i: usize| { let it = &items[i]; (if it.parsed.special.is_some() { it.parsed.special_number } else { it.episode_number() }).unwrap_or(i32::MAX) };
        n(a).cmp(&n(b)).then_with(|| items[a].filename.cmp(&items[b].filename))
    });
}

/// Regroupe les épisodes par série et saison ; le reste reste seul.
pub fn build(items: &[Item], subset: &[usize]) -> Vec<Entry> {
    let mut singles = vec![];
    let mut order: Vec<String> = vec![];
    let mut buckets: HashMap<String, Season> = HashMap::new();
    for &i in subset {
        let it = &items[i];
        if it.kind() != Kind::Episode { singles.push(Entry::Single(i)); continue; }
        let gt = it.grouping_title();
        let key = match it.parsed.special {
            Some(s) => format!("{gt}|x{}", s.bucket()),
            None => format!("{gt}|s{}", it.season_number().map(|s| s.to_string()).unwrap_or("?".into())),
        };
        buckets.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Season { id: key.clone(), series_title: gt.clone(), season: if it.parsed.special.is_some() { None } else { it.season_number() }, special: it.parsed.special, episodes: vec![] }
        }).episodes.push(i);
    }
    for k in order {
        let mut s = buckets.remove(&k).unwrap();
        sort_episodes(items, &mut s.episodes);
        singles.push(Entry::Season(s));
    }
    singles
}

impl Season {
    pub fn label(&self) -> String {
        if let Some(s) = self.special { return s.bucket().into(); }
        match self.season { Some(n) => format!("Saison {n}"), None => "Épisodes".into() }
    }
    pub fn subtitle(&self) -> String {
        let n = self.episodes.len();
        let pl = if n > 1 { "s" } else { "" };
        if self.special.is_some() { format!("{} · {n} titre{pl}", self.label()) } else { format!("{} · {n} épisode{pl}", self.label()) }
    }
    fn first<T>(&self, items: &[Item], f: impl Fn(&Metadata) -> Option<T>) -> Option<T> {
        self.episodes.iter().find_map(|&i| items[i].metadata.as_ref().and_then(&f))
    }
    pub fn poster(&self, items: &[Item]) -> Option<String> { self.first(items, |m| m.poster_url.clone()) }
    pub fn year(&self, items: &[Item]) -> Option<i32> { self.first(items, |m| m.year) }
    pub fn genres(&self, items: &[Item]) -> Vec<String> { self.first(items, |m| if m.genres.is_empty() { None } else { Some(m.genres.clone()) }).unwrap_or_default() }
    pub fn overview(&self, items: &[Item]) -> Option<String> { self.first(items, |m| m.overview.clone().filter(|s| !s.is_empty())) }
    pub fn identified(&self, items: &[Item]) -> bool { self.episodes.iter().any(|&i| items[i].matched().is_some()) }
    pub fn total_size(&self, items: &[Item]) -> u64 { self.episodes.iter().map(|&i| items[i].size).sum() }
    pub fn latest(&self, items: &[Item]) -> i64 { self.episodes.iter().map(|&i| items[i].modified).max().unwrap_or(0) }
}

impl Entry {
    pub fn key(&self, items: &[Item]) -> String {
        match self { Entry::Single(i) => format!("f:{}", items[*i].id), Entry::Season(s) => format!("s:{}", s.id) }
    }
    pub fn title(&self, items: &[Item]) -> String {
        match self { Entry::Single(i) => items[*i].grouping_title(), Entry::Season(s) => s.series_title.clone() }
    }
    pub fn subtitle(&self, items: &[Item]) -> String {
        match self { Entry::Single(i) => items[*i].display_subtitle(), Entry::Season(s) => s.subtitle() }
    }
    pub fn poster(&self, items: &[Item]) -> Option<String> {
        match self { Entry::Single(i) => items[*i].metadata.as_ref().and_then(|m| m.poster_url.clone()), Entry::Season(s) => s.poster(items) }
    }
    pub fn kind(&self, items: &[Item]) -> Kind { match self { Entry::Single(i) => items[*i].kind(), Entry::Season(_) => Kind::Episode } }
    pub fn season_rank(&self) -> i32 {
        match self {
            Entry::Single(_) => 0,
            Entry::Season(s) => match s.special { None => s.season.unwrap_or(9000), Some(Special::Extra) => 10001, Some(_) => 10000 },
        }
    }
    pub fn latest(&self, items: &[Item]) -> i64 { match self { Entry::Single(i) => items[*i].modified, Entry::Season(s) => s.latest(items) } }
    pub fn size(&self, items: &[Item]) -> u64 { match self { Entry::Single(i) => items[*i].size, Entry::Season(s) => s.total_size(items) } }
}

// ── Reprise et épisodes vus ───────────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub position: f64,
    pub duration: f64,
    pub watched: bool,
    pub last_played: i64,
}

/// Au-delà, l'épisode est vu : le reste, c'est le générique.
pub const COMPLETION: f64 = 0.92;
/// En dessous, « reprendre » voudrait dire « recommencer ».
pub const MIN_RESUME: f64 = 5.0;

impl Record {
    pub fn progress(&self) -> f64 { if self.duration > 0.0 { (self.position / self.duration).clamp(0.0, 1.0) } else { 0.0 } }
    pub fn resumable(&self) -> bool { self.position > MIN_RESUME && self.progress() < COMPLETION }
}

pub fn format_time(sec: f64) -> String {
    let t = sec.max(0.0) as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub struct Watch<'a> { pub records: &'a HashMap<String, Record>, pub items: &'a [Item] }

impl Watch<'_> {
    pub fn rec(&self, i: usize) -> Option<&Record> { self.records.get(&self.items[i].id) }
    pub fn watched(&self, i: usize) -> bool { self.rec(i).map(|r| r.watched).unwrap_or(false) }
    pub fn progress(&self, i: usize) -> f64 { self.rec(i).map(|r| r.progress()).unwrap_or(0.0) }
    pub fn resume(&self, i: usize) -> Option<f64> { self.rec(i).filter(|r| r.resumable()).map(|r| r.position) }
    pub fn watched_count(&self, s: &Season) -> usize { s.episodes.iter().filter(|&&i| self.watched(i)).count() }
    pub fn season_progress(&self, s: &Season) -> f64 {
        if s.episodes.is_empty() { return 0.0; }
        s.episodes.iter().map(|&i| if self.watched(i) { 1.0 } else { self.progress(i) }).sum::<f64>() / s.episodes.len() as f64
    }

    /// L'épisode à proposer : celui joué en dernier s'il est en cours, sinon le suivant non vu.
    pub fn next_episode(&self, s: &Season) -> Option<usize> {
        let eps = &s.episodes;
        let last = eps.iter().enumerate().filter_map(|(k, &i)| self.rec(i).map(|r| (k, r.last_played))).max_by_key(|(_, d)| *d).map(|(k, _)| k);
        if let Some(k) = last {
            if self.resume(eps[k]).is_some() { return Some(eps[k]); }
            if let Some(&n) = eps[k..].iter().find(|&&i| !self.watched(i)) { return Some(n); }
        }
        eps.iter().copied().find(|&i| !self.watched(i))
    }
    pub fn last_played(&self, s: &Season) -> Option<i64> { s.episodes.iter().filter_map(|&i| self.rec(i).map(|r| r.last_played)).max() }
    pub fn season_in_progress(&self, s: &Season) -> bool { self.last_played(s).is_some() && self.next_episode(s).is_some() }

    pub fn entry_progress(&self, e: &Entry) -> f64 {
        match e { Entry::Single(i) => if self.watched(*i) { 1.0 } else { self.progress(*i) }, Entry::Season(s) => self.season_progress(s) }
    }
    pub fn entry_watched(&self, e: &Entry) -> bool {
        match e { Entry::Single(i) => self.watched(*i), Entry::Season(s) => !s.episodes.is_empty() && self.watched_count(s) == s.episodes.len() }
    }
}

// ── Sections, recherche, tri ──────────────────────────────────────────────

pub const SECTIONS: &[(&str, &str, &str)] = &[
    ("all", "Tout", "grid"), ("continueWatching", "Reprendre", "playCircle"), ("movies", "Films", "film"), ("shows", "Séries", "tv"),
    ("audio", "Audio", "music"), ("recent", "Ajouts récents", "clock"), ("unmatched", "Non identifiés", "question"),
];
pub const SORTS: &[(&str, &str)] = &[("title", "Titre"), ("dateAdded", "Date d'ajout"), ("size", "Taille"), ("kind", "Type")];

pub fn section_title(sel: &str) -> String {
    if let Some(s) = sel.strip_prefix("series:") { return s.into(); }
    SECTIONS.iter().find(|s| s.0 == sel).map(|s| s.1).unwrap_or("Tout").into()
}

pub fn series_names(items: &[Item]) -> Vec<String> {
    let mut v: Vec<String> = items.iter().filter(|i| i.kind() == Kind::Episode).map(|i| i.grouping_title()).collect();
    v.sort_by(|a, b| collator(a, b));
    v.dedup();
    v
}

fn contains(hay: &str, q: &str) -> bool { hay.to_lowercase().contains(&q.to_lowercase()) }

/// Les tuiles « Reprendre », la plus récente d'abord, avec la saison entière.
pub fn continue_entries(items: &[Item], w: &Watch) -> Vec<Entry> {
    let all: Vec<usize> = (0..items.len()).collect();
    let mut dated: Vec<(Entry, i64)> = vec![];
    for e in build(items, &all) {
        match &e {
            Entry::Single(i) => if let (Some(_), Some(r)) = (w.resume(*i), w.rec(*i)) { let d = r.last_played; dated.push((e, d)); },
            Entry::Season(s) => if w.season_in_progress(s) { let d = w.last_played(s).unwrap_or(0); dated.push((e, d)); },
        }
    }
    dated.sort_by(|a, b| b.1.cmp(&a.1));
    dated.into_iter().map(|x| x.0).collect()
}

pub struct Counts { pub all: usize, pub movies: usize, pub shows: usize, pub audio: usize, pub unmatched: usize, pub resume: usize }

pub fn counts(items: &[Item], w: &Watch) -> Counts {
    Counts {
        all: items.len(),
        movies: items.iter().filter(|i| i.kind() == Kind::Movie).count(),
        shows: items.iter().filter(|i| i.kind() == Kind::Episode).count(),
        audio: items.iter().filter(|i| i.kind() == Kind::Audio).count(),
        unmatched: items.iter().filter(|i| i.matched().is_none()).count(),
        resume: continue_entries(items, w).len(),
    }
}

pub fn visible_items(items: &[Item], sel: &str, search: &str) -> Vec<usize> {
    let all = 0..items.len();
    let mut r: Vec<usize> = match sel {
        "movies" => all.filter(|&i| items[i].kind() == Kind::Movie).collect(),
        "shows" => all.filter(|&i| items[i].kind() == Kind::Episode).collect(),
        "audio" => all.filter(|&i| items[i].kind() == Kind::Audio).collect(),
        "unmatched" => all.filter(|&i| items[i].matched().is_none()).collect(),
        "recent" => { let mut v: Vec<usize> = all.collect(); v.sort_by(|&a, &b| items[b].modified.cmp(&items[a].modified)); v.truncate(60); return v; }
        s if s.starts_with("series:") => { let n = &s[7..]; return all.filter(|&i| items[i].kind() == Kind::Episode && items[i].grouping_title() == n).collect(); }
        _ => all.collect(),
    };
    let q = search.trim();
    if !q.is_empty() {
        r.retain(|&i| { let it = &items[i]; contains(&it.display_title(), q) || contains(&it.grouping_title(), q) || contains(&it.filename, q) });
    }
    r
}

pub fn visible_entries(items: &[Item], sel: &str, search: &str, sort: &str, w: &Watch) -> Vec<Entry> {
    if sel == "continueWatching" {
        let q = search.trim();
        return continue_entries(items, w).into_iter().filter(|e| q.is_empty() || contains(&e.title(items), q)).collect();
    }
    let mut entries = build(items, &visible_items(items, sel, search));
    if sel == "recent" { entries.sort_by(|a, b| b.latest(items).cmp(&a.latest(items))); return entries; }
    let by_title = |a: &Entry, b: &Entry| collator(&a.title(items), &b.title(items)).then(a.season_rank().cmp(&b.season_rank()));
    match sort {
        "dateAdded" => entries.sort_by(|a, b| b.latest(items).cmp(&a.latest(items))),
        "size" => entries.sort_by(|a, b| b.size(items).cmp(&a.size(items))),
        "kind" => entries.sort_by(|a, b| a.kind(items).rank().cmp(&b.kind(items).rank()).then_with(|| by_title(a, b))),
        _ => entries.sort_by(by_title),
    }
    entries
}

/// La saison entière qui contient cet épisode, dans l'ordre de lecture.
pub fn season_of(items: &[Item], i: usize) -> Option<Season> {
    if items[i].kind() != Kind::Episode { return None; }
    let all: Vec<usize> = (0..items.len()).collect();
    build(items, &all).into_iter().find_map(|e| match e { Entry::Season(s) if s.episodes.contains(&i) => Some(s), _ => None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use std::path::Path;

    fn item(name: &str) -> Item {
        let p = PathBuf::from(format!("/media/{name}"));
        let hint = if name.contains('/') { name.split('/').next() } else { None };
        Item { id: p.to_string_lossy().into(), filename: p.file_name().unwrap().to_string_lossy().into(), ext: "mkv".into(),
            parsed: parse(Path::new(&p), hint), path: p, size: 1, modified: 0, metadata: None }
    }
    fn rec(pos: f64, watched: bool, at: i64) -> Record { Record { position: pos, duration: 100.0, watched, last_played: at } }

    #[test]
    fn seasons_and_specials() {
        let items = vec![item("Show.S01E02.mkv"), item("Show.S01E01.mkv"), item("Show.S02E01.mkv"), item("Show - OAV - 01 - Bonus Film.mkv"), item("Film.2010.mkv")];
        let all: Vec<usize> = (0..items.len()).collect();
        let e = build(&items, &all);
        let seasons: Vec<&Season> = e.iter().filter_map(|x| if let Entry::Season(s) = x { Some(s) } else { None }).collect();
        assert_eq!(seasons.len(), 3);
        let s1 = seasons.iter().find(|s| s.season == Some(1)).unwrap();
        assert_eq!(s1.episodes.iter().map(|&i| items[i].parsed.episode.unwrap()).collect::<Vec<_>>(), vec![1, 2]);
        assert!(seasons.iter().any(|s| s.special == Some(Special::Oav) && s.label() == "Films et hors-séries"));
    }

    #[test]
    fn resume_window() {
        assert!(!Record { position: 4.0, duration: 100.0, ..Default::default() }.resumable());
        assert!(Record { position: 50.0, duration: 100.0, ..Default::default() }.resumable());
        assert!(!Record { position: 95.0, duration: 100.0, ..Default::default() }.resumable());
    }

    #[test]
    fn next_episode() {
        let items = vec![item("Show.S01E01.mkv"), item("Show.S01E02.mkv"), item("Show.S01E03.mkv"), item("Show.S01E04.mkv")];
        let s = match &build(&items, &[0, 1, 2, 3])[0] { Entry::Season(s) => s.clone(), _ => panic!() };
        let mut r = HashMap::new();
        r.insert(items[0].id.clone(), rec(90.0, false, 1));
        r.insert(items[1].id.clone(), rec(100.0, true, 2));
        r.insert(items[2].id.clone(), rec(30.0, false, 3));
        assert_eq!(Watch { records: &r, items: &items }.next_episode(&s), Some(2));
        let mut r2 = HashMap::new();
        r2.insert(items[1].id.clone(), rec(100.0, true, 2));
        let w = Watch { records: &r2, items: &items };
        assert_eq!(w.next_episode(&s), Some(2));
        assert_eq!(w.watched_count(&s), 1);
        assert!(w.season_in_progress(&s));
    }

    #[test]
    fn natural_sort() {
        let mut v = vec!["Épisode 10", "episode 9", "Alpha"];
        v.sort_by(|a, b| collator(a, b));
        assert_eq!(v, vec!["Alpha", "episode 9", "Épisode 10"]);
    }
}
