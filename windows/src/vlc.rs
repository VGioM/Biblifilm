//! libVLC dans la fenêtre : l'équivalent Windows de VLCKit sur Mac. VLC décode
//! (décodage matériel compris) et écrit chaque image dans un tampon ; l'image
//! est envoyée dans une texture OpenGL et copiée dans la fenêtre, à l'échelle,
//! puis l'interface du lecteur est dessinée par-dessus.
//!
//! libvlc.dll est chargée à l'exécution : celle livrée avec Biblifilm
//! (`vlc\libvlc.dll` à côté de l'exe, avec ses plugins), sinon celle de VLC
//! installé (`C:\Program Files\VideoLAN\VLC`).

#![allow(non_camel_case_types)]

use libloading::Library;
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type Ptr = *mut c_void;
type LockCb = extern "C" fn(Ptr, *mut Ptr) -> Ptr;
type UnlockCb = extern "C" fn(Ptr, Ptr, *const Ptr);
type DisplayCb = extern "C" fn(Ptr, Ptr);
type SetupCb = extern "C" fn(*mut Ptr, *mut c_char, *mut c_uint, *mut c_uint, *mut c_uint, *mut c_uint) -> c_uint;
type CleanupCb = extern "C" fn(Ptr);
type EventCb = extern "C" fn(*const c_void, Ptr);

#[repr(C)]
struct TrackDescription { id: c_int, name: *const c_char, next: *mut TrackDescription }

#[repr(C)]
struct MediaTrack {
    codec: u32, original_fourcc: u32, id: c_int, kind: c_int, profile: c_int, level: c_int,
    detail: Ptr, bitrate: c_uint, language: *const c_char, description: *const c_char,
}

const EV_PLAYING: c_int = 260;
const EV_END_REACHED: c_int = 265;
const EV_ERROR: c_int = 266;
const ST_OPENING: c_int = 1;
const ST_BUFFERING: c_int = 2;
const ST_PLAYING: c_int = 3;
const ST_PAUSED: c_int = 4;
const ST_ENDED: c_int = 6;
const ST_ERROR: c_int = 7;

struct Api {
    _lib: Library,
    new: unsafe extern "C" fn(c_int, *const *const c_char) -> Ptr,
    media_new_path: unsafe extern "C" fn(Ptr, *const c_char) -> Ptr,
    media_add_option: unsafe extern "C" fn(Ptr, *const c_char),
    media_release: unsafe extern "C" fn(Ptr),
    media_tracks_get: unsafe extern "C" fn(Ptr, *mut *mut *mut MediaTrack) -> c_uint,
    media_tracks_release: unsafe extern "C" fn(*mut *mut MediaTrack, c_uint),
    mp_new: unsafe extern "C" fn(Ptr) -> Ptr,
    mp_release: unsafe extern "C" fn(Ptr),
    mp_set_media: unsafe extern "C" fn(Ptr, Ptr),
    mp_play: unsafe extern "C" fn(Ptr) -> c_int,
    mp_stop: unsafe extern "C" fn(Ptr),
    mp_set_pause: unsafe extern "C" fn(Ptr, c_int),
    mp_get_time: unsafe extern "C" fn(Ptr) -> i64,
    mp_set_time: unsafe extern "C" fn(Ptr, i64),
    mp_get_length: unsafe extern "C" fn(Ptr) -> i64,
    mp_get_state: unsafe extern "C" fn(Ptr) -> c_int,
    mp_event_manager: unsafe extern "C" fn(Ptr) -> Ptr,
    event_attach: unsafe extern "C" fn(Ptr, c_int, EventCb, Ptr) -> c_int,
    audio_set_volume: unsafe extern "C" fn(Ptr, c_int) -> c_int,
    audio_get_track_description: unsafe extern "C" fn(Ptr) -> *mut TrackDescription,
    audio_get_track: unsafe extern "C" fn(Ptr) -> c_int,
    audio_set_track: unsafe extern "C" fn(Ptr, c_int) -> c_int,
    spu_description: unsafe extern "C" fn(Ptr) -> *mut TrackDescription,
    spu_get: unsafe extern "C" fn(Ptr) -> c_int,
    spu_set: unsafe extern "C" fn(Ptr, c_int) -> c_int,
    track_description_release: unsafe extern "C" fn(*mut TrackDescription),
    video_set_callbacks: unsafe extern "C" fn(Ptr, LockCb, UnlockCb, DisplayCb, Ptr),
    video_set_format_callbacks: unsafe extern "C" fn(Ptr, SetupCb, CleanupCb),
    instance: usize,
}

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

/// Où chercher libvlc.dll : celle livrée à côté de l'exe, puis VLC installé.
fn candidates() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        v.push(d.join("vlc").join("libvlc.dll"));
        // En développement : `cargo run` depuis windows/, libVLC dans extra/vlc.
        for up in d.ancestors().take(4) { v.push(up.join("extra").join("vlc").join("libvlc.dll")); }
    }
    if let Some(p) = std::env::var_os("ProgramFiles") { v.push(PathBuf::from(p).join(r"VideoLAN\VLC\libvlc.dll")); }
    v
}

fn load() -> Result<Api, String> {
    let Some(path) = candidates().into_iter().find(|p| p.is_file()) else {
        return Err("libVLC est introuvable (ni à côté de Biblifilm, ni dans VLC installé)".into());
    };
    let dir = path.parent().unwrap().to_path_buf();
    // libvlccore.dll est cherchée dans le dossier de libvlc.dll, et les plugins à côté.
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;
    let lib: Library = unsafe { libloading::os::windows::Library::load_with_flags(&path, LOAD_WITH_ALTERED_SEARCH_PATH) }
        .map_err(|e| format!("libVLC ne se charge pas ({e})"))?.into();
    std::env::set_var("VLC_PLUGIN_PATH", dir.join("plugins"));
    unsafe {
        macro_rules! sym { ($n:literal) => {{ let s: libloading::Symbol<_> = lib.get(concat!($n, "\0").as_bytes()).map_err(|e| format!("{} : {e}", $n))?; *s }} }
        let mut api = Api {
            new: sym!("libvlc_new"),
            media_new_path: sym!("libvlc_media_new_path"), media_add_option: sym!("libvlc_media_add_option"), media_release: sym!("libvlc_media_release"),
            media_tracks_get: sym!("libvlc_media_tracks_get"), media_tracks_release: sym!("libvlc_media_tracks_release"),
            mp_new: sym!("libvlc_media_player_new"), mp_release: sym!("libvlc_media_player_release"), mp_set_media: sym!("libvlc_media_player_set_media"),
            mp_play: sym!("libvlc_media_player_play"), mp_stop: sym!("libvlc_media_player_stop"), mp_set_pause: sym!("libvlc_media_player_set_pause"),
            mp_get_time: sym!("libvlc_media_player_get_time"), mp_set_time: sym!("libvlc_media_player_set_time"), mp_get_length: sym!("libvlc_media_player_get_length"),
            mp_get_state: sym!("libvlc_media_player_get_state"), mp_event_manager: sym!("libvlc_media_player_event_manager"), event_attach: sym!("libvlc_event_attach"),
            audio_set_volume: sym!("libvlc_audio_set_volume"), audio_get_track_description: sym!("libvlc_audio_get_track_description"),
            audio_get_track: sym!("libvlc_audio_get_track"), audio_set_track: sym!("libvlc_audio_set_track"),
            spu_description: sym!("libvlc_video_get_spu_description"), spu_get: sym!("libvlc_video_get_spu"), spu_set: sym!("libvlc_video_set_spu"),
            track_description_release: sym!("libvlc_track_description_list_release"),
            video_set_callbacks: sym!("libvlc_video_set_callbacks"), video_set_format_callbacks: sym!("libvlc_video_set_format_callbacks"),
            instance: 0, _lib: lib,
        };
        let trace = std::env::var("VLC_TRACE").is_ok();
        let mut args: Vec<CString> = ["--no-video-title-show", "--no-snapshot-preview", "--no-stats", "--avcodec-hw=any"].iter().map(|s| cstr(s)).collect();
        if trace { args.push(cstr("--verbose=2")); } else { args.push(cstr("--quiet")); }
        let ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
        let inst = (api.new)(ptrs.len() as c_int, ptrs.as_ptr());
        if inst.is_null() { return Err("libVLC n'a pas pu démarrer".into()); }
        api.instance = inst as usize;
        Ok(api)
    }
}

fn api() -> Result<&'static Api, String> { API.get_or_init(load).as_ref().map_err(|e| e.clone()) }

/// Charge libVLC dans un thread, pour que la fenêtre s'ouvre sans attendre
/// (la première ouverture lit tous les plugins).
pub fn preload() { std::thread::spawn(|| { let _ = api(); }); }

/// L'erreur de chargement, si le chargement est terminé et a échoué (sans bloquer).
pub fn load_error() -> Option<String> { API.get().and_then(|r| r.as_ref().err().cloned()) }

/// Le dossier de la libVLC chargée (ou livrée), pour « Ouvrir dans VLC ».
pub fn bundled_vlc_exe() -> Option<PathBuf> {
    candidates().into_iter().filter(|p| p.is_file()).map(|p| p.with_file_name("vlc.exe")).find(|p| p.is_file())
}

fn cstr(s: &str) -> CString { CString::new(s.replace('\0', "")).unwrap() }

// ── Les images : VLC écrit, la fenêtre lit ────────────────────────────────

#[derive(Default)]
struct Frame { w: u32, h: u32, data: Vec<u8>, serial: u64 }

/// Ce que les fonctions de rappel de VLC partagent avec l'app. Vit plus
/// longtemps que le lecteur VLC (libéré avant lui).
struct Shared {
    frame: Mutex<Frame>,
    /// Le tampon où VLC décode : seul le thread vidéo de VLC y touche.
    back: UnsafeCell<Vec<u8>>,
    size: Mutex<(u32, u32)>,
    serial: AtomicU64,
    ctx: egui::Context,
}
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

extern "C" fn cb_setup(opaque: *mut Ptr, chroma: *mut c_char, width: *mut c_uint, height: *mut c_uint, pitches: *mut c_uint, lines: *mut c_uint) -> c_uint {
    unsafe {
        let sh = &*(*opaque as *const Shared);
        let (w, h) = (*width, *height);
        if w == 0 || h == 0 { return 0; }
        ptr::copy_nonoverlapping(b"RV32".as_ptr() as *const c_char, chroma, 4);
        let pitch = w * 4;
        // Des lignes en plus : certains décodeurs écrivent un peu au-delà.
        let rows = (h + 31) & !31;
        *pitches = pitch;
        *lines = rows;
        *sh.back.get() = vec![0u8; (pitch * rows) as usize];
        *sh.size.lock().unwrap() = (w, h);
        1
    }
}
extern "C" fn cb_cleanup(_opaque: Ptr) {}
extern "C" fn cb_lock(opaque: Ptr, planes: *mut Ptr) -> Ptr {
    unsafe {
        let sh = &*(opaque as *const Shared);
        *planes = (*sh.back.get()).as_mut_ptr() as Ptr;
    }
    ptr::null_mut()
}
extern "C" fn cb_unlock(_opaque: Ptr, _pic: Ptr, _planes: *const Ptr) {}
extern "C" fn cb_display(opaque: Ptr, _pic: Ptr) {
    let sh = unsafe { &*(opaque as *const Shared) };
    let (w, h) = *sh.size.lock().unwrap();
    let n = (w * h * 4) as usize;
    let back = unsafe { &*sh.back.get() };
    if back.len() < n { return; }
    if let Ok(mut f) = sh.frame.lock() {
        if f.data.len() != n { f.data = vec![0u8; n]; }
        f.data.copy_from_slice(&back[..n]);
        f.w = w; f.h = h;
        f.serial = sh.serial.fetch_add(1, Ordering::Relaxed) + 1;
    }
    sh.ctx.request_repaint();
}
extern "C" fn cb_event(_ev: *const c_void, opaque: Ptr) {
    // Ici, aucun appel à libVLC (on est dans son thread) : on réveille seulement l'app.
    let sh = unsafe { &*(opaque as *const Shared) };
    sh.ctx.request_repaint();
}

/// Le dessin de l'image : une texture et le framebuffer qui la lit, créés au
/// premier dessin (il faut le contexte OpenGL courant).
#[derive(Default)]
pub struct Surface {
    shared: Option<Arc<Shared>>,
    gl: Option<(egui_glow::glow::NativeTexture, egui_glow::glow::NativeFramebuffer)>,
    tex_size: (u32, u32),
    serial: u64,
    closing: bool,
}

impl Surface {
    /// Copie l'image courante, à l'échelle et centrée, dans le tampon OpenGL actif (toute la fenêtre).
    pub fn render(&mut self, gl: &egui_glow::glow::Context, w: i32, h: i32, _egui_ctx: *const egui::Context) {
        use egui_glow::glow::{self, HasContext};
        if self.closing {
            if let Some((t, f)) = self.gl.take() { unsafe { gl.delete_texture(t); gl.delete_framebuffer(f); } }
            return;
        }
        let Some(sh) = self.shared.clone() else { return };
        let Ok(frame) = sh.frame.lock() else { return };
        if frame.w == 0 || frame.data.is_empty() { return; }
        unsafe {
            if self.gl.is_none() {
                let (Ok(t), Ok(f)) = (gl.create_texture(), gl.create_framebuffer()) else { return };
                self.gl = Some((t, f));
                self.tex_size = (0, 0);
            }
            let (tex, fbo) = self.gl.unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            if frame.serial != self.serial || self.tex_size != (frame.w, frame.h) {
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                if self.tex_size != (frame.w, frame.h) {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                    gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, frame.w as i32, frame.h as i32, 0, glow::BGRA, glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&frame.data)));
                    self.tex_size = (frame.w, frame.h);
                } else {
                    gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, frame.w as i32, frame.h as i32, glow::BGRA, glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&frame.data)));
                }
                self.serial = frame.serial;
            }
            let (fw, fh) = (frame.w as f32, frame.h as f32);
            drop(frame);
            let prev_read = gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(glow::READ_FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
            let scale = (w as f32 / fw).min(h as f32 / fh);
            let (dw, dh) = ((fw * scale).round() as i32, (fh * scale).round() as i32);
            let (x0, y0) = ((w - dw) / 2, (h - dh) / 2);
            // VLC écrit de haut en bas, OpenGL compte de bas en haut : on retourne l'image.
            gl.blit_framebuffer(0, 0, fw as i32, fh as i32, x0, y0 + dh, x0 + dw, y0, glow::COLOR_BUFFER_BIT, glow::LINEAR);
            let prev = std::num::NonZeroU32::new(prev_read as u32).map(glow::NativeFramebuffer);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, prev);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }
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

pub enum Event {
    /// Un fichier est ouvert (index dans la file).
    Loaded(usize),
    /// La fin réelle d'un fichier : l'épisode est vu.
    Eof(usize),
    /// Un fichier qui ne s'ouvre pas.
    Error(usize, String),
    /// La file est finie, ou le lecteur s'est arrêté.
    Finished,
}

pub struct Player {
    mp: Ptr,
    media: Ptr,
    shared: Arc<Shared>,
    files: Vec<String>,
    alang: String,
    pub surface: Arc<Mutex<Surface>>,
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
    /// Un fichier vient d'être demandé : l'état « fini » du précédent ne compte plus.
    starting: bool,
    done: bool,
    closed: bool,
    egui_ctx: Box<egui::Context>,
}

unsafe impl Send for Player {}

impl Player {
    pub fn new(files: &[String], start: f64, egui_ctx: &egui::Context, volume: f64, alang: &str) -> Result<Player, String> {
        let a = api()?;
        let mp = unsafe { (a.mp_new)(a.instance as Ptr) };
        if mp.is_null() { return Err("VLC n'a pas pu démarrer".into()); }
        let shared = Arc::new(Shared { frame: Mutex::default(), back: UnsafeCell::new(vec![]), size: Mutex::new((0, 0)), serial: AtomicU64::new(0), ctx: egui_ctx.clone() });
        let opaque = Arc::as_ptr(&shared) as Ptr;
        unsafe {
            (a.video_set_callbacks)(mp, cb_lock, cb_unlock, cb_display, opaque);
            (a.video_set_format_callbacks)(mp, cb_setup, cb_cleanup);
            let em = (a.mp_event_manager)(mp);
            for ev in [EV_PLAYING, EV_END_REACHED, EV_ERROR] { (a.event_attach)(em, ev, cb_event, opaque); }
            (a.audio_set_volume)(mp, volume.round() as c_int);
        }
        let surface = Arc::new(Mutex::new(Surface { shared: Some(shared.clone()), ..Default::default() }));
        let mut p = Player {
            mp, media: ptr::null_mut(), shared, files: files.to_vec(), alang: alang.to_string(), surface, index: 0, count: files.len(),
            position: 0.0, duration: 0.0, paused: false, volume, tracks: vec![], played: 0.0, loaded: false, starting: true, done: false, closed: false,
            egui_ctx: Box::new(egui_ctx.clone()),
        };
        p.open(0, start);
        Ok(p)
    }

    fn open(&mut self, i: usize, start: f64) {
        let Ok(a) = api() else { return };
        let Some(f) = self.files.get(i) else { return };
        unsafe {
            // libVLC ne reconnaît un chemin absolu Windows qu'avec des « \ » (« C:/… » échoue).
            let f = f.replace('/', "\\");
            let m = (a.media_new_path)(a.instance as Ptr, cstr(&f).as_ptr());
            if std::env::var("BF_DEBUG").is_ok() { eprintln!("vlc open {f} media={m:?}"); }
            if m.is_null() { return; }
            if start > 0.0 { (a.media_add_option)(m, cstr(&format!(":start-time={}", start.floor())).as_ptr()); }
            (a.media_add_option)(m, cstr(&format!(":audio-language={}", self.alang)).as_ptr());
            (a.media_add_option)(m, cstr(&format!(":sub-language={}", self.alang)).as_ptr());
            (a.mp_set_media)(self.mp, m);
            if !self.media.is_null() { (a.media_release)(self.media); }
            self.media = m;
            let r = (a.mp_play)(self.mp);
            if std::env::var("BF_DEBUG").is_ok() { eprintln!("vlc play -> {r}"); }
        }
        self.index = i;
        self.position = 0.0; self.duration = 0.0; self.played = 0.0; self.loaded = false; self.tracks.clear();
        self.starting = true;
        self.paused = false;
    }

    /// Le fichier suivant de la file, ou la fin.
    fn advance(&mut self, out: &mut Vec<Event>) {
        if self.index + 1 < self.count { let i = self.index + 1; self.open(i, 0.0); } else { self.done = true; out.push(Event::Finished); }
    }

    /// Gardé pour la compatibilité avec l'interface de mpv : seules quelques propriétés existent.
    pub fn command(&self, _args: &[&str]) {}

    pub fn set(&mut self, name: &str, value: &str) {
        if name == "pause" { self.set_pause(value == "yes"); }
    }

    pub fn get(&self, _name: &str) -> Option<String> { None }

    fn set_pause(&mut self, on: bool) {
        if let Ok(a) = api() { unsafe { (a.mp_set_pause)(self.mp, on as c_int) }; self.paused = on; }
    }

    pub fn toggle_pause(&mut self) { let p = !self.paused; self.set_pause(p); }

    pub fn seek_absolute(&mut self, s: f64) {
        let Ok(a) = api() else { return };
        let max = if self.duration > 1.0 { self.duration - 1.0 } else { f64::MAX };
        let t = s.clamp(0.0, max);
        unsafe { (a.mp_set_time)(self.mp, (t * 1000.0) as i64) };
        self.position = t;
    }
    pub fn seek_relative(&mut self, s: f64) { let t = self.position + s; self.seek_absolute(t); }

    pub fn set_volume(&mut self, v: f64) {
        self.volume = v.clamp(0.0, 130.0);
        if let Ok(a) = api() { unsafe { (a.audio_set_volume)(self.mp, self.volume.round() as c_int) }; }
    }

    pub fn next(&mut self) {
        let mut ev = vec![];
        if self.index + 1 < self.count { self.advance(&mut ev); }
    }

    pub fn select_track(&self, kind: &str, id: Option<i64>) {
        let Ok(a) = api() else { return };
        let id = id.map(|i| i as c_int).unwrap_or(-1);
        unsafe { if kind == "audio" { (a.audio_set_track)(self.mp, id); } else { (a.spu_set)(self.mp, id); } }
    }

    pub fn refresh_tracks(&mut self) {
        let Ok(a) = api() else { return };
        // La langue, le titre et le codec de chaque piste, d'après le fichier.
        let mut info: std::collections::HashMap<i64, (String, String, String)> = Default::default();
        if !self.media.is_null() {
            unsafe {
                let mut list: *mut *mut MediaTrack = ptr::null_mut();
                let n = (a.media_tracks_get)(self.media, &mut list);
                for i in 0..n as usize {
                    let t = &**list.add(i);
                    let s = |p: *const c_char| if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().to_string() };
                    let codec: String = t.codec.to_le_bytes().iter().filter(|b| b.is_ascii_graphic()).map(|&b| b as char).collect();
                    info.insert(t.id as i64, (s(t.description), s(t.language), codec.trim().to_lowercase()));
                }
                if n > 0 { (a.media_tracks_release)(list, n); }
            }
        }
        let mut v = vec![];
        let mut read = |kind: &str, list: *mut TrackDescription, cur: c_int| unsafe {
            let mut p = list;
            while !p.is_null() {
                let d = &*p;
                if d.id >= 0 {
                    let name = if d.name.is_null() { String::new() } else { CStr::from_ptr(d.name).to_string_lossy().to_string() };
                    let (title, lang, codec) = info.get(&(d.id as i64)).cloned().unwrap_or_default();
                    // Sans titre ni langue dans le fichier, le nom que VLC donne (« Piste 2 - [Anglais] »).
                    let title = if title.is_empty() && lang.is_empty() { name } else { title };
                    v.push(Track { id: d.id as i64, kind: kind.to_string(), title, lang, codec, selected: d.id == cur });
                }
                p = d.next;
            }
            if !list.is_null() { (a.track_description_release)(list); }
        };
        unsafe {
            read("audio", (a.audio_get_track_description)(self.mp), (a.audio_get_track)(self.mp));
            read("sub", (a.spu_description)(self.mp), (a.spu_get)(self.mp));
        }
        if std::env::var("BF_DEBUG").is_ok() { eprintln!("vlc tracks {:?}", v); }
        self.tracks = v;
    }

    /// Les événements arrivés depuis la dernière image (l'état de VLC, relu).
    pub fn poll(&mut self) -> Vec<Event> {
        let Ok(a) = api() else { return vec![Event::Finished] };
        let mut out = vec![];
        if self.closed || self.done { return out; }
        let st = unsafe { (a.mp_get_state)(self.mp) };
        if std::env::var("BF_DEBUG").is_ok() { eprintln!("vlc state={st} frames={} pos={:.1} dur={:.1}", self.shared.serial.load(Ordering::Relaxed), self.position, self.duration); }
        if self.starting {
            if [ST_OPENING, ST_BUFFERING, ST_PLAYING, ST_PAUSED].contains(&st) { self.starting = false; } else if st != ST_ERROR { return out; }
        }
        match st {
            ST_PLAYING | ST_PAUSED => {
                if st == ST_PLAYING || !self.loaded { self.paused = st == ST_PAUSED; }
                let len = unsafe { (a.mp_get_length)(self.mp) };
                if len > 0 { self.duration = len as f64 / 1000.0; }
                let t = unsafe { (a.mp_get_time)(self.mp) };
                if t >= 0 {
                    let t = t as f64 / 1000.0;
                    let step = t - self.position;
                    if !self.paused && step > 0.0 && step < 2.5 { self.played += step; }
                    self.position = t;
                }
                if !self.loaded && st == ST_PLAYING {
                    self.loaded = true;
                    self.refresh_tracks();
                    out.push(Event::Loaded(self.index));
                }
                // Le temps avance sans image nouvelle (pause, son seul) : on relit l'état.
                if !self.paused { self.egui_ctx.request_repaint_after(std::time::Duration::from_millis(250)); }
            }
            ST_ENDED => {
                self.loaded = false;
                if self.duration > 0.0 { self.position = self.duration; }
                out.push(Event::Eof(self.index));
                self.advance(&mut out);
            }
            ST_ERROR => {
                self.loaded = false;
                self.done = true;
                out.push(Event::Error(self.index, "fichier illisible".into()));
            }
            _ => { self.egui_ctx.request_repaint_after(std::time::Duration::from_millis(250)); }
        }
        out
    }

    /// Demande l'arrêt : le prochain dessin libère la texture, puis `can_drop` devient vrai.
    pub fn close(&mut self) {
        if let Ok(a) = api() { if !self.closed { unsafe { (a.mp_stop)(self.mp) }; } }
        self.closed = true;
        if let Ok(mut s) = self.surface.lock() { s.closing = true; }
    }
    /// Le contexte egui que le lecteur réveille : vit aussi longtemps que la séance.
    pub fn ctx_ptr(&self) -> usize { &*self.egui_ctx as *const egui::Context as usize }
    pub fn can_drop(&self) -> bool { self.surface.lock().map(|s| s.gl.is_none()).unwrap_or(true) }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Ok(a) = api() {
            unsafe {
                if !self.closed { (a.mp_stop)(self.mp); }
                // Après l'arrêt, VLC n'appelle plus nos fonctions : Shared peut partir ensuite.
                (a.mp_release)(self.mp);
                if !self.media.is_null() { (a.media_release)(self.media); }
            }
        }
        if let Ok(mut s) = self.surface.lock() { s.shared = None; }
        let _ = &self.shared;
    }
}
