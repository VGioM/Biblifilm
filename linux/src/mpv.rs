//! libmpv dans la fenêtre : l'équivalent Linux de VLCKit sur Mac. La vidéo est
//! rendue par mpv dans le contexte OpenGL de l'app (API de rendu de libmpv), et
//! l'interface du lecteur est dessinée par-dessus.
//!
//! libmpv est chargé à l'exécution (dlopen) : l'AppImage embarque la sienne,
//! construite avec un FFmpeg complet ; une libmpv du système est prise sinon.

#![allow(non_camel_case_types)]

use libloading::{Library, Symbol};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

type mpv_handle = c_void;
type mpv_render_context = c_void;

#[repr(C)]
struct mpv_event { event_id: c_int, error: c_int, reply_userdata: u64, data: *mut c_void }
#[repr(C)]
struct mpv_event_property { name: *const c_char, format: c_int, data: *mut c_void }
#[repr(C)]
struct mpv_event_end_file { reason: c_int, error: c_int, playlist_entry_id: i64, playlist_insert_id: i64, playlist_insert_num_entries: c_int }
#[repr(C)]
struct mpv_render_param { kind: c_int, data: *mut c_void }
#[repr(C)]
struct mpv_opengl_init_params { get_proc_address: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void, ctx: *mut c_void }
#[repr(C)]
struct mpv_opengl_fbo { fbo: c_int, w: c_int, h: c_int, internal_format: c_int }

const FORMAT_DOUBLE: c_int = 5;
const FORMAT_INT64: c_int = 4;
const FORMAT_FLAG: c_int = 3;
const EV_SHUTDOWN: c_int = 1;
const EV_END_FILE: c_int = 7;
const EV_FILE_LOADED: c_int = 8;
const EV_PROPERTY_CHANGE: c_int = 22;
const END_EOF: c_int = 0;
const END_ERROR: c_int = 4;

struct Api {
    _lib: Library,
    create: unsafe extern "C" fn() -> *mut mpv_handle,
    initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    set_option_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    get_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    command: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int,
    observe_property: unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int,
    wait_event: unsafe extern "C" fn(*mut mpv_handle, c_double) -> *mut mpv_event,
    set_wakeup_callback: unsafe extern "C" fn(*mut mpv_handle, Option<extern "C" fn(*mut c_void)>, *mut c_void),
    render_create: unsafe extern "C" fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut mpv_render_param) -> c_int,
    render_render: unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int,
    render_set_update_callback: unsafe extern "C" fn(*mut mpv_render_context, Option<extern "C" fn(*mut c_void)>, *mut c_void),
    render_update: unsafe extern "C" fn(*mut mpv_render_context) -> u64,
    render_free: unsafe extern "C" fn(*mut mpv_render_context),
}

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn load() -> Result<Api, String> {
    // Celle de l'AppImage d'abord (son AppRun la met dans LD_LIBRARY_PATH), puis celle du système.
    let mut last = String::new();
    let mut names: Vec<String> = vec![];
    if let Ok(dir) = std::env::var("APPDIR") { names.push(format!("{dir}/usr/lib/libmpv.so.2")); }
    names.extend(["libmpv.so.2", "libmpv.so.1", "libmpv.so"].map(String::from));
    for name in &names {
        match unsafe { Library::new(name.as_str()) } {
            Ok(lib) => unsafe {
                macro_rules! sym { ($n:literal) => {{ let s: Symbol<_> = lib.get($n).map_err(|e| e.to_string())?; *s }} }
                let api = Api {
                    create: sym!(b"mpv_create\0"), initialize: sym!(b"mpv_initialize\0"), terminate_destroy: sym!(b"mpv_terminate_destroy\0"),
                    set_option_string: sym!(b"mpv_set_option_string\0"), set_property_string: sym!(b"mpv_set_property_string\0"),
                    get_property_string: sym!(b"mpv_get_property_string\0"), free: sym!(b"mpv_free\0"), command: sym!(b"mpv_command\0"),
                    observe_property: sym!(b"mpv_observe_property\0"), wait_event: sym!(b"mpv_wait_event\0"),
                    set_wakeup_callback: sym!(b"mpv_set_wakeup_callback\0"), render_create: sym!(b"mpv_render_context_create\0"),
                    render_render: sym!(b"mpv_render_context_render\0"), render_set_update_callback: sym!(b"mpv_render_context_set_update_callback\0"),
                    render_update: sym!(b"mpv_render_context_update\0"), render_free: sym!(b"mpv_render_context_free\0"),
                    _lib: lib,
                };
                return Ok(api);
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("libmpv introuvable ({last})"))
}

fn api() -> Result<&'static Api, String> { API.get_or_init(load).as_ref().map_err(|e| e.clone()) }

pub fn available() -> Result<(), String> { api().map(|_| ()) }

// ── Le contexte OpenGL de la fenêtre, pour mpv ────────────────────────────

static EGL: OnceLock<Option<(Library, unsafe extern "C" fn(*const c_char) -> *mut c_void)>> = OnceLock::new();

extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let egl = EGL.get_or_init(|| unsafe {
        let lib = Library::new("libEGL.so.1").ok()?;
        let f: unsafe extern "C" fn(*const c_char) -> *mut c_void = *lib.get(b"eglGetProcAddress\0").ok()?;
        Some((lib, f))
    });
    match egl { Some((_, f)) => unsafe { f(name) }, None => ptr::null_mut() }
}

struct RenderCtx(*mut mpv_render_context);
unsafe impl Send for RenderCtx {}

/// Ce que le dessin de l'image partage avec la séance : le contexte de rendu,
/// créé au premier dessin (il faut le contexte OpenGL courant).
#[derive(Default)]
pub struct Surface { ctx: Option<RenderCtx>, handle: usize, failed: bool, closing: bool }

extern "C" fn repaint_cb(d: *mut c_void) {
    let ctx = unsafe { &*(d as *const egui::Context) };
    ctx.request_repaint();
}

impl Surface {
    /// Dessine l'image courante dans le tampon OpenGL actif (toute la fenêtre).
    pub fn render(&mut self, gl: &egui_glow::glow::Context, w: i32, h: i32, egui_ctx: *const egui::Context) {
        use egui_glow::glow::{self, HasContext};
        let Ok(a) = api() else { return };
        // La fin : le contexte de rendu se libère pendant que celui d'OpenGL est actif.
        if self.closing { if let Some(c) = self.ctx.take() { unsafe { (a.render_free)(c.0) }; } return; }
        if self.handle == 0 || self.failed { return; }
        if self.ctx.is_none() {
            let api_type = CString::new("opengl").unwrap();
            let mut init = mpv_opengl_init_params { get_proc_address, ctx: ptr::null_mut() };
            let mut params = [
                mpv_render_param { kind: 1, data: api_type.as_ptr() as *mut c_void },
                mpv_render_param { kind: 2, data: &mut init as *mut _ as *mut c_void },
                mpv_render_param { kind: 0, data: ptr::null_mut() },
            ];
            let mut rc: *mut mpv_render_context = ptr::null_mut();
            let r = unsafe { (a.render_create)(&mut rc, self.handle as *mut mpv_handle, params.as_mut_ptr()) };
            if r < 0 || rc.is_null() { self.failed = true; return; }
            unsafe { (a.render_set_update_callback)(rc, Some(repaint_cb), egui_ctx as *mut c_void) };
            self.ctx = Some(RenderCtx(rc));
        }
        let rc = self.ctx.as_ref().unwrap().0;
        unsafe { (a.render_update)(rc) };
        let fbo: i32 = unsafe { gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) };
        let mut f = mpv_opengl_fbo { fbo, w, h, internal_format: 0 };
        let mut flip: c_int = 1;
        // Ne jamais attendre l'heure d'affichage de l'image : le dessin de toute la
        // fenêtre passe par ici, il ne doit pas dépendre du cœur de mpv.
        let mut block: c_int = 0;
        let mut params = [
            mpv_render_param { kind: 3, data: &mut f as *mut _ as *mut c_void },
            mpv_render_param { kind: 4, data: &mut flip as *mut _ as *mut c_void },
            mpv_render_param { kind: 12, data: &mut block as *mut _ as *mut c_void },
            mpv_render_param { kind: 0, data: ptr::null_mut() },
        ];
        unsafe { (a.render_render)(rc, params.as_mut_ptr()) };
    }

    fn free(&mut self) {
        if let (Some(c), Ok(a)) = (self.ctx.take(), api()) { unsafe { (a.render_free)(c.0) }; }
        self.handle = 0;
        self.failed = false;
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
    /// La file est finie, ou mpv s'est arrêté.
    Finished,
}

pub struct Player {
    handle: *mut mpv_handle,
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
    egui_ctx: Box<egui::Context>,
}

unsafe impl Send for Player {}

fn cstr(s: &str) -> CString { CString::new(s.replace('\0', "")).unwrap() }

impl Player {
    pub fn new(files: &[String], start: f64, egui_ctx: &egui::Context, volume: f64, alang: &str) -> Result<Player, String> {
        let a = api()?;
        let h = unsafe { (a.create)() };
        if h.is_null() { return Err("mpv n'a pas pu démarrer".into()); }
        let opt = |k: &str, v: &str| unsafe { (a.set_option_string)(h, cstr(k).as_ptr(), cstr(v).as_ptr()) };
        opt("vo", "libmpv");
        // Décodage matériel par copie : sans interopérabilité OpenGL, qui sur une
        // carte NVIDIA (CUDA) bloquait le rendu en attendant le cœur, et inversement.
        opt("hwdec", "auto-copy-safe");
        opt("input-default-bindings", "no");
        opt("input-vo-keyboard", "no");
        opt("osc", "no");
        // MPV_TRACE=1 : le journal de mpv sur la sortie d'erreur, pour le diagnostic.
        if std::env::var("MPV_TRACE").is_ok() { opt("terminal", "yes"); opt("msg-level", "all=v"); } else { opt("terminal", "no"); }
        opt("idle", "yes");
        opt("keep-open", "no");
        opt("alang", alang);
        opt("slang", alang);
        opt("sub-auto", "fuzzy");
        opt("volume", &format!("{volume}"));
        opt("volume-max", "130");
        if start > 0.0 { opt("start", &format!("{}", start.floor())); }
        if unsafe { (a.initialize)(h) } < 0 { unsafe { (a.terminate_destroy)(h) }; return Err("mpv n'a pas pu démarrer".into()); }
        let ctx = Box::new(egui_ctx.clone());
        unsafe { (a.set_wakeup_callback)(h, Some(repaint_cb), &*ctx as *const egui::Context as *mut c_void) };
        for (i, (name, fmt)) in [("time-pos", FORMAT_DOUBLE), ("duration", FORMAT_DOUBLE), ("pause", FORMAT_FLAG), ("playlist-pos", FORMAT_INT64), ("volume", FORMAT_DOUBLE)].iter().enumerate() {
            unsafe { (a.observe_property)(h, i as u64 + 1, cstr(name).as_ptr(), *fmt) };
        }
        let surface = Arc::new(Mutex::new(Surface { ctx: None, handle: h as usize, failed: false, closing: false }));
        let p = Player { handle: h, surface, index: 0, count: files.len(), position: 0.0, duration: 0.0, paused: false, volume, tracks: vec![], played: 0.0, loaded: false, egui_ctx: ctx };
        for (i, f) in files.iter().enumerate() {
            p.command(&["loadfile", f, if i == 0 { "replace" } else { "append" }]);
        }
        Ok(p)
    }

    pub fn command(&self, args: &[&str]) {
        let Ok(a) = api() else { return };
        let owned: Vec<CString> = args.iter().map(|s| cstr(s)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(ptr::null());
        unsafe { (a.command)(self.handle, ptrs.as_mut_ptr()) };
    }

    pub fn set(&self, name: &str, value: &str) {
        if let Ok(a) = api() { unsafe { (a.set_property_string)(self.handle, cstr(name).as_ptr(), cstr(value).as_ptr()) }; }
    }

    pub fn get(&self, name: &str) -> Option<String> {
        let a = api().ok()?;
        let p = unsafe { (a.get_property_string)(self.handle, cstr(name).as_ptr()) };
        if p.is_null() { return None; }
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
        unsafe { (a.free)(p as *mut c_void) };
        Some(s)
    }

    pub fn toggle_pause(&self) { self.command(&["cycle", "pause"]); }
    pub fn seek_relative(&self, s: f64) { self.command(&["seek", &format!("{s}"), "relative"]); }
    pub fn seek_absolute(&self, s: f64) { self.command(&["seek", &format!("{s}"), "absolute"]); }
    pub fn set_volume(&mut self, v: f64) { self.volume = v.clamp(0.0, 130.0); self.set("volume", &format!("{}", self.volume)); }
    pub fn next(&self) { self.command(&["playlist-next", "force"]); }
    pub fn select_track(&self, kind: &str, id: Option<i64>) {
        let prop = if kind == "audio" { "aid" } else { "sid" };
        self.set(prop, &id.map(|i| i.to_string()).unwrap_or_else(|| "no".into()));
    }

    pub fn refresh_tracks(&mut self) {
        let n: usize = self.get("track-list/count").and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut v = vec![];
        for i in 0..n {
            let g = |k: &str| self.get(&format!("track-list/{i}/{k}")).unwrap_or_default();
            v.push(Track { id: g("id").parse().unwrap_or(0), kind: g("type"), title: g("title"), lang: g("lang"), codec: g("codec"), selected: g("selected") == "yes" });
        }
        self.tracks = v;
    }

    /// Les événements arrivés depuis la dernière image.
    pub fn poll(&mut self) -> Vec<Event> {
        let Ok(a) = api() else { return vec![Event::Finished] };
        let mut out = vec![];
        loop {
            let ev = unsafe { &*(a.wait_event)(self.handle, 0.0) };
            match ev.event_id {
                0 => break,
                EV_SHUTDOWN => { out.push(Event::Finished); break; }
                EV_PROPERTY_CHANGE => {
                    let p = unsafe { &*(ev.data as *const mpv_event_property) };
                    if p.data.is_null() { continue; }
                    let name = unsafe { CStr::from_ptr(p.name) }.to_string_lossy();
                    match (name.as_ref(), p.format) {
                        ("time-pos", FORMAT_DOUBLE) => {
                            let t = unsafe { *(p.data as *const f64) };
                            let step = t - self.position;
                            if step > 0.0 && step < 2.5 { self.played += step; }
                            self.position = t;
                        }
                        ("duration", FORMAT_DOUBLE) => self.duration = unsafe { *(p.data as *const f64) },
                        ("pause", FORMAT_FLAG) => self.paused = unsafe { *(p.data as *const c_int) } != 0,
                        ("volume", FORMAT_DOUBLE) => self.volume = unsafe { *(p.data as *const f64) },
                        ("playlist-pos", FORMAT_INT64) => {
                            let i = unsafe { *(p.data as *const i64) };
                            if i >= 0 && i as usize != self.index {
                                self.index = i as usize;
                                self.position = 0.0; self.duration = 0.0; self.played = 0.0; self.loaded = false;
                            }
                        }
                        _ => {}
                    }
                }
                EV_FILE_LOADED => {
                    // --start ne vaut que pour le premier fichier de la file.
                    self.set("start", "none");
                    self.loaded = true;
                    self.refresh_tracks();
                    out.push(Event::Loaded(self.index));
                }
                EV_END_FILE => {
                    let e = unsafe { &*(ev.data as *const mpv_event_end_file) };
                    self.loaded = false;
                    if e.reason == END_EOF { out.push(Event::Eof(self.index)); }
                    if e.reason == END_ERROR { out.push(Event::Error(self.index, "fichier illisible".into())); }
                    let left = self.get("playlist-count").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                    let pos = self.get("playlist-pos").and_then(|s| s.parse::<i64>().ok()).unwrap_or(-1);
                    if e.reason == END_EOF && (pos < 0 || pos + 1 >= left) && self.index + 1 >= self.count { out.push(Event::Finished); }
                }
                _ => {}
            }
        }
        out
    }
}

impl Player {
    /// Demande l'arrêt : le prochain dessin libère le rendu, puis `can_drop` devient vrai.
    pub fn close(&self) { self.command(&["stop"]); if let Ok(mut s) = self.surface.lock() { s.closing = true; } }
    /// Le contexte egui que mpv réveille : vit aussi longtemps que la séance.
    pub fn ctx_ptr(&self) -> usize { &*self.egui_ctx as *const egui::Context as usize }
    pub fn can_drop(&self) -> bool { self.surface.lock().map(|s| s.ctx.is_none()).unwrap_or(true) }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Ok(a) = api() {
            if let Ok(mut s) = self.surface.lock() { s.free(); }
            unsafe { (a.terminate_destroy)(self.handle) };
        }
        let _ = &self.egui_ctx;
    }
}
