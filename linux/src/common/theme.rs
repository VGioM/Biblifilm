//! Couleurs et typographie. Chaque app choisit sa palette (Biblifilm et
//! Biblijeu : les gris de Music.app et le bleu système ; Recomps : le bleu nuit
//! et le cyan de recomplauncher.com) ; le reste est commun.

use egui::{Color32, FontData, FontDefinitions, FontFamily, Stroke, Visuals};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Palette {
    pub accent: Color32,
    pub accent2: Color32,
    pub on_accent: Color32,
    pub warm: Color32,
    pub ok: Color32,
    pub content: Color32,
    pub sidebar: Color32,
    pub toolbar: Color32,
    pub panel: Color32,
    pub panel2: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub tertiary: Color32,
    pub hairline: Color32,
    pub hairline_strong: Color32,
    pub selection: Color32,
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 { Color32::from_rgba_premultiplied(
    ((r as u16 * a as u16) / 255) as u8, ((g as u16 * a as u16) / 255) as u8, ((b as u16 * a as u16) / 255) as u8, a) }

/// Music.app en mode sombre, bleu système. Partagée par Biblifilm et Biblijeu.
pub const MUSIC: Palette = Palette {
    accent: Color32::from_rgb(0x0A, 0x85, 0xFF),
    accent2: Color32::from_rgb(0x0A, 0x85, 0xFF),
    on_accent: Color32::WHITE,
    warm: Color32::from_rgb(0xFA, 0x9E, 0x3D),
    ok: Color32::from_rgb(0x34, 0xC7, 0x59),
    content: Color32::from_rgb(0x1C, 0x1C, 0x1F),
    sidebar: Color32::from_rgb(0x26, 0x26, 0x28),
    toolbar: Color32::from_rgb(0x21, 0x21, 0x24),
    panel: Color32::from_rgb(0x21, 0x21, 0x24),
    panel2: Color32::from_rgb(0x2C, 0x2C, 0x2E),
    text: Color32::from_rgb(0xF2, 0xF2, 0xF4),
    muted: rgba(235, 235, 245, 158),
    tertiary: rgba(235, 235, 245, 92),
    hairline: rgba(255, 255, 255, 20),
    hairline_strong: rgba(255, 255, 255, 36),
    selection: rgba(255, 255, 255, 26),
};

/// Recomps : bleu nuit, dégradé cyan → bleu.
pub const RECOMPS: Palette = Palette {
    accent: Color32::from_rgb(0x34, 0xF2, 0xE8),
    accent2: Color32::from_rgb(0x37, 0x7B, 0xB6),
    on_accent: Color32::from_rgb(0x0B, 0x12, 0x20),
    warm: Color32::from_rgb(0xFF, 0xB5, 0x47),
    ok: Color32::from_rgb(0x3C, 0xDC, 0x8C),
    content: Color32::from_rgb(0x0B, 0x12, 0x20),
    sidebar: Color32::from_rgb(0x0F, 0x18, 0x29),
    toolbar: Color32::from_rgb(0x0B, 0x12, 0x20),
    panel: Color32::from_rgb(0x0F, 0x18, 0x29),
    panel2: Color32::from_rgb(0x14, 0x1F, 0x33),
    text: Color32::from_rgb(0xF4, 0xF8, 0xFC),
    muted: Color32::from_rgb(0xA7, 0xB3, 0xC4),
    tertiary: rgba(167, 179, 196, 178),
    hairline: rgba(244, 248, 252, 23),
    hairline_strong: rgba(244, 248, 252, 41),
    selection: rgba(52, 242, 232, 36),
};

/// La police du bureau (Inter, Cantarell, Noto Sans…), trouvée par fontconfig ;
/// celle d'egui en dernier recours.
fn system_font(names: &[&str]) -> Option<Vec<u8>> {
    for name in names {
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}|%{family}", name]).output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        let mut parts = s.splitn(2, '|');
        let file = parts.next().unwrap_or("");
        let family = parts.next().unwrap_or("").to_lowercase();
        if family.contains(&name.to_lowercase()) && (file.ends_with(".ttf") || file.ends_with(".otf")) {
            if let Ok(bytes) = std::fs::read(file) {
                return Some(bytes);
            }
        }
    }
    None
}

pub fn install(ctx: &egui::Context, p: &Palette) {
    let mut fonts = FontDefinitions::default();
    let regular = system_font(&["Inter", "Cantarell", "Noto Sans", "DejaVu Sans"]);
    let bold = system_font(&["Inter:bold", "Cantarell:bold", "Noto Sans:bold", "DejaVu Sans:bold"]);
    if let Some(r) = regular {
        fonts.font_data.insert("sys".into(), Arc::new(FontData::from_owned(r)));
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "sys".into());
    }
    if let Some(b) = bold {
        fonts.font_data.insert("sys-bold".into(), Arc::new(FontData::from_owned(b)));
        let mut fam = vec!["sys-bold".to_string()];
        fam.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(FontFamily::Name("bold".into()), fam);
    } else {
        let fam = fonts.families[&FontFamily::Proportional].clone();
        fonts.families.insert(FontFamily::Name("bold".into()), fam);
    }
    ctx.set_fonts(fonts);

    let mut v = Visuals::dark();
    v.panel_fill = p.content;
    v.window_fill = p.panel;
    v.extreme_bg_color = Color32::from_rgba_premultiplied(18, 18, 18, 18);
    v.faint_bg_color = p.panel2;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.accent.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.hyperlink_color = p.accent;
    v.window_corner_radius = 12.into();
    v.menu_corner_radius = 9.into();
    v.window_stroke = Stroke::new(1.0_f32, p.hairline_strong);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(140) };
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = 7.into();
    }
    v.widgets.inactive.weak_bg_fill = Color32::from_white_alpha(18);
    v.widgets.inactive.bg_fill = Color32::from_white_alpha(18);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.hairline_strong);
    v.widgets.hovered.weak_bg_fill = Color32::from_white_alpha(30);
    v.widgets.hovered.bg_fill = Color32::from_white_alpha(30);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.hairline_strong);
    v.widgets.active.weak_bg_fill = Color32::from_white_alpha(40);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.hairline);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(11.0, 5.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        use egui::{FontId, TextStyle};
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.5));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.5));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(11.5));
        s.text_styles.insert(TextStyle::Heading, FontId::new(17.0, FontFamily::Name("bold".into())));
    });
}

pub fn bold(size: f32) -> egui::FontId { egui::FontId::new(size, FontFamily::Name("bold".into())) }
pub fn regular(size: f32) -> egui::FontId { egui::FontId::proportional(size) }

/// Dégradé de remplacement déterministe, pour qu'une vignette sans image ait l'air voulue.
pub fn placeholder_colors(seed: &str) -> (Color32, Color32) {
    let mut hash: u32 = 5381;
    for c in seed.chars() { hash = hash.wrapping_mul(33).wrapping_add(c as u32) & 0xffffff; }
    let hue = (hash % 360) as f32 / 360.0;
    let a = egui::ecolor::HsvaGamma { h: hue, s: 0.38, v: 0.42, a: 1.0 };
    let b = egui::ecolor::HsvaGamma { h: (hue + 0.08) % 1.0, s: 0.45, v: 0.24, a: 1.0 };
    (a.into(), b.into())
}
