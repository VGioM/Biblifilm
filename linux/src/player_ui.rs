//! L'écran de lecture : la vidéo (rendue par libmpv dans le contexte OpenGL de
//! la fenêtre) occupe toute la fenêtre, et les commandes sont posées dessus.
//!
//! Espace lecture/pause · ←/→ 10 s · ↑/↓ et molette : volume · B piste audio
//! suivante · V sous-titres suivants (dont « aucun ») · F ou double-clic : plein
//! écran · Échap : quitte le plein écran, sinon revient à la bibliothèque ·
//! clic : pause. Les commandes s'effacent après 3 s de lecture, et le pointeur
//! avec elles.

use crate::app::App;
use crate::common::{icons, theme};
use crate::model::format_time;
use egui::{Align, Align2, Color32, CursorIcon, Layout, Rect, RichText, Sense, Vec2};
use std::time::Instant;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let p = app.p;
    let mut close = false;
    let mut toggle_fs = false;
    let mut external: Option<String> = None;

    // ── Clavier et molette ──────────────────────────────────────────────
    let (keys, scroll, moved) = ctx.input(|i| {
        let k: Vec<egui::Key> = [egui::Key::Space, egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::ArrowUp, egui::Key::ArrowDown,
            egui::Key::B, egui::Key::V, egui::Key::F, egui::Key::Escape, egui::Key::N].into_iter().filter(|k| i.key_pressed(*k)).collect();
        (k, i.raw_scroll_delta.y, i.pointer.delta() != Vec2::ZERO || i.pointer.any_down())
    });
    if let Some(s) = &mut app.session {
        if moved || !keys.is_empty() { s.last_activity = Instant::now(); }
        for k in keys {
            let pl = &mut s.player;
            match k {
                egui::Key::Space => pl.toggle_pause(),
                egui::Key::ArrowLeft => { pl.seek_relative(-10.0); s.osd = Some(("−10 s".into(), Instant::now())); }
                egui::Key::ArrowRight => { pl.seek_relative(10.0); s.osd = Some(("+10 s".into(), Instant::now())); }
                egui::Key::ArrowUp => { let v = pl.volume + 5.0; pl.set_volume(v); s.osd = Some((format!("Volume {} %", pl.volume.round()), Instant::now())); }
                egui::Key::ArrowDown => { let v = pl.volume - 5.0; pl.set_volume(v); s.osd = Some((format!("Volume {} %", pl.volume.round()), Instant::now())); }
                egui::Key::F => toggle_fs = true,
                egui::Key::N => pl.next(),
                egui::Key::Escape => if app.fullscreen { toggle_fs = true } else { close = true },
                _ => {}
            }
        }
        // Le geste de volume : deux doigts haut / bas, pas de 5.
        if scroll.abs() > 0.5 {
            let pl = &mut s.player;
            let v = pl.volume + if scroll > 0.0 { 5.0 } else { -5.0 };
            pl.set_volume(v);
            s.osd = Some((format!("Volume {} %", pl.volume.round()), Instant::now()));
        }
    }
    // B et V : la piste suivante, retenue pour les épisodes suivants.
    for (key, kind) in [(egui::Key::B, "audio"), (egui::Key::V, "sub")] {
        if ctx.input(|i| i.key_pressed(key)) {
            if let Some(s) = &app.session {
                let list: Vec<_> = s.player.tracks.iter().filter(|t| t.kind == kind).cloned().collect();
                let cur = list.iter().position(|t| t.selected);
                let next = if kind == "sub" {
                    match cur { None => list.first().cloned(), Some(k) if k + 1 < list.len() => Some(list[k + 1].clone()), _ => None }
                } else if list.len() > 1 { Some(list[(cur.unwrap_or(0) + 1) % list.len()].clone()) } else { None };
                if kind == "sub" || next.is_some() { app.choose_track(kind, next); }
            }
        }
    }

    let Some(s) = &mut app.session else { return };
    let idle = !s.player.paused && s.last_activity.elapsed().as_secs() >= 3 && s.error.is_none();
    if !idle { ctx.request_repaint_after(std::time::Duration::from_millis(500)); }

    egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::BLACK)).show(ctx, |ui| {
        let full = ui.max_rect();
        // La vidéo, dessinée par mpv dans le tampon de la fenêtre.
        let surface = s.player.surface.clone();
        let ctx_ptr = s.player.ctx_ptr();
        let cb = egui_glow::CallbackFn::new(move |info, painter| {
            let [w, h] = info.screen_size_px;
            if let Ok(mut sf) = surface.lock() { sf.render(painter.gl(), w as i32, h as i32, ctx_ptr as *const egui::Context); }
        });
        ui.painter().add(egui::PaintCallback { rect: full, callback: std::sync::Arc::new(cb) });

        let bg = ui.interact(full, ui.id().with("video"), Sense::click());
        if bg.double_clicked() { toggle_fs = true; } else if bg.clicked() { s.player.toggle_pause(); }
        if idle { ctx.set_cursor_icon(CursorIcon::None); }

        // Un fichier qui ne s'ouvre pas : une erreur et une porte de sortie.
        if let Some(e) = s.error.clone() {
            let id = s.queue[s.player.index.min(s.queue.len() - 1)].clone();
            egui::Area::new(egui::Id::new("player-error")).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                egui::Frame::new().fill(Color32::from_black_alpha(200)).corner_radius(12).inner_margin(20).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add(icons::image("warning", 34.0, p.warm));
                        ui.label(RichText::new("Ce fichier ne peut pas être lu").font(theme::bold(16.0)));
                        ui.label(RichText::new(format!("mpv : {e}")).color(Color32::from_white_alpha(180)));
                        ui.horizontal(|ui| {
                            if ui.button("Ouvrir dans un lecteur externe").clicked() { external = Some(id.clone()); close = true; }
                            if ui.button("Retour").clicked() { close = true; }
                        });
                    });
                });
            });
        }

        if !idle {
            let (title, sub) = s.titles.get(s.player.index).cloned().unwrap_or_default();
            // En haut : retour et titre.
            let top = Rect::from_min_size(full.min, Vec2::new(full.width(), 90.0));
            gradient(ui, top, true);
            let back = crate::common::ui::circle_button(ui, top.min + Vec2::new(40.0, 40.0), "chevronLeft", ui.id().with("back"));
            if back.on_hover_text("Retour (Échap)").clicked() { close = true; }
            ui.painter().text(top.min + Vec2::new(74.0, 24.0), Align2::LEFT_TOP, &title, theme::bold(18.0), Color32::WHITE);
            ui.painter().text(top.min + Vec2::new(74.0, 48.0), Align2::LEFT_TOP, &sub, theme::regular(13.0), Color32::from_white_alpha(180));

            // En bas : barre de lecture et commandes.
            let bottom = Rect::from_min_max(egui::pos2(full.left(), full.bottom() - 110.0), full.max);
            gradient(ui, bottom, false);
            let inner = bottom.shrink2(Vec2::new(22.0, 16.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - 70.0), inner.max)).layout(Layout::top_down(Align::Min)));
            let ui = &mut child;
            let pl = &mut s.player;
            let dur = pl.duration.max(0.0);
            let mut pos = pl.position.min(dur.max(0.001));
            ui.spacing_mut().slider_width = inner.width();
            let r = ui.add(egui::Slider::new(&mut pos, 0.0..=dur.max(0.001)).show_value(false).trailing_fill(true));
            if r.drag_stopped() || (r.changed() && !r.dragged()) { pl.seek_absolute(pos); }
            ui.horizontal(|ui| {
                let white = Color32::WHITE;
                let btn = |ui: &mut egui::Ui, icon: &str, tip: &str| ui.add(egui::Button::image(icons::image(icon, 22.0, white)).frame(false)).on_hover_text(tip.to_string()).clicked();
                if btn(ui, if pl.paused { "play" } else { "pause" }, "Lecture/pause (Espace)") { pl.toggle_pause(); }
                if ui.add(egui::Button::new(RichText::new("−10").color(white)).frame(false)).on_hover_text("Reculer de 10 s (←)").clicked() { pl.seek_relative(-10.0); }
                if ui.add(egui::Button::new(RichText::new("+10").color(white)).frame(false)).on_hover_text("Avancer de 10 s (→)").clicked() { pl.seek_relative(10.0); }
                ui.label(RichText::new(format!("{} / {}", format_time(pl.position), format_time(dur))).color(Color32::from_white_alpha(220)).monospace());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if btn(ui, "grid", "Plein écran (F, double-clic)") { toggle_fs = true; }
                    let mut vol = pl.volume;
                    ui.spacing_mut().slider_width = 90.0;
                    if ui.add(egui::Slider::new(&mut vol, 0.0..=130.0).show_value(false)).changed() { pl.set_volume(vol); }
                    ui.add(icons::image("volume", 18.0, white));
                    if pl.index + 1 < pl.count && btn(ui, "chevronRight", "Épisode suivant (N)") { pl.next(); }
                    let tracks = pl.tracks.clone();
                    track_menu(ui, "sub", "Sous-titres (V)", &tracks, &p);
                    track_menu(ui, "audio", "Pistes audio (B)", &tracks, &p);
                });
            });
        }

        if let Some((t, at)) = &s.osd {
            if at.elapsed().as_millis() < 900 {
                ui.painter().rect_filled(Rect::from_center_size(full.center(), Vec2::new(200.0, 46.0)), 12, Color32::from_black_alpha(150));
                ui.painter().text(full.center(), Align2::CENTER_CENTER, t, theme::regular(15.0), Color32::WHITE);
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            } else { s.osd = None; }
        }
    });

    // Les choix de pistes faits dans les menus.
    let picks = PICKS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for (kind, track) in picks { app.choose_track(kind, track); }

    if toggle_fs {
        app.fullscreen = !app.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(app.fullscreen));
    }
    if let Some(id) = external { app.open_externally(&id); }
    if close {
        if app.fullscreen { app.fullscreen = false; ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false)); }
        app.stop_playback();
    }
}

thread_local! { static PICKS: std::cell::RefCell<Vec<(&'static str, Option<crate::mpv::Track>)>> = std::cell::RefCell::new(vec![]); }

/// Les pistes audio ou de sous-titres, avec « Aucun » en tête pour les sous-titres.
fn track_menu(ui: &mut egui::Ui, kind: &'static str, tip: &str, tracks: &[crate::mpv::Track], p: &crate::common::theme::Palette) {
    let list: Vec<&crate::mpv::Track> = tracks.iter().filter(|t| t.kind == kind).collect();
    if list.is_empty() && kind == "audio" { return; }
    let icon = if kind == "audio" { "music" } else { "doc" };
    ui.menu_image_button(icons::image(icon, 18.0, Color32::WHITE), |ui| {
        ui.set_min_width(260.0);
        ui.label(RichText::new(if kind == "audio" { "Audio" } else { "Sous-titres" }).font(theme::bold(11.0)).color(p.muted));
        if kind == "sub" && ui.radio(!list.iter().any(|t| t.selected), "Aucun").clicked() {
            PICKS.with(|v| v.borrow_mut().push((kind, None)));
            ui.close_menu();
        }
        for t in &list {
            if ui.radio(t.selected, t.label()).clicked() {
                PICKS.with(|v| v.borrow_mut().push((kind, Some((*t).clone()))));
                ui.close_menu();
            }
        }
    }).response.on_hover_text(tip.to_string());
}

/// Un voile dégradé derrière les commandes, pour qu'elles se lisent sur n'importe quelle image.
fn gradient(ui: &egui::Ui, rect: Rect, top: bool) {
    let (a, b) = (Color32::from_black_alpha(180), Color32::TRANSPARENT);
    let (c1, c2) = if top { (a, b) } else { (b, a) };
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), c1);
    m.colored_vertex(rect.right_top(), c1);
    m.colored_vertex(rect.right_bottom(), c2);
    m.colored_vertex(rect.left_bottom(), c2);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(m));
}
