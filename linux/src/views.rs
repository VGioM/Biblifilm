//! La bibliothèque : barre latérale, barre d'outils, grille d'affiches (un
//! film, ou une saison en pile), page d'une saison, panneau d'informations,
//! feuille « Corriger », réglages. Les clics deviennent des actions, appliquées
//! une fois l'image dessinée.

use crate::app::{App, Correction, SettingsTab};
use crate::common::images::{Size, Source};
use crate::common::{icons, selfupdate::SelfUpdate, sys, theme, ui};
use crate::metadata::Candidate;
use crate::model::{self, Entry, Season};
use crate::parser::Kind;
use egui::{Align, Align2, Color32, CursorIcon, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2};
use serde_json::json;

pub enum Action {
    Select(String),
    SelectItem(Option<String>),
    OpenSeason(Season),
    CloseSeason,
    Play(String, bool),
    PlayEntry(Entry),
    PlayAll,
    External(String),
    SetWatched(Vec<String>, bool),
    Refetch(String, Option<String>),
    RefetchMany(Vec<String>),
    Correct(String),
    Reveal(String),
    CopyPath(String),
    ChooseFolder,
    Rescan,
    FetchMissing,
    Settings(SettingsTab),
    Setting(&'static str, serde_json::Value),
    ApplyCandidate(String, Candidate, String),
    CheckUpdates,
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let mut acts: Vec<Action> = vec![];
    shortcuts(app, ctx, &mut acts);
    let p = app.p;

    egui::SidePanel::left("sidebar").resizable(false).exact_width(215.0)
        .frame(egui::Frame::new().fill(p.sidebar).inner_margin(egui::Margin { left: 8, right: 8, top: 10, bottom: 8 }).stroke(Stroke::new(1.0_f32, p.hairline)))
        .show(ctx, |ui| sidebar(app, ui, &mut acts));

    let entries: Vec<Entry> = if app.opened_season.is_some() { vec![] } else {
        model::visible_entries(&app.items, &app.sel(), &app.search, &app.settings.string("sortOrder"), &app.w())
    };

    egui::TopBottomPanel::top("toolbar").exact_height(54.0)
        .frame(egui::Frame::new().fill(p.toolbar).inner_margin(egui::Margin::symmetric(12, 8)).stroke(Stroke::new(1.0_f32, p.hairline)))
        .show(ctx, |ui| toolbar(app, ui, &entries, &mut acts));

    banner(app, ctx, &mut acts);

    let selected = app.selected.clone().and_then(|id| app.index_of(&id));
    if let (Some(i), true) = (selected, app.settings.bool("inspectorVisible")) {
        egui::SidePanel::right("inspector").resizable(false).exact_width(305.0)
            .frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0_f32, p.hairline)))
            .show(ctx, |ui| egui::ScrollArea::vertical().show(ui, |ui| inspector(app, ui, i, &mut acts)));
    }

    egui::CentralPanel::default().frame(egui::Frame::new().fill(p.content)).show(ctx, |ui| {
        if app.library_path().is_none() {
            empty(ui, &p, "folder", "Aucun dossier sélectionné", "Choisissez le dossier qui contient vos films et séries. Biblifilm l'analysera et retrouvera les titres et les jaquettes.", Some(("Choisir un dossier…", Action::ChooseFolder)), &mut acts);
        } else if app.scanning && app.items.is_empty() {
            ui.centered_and_justified(|ui| { ui.vertical_centered(|ui| { ui.add_space(ui.available_height() / 2.0 - 30.0); ui.add(egui::Spinner::new().size(30.0)); ui.label(RichText::new(app.scan_progress.lock().unwrap().clone()).color(p.muted)); }); });
        } else if let Some(s) = current_season(app) {
            season_page(app, ui, &s, &mut acts);
        } else if entries.is_empty() {
            if app.search.trim().is_empty() { empty(ui, &p, "film", "Rien à afficher ici", "Aucun média ne correspond à cette catégorie.", None, &mut acts); }
            else { let m = format!("Aucun média ne correspond à « {} ».", app.search.trim()); empty(ui, &p, "search", "Aucun résultat", &m, None, &mut acts); }
        } else {
            grid(app, ui, &entries, &mut acts);
        }
    });

    correction_sheet(app, ctx, &mut acts);
    settings_sheet(app, ctx, &mut acts);
    if let Some(e) = app.error.clone() {
        let close = ui::sheet(ctx, "error", 420.0, |ui| {
            ui.label(RichText::new("Erreur").font(theme::bold(15.0)));
            ui.add(egui::Label::new(&e).selectable(true));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.button("OK").clicked()).inner
        });
        if close { app.error = None; }
    }
    apply(app, ctx, acts);
}

fn current_season(app: &App) -> Option<Season> {
    let id = app.opened_season.as_ref()?;
    let all: Vec<usize> = (0..app.items.len()).collect();
    model::build(&app.items, &all).into_iter().find_map(|e| match e { Entry::Season(s) if &s.id == id => Some(s), _ => None })
}

fn apply(app: &mut App, ctx: &egui::Context, acts: Vec<Action>) {
    for a in acts {
        match a {
            Action::Select(s) => { app.opened_season = None; app.selected = None; app.settings.set("lastSelection", s); }
            Action::SelectItem(id) => { if id.is_some() && !app.settings.bool("inspectorVisible") { app.settings.set("inspectorVisible", true); } app.selected = id; }
            Action::OpenSeason(s) => { app.selected = s.episodes.first().map(|&i| app.items[i].id.clone()); app.opened_season = Some(s.id); }
            Action::CloseSeason => { app.opened_season = None; app.selected = None; }
            Action::Play(id, fs) => app.play(ctx, &id, fs, None),
            Action::PlayEntry(e) => app.play_entry(ctx, &e),
            Action::PlayAll => {
                let entries = model::visible_entries(&app.items, &app.sel(), &app.search, &app.settings.string("sortOrder"), &app.w());
                let ids: Vec<usize> = entries.iter().flat_map(|e| match e { Entry::Single(i) => vec![*i], Entry::Season(s) => s.episodes.clone() }).collect();
                app.play_all(ctx, ids);
            }
            Action::External(id) => app.open_externally(&id),
            Action::SetWatched(ids, w) => app.watch.set_watched(&ids, w),
            Action::Refetch(id, o) => app.refetch(ctx, &id, o),
            Action::RefetchMany(ids) => for id in ids { app.refetch(ctx, &id, None) },
            Action::Correct(id) => {
                if let Some(i) = app.index_of(&id) {
                    // Ce que dit le nom du fichier, pas la correspondance affichée : c'est souvent elle qui est fausse.
                    app.correction = Some(Correction { item: id, query: app.items[i].parsed.title.clone(), results: None, searching: false });
                    app.search_candidates(ctx);
                }
            }
            Action::Reveal(id) => if let Some(i) = app.index_of(&id) { sys::reveal(&app.items[i].path) },
            Action::CopyPath(id) => if let Some(i) = app.index_of(&id) { sys::copy_text(&app.items[i].path.to_string_lossy()) },
            Action::ChooseFolder => app.choose_folder(),
            Action::Rescan => app.rescan(ctx, false),
            Action::FetchMissing => app.fetch_metadata(ctx, false),
            Action::Settings(t) => app.settings_open = Some(t),
            Action::Setting(k, v) => app.set_setting(ctx, k, v),
            Action::ApplyCandidate(id, c, q) => app.apply_candidate(ctx, &id, c, q),
            Action::CheckUpdates => app.update.check_now(ctx),
        }
    }
}

fn shortcuts(app: &mut App, ctx: &egui::Context, acts: &mut Vec<Action>) {
    let modal = app.settings_open.is_some() || app.correction.is_some() || app.error.is_some();
    ctx.input(|i| {
        let ctrl = i.modifiers.command;
        if ctrl && i.key_pressed(egui::Key::O) { acts.push(Action::ChooseFolder); }
        if ctrl && i.key_pressed(egui::Key::R) { acts.push(Action::Rescan); }
        if ctrl && i.key_pressed(egui::Key::F) { app.focus_search = true; }
        if ctrl && i.key_pressed(egui::Key::Comma) { acts.push(Action::Settings(SettingsTab::General)); }
        if ctrl && i.key_pressed(egui::Key::I) { acts.push(Action::Setting("inspectorVisible", json!(!app.settings.bool("inspectorVisible")))); }
        if i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft) { acts.push(Action::CloseSeason); }
        if !modal && !i.focused && i.key_pressed(egui::Key::Escape) {
            if app.opened_season.is_some() { acts.push(Action::CloseSeason); } else if app.selected.is_some() { acts.push(Action::SelectItem(None)); }
        }
        if !modal && i.key_pressed(egui::Key::Enter) && !ctx.wants_keyboard_input() {
            if let Some(id) = &app.selected { acts.push(Action::Play(id.clone(), false)); }
        }
    });
}

fn empty(ui: &mut Ui, p: &theme::Palette, icon: &str, title: &str, msg: &str, action: Option<(&str, Action)>, acts: &mut Vec<Action>) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - 90.0).max(20.0));
        ui.add(icons::image(icon, 46.0, p.tertiary));
        ui.add_space(6.0);
        ui.label(RichText::new(title).font(theme::bold(17.0)));
        ui.add_sized([380.0, 0.0], egui::Label::new(RichText::new(msg).color(p.muted)).wrap());
        if let Some((label, a)) = action {
            ui.add_space(6.0);
            if ui::primary(ui, p, label).clicked() { acts.push(a); }
        }
    });
}

// ── Barre latérale ────────────────────────────────────────────────────────

fn sidebar(app: &mut App, ui: &mut Ui, acts: &mut Vec<Action>) {
    let p = app.p;
    let c = model::counts(&app.items, &app.w());
    let sel = app.sel();
    egui::ScrollArea::vertical().auto_shrink(false).max_height(ui.available_height() - 64.0).show(ui, |ui| {
        ui::section_title(ui, &p, "Bibliothèque");
        for (id, title, icon) in model::SECTIONS {
            let count = match *id { "all" => c.all, "continueWatching" => c.resume, "movies" => c.movies, "shows" => c.shows, "audio" => c.audio, "unmatched" => c.unmatched, _ => 0 };
            if (*id == "continueWatching" && c.resume == 0) || (*id == "unmatched" && c.unmatched == 0) || (*id == "audio" && !app.settings.bool("includeAudio")) { continue; }
            let cnt = if count > 0 { Some(count.to_string()) } else { None };
            if ui::side_row(ui, &p, icon, title, cnt, None, sel == *id && app.opened_season.is_none()).clicked() { acts.push(Action::Select(id.to_string())); }
        }
        let series = model::series_names(&app.items);
        if !series.is_empty() {
            let open = app.settings.bool("seriesExpanded");
            ui.add_space(8.0);
            let r = ui.add(egui::Button::image_and_text(icons::image(if open { "chevronDown" } else { "chevronRight" }, 12.0, p.muted), RichText::new("Séries").font(theme::bold(11.0)).color(p.muted)).frame(false));
            if r.clicked() { acts.push(Action::Setting("seriesExpanded", json!(!open))); }
            if open {
                for n in series {
                    let key = format!("series:{n}");
                    if ui::side_row(ui, &p, "stack", &n, None, None, sel == key).clicked() { acts.push(Action::Select(key)); }
                }
            }
        }
    });
    ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
        if ui.add(egui::Button::image_and_text(icons::image("gear", 14.0, p.muted), RichText::new("Réglages…").color(p.muted)).frame(false)).clicked() { acts.push(Action::Settings(SettingsTab::General)); }
        let label = if app.library_path().is_some() { "Changer de dossier…" } else { "Choisir un dossier…" };
        if ui.add(egui::Button::image_and_text(icons::image("folderPlus", 14.0, p.muted), RichText::new(label).color(p.muted)).frame(false)).clicked() { acts.push(Action::ChooseFolder); }
        ui.separator();
    });
}

// ── Barre d'outils ────────────────────────────────────────────────────────

fn toolbar(app: &mut App, ui: &mut Ui, entries: &[Entry], acts: &mut Vec<Action>) {
    let p = app.p;
    let season = current_season(app);
    ui.horizontal_centered(|ui| {
        if season.is_some() && ui::icon_button(ui, &p, "chevronLeft", 18.0, "Retour à la bibliothèque (Alt+←)").clicked() { acts.push(Action::CloseSeason); }
        if ui.add_enabled(!entries.is_empty(), egui::Button::image(icons::image("playCircle", 19.0, p.text)).frame(false)).on_hover_text("Tout lire").clicked() { acts.push(Action::PlayAll); }
        let title = season.as_ref().map(|s| s.series_title.clone()).unwrap_or_else(|| model::section_title(&app.sel()));
        let sub = if app.scanning { app.scan_progress.lock().unwrap().clone() }
            else if app.fetching { format!("Recherche des infos… {} %", if app.fetch_total > 0 { app.fetch_done * 100 / app.fetch_total } else { 0 }) }
            else if let Some(s) = &season { s.subtitle() }
            else if !entries.is_empty() { format!("{} élément{}", entries.len(), if entries.len() > 1 { "s" } else { "" }) } else { String::new() };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(title).font(theme::bold(15.0)));
            ui.label(RichText::new(sub).size(11.5).color(p.muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let focus = std::mem::take(&mut app.focus_search);
            let before = app.search.clone();
            ui::search_field(ui, &p, &mut app.search, "Rechercher", 220.0, focus);
            if app.search != before { app.opened_season = None; }
            if ui::icon_button(ui, &p, "sidebar", 18.0, "Afficher/masquer les informations (Ctrl+I)").clicked() {
                acts.push(Action::Setting("inspectorVisible", json!(!app.settings.bool("inspectorVisible"))));
            }
            ui.menu_image_button(icons::image("more", 19.0, p.text), |ui| {
                ui.set_min_width(230.0);
                let sort = app.settings.string("sortOrder");
                for (id, label) in model::SORTS {
                    if ui.radio(sort == *id, format!("Trier par {}", label.to_lowercase())).clicked() { acts.push(Action::Setting("sortOrder", json!(id))); ui.close_menu(); }
                }
                ui.separator();
                if ui.add_enabled(app.library_path().is_some(), egui::Button::new("Analyser à nouveau")).clicked() { acts.push(Action::Rescan); ui.close_menu(); }
                if ui.add_enabled(!app.items.is_empty(), egui::Button::new("Rechercher les infos manquantes")).clicked() { acts.push(Action::FetchMissing); ui.close_menu(); }
                ui.separator();
                if ui.button("Réglages…").clicked() { acts.push(Action::Settings(SettingsTab::General)); ui.close_menu(); }
                if ui.button(crate::common::selfupdate::MENU_TITLE).clicked() { acts.push(Action::CheckUpdates); ui.close_menu(); }
            }).response.on_hover_text("Options");
            if app.fetching { ui.add(egui::Spinner::new().size(14.0)); }
        });
    });
}

fn banner(app: &mut App, ctx: &egui::Context, acts: &mut Vec<Action>) {
    // Sans clé TMDB, les films ne peuvent pas être identifiés : le dire franchement.
    let unmatched = app.items.iter().filter(|i| i.matched().is_none()).count();
    if app.tmdb_key().is_some() || unmatched == 0 || app.banner_dismissed || app.library_path().is_none() || app.opened_season.is_some() { return; }
    let p = app.p;
    egui::TopBottomPanel::top("banner").frame(egui::Frame::new().fill(Color32::from_rgb(0x3a, 0x3a, 0x3e)).inner_margin(egui::Margin::symmetric(16, 9))).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add(icons::image("key", 16.0, p.accent));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(RichText::new(format!("{unmatched} fichiers non identifiés")).font(theme::bold(12.5)));
                ui.label(RichText::new("Sans clé TMDB, les films ne peuvent pas être identifiés — seules les séries le sont.").size(11.5).color(p.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::icon_button(ui, &p, "x", 12.0, "").clicked() { app.banner_dismissed = true; }
                if ui::primary(ui, &p, "Ajouter une clé…").clicked() { acts.push(Action::Settings(SettingsTab::Metadata)); }
            });
        });
    });
}

// ── Grille ────────────────────────────────────────────────────────────────

fn entry_menu(app: &App, ui: &mut Ui, e: &Entry, acts: &mut Vec<Action>) {
    match e {
        Entry::Single(i) => item_menu(app, ui, *i, acts),
        Entry::Season(s) => {
            let ids: Vec<String> = s.episodes.iter().map(|&i| app.items[i].id.clone()).collect();
            if ui.button("Voir les épisodes").clicked() { acts.push(Action::OpenSeason(s.clone())); ui.close_menu(); }
            if ui.button("Lire la saison").clicked() { acts.push(Action::PlayEntry(e.clone())); ui.close_menu(); }
            ui.separator();
            if ui.button("Marquer la saison comme vue").clicked() { acts.push(Action::SetWatched(ids.clone(), true)); ui.close_menu(); }
            if ui.button("Marquer comme non vue").clicked() { acts.push(Action::SetWatched(ids.clone(), false)); ui.close_menu(); }
            ui.separator();
            if ui.button("Rechercher les infos à nouveau").clicked() { acts.push(Action::RefetchMany(ids)); ui.close_menu(); }
        }
    }
}

fn item_menu(app: &App, ui: &mut Ui, i: usize, acts: &mut Vec<Action>) {
    let w = app.w();
    let id = app.items[i].id.clone();
    if ui.button("Lire").clicked() { acts.push(Action::Play(id.clone(), false)); ui.close_menu(); }
    if w.resume(i).is_some() && ui.button("Lire depuis le début").clicked() { acts.push(Action::Play(id.clone(), true)); ui.close_menu(); }
    let seen = w.watched(i);
    if ui.button(if seen { "Marquer comme non vu" } else { "Marquer comme vu" }).clicked() { acts.push(Action::SetWatched(vec![id.clone()], !seen)); ui.close_menu(); }
    ui.separator();
    if ui.button(if app.has_vlc() { "Ouvrir dans VLC" } else { "Ouvrir dans un lecteur externe" }).clicked() { acts.push(Action::External(id.clone())); ui.close_menu(); }
    if ui.button("Afficher dans le dossier").clicked() { acts.push(Action::Reveal(id.clone())); ui.close_menu(); }
    ui.separator();
    if ui.button("Rechercher les infos à nouveau").clicked() { acts.push(Action::Refetch(id.clone(), None)); ui.close_menu(); }
    if ui.button("Corriger le titre…").clicked() { acts.push(Action::Correct(id.clone())); ui.close_menu(); }
    if ui.button("Copier le chemin").clicked() { acts.push(Action::CopyPath(id)); ui.close_menu(); }
}

fn grid(app: &mut App, ui: &mut Ui, entries: &[Entry], acts: &mut Vec<Action>) {
    let p = app.p;
    let (pad, gap) = (22.0, 16.0);
    let width = ui.available_width() - 2.0 * pad;
    let cols = ((width + gap) / (148.0 + gap)).floor().max(1.0) as usize;
    let card_w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).min(230.0);
    let cover_h = card_w * 1.5;
    let row_h = 10.0 + cover_h + 50.0;
    let rows = entries.len().div_ceil(cols);
    let mut clicked_any = false;
    let scroll = egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, rows, |ui, range| {
        ui.add_space(18.0);
        for r in range {
            let (row_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
            for c in 0..cols {
                let k = r * cols + c;
                let Some(e) = entries.get(k) else { break };
                let x = row_rect.left() + pad + c as f32 * (card_w + gap);
                let card = Rect::from_min_size(egui::pos2(x, row_rect.top()), Vec2::new(card_w, row_h - 4.0));
                if card_ui(app, ui, e, card, cover_h, acts) { clicked_any = true; }
            }
        }
        ui.add_space(30.0);
    });
    // Un clic qui manque les affiches désélectionne, ce qui ferme le panneau.
    let bg = ui.interact(scroll.inner_rect, ui.id().with("grid-bg"), Sense::click());
    if bg.clicked() && !clicked_any { acts.push(Action::SelectItem(None)); }
    let _ = p;
}

/// Une tuile. Rend vrai si elle a été cliquée.
fn card_ui(app: &mut App, ui: &mut Ui, e: &Entry, rect: Rect, cover_h: f32, acts: &mut Vec<Action>) -> bool {
    let p = app.p;
    let season = matches!(e, Entry::Season(_));
    let key = e.key(&app.items);
    let id = ui.id().with(&key);
    let resp = ui.interact(rect, id, Sense::click());
    let hovered = resp.hovered() || resp.context_menu_opened();
    let selected = matches!(e, Entry::Single(i) if app.selected.as_deref() == Some(app.items[*i].id.as_str()));
    let painter = ui.painter_at(rect.expand(12.0));
    if selected { painter.rect_filled(rect.expand(7.0), 10, Color32::from_white_alpha(26)); }
    let kind = e.kind(&app.items);
    let aspect_h = if kind == Kind::Audio { rect.width() } else { cover_h };
    let top = rect.top() + 10.0;
    let cover = Rect::from_min_size(egui::pos2(rect.left(), top), Vec2::new(rect.width(), aspect_h));
    if season {
        // Des feuillets qui dépassent derrière l'affiche : « plusieurs épisodes ».
        painter.rect_filled(Rect::from_min_max(egui::pos2(cover.left() + 15.0, top - 9.0), egui::pos2(cover.right() - 15.0, top + 20.0)), 8, Color32::from_white_alpha(33));
        painter.rect_filled(Rect::from_min_max(egui::pos2(cover.left() + 7.5, top - 4.5), egui::pos2(cover.right() - 7.5, top + 20.0)), 8, Color32::from_white_alpha(51));
    }
    ui::shadow(ui, cover, 8, hovered);
    let title = e.title(&app.items);
    let source = e.poster(&app.items).map(Source::Url);
    ui::cover(ui, &mut app.images, cover, source.as_ref(), &title, kind.icon(), 8, Size::Thumb, false);
    let w = app.w();
    let progress = w.entry_progress(e);
    let done = w.entry_watched(e);
    let seen = if let Entry::Season(s) = e { w.watched_count(s) } else { 0 };
    let pnt = ui.painter();
    // La barre de reprise en bas de l'affiche.
    if progress > 0.01 && !done {
        let bar = Rect::from_min_max(egui::pos2(cover.left(), cover.bottom() - 3.5), cover.right_bottom());
        pnt.rect_filled(bar, egui::CornerRadius { sw: 8, se: 8, ..Default::default() }, Color32::from_black_alpha(140));
        pnt.rect_filled(Rect::from_min_size(bar.min, Vec2::new((bar.width() * progress as f32).max(3.0), bar.height())), egui::CornerRadius { sw: 8, ..Default::default() }, p.accent);
    }
    if done {
        let c = egui::pos2(cover.right() - 15.0, cover.top() + 15.0);
        pnt.circle_filled(c, 9.0, p.accent);
        icons::image("check", 12.0, Color32::WHITE).paint_at(ui, Rect::from_center_size(c, Vec2::splat(12.0)));
    }
    if let Entry::Season(s) = e { ui::badge(ui, cover.right_bottom() + Vec2::new(-7.0, -8.0), Align2::RIGHT_BOTTOM, &s.episodes.len().to_string()); }
    let mut clicked = false;
    if hovered {
        pnt.rect_filled(cover, 8, Color32::from_black_alpha(87));
        let c = cover.center();
        if season {
            if ui::circle_button(ui, c - Vec2::new(25.0, 0.0), "list", id.with("list")).clicked() { if let Entry::Season(s) = e { acts.push(Action::OpenSeason(s.clone())); } clicked = true; }
            if ui::circle_button(ui, c + Vec2::new(25.0, 0.0), "play", id.with("play")).clicked() { acts.push(Action::PlayEntry(e.clone())); clicked = true; }
        } else if ui::circle_button(ui, c, "play", id.with("play")).clicked() { acts.push(Action::PlayEntry(e.clone())); clicked = true; }
        ui.ctx().set_cursor_icon(CursorIcon::Default);
    }
    let ty = cover.bottom() + 7.0;
    ui::text_in(ui, Rect::from_min_size(egui::pos2(rect.left(), ty), Vec2::new(rect.width(), 17.0)), &title, theme::regular(12.5), p.text);
    let mut sub = e.subtitle(&app.items);
    if let Entry::Season(s) = e { if seen > 0 { sub = format!("{sub} · {seen}/{} vus", s.episodes.len()); } }
    ui::text_in(ui, Rect::from_min_size(egui::pos2(rect.left(), ty + 18.0), Vec2::new(rect.width(), 15.0)), &sub, theme::regular(11.5), p.muted);
    if !clicked {
        if resp.double_clicked() { match e { Entry::Season(s) => acts.push(Action::OpenSeason(s.clone())), _ => acts.push(Action::PlayEntry(e.clone())) } clicked = true; }
        else if resp.clicked() { match e { Entry::Season(s) => acts.push(Action::OpenSeason(s.clone())), Entry::Single(i) => acts.push(Action::SelectItem(Some(app.items[*i].id.clone()))) } clicked = true; }
    }
    resp.context_menu(|ui| entry_menu(app, ui, e, acts));
    clicked
}

// ── Page d'une saison ─────────────────────────────────────────────────────

fn season_page(app: &mut App, ui: &mut Ui, s: &Season, acts: &mut Vec<Action>) {
    let p = app.p;
    let ids: Vec<String> = s.episodes.iter().map(|&i| app.items[i].id.clone()).collect();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        egui::Frame::new().inner_margin(22).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(168.0, 252.0), Sense::hover());
                ui::shadow(ui, r, 10, true);
                let src = s.poster(&app.items).map(Source::Url);
                ui::cover(ui, &mut app.images, r, src.as_ref(), &s.series_title, "tv", 10, Size::Large, false);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(&s.series_title).font(theme::bold(22.0))).wrap());
                    ui.label(RichText::new(s.subtitle()).size(17.0).color(p.accent));
                    let meta: Vec<String> = [s.year(&app.items).map(|y| y.to_string()), Some(sys::bytes(s.total_size(&app.items)))].into_iter().flatten().collect();
                    ui.label(RichText::new(meta.join(" · ")).color(p.muted));
                    let g = s.genres(&app.items);
                    if !g.is_empty() { ui::chips(ui, &p, &g.into_iter().take(3).collect::<Vec<_>>()); }
                    if let Some(o) = s.overview(&app.items) {
                        let mut job = egui::text::LayoutJob::simple(o, theme::regular(13.0), p.text.gamma_multiply(0.85), ui.available_width().min(700.0));
                        job.wrap.max_rows = 4;
                        ui.label(job);
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let w = app.w();
                        let next = w.next_episode(s);
                        let label = match next {
                            Some(n) if w.resume(n).is_some() => format!("Reprendre l'épisode {}", app.items[n].episode_number().unwrap_or(1)),
                            Some(n) if w.watched_count(s) > 0 => format!("Lire l'épisode {}", app.items[n].episode_number().unwrap_or(1)),
                            _ => "Lire la saison".into(),
                        };
                        if ui::primary_icon(ui, &p, "play", &label, true).clicked() { acts.push(Action::PlayEntry(Entry::Season(s.clone()))); }
                        ui.menu_image_button(icons::image("more", 18.0, p.text), |ui| {
                            if ui.button("Marquer la saison comme vue").clicked() { acts.push(Action::SetWatched(ids.clone(), true)); ui.close_menu(); }
                            if ui.button("Marquer comme non vue").clicked() { acts.push(Action::SetWatched(ids.clone(), false)); ui.close_menu(); }
                            if ui.button("Rechercher les infos à nouveau").clicked() { acts.push(Action::RefetchMany(ids.clone())); ui.close_menu(); }
                        });
                    });
                    if !s.identified(&app.items) {
                        ui.label(RichText::new("Série non identifiée — ajoutez une clé TMDB dans les réglages, ou utilisez « Corriger » sur un épisode.").size(11.5).color(p.muted));
                    }
                });
            });
            ui.add_space(14.0);
            ui.separator();
            for (k, &i) in s.episodes.iter().enumerate() {
                episode_row(app, ui, i, k, acts);
                if k + 1 < s.episodes.len() {
                    let r = ui.available_rect_before_wrap();
                    ui.painter().hline((r.left() + 44.0)..=r.right(), r.top(), Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
                }
            }
        });
    });
}

fn episode_row(app: &App, ui: &mut Ui, i: usize, k: usize, acts: &mut Vec<Action>) {
    let p = app.p;
    let it = &app.items[i];
    let w = app.w();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
    let selected = app.selected.as_deref() == Some(it.id.as_str());
    let hovered = resp.hovered();
    if selected { ui.painter().rect_filled(rect, 7, Color32::from_white_alpha(26)); } else if hovered { ui.painter().rect_filled(rect, 7, Color32::from_white_alpha(13)); }
    let num = Rect::from_min_size(rect.min + Vec2::new(10.0, 0.0), Vec2::new(32.0, rect.height()));
    let seen = w.watched(i);
    let mut play_clicked = false;
    if hovered {
        let pr = ui.interact(Rect::from_center_size(num.center(), Vec2::splat(24.0)), ui.id().with(("ep-play", i)), Sense::click());
        icons::image("play", 13.0, p.text).paint_at(ui, Rect::from_center_size(num.center(), Vec2::splat(13.0)));
        if pr.clicked() { play_clicked = true; acts.push(Action::Play(it.id.clone(), false)); }
    } else if seen {
        icons::image("check", 12.0, p.accent).paint_at(ui, Rect::from_center_size(num.center(), Vec2::splat(12.0)));
    } else {
        ui.painter().text(num.center(), Align2::CENTER_CENTER, it.episode_number().unwrap_or(k as i32 + 1).to_string(), theme::regular(12.5), p.muted);
    }
    let size_txt = sys::bytes(it.size);
    let sg = ui.painter().layout_no_wrap(size_txt, theme::regular(11.5), p.muted);
    ui.painter().galley(egui::pos2(rect.right() - 10.0 - sg.size().x, rect.center().y - sg.size().y / 2.0), sg.clone(), p.muted);
    let tx = rect.left() + 54.0;
    let tw = rect.right() - 20.0 - sg.size().x - tx;
    ui::text_in(ui, Rect::from_min_size(egui::pos2(tx, rect.top() + 7.0), Vec2::new(tw, 17.0)), &it.row_title(), theme::regular(13.0), if seen { p.muted } else { p.text });
    let line2 = Rect::from_min_size(egui::pos2(tx, rect.top() + 27.0), Vec2::new(tw, 13.0));
    if let (Some(_), Some(r)) = (w.resume(i), w.rec(i)) {
        let bar = Rect::from_min_size(egui::pos2(tx, line2.center().y - 1.5), Vec2::new(90.0, 3.0));
        ui.painter().rect_filled(bar, 2, Color32::from_white_alpha(36));
        ui.painter().rect_filled(Rect::from_min_size(bar.min, Vec2::new((90.0 * r.progress() as f32).max(3.0), 3.0)), 2, p.accent);
        let left = (r.duration - r.position).max(0.0) / 60.0;
        let rem = if left >= 60.0 { format!("{} h {} min", (left / 60.0) as i64, (left % 60.0) as i64) } else { format!("{} min", left as i64) };
        ui.painter().text(egui::pos2(tx + 98.0, line2.center().y), Align2::LEFT_CENTER, format!("reste {rem}  · {}", model::format_time(r.position)), theme::regular(10.5), p.accent);
    } else {
        ui::text_in(ui, line2, &it.filename, theme::regular(11.0), p.tertiary);
    }
    if !play_clicked {
        if resp.double_clicked() { acts.push(Action::Play(it.id.clone(), false)); }
        else if resp.clicked() { acts.push(Action::SelectItem(Some(it.id.clone()))); }
    }
    resp.context_menu(|ui| item_menu(app, ui, i, acts));
}

// ── Panneau d'informations ────────────────────────────────────────────────

fn inspector(app: &mut App, ui: &mut Ui, i: usize, acts: &mut Vec<Action>) {
    let p = app.p;
    egui::Frame::new().inner_margin(18).show(ui, |ui| {
        ui.set_width(269.0);
        let it = app.items[i].clone();
        let m = it.matched().cloned();
        let still = m.as_ref().and_then(|m| m.still_url.clone());
        let src = still.clone().or(m.as_ref().and_then(|m| m.poster_url.clone())).map(Source::Url);
        let h = if still.is_some() { 269.0 * 9.0 / 16.0 } else if it.kind() == Kind::Audio { 269.0 } else { 269.0 * 1.5 };
        let (r, _) = ui.allocate_exact_size(Vec2::new(269.0, h), Sense::hover());
        ui::shadow(ui, r, 10, true);
        ui::cover(ui, &mut app.images, r, src.as_ref(), &it.grouping_title(), it.kind().icon(), 10, Size::Large, false);
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(it.display_title()).font(theme::bold(17.0))).wrap().selectable(true));
        if let (Some(s), Kind::Episode) = (m.as_ref().and_then(|m| m.series_title.clone()), it.kind()) { ui.label(RichText::new(s).color(p.accent)); }
        ui.label(RichText::new(it.display_subtitle()).color(p.muted));
        ui.add_space(6.0);
        let w = app.w();
        let resume = w.resume(i);
        let full = |ui: &mut Ui, b: egui::Button| ui.add_sized([269.0, 30.0], b);
        let label = match (resume, w.rec(i)) { (Some(_), Some(r)) => format!("Reprendre à {}", model::format_time(r.position)), _ => "Lire".into() };
        if ui.add_sized([269.0, 34.0], egui::Button::image_and_text(icons::image("play", 13.0, p.on_accent), RichText::new(label).color(p.on_accent).strong()).fill(p.accent)).clicked() { acts.push(Action::Play(it.id.clone(), false)); }
        if resume.is_some() && full(ui, egui::Button::image_and_text(icons::image("restart", 13.0, p.text), "Reprendre au début")).clicked() { acts.push(Action::Play(it.id.clone(), true)); }
        let ext = if app.has_vlc() { "Ouvrir dans VLC" } else { "Ouvrir dans un lecteur externe" };
        if full(ui, egui::Button::image_and_text(icons::image("external", 14.0, p.text), ext)).clicked() { acts.push(Action::External(it.id.clone())); }
        let seen = w.watched(i);
        if full(ui, egui::Button::image_and_text(icons::image(if seen { "eyeOff" } else { "checkCircle" }, 14.0, p.text), if seen { "Marquer comme non vu" } else { "Marquer comme vu" })).clicked() { acts.push(Action::SetWatched(vec![it.id.clone()], !seen)); }
        ui.horizontal(|ui| {
            if ui.add_sized([130.5, 30.0], egui::Button::image_and_text(icons::image("folder", 14.0, p.text), "Dossier")).clicked() { acts.push(Action::Reveal(it.id.clone())); }
            if ui.add_sized([130.5, 30.0], egui::Button::image_and_text(icons::image("wand", 14.0, p.text), "Corriger")).clicked() { acts.push(Action::Correct(it.id.clone())); }
        });
        if let Some(o) = m.as_ref().and_then(|m| m.overview.clone()) {
            ui.add_space(8.0);
            ui.label(RichText::new("Synopsis").font(theme::bold(11.0)).color(p.muted));
            ui.add(egui::Label::new(RichText::new(o).size(12.5)).wrap().selectable(true));
        }
        if let Some(g) = m.as_ref().map(|m| m.genres.clone()).filter(|g| !g.is_empty()) { ui::chips(ui, &p, &g.into_iter().take(3).collect::<Vec<_>>()); }
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Fichier").font(theme::bold(11.0)).color(p.muted));
        ui::info_row(ui, &p, "Nom", &it.filename);
        ui::info_row(ui, &p, "Type", it.kind().name());
        ui::info_row(ui, &p, "Taille", &sys::bytes(it.size));
        ui::info_row(ui, &p, "Format", &it.ext.to_uppercase());
        if let Some(r) = m.as_ref().and_then(|m| m.rating).filter(|r| *r > 0.0) { ui::info_row(ui, &p, "Note", &format!("{:.1}/10", r).replace('.', ",")); }
        if let Some(s) = m.as_ref().and_then(|m| m.source.clone()) { ui::info_row(ui, &p, "Source", &s); }
        if let Some(g) = &it.parsed.release_group { ui::info_row(ui, &p, "Groupe", g); }
        ui::info_row(ui, &p, "Chemin", &it.path.to_string_lossy());
        if m.is_none() {
            ui.add_space(8.0);
            egui::Frame::new().fill(Color32::from_white_alpha(13)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                ui.label(RichText::new("Titre non identifié").font(theme::bold(12.0)));
                ui.label(RichText::new(if app.tmdb_key().is_some() { "Aucune correspondance trouvée. Essayez « Corriger » pour choisir le bon titre." } else { "Ajoutez une clé TMDB dans les réglages pour de bien meilleurs résultats." }).size(11.5).color(p.muted));
            });
        }
    });
}

// ── Corriger ──────────────────────────────────────────────────────────────

fn correction_sheet(app: &mut App, ctx: &egui::Context, acts: &mut Vec<Action>) {
    let Some(c) = &app.correction else { return };
    let p = app.p;
    let Some(i) = app.index_of(&c.item) else { app.correction = None; return };
    let series = app.items[i].kind() == Kind::Episode;
    let has_key = app.tmdb_key().is_some();
    let mut search = false;
    let close = ui::sheet(ctx, "correction", 470.0, |ui| {
        let c = app.correction.as_mut().unwrap();
        ui.label(RichText::new("Corriger le titre").font(theme::bold(15.0)));
        ui.label(RichText::new(if series { "Choisissez la bonne série. La correction s'applique à tous les épisodes." } else { "Choisissez le bon film dans la liste." }).size(12.0).color(p.muted));
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut c.query).desired_width(330.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { search = true; }
            if ui::primary(ui, &p, "Rechercher").clicked() { search = true; }
        });
        let mut picked = None;
        egui::Frame::new().fill(Color32::from_white_alpha(13)).corner_radius(8).show(ui, |ui| {
            ui.set_min_height(110.0);
            ui.set_width(ui.available_width());
            if !has_key { ui.centered_and_justified(|ui| ui.label(RichText::new("Ajoutez une clé TMDB dans les réglages pour choisir parmi les titres.").color(p.muted))); }
            else if c.searching { ui.centered_and_justified(|ui| ui.add(egui::Spinner::new())); }
            else if let Some(list) = &c.results {
                if list.is_empty() { ui.centered_and_justified(|ui| ui.label(RichText::new("Aucun résultat. Essayez le titre original, sans l'année.").color(p.muted))); }
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (k, cand) in list.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 78.0), Sense::click());
                        if resp.hovered() { ui.painter().rect_filled(rect, 6, Color32::from_white_alpha(15)); }
                        let cov = Rect::from_min_size(rect.min + Vec2::splat(8.0), Vec2::new(44.0, 62.0));
                        let src = cand.poster_url.clone().map(Source::Url);
                        ui::cover(ui, &mut app.images, cov, src.as_ref(), &cand.title, if cand.is_series { "tv" } else { "film" }, 4, Size::Thumb, false);
                        let x = cov.right() + 10.0;
                        let w = rect.right() - x - 8.0;
                        ui::text_in(ui, Rect::from_min_size(egui::pos2(x, rect.top() + 9.0), Vec2::new(w, 16.0)), &cand.title, theme::bold(13.0), p.text);
                        ui::text_in(ui, Rect::from_min_size(egui::pos2(x, rect.top() + 27.0), Vec2::new(w, 14.0)), &cand.subtitle(), theme::regular(11.5), p.accent);
                        if let Some(o) = &cand.overview {
                            let mut job = egui::text::LayoutJob::simple(o.clone(), theme::regular(10.5), p.muted, w);
                            job.wrap.max_rows = 2;
                            let g = ui.painter().layout_job(job);
                            ui.painter().galley(egui::pos2(x, rect.top() + 44.0), g, p.muted);
                        }
                        if resp.clicked() { picked = Some(k); }
                    }
                });
            }
        });
        if let (Some(k), Some(list)) = (picked, &c.results) { acts.push(Action::ApplyCandidate(c.item.clone(), list[k].clone(), c.query.clone())); return true; }
        ui.horizontal(|ui| {
            if ui.button("Recherche automatique").on_hover_text("Relance la recherche et garde le meilleur résultat.").clicked() { acts.push(Action::Refetch(c.item.clone(), Some(c.query.clone()))); return true; }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.button("Annuler").clicked()).inner
        }).inner
    });
    if search { app.search_candidates(ctx); }
    if close { app.correction = None; }
}

// ── Réglages ──────────────────────────────────────────────────────────────

const LANGS: &[(&str, &str)] = &[("fr-FR", "Français"), ("en-US", "English"), ("es-ES", "Español"), ("de-DE", "Deutsch"), ("it-IT", "Italiano"), ("pt-BR", "Português"), ("ja-JP", "日本語")];

fn settings_sheet(app: &mut App, ctx: &egui::Context, acts: &mut Vec<Action>) {
    let Some(tab) = app.settings_open else { return };
    let p = app.p;
    let mut new_tab = tab;
    let close = ui::sheet(ctx, "settings", 560.0, |ui| {
        let x = ui.horizontal(|ui| {
            ui.label(RichText::new("Réglages").font(theme::bold(15.0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui::icon_button(ui, &p, "x", 13.0, "").clicked()).inner
        }).inner;
        ui.horizontal(|ui| {
            for (t, l) in [(SettingsTab::General, "Général"), (SettingsTab::Playback, "Lecture"), (SettingsTab::Metadata, "Métadonnées")] {
                if ui.selectable_label(tab == t, l).clicked() { new_tab = t; }
            }
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| match tab {
            SettingsTab::General => general(app, ui, acts),
            SettingsTab::Playback => playback(app, ui),
            SettingsTab::Metadata => metadata(app, ui, ctx, acts),
        });
        x
    });
    if close { app.settings_open = None; } else { app.settings_open = Some(new_tab); }
}

fn toggle(app: &App, ui: &mut Ui, key: &'static str, label: &str, help: &str, acts: &mut Vec<Action>) {
    let mut v = app.settings.bool(key);
    if ui::check_help(ui, &app.p, &mut v, label, help).changed() { acts.push(Action::Setting(key, json!(v))); }
}

fn general(app: &mut App, ui: &mut Ui, acts: &mut Vec<Action>) {
    let p = app.p;
    ui::group(ui, &p, "Dossier", |ui| {
        ui.horizontal(|ui| {
            let path = app.library_path().map(|p| p.display().to_string()).unwrap_or("Aucun".into());
            ui.add(egui::Label::new(path).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| if ui.button("Choisir…").clicked() { acts.push(Action::ChooseFolder); });
        });
        toggle(app, ui, "includeAudio", "Inclure les fichiers audio", "", acts);
        toggle(app, ui, "skipSmallFiles", "Ignorer les fichiers de moins de 20 Mo", "Évite d'importer les extraits, bandes-annonces et échantillons.", acts);
        toggle(app, ui, "watchFolder", "Détecter automatiquement les nouveaux fichiers", "Surveille le dossier en continu : un film ajouté apparaît tout seul, avec sa jaquette.", acts);
        if ui.add_enabled(app.library_path().is_some(), egui::Button::new("Analyser à nouveau")).clicked() { acts.push(Action::Rescan); }
    });
    ui.add_space(8.0);
    ui::group(ui, &p, "Mises à jour de Biblifilm", |ui| {
        let mut auto = SelfUpdate::automatic(&app.settings);
        if ui::check(ui, &p, &mut auto, crate::common::selfupdate::AUTO_TITLE).changed() { SelfUpdate::set_automatic(&mut app.settings, auto); }
        ui.horizontal(|ui| {
            if ui.button("Rechercher maintenant").clicked() { acts.push(Action::CheckUpdates); }
            ui.label(RichText::new(format!("Version {}", app.update.version())).color(p.muted));
        });
    });
}

fn playback(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    ui::group(ui, &p, "Lecteur", |ui| {
        match app.mpv_error.clone().or_else(crate::mpv::load_error) {
            None => {
                ui.horizontal(|ui| { ui.add(icons::image("checkCircle", 15.0, p.ok)); ui.label("mpv est intégré à Biblifilm"); });
                ui.label(RichText::new("Tous les formats sont lus dans la fenêtre (MKV, x265 10 bits, EAC3, sous-titres ASS). Espace lecture/pause, ←/→ 10 s, ↑/↓ volume, B piste audio, V sous-titres, F plein écran, Échap.").size(11.5).color(p.muted));
            }
            Some(e) => {
                ui.horizontal(|ui| { ui.add(icons::image("warning", 15.0, p.warm)); ui.label(format!("Lecteur indisponible : {e}")); });
                ui.label(RichText::new("La version AppImage embarque libmpv. Depuis les sources, installez la libmpv de votre distribution (paquet mpv-libs ou libmpv2).").size(11.5).color(p.muted));
            }
        }
        ui.horizontal(|ui| {
            ui.add(icons::image(if app.has_vlc() { "checkCircle" } else { "info" }, 15.0, if app.has_vlc() { p.ok } else { p.muted }));
            ui.label(if app.has_vlc() { "VLC : installé (« Ouvrir dans VLC »)" } else { "VLC : introuvable — « Ouvrir dans un lecteur externe » utilise l'app par défaut" });
        });
    });
    ui.add_space(8.0);
    ui::group(ui, &p, "Historique de visionnage", |ui| {
        ui.horizontal(|ui| { ui.label("Épisodes suivis"); ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(app.watch.records.len().to_string()).color(p.muted))); });
        ui.horizontal(|ui| {
            if ui.button("Restaurer la sauvegarde précédente").clicked() {
                let n = app.watch.restore();
                app.restore_msg = Some(if n > 0 { format!("{n} entrées restaurées.") } else { "Aucune sauvegarde utilisable.".into() });
            }
            if let Some(m) = &app.restore_msg { ui.label(RichText::new(m).color(p.muted)); }
        });
    });
}

fn metadata(app: &mut App, ui: &mut Ui, ctx: &egui::Context, acts: &mut Vec<Action>) {
    let p = app.p;
    ui::group(ui, &p, "Clé API TMDB", |ui| {
        let mut key = app.settings.string("tmdbAPIKey");
        if ui.add(egui::TextEdit::singleline(&mut key).password(true).hint_text("Collez votre clé ici").desired_width(f32::INFINITY)).changed() {
            acts.push(Action::Setting("tmdbAPIKey", json!(key)));
        }
        ui.label(RichText::new("Gratuite sur themoviedb.org → Paramètres → API. Sans clé, Biblifilm utilise TVmaze et Wikidata, avec des résultats plus limités.").size(11.5).color(p.muted));
        if ui.link("Obtenir une clé TMDB").clicked() { sys::open_url("https://www.themoviedb.org/settings/api"); }
        ui.horizontal(|ui| {
            if ui.add_enabled(app.tmdb_key().is_some(), egui::Button::new("Tester la clé")).clicked() { app.test_key(ctx); app.key_status = None; }
            match &app.key_status {
                Some(Ok(())) => { ui.label(RichText::new("Clé valide").color(p.ok)); }
                Some(Err(e)) => { ui.label(RichText::new(e).color(p.warm)); }
                None => {}
            }
        });
    });
    ui.add_space(8.0);
    ui::group(ui, &p, "", |ui| {
        ui.horizontal(|ui| {
            ui.label("Langue");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let cur = app.settings.string("preferredLanguage");
                let label = LANGS.iter().find(|l| l.0 == cur).map(|l| l.1).unwrap_or("Français");
                egui::ComboBox::from_id_salt("lang").selected_text(label).show_ui(ui, |ui| {
                    for (c, l) in LANGS { if ui.selectable_label(cur == *c, *l).clicked() { acts.push(Action::Setting("preferredLanguage", json!(c))); } }
                });
            });
        });
        toggle(app, ui, "autoFetchMetadata", "Rechercher automatiquement après l'analyse", "", acts);
    });
    ui.add_space(8.0);
    ui::group(ui, &p, "", |ui| {
        ui.horizontal(|ui| {
            if ui.add_enabled(!app.items.is_empty(), egui::Button::new("Tout rechercher à nouveau")).clicked() { app.fetch_metadata(ctx, true); }
            if ui.button("Vider le cache").clicked() { app.meta.clear(); let c = app.cache.clone(); app.images.clear(&c); }
        });
        ui.label(RichText::new(format!("{} fiches en cache · {} d'images", app.meta.count(), sys::bytes(crate::common::images::disk_size(&app.cache)))).size(11.5).color(p.muted));
    });
}
