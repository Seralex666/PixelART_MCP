//! Datei › Einstellungen mit Tabs: Allgemein (Speicherordner, Sprache), Claude
//! (Verbinden), Stile (die Stil-Bibliothek) und Tipps.

use crate::app::{c32, section, App};
use crate::dialogs::{buttons, connect_info, finish, Outcome};
use eframe::egui::{self, RichText};
use pixel_core::lang::{tr, Lang};
use pixel_core::project::LIGHT_DIRS;
use pixel_core::stylebook::{OutlineRule, StylePreset};
use pixel_core::trf;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Claude,
    Styles,
    Tips,
}

impl SettingsTab {
    const ALL: [SettingsTab; 4] = [
        SettingsTab::General,
        SettingsTab::Claude,
        SettingsTab::Styles,
        SettingsTab::Tips,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => tr("Allgemein", "General"),
            SettingsTab::Claude => tr("Claude verbinden", "Connect Claude"),
            SettingsTab::Styles => tr("Stile", "Styles"),
            SettingsTab::Tips => tr("Tipps", "Tips"),
        }
    }
}

/// Ein eigener Stil, der gerade bearbeitet wird.
pub struct StyleDraft {
    style: StylePreset,
    /// Name beim Öffnen; wird er geändert, verschwindet die alte Datei.
    old_name: Option<String>,
    sizes: String,
    rules: String,
}

impl StyleDraft {
    fn new(style: StylePreset, old_name: Option<String>) -> Self {
        let sizes = style
            .sizes
            .iter()
            .map(|[w, h]| format!("{w}x{h}"))
            .collect::<Vec<_>>()
            .join(", ");
        let rules = style.rules.join("\n");
        Self {
            style,
            old_name,
            sizes,
            rules,
        }
    }
}

/// Formular für Datei › Einstellungen.
pub struct SettingsForm {
    pub home: String,
    pub lang: Lang,
    pub tab: SettingsTab,
    styles: Vec<StylePreset>,
    selected: usize,
    draft: Option<StyleDraft>,
}

impl SettingsForm {
    pub fn new(app: &App) -> Self {
        Self {
            home: app.ws.home().display().to_string(),
            lang: pixel_core::lang::get(),
            tab: SettingsTab::General,
            styles: app.ws.styles(),
            selected: 0,
            draft: None,
        }
    }

    pub fn at(app: &App, tab: SettingsTab) -> Self {
        Self {
            tab,
            ..Self::new(app)
        }
    }

    fn reload(&mut self, app: &App, select: &str) {
        self.styles = app.ws.styles();
        self.selected = self
            .styles
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(select))
            .unwrap_or(0);
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, f: &mut SettingsForm) -> Outcome {
    ui.set_width(660.0);
    ui.heading(tr("Einstellungen", "Settings"));
    ui.horizontal(|ui| {
        for tab in SettingsTab::ALL {
            if ui.selectable_label(f.tab == tab, tab.label()).clicked() {
                f.tab = tab;
            }
        }
    });
    ui.separator();
    match f.tab {
        SettingsTab::General => return general(app, ui, f),
        SettingsTab::Claude => connect_info(app, ui),
        SettingsTab::Styles => styles(app, ui, f),
        SettingsTab::Tips => tips(ui),
    }
    ui.add_space(8.0);
    if ui.button(tr("Schließen", "Close")).clicked() {
        return Outcome::Close;
    }
    Outcome::Keep
}

fn general(app: &mut App, ui: &mut egui::Ui, f: &mut SettingsForm) -> Outcome {
    section(ui, tr("Speicherordner", "Save folder"));
    ui.label(tr(
        "Hier liegen deine Projekte, eigene Formen und Stile, und hierhin wird exportiert.",
        "Your projects, own shapes and styles live here, and exports are written here.",
    ));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut f.home).desired_width(420.0));
        if ui.button(tr("Durchsuchen…", "Browse…")).clicked() {
            let mut dialog =
                rfd::FileDialog::new().set_title(tr("Speicherordner wählen", "Choose save folder"));
            let current = PathBuf::from(f.home.trim());
            if current.is_dir() {
                dialog = dialog.set_directory(&current);
            }
            if let Some(dir) = dialog.pick_folder() {
                f.home = dir.display().to_string();
            }
        }
    });
    let changed = std::path::Path::new(f.home.trim()) != app.ws.home();
    if changed {
        ui.weak(tr(
            "Nach dem Wechsel öffnet sich das zuletzt bearbeitete Projekt im neuen Ordner. Claude arbeitet weiter im alten Ordner, bis du die Verbindung neu einträgst (Tab „Claude verbinden“).",
            "After switching, the most recently edited project in the new folder opens. Claude keeps working in the old folder until you set up the connection again (tab \"Connect Claude\").",
        ));
    }
    if std::env::var_os("PIXELART_HOME").is_some_and(|v| !v.is_empty()) {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            tr(
                "Die Umgebungsvariable PIXELART_HOME ist gesetzt. Sie gilt beim nächsten Start vor dieser Einstellung.",
                "The environment variable PIXELART_HOME is set. At the next start it takes precedence over this setting.",
            ),
        );
    }

    section(ui, tr("Sprache", "Language"));
    ui.horizontal(|ui| {
        for lang in Lang::ALL {
            ui.radio_value(&mut f.lang, lang, lang.name());
        }
    });
    ui.weak(tr(
        "Gilt sofort für das ganze Programm. Claude bekommt seine Werkzeuge immer auf Englisch.",
        "Applies right away to the whole program. Claude always gets its tools in English.",
    ));

    let (ok, cancel) = buttons(ui, tr("Übernehmen", "Apply"), !f.home.trim().is_empty());
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let result = app.apply_settings(PathBuf::from(f.home.trim()), f.lang);
    finish(app, result, |app, ()| {
        app.info(tr("Einstellungen gespeichert", "Settings saved"));
    })
}

// ---------------------------------------------------------------- Stile

fn styles(app: &mut App, ui: &mut egui::Ui, f: &mut SettingsForm) {
    if f.draft.is_some() {
        edit_style(app, ui, f);
        return;
    }
    ui.label(tr(
        "Ein Stil legt fest, wie Bilder aussehen: Palette, Größen, Umriss, Schattierung und Regeln. Sag Claude einfach den Namen, etwa „im Stil Manga“, dann hält es sich genau daran.",
        "A style defines how pictures look: palette, sizes, outline, shading and rules. Just tell Claude the name, e.g. \"in manga style\", and it follows it exactly.",
    ));
    ui.add_space(4.0);
    let project_style = app
        .doc
        .as_ref()
        .and_then(|d| d.project().style.preset.clone());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(170.0);
            egui::ScrollArea::vertical()
                .id_salt("style-list")
                .max_height(330.0)
                .show(ui, |ui| {
                    let mut heading_own = false;
                    ui.weak(tr("Fest eingebaut", "Built in"));
                    for (i, s) in f.styles.iter().enumerate() {
                        if !s.builtin && !heading_own {
                            heading_own = true;
                            ui.add_space(6.0);
                            ui.weak(tr("Eigene", "Own"));
                        }
                        let mut text = s.name.clone();
                        if project_style.as_deref().is_some_and(|p| s.matches(p)) {
                            text.push_str(tr("  · Projekt", "  · project"));
                        }
                        if ui.selectable_label(f.selected == i, text).clicked() {
                            f.selected = i;
                        }
                    }
                    if !heading_own {
                        ui.add_space(6.0);
                        ui.weak(tr("Noch keine eigenen Stile", "No own styles yet"));
                    }
                });
            ui.add_space(6.0);
            if ui.button(tr("+ Neuer Stil", "+ New style")).clicked() {
                let mut st = StylePreset::new(tr("Mein Stil", "My style"));
                if let Some(d) = &app.doc {
                    if !d.project().style.palette.is_empty() {
                        st = StylePreset::from_project(st.name.clone(), &d.project().style);
                    }
                }
                f.draft = Some(StyleDraft::new(st, None));
            }
        });
        ui.add_space(14.0);
        ui.vertical(|ui| {
            let Some(st) = f.styles.get(f.selected).cloned() else {
                return;
            };
            egui::ScrollArea::vertical()
                .id_salt("style-details")
                .max_height(360.0)
                .show(ui, |ui| style_details(ui, &st));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let apply = ui
                    .add_enabled(
                        app.doc.is_some(),
                        egui::Button::new(tr("Für das Projekt verwenden", "Use for the project")),
                    )
                    .on_hover_text(tr(
                        "Neue Bilder und Figuren starten mit der Palette des Stils; Licht und Umrissfarbe gelten für das ganze Projekt. Vorhandene Bilder bleiben, wie sie sind.",
                        "New images and characters start with the style's palette; light and outline color apply to the whole project. Existing pictures stay as they are.",
                    ));
                if apply.clicked() {
                    apply_to_project(app, &st);
                }
                if ui
                    .button(tr("Als eigenen Stil kopieren", "Copy as own style"))
                    .clicked()
                {
                    let mut copy = st.clone();
                    copy.name = trf!("{} (eigen)", "{} (own)", st.name);
                    copy.builtin = false;
                    copy.id = "";
                    f.draft = Some(StyleDraft::new(copy, None));
                }
                if !st.builtin {
                    if ui.button(tr("Bearbeiten", "Edit")).clicked() {
                        f.draft = Some(StyleDraft::new(st.clone(), Some(st.name.clone())));
                    }
                    if ui.button(tr("Löschen", "Delete")).clicked() {
                        match app.ws.delete_style(&st.name) {
                            Ok(_) => {
                                app.info(trf!(
                                    "Stil „{}“ gelöscht",
                                    "Style '{}' deleted",
                                    st.name
                                ));
                                f.reload(app, "");
                            }
                            Err(err) => app.dialog_error = Some(err.to_string()),
                        }
                    }
                }
            });
        });
    });
}

fn apply_to_project(app: &mut App, st: &StylePreset) {
    let Some(doc) = app.doc.as_mut() else {
        return;
    };
    let mut style = doc.project().style.clone();
    st.apply_to(&mut style);
    match doc.set_style(style) {
        Ok(()) => app.info(trf!(
            "Das Projekt hat jetzt den Stil „{}“",
            "The project now has the style '{}'",
            st.name
        )),
        Err(err) => app.dialog_error = Some(err),
    }
}

fn yes_no(b: bool) -> &'static str {
    if b {
        tr("ja", "yes")
    } else {
        tr("nein", "no")
    }
}

fn light_label(dir: &str) -> &'static str {
    match dir {
        "top-left" => tr("von oben links", "from the top left"),
        "top" => tr("von oben", "from the top"),
        "top-right" => tr("von oben rechts", "from the top right"),
        "left" => tr("von links", "from the left"),
        "right" => tr("von rechts", "from the right"),
        "bottom-left" => tr("von unten links", "from the bottom left"),
        "bottom" => tr("von unten", "from the bottom"),
        _ => tr("von unten rechts", "from the bottom right"),
    }
}

fn style_details(ui: &mut egui::Ui, st: &StylePreset) {
    ui.label(RichText::new(&st.name).heading());
    if !st.description.is_empty() {
        ui.label(&st.description);
    }
    ui.add_space(4.0);
    swatches(ui, &st.palette);
    ui.add_space(4.0);
    egui::Grid::new("style-facts")
        .num_columns(2)
        .spacing([16.0, 3.0])
        .show(ui, |ui| {
            let mut row = |k: &str, v: String| {
                ui.weak(k);
                ui.label(v);
                ui.end_row();
            };
            row(
                tr("Farben", "Colors"),
                trf!(
                    "{} in der Palette, {} Bit",
                    "{} in the palette, {} bit",
                    st.palette.len(),
                    st.color_depth
                ),
            );
            if let Some(n) = st.colors_per_sprite {
                row(
                    tr("Pro Figur", "Per sprite"),
                    trf!("höchstens {n} Farben", "at most {n} colors"),
                );
            }
            if !st.sizes.is_empty() {
                row(
                    tr("Größen", "Sizes"),
                    st.sizes
                        .iter()
                        .map(|[w, h]| format!("{w}×{h}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            let outline = if st.outline == OutlineRule::None {
                st.outline.label().to_string()
            } else {
                format!("{}, {} px", st.outline.label(), st.outline_width)
            };
            row(tr("Umriss", "Outline"), outline);
            row(
                tr("Schattierung", "Shading"),
                trf!("{} Stufen je Farbe", "{} shades per color", st.shades),
            );
            row(tr("Dithering", "Dithering"), yes_no(st.dithering).into());
            row(
                tr("Farbton-Verschiebung", "Hue shifting"),
                yes_no(st.hue_shift).into(),
            );
            row(
                tr("Kantenglättung", "Anti-aliasing"),
                yes_no(st.anti_alias).into(),
            );
            if let Some(dir) = &st.light_dir {
                row(tr("Licht", "Light"), light_label(dir).into());
            }
        });
    if !st.rules.is_empty() {
        section(ui, tr("Regeln", "Rules"));
        for r in &st.rules {
            ui.label(format!("• {r}"));
        }
    }
}

/// Farbfelder einer Palette, etwas größer als in der Seitenleiste.
fn swatches(ui: &mut egui::Ui, colors: &[pixel_core::Rgba]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
        for &c in colors.iter().take(256) {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, c32(c));
            response.on_hover_text(c.to_string());
        }
    });
}

fn edit_style(app: &mut App, ui: &mut egui::Ui, f: &mut SettingsForm) {
    let Some(d) = f.draft.as_mut() else {
        return;
    };
    let mut save = false;
    let mut cancel = false;
    egui::ScrollArea::vertical()
        .id_salt("style-edit")
        .max_height(400.0)
        .show(ui, |ui| {
            let st = &mut d.style;
            egui::Grid::new("style-edit-grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tr("Name", "Name"));
                    ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(260.0));
                    ui.end_row();
                    ui.label(tr("Beschreibung", "Description"));
                    ui.add(
                        egui::TextEdit::multiline(&mut st.description)
                            .desired_rows(2)
                            .desired_width(420.0),
                    );
                    ui.end_row();
                    ui.label(tr("Palette", "Palette"));
                    ui.vertical(|ui| {
                        swatches(ui, &st.palette);
                        ui.horizontal_wrapped(|ui| {
                            let project = app
                                .doc
                                .as_ref()
                                .map(|d| d.project().style.palette.clone())
                                .filter(|p| !p.is_empty());
                            if ui
                                .add_enabled(
                                    project.is_some(),
                                    egui::Button::new(tr("Vom Projekt", "From the project")),
                                )
                                .clicked()
                            {
                                st.palette = project.unwrap_or_default();
                            }
                            let sprite = app
                                .sprite()
                                .map(|s| s.palette.colors.clone())
                                .filter(|p| !p.is_empty());
                            if ui
                                .add_enabled(
                                    sprite.is_some(),
                                    egui::Button::new(tr(
                                        "Vom gewählten Bild",
                                        "From the selected picture",
                                    )),
                                )
                                .clicked()
                            {
                                st.palette = sprite.unwrap_or_default();
                            }
                            if ui.button(tr("32 Farbtreppen", "32 ramps")).clicked() {
                                st.palette = pixel_core::Palette::ramps_32().colors;
                            }
                            if ui.button(tr("64 Farbtreppen", "64 ramps")).clicked() {
                                st.palette = pixel_core::Palette::ramps_64().colors;
                            }
                        });
                    });
                    ui.end_row();
                    ui.label(tr("Farbtiefe", "Color depth"));
                    ui.horizontal(|ui| {
                        for bits in [8u8, 16, 32] {
                            ui.radio_value(&mut st.color_depth, bits, format!("{bits} Bit"));
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Größen", "Sizes"));
                    ui.add(
                        egui::TextEdit::singleline(&mut d.sizes)
                            .hint_text("32x32, 64x64")
                            .desired_width(260.0),
                    );
                    ui.end_row();
                    ui.label(tr("Farben pro Figur", "Colors per sprite"));
                    let mut n = st.colors_per_sprite.unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut n).range(0..=256));
                        ui.weak(tr("0 = keine Grenze", "0 = no limit"));
                    });
                    st.colors_per_sprite = (n > 0).then_some(n);
                    ui.end_row();
                    ui.label(tr("Umriss", "Outline"));
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("style-outline")
                            .selected_text(st.outline.label())
                            .width(250.0)
                            .show_ui(ui, |ui| {
                                for o in OutlineRule::ALL {
                                    ui.selectable_value(&mut st.outline, o, o.label());
                                }
                            });
                        ui.add(
                            egui::DragValue::new(&mut st.outline_width)
                                .range(1..=4)
                                .suffix(" px"),
                        );
                    });
                    ui.end_row();
                    ui.label(tr("Stufen je Farbe", "Shades per color"));
                    ui.add(egui::DragValue::new(&mut st.shades).range(1..=8));
                    ui.end_row();
                    ui.label("");
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut st.dithering, tr("Dithering", "Dithering"));
                        ui.checkbox(
                            &mut st.hue_shift,
                            tr("Farbton-Verschiebung", "Hue shifting"),
                        );
                        ui.checkbox(&mut st.anti_alias, tr("Kantenglättung", "Anti-aliasing"));
                    });
                    ui.end_row();
                    ui.label(tr("Licht", "Light"));
                    let current = st
                        .light_dir
                        .as_deref()
                        .map(light_label)
                        .unwrap_or(tr("keine Vorgabe", "not set"));
                    egui::ComboBox::from_id_salt("style-light")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut st.light_dir, None, tr("keine Vorgabe", "not set"));
                            for dir in LIGHT_DIRS {
                                ui.selectable_value(
                                    &mut st.light_dir,
                                    Some(dir.to_string()),
                                    light_label(dir),
                                );
                            }
                        });
                    ui.end_row();
                    ui.label(tr("Regeln", "Rules"));
                    ui.vertical(|ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut d.rules)
                                .desired_rows(5)
                                .desired_width(420.0),
                        );
                        ui.weak(tr(
                            "Eine Regel pro Zeile, zum Beispiel „Umriss 1 Pixel, kein reines Schwarz“. Claude hält sich daran.",
                            "One rule per line, e.g. \"1 pixel outline, no pure black\". Claude follows them.",
                        ));
                    });
                    ui.end_row();
                });
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        save = ui.button(tr("Stil speichern", "Save style")).clicked();
        cancel = ui.button(tr("Abbrechen", "Cancel")).clicked();
    });
    if cancel {
        f.draft = None;
        app.dialog_error = None;
        return;
    }
    if !save {
        return;
    }
    let mut st = d.style.clone();
    st.name = st.name.trim().to_string();
    st.rules = d
        .rules
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let mut sizes = Vec::new();
    for part in d
        .sizes
        .split([',', ';'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let parsed = part
            .split(['x', 'X', '×'])
            .map(|n| n.trim().parse::<u32>())
            .collect::<Vec<_>>();
        match parsed.as_slice() {
            [Ok(w), Ok(h)] => sizes.push([*w, *h]),
            _ => {
                app.dialog_error = Some(trf!(
                    "Größe „{part}“ nicht verstanden; bitte wie 32x32 schreiben",
                    "Size '{part}' not understood; please write it like 32x32"
                ));
                return;
            }
        }
    }
    st.sizes = sizes;
    let renamed = d
        .old_name
        .as_ref()
        .filter(|old| pixel_core::stylebook::key(old) != pixel_core::stylebook::key(&st.name))
        .cloned();
    match app.ws.save_style(&st) {
        Ok(_) => {
            if let Some(old) = renamed {
                let _ = app.ws.delete_style(&old);
            }
            app.dialog_error = None;
            app.info(trf!("Stil „{}“ gespeichert", "Style '{}' saved", st.name));
            f.draft = None;
            f.reload(app, &st.name);
        }
        Err(err) => app.dialog_error = Some(err.to_string()),
    }
}

// ---------------------------------------------------------------- Tipps

fn tips(ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_salt("tips")
        .max_height(400.0)
        .show(ui, |ui| {
            let list = |ui: &mut egui::Ui, items: &[(&'static str, &'static str)]| {
                for (de, en) in items {
                    ui.label(format!("• {}", tr(de, en)));
                }
            };
            section(ui, tr("Zeichnen", "Drawing"));
            list(ui, &[
                ("Rechte Maustaste nimmt eine Farbe auf, Leertaste + Ziehen verschiebt den Ausschnitt.", "The right mouse button picks a color, space + drag pans the view."),
                ("„Pixel-perfekt“ beim Stift verhindert doppelte Pixel in Ecken, für saubere 1-Pixel-Linien.", "\"Pixel-perfect\" on the pen avoids doubled pixels in corners, for clean 1-pixel lines."),
                ("Aufhellen und Abdunkeln gehen entlang der Farbtreppen deiner Palette, so bleibt die Schattierung stimmig.", "Lighten and darken walk along your palette's ramps, so the shading stays consistent."),
                ("Kontur… und Licht… zeichnen Umriss, Licht und Schatten auf eigenen Ebenen und folgen jeder Änderung.", "Outline… and Light… draw outline, light and shadow on their own layers and follow every change."),
                ("Effekte… legt Leuchten, Blitz, Funken, Rauch, Spiegelung oder Farbzyklen als Schleife über die Frames; aus einem Standbild wird dabei eine Animation.", "Effects… adds glow, lightning, sparks, smoke, reflections or color cycling as a loop over the frames; a still image becomes an animation."),
                ("Spiegeln beim Zeichnen spart bei Figuren und Symbolen die halbe Arbeit.", "Mirror drawing saves half the work on characters and icons."),
                ("Zwiebelhaut zeigt den vorigen Frame durchscheinend, das hilft bei flüssigen Animationen.", "Onion skin shows the previous frame faintly, which helps with smooth animations."),
                ("Ein Foto als Vorlage importieren und selbst nachzeichnen ergibt oft schönere Pixel-Art als das automatische Umwandeln.", "Importing a photo as a reference and tracing it yourself often gives nicer pixel art than converting it automatically."),
            ]);
            section(ui, tr("Mit Claude", "With Claude"));
            ui.label(tr(
                "Claude arbeitet in denselben Projekten. Was Claude zeichnet, siehst du nach kurzer Zeit hier, und Rückgängig nimmt es auch wieder zurück. Beispiele, die du Claude schreiben kannst:",
                "Claude works in the same projects. What Claude draws shows up here shortly, and undo takes it back too. Examples you can write to Claude:",
            ));
            list(ui, &[
                ("„Leg im Projekt Ritterspiel eine Figur ‚Magierin‘ im Stil Manga an, 64×96 Pixel.“", "\"Create a character 'Sorceress' in manga style in the project Ritterspiel, 64×96 pixels.\""),
                ("„Gib dem ganzen Projekt den Stil 16 Bit.“", "\"Give the whole project the 16 bit style.\""),
                ("„Zeichne eine Burgmauer als Kachel im Stil Burgen, nahtlos kachelbar.“", "\"Draw a castle wall tile in the castles style, seamlessly tileable.\""),
                ("„Speichere den Stil dieses Projekts als eigenen Stil ‚Sumpf‘.“", "\"Save this project's style as an own style 'Swamp'.\""),
                ("„Lass an der Laufanimation eine Fackel von links nach rechts vorbeiziehen, mit Schatten am Boden.“", "\"Let a torch pass the walk animation from left to right, with a ground shadow.\""),
                ("„Mach aus diesem Foto Pixel-Art mit 64 Pixeln und der Palette des Projekts.“", "\"Turn this photo into 64 pixel pixel art with the project's palette.\""),
                ("„Lass das Schwert des Ritters bläulich leuchten und Funken sprühen, als Schleife mit 8 Frames.“", "\"Make the knight's sword glow blue and throw sparks, as a loop with 8 frames.\""),
                ("„Mal unter das Bild einen See und lass das Wasser mit einem Farbzyklus fließen.“", "\"Paint a lake below the picture and let the water flow with a color cycle.\""),
            ]);
            section(ui, tr("Gut zu wissen", "Good to know"));
            list(ui, &[
                ("Gespeichert wird automatisch nach jeder Änderung.", "Everything is saved automatically after each change."),
                ("Eigene Formen liegen im Speicherordner unter „shapes“, eigene Stile unter „styles“. Beide gelten in allen Projekten.", "Own shapes live in the save folder under \"shapes\", own styles under \"styles\". Both apply to all projects."),
                ("Eine Figur hat eine feste Palette; ihre Animationen bleiben dadurch immer in denselben Farben.", "A character has a locked palette, so its animations always keep the same colors."),
            ]);
        });
}
