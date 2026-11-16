//! Pour les tests de bout en bout seulement : `APP_SCREENSHOT=/chemin.png`
//! enregistre une capture de la fenêtre après `APP_SCREENSHOT_DELAY` secondes
//! (5 par défaut), puis l'app se ferme si `APP_SCREENSHOT_EXIT=1`. Sans ces
//! variables, rien ne se passe.

use std::time::Instant;

pub struct TestHook { path: Option<String>, delay: f32, exit: bool, start: Instant, requested: bool }

impl Default for TestHook {
    fn default() -> Self {
        TestHook {
            path: std::env::var("APP_SCREENSHOT").ok().filter(|s| !s.is_empty()),
            delay: std::env::var("APP_SCREENSHOT_DELAY").ok().and_then(|s| s.parse().ok()).unwrap_or(5.0),
            exit: std::env::var("APP_SCREENSHOT_EXIT").map(|s| s == "1").unwrap_or(false),
            start: Instant::now(), requested: false,
        }
    }
}

impl TestHook {
    pub fn elapsed(&self) -> f32 { self.start.elapsed().as_secs_f32() }
    pub fn active(&self) -> bool { self.path.is_some() }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else { return };
        if std::env::var("APP_TRACE").is_ok() { eprintln!("frame {:.1}s requested={}", self.elapsed(), self.requested); }
        if !self.requested && self.elapsed() >= self.delay {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let shot = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
        if let Some(img) = shot {
            if std::env::var("APP_TRACE").is_ok() { eprintln!("screenshot {:?}", img.size); }
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, raw) { let _ = buf.save(&path); }
            self.path = None;
            if self.exit { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        }
    }
}
