//! Met l'app à jour depuis ses releases GitHub (version Linux de
//! SelfUpdate.swift). Le même fichier vit dans Biblifilm, Biblijeu et Recomps.
//!
//! Au lancement (au plus une fois toutes les 20 h, sauf si la recherche
//! automatique est désactivée), l'app lit la dernière release de son dépôt. Une
//! même release porte un fichier par plateforme : seul
//! `<App>-<version>-linux-x86_64.AppImage` est pris ici, jamais le zip macOS. Si
//! la version est plus récente, l'app propose de l'installer : l'AppImage est
//! téléchargée à côté de celle en place, vérifiée (empreinte SHA-256 publiée par
//! GitHub, en-tête ELF, taille), rendue exécutable, puis échangée avec
//! l'ancienne par un simple renommage, et l'app se relance.

use super::store::Settings;
use super::theme::Palette;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

pub const MENU_TITLE: &str = "Rechercher les mises à jour…";
pub const AUTO_TITLE: &str = "Rechercher automatiquement les mises à jour";
const K_AUTO: &str = "SelfUpdate.automatic";
const K_LAST: &str = "SelfUpdate.lastCheck";
const K_SKIP: &str = "SelfUpdate.skippedVersion";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub download: String,
    pub size: u64,
    pub digest: Option<String>,
}

#[derive(Deserialize)]
struct GhAsset { name: String, browser_download_url: String, size: u64, digest: Option<String> }
#[derive(Deserialize)]
struct GhRelease { tag_name: String, html_url: Option<String>, body: Option<String>, assets: Vec<GhAsset> }

pub fn clean(tag: &str) -> String {
    let t = tag.trim();
    t.strip_prefix('v').or_else(|| t.strip_prefix('V')).unwrap_or(t).to_string()
}

fn parts(v: &str) -> Vec<u64> {
    clean(v).split('.').map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)).collect()
}

/// Vrai quand `a` est strictement plus récente que `b`.
pub fn is_newer(a: &str, b: &str) -> bool {
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q { return p > q; }
    }
    false
}

/// L'AppImage Linux de la release, et elle seule.
pub fn linux_asset(names: &[String]) -> Option<usize> {
    names.iter().position(|n| n.to_lowercase().ends_with("-linux-x86_64.appimage"))
        .or_else(|| names.iter().position(|n| { let l = n.to_lowercase(); l.ends_with(".appimage") && (l.contains("x86_64") || l.contains("amd64") || l.contains("x64")) }))
}

/// Les notes telles qu'on les lit, sans le Markdown ni les parties « Installation » et « Linux ».
pub fn readable_notes(body: &str) -> String {
    let mut text = body.replace("\r\n", "\n");
    for cut in ["## Installation", "## Linux"] {
        if let Some(i) = text.find(cut) { text.truncate(i); }
    }
    text.lines().map(|l| {
        let mut s = l.trim_start_matches('#').to_string();
        if let Some(r) = s.strip_prefix("- ") { s = format!("• {r}"); }
        s.replace("**", "").replace('`', "").trim().to_string()
    }).collect::<Vec<_>>().join("\n").trim().to_string()
}

fn latest(repo: &str, app: &str) -> Result<Release, String> {
    let agent = super::images::http_agent();
    let mut resp = agent.get(&format!("https://api.github.com/repos/{repo}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", &format!("{app}-updater"))
        .call().map_err(|e| format!("GitHub n'a pas répondu correctement ({e})."))?;
    let rel: GhRelease = resp.body_mut().read_json().map_err(|_| "GitHub n'a pas répondu correctement (réponse illisible).".to_string())?;
    let names: Vec<String> = rel.assets.iter().map(|a| a.name.clone()).collect();
    let i = linux_asset(&names).ok_or("La dernière release ne contient pas encore de version Linux.")?;
    let a = &rel.assets[i];
    Ok(Release {
        version: clean(&rel.tag_name),
        notes: readable_notes(rel.body.as_deref().unwrap_or("")),
        page: rel.html_url.clone().unwrap_or_else(|| format!("https://github.com/{repo}/releases")),
        download: a.browser_download_url.clone(),
        size: a.size,
        digest: a.digest.as_ref().and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_lowercase()),
    })
}

/// Télécharge, vérifie et met la nouvelle AppImage à la place de `target`.
fn install(rel: &Release, target: &Path, progress: impl Fn(f32)) -> Result<(), String> {
    let dir = target.parent().ok_or("emplacement inconnu")?;
    let writable = |p: &Path| std::fs::metadata(p).map(|m| !m.permissions().readonly()).unwrap_or(false);
    if !writable(dir) || !writable(target) {
        return Err("Impossible de remplacer l'app à son emplacement actuel (droits insuffisants).".into());
    }
    let staging = dir.join(format!(".{}.update-{}", target.file_name().unwrap().to_string_lossy(), std::process::id()));
    let result = (|| -> Result<(), String> {
        let agent = super::images::http_agent();
        let resp = agent.get(&rel.download).call().map_err(|e| format!("Téléchargement impossible ({e})."))?;
        let mut reader = resp.into_body().into_reader();
        let mut out = std::fs::File::create(&staging).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut done: u64 = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if rel.size > 0 { progress(done as f32 / rel.size as f32); }
        }
        out.sync_all().ok();
        drop(out);
        if rel.size > 0 && done != rel.size { return Err("Le fichier téléchargé est incomplet.".into()); }
        let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if let Some(d) = &rel.digest { if &hex != d { return Err("L'empreinte du fichier téléchargé ne correspond pas à celle publiée sur GitHub.".into()); } }
        let mut head = [0u8; 4];
        std::fs::File::open(&staging).and_then(|mut f| f.read_exact(&mut head)).map_err(|e| e.to_string())?;
        if head != [0x7f, b'E', b'L', b'F'] { return Err("Le fichier téléchargé n'est pas une AppImage.".into()); }
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        std::fs::rename(&staging, target).map_err(|e| e.to_string())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&staging); }
    result
}

enum Msg { Found(Release, bool), UpToDate(bool), Failed(String, bool), Progress(f32), Installed, InstallFailed(String) }

enum Dialog {
    None,
    Offer(Release),
    Downloading(f32),
    Info(String, String),
    InstallError(String, String),
}

pub struct SelfUpdate {
    repo: String,
    app: String,
    version: String,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    dialog: Dialog,
    busy: bool,
    started: std::time::Instant,
    auto_done: bool,
    pub relaunch_requested: bool,
}

impl SelfUpdate {
    pub fn new(repo: &str, app: &str) -> Self {
        let (tx, rx) = mpsc::channel();
        SelfUpdate { repo: repo.into(), app: app.into(), version: env!("CARGO_PKG_VERSION").into(), tx, rx, dialog: Dialog::None,
            busy: false, started: std::time::Instant::now(), auto_done: false, relaunch_requested: false }
    }

    pub fn automatic(settings: &Settings) -> bool { settings.get(K_AUTO).as_bool().unwrap_or(true) }
    pub fn set_automatic(settings: &mut Settings, on: bool) { settings.set(K_AUTO, on); }
    pub fn version(&self) -> &str { &self.version }

    pub fn check_now(&mut self, ctx: &egui::Context) { self.check(ctx, true); }

    fn check(&mut self, ctx: &egui::Context, interactive: bool) {
        if self.busy { return; }
        self.busy = true;
        let (repo, app, tx, ctx) = (self.repo.clone(), self.app.clone(), self.tx.clone(), ctx.clone());
        let current = self.version.clone();
        std::thread::spawn(move || {
            let msg = match latest(&repo, &app) {
                Ok(r) if is_newer(&r.version, &current) => Msg::Found(r, interactive),
                Ok(_) => Msg::UpToDate(interactive),
                Err(e) => Msg::Failed(e, interactive),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// À appeler à chaque image : lance la vérification du jour, affiche les dialogues.
    pub fn ui(&mut self, ctx: &egui::Context, settings: &mut Settings, p: &Palette) {
        // Laisse la fenêtre s'ouvrir avant de solliciter le réseau.
        if !self.auto_done && self.started.elapsed().as_secs() >= 4 {
            self.auto_done = true;
            let last = settings.get(K_LAST).as_i64().unwrap_or(0);
            if Self::automatic(settings) && super::sys::now_ms() - last > 20 * 3600 * 1000 { self.check(ctx, false); }
        } else if !self.auto_done { ctx.request_repaint_after(std::time::Duration::from_secs(1)); }

        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Found(r, interactive) => {
                    self.busy = false;
                    settings.set(K_LAST, super::sys::now_ms());
                    if interactive || settings.string(K_SKIP) != r.version { self.dialog = Dialog::Offer(r); }
                }
                Msg::UpToDate(i) => {
                    self.busy = false;
                    settings.set(K_LAST, super::sys::now_ms());
                    if i { self.dialog = Dialog::Info(format!("{} est à jour", self.app), format!("Vous avez la dernière version ({}).", self.version)); }
                }
                Msg::Failed(e, i) => { self.busy = false; if i { self.dialog = Dialog::Info("Impossible de vérifier les mises à jour".into(), e); } }
                Msg::Progress(f) => self.dialog = Dialog::Downloading(f),
                Msg::Installed => { self.relaunch_requested = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                Msg::InstallFailed(e) => {
                    let page = if let Dialog::Offer(r) = &self.dialog { r.page.clone() } else { format!("https://github.com/{}/releases", self.repo) };
                    self.dialog = Dialog::InstallError(e, page);
                }
            }
        }

        let mut next: Option<Dialog> = None;
        match &self.dialog {
            Dialog::None => {}
            Dialog::Info(title, text) => {
                let close = super::ui::modal(ctx, "selfupdate-info", 380.0, |ui| {
                    ui.label(egui::RichText::new(title).font(super::theme::bold(15.0)));
                    ui.label(text);
                    ui.add_space(6.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.button("OK").clicked()).inner
                });
                if close { next = Some(Dialog::None); }
            }
            Dialog::Offer(r) => {
                let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
                let r = r.clone();
                let mut choice = 0;
                super::ui::modal(ctx, "selfupdate-offer", 460.0, |ui| {
                    ui.label(egui::RichText::new(format!("{} {} est disponible", self.app, r.version)).font(super::theme::bold(15.0)));
                    let size = if r.size > 0 { format!(" ({})", super::sys::bytes(r.size)) } else { String::new() };
                    ui.label(if appimage.is_some() {
                        format!("Vous avez la version {}. La mise à jour télécharge la nouvelle version{size}, remplace l'AppImage et relance l'app.", self.version)
                    } else {
                        format!("Vous avez la version {}. L'app n'est pas lancée depuis son AppImage : la page de la release va s'ouvrir.", self.version)
                    });
                    if !r.notes.is_empty() {
                        egui::Frame::new().fill(p.panel2).corner_radius(8).inner_margin(8).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| { ui.set_width(ui.available_width()); ui.label(egui::RichText::new(&r.notes).size(12.0)); });
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Ignorer cette version").clicked() { choice = 3; }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if super::ui::primary(ui, p, if appimage.is_some() { "Mettre à jour" } else { "Ouvrir la page" }).clicked() { choice = 1; }
                            if ui.button("Plus tard").clicked() { choice = 2; }
                        });
                    });
                });
                match choice {
                    1 => match appimage {
                        Some(target) => {
                            let (tx, ctx2) = (self.tx.clone(), ctx.clone());
                            let r2 = r.clone();
                            std::thread::spawn(move || {
                                let tx2 = tx.clone();
                                let c3 = ctx2.clone();
                                let res = install(&r2, &target, move |f| { let _ = tx2.send(Msg::Progress(f)); c3.request_repaint(); });
                                let _ = tx.send(match res { Ok(()) => Msg::Installed, Err(e) => Msg::InstallFailed(e) });
                                ctx2.request_repaint();
                            });
                            next = Some(Dialog::Downloading(0.0));
                        }
                        None => { super::sys::open_url(&r.page); next = Some(Dialog::None); }
                    },
                    2 => next = Some(Dialog::None),
                    3 => { settings.set(K_SKIP, r.version.clone()); next = Some(Dialog::None); }
                    _ => {}
                }
            }
            Dialog::Downloading(f) => {
                let f = *f;
                super::ui::modal(ctx, "selfupdate-dl", 360.0, |ui| {
                    ui.label(format!("Téléchargement de {}…", self.app));
                    ui.add(egui::ProgressBar::new(f).show_percentage());
                });
            }
            Dialog::InstallError(e, page) => {
                let (e, page) = (e.clone(), page.clone());
                let mut c = 0;
                super::ui::modal(ctx, "selfupdate-err", 420.0, |ui| {
                    ui.label(egui::RichText::new("La mise à jour n'a pas pu être installée").font(super::theme::bold(15.0)));
                    ui.label(format!("{e}\n\nVous pouvez la télécharger à la main depuis la page de la release."));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if super::ui::primary(ui, p, "Ouvrir la page").clicked() { c = 1; }
                        if ui.button("Fermer").clicked() { c = 2; }
                    });
                });
                if c == 1 { super::sys::open_url(&page); }
                if c > 0 { next = Some(Dialog::None); }
            }
        }
        if let Some(n) = next { self.dialog = n; }
    }

    /// Après la fermeture : relance la nouvelle AppImage si une mise à jour vient d'être posée.
    pub fn relaunch_if_needed(&self) {
        if !self.relaunch_requested { return; }
        if let Some(app) = std::env::var_os("APPIMAGE") {
            let pid = std::process::id();
            let mut c = super::sys::clean_command("sh");
            c.args(["-c", &format!("while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; exec \"$0\""), &app.to_string_lossy()]);
            let _ = super::sys::spawn_detached(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(!is_newer("v1.3.2", "1.3.2"));
        assert!(!is_newer("1.3", "1.3.0"));
        assert!(is_newer("2", "1.99"));
    }
    #[test]
    fn only_the_linux_appimage() {
        let names = vec!["App-1.4.0-macOS-arm64.zip".to_string(), "App-1.4.0-linux-x86_64.AppImage".into(), "App-1.4.0-windows-x64.zip".into()];
        assert_eq!(linux_asset(&names), Some(1));
        assert_eq!(linux_asset(&names[..1].to_vec()), None);
    }
    #[test]
    fn notes() {
        assert_eq!(readable_notes("## Nouveautés\n- **Un** `deux`\n\n## Installation\n1. truc"), "Nouveautés\n• Un deux");
    }
}
