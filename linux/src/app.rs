//! L'état de Biblifilm et ce qui tourne en fond : analyse du dossier,
//! identification en ligne, lecture. L'affichage est dans `views.rs` et
//! `player_ui.rs`.

use crate::common::images::ImageCache;
use crate::common::selfupdate::SelfUpdate;
use crate::common::store::{self, Settings};
use crate::common::sys;
use crate::common::theme::{self, Palette};
use crate::common::ui::Toast;
use crate::library::{self, ScanOptions, WatchStore};
use crate::metadata::{Candidate, Service, Tmdb};
use crate::model::{self, Entry, Item, Metadata, Watch};
use crate::mpv;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

pub const REPO: &str = "VGioM/Biblifilm";

pub enum Bg {
    Scanned(u64, Vec<Item>, bool),
    Meta(u64, String, Metadata),
    MetaDone(u64),
    Candidates(Vec<Candidate>),
    KeyTest(Option<String>),
}

/// Une séance de lecture dans la fenêtre.
pub struct Session {
    pub player: mpv::Player,
    pub queue: Vec<String>,
    pub titles: Vec<(String, String)>,
    pub last_saved: f64,
    pub last_save_at: Instant,
    pub last_activity: Instant,
    pub started: Instant,
    pub osd: Option<(String, Instant)>,
    pub error: Option<String>,
    pub closing: bool,
    pub applying_tracks: bool,
}

#[derive(PartialEq, Clone, Copy)]
pub enum SettingsTab { General, Playback, Metadata }

pub struct Correction {
    pub item: String,
    pub query: String,
    pub results: Option<Vec<Candidate>>,
    pub searching: bool,
}

pub struct App {
    pub p: Palette,
    pub dir: PathBuf,
    pub cache: PathBuf,
    pub settings: Settings,
    pub images: ImageCache,
    pub meta: Arc<Service>,
    pub watch: WatchStore,
    pub items: Vec<Item>,

    pub search: String,
    pub focus_search: bool,
    pub opened_season: Option<String>,
    pub selected: Option<String>,
    pub banner_dismissed: bool,

    pub scanning: bool,
    pub scan_progress: Arc<Mutex<String>>,
    pub fetching: bool,
    pub fetch_total: usize,
    pub fetch_done: usize,
    run: Arc<AtomicU64>,
    scan_run: u64,
    pub tx: mpsc::Sender<Bg>,
    rx: mpsc::Receiver<Bg>,
    watcher: Option<notify::RecommendedWatcher>,
    changed: Arc<AtomicBool>,
    changed_at: Option<Instant>,
    key_changed_at: Option<Instant>,
    last_key: String,

    pub session: Option<Session>,
    pub inhibit: Option<sys::Inhibit>,
    pub mpv_error: Option<String>,

    pub error: Option<String>,
    pub toast: Toast,
    pub settings_open: Option<SettingsTab>,
    pub correction: Option<Correction>,
    pub key_status: Option<Result<(), String>>,
    pub restore_msg: Option<String>,
    pub pending_folder: Option<mpsc::Receiver<Vec<PathBuf>>>,
    pub update: SelfUpdate,
    pub fullscreen: bool,
    pub test: crate::common::testhook::TestHook,
    test_done: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let p = theme::MUSIC;
        theme::install(&cc.egui_ctx, &p);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let dir = store::support_dir("Biblifilm", Some("BIBLIFILM_SUPPORT_DIR"));
        let cache = store::cache_dir("Biblifilm").join("Artwork");
        let settings = Settings::new(dir.join("settings.json"), json!({
            "tmdbAPIKey": "", "libraryPath": null, "preferredLanguage": "fr-FR", "includeAudio": true, "skipSmallFiles": true,
            "autoFetchMetadata": true, "watchFolder": true, "lastSelection": "all", "sortOrder": "title", "inspectorVisible": true,
            "seriesExpanded": true, "trackPrefs": {}, "volume": 100.0
        }));
        let (tx, rx) = mpsc::channel();
        let last_key = settings.string("tmdbAPIKey").trim().to_string();
        let mut app = App {
            p, images: ImageCache::new(&cc.egui_ctx, cache.clone(), 6), meta: Arc::new(Service::new(dir.join("metadata-cache.json"))),
            watch: WatchStore::load(&dir), dir, cache, settings, items: vec![],
            search: String::new(), focus_search: false, opened_season: None, selected: None, banner_dismissed: false,
            scanning: false, scan_progress: Arc::default(), fetching: false, fetch_total: 0, fetch_done: 0, run: Arc::default(), scan_run: 0,
            tx, rx, watcher: None, changed: Arc::default(), changed_at: None, key_changed_at: None, last_key,
            session: None, inhibit: None, mpv_error: { mpv::preload(); None },
            error: None, toast: Toast::default(), settings_open: None, correction: None, key_status: None, restore_msg: None,
            pending_folder: None, update: SelfUpdate::new(REPO, "Biblifilm"), fullscreen: false,
            test: Default::default(), test_done: false,
        };
        app.rescan(&cc.egui_ctx, false);
        app
    }

    pub fn library_path(&self) -> Option<PathBuf> { self.settings.get("libraryPath").as_str().filter(|s| !s.is_empty()).map(PathBuf::from) }
    pub fn tmdb_key(&self) -> Option<String> { let k = self.settings.string("tmdbAPIKey").trim().to_string(); if k.is_empty() { None } else { Some(k) } }
    pub fn lang(&self) -> String { self.settings.string("preferredLanguage") }
    pub fn sel(&self) -> String { self.settings.string("lastSelection") }
    pub fn w(&self) -> Watch<'_> { Watch { records: &self.watch.records, items: &self.items } }
    pub fn index_of(&self, id: &str) -> Option<usize> { self.items.iter().position(|i| i.id == id) }

    // ── Analyse ───────────────────────────────────────────────────────────

    /// `incremental` : garder les fiches déjà résolues (nouveau fichier détecté).
    pub fn rescan(&mut self, ctx: &egui::Context, incremental: bool) {
        let Some(root) = self.library_path() else { return };
        self.scan_run += 1;
        let run = self.scan_run;
        if !incremental { self.scanning = true; self.run.fetch_add(1, Ordering::SeqCst); self.fetching = false; }
        *self.scan_progress.lock().unwrap() = "Analyse…".into();
        let opts = ScanOptions { include_audio: self.settings.bool("includeAudio"), skip_small: self.settings.bool("skipSmallFiles") };
        let (tx, ctx, progress) = (self.tx.clone(), ctx.clone(), self.scan_progress.clone());
        std::thread::spawn(move || {
            let c2 = ctx.clone();
            let items = library::scan(&root, &opts, &move |n| { *progress.lock().unwrap() = format!("{n} fichiers trouvés…"); c2.request_repaint(); });
            let _ = tx.send(Bg::Scanned(run, items, incremental));
            ctx.request_repaint();
        });
    }

    fn apply_cached(&mut self) {
        let lang = self.lang();
        for it in self.items.iter_mut() {
            if it.metadata.is_none() { it.metadata = self.meta.cached(&it.parsed, &lang); }
        }
    }

    fn start_watching(&mut self, ctx: &egui::Context) {
        self.watcher = None;
        if !self.settings.bool("watchFolder") { return; }
        if let Some(root) = self.library_path() { self.watcher = library::watch(&root, ctx.clone(), self.changed.clone()); }
    }

    // ── Métadonnées ───────────────────────────────────────────────────────

    /// Cherche les fiches manquantes, quatre à la fois.
    pub fn fetch_metadata(&mut self, ctx: &egui::Context, force: bool) {
        let run = self.run.fetch_add(1, Ordering::SeqCst) + 1;
        if force { self.meta.clear(); for it in self.items.iter_mut() { it.metadata = None; } }
        let targets: Vec<(String, crate::parser::Parsed)> = self.items.iter().filter(|i| i.metadata.is_none()).map(|i| (i.id.clone(), i.parsed.clone())).collect();
        if targets.is_empty() { self.fetching = false; return; }
        self.fetching = true; self.fetch_total = targets.len(); self.fetch_done = 0;
        let queue = Arc::new(Mutex::new(targets));
        let (key, lang) = (self.tmdb_key(), self.lang());
        let workers = Arc::new(AtomicU64::new(4));
        for _ in 0..4 {
            let (queue, tx, ctx, meta, key, lang, runc, workers) = (queue.clone(), self.tx.clone(), ctx.clone(), self.meta.clone(), key.clone(), lang.clone(), self.run.clone(), workers.clone());
            std::thread::spawn(move || {
                let mut n = 0;
                loop {
                    if runc.load(Ordering::SeqCst) != run { break; }
                    let Some((id, p)) = queue.lock().unwrap().pop() else { break };
                    let m = meta.lookup(&p, key.as_deref(), &lang);
                    let _ = tx.send(Bg::Meta(run, id, m));
                    ctx.request_repaint();
                    n += 1;
                    // Enregistré au fil de l'eau : quitter pendant la première analyse ne perd pas le travail fait.
                    if n % 25 == 0 { meta.flush(); }
                }
                meta.flush();
                if workers.fetch_sub(1, Ordering::SeqCst) == 1 { let _ = tx.send(Bg::MetaDone(run)); ctx.request_repaint(); }
            });
        }
    }

    fn retry_unmatched(&mut self, ctx: &egui::Context) {
        self.meta.forget_unmatched();
        for it in self.items.iter_mut() { if it.matched().is_none() { it.metadata = None; } }
        self.fetch_metadata(ctx, false);
    }

    fn search_name(&self, i: usize, over: Option<&str>) -> crate::parser::Parsed {
        let mut p = self.items[i].parsed.clone();
        if let Some(o) = over.filter(|o| !o.trim().is_empty()) {
            let (t, y) = crate::parser::normalize_user_title(o);
            p.title = t;
            if y.is_some() { p.year = y; }
        }
        p
    }

    /// Relance la recherche pour un fichier, sous son nom ou sous le titre tapé.
    pub fn refetch(&mut self, ctx: &egui::Context, id: &str, over: Option<String>) {
        let Some(i) = self.index_of(id) else { return };
        let q = self.search_name(i, over.as_deref());
        let original = self.items[i].parsed.clone();
        self.items[i].metadata = None;
        let (meta, key, lang, tx, ctx, id) = (self.meta.clone(), self.tmdb_key(), self.lang(), self.tx.clone(), ctx.clone(), id.to_string());
        let run = self.run.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            // Repartir de zéro, sinon la réponse en cache (peut-être fausse) revient tout droit.
            meta.forget(&original, &lang);
            meta.forget(&q, &lang);
            let m = meta.lookup(&q, key.as_deref(), &lang);
            // Rangé sous la clé du fichier, pas du titre tapé, pour survivre au relancement.
            meta.store(m.clone(), &original, &lang);
            let _ = tx.send(Bg::Meta(run, id, m));
            ctx.request_repaint();
        });
    }

    pub fn search_candidates(&mut self, ctx: &egui::Context) {
        let Some((item, query)) = self.correction.as_ref().map(|c| (c.item.clone(), c.query.clone())) else { return };
        let Some(i) = self.index_of(&item) else { return };
        let key = self.settings.string("tmdbAPIKey").trim().to_string();
        if key.is_empty() { if let Some(c) = &mut self.correction { c.results = Some(vec![]); } return; }
        let q = self.search_name(i, Some(&query));
        if let Some(c) = &mut self.correction { c.searching = true; }
        let (tx, ctx, lang) = (self.tx.clone(), ctx.clone(), self.lang());
        std::thread::spawn(move || { let _ = tx.send(Bg::Candidates(Tmdb::new(&key).candidates(&q, &lang))); ctx.request_repaint(); });
    }

    /// Le choix de l'utilisateur, appliqué à tous les épisodes de la même série.
    pub fn apply_candidate(&mut self, ctx: &egui::Context, id: &str, cand: Candidate, over: String) {
        let Some(i) = self.index_of(id) else { return };
        let Some(key) = self.tmdb_key() else { return };
        let q = self.search_name(i, Some(&over));
        let show = self.items[i].parsed.show_key();
        let targets: Vec<(String, crate::parser::Parsed)> = self.items.iter()
            .filter(|x| x.id == id || (self.items[i].kind() == crate::parser::Kind::Episode && x.parsed.show_key() == show))
            .map(|x| (x.id.clone(), x.parsed.clone())).collect();
        let (meta, tx, ctx, lang) = (self.meta.clone(), self.tx.clone(), ctx.clone(), self.lang());
        let run = self.run.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            let t = Tmdb::new(&key);
            for (tid, p) in targets {
                let mut ask = p.clone();
                ask.title = q.title.clone();
                if q.year.is_some() { ask.year = q.year; }
                if let Some(m) = t.for_candidate(&cand, &ask, &lang) {
                    meta.store(m.clone(), &p, &lang);
                    let _ = tx.send(Bg::Meta(run, tid, m));
                    ctx.request_repaint();
                }
            }
        });
    }

    pub fn test_key(&mut self, ctx: &egui::Context) {
        let (key, tx, ctx) = (self.settings.string("tmdbAPIKey"), self.tx.clone(), ctx.clone());
        self.key_status = None;
        std::thread::spawn(move || { let _ = tx.send(Bg::KeyTest(Tmdb::validate(&key))); ctx.request_repaint(); });
    }

    pub fn set_setting(&mut self, ctx: &egui::Context, key: &str, value: serde_json::Value) {
        let before = self.settings.get(key);
        self.settings.set(key, value.clone());
        match key {
            "watchFolder" => self.start_watching(ctx),
            "includeAudio" | "skipSmallFiles" if before != value => self.rescan(ctx, false),
            "preferredLanguage" if before != value => { for it in self.items.iter_mut() { it.metadata = None; } self.apply_cached(); self.fetch_metadata(ctx, false); }
            // Coller une clé relance tout ce qui avait échoué sans elle, une fois la clé entièrement tapée.
            "tmdbAPIKey" => { self.key_changed_at = Some(Instant::now()); self.key_status = None; }
            _ => {}
        }
    }

    pub fn choose_folder(&mut self) {
        let start = self.library_path().or_else(dirs::video_dir);
        self.pending_folder = Some(sys::pick(sys::Pick::Folder, "Choisissez le dossier contenant vos films et séries", start.as_deref(), &[]));
    }

    // ── Événements de fond ────────────────────────────────────────────────

    pub fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Bg::Scanned(run, mut items, incremental) => {
                    if run != self.scan_run { continue; }
                    let prev: std::collections::HashMap<String, Option<Metadata>> = self.items.drain(..).map(|i| (i.id, i.metadata)).collect();
                    let mut added = 0;
                    for it in items.iter_mut() { match prev.get(&it.id) { Some(m) => it.metadata = m.clone(), None => added += 1 } }
                    let changed = !incremental || added > 0 || items.len() != prev.len();
                    self.items = items;
                    self.scanning = false;
                    if changed {
                        self.apply_cached();
                        if self.settings.bool("autoFetchMetadata") { self.fetch_metadata(ctx, false); }
                    }
                    if !incremental { self.start_watching(ctx); self.drop_stale_selection(); }
                }
                Bg::Meta(run, id, m) => {
                    let _ = run;
                    if let Some(i) = self.index_of(&id) { self.items[i].metadata = Some(m); }
                    self.fetch_done += 1;
                }
                Bg::MetaDone(run) => { if run == self.run.load(Ordering::SeqCst) { self.fetching = false; } }
                Bg::Candidates(c) => { if let Some(x) = &mut self.correction { x.results = Some(c); x.searching = false; } }
                Bg::KeyTest(r) => {
                    let ok = r.is_none();
                    self.key_status = Some(r.map_or(Ok(()), Err));
                    // Une clé qui marche : tout ce qui avait échoué est cherché à nouveau.
                    if ok { self.fetch_metadata(ctx, true); }
                }
            }
        }
        // Les événements du dossier, regroupés sur 4 secondes.
        if self.changed.swap(false, Ordering::Relaxed) { self.changed_at = Some(Instant::now()); }
        if let Some(t) = self.changed_at {
            if t.elapsed().as_secs() >= 4 && !self.scanning { self.changed_at = None; self.rescan(ctx, true); }
            else { ctx.request_repaint_after(std::time::Duration::from_millis(500)); }
        }
        if let Some(t) = self.key_changed_at {
            if t.elapsed().as_secs() >= 2 {
                self.key_changed_at = None;
                let k = self.settings.string("tmdbAPIKey").trim().to_string();
                if k != self.last_key && k.len() >= 20 { self.last_key = k; self.retry_unmatched(ctx); }
            } else { ctx.request_repaint_after(std::time::Duration::from_millis(300)); }
        }
        if let Some(rx) = &self.pending_folder {
            if let Ok(v) = rx.try_recv() {
                self.pending_folder = None;
                if let Some(p) = v.into_iter().next() {
                    self.settings.set("libraryPath", p.to_string_lossy().to_string());
                    self.items.clear();
                    self.rescan(ctx, false);
                }
            } else { ctx.request_repaint_after(std::time::Duration::from_millis(250)); }
        }
        if self.fetching { ctx.request_repaint_after(std::time::Duration::from_millis(400)); }
        self.watch.tick();
    }

    /// L'onglet mémorisé n'a peut-être plus de rangée : une série partie, rien à reprendre.
    fn drop_stale_selection(&mut self) {
        let sel = self.sel();
        let stale = if let Some(n) = sel.strip_prefix("series:") { !model::series_names(&self.items).iter().any(|s| s == n) }
            else { (sel == "audio" && !self.settings.bool("includeAudio")) || (sel == "continueWatching" && model::continue_entries(&self.items, &self.w()).is_empty()) };
        if stale { self.settings.set("lastSelection", "all"); }
    }

    // ── Lecture ───────────────────────────────────────────────────────────

    /// Lit un fichier ; un épisode met en file la suite de sa saison.
    pub fn play(&mut self, ctx: &egui::Context, id: &str, from_start: bool, followers: Option<Vec<usize>>) {
        let Some(i) = self.index_of(id) else { return };
        if !self.items[i].path.exists() { self.error = Some(format!("Le fichier est introuvable :\n{}", self.items[i].path.display())); return; }
        let rest = followers.unwrap_or_else(|| model::season_of(&self.items, i).map(|s| {
            let k = s.episodes.iter().position(|&x| x == i).unwrap_or(0);
            s.episodes[k + 1..].to_vec()
        }).unwrap_or_default());
        let start = if from_start { 0.0 } else { self.w().resume(i).unwrap_or(0.0) };
        let queue: Vec<usize> = std::iter::once(i).chain(rest).collect();
        let files: Vec<String> = queue.iter().map(|&k| self.items[k].path.to_string_lossy().to_string()).collect();
        self.stop_playback();
        match mpv::Player::new(&files, start, ctx, self.settings.f64("volume").max(0.0), "fr,fre,fra,en,eng") {
            Ok(player) => {
                self.session = Some(Session {
                    player, queue: queue.iter().map(|&k| self.items[k].id.clone()).collect(),
                    titles: queue.iter().map(|&k| (self.items[k].player_title(), self.items[k].player_subtitle())).collect(),
                    last_saved: -1.0, last_save_at: Instant::now(), last_activity: Instant::now(), started: Instant::now(),
                    osd: None, error: None, closing: false, applying_tracks: false,
                });
            }
            Err(e) => { self.mpv_error = Some(e.clone()); self.error = Some(format!("Le lecteur n'a pas pu démarrer : {e}")); }
        }
    }

    pub fn play_entry(&mut self, ctx: &egui::Context, e: &Entry) {
        match e {
            Entry::Single(i) => { let id = self.items[*i].id.clone(); self.play(ctx, &id, false, None) }
            Entry::Season(s) => {
                let start = self.w().next_episode(s).or(s.episodes.first().copied());
                if let Some(i) = start { let id = self.items[i].id.clone(); self.play(ctx, &id, false, None) }
            }
        }
    }

    pub fn play_all(&mut self, ctx: &egui::Context, ids: Vec<usize>) {
        if let Some((&first, rest)) = ids.split_first() { let id = self.items[first].id.clone(); self.play(ctx, &id, false, Some(rest.to_vec())); }
    }

    /// Écrit la position, seulement si elle a bougé et après 3 s vraiment jouées.
    pub fn save_position(&mut self, routine: bool) {
        let Some(s) = &mut self.session else { return };
        let pl = &s.player;
        if pl.played <= 3.0 || pl.duration <= 0.0 { return; }
        let pos = (pl.position * 10.0).round() / 10.0;
        if routine && (pos == s.last_saved || s.last_save_at.elapsed().as_secs_f32() < 2.0) { return; }
        s.last_saved = pos;
        s.last_save_at = Instant::now();
        let id = s.queue[pl.index.min(s.queue.len() - 1)].clone();
        let (p, d) = (pl.position, pl.duration);
        self.watch.update(&id, p, d);
    }

    pub fn stop_playback(&mut self) {
        self.save_position(false);
        if let Some(s) = &mut self.session { s.player.close(); s.closing = true; }
        self.inhibit = None;
        self.watch.save_now();
    }

    /// Les pistes mémorisées (par libellé : mpv renumérote à chaque fichier), réappliquées.
    pub fn apply_track_prefs(&mut self) {
        let prefs = self.settings.get("trackPrefs");
        let Some(s) = &mut self.session else { return };
        s.applying_tracks = true;
        for kind in ["audio", "sub"] {
            let want = &prefs[kind];
            if want.is_null() { continue; }
            if want.as_str() == Some("none") { if kind == "sub" { s.player.select_track("sub", None); } continue; }
            let (t, l) = (want["title"].as_str().unwrap_or(""), want["lang"].as_str().unwrap_or(""));
            let list: Vec<&mpv::Track> = s.player.tracks.iter().filter(|x| x.kind == kind).collect();
            let hit = list.iter().find(|x| x.title == t && x.lang == l)
                .or_else(|| if t.is_empty() { None } else { list.iter().find(|x| x.title == t) })
                .or_else(|| if l.is_empty() { None } else { list.iter().find(|x| x.lang == l) });
            if let Some(h) = hit { s.player.select_track(kind, Some(h.id)); }
        }
        s.player.refresh_tracks();
        s.applying_tracks = false;
    }

    /// Un choix de piste fait par l'utilisateur, retenu pour la suite.
    pub fn choose_track(&mut self, kind: &str, track: Option<mpv::Track>) {
        let Some(s) = &mut self.session else { return };
        s.player.select_track(kind, track.as_ref().map(|t| t.id));
        s.player.refresh_tracks();
        let mut prefs = self.settings.get("trackPrefs");
        if !prefs.is_object() { prefs = json!({}); }
        prefs[kind] = match &track { Some(t) => json!({ "title": t.title, "lang": t.lang }), None => json!("none") };
        self.settings.set("trackPrefs", prefs);
        let label = track.map(|t| t.label()).unwrap_or_else(|| "aucun".into());
        if let Some(s) = &mut self.session { s.osd = Some((format!("{} : {label}", if kind == "audio" { "Audio" } else { "Sous-titres" }), Instant::now())); }
    }

    /// Le suivi de la séance, à chaque image.
    pub fn tick_player(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.session else { return };
        if s.closing {
            if s.player.can_drop() { self.session = None; ctx.request_repaint(); } else { ctx.request_repaint(); }
            return;
        }
        let events = s.player.poll();
        for ev in events {
            match ev {
                mpv::Event::Loaded(_) => { self.apply_track_prefs(); }
                mpv::Event::Eof(k) => {
                    // Fin réelle du fichier : l'épisode est vu.
                    if let Some(s) = &self.session {
                        if s.player.played > 3.0 && s.player.duration > 0.0 {
                            let (id, d) = (s.queue[k.min(s.queue.len() - 1)].clone(), s.player.duration);
                            self.watch.update(&id, d, d);
                        }
                    }
                }
                mpv::Event::Error(_, e) => { if let Some(s) = &mut self.session { s.error = Some(e); } }
                mpv::Event::Finished => { self.stop_playback(); return; }
            }
        }
        let Some(s) = &mut self.session else { return };
        // Un fichier qui ne démarre pas au bout de 15 s : une erreur et une porte de sortie,
        // plutôt qu'une « fin » instantanée qui marquerait toute la saison comme vue.
        if !s.player.loaded && s.player.duration <= 0.0 && s.started.elapsed().as_secs() >= 15 && s.error.is_none() {
            s.error = Some("le fichier ne démarre pas".into());
        }
        let playing = !s.player.paused && s.player.loaded;
        // L'écran reste allumé tant qu'un film est vraiment en lecture, pas en pause.
        if playing && self.inhibit.is_none() { self.inhibit = Some(sys::Inhibit::new("Biblifilm", "Lecture d'un film")); }
        if !playing && self.inhibit.is_some() { self.inhibit = None; self.save_position(false); }
        self.save_position(true);
    }

    pub fn open_externally(&self, id: &str) {
        let Some(i) = self.index_of(id) else { return };
        let path = self.items[i].path.clone();
        let flatpak_vlc = std::path::Path::new("/var/lib/flatpak/app/org.videolan.VLC").exists()
            || dirs::data_dir().map(|d| d.join("flatpak/app/org.videolan.VLC").exists()).unwrap_or(false);
        let mut c;
        if sys::which("vlc").is_some() {
            c = sys::clean_command("vlc");
        } else if flatpak_vlc {
            c = sys::clean_command("flatpak");
            c.args(["run", &format!("--filesystem={}:ro", path.parent().unwrap_or(&path).display()), "org.videolan.VLC"]);
        } else {
            c = sys::clean_command("xdg-open");
        }
        c.arg(&path);
        let _ = sys::spawn_detached(c);
    }

    pub fn has_vlc(&self) -> bool {
        sys::which("vlc").is_some() || std::path::Path::new("/var/lib/flatpak/app/org.videolan.VLC").exists()
            || dirs::data_dir().map(|d| d.join("flatpak/app/org.videolan.VLC").exists()).unwrap_or(false)
    }
}

impl App {
    /// Tests de bout en bout seulement : `BIBLIFILM_TEST="play:<morceau du nom>"`,
    /// `"season:<morceau>"` ou `"select:<morceau>"`, une fois l'analyse finie.
    fn run_test_script(&mut self, ctx: &egui::Context) {
        if self.test_done || self.scanning || self.items.is_empty() { return; }
        let Ok(script) = std::env::var("BIBLIFILM_TEST") else { self.test_done = true; return };
        self.test_done = true;
        let (verb, arg) = script.split_once(':').unwrap_or((&script, ""));
        let Some(i) = self.items.iter().position(|x| x.filename.contains(arg)) else { return };
        let id = self.items[i].id.clone();
        match verb {
            "play" => self.play(ctx, &id, false, None),
            "paused" => { self.play(ctx, &id, false, None); if let Some(s) = &self.session { s.player.set("pause", "yes"); } }
            "season" => { if let Some(s) = model::season_of(&self.items, i) { self.selected = Some(id); self.opened_season = Some(s.id); } }
            _ => self.selected = Some(id),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.images.begin_frame(ctx);
        self.pump(ctx);
        self.tick_player(ctx);
        if self.session.as_ref().map(|s| !s.closing).unwrap_or(false) {
            crate::player_ui::show(self, ctx);
        } else {
            crate::views::show(self, ctx);
        }
        self.toast.ui(ctx, &self.p);
        self.run_test_script(ctx);
        self.test.ui(ctx);
        let p = self.p;
        self.update.ui(ctx, &mut self.settings, &p);
    }

    fn on_exit(&mut self, _gl: Option<&egui_glow::glow::Context>) {
        // Une dernière écriture en quittant, sinon une fermeture en pleine lecture
        // perd jusqu'à deux secondes — assez pour effacer une reprise en début d'épisode.
        self.save_position(false);
        self.watch.save_now();
        self.meta.flush();
        self.update.relaunch_if_needed();
    }
}
