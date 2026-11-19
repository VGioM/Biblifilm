//! L'état de Biblifilm et ce qui tourne en fond : analyse du dossier,
//! identification en ligne, lecture dans VLC. L'affichage est dans `views.rs`.

use crate::common::images::ImageCache;
use crate::common::selfupdate::SelfUpdate;
use crate::common::store::{self, Settings};
use crate::common::sys;
use crate::common::theme::{self, Palette};
use crate::common::ui::Toast;
use crate::library::{self, ScanOptions, WatchStore};
use crate::metadata::{Candidate, Service, Tmdb};
use crate::model::{self, Entry, Item, Metadata, Watch};
use crate::vlc;
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

/// Une séance de lecture dans VLC, suivie pour la reprise et les épisodes vus.
pub struct Session {
    pub player: vlc::Player,
    pub queue: Vec<String>,
    pub titles: Vec<(String, String)>,
    /// Le fichier de la file en cours, sa position et sa durée (s).
    pub index: usize,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    /// Secondes vraiment jouées de ce fichier : une ouverture éclair n'efface pas la reprise.
    pub played: f64,
    pub last_saved: f64,
    pub last_save_at: Instant,
}

/// Une lecture demandée pendant que VLC s'installe.
#[derive(Clone)]
pub struct PendingPlay { pub id: String, pub from_start: bool, pub followers: Option<Vec<usize>> }

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
    pub vlc: vlc::Manager,
    pub pending_play: Option<PendingPlay>,

    pub error: Option<String>,
    pub toast: Toast,
    pub settings_open: Option<SettingsTab>,
    pub correction: Option<Correction>,
    pub key_status: Option<Result<(), String>>,
    pub restore_msg: Option<String>,
    pub pending_folder: Option<mpsc::Receiver<Vec<PathBuf>>>,
    pub update: SelfUpdate,
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
            "seriesExpanded": true, "vlcFullscreen": true
        }));
        let (tx, rx) = mpsc::channel();
        let vlc = vlc::Manager::new(&dir, &cc.egui_ctx);
        let last_key = settings.string("tmdbAPIKey").trim().to_string();
        let mut app = App {
            p, images: ImageCache::new(&cc.egui_ctx, cache.clone(), 6), meta: Arc::new(Service::new(dir.join("metadata-cache.json"))),
            watch: WatchStore::load(&dir), dir, cache, settings, items: vec![],
            search: String::new(), focus_search: false, opened_season: None, selected: None, banner_dismissed: false,
            scanning: false, scan_progress: Arc::default(), fetching: false, fetch_total: 0, fetch_done: 0, run: Arc::default(), scan_run: 0,
            tx, rx, watcher: None, changed: Arc::default(), changed_at: None, key_changed_at: None, last_key,
            session: None, vlc, pending_play: None,
            error: None, toast: Toast::default(), settings_open: None, correction: None, key_status: None, restore_msg: None,
            pending_folder: None, update: SelfUpdate::new(REPO, "Biblifilm"),
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

    /// Lit un fichier dans VLC ; un épisode met en file la suite de sa saison.
    /// Si VLC manque, il est installé d'abord, puis la lecture part toute seule.
    pub fn play(&mut self, ctx: &egui::Context, id: &str, from_start: bool, followers: Option<Vec<usize>>) {
        let Some(i) = self.index_of(id) else { return };
        if !self.items[i].path.exists() { self.error = Some(format!("Le fichier est introuvable :\n{}", self.items[i].path.display())); return; }
        let Some(kind) = self.vlc.kind() else {
            self.pending_play = Some(PendingPlay { id: id.to_string(), from_start, followers });
            match self.vlc.state() {
                vlc::State::Missing | vlc::State::Failed(_) => { self.vlc.install(ctx, |_| {}); self.toast.show("Installation de VLC… la lecture démarrera ensuite"); }
                _ => self.toast.show("VLC se prépare…"),
            }
            return;
        };
        let rest = followers.unwrap_or_else(|| model::season_of(&self.items, i).map(|s| {
            let k = s.episodes.iter().position(|&x| x == i).unwrap_or(0);
            s.episodes[k + 1..].to_vec()
        }).unwrap_or_default());
        let start = if from_start { 0.0 } else { self.w().resume(i).unwrap_or(0.0) };
        let queue: Vec<usize> = std::iter::once(i).chain(rest).collect();
        let files: Vec<String> = queue.iter().map(|&k| self.items[k].path.to_string_lossy().to_string()).collect();
        self.stop_playback();
        match vlc::Player::spawn(&kind, &files, start, self.settings.bool("vlcFullscreen"), ctx) {
            Ok(player) => {
                self.session = Some(Session {
                    player, queue: queue.iter().map(|&k| self.items[k].id.clone()).collect(),
                    titles: queue.iter().map(|&k| (self.items[k].player_title(), self.items[k].player_subtitle())).collect(),
                    index: 0, position: start, duration: 0.0, playing: false, played: 0.0,
                    last_saved: -1.0, last_save_at: Instant::now(),
                });
            }
            Err(e) => self.error = Some(format!("VLC n'a pas pu démarrer : {e}")),
        }
    }

    /// Une lecture en attente de VLC part dès qu'il est prêt.
    fn tick_vlc(&mut self, ctx: &egui::Context) {
        let Some(p) = self.pending_play.clone() else { return };
        match self.vlc.state() {
            vlc::State::Ready { .. } => { self.pending_play = None; self.play(ctx, &p.id, p.from_start, p.followers); }
            vlc::State::Failed(e) => { self.pending_play = None; self.error = Some(format!("VLC n'a pas pu être installé : {e}")); }
            vlc::State::Missing => self.vlc.install(ctx, |_| {}),
            _ => {}
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
        if s.played <= 3.0 || s.duration <= 0.0 { return; }
        let pos = (s.position * 10.0).round() / 10.0;
        if routine && (pos == s.last_saved || s.last_save_at.elapsed().as_secs_f32() < 2.0) { return; }
        s.last_saved = pos;
        s.last_save_at = Instant::now();
        let id = s.queue[s.index.min(s.queue.len() - 1)].clone();
        let (p, d) = (s.position, s.duration);
        self.watch.update(&id, p, d);
    }

    /// Ferme la fenêtre VLC de la lecture en cours.
    pub fn stop_playback(&mut self) {
        self.save_position(false);
        if let Some(s) = self.session.take() { s.player.close(); }
        self.watch.save_now();
    }

    /// Le suivi de la lecture dans VLC.
    pub fn tick_player(&mut self) {
        let Some(s) = &mut self.session else { return };
        for ev in s.player.poll() {
            let Some(s) = &mut self.session else { return };
            match ev {
                vlc::Event::Status { index, position, duration, playing } => {
                    if index != s.index { s.index = index; s.played = 0.0; s.last_saved = -1.0; }
                    if playing && duration > 0.0 { s.played += 1.0; }
                    s.position = position;
                    if duration > 0.0 { s.duration = duration; }
                    s.playing = playing;
                    if !playing { self.save_position(false); } else { self.save_position(true); }
                }
                // Fin réelle du fichier : l'épisode est vu.
                vlc::Event::Ended { index, duration } => {
                    let id = s.queue[index.min(s.queue.len() - 1)].clone();
                    self.watch.update(&id, duration, duration);
                }
                vlc::Event::Exited => {
                    self.save_position(false);
                    self.session = None;
                    self.watch.save_now();
                    return;
                }
            }
        }
    }

    /// Le titre en cours de lecture dans VLC, pour la barre « En lecture ».
    pub fn now_playing(&self) -> Option<(String, String, f64, f64, bool)> {
        let s = self.session.as_ref()?;
        let (t, sub) = s.titles.get(s.index).cloned().unwrap_or_default();
        Some((t, sub, s.position, s.duration, s.playing))
    }

    /// Ouvre le fichier avec l'application choisie par le bureau pour ce type de fichier.
    pub fn open_externally(&self, id: &str) {
        let Some(i) = self.index_of(id) else { return };
        sys::open_path(&self.items[i].path);
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
            "season" => { if let Some(s) = model::season_of(&self.items, i) { self.selected = Some(id); self.opened_season = Some(s.id); } }
            _ => self.selected = Some(id),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.images.begin_frame(ctx);
        self.pump(ctx);
        self.tick_vlc(ctx);
        self.tick_player();
        crate::views::show(self, ctx);
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
