//! Ce que les apps demandent au bureau Windows : ouvrir un lien ou un dossier,
//! montrer un fichier dans l'Explorateur, la corbeille, le presse-papiers, les
//! fenêtres de choix de dossier et la mise en veille.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Le programme `cmd` dans le PATH (avec les extensions de PATHEXT), s'il y est.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
        .split(';').filter(|e| !e.is_empty()).map(|e| e.to_lowercase()).collect();
    let has_ext = Path::new(cmd).extension().is_some();
    std::env::split_paths(&path).find_map(|d| {
        if has_ext { let p = d.join(cmd); if p.is_file() { return Some(p); } }
        exts.iter().map(|e| d.join(format!("{cmd}{e}"))).find(|p| p.is_file())
    })
}

/// Le tar de Windows (bsdtar, livré depuis Windows 10) : il lit zip, tar.gz,
/// tar.xz et 7z.
pub fn system_tar() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join("tar.exe")
}

/// Une commande sans fenêtre de console (pour les outils lancés en arrière-plan).
pub fn clean_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// Lance un programme détaché : fermer l'app ne le ferme pas.
pub fn spawn_detached(mut cmd: Command) -> std::io::Result<()> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}

fn shell_open(target: &std::ffi::OsStr) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &std::ffi::OsStr| { use std::os::windows::ffi::OsStrExt; s.encode_wide().chain(Some(0)).collect::<Vec<u16>>() };
    let verb = wide("open".as_ref());
    let file = wide(target);
    unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL); }
}

pub fn open_url(url: &str) {
    if url.starts_with("https://") || url.starts_with("steam://") {
        shell_open(url.as_ref());
    }
}

pub fn open_path(p: &Path) {
    shell_open(p.as_os_str());
}

/// Montre le fichier, sélectionné, dans l'Explorateur ; un dossier est ouvert.
pub fn reveal(p: &Path) {
    if p.is_dir() { return open_path(p); }
    if !p.exists() { return open_path(p.parent().unwrap_or(p)); }
    let mut c = Command::new("explorer.exe");
    c.raw_arg(format!("/select,\"{}\"", p.display()));
    let _ = c.spawn();
}

pub fn trash(p: &Path) -> Result<(), String> {
    trash::delete(p).map_err(|e| e.to_string())
}

pub fn copy_text(text: &str) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(text.to_string());
    }
}

/// Une fenêtre de choix, sans bloquer l'interface : le résultat arrive plus
/// tard sur le canal.
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

/// Empêche l'écran de s'éteindre tant que la valeur vit (PowerSetRequest).
pub struct Inhibit { handle: windows_sys::Win32::Foundation::HANDLE }

unsafe impl Send for Inhibit {}

impl Inhibit {
    pub fn new(app: &str, reason: &str) -> Inhibit {
        use windows_sys::Win32::System::Power::{PowerCreateRequest, PowerSetRequest, PowerRequestDisplayRequired, PowerRequestSystemRequired};
        use windows_sys::Win32::System::Threading::{REASON_CONTEXT, REASON_CONTEXT_0, POWER_REQUEST_CONTEXT_SIMPLE_STRING};
        let mut text: Vec<u16> = format!("{app} : {reason}").encode_utf16().chain(Some(0)).collect();
        let ctx = REASON_CONTEXT { Version: 0, Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING, Reason: REASON_CONTEXT_0 { SimpleReasonString: text.as_mut_ptr() } };
        unsafe {
            let h = PowerCreateRequest(&ctx);
            if !h.is_null() && h as isize != -1 {
                PowerSetRequest(h, PowerRequestDisplayRequired);
                PowerSetRequest(h, PowerRequestSystemRequired);
            }
            Inhibit { handle: h }
        }
    }
}

impl Drop for Inhibit {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Power::{PowerClearRequest, PowerRequestDisplayRequired, PowerRequestSystemRequired};
        if self.handle.is_null() || self.handle as isize == -1 { return; }
        unsafe {
            PowerClearRequest(self.handle, PowerRequestDisplayRequired);
            PowerClearRequest(self.handle, PowerRequestSystemRequired);
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
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
