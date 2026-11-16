//! Les briques d'interface communes : feuilles modales, boutons, rangées de la
//! barre latérale, vignettes (image ou dégradé de remplacement), pastilles,
//! message en bas de fenêtre.

use super::icons;
use super::images::{ImageCache, Size, Source};
use super::theme::{self, Palette};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

/// Une feuille modale centrée ; Échap ou un clic dehors la ferme (rend vrai).
/// `body` rend vrai pour demander la fermeture.
pub fn modal<R>(ctx: &egui::Context, id: &str, width: f32, body: impl FnOnce(&mut Ui) -> R) -> R {
    let mut out = None;
    egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.set_width(width);
        out = Some(body(ui));
    });
    out.unwrap()
}

/// Comme `modal`, mais rend aussi vrai si on clique dehors ou presse Échap.
pub fn sheet(ctx: &egui::Context, id: &str, width: f32, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    let mut close = false;
    let r = egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.set_width(width);
        close = body(ui);
    });
    close || r.should_close()
}

pub fn primary(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).color(p.on_accent).strong()).fill(p.accent).stroke(Stroke::NONE))
}

pub fn primary_icon(ui: &mut Ui, p: &Palette, icon: &str, text: &str, large: bool) -> Response {
    let img = icons::image(icon, if large { 14.0 } else { 12.0 }, p.on_accent);
    let mut b = egui::Button::image_and_text(img, RichText::new(text).color(p.on_accent).strong()).fill(p.accent).stroke(Stroke::NONE);
    if large { b = b.min_size(Vec2::new(0.0, 34.0)); }
    ui.add(b)
}

pub fn button_icon(ui: &mut Ui, p: &Palette, icon: &str, text: &str) -> Response {
    ui.add(egui::Button::image_and_text(icons::image(icon, 14.0, p.text), text))
}

/// Un bouton fait d'une icône seule, sans cadre tant qu'on ne le survole pas.
pub fn icon_button(ui: &mut Ui, p: &Palette, icon: &str, size: f32, tip: &str) -> Response {
    let r = ui.add(egui::Button::image(icons::image(icon, size, p.text)).frame(false).min_size(Vec2::splat(size + 12.0)));
    if tip.is_empty() { r } else { r.on_hover_text(tip) }
}

/// Une rangée de la barre latérale : icône, libellé, compteur.
pub fn side_row(ui: &mut Ui, p: &Palette, icon: &str, label: &str, count: Option<String>, count_color: Option<Color32>, active: bool) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, 7, p.selection);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 7, Color32::from_white_alpha(12));
    }
    let ic = Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(16.0));
    icons::image(icon, 16.0, p.accent).paint_at(ui, ic);
    let right = if let Some(c) = &count {
        let g = ui.painter().layout_no_wrap(c.clone(), FontId::proportional(11.5), count_color.unwrap_or(p.tertiary));
        let pos = egui::pos2(rect.right() - 10.0 - g.size().x, rect.center().y - g.size().y / 2.0);
        ui.painter().galley(pos, g.clone(), p.tertiary);
        g.size().x + 16.0
    } else { 10.0 };
    let max = rect.width() - 34.0 - right;
    let galley = ui.painter().layout(label.to_string(), FontId::proportional(13.5), p.text, f32::INFINITY);
    let galley = if galley.size().x > max { elided(ui, label, FontId::proportional(13.5), p.text, max) } else { galley };
    ui.painter().galley(egui::pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    resp
}

pub fn section_title(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text).font(theme::bold(11.0)).color(p.muted));
}

/// Un texte coupé d'une ellipse à `max` points.
pub fn elided(ui: &Ui, text: &str, font: FontId, color: Color32, max: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: max, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

/// Écrit `text` sur une ligne dans `rect`, coupé s'il le faut.
pub fn text_in(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    let g = elided(ui, text, font, color, rect.width());
    ui.painter().galley(rect.left_top(), g, color);
}

/// Le dégradé de remplacement d'une vignette sans image, avec son pictogramme.
pub fn placeholder(ui: &Ui, rect: Rect, radius: u8, seed: &str, icon: &str) {
    let (a, b) = theme::placeholder_colors(seed);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), a.lerp_to_gamma(b, 0.5));
    mesh.colored_vertex(rect.right_bottom(), b);
    mesh.colored_vertex(rect.left_bottom(), a.lerp_to_gamma(b, 0.5));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    // Les coins arrondis : le dégradé est peint, puis les coins masqués par un cadre de la couleur du fond.
    ui.painter().add(egui::Shape::mesh(mesh));
    let _ = radius;
    let ic = Rect::from_center_size(rect.center(), Vec2::splat((rect.width() * 0.18).clamp(18.0, 34.0)));
    icons::image(icon, ic.width(), Color32::from_white_alpha(115)).paint_at(ui, ic);
}

pub enum Fit { Fill, Contain }

/// Peint une image dans `rect` (remplie et rognée, ou contenue), coins arrondis.
pub fn paint_texture(ui: &Ui, rect: Rect, tex: &egui::TextureHandle, size: [usize; 2], radius: u8, fit: Fit) {
    let (w, h) = (size[0] as f32, size[1] as f32);
    let ra = rect.width() / rect.height();
    let ia = w / h;
    match fit {
        Fit::Fill => {
            let uv = if ia > ra {
                let k = ra / ia; Rect::from_min_max(egui::pos2((1.0 - k) / 2.0, 0.0), egui::pos2((1.0 + k) / 2.0, 1.0))
            } else {
                let k = ia / ra; Rect::from_min_max(egui::pos2(0.0, (1.0 - k) / 2.0), egui::pos2(1.0, (1.0 + k) / 2.0))
            };
            egui::Image::new((tex.id(), rect.size())).uv(uv).corner_radius(radius).paint_at(ui, rect);
        }
        Fit::Contain => {
            let s = if ia > ra { Vec2::new(rect.width(), rect.width() / ia) } else { Vec2::new(rect.height() * ia, rect.height()) };
            let r = Rect::from_center_size(rect.center(), s);
            egui::Image::new((tex.id(), s)).corner_radius(radius).paint_at(ui, r);
        }
    }
}

/// Une vignette : l'image si elle est là, sinon le dégradé (et un indicateur pendant le chargement).
/// `blur_behind` : une image contenue, posée sur une copie floue d'elle-même.
pub fn cover(ui: &Ui, images: &mut ImageCache, rect: Rect, source: Option<&Source>, seed: &str, icon: &str, radius: u8, size: Size, contain: bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, radius, Color32::from_gray(40));
    let tex = source.and_then(|s| images.get(s, size));
    match tex {
        Some((t, sz)) if contain => {
            if let Some(s) = source { if let Some((b, bs)) = images.get(s, Size::Blur) {
                paint_texture(ui, rect, &b, bs, radius, Fit::Fill);
                painter.rect_filled(rect, radius, Color32::from_black_alpha(80));
            } }
            paint_texture(ui, rect, &t, sz, radius, Fit::Contain);
        }
        Some((t, sz)) => paint_texture(ui, rect, &t, sz, radius, Fit::Fill),
        None => {
            placeholder(ui, rect, radius, seed, icon);
        }
    }
    painter.rect_stroke(rect, radius, Stroke::new(0.5_f32, Color32::from_white_alpha(23)), egui::StrokeKind::Inside);
}

/// Une ombre douce sous une vignette.
pub fn shadow(ui: &Ui, rect: Rect, radius: u8, strong: bool) {
    let s = egui::epaint::Shadow { offset: [0, if strong { 6 } else { 3 }], blur: if strong { 18 } else { 10 }, spread: 0, color: Color32::from_black_alpha(if strong { 130 } else { 105 }) };
    ui.painter().add(s.as_shape(rect, CornerRadius::same(radius)));
}

/// Une pastille sombre (« PS1 », « 3 disques »).
pub fn badge(ui: &Ui, pos: egui::Pos2, anchor: Align2, text: &str) -> Rect {
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::bold(10.0), Color32::from_white_alpha(242));
    let size = g.size() + Vec2::new(12.0, 5.0);
    let rect = anchor.anchor_size(pos, size);
    ui.painter().rect(rect, 99, Color32::from_black_alpha(158), Stroke::new(0.5_f32, Color32::from_white_alpha(56)), egui::StrokeKind::Inside);
    ui.painter().galley(rect.min + Vec2::new(6.0, 2.5), g, Color32::WHITE);
    rect
}

/// Un rond sombre cerclé de blanc, avec une icône (lecture, informations).
pub fn circle_button(ui: &mut Ui, center: egui::Pos2, icon: &str, id: egui::Id) -> Response {
    let rect = Rect::from_center_size(center, Vec2::splat(40.0));
    let resp = ui.interact(rect, id, Sense::click());
    let fill = if resp.hovered() { Color32::from_black_alpha(190) } else { Color32::from_black_alpha(140) };
    ui.painter().circle(center, 20.0, fill, Stroke::new(1.2_f32, Color32::from_white_alpha(217)));
    let ic = Rect::from_center_size(center, Vec2::splat(16.0));
    icons::image(icon, 16.0, Color32::WHITE).paint_at(ui, ic);
    resp
}

/// Des pastilles de texte (genres, étiquettes), passant à la ligne.
pub fn chips(ui: &mut Ui, p: &Palette, items: &[String]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for t in items {
            egui::Frame::new().fill(Color32::from_white_alpha(26)).corner_radius(99).inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| ui.label(RichText::new(t).size(11.0).color(p.muted)));
        }
    });
}

/// Une ligne « clé  valeur » du panneau d'informations.
pub fn info_row(ui: &mut Ui, p: &Palette, key: &str, value: &str) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(Vec2::new(60.0, 14.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.set_min_width(60.0);
            ui.label(RichText::new(key).size(11.5).color(p.muted));
        });
        ui.add(egui::Label::new(RichText::new(value).size(11.5)).wrap().selectable(true));
    });
}

/// Un message en bas de la fenêtre, qui s'efface seul après 6 secondes.
pub struct Toast { text: Option<(String, std::time::Instant)> }
impl Default for Toast { fn default() -> Self { Toast { text: None } } }
impl Toast {
    pub fn show(&mut self, text: impl Into<String>) { self.text = Some((text.into(), std::time::Instant::now())); }
    pub fn ui(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some((t, at)) = &self.text else { return };
        if at.elapsed().as_secs() >= 6 { self.text = None; return; }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        let t = t.clone();
        let r = egui::Area::new(egui::Id::new("toast")).anchor(Align2::CENTER_BOTTOM, [0.0, -18.0]).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(p.panel2).corner_radius(99).stroke(Stroke::new(1.0_f32, p.accent)).inner_margin(egui::Margin::symmetric(16, 9))
                .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| ui.label(RichText::new(t).strong()));
        });
        if r.response.interact(Sense::click()).clicked() { self.text = None; }
    }
}

/// Un champ de recherche avec sa loupe et sa croix.
pub fn search_field(ui: &mut Ui, p: &Palette, text: &mut String, hint: &str, width: f32, focus: bool) -> Response {
    let frame = egui::Frame::new().fill(Color32::from_white_alpha(18)).corner_radius(8).stroke(Stroke::new(1.0_f32, p.hairline)).inner_margin(egui::Margin::symmetric(8, 4));
    frame.show(ui, |ui| {
        ui.set_width(width);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(icons::image("search", 14.0, p.muted));
            let w = width - (if text.is_empty() { 30.0 } else { 54.0 });
            let r = ui.add(egui::TextEdit::singleline(text).hint_text(hint).frame(false).desired_width(w));
            if focus { r.request_focus(); }
            if !text.is_empty() && ui.add(egui::Button::image(icons::image("x", 11.0, p.muted)).frame(false)).clicked() { text.clear(); }
            r
        }).inner
    }).inner
}

/// Une case à cocher lisible sur fond sombre : carré arrondi, rempli de l'accent une fois cochée.
pub fn check(ui: &mut Ui, p: &Palette, value: &mut bool, label: &str) -> Response {
    let g = ui.painter().layout_no_wrap(label.to_string(), egui::TextStyle::Body.resolve(ui.style()), p.text);
    let w = 18.0 + if label.is_empty() { 0.0 } else { 8.0 + g.size().x };
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(w, g.size().y.max(18.0)), Sense::click());
    if resp.clicked() { *value = !*value; resp.mark_changed(); }
    let b = Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 8.0), Vec2::splat(16.0));
    if *value {
        ui.painter().rect_filled(b, 4, p.accent);
        icons::image("check", 12.0, p.on_accent).paint_at(ui, Rect::from_center_size(b.center(), Vec2::splat(12.0)));
    } else {
        ui.painter().rect(b, 4, Color32::from_white_alpha(if resp.hovered() { 30 } else { 14 }), Stroke::new(1.0_f32, p.hairline_strong), egui::StrokeKind::Inside);
    }
    if !label.is_empty() { ui.painter().galley(egui::pos2(b.right() + 8.0, rect.center().y - g.size().y / 2.0), g, p.text); }
    resp
}

/// Une case à cocher avec un texte d'aide en dessous.
pub fn check_help(ui: &mut Ui, p: &Palette, value: &mut bool, label: &str, help: &str) -> Response {
    let r = check(ui, p, value, label);
    if !help.is_empty() {
        ui.horizontal(|ui| { ui.add_space(26.0); ui.add(egui::Label::new(RichText::new(help).size(11.5).color(p.muted)).wrap()); });
    }
    r
}

/// Un groupe de réglages, encadré.
pub fn group<R>(ui: &mut Ui, p: &Palette, title: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new().fill(Color32::from_white_alpha(10)).stroke(Stroke::new(1.0_f32, p.hairline)).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() { ui.label(RichText::new(title).font(theme::bold(12.5))); }
            body(ui)
        }).inner
}

/// Un indicateur d'attente.
pub fn spinner(ui: &mut Ui, size: f32) { ui.add(egui::Spinner::new().size(size)); }
