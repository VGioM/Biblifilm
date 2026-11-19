//! La lecture se fait dans VLC. Biblifilm l'installe s'il manque (le Flatpak de
//! Flathub, la façon propre sur un système immuable comme Bazzite), le tient à
//! jour, et suit la lecture par l'interface web locale de VLC (127.0.0.1, port
//! et mot de passe tirés au hasard) pour garder la reprise et les épisodes vus.

use crate::common::sys;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub const FLATPAK_ID: &str = "org.videolan.VLC";
/// Une mise à jour de VLC au plus par jour.
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scope { System, User }

impl Scope {
    fn flag(self) -> &'static str { match self { Scope::System => "--system", Scope::User => "--user" } }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Kind { Flatpak(Scope), Native(PathBuf) }

fn flatpak_dir(scope: Scope) -> Option<PathBuf> {
    match scope {
        Scope::System => Some(PathBuf::from("/var/lib/flatpak")),
        Scope::User => dirs::data_dir().map(|d| d.join("flatpak")),
    }
}

fn flatpak_installed(scope: Scope) -> bool {
    flatpak_dir(scope).map(|d| d.join("app").join(FLATPAK_ID).join("current").exists()).unwrap_or(false)
}

/// Le VLC à utiliser : le Flatpak d'abord (c'est lui que Biblifilm maintient), sinon celui du système.
pub fn find() -> Option<Kind> {
    if flatpak_installed(Scope::System) { return Some(Kind::Flatpak(Scope::System)); }
    if flatpak_installed(Scope::User) { return Some(Kind::Flatpak(Scope::User)); }
    sys::which("vlc").map(Kind::Native)
}

fn flatpak(args: &[&str]) -> Result<String, String> {
    let out = sys::clean_command("flatpak").args(args).env("LC_ALL", "C").stdin(Stdio::null()).output()
        .map_err(|e| format!("flatpak introuvable ({e})"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() { Ok(text) } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("échec").trim().to_string())
    }
}

/// Installe le Flatpak de VLC : pour tout le système (sans mot de passe pour un
/// administrateur sur Fedora/Bazzite), sinon pour l'utilisateur seul.
pub fn install() -> Result<Scope, String> {
    if sys::which("flatpak").is_none() {
        return Err("Flatpak n'est pas disponible : installez VLC avec le gestionnaire de paquets de votre distribution.".into());
    }
    let system = flatpak(&["install", "--system", "--noninteractive", "-y", "flathub", FLATPAK_ID]);
    if system.is_ok() && flatpak_installed(Scope::System) { return Ok(Scope::System); }
    let _ = flatpak(&["remote-add", "--user", "--if-not-exists", "flathub", "https://dl.flathub.org/repo/flathub.flatpakrepo"]);
    match flatpak(&["install", "--user", "--noninteractive", "-y", "flathub", FLATPAK_ID]) {
        Ok(_) if flatpak_installed(Scope::User) => Ok(Scope::User),
        Ok(_) => Err("l'installation n'a rien installé".into()),
        Err(e) => Err(system.err().map(|s| format!("{s} / {e}")).unwrap_or(e)),
    }
}

/// Met le Flatpak de VLC à jour. Sans risque pendant une lecture : la nouvelle
/// version sert au lancement suivant.
pub fn update(scope: Scope) -> Result<(), String> {
    flatpak(&["update", scope.flag(), "--noninteractive", "-y", FLATPAK_ID]).map(|_| ())
}

/// « 3.0.21 », lu dans les métadonnées du Flatpak ou demandé au VLC du système.
pub fn version(kind: &Kind) -> Option<String> {
    match kind {
        Kind::Flatpak(scope) => {
            let out = flatpak(&["info", scope.flag(), FLATPAK_ID]).ok()?;
            out.lines().find_map(|l| l.trim().strip_prefix("Version:").map(|v| v.trim().to_string()))
        }
        Kind::Native(path) => {
            let out = sys::clean_command(path).arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
            String::from_utf8_lossy(&out.stdout).split_whitespace().nth(2).map(str::to_string)
        }
    }
}

// ── L'état de VLC, partagé avec l'interface ─────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
pub enum State {
    Checking,
    Missing,
    Installing,
    Ready { kind: Kind, version: Option<String>, updating: bool },
    Failed(String),
}

#[derive(Clone)]
pub struct Manager { state: Arc<Mutex<State>>, stamp: PathBuf }

impl Manager {
    /// Cherche VLC en fond, puis le met à jour si la dernière vérification date de plus d'un jour.
    pub fn new(support: &Path, ctx: &egui::Context) -> Manager {
        let m = Manager { state: Arc::new(Mutex::new(State::Checking)), stamp: support.join("vlc-last-update") };
        let (m2, ctx) = (m.clone(), ctx.clone());
        std::thread::spawn(move || {
            m2.refresh();
            ctx.request_repaint();
            m2.update_if_due(&ctx, false);
        });
        m
    }

    pub fn state(&self) -> State { self.state.lock().unwrap().clone() }
    fn set(&self, s: State) { *self.state.lock().unwrap() = s; }

    pub fn kind(&self) -> Option<Kind> { match self.state() { State::Ready { kind, .. } => Some(kind), _ => None } }

    fn refresh(&self) {
        self.set(match find() {
            Some(kind) => State::Ready { version: version(&kind), kind, updating: false },
            None => State::Missing,
        });
    }

    /// Lance l'installation ; `done` est appelé ensuite, réussie ou non.
    pub fn install(&self, ctx: &egui::Context, done: impl FnOnce(Result<(), String>) + Send + 'static) {
        if matches!(self.state(), State::Installing) { return; }
        self.set(State::Installing);
        let (m, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = install().map(|_| ());
            match &r {
                Ok(()) => { m.refresh(); let _ = std::fs::write(&m.stamp, sys::now_ms().to_string()); }
                Err(e) => m.set(State::Failed(e.clone())),
            }
            done(r);
            ctx.request_repaint();
        });
    }

    /// `force` : le bouton des réglages ; sinon une fois par jour.
    pub fn update_if_due(&self, ctx: &egui::Context, force: bool) {
        let State::Ready { kind: Kind::Flatpak(scope), version, updating: false } = self.state() else { return };
        let last = std::fs::read_to_string(&self.stamp).ok().and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0);
        if !force && sys::now_ms() - last < UPDATE_EVERY.as_millis() as i64 { return; }
        self.set(State::Ready { kind: Kind::Flatpak(scope), version, updating: true });
        ctx.request_repaint();
        let (m, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            let ok = update(scope).is_ok();
            if ok { let _ = std::fs::write(&m.stamp, sys::now_ms().to_string()); }
            m.refresh();
            ctx.request_repaint();
        });
    }
}

// ── Une lecture dans VLC ────────────────────────────────────────────────────

pub enum Event {
    /// L'état courant : fichier de la file, position et durée (s), en lecture ou non.
    Status { index: usize, position: f64, duration: f64, playing: bool },
    /// Le fichier `index` est allé jusqu'au bout (pas sauté ni fermé en cours).
    Ended { index: usize, duration: f64 },
    /// VLC est fermé.
    Exited,
}

pub struct Player {
    kind: Kind,
    child: Arc<Mutex<Child>>,
    rx: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
}

/// En deçà de la fin, un fichier qui change ou se ferme a été lu jusqu'au bout.
const END_MARGIN: f64 = 5.0;

impl Player {
    /// Ouvre la file dans une nouvelle fenêtre VLC, le premier fichier à `start` secondes.
    pub fn spawn(kind: &Kind, files: &[String], start: f64, fullscreen: bool, ctx: &egui::Context) -> Result<Player, String> {
        let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port())
            .map_err(|e| format!("aucun port local libre ({e})"))?;
        let password = random_password();
        let mut c = match kind {
            Kind::Flatpak(_) => {
                let mut c = sys::clean_command("flatpak");
                c.arg("run");
                // VLC voit déjà le disque, sauf quelques dossiers (/tmp…) : on ouvre ceux de la file.
                let mut dirs: Vec<&Path> = files.iter().filter_map(|f| Path::new(f).parent()).collect();
                dirs.dedup();
                for d in dirs { c.arg(format!("--filesystem={}:ro", d.display())); }
                c.arg(FLATPAK_ID);
                c
            }
            Kind::Native(p) => sys::clean_command(p),
        };
        c.args(["--no-one-instance", "--no-playlist-enqueue", "--play-and-exit", "--no-random", "--no-loop", "--no-repeat",
            "--no-qt-privacy-ask", "--qt-continue=0", "--audio-language=fre,fra,fr", "--extraintf=http",
            "--http-host=127.0.0.1"]);
        c.arg(format!("--http-port={port}")).arg(format!("--http-password={password}"));
        if fullscreen { c.arg("--fullscreen"); }
        for (k, f) in files.iter().enumerate() {
            c.arg(f);
            // Option propre à ce fichier : les suivants commencent au début.
            if k == 0 && start > 0.0 { c.arg(format!(":start-time={}", start.floor())); }
        }
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = c.spawn().map_err(|e| format!("VLC ne démarre pas ({e})"))?;
        let child = Arc::new(Mutex::new(child));
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (c2, s2, ctx) = (child.clone(), stop.clone(), ctx.clone());
        std::thread::spawn(move || watch(port, password, c2, s2, tx, ctx));
        Ok(Player { kind: kind.clone(), child, rx, stop })
    }

    pub fn poll(&self) -> Vec<Event> { self.rx.try_iter().collect() }

    /// Ferme la fenêtre VLC de cette lecture.
    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Kind::Flatpak(_) = self.kind {
            // Tuer « flatpak run » ne ferme pas toujours VLC dans son bac à sable.
            let _ = sys::clean_command("flatpak").args(["kill", FLATPAK_ID]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
        if let Ok(mut c) = self.child.lock() { let _ = c.kill(); let _ = c.wait(); }
    }
}

fn random_password() -> String {
    use sha2::{Digest, Sha256};
    let seed = format!("{:?}{}{:?}", Instant::now(), std::process::id(), std::time::SystemTime::now());
    Sha256::digest(seed.as_bytes()).iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for ch in data.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= ch.len() { out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char) } else { out.push('=') }
        }
    }
    out
}

/// Les identifiants des fichiers de la liste de lecture, dans l'ordre.
fn playlist_ids(v: &Value) -> Vec<i64> {
    fn leaves(v: &Value, out: &mut Vec<i64>) {
        match v["children"].as_array() {
            Some(ch) => for c in ch { leaves(c, out) },
            None => if let Some(id) = v["id"].as_str().and_then(|s| s.parse().ok()).or_else(|| v["id"].as_i64()) { out.push(id) },
        }
    }
    let mut out = vec![];
    // La racine contient « Liste de lecture » puis « Médiathèque » : seule la première compte.
    if let Some(first) = v["children"].as_array().and_then(|c| c.first()) { leaves(first, &mut out); }
    out
}

/// Suit la lecture une fois par seconde, jusqu'à la fermeture de VLC.
fn watch(port: u16, password: String, child: Arc<Mutex<Child>>, stop: Arc<AtomicBool>, tx: mpsc::Sender<Event>, ctx: egui::Context) {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(2))).http_status_as_error(false).build().into();
    let auth = format!("Basic {}", base64(format!(":{password}").as_bytes()));
    let get = |path: &str| -> Option<Value> {
        let mut r = agent.get(&format!("http://127.0.0.1:{port}/requests/{path}")).header("Authorization", &auth).call().ok()?;
        if r.status() != 200 { return None; }
        r.body_mut().read_json::<Value>().ok()
    };
    let mut ids: Vec<i64> = vec![];
    // Le dernier état connu : fichier, position, durée.
    let mut last: Option<(usize, f64, f64)> = None;
    let send = |e: Event| { let _ = tx.send(e); ctx.request_repaint(); };
    loop {
        if stop.load(Ordering::SeqCst) { return; }
        let exited = child.lock().map(|mut c| !matches!(c.try_wait(), Ok(None))).unwrap_or(true);
        if exited {
            if let Some((index, pos, dur)) = last { if dur > 0.0 && dur - pos <= END_MARGIN { send(Event::Ended { index, duration: dur }); } }
            send(Event::Exited);
            return;
        }
        if let Some(st) = get("status.json") {
            let plid = st["currentplid"].as_i64().unwrap_or(-1);
            if plid >= 0 && !ids.contains(&plid) { if let Some(pl) = get("playlist.json") { ids = playlist_ids(&pl); } }
            if let Some(index) = ids.iter().position(|&x| x == plid) {
                let dur = st["length"].as_f64().unwrap_or(0.0);
                let pos = st["position"].as_f64().map(|p| p * dur).filter(|_| dur > 0.0).unwrap_or_else(|| st["time"].as_f64().unwrap_or(0.0));
                if let Some((prev, ppos, pdur)) = last {
                    if prev != index && pdur > 0.0 && pdur - ppos <= END_MARGIN { send(Event::Ended { index: prev, duration: pdur }); }
                }
                let playing = st["state"].as_str() == Some("playing");
                if dur > 0.0 { last = Some((index, pos, dur)); }
                send(Event::Status { index, position: pos, duration: dur, playing });
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b":secret"), "OnNlY3JldA==");
    }

    #[test]
    fn playlist_order() {
        let v: Value = serde_json::from_str(r#"{"children":[
            {"name":"Liste de lecture","id":"1","children":[{"id":"4","uri":"a"},{"id":"5","uri":"b"}]},
            {"name":"Médiathèque","id":"2","children":[{"id":"9"}]}]}"#).unwrap();
        assert_eq!(playlist_ids(&v), vec![4, 5]);
    }
}
