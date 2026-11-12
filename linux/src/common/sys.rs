//! Ce que les apps demandent au bureau : ouvrir un lien ou un dossier, montrer
//! un fichier dans le gestionnaire de fichiers, la corbeille, le presse-papiers,
//! les fenêtres de choix de dossier (portails XDG) et la mise en veille.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

/// Le programme `cmd` dans le PATH, s'il y est.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(cmd)).find(|p| {
        p.is_file() && std::fs::metadata(p).map(|m| { use std::os::unix::fs::PermissionsExt; m.permissions().mode() & 0o111 != 0 }).unwrap_or(false)
    })
}

/// L'environnement sans les variables propres à l'AppImage : un programme lancé
/// d'ici ne doit pas charger nos bibliothèques à la place des siennes.
pub fn clean_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(program);
    if std::env::var_os("APPIMAGE").is_some() {
        for v in ["LD_LIBRARY_PATH", "LD_PRELOAD", "GDK_PIXBUF_MODULE_FILE", "GIO_MODULE_DIR", "GSETTINGS_SCHEMA_DIR", "PYTHONHOME", "PYTHONPATH", "PERLLIB", "QT_PLUGIN_PATH"] {
            c.env_remove(v);
        }
        if let Ok(orig) = std::env::var("APPIMAGE_ORIGINAL_LD_LIBRARY_PATH") { c.env("LD_LIBRARY_PATH", orig); }
    }
    c
}

/// Lance un programme détaché : fermer l'app ne le ferme pas.
pub fn spawn_detached(mut cmd: Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe { cmd.pre_exec(|| { libc_setsid(); Ok(()) }); }
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}

extern "C" { fn setsid() -> i32; }
fn libc_setsid() { unsafe { setsid(); } }

pub fn open_url(url: &str) {
    if url.starts_with("https://") || url.starts_with("steam://") {
        let mut c = clean_command("xdg-open");
        c.arg(url);
        let _ = spawn_detached(c);
    }
}

pub fn open_path(p: &Path) {
    let mut c = clean_command("xdg-open");
    c.arg(p);
    let _ = spawn_detached(c);
}

fn file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) { s.push(b as char) } else { s.push_str(&format!("%{b:02X}")) }
    }
    s
}

/// Montre le fichier, sélectionné, dans le gestionnaire de fichiers (Dolphin,
/// Nautilus… par l'interface D-Bus FileManager1) ; sinon ouvre son dossier.
pub fn reveal(p: &Path) {
    let uri = file_uri(p);
    let ok = clean_command("dbus-send")
        .args(["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "--type=method_call",
               "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems", &format!("array:string:{uri}"), "string:"])
        .stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        open_path(if p.is_dir() { p } else { p.parent().unwrap_or(p) });
    }
}

pub fn trash(p: &Path) -> Result<(), String> {
    trash::delete(p).map_err(|e| e.to_string())
}

pub fn copy_text(text: &str) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(text.to_string());
    }
}

/// Une fenêtre de choix (portail XDG), sans bloquer l'interface : le résultat
/// arrive plus tard sur le canal.
pub enum Pick { Folder, Folders, File, Files }

pub fn pick(kind: Pick, title: &str, start: Option<&Path>, filters: &[(&str, &[&str])]) -> mpsc::Receiver<Vec<PathBuf>> {
    let (tx, rx) = mpsc::channel();
    let title = title.to_string();
    let start = start.map(|p| p.to_path_buf());
    let filters: Vec<(String, Vec<String>)> = filters.iter().map(|(n, e)| (n.to_string(), e.iter().map(|s| s.to_string()).collect())).collect();
    std::thread::spawn(move || {
        let mut d = rfd::FileDialog::new().set_title(&title);
        if let Some(s) = start { d = d.set_directory(s); }
        for (n, e) in &filters { d = d.add_filter(n, e); }
        let out = match kind {
            Pick::Folder => d.pick_folder().into_iter().collect(),
            Pick::Folders => d.pick_folders().unwrap_or_default(),
            Pick::File => d.pick_file().into_iter().collect(),
            Pick::Files => d.pick_files().unwrap_or_default(),
        };
        let _ = tx.send(out);
    });
    rx
}

/// Empêche l'écran de s'éteindre tant que la valeur vit (org.freedesktop.ScreenSaver).
pub struct Inhibit { child: Option<std::process::Child> }

impl Inhibit {
    pub fn new(app: &str, reason: &str) -> Inhibit {
        // gdbus garde la connexion ouverte tant qu'il tourne : l'inhibition dure
        // exactement aussi longtemps que lui.
        let script = format!(
            "gdbus call --session --dest org.freedesktop.ScreenSaver --object-path /org/freedesktop/ScreenSaver --method org.freedesktop.ScreenSaver.Inhibit '{app}' '{reason}' >/dev/null && exec sleep infinity"
        );
        let child = clean_command("sh").args(["-c", &script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
        Inhibit { child }
    }
}

impl Drop for Inhibit {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() { let _ = c.kill(); let _ = c.wait(); }
    }
}

/// Octets → « 1,2 Go » (puissances de 1000, comme le Finder).
pub fn bytes(n: u64) -> String {
    if n == 0 { return "0 o".into(); }
    let units = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < units.len() - 1 { v /= 1000.0; i += 1; }
    let s = if v >= 100.0 || i == 0 { format!("{v:.0}") } else { format!("{v:.1}") };
    format!("{} {}", s.replace('.', ","), units[i])
}

/// « il y a 3 jours »
pub fn relative(ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let d = (now - ms) / 1000;
    let plural = |n: i64, w: &str| format!("il y a {n} {w}{}", if n > 1 && !w.ends_with('s') { "s" } else { "" });
    match d {
        d if d < 60 => "à l'instant".into(),
        d if d < 3600 => plural(d / 60, "minute"),
        d if d < 86400 => plural(d / 3600, "heure"),
        d if d < 30 * 86400 => if d / 86400 == 1 { "hier".into() } else { plural(d / 86400, "jour") },
        d if d < 365 * 86400 => format!("il y a {} mois", d / (30 * 86400)),
        d => plural(d / (365 * 86400), "an"),
    }
}

pub fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }
