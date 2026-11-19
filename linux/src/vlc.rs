//! libVLC dans la fenêtre : le même moteur que VLCKit sur Mac. VLC décode
//! (décodage matériel compris) et livre chaque image dans un tampon mémoire ;
//! l'app l'affiche comme une texture, avec l'interface du lecteur par-dessus.
//!
//! libVLC est chargée à l'exécution (dlopen) : l'AppImage embarque la sienne et
//! ses modules (FFmpeg complet, sous-titres ASS…) ; celle du système sert sinon.

#![allow(non_camel_case_types)]

use libloading::{Library, Symbol};
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

type inst_t = c_void;
type media_t = c_void;
type mp_t = c_void;

#[repr(C)]
struct track_description_t { i_id: c_int, psz_name: *mut c_char, p_next: *mut track_description_t }

type LockCb = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void;
type UnlockCb = unsafe extern "C" fn(*mut c_void, *mut c_void, *const *mut c_void);
type DisplayCb = unsafe extern "C" fn(*mut c_void, *mut c_void);
type FormatCb = unsafe extern "C" fn(*mut *mut c_void, *mut c_char, *mut c_uint, *mut c_uint, *mut c_uint, *mut c_uint) -> c_uint;
type CleanupCb = unsafe extern "C" fn(*mut c_void);

struct Api {
    new: unsafe extern "C" fn(c_int, *const *const c_char) -> *mut inst_t,
    media_new_path: unsafe extern "C" fn(*mut inst_t, *const c_char) -> *mut media_t,
    media_add_option: unsafe extern "C" fn(*mut media_t, *const c_char),
    media_release: unsafe extern "C" fn(*mut media_t),
    mp_new: unsafe extern "C" fn(*mut inst_t) -> *mut mp_t,
    mp_release: unsafe extern "C" fn(*mut mp_t),
    mp_set_media: unsafe extern "C" fn(*mut mp_t, *mut media_t),
    mp_play: unsafe extern "C" fn(*mut mp_t) -> c_int,
    mp_stop: unsafe extern "C" fn(*mut mp_t),
    mp_set_pause: unsafe extern "C" fn(*mut mp_t, c_int),
    mp_get_time: unsafe extern "C" fn(*mut mp_t) -> i64,
    mp_set_time: unsafe extern "C" fn(*mut mp_t, i64),
    mp_get_length: unsafe extern "C" fn(*mut mp_t) -> i64,
    mp_get_state: unsafe extern "C" fn(*mut mp_t) -> c_int,
    video_set_callbacks: unsafe extern "C" fn(*mut mp_t, Option<LockCb>, Option<UnlockCb>, Option<DisplayCb>, *mut c_void),
    video_set_format_callbacks: unsafe extern "C" fn(*mut mp_t, Option<FormatCb>, Option<CleanupCb>),
    audio_set_volume: unsafe extern "C" fn(*mut mp_t, c_int) -> c_int,
    audio_get_track_description: unsafe extern "C" fn(*mut mp_t) -> *mut track_description_t,
    audio_get_track: unsafe extern "C" fn(*mut mp_t) -> c_int,
    audio_set_track: unsafe extern "C" fn(*mut mp_t, c_int) -> c_int,
    spu_description: unsafe extern "C" fn(*mut mp_t) -> *mut track_description_t,
    spu_get: unsafe extern "C" fn(*mut mp_t) -> c_int,
    spu_set: unsafe extern "C" fn(*mut mp_t, c_int) -> c_int,
    track_list_release: unsafe extern "C" fn(*mut track_description_t),
    _lib: Library,
}

/// L'API et l'instance libVLC, partagées par toutes les lectures.
struct Vlc { api: Api, inst: usize }
unsafe impl Send for Vlc {}
unsafe impl Sync for Vlc {}

static VLC: OnceLock<Result<Vlc, String>> = OnceLock::new();

fn load() -> Result<Vlc, String> {
    let mut names: Vec<String> = vec![];
    if let Ok(dir) = std::env::var("APPDIR") {
        names.push(format!("{dir}/usr/lib/libvlc.so.5"));
        // Les modules embarqués, pas ceux d'un VLC du système d'une autre version.
        std::env::set_var("VLC_PLUGIN_PATH", format!("{dir}/usr/lib/vlc/plugins"));
    }
    names.extend(["libvlc.so.5", "libvlc.so"].map(String::from));
    let mut last = String::new();
    for name in &names {
        match unsafe { Library::new(name.as_str()) } {
            Ok(lib) => unsafe {
                macro_rules! sym { ($n:literal) => {{ let s: Symbol<_> = lib.get($n).map_err(|e| e.to_string())?; *s }} }
                let api = Api {
                    new: sym!(b"libvlc_new\0"), media_new_path: sym!(b"libvlc_media_new_path\0"),
                    media_add_option: sym!(b"libvlc_media_add_option\0"), media_release: sym!(b"libvlc_media_release\0"),
                    mp_new: sym!(b"libvlc_media_player_new\0"), mp_release: sym!(b"libvlc_media_player_release\0"),
                    mp_set_media: sym!(b"libvlc_media_player_set_media\0"), mp_play: sym!(b"libvlc_media_player_play\0"),
                    mp_stop: sym!(b"libvlc_media_player_stop\0"), mp_set_pause: sym!(b"libvlc_media_player_set_pause\0"),
                    mp_get_time: sym!(b"libvlc_media_player_get_time\0"), mp_set_time: sym!(b"libvlc_media_player_set_time\0"),
                    mp_get_length: sym!(b"libvlc_media_player_get_length\0"), mp_get_state: sym!(b"libvlc_media_player_get_state\0"),
                    video_set_callbacks: sym!(b"libvlc_video_set_callbacks\0"), video_set_format_callbacks: sym!(b"libvlc_video_set_format_callbacks\0"),
                    audio_set_volume: sym!(b"libvlc_audio_set_volume\0"), audio_get_track_description: sym!(b"libvlc_audio_get_track_description\0"),
                    audio_get_track: sym!(b"libvlc_audio_get_track\0"), audio_set_track: sym!(b"libvlc_audio_set_track\0"),
                    spu_description: sym!(b"libvlc_video_get_spu_description\0"), spu_get: sym!(b"libvlc_video_get_spu\0"),
                    spu_set: sym!(b"libvlc_video_set_spu\0"), track_list_release: sym!(b"libvlc_track_description_list_release\0"),
                    _lib: lib,
                };
                let args: Vec<CString> = ["--quiet", "--no-video-title-show", "--no-osd", "--no-stats", "--no-lua", "--no-xlib",
                    "--sub-autodetect-fuzzy=3", "--audio-language=fr,fre,fra,en,eng", "--sub-language=fr,fre,fra,en,eng"]
                    .iter().map(|s| CString::new(*s).unwrap()).collect();
                let ptrs: Vec<*const c_char> = args.iter().map(|c| c.as_ptr()).collect();
                let inst = (api.new)(ptrs.len() as c_int, ptrs.as_ptr());
                if inst.is_null() { return Err("libVLC n'a pas pu démarrer (modules introuvables ?)".into()); }
                return Ok(Vlc { api, inst: inst as usize });
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("libVLC introuvable ({last})"))
}

fn vlc() -> Result<&'static Vlc, String> { VLC.get_or_init(load).as_ref().map_err(|e| e.clone()) }

/// Charge libVLC et ses modules dans un thread, pour que la fenêtre s'ouvre sans attendre.
pub fn preload() { std::thread::spawn(|| { let _ = vlc(); }); }

/// L'erreur de chargement, si le chargement est terminé et a échoué (sans bloquer).
pub fn load_error() -> Option<String> { VLC.get().and_then(|r| r.as_ref().err().cloned()) }

// ── Les images : VLC écrit dans un tampon, l'app en fait une texture ───────

/// Au-delà, VLC réduit l'image : une texture 4K à chaque image coûte cher pour rien.
const MAX_W: u32 = 2560;
const MAX_H: u32 = 1440;

pub struct Frame { pub w: usize, pub h: usize, pub pixels: Vec<egui::Color32> }

struct Video {
    /// Le tampon où VLC écrit (BGRA), touché seulement par le thread vidéo de VLC.
    buf: UnsafeCell<Vec<u8>>,
    size: Mutex<(usize, usize)>,
    latest: Mutex<Option<Frame>>,
    ctx: egui::Context,
}
unsafe impl Send for Video {}
unsafe impl Sync for Video {}

unsafe extern "C" fn format_cb(opaque: *mut *mut c_void, chroma: *mut c_char, w: *mut c_uint, h: *mut c_uint, pitches: *mut c_uint, lines: *mut c_uint) -> c_uint {
    let v = &*(*opaque as *const Video);
    let (mut width, mut height) = (*w, *h);
    if width == 0 || height == 0 { return 0; }
    let scale = (MAX_W as f64 / width as f64).min(MAX_H as f64 / height as f64).min(1.0);
    width = ((width as f64 * scale) as u32).max(2) & !1;
    height = ((height as f64 * scale) as u32).max(2) & !1;
    ptr::copy_nonoverlapping(b"RV32".as_ptr() as *const c_char, chroma, 4);
    *w = width; *h = height;
    *pitches = width * 4;
    *lines = height;
    *v.buf.get() = vec![0u8; (width * height * 4) as usize];
    *v.size.lock().unwrap() = (width as usize, height as usize);
    1
}

unsafe extern "C" fn cleanup_cb(_opaque: *mut c_void) {}

unsafe extern "C" fn lock_cb(opaque: *mut c_void, planes: *mut *mut c_void) -> *mut c_void {
    let v = &*(opaque as *const Video);
    *planes = (*v.buf.get()).as_mut_ptr() as *mut c_void;
    ptr::null_mut()
}

unsafe extern "C" fn unlock_cb(_opaque: *mut c_void, _picture: *mut c_void, _planes: *const *mut c_void) {}

unsafe extern "C" fn display_cb(opaque: *mut c_void, _picture: *mut c_void) {
    let v = &*(opaque as *const Video);
    let (w, h) = *v.size.lock().unwrap();
    let buf = &*v.buf.get();
    if buf.len() < w * h * 4 { return; }
    // RV32 : B, G, R, X en mémoire.
    let pixels: Vec<egui::Color32> = buf[..w * h * 4].chunks_exact(4).map(|c| egui::Color32::from_rgb(c[2], c[1], c[0])).collect();
    *v.latest.lock().unwrap() = Some(Frame { w, h, pixels });
    v.ctx.request_repaint();
}

// ── Une séance de lecture ─────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct Track { pub id: i64, pub kind: String, pub title: String, pub lang: String, pub codec: String, pub selected: bool }

impl Track {
    pub fn label(&self) -> String {
        let mut parts = vec![];
        if !self.title.is_empty() { parts.push(self.title.clone()); }
        if !self.lang.is_empty() { parts.push(format!("[{}]", self.lang)); }
        if parts.is_empty() { parts.push(format!("Piste {}", self.id)); }
        parts.join(" ")
    }
}

/// « Track 2 - [English] » → (« Piste 2 », « Anglais ») : libVLC nomme en anglais
/// quand ses traductions ne sont pas installées.
fn split_name(name: &str) -> (String, String) {
    let (title, lang) = match name.rfind('[') {
        Some(open) if name.ends_with(']') => (name[..open].trim().trim_end_matches('-').trim(), name[open + 1..name.len() - 1].trim()),
        _ => (name.trim(), ""),
    };
    let title = match title.strip_prefix("Track ") { Some(n) => format!("Piste {n}"), None => title.to_string() };
    (title, french_language(lang))
}

fn french_language(lang: &str) -> String {
    const NAMES: &[(&str, &str)] = &[("English", "Anglais"), ("French", "Français"), ("German", "Allemand"), ("Spanish", "Espagnol"),
        ("Castilian", "Espagnol"), ("Italian", "Italien"), ("Portuguese", "Portugais"), ("Japanese", "Japonais"), ("Korean", "Coréen"),
        ("Chinese", "Chinois"), ("Russian", "Russe"), ("Dutch", "Néerlandais"), ("Swedish", "Suédois"), ("Danish", "Danois"),
        ("Norwegian", "Norvégien"), ("Finnish", "Finnois"), ("Polish", "Polonais"), ("Czech", "Tchèque"), ("Hungarian", "Hongrois"),
        ("Greek", "Grec"), ("Turkish", "Turc"), ("Arabic", "Arabe"), ("Hebrew", "Hébreu"), ("Hindi", "Hindi"), ("Thai", "Thaï"),
        ("Undetermined", "")];
    NAMES.iter().find(|(en, _)| lang.eq_ignore_ascii_case(en) || lang.starts_with(&format!("{en};")))
        .map(|(_, fr)| fr.to_string()).unwrap_or_else(|| lang.to_string())
}

pub enum Event {
    /// Un fichier est ouvert (index dans la file).
    Loaded(usize),
    /// La fin réelle d'un fichier : l'épisode est vu.
    Eof(usize),
    /// Un fichier qui ne s'ouvre pas.
    Error(usize, String),
    /// La file est finie.
    Finished,
}

// États de libvlc_media_player_get_state.
const ST_PLAYING: c_int = 3;
const ST_PAUSED: c_int = 4;
const ST_ENDED: c_int = 6;
const ST_ERROR: c_int = 7;

pub struct Player {
    mp: usize,
    video: Arc<Video>,
    files: Vec<String>,
    texture: Option<egui::TextureHandle>,
    pub index: usize,
    pub count: usize,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub volume: f64,
    pub tracks: Vec<Track>,
    /// Ce qui a vraiment été joué de ce fichier, en secondes : la lecture n'est
    /// comptée qu'au-delà de 3 s.
    pub played: f64,
    pub loaded: bool,
    loaded_at: Option<Instant>,
    tracks_at: Option<Instant>,
    failed: bool,
    /// La fin d'un fichier vient d'être signalée : le suivant s'ouvre au tour d'après,
    /// une fois la fin enregistrée par l'app avec la durée de ce fichier.
    advance: bool,
    closed: bool,
}

unsafe impl Send for Player {}

fn cstr(s: &str) -> CString { CString::new(s.replace('\0', "")).unwrap() }

impl Player {
    pub fn new(files: &[String], start: f64, egui_ctx: &egui::Context, volume: f64, _alang: &str) -> Result<Player, String> {
        let v = vlc()?;
        let mp = unsafe { (v.api.mp_new)(v.inst as *mut inst_t) };
        if mp.is_null() { return Err("le lecteur VLC n'a pas pu être créé".into()); }
        let video = Arc::new(Video { buf: UnsafeCell::new(vec![]), size: Mutex::new((0, 0)), latest: Mutex::new(None), ctx: egui_ctx.clone() });
        let opaque = Arc::as_ptr(&video) as *mut c_void;
        unsafe {
            (v.api.video_set_callbacks)(mp, Some(lock_cb), Some(unlock_cb), Some(display_cb), opaque);
            (v.api.video_set_format_callbacks)(mp, Some(format_cb), Some(cleanup_cb));
        }
        let mut p = Player {
            mp: mp as usize, video, files: files.to_vec(), texture: None, index: 0, count: files.len(), position: start.max(0.0), duration: 0.0,
            paused: false, volume, tracks: vec![], played: 0.0, loaded: false, loaded_at: None, tracks_at: None, failed: false, advance: false, closed: false,
        };
        p.open(0, start)?;
        Ok(p)
    }

    fn mp(&self) -> *mut mp_t { self.mp as *mut mp_t }

    /// Ouvre le fichier `k` de la file, à `start` secondes.
    fn open(&mut self, k: usize, start: f64) -> Result<(), String> {
        let v = vlc()?;
        let path = cstr(&self.files[k]);
        unsafe {
            let m = (v.api.media_new_path)(v.inst as *mut inst_t, path.as_ptr());
            if m.is_null() { return Err("fichier illisible".into()); }
            if start > 0.0 { (v.api.media_add_option)(m, cstr(&format!(":start-time={}", start.floor())).as_ptr()); }
            (v.api.mp_set_media)(self.mp(), m);
            (v.api.media_release)(m);
            if (v.api.mp_play)(self.mp()) != 0 { return Err("fichier illisible".into()); }
        }
        self.index = k;
        self.position = start.max(0.0);
        self.duration = 0.0; self.played = 0.0; self.loaded = false; self.loaded_at = None; self.failed = false; self.tracks.clear();
        Ok(())
    }

    pub fn set(&self, name: &str, value: &str) {
        if name == "pause" { if let Ok(v) = vlc() { unsafe { (v.api.mp_set_pause)(self.mp(), (value == "yes") as c_int) }; } }
    }

    pub fn toggle_pause(&self) {
        if let Ok(v) = vlc() { unsafe { (v.api.mp_set_pause)(self.mp(), (!self.paused) as c_int) }; }
    }
    pub fn seek_absolute(&self, s: f64) {
        let s = if self.duration > 0.0 { s.clamp(0.0, (self.duration - 1.0).max(0.0)) } else { s.max(0.0) };
        if let Ok(v) = vlc() { unsafe { (v.api.mp_set_time)(self.mp(), (s * 1000.0) as i64) }; }
    }
    pub fn seek_relative(&self, s: f64) { self.seek_absolute(self.position + s); }
    pub fn set_volume(&mut self, vol: f64) {
        self.volume = vol.clamp(0.0, 130.0);
        if let Ok(v) = vlc() { unsafe { (v.api.audio_set_volume)(self.mp(), self.volume.round() as c_int) }; }
    }
    pub fn next(&mut self) {
        if self.index + 1 < self.count { let k = self.index + 1; let _ = self.open(k, 0.0); }
    }
    pub fn select_track(&self, kind: &str, id: Option<i64>) {
        let Ok(v) = vlc() else { return };
        let id = id.unwrap_or(-1) as c_int;
        unsafe { if kind == "audio" { (v.api.audio_set_track)(self.mp(), id); } else { (v.api.spu_set)(self.mp(), id); } }
    }

    pub fn refresh_tracks(&mut self) {
        let Ok(v) = vlc() else { return };
        let mut out = vec![];
        unsafe {
            for (kind, list, cur) in [
                ("audio", (v.api.audio_get_track_description)(self.mp()), (v.api.audio_get_track)(self.mp())),
                ("sub", (v.api.spu_description)(self.mp()), (v.api.spu_get)(self.mp())),
            ] {
                let mut node = list;
                while !node.is_null() {
                    let n = &*node;
                    // L'entrée -1 est « Désactiver » : « Aucun » est déjà proposé par l'interface.
                    if n.i_id >= 0 {
                        let name = if n.psz_name.is_null() { String::new() } else { CStr::from_ptr(n.psz_name).to_string_lossy().to_string() };
                        let (title, lang) = split_name(&name);
                        out.push(Track { id: n.i_id as i64, kind: kind.into(), title, lang, codec: String::new(), selected: n.i_id == cur });
                    }
                    node = n.p_next;
                }
                if !list.is_null() { (v.api.track_list_release)(list); }
            }
        }
        self.tracks = out;
        self.tracks_at = Some(Instant::now());
    }

    /// L'état de VLC, relu à chaque image.
    pub fn poll(&mut self) -> Vec<Event> {
        let Ok(v) = vlc() else { return vec![Event::Finished] };
        if self.closed { return vec![]; }
        // Sans nouvelle image (pause, fin du fichier), l'état doit quand même être relu.
        self.video.ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let mut out = vec![];
        if self.advance {
            self.advance = false;
            let k = self.index + 1;
            if let Err(e) = self.open(k, 0.0) { out.push(Event::Error(k, e)); }
            return out;
        }
        let (state, t, len) = unsafe { ((v.api.mp_get_state)(self.mp()), (v.api.mp_get_time)(self.mp()), (v.api.mp_get_length)(self.mp())) };
        if len > 0 { self.duration = len as f64 / 1000.0; }
        // VLC_TRACE=1 : l'état du lecteur sur la sortie d'erreur, pour le diagnostic.
        if std::env::var("VLC_TRACE").is_ok() { eprintln!("vlc state={state} t={t} len={len}"); }
        // Avant que l'horloge démarre, VLC répond 0 : on garde la position de départ.
        if t > 0 && (state == ST_PLAYING || state == ST_PAUSED) {
            let pos = t.max(0) as f64 / 1000.0;
            let step = pos - self.position;
            if state == ST_PLAYING && step > 0.0 && step < 2.5 { self.played += step; }
            self.position = pos;
        }
        self.paused = state == ST_PAUSED;
        if !self.loaded && (state == ST_PLAYING || state == ST_PAUSED) && len > 0 {
            self.loaded = true;
            self.loaded_at = Some(Instant::now());
            unsafe { (v.api.audio_set_volume)(self.mp(), self.volume.round() as c_int) };
            self.refresh_tracks();
            out.push(Event::Loaded(self.index));
        }
        // Les pistes arrivent parfois après le début (sous-titres externes) : relues quelques secondes.
        if let (Some(at), Some(tr)) = (self.loaded_at, self.tracks_at) {
            if at.elapsed().as_secs() < 6 && tr.elapsed().as_millis() > 1000 { self.refresh_tracks(); }
        }
        match state {
            ST_ENDED => {
                out.push(Event::Eof(self.index));
                if self.index + 1 < self.count { self.advance = true; } else { out.push(Event::Finished); self.closed = true; }
            }
            ST_ERROR if !self.failed => { self.failed = true; out.push(Event::Error(self.index, "fichier illisible".into())); }
            _ => {}
        }
        out
    }

    /// La dernière image, en texture, et sa taille (rien tant que VLC n'a rien montré).
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<(egui::TextureId, egui::Vec2)> {
        if let Some(f) = self.video.latest.lock().unwrap().take() {
            let img = egui::ColorImage { size: [f.w, f.h], pixels: f.pixels };
            match &mut self.texture {
                Some(t) if t.size() == [f.w, f.h] => t.set(img, egui::TextureOptions::LINEAR),
                _ => self.texture = Some(ctx.load_texture("vlc-video", img, egui::TextureOptions::LINEAR)),
            }
        }
        self.texture.as_ref().map(|t| (t.id(), t.size_vec2()))
    }
}

impl Player {
    /// Arrête VLC en fond (l'arrêt attend ses threads) ; le lecteur peut ensuite être lâché.
    pub fn close(&mut self) {
        if self.closed && self.mp == 0 { return; }
        self.closed = true;
        let (mp, video) = (std::mem::replace(&mut self.mp, 0), self.video.clone());
        if mp == 0 { return; }
        std::thread::spawn(move || {
            if let Ok(v) = vlc() { unsafe { (v.api.mp_stop)(mp as *mut mp_t); (v.api.mp_release)(mp as *mut mp_t); } }
            drop(video);
        });
    }
    pub fn can_drop(&self) -> bool { self.mp == 0 }
}

impl Drop for Player {
    fn drop(&mut self) { self.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_names() {
        assert_eq!(split_name("Commentaires - [Anglais]"), ("Commentaires".into(), "Anglais".into()));
        assert_eq!(split_name("Track 2 - [English]"), ("Piste 2".into(), "Anglais".into()));
        assert_eq!(split_name("VF - [French]"), ("VF".into(), "Français".into()));
        assert_eq!(split_name("Piste 1 - [Français]"), ("Piste 1".into(), "Français".into()));
        assert_eq!(split_name("Piste 2"), ("Piste 2".into(), String::new()));
    }
}
