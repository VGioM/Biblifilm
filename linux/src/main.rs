//! Biblifilm pour Linux : analyse un dossier, identifie chaque fichier en ligne
//! (titre complet, épisode, synopsis, affiche) et le lit dans la fenêtre avec
//! libmpv, en suivant la reprise de lecture et les épisodes vus.

mod app;
mod common;
mod library;
mod metadata;
mod model;
mod mpv;
mod parser;
mod player_ui;
mod views;

fn main() -> eframe::Result {
    let icon = image::load_from_memory(include_bytes!("../resources/icon.png")).ok().map(|i| {
        let i = i.to_rgba8();
        let (w, h) = i.dimensions();
        egui::IconData { rgba: i.into_raw(), width: w, height: h }
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Biblifilm").with_app_id("biblifilm").with_inner_size([1280.0, 820.0]).with_min_inner_size([940.0, 580.0]);
    if let Some(i) = icon { viewport = viewport.with_icon(i); }
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Glow, vsync: true, ..Default::default() };
    eframe::run_native("Biblifilm", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
