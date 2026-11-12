//! Les jaquettes : téléchargées une fois et gardées sur disque
//! (~/.cache/<App>/images), décodées et réduites hors de l'interface, puis
//! envoyées à la carte graphique. Seules les images vues récemment restent en
//! mémoire : une grille de 1 500 jaquettes ne doit pas coûter des gigaoctets.

use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Size {
    /// Vignette de grille (256 px de large au plus).
    Thumb,
    /// Panneau d'informations, page d'une saison (900 px).
    Large,
    /// Une copie minuscule, que le filtrage linéaire étire en flou.
    Blur,
}

impl Size {
    fn max(self) -> u32 {
        match self { Size::Thumb => 256, Size::Large => 900, Size::Blur => 18 }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Source {
    Url(String),
    File(PathBuf),
}

impl Source {
    pub fn key(&self) -> String {
        match self { Source::Url(u) => u.clone(), Source::File(p) => p.to_string_lossy().to_string() }
    }
}

enum State {
    Loading,
    Ready(TextureHandle, [usize; 2]),
    Failed,
}

struct Job { source: Source, size: Size }
struct Done { source: Source, size: Size, image: Option<ColorImage> }

pub struct ImageCache {
    states: HashMap<(Source, Size), (State, u64)>,
    queue: Arc<Mutex<VecDeque<Job>>>,
    signal: Arc<(Mutex<bool>, std::sync::Condvar)>,
    done_rx: mpsc::Receiver<Done>,
    frame: u64,
    budget: usize,
}

fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() { h = (h ^ b as u64).wrapping_mul(0x100000001b3); }
    format!("{h:016x}")
}

pub fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .user_agent(concat!("VGioM-apps/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn fetch_bytes(source: &Source, disk: &std::path::Path, agent: &ureq::Agent) -> Option<Vec<u8>> {
    match source {
        Source::File(p) => std::fs::read(p).ok(),
        Source::Url(u) => {
            let file = disk.join(fnv(u));
            if let Ok(b) = std::fs::read(&file) { return Some(b); }
            let mut resp = agent.get(u).call().ok()?;
            let bytes = resp.body_mut().with_config().limit(40 << 20).read_to_vec().ok()?;
            let tmp = file.with_extension("part");
            if std::fs::write(&tmp, &bytes).is_ok() { let _ = std::fs::rename(&tmp, &file); }
            Some(bytes)
        }
    }
}

fn decode(bytes: &[u8], path_hint: Option<&std::path::Path>, size: Size) -> Option<ColorImage> {
    let is_svg = path_hint.map(|p| p.extension().map(|e| e == "svg").unwrap_or(false)).unwrap_or(false)
        || bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml");
    let rgba = if is_svg {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
        let target = size.max().min(512);
        let s = tree.size();
        let scale = target as f32 / s.width().max(s.height());
        let (w, h) = ((s.width() * scale).ceil() as u32, (s.height() * scale).ceil() as u32);
        let mut pix = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
        image::RgbaImage::from_raw(pix.width(), pix.height(), pix.take())?
    } else {
        image::load_from_memory(bytes).ok()?.to_rgba8()
    };
    let (w, h) = rgba.dimensions();
    let max = size.max();
    let rgba = if w > max || h > max * 2 {
        let scale = (max as f32 / w as f32).min((max * 2) as f32 / h as f32);
        let filter = if size == Size::Blur { image::imageops::FilterType::Triangle } else { image::imageops::FilterType::CatmullRom };
        image::imageops::resize(&rgba, ((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1), filter)
    } else { rgba };
    let (w, h) = rgba.dimensions();
    Some(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw()))
}

impl ImageCache {
    pub fn new(ctx: &egui::Context, disk: PathBuf, workers: usize) -> Self {
        let _ = std::fs::create_dir_all(&disk);
        let queue: Arc<Mutex<VecDeque<Job>>> = Arc::default();
        let signal = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let (tx, rx) = mpsc::channel();
        for _ in 0..workers {
            let (queue, signal, tx, ctx, disk) = (queue.clone(), signal.clone(), tx.clone(), ctx.clone(), disk.clone());
            std::thread::spawn(move || {
                let agent = http_agent();
                loop {
                    let job = {
                        let (lock, cvar) = &*signal;
                        let mut g = lock.lock().unwrap();
                        loop {
                            if let Some(j) = queue.lock().unwrap().pop_back() { break j; }
                            g = cvar.wait(g).unwrap();
                        }
                    };
                    let hint = if let Source::File(p) = &job.source { Some(p.clone()) } else { None };
                    let image = fetch_bytes(&job.source, &disk, &agent).and_then(|b| decode(&b, hint.as_deref(), job.size));
                    if tx.send(Done { source: job.source, size: job.size, image }).is_err() { return; }
                    ctx.request_repaint();
                }
            });
        }
        ImageCache { states: HashMap::new(), queue, signal, done_rx: rx, frame: 0, budget: 180 }
    }

    /// À appeler une fois par image affichée : les dernières demandées passent
    /// en tête de file (la pile sert les plus récentes d'abord).
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        while let Ok(d) = self.done_rx.try_recv() {
            let key = (d.source.clone(), d.size);
            let state = match d.image {
                Some(img) => {
                    let size = img.size;
                    let opts = TextureOptions { magnification: egui::TextureFilter::Linear, minification: egui::TextureFilter::Linear, ..Default::default() };
                    State::Ready(ctx.load_texture(format!("img:{}:{:?}", d.source.key(), d.size), img, opts), size)
                }
                None => State::Failed,
            };
            self.states.insert(key, (state, self.frame));
        }
        // Les images les moins récemment vues s'en vont au-delà du budget.
        let ready = self.states.values().filter(|(s, _)| matches!(s, State::Ready(..))).count();
        if ready > self.budget {
            let mut by_age: Vec<_> = self.states.iter().filter(|(_, (s, _))| matches!(s, State::Ready(..))).map(|(k, (_, f))| (k.clone(), *f)).collect();
            by_age.sort_by_key(|(_, f)| *f);
            for (k, f) in by_age.into_iter().take(ready - self.budget) {
                if f + 2 < self.frame { self.states.remove(&k); }
            }
        }
    }

    /// La texture si elle est prête ; sinon la demande est lancée.
    pub fn get(&mut self, source: &Source, size: Size) -> Option<(TextureHandle, [usize; 2])> {
        let key = (source.clone(), size);
        let frame = self.frame;
        match self.states.get_mut(&key) {
            Some((State::Ready(t, s), f)) => { *f = frame; Some((t.clone(), *s)) }
            Some((_, f)) => { *f = frame; None }
            None => {
                self.states.insert(key, (State::Loading, frame));
                self.queue.lock().unwrap().push_back(Job { source: source.clone(), size });
                let (lock, cvar) = &*self.signal;
                let _g = lock.lock().unwrap();
                cvar.notify_one();
                None
            }
        }
    }

    pub fn is_loading(&self, source: &Source, size: Size) -> bool {
        matches!(self.states.get(&(source.clone(), size)), Some((State::Loading, _)))
    }

    pub fn failed(&self, source: &Source, size: Size) -> bool {
        matches!(self.states.get(&(source.clone(), size)), Some((State::Failed, _)))
    }

    /// Oublie tout (mémoire et disque).
    pub fn clear(&mut self, disk: &std::path::Path) {
        self.states.clear();
        if let Ok(rd) = std::fs::read_dir(disk) { for e in rd.flatten() { let _ = std::fs::remove_file(e.path()); } }
    }
}

/// Taille du cache disque, en octets.
pub fn disk_size(disk: &std::path::Path) -> u64 {
    std::fs::read_dir(disk).map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0)
}
