//! L'analyse du dossier, sa surveillance, et l'historique de visionnage.

use crate::common::store;
use crate::model::{Item, Record, COMPLETION};
use crate::parser::{self, AUDIO, VIDEO};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SKIPPED: &[&str] = &[".Trash", "$RECYCLE.BIN", "node_modules", ".git", ".svn", "lost+found", "@eaDir", "Extras", "Sample", "Samples", "Featurettes"];
/// En dessous : un extrait ou une bande-annonce.
const MIN_VIDEO: u64 = 20 * 1024 * 1024;

pub struct ScanOptions { pub include_audio: bool, pub skip_small: bool }

pub fn scan(root: &Path, opts: &ScanOptions, progress: &dyn Fn(usize)) -> Vec<Item> {
    let mut items = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') { continue; }
            let p = e.path();
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            if meta.is_dir() {
                if !SKIPPED.contains(&name.as_str()) { stack.push(p); }
                continue;
            }
            if !meta.is_file() { continue; }
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            let (video, audio) = (VIDEO.contains(&ext.as_str()), AUDIO.contains(&ext.as_str()));
            if !video && !(audio && opts.include_audio) { continue; }
            if opts.skip_small && video && meta.len() < MIN_VIDEO { continue; }
            // Le dossier parent est le meilleur indice quand le nom ne porte pas la série.
            let hint = if dir.as_path() == root { None } else { dir.file_name().map(|n| n.to_string_lossy().to_string()) };
            let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0);
            items.push(Item {
                id: p.to_string_lossy().to_string(), filename: name, ext, parsed: parser::parse(&p, hint.as_deref()),
                path: p, size: meta.len(), modified, metadata: None,
            });
            if items.len() % 25 == 0 { progress(items.len()); }
        }
    }
    progress(items.len());
    items.sort_by(|a, b| crate::model::collator(&a.display_title(), &b.display_title()));
    items
}

/// La surveillance du dossier (inotify). Les événements sont regroupés : un
/// téléchargement en cours ne doit pas déclencher dix analyses.
pub fn watch(root: &Path, ctx: egui::Context, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, notify::EventKind::Create(_) | notify::EventKind::Remove(_) | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))) {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                ctx.request_repaint_after(std::time::Duration::from_secs(4));
            }
        }
    }).ok()?;
    w.watch(root, RecursiveMode::Recursive).ok()?;
    Some(w)
}

// ── Historique ────────────────────────────────────────────────────────────

pub struct WatchStore {
    file: PathBuf,
    pub records: HashMap<String, Record>,
    dirty_since: Option<std::time::Instant>,
}

impl WatchStore {
    pub fn load(dir: &Path) -> Self {
        let file = dir.join("watch-state.json");
        WatchStore { records: store::read_json(&file).unwrap_or_default(), file, dirty_since: None }
    }

    /// Appelé pendant la lecture.
    pub fn update(&mut self, id: &str, position: f64, duration: f64) {
        let r = self.records.entry(id.to_string()).or_default();
        r.position = position;
        if duration > 0.0 { r.duration = duration; }
        r.last_played = crate::common::sys::now_ms();
        if r.progress() >= COMPLETION { r.watched = true; }
        self.touch();
    }

    pub fn set_watched(&mut self, ids: &[String], watched: bool) {
        for id in ids {
            let r = self.records.entry(id.clone()).or_default();
            r.watched = watched;
            r.last_played = crate::common::sys::now_ms();
            // Non vu efface la reprise ; vu la met à la fin.
            r.position = if watched { r.duration } else { 0.0 };
        }
        self.touch();
    }

    fn touch(&mut self) { if self.dirty_since.is_none() { self.dirty_since = Some(std::time::Instant::now()); } }

    /// Écrit au plus toutes les 1,5 s.
    pub fn tick(&mut self) {
        if let Some(t) = self.dirty_since { if t.elapsed().as_millis() >= 1500 { self.save_now(); } }
    }

    /// L'ancienne version est gardée en sauvegarde : l'historique est irremplaçable.
    pub fn save_now(&mut self) {
        self.dirty_since = None;
        let _ = store::write_json(&self.file, &self.records, !self.records.is_empty());
    }

    pub fn restore(&mut self) -> usize {
        let bak = self.file.with_file_name("watch-state.backup.json");
        match store::read_json::<HashMap<String, Record>>(&bak) {
            Some(r) => { let n = r.len(); self.records = r; self.save_now(); n }
            None => 0,
        }
    }
}
