//! Dialoge: Neues Projekt, neue Figur, neues Bild, neue Animation, Export,
//! Umbenennen, Löschen und das Menü „Hilfe“.

use crate::app::{section, App, MAX_FRAMES};
use crate::doc::{AssetId, AssetKind, Res};
use crate::help::{self, ConnectInfo};
use crate::import_dialog::{self, ImportForm};
use crate::outline_dialog::OutlineForm;
use crate::tools::Tool;
use eframe::egui::{self, RichText};
use pixel_core::export::MAX_SCALE;
use pixel_core::lang::{tr, Lang};
use pixel_core::trf;
use pixel_core::workspace::ExportFormat;
use pixel_core::{AnimationKind, ColorDepth, Palette, Rgba, MAX_SIZE, TEMPLATES};
use std::path::PathBuf;
use uuid::Uuid;

pub enum Dialog {
    None,
    NewProject { name: String },
    NewSprite(NewSprite),
    NewAnimation(NewAnimation),
    Rename { id: AssetId, name: String },
    RenameLayer { layer: usize, name: String },
    Delete(AssetId),
    Export(ExportForm),
    Import(Box<ImportForm>),
    Connect,
    Shortcuts,
    About,
    Settings(SettingsForm),
    Outline(Box<OutlineForm>),
    DeleteProject { name: String, exports: bool },
    ReplaceColor(ReplaceForm),
    Resize(ResizeForm),
    SaveShape { name: String },
}

/// Formular für Datei → Einstellungen.
pub struct SettingsForm {
    pub home: String,
    pub lang: Lang,
}

impl SettingsForm {
    pub fn new(app: &App) -> Self {
        Self {
            home: app.ws.home().display().to_string(),
            lang: pixel_core::lang::get(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PalettePreset {
    Ramps32,
    Ramps64,
    Default16,
    Gray4,
    Gray8,
    Gray16,
    /// Die Palette aus dem Stil des Projekts.
    Style,
    /// Farben aus einer Bilddatei.
    FromImage,
}

impl PalettePreset {
    const FIXED: [Self; 6] = [
        Self::Ramps32,
        Self::Ramps64,
        Self::Default16,
        Self::Gray4,
        Self::Gray8,
        Self::Gray16,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Ramps32 => tr("Farbtreppen (32 Farben)", "Color ramps (32 colors)"),
            Self::Ramps64 => tr("Farbtreppen (64 Farben)", "Color ramps (64 colors)"),
            Self::Default16 => tr("Standard (16 Farben)", "Default (16 colors)"),
            Self::Gray4 => tr("Graustufen (4)", "Grayscale (4)"),
            Self::Gray8 => tr("Graustufen (8)", "Grayscale (8)"),
            Self::Gray16 => tr("Graustufen (16)", "Grayscale (16)"),
            Self::Style => tr("Palette des Projekts", "Project palette"),
            Self::FromImage => tr("Aus einem Bild…", "From an image…"),
        }
    }

    fn fixed(self) -> Palette {
        match self {
            Self::Ramps64 => Palette::ramps_64(),
            Self::Default16 => Palette::default_16(),
            Self::Gray4 => Palette::grayscale(4),
            Self::Gray8 => Palette::grayscale(8),
            Self::Gray16 => Palette::grayscale(16),
            _ => Palette::ramps_32(),
        }
    }
}

/// Formular für eine neue Figur oder ein neues Bild.
pub struct NewSprite {
    pub character: bool,
    pub name: String,
    /// Index in [`TEMPLATES`]; `None` heißt eigene Größe.
    pub template: Option<usize>,
    pub width: u32,
    pub height: u32,
    pub bits: u8,
    pub palette: PalettePreset,
    /// Die Farben aus einem Bild und der Dateiname, wenn „Aus einem Bild…“ gewählt ist.
    pub image_colors: Option<(String, Vec<Rgba>)>,
    /// Wie viele Farben aus einem Bild höchstens übernommen werden.
    pub image_max: usize,
}

impl NewSprite {
    pub fn new(character: bool) -> Self {
        // Für Figuren die kleine Figuren-Vorlage, sonst die erste Kachel.
        let wanted = if character { "char-" } else { "tile-" };
        let template = TEMPLATES
            .iter()
            .position(|t| t.id.starts_with(wanted))
            .or(Some(0));
        let (width, height) = template
            .and_then(|i| TEMPLATES.get(i))
            .map_or((32, 32), |t| (t.width, t.height));
        Self {
            character,
            name: String::new(),
            template,
            width,
            height,
            bits: 32,
            palette: PalettePreset::Ramps32,
            image_colors: None,
            image_max: 32,
        }
    }
}

pub struct NewAnimation {
    pub character: Uuid,
    /// Index in [`AnimationKind::PRESETS`]; `None` heißt eigene Art.
    pub kind: Option<usize>,
    pub custom: String,
    pub frames: usize,
    pub name: String,
}

impl NewAnimation {
    pub fn new(character: Uuid) -> Self {
        Self {
            character,
            kind: Some(0),
            custom: String::new(),
            frames: AnimationKind::PRESETS[0].default_frames(),
            name: String::new(),
        }
    }

    fn kind(&self) -> AnimationKind {
        match self.kind.and_then(|i| AnimationKind::PRESETS.get(i)) {
            Some(k) => k.clone(),
            None => AnimationKind::Custom(self.custom.trim().to_string()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Png,
    PngIndexed,
    Jpg,
    Sheet,
    Gif,
    WebM,
}

impl ExportKind {
    /// Formate, die jeden Frame als eigene Datei schreiben.
    fn is_single_frames(self) -> bool {
        matches!(self, Self::Png | Self::PngIndexed | Self::Jpg)
    }
}

pub struct ExportForm {
    pub kind: ExportKind,
    pub scale: u32,
    pub all_frames: bool,
    pub columns: u32,
    pub background: [u8; 3],
    pub quality: u8,
    /// Ergebnis des letzten Exports.
    pub written: Vec<PathBuf>,
    /// Hinweis zum letzten Export, etwa zu reduzierten Farben.
    pub note: Option<String>,
    /// Ob ffmpeg für WebM da ist. Wird erst gesucht, wenn WebM gewählt ist.
    pub ffmpeg: Option<bool>,
}

impl Default for ExportForm {
    fn default() -> Self {
        Self {
            kind: ExportKind::Png,
            scale: 1,
            all_frames: true,
            columns: 0,
            background: [255, 255, 255],
            quality: 95,
            written: Vec::new(),
            note: None,
            ffmpeg: None,
        }
    }
}

/// Was nach dem Zeichnen eines Dialogs geschehen soll.
pub(crate) enum Outcome {
    Keep,
    Close,
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let mut dialog = std::mem::replace(&mut app.dialog, Dialog::None);
    if matches!(dialog, Dialog::None) {
        return;
    }
    let modal = egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
        let outcome = match &mut dialog {
            Dialog::None => Outcome::Close,
            Dialog::NewProject { name } => new_project(app, ui, name),
            Dialog::NewSprite(form) => new_sprite(app, ui, form),
            Dialog::NewAnimation(form) => new_animation(app, ui, form),
            Dialog::Rename { id, name } => rename(app, ui, *id, name),
            Dialog::RenameLayer { layer, name } => rename_layer(app, ui, *layer, name),
            Dialog::Delete(id) => delete(app, ui, *id),
            Dialog::Export(form) => export(app, ui, form),
            Dialog::Import(form) => import_dialog::show(app, ui, form),
            Dialog::Connect => connect(app, ui),
            Dialog::Shortcuts => shortcuts(ui),
            Dialog::About => about(app, ui),
            Dialog::Settings(form) => settings(app, ui, form),
            Dialog::Outline(form) => crate::outline_dialog::show(app, ui, form),
            Dialog::DeleteProject { name, exports } => delete_project(app, ui, name, exports),
            Dialog::ReplaceColor(form) => replace_color(app, ui, form),
            Dialog::Resize(form) => resize(app, ui, form),
            Dialog::SaveShape { name } => save_shape(app, ui, name),
        };
        if let Some(err) = &app.dialog_error {
            ui.add_space(4.0);
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
        outcome
    });
    // Nur Knöpfe und Esc schließen. Ein Klick neben den Dialog soll keine Eingaben verwerfen.
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !modal.any_popup_open;
    let close = matches!(modal.inner, Outcome::Close) || escape;
    // Ein Dialog kann einen anderen geöffnet haben; der hat dann Vorrang.
    if matches!(app.dialog, Dialog::None) && !close {
        app.dialog = dialog;
    } else if close {
        app.dialog_error = None;
    }
}

/// Zeile mit „Abbrechen“ und einer Bestätigung. Gibt zurück, was gedrückt wurde.
pub(crate) fn buttons(ui: &mut egui::Ui, confirm: &str, enabled: bool) -> (bool, bool) {
    ui.add_space(10.0);
    let mut ok = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ok = ui
            .add_enabled(enabled, egui::Button::new(RichText::new(confirm).strong()))
            .clicked();
        cancel = ui.button(tr("Abbrechen", "Cancel")).clicked();
    });
    // Die Eingabetaste bestätigt, wie man es von Dialogen gewohnt ist.
    if enabled && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ok = true;
    }
    (ok, cancel)
}

/// Schließt den Dialog bei Erfolg, sonst bleibt er mit der Fehlermeldung offen.
pub(crate) fn finish<T>(app: &mut App, result: Res<T>, done: impl FnOnce(&mut App, T)) -> Outcome {
    match result {
        Ok(value) => {
            done(app, value);
            Outcome::Close
        }
        Err(err) => {
            app.dialog_error = Some(err);
            Outcome::Keep
        }
    }
}

fn name_field(ui: &mut egui::Ui, name: &mut String) {
    let resp = ui.add(
        egui::TextEdit::singleline(name)
            .hint_text("Name")
            .desired_width(260.0),
    );
    // Beim Öffnen gleich lostippen können.
    if name.is_empty() && !resp.has_focus() && ui.memory(|m| m.focused().is_none()) {
        resp.request_focus();
    }
}

fn new_project(app: &mut App, ui: &mut egui::Ui, name: &mut String) -> Outcome {
    ui.heading(tr("Neues Projekt", "New project"));
    ui.label(tr(
        "Ein Projekt sammelt Figuren, Animationen und Bilder in einer Datei.",
        "A project keeps characters, animations and images together in one file.",
    ));
    ui.add_space(6.0);
    name_field(ui, name);
    ui.weak(tr(
        "Buchstaben, Ziffern, Leerzeichen, - und _",
        "Letters, digits, spaces, - and _",
    ));
    let (ok, cancel) = buttons(ui, tr("Anlegen", "Create"), !name.trim().is_empty());
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let created = app.ws.create(name).map_err(|e| e.to_string());
    finish(app, created, |app, _| {
        app.projects = app.ws.project_names();
        app.open_project(name.trim());
    })
}

fn new_sprite(app: &mut App, ui: &mut egui::Ui, f: &mut NewSprite) -> Outcome {
    ui.set_width(380.0);
    if f.character {
        ui.heading(tr("Neue Figur", "New character"));
        ui.label(tr("Größe, Farbtiefe und Palette einer Figur stehen danach fest. Alle Animationen übernehmen sie, deshalb sieht die Figur überall gleich aus. Farben lassen sich später ergänzen.", "Size, color depth and palette of a character are fixed afterwards. All animations take them over, so the character looks the same everywhere. Colors can be added later."));
    } else {
        ui.heading(tr("Neues Bild", "New image"));
        ui.label(tr(
            "Ein freies Bild, zum Beispiel ein Tile, ein Item oder eine Szene.",
            "A free image, for example a tile, an item or a scene.",
        ));
    }
    ui.add_space(6.0);
    name_field(ui, &mut f.name);

    section(ui, tr("Größe", "Size"));
    let selected = f
        .template
        .and_then(|i| TEMPLATES.get(i))
        .map_or(tr("Eigene Größe", "Custom size"), |t| t.name);
    egui::ComboBox::from_id_salt("template")
        .selected_text(selected)
        .width(260.0)
        .show_ui(ui, |ui| {
            for (i, t) in TEMPLATES.iter().enumerate() {
                let label = format!("{} · {}", t.name, t.purpose);
                if ui.selectable_label(f.template == Some(i), label).clicked() {
                    f.template = Some(i);
                    f.width = t.width;
                    f.height = t.height;
                }
            }
            if ui
                .selectable_label(f.template.is_none(), tr("Eigene Größe", "Custom size"))
                .clicked()
            {
                f.template = None;
            }
        });
    ui.horizontal(|ui| {
        let custom = f.template.is_none();
        ui.add_enabled(
            custom,
            egui::DragValue::new(&mut f.width)
                .range(1..=MAX_SIZE)
                .prefix(tr("Breite ", "Width ")),
        );
        ui.add_enabled(
            custom,
            egui::DragValue::new(&mut f.height)
                .range(1..=MAX_SIZE)
                .prefix(tr("Höhe ", "Height ")),
        );
        ui.weak(trf!(
            "Pixel, höchstens {MAX_SIZE}",
            "pixels, at most {MAX_SIZE}"
        ));
    });

    section(ui, tr("Farbtiefe", "Color depth"));
    ui.horizontal(|ui| {
        ui.radio_value(&mut f.bits, 8, "8 Bit").on_hover_text(tr(
            "Nur Farben aus der Palette, höchstens 256",
            "Only palette colors, at most 256",
        ));
        ui.radio_value(&mut f.bits, 16, "16 Bit").on_hover_text(tr(
            "65 536 Farben, Pixel sind deckend oder durchsichtig",
            "65,536 colors, pixels are opaque or transparent",
        ));
        ui.radio_value(&mut f.bits, 32, "32 Bit").on_hover_text(tr(
            "Alle Farben mit Transparenz",
            "All colors with transparency",
        ));
    });

    section(ui, "Palette");
    let style: Vec<Rgba> = app
        .doc
        .as_ref()
        .map(|d| d.project().style.palette.clone())
        .unwrap_or_default();
    egui::ComboBox::from_id_salt("palette")
        .selected_text(f.palette.label())
        .width(260.0)
        .show_ui(ui, |ui| {
            for p in PalettePreset::FIXED {
                ui.selectable_value(&mut f.palette, p, p.label());
            }
            if !style.is_empty() {
                ui.selectable_value(
                    &mut f.palette,
                    PalettePreset::Style,
                    PalettePreset::Style.label(),
                );
            }
            ui.selectable_value(
                &mut f.palette,
                PalettePreset::FromImage,
                PalettePreset::FromImage.label(),
            );
        });
    if f.palette == PalettePreset::FromImage {
        ui.horizontal(|ui| {
            if ui.button(tr("Bild wählen…", "Choose image…")).clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter(tr("Bilder", "Images"), &["png", "jpg", "jpeg", "gif"])
                    .pick_file();
                if let Some(path) = picked {
                    match pixel_core::import::read(&path) {
                        Ok(picture) => {
                            let pixels = picture
                                .frames
                                .iter()
                                .flat_map(|fr| fr.pixels.iter().copied());
                            let (colors, _) = pixel_core::quant::palette_of(pixels, f.image_max);
                            let file = path
                                .file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                            f.image_colors = Some((file, colors));
                            app.dialog_error = None;
                        }
                        Err(err) => app.dialog_error = Some(err.to_string()),
                    }
                }
            }
            ui.label(tr("höchstens", "at most"));
            egui::ComboBox::from_id_salt("image_max")
                .selected_text(trf!("{} Farben", "{} colors", f.image_max))
                .show_ui(ui, |ui| {
                    for n in [16, 32, 64, 128, 256] {
                        ui.selectable_value(&mut f.image_max, n, trf!("{n} Farben", "{n} colors"));
                    }
                });
            if let Some((file, _)) = &f.image_colors {
                ui.weak(file);
            }
        });
    }
    let palette = match f.palette {
        PalettePreset::Style => Some(Palette::new(tr("Projekt", "Project"), style.clone())),
        PalettePreset::FromImage => f
            .image_colors
            .as_ref()
            .map(|(name, colors)| Palette::new(name.clone(), colors.clone())),
        other => Some(other.fixed()),
    };
    if let Some(p) = &palette {
        palette_strip(ui, &p.colors);
    }
    ui.weak(tr(
        "Die Farbtiefe bestimmt, welche Farben überhaupt möglich sind (32 Bit: alle). Die Palette ist deine Farbauswahl rechts zum Zeichnen. Bei 16 und 32 Bit kannst du trotzdem jede Farbe benutzen und mit „+ aktuelle Farbe“ aufnehmen, bis 256. Bei einer Figur ist die Palette fest, damit jede Animation gleich aussieht, lässt sich aber ergänzen.",
        "The color depth sets which colors are possible at all (32 bit: all). The palette is your color selection on the right for drawing. At 16 and 32 bit you can still use any color and add it with \"+ current color\", up to 256. A character's palette is fixed so every animation looks the same, but it can be extended.",
    ));

    let ready = !f.name.trim().is_empty() && palette.as_ref().is_some_and(|p| !p.is_empty());
    let (ok, cancel) = buttons(ui, tr("Anlegen", "Create"), ready);
    if cancel {
        return Outcome::Close;
    }
    let (true, Some(palette)) = (ok, palette) else {
        return Outcome::Keep;
    };
    let depth = ColorDepth::from_bits(f.bits).unwrap_or(ColorDepth::Rgba32);
    let Some(doc) = &mut app.doc else {
        return Outcome::Close;
    };
    let created = if f.character {
        doc.create_character(&f.name, f.width, f.height, depth, palette)
    } else {
        doc.create_image(&f.name, f.width, f.height, depth, palette)
    };
    finish(app, created, |app, id| {
        app.select(id);
        app.info(trf!("„{}“ angelegt", "\"{}\" created", f.name.trim()));
    })
}

/// Deutscher Name einer Animationsart, mit dem Kürzel, das Claude benutzt.
pub(crate) fn kind_label(k: &AnimationKind) -> &'static str {
    match k {
        AnimationKind::Idle => tr("Stehen (idle)", "Idle"),
        AnimationKind::Walk => tr("Gehen (walk)", "Walk"),
        AnimationKind::Run => tr("Rennen (run)", "Run"),
        AnimationKind::Jump => tr("Springen (jump)", "Jump"),
        AnimationKind::Attack => tr("Angriff (attack)", "Attack"),
        AnimationKind::Hurt => tr("Treffer (hurt)", "Hurt"),
        AnimationKind::Death => tr("Sterben (death)", "Death"),
        AnimationKind::Custom(_) => tr("Eigene", "Custom"),
    }
}

fn new_animation(app: &mut App, ui: &mut egui::Ui, f: &mut NewAnimation) -> Outcome {
    ui.set_width(380.0);
    ui.heading(tr("Neue Animation", "New animation"));
    ui.label(tr("Jeder Frame startet als Kopie der Grundpose der Figur. Du änderst dann nur, was sich bewegt.", "Every frame starts as a copy of the character's base pose. Then you only change what moves."));
    let characters: Vec<(Uuid, String)> = app
        .doc
        .as_ref()
        .map(|d| {
            d.project()
                .characters
                .iter()
                .map(|c| (c.id, c.name.clone()))
                .collect()
        })
        .unwrap_or_default();

    section(ui, tr("Figur", "Character"));
    let current = characters
        .iter()
        .find(|(id, _)| *id == f.character)
        .map_or("", |(_, n)| n.as_str());
    egui::ComboBox::from_id_salt("character")
        .selected_text(current)
        .width(260.0)
        .show_ui(ui, |ui| {
            for (id, name) in &characters {
                ui.selectable_value(&mut f.character, *id, name);
            }
        });

    section(ui, tr("Art", "Kind"));
    let label = kind_label;
    let selected = f
        .kind
        .and_then(|i| AnimationKind::PRESETS.get(i))
        .map_or(tr("Eigene", "Custom"), label);
    egui::ComboBox::from_id_salt("kind")
        .selected_text(selected)
        .width(260.0)
        .show_ui(ui, |ui| {
            for (i, k) in AnimationKind::PRESETS.iter().enumerate() {
                if ui.selectable_label(f.kind == Some(i), label(k)).clicked() {
                    f.kind = Some(i);
                    f.frames = k.default_frames();
                }
            }
            if ui
                .selectable_label(f.kind.is_none(), tr("Eigene", "Custom"))
                .clicked()
            {
                f.kind = None;
            }
        });
    if f.kind.is_none() {
        ui.add(
            egui::TextEdit::singleline(&mut f.custom)
                .hint_text(tr(
                    "Bezeichnung, zum Beispiel winken",
                    "Name, for example wave",
                ))
                .desired_width(260.0),
        );
    }
    ui.weak(f.kind().guide().to_string());

    section(ui, "Frames");
    ui.add(egui::DragValue::new(&mut f.frames).range(1..=MAX_FRAMES));

    section(ui, tr("Name (optional)", "Name (optional)"));
    let default_name = format!("{current} {}", f.kind().name());
    ui.add(
        egui::TextEdit::singleline(&mut f.name)
            .hint_text(default_name)
            .desired_width(260.0),
    );

    let valid = f.kind.is_some() || !f.custom.trim().is_empty();
    let (ok, cancel) = buttons(ui, tr("Anlegen", "Create"), valid);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let Some(doc) = &mut app.doc else {
        return Outcome::Close;
    };
    let created = doc.create_animation(f.character, f.kind(), f.frames, &f.name);
    finish(app, created, |app, id| {
        app.select(id);
        app.info(tr("Animation angelegt", "Animation created"));
    })
}

fn rename(app: &mut App, ui: &mut egui::Ui, id: AssetId, name: &mut String) -> Outcome {
    ui.heading(trf!("{} umbenennen", "Rename {}", id.kind.label()));
    ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
    let (ok, cancel) = buttons(ui, tr("Umbenennen", "Rename"), !name.trim().is_empty());
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let Some(doc) = &mut app.doc else {
        return Outcome::Close;
    };
    let renamed = doc.rename(id, name);
    finish(app, renamed, |app, ()| app.info(tr("Umbenannt", "Renamed")))
}

fn rename_layer(app: &mut App, ui: &mut egui::Ui, layer: usize, name: &mut String) -> Outcome {
    ui.heading(tr("Ebene umbenennen", "Rename layer"));
    ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
    let (ok, cancel) = buttons(ui, tr("Umbenennen", "Rename"), !name.trim().is_empty());
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let (Some(doc), Some(id)) = (&mut app.doc, app.sel) else {
        return Outcome::Close;
    };
    let new_name = name.trim().to_string();
    let renamed = doc.edit(id, |s| {
        // Dieselben Regeln wie im MCP-Server, der Ebenen über ihren Namen findet.
        if new_name.len() > 64 {
            return Err(tr(
                "Der Name darf höchstens 64 Zeichen lang sein",
                "The name may have at most 64 characters",
            )
            .into());
        }
        if s.layer_index(&new_name).is_some_and(|other| other != layer) {
            return Err(trf!(
                "Eine Ebene namens „{new_name}“ gibt es schon",
                "A layer named \"{new_name}\" already exists"
            ));
        }
        s.rename_layer(layer, new_name.clone())
            .map_err(|e| e.to_string())
    });
    finish(app, renamed, |app, ()| {
        app.fix_selection();
        app.info(tr("Ebene umbenannt", "Layer renamed"));
    })
}

fn delete(app: &mut App, ui: &mut egui::Ui, id: AssetId) -> Outcome {
    let Some(doc) = &mut app.doc else {
        return Outcome::Close;
    };
    let name = doc.asset_name(id).unwrap_or_default().to_string();
    ui.heading(trf!("„{name}“ löschen?", "Delete \"{name}\"?"));
    if id.kind == AssetKind::Character {
        let n = doc.project().animations_of(id.id).count();
        if n > 0 {
            ui.label(trf!(
                "Mit der Figur werden auch ihre {n} Animationen gelöscht.",
                "Its {n} animations are deleted together with the character."
            ));
        }
    }
    ui.label(tr(
        "Das lässt sich nicht rückgängig machen.",
        "This cannot be undone.",
    ));
    let (ok, cancel) = buttons(ui, tr("Löschen", "Delete"), true);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let deleted = doc.delete(id);
    finish(app, deleted, |app, ()| {
        app.fix_selection();
        app.info(trf!("„{name}“ gelöscht", "\"{name}\" deleted"));
    })
}

fn delete_project(app: &mut App, ui: &mut egui::Ui, name: &str, exports: &mut bool) -> Outcome {
    ui.set_max_width(420.0);
    ui.heading(trf!(
        "Projekt „{name}“ löschen?",
        "Delete project \"{name}\"?"
    ));
    ui.label(tr(
        "Die Projektdatei mit allen Figuren, Animationen und Bildern wird endgültig gelöscht. Das lässt sich nicht rückgängig machen.",
        "The project file with all its characters, animations and images is deleted for good. This cannot be undone.",
    ));
    let folder = app.ws.exports_dir(name);
    if folder.is_dir() {
        ui.add_space(4.0);
        ui.checkbox(
            exports,
            trf!(
                "Auch die exportierten Dateien löschen ({})",
                "Also delete the exported files ({})",
                folder.display()
            ),
        );
    }
    let (ok, cancel) = buttons(ui, tr("Endgültig löschen", "Delete for good"), true);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let deleted = app.ws.delete(name, *exports).map_err(|e| e.to_string());
    finish(app, deleted, |app, _| app.project_deleted(name))
}

fn export(app: &mut App, ui: &mut egui::Ui, f: &mut ExportForm) -> Outcome {
    ui.set_width(420.0);
    let (Some(doc), Some(id)) = (&app.doc, app.sel) else {
        return Outcome::Close;
    };
    let Some(s) = doc.sprite(id) else {
        return Outcome::Close;
    };
    let (w, h, frames) = (s.width(), s.height(), s.frames().len());
    let name = doc.asset_name(id).unwrap_or_default().to_string();
    ui.heading(trf!("„{name}“ exportieren", "Export \"{name}\""));

    section(ui, "Format");
    egui::Grid::new("export-formats")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.weak(tr("Einzelbilder", "Single images"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.kind, ExportKind::Png, "PNG")
                    .on_hover_text(tr("Verlustfrei, mit Transparenz", "Lossless, with transparency"));
                ui.radio_value(&mut f.kind, ExportKind::PngIndexed, tr("PNG mit Palette", "PNG with palette"))
                    .on_hover_text(
                        tr("Dieselben Pixel, aber mit Farbtabelle gespeichert (PNG-8). Kleinere Datei, höchstens 256 Farben.", "The same pixels, saved with a color table (PNG-8). Smaller file, at most 256 colors."),
                    );
                ui.radio_value(&mut f.kind, ExportKind::Jpg, "JPG")
                    .on_hover_text(tr("Ohne Transparenz, leicht verlustbehaftet", "No transparency, slightly lossy"));
            });
            ui.end_row();
            ui.weak("Animation");
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.kind, ExportKind::Sheet, "Spritesheet")
                    .on_hover_text(
                        tr("Alle Frames in einem PNG, dazu eine JSON-Datei mit den Positionen", "All frames in one PNG, plus a JSON file with their positions"),
                    );
                ui.radio_value(&mut f.kind, ExportKind::Gif, "GIF")
                    .on_hover_text(
                        tr("Animiertes Bild, das überall abspielt. Bis 255 Farben bleiben exakt.", "Animated image that plays everywhere. Up to 255 colors stay exact."),
                    );
                ui.radio_value(&mut f.kind, ExportKind::WebM, "WebM")
                    .on_hover_text(tr("Video mit Transparenz, braucht das Programm ffmpeg", "Video with transparency, needs the program ffmpeg"));
            });
            ui.end_row();
        });

    // Ein Video speichert Farbe nur für Blöcke aus 2×2 Bildpunkten. Bei gerader
    // Vergrößerung deckt sich jeder Block mit einem Pixel.
    let even_only = f.kind == ExportKind::WebM;
    if even_only && !f.scale.is_multiple_of(2) {
        f.scale = (f.scale + 1).min(MAX_SCALE);
    }
    section(ui, tr("Vergrößerung", "Scale"));
    ui.horizontal(|ui| {
        let lowest = if even_only { 2 } else { 1 };
        ui.add(
            egui::DragValue::new(&mut f.scale)
                .range(lowest..=MAX_SCALE)
                .suffix("×"),
        );
        ui.weak(trf!(
            "ein Frame wird {}×{} Pixel groß, ohne Weichzeichnen",
            "one frame becomes {}×{} pixels, without blurring",
            w * f.scale,
            h * f.scale
        ));
    });
    if even_only && !f.scale.is_multiple_of(2) {
        // Beim Ziehen am Feld kann eine ungerade Zahl entstehen.
        f.scale = (f.scale + 1).min(MAX_SCALE);
    }

    let looping = doc.looping(id).unwrap_or(true);
    let mut install_ffmpeg = false;
    // Ein Video kennt keine Schleife, das entscheidet das Abspielprogramm.
    let timing = |ui: &mut egui::Ui, mentions_loop: bool| {
        ui.weak(if frames < 2 {
            tr(
                "Das Asset hat nur einen Frame, es entsteht ein Standbild.",
                "The asset has only one frame, so this gives a still image.",
            )
        } else if !mentions_loop {
            tr(
                "Spielt mit den Frame-Dauern aus der Zeitleiste.",
                "Plays with the frame durations from the timeline.",
            )
        } else if looping {
            tr(
                "Spielt mit den Frame-Dauern aus der Zeitleiste, in Schleife.",
                "Plays with the frame durations from the timeline, looping.",
            )
        } else {
            tr(
                "Spielt mit den Frame-Dauern aus der Zeitleiste, einmal bis zum Ende.",
                "Plays with the frame durations from the timeline, once to the end.",
            )
        });
    };
    match f.kind {
        ExportKind::Sheet => {
            section(ui, tr("Spalten", "Columns"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut f.columns).range(0..=frames as u32));
                ui.weak(tr(
                    "0 legt alle Frames in eine Zeile",
                    "0 puts all frames in one row",
                ));
            });
        }
        ExportKind::Png | ExportKind::PngIndexed | ExportKind::Jpg => {
            if frames > 1 {
                section(ui, "Frames");
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut f.all_frames,
                        true,
                        trf!("alle {frames} einzeln", "all {frames} separately"),
                    );
                    ui.radio_value(
                        &mut f.all_frames,
                        false,
                        trf!("nur Frame {}", "only frame {}", app.frame + 1),
                    );
                });
            }
        }
        ExportKind::Gif => {
            section(ui, "GIF");
            timing(ui, true);
            ui.weak(tr("GIF kennt nur ganz deckend oder ganz durchsichtig. Halbtransparente Pixel werden zu einem von beiden.", "GIF only knows fully opaque or fully transparent. Half-transparent pixels become one of the two."));
        }
        ExportKind::WebM => {
            section(ui, "WebM");
            timing(ui, false);
            ui.weak(tr("Video-Kompression kann Farben um ein bis zwei Stufen verschieben. Für exakte Pixel nimm Spritesheet, PNG oder GIF.", "Video compression can shift colors by one or two steps. For exact pixels use sprite sheet, PNG or GIF."));
            let found = *f
                .ffmpeg
                .get_or_insert_with(|| pixel_core::video::find_ffmpeg().is_some());
            if app.installing_ffmpeg() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("ffmpeg wird installiert. Windows zeigt den Fortschritt in einem eigenen Fenster; das kann ein paar Minuten dauern.", "ffmpeg is being installed. Windows shows the progress in its own window; this can take a few minutes."));
                });
            } else if !found {
                ui.add_space(4.0);
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    tr(
                        "Für WebM fehlt noch das kostenlose Programm ffmpeg.",
                        "WebM still needs the free program ffmpeg.",
                    ),
                );
                if pixel_core::video::CAN_INSTALL_FFMPEG {
                    ui.horizontal(|ui| {
                        install_ffmpeg = ui
                            .button(RichText::new(tr("ffmpeg jetzt installieren", "Install ffmpeg now")).strong())
                            .on_hover_text(
                                tr("Lädt ffmpeg über den Windows-Paketmanager winget herunter und richtet es ein. Danach geht der WebM-Export sofort, auch für Claude.", "Downloads ffmpeg with the Windows package manager winget and sets it up. After that WebM export works right away, also for Claude."),
                            )
                            .clicked();
                        if ui.button(tr("Erneut suchen", "Search again")).clicked() {
                            f.ffmpeg = None;
                        }
                    });
                    ui.weak(pixel_core::video::ffmpeg_manual());
                } else {
                    ui.weak(tr(
                        "Installiere ffmpeg über die Paketverwaltung deines Systems.",
                        "Install ffmpeg with your system's package manager.",
                    ));
                    if ui.button(tr("Erneut suchen", "Search again")).clicked() {
                        f.ffmpeg = None;
                    }
                }
            }
        }
    }
    if f.kind == ExportKind::Jpg {
        section(ui, "JPG");
        ui.horizontal(|ui| {
            ui.label(tr("Hintergrund", "Background"));
            ui.color_edit_button_srgb(&mut f.background);
            ui.add(
                egui::DragValue::new(&mut f.quality)
                    .range(1..=100)
                    .prefix(tr("Qualität ", "Quality ")),
            );
        });
    }

    if let Some(note) = &f.note {
        ui.add_space(8.0);
        ui.colored_label(ui.visuals().warn_fg_color, note);
    }
    if !f.written.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Exportiert:", "Exported:")).strong());
        for path in f.written.iter().take(6) {
            ui.monospace(path.display().to_string());
        }
        if f.written.len() > 6 {
            ui.weak(trf!("und {} weitere", "and {} more", f.written.len() - 6));
        }
        if let Some(dir) = f.written.first().and_then(|p| p.parent()) {
            if ui.button(tr("Ordner öffnen", "Open folder")).clicked() {
                help::open_folder(dir);
            }
        }
    }

    ui.add_space(10.0);
    let mut go = false;
    let mut close = false;
    ui.horizontal(|ui| {
        go = ui
            .button(RichText::new(tr("Exportieren", "Export")).strong())
            .clicked();
        close = ui.button(tr("Schließen", "Close")).clicked();
    });
    if close {
        return Outcome::Close;
    }
    if go {
        let [r, g, b] = f.background;
        let format = match f.kind {
            ExportKind::Png => ExportFormat::Png,
            ExportKind::Jpg => ExportFormat::Jpg {
                background: Rgba::rgb(r, g, b),
                quality: f.quality,
            },
            ExportKind::Sheet => ExportFormat::Sheet { columns: f.columns },
            ExportKind::PngIndexed => ExportFormat::PngIndexed,
            ExportKind::Gif => ExportFormat::Gif { looping },
            ExportKind::WebM => ExportFormat::WebM,
        };
        let frame = (f.kind.is_single_frames() && !f.all_frames && frames > 1).then_some(app.frame);
        // Nach einer Installation von ffmpeg soll der nächste Versuch neu suchen.
        f.ffmpeg = None;
        f.note = None;
        f.written.clear();
        match doc.export(id, format, frame, f.scale) {
            Ok(out) => {
                app.dialog_error = None;
                if let Some(info) = out.gif.filter(|info| !info.exact) {
                    f.note = Some(trf!(
                        "Das Bild hat mehr Farben, als ein GIF fasst. Sie wurden auf {} ähnliche Farben reduziert. Exakt bleiben Spritesheet und PNG.", "The image has more colors than a GIF holds. They were reduced to {} similar colors. Sprite sheet and PNG stay exact.",
                        info.colors
                    ));
                }
                let n = out.files.len();
                f.written = out.files;
                app.info(if n == 1 {
                    tr("1 Datei exportiert", "1 file exported").to_string()
                } else {
                    trf!("{n} Dateien exportiert", "{n} files exported")
                });
            }
            Err(err) => app.dialog_error = Some(err),
        }
    }
    if install_ffmpeg {
        app.install_ffmpeg(ui.ctx());
    }
    Outcome::Keep
}

/// Mehrzeiliger Text zum Markieren und Kopieren. Der Knopf steht darüber,
/// damit er auch bei langen Texten sofort zu sehen ist.
fn copy_block(app: &mut App, ui: &mut egui::Ui, text: &str, button: &str, rows: usize) {
    if ui.button(button).clicked() {
        ui.ctx().copy_text(text.to_string());
        app.info(tr(
            "In die Zwischenablage kopiert",
            "Copied to the clipboard",
        ));
    }
    let mut shown = text;
    ui.add(
        egui::TextEdit::multiline(&mut shown)
            .code_editor()
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    );
}

fn connect(app: &mut App, ui: &mut egui::Ui) -> Outcome {
    ui.set_width(640.0);
    let project = app.doc.as_ref().map(|d| d.name().to_string());
    let info = ConnectInfo::detect(app.ws.home(), project.as_deref());
    ui.heading(tr("Mit Claude verbinden", "Connect to Claude"));
    ui.label(tr("Claude zeichnet über einen MCP-Server, und der steckt in diesem Programm: Mit der Angabe „--mcp“ gestartet, öffnet es kein Fenster, sondern arbeitet für Claude. Claude startet es so von selbst, sobald es einmal eingetragen ist. Beide arbeiten im selben Arbeitsordner. Deshalb siehst du hier nach kurzer Zeit, was Claude zeichnet, und Claude sieht, was du zeichnest.", "Claude draws through an MCP server, and that server is built into this program: started with \"--mcp\" it opens no window but works for Claude. Claude starts it like that by itself once it is set up. Both work in the same workspace folder, so you see here shortly what Claude draws, and Claude sees what you draw."));
    ui.add_space(6.0);
    egui::Grid::new("connect-facts")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr("Programm:", "Program:"));
            ui.monospace(info.server.display().to_string());
            ui.end_row();
            ui.label(tr("Arbeitsordner:", "Workspace folder:"));
            ui.horizontal(|ui| {
                ui.monospace(info.home.display().to_string());
                if ui.small_button(tr("öffnen", "open")).clicked() {
                    help::open_folder(&info.home);
                }
            });
            ui.end_row();
        });
    ui.weak(tr("Die Angaben unten gelten für diesen Speicherort. Verschiebst du das Programm, trage es bei Claude neu ein.", "The details below are for this location. If you move the program, set it up in Claude again."));
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .max_height(430.0)
        .show(ui, |ui| {
            egui::CollapsingHeader::new(tr("1. Am einfachsten: Claude richtet es selbst ein", "1. Easiest: Claude sets it up by itself"))
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(tr("Kopiere diesen Text und schick ihn Claude. Er enthält alle Angaben. Das klappt überall, wo Claude auf deinem PC Befehle ausführen oder Dateien bearbeiten darf, zum Beispiel in Claude Code. Im normalen Chat ohne Zugriff auf deinen PC nimm stattdessen Weg 2.", "Copy this text and send it to Claude. It contains all details. This works wherever Claude may run commands or edit files on your PC, for example in Claude Code. In a normal chat without access to your PC use way 2 instead."));
                    copy_block(app, ui, &info.setup_prompt(), tr("Prompt kopieren", "Copy prompt"), 8);
                });
            egui::CollapsingHeader::new(tr("2. Claude Desktop von Hand", "2. Claude Desktop by hand")).show(ui, |ui| {
                ui.label(tr("1. In Claude Desktop: Einstellungen › Entwickler › Konfiguration bearbeiten. Es öffnet sich die Datei claude_desktop_config.json.", "1. In Claude Desktop: Settings › Developer › Edit Config. This opens the file claude_desktop_config.json."));
                ui.label(tr("2. Diesen Inhalt eintragen. Steht dort schon ein Abschnitt \"mcpServers\", nur den Eintrag \"pixelart\" dazusetzen, mit einem Komma getrennt.", "2. Enter this content. If there already is a section \"mcpServers\", only add the entry \"pixelart\", separated by a comma."));
                copy_block(app, ui, &info.desktop_json(), tr("Eintrag kopieren", "Copy entry"), 9);
                ui.label(tr("3. Claude Desktop ganz beenden, auch im Infobereich neben der Uhr, und neu starten.", "3. Quit Claude Desktop completely, also in the system tray next to the clock, and start it again."));
            });
            egui::CollapsingHeader::new(tr("3. Claude Code von Hand", "3. Claude Code by hand")).show(ui, |ui| {
                ui.label(tr("Diesen Befehl einmal in der Eingabeaufforderung ausführen:", "Run this command once in a command prompt:"));
                copy_block(app, ui, &info.claude_code_command(), tr("Befehl kopieren", "Copy command"), 2);
            });
            egui::CollapsingHeader::new(tr("4. Andere MCP-Programme", "4. Other MCP programs")).show(ui, |ui| {
                ui.label(tr("Jedes Programm, das lokale MCP-Server über stdio starten kann, funktioniert. Einzutragen sind immer dieselben Angaben:", "Every program that can start local MCP servers over stdio works. You always enter the same details:"));
                let text = trf!(
                    "Programm:  {}\nArgumente: {} --home \"{}\"\nÜbertragung: stdio", "Program:   {}\nArguments: {} --home \"{}\"\nTransport: stdio",
                    info.server.display(),
                    help::MCP_FLAG,
                    info.home.display()
                );
                copy_block(app, ui, &text, tr("Angaben kopieren", "Copy details"), 3);
            });
            egui::CollapsingHeader::new(tr("5. Prompt zum Loslegen, wenn die Verbindung steht", "5. Prompt to get started once connected"))
                .show(ui, |ui| {
                    ui.label(tr("Dieser Text erklärt Claude, wie es mit dem Studio arbeitet. Schick ihn am Anfang eines Gesprächs und schreib darunter, was gezeichnet werden soll.", "This text explains to Claude how to work with the studio. Send it at the start of a conversation and write below what should be drawn."));
                    copy_block(app, ui, &info.usage_prompt(), tr("Prompt kopieren", "Copy prompt"), 8);
                });
        });
    ui.add_space(8.0);
    if ui.button(tr("Schließen", "Close")).clicked() {
        return Outcome::Close;
    }
    Outcome::Keep
}

fn settings(app: &mut App, ui: &mut egui::Ui, f: &mut SettingsForm) -> Outcome {
    ui.set_width(520.0);
    ui.heading(tr("Einstellungen", "Settings"));

    section(ui, tr("Speicherordner", "Save folder"));
    ui.label(tr(
        "Hier liegen deine Projekte, und hierhin wird exportiert.",
        "Your projects live here, and exports are written here.",
    ));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut f.home).desired_width(360.0));
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
            "Nach dem Wechsel öffnet sich das zuletzt bearbeitete Projekt im neuen Ordner. Claude arbeitet weiter im alten Ordner, bis du die Verbindung neu einträgst (Hilfe › Mit Claude verbinden).",
            "After switching, the most recently edited project in the new folder opens. Claude keeps working in the old folder until you set up the connection again (Help › Connect to Claude).",
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

fn shortcuts(ui: &mut egui::Ui) -> Outcome {
    ui.heading(tr("Tastenkürzel", "Keyboard shortcuts"));
    egui::Grid::new("shortcuts")
        .num_columns(2)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            for tool in Tool::ALL {
                ui.monospace(tool.key().to_string());
                ui.label(tool.label());
                ui.end_row();
            }
            let rows = [
                (
                    tr("Rechte Maustaste", "Right mouse button"),
                    tr("Farbe aufnehmen", "Pick color"),
                ),
                (
                    tr("Mausrad", "Mouse wheel"),
                    tr("Vergrößern und verkleinern", "Zoom in and out"),
                ),
                (
                    "+  −  0",
                    tr(
                        "Vergrößern, verkleinern, einpassen",
                        "Zoom in, zoom out, fit",
                    ),
                ),
                (
                    tr("Leertaste + Ziehen", "Space + drag"),
                    tr("Ausschnitt verschieben", "Pan the view"),
                ),
                (
                    tr("Mittlere Maustaste", "Middle mouse button"),
                    tr("Ausschnitt verschieben", "Pan the view"),
                ),
                (
                    tr("Strg+A, Esc", "Ctrl+A, Esc"),
                    tr("Alles auswählen, Auswahl aufheben", "Select all, deselect"),
                ),
                (
                    tr("Strg+C, Strg+X, Strg+V", "Ctrl+C, Ctrl+X, Ctrl+V"),
                    tr(
                        "Auswahl kopieren, ausschneiden, einfügen",
                        "Copy, cut, paste the selection",
                    ),
                ),
                (
                    tr("Entf", "Delete"),
                    tr("Auswahl leeren", "Clear the selection"),
                ),
                (
                    tr("Pfeil links / rechts", "Left / right arrow"),
                    tr("Vorheriger und nächster Frame", "Previous and next frame"),
                ),
                (
                    tr("Eingabetaste", "Enter"),
                    tr(
                        "Animation abspielen und anhalten",
                        "Play and pause the animation",
                    ),
                ),
                (tr("Strg+Z", "Ctrl+Z"), tr("Rückgängig", "Undo")),
                (tr("Strg+Y", "Ctrl+Y"), tr("Wiederholen", "Redo")),
                (tr("Strg+E", "Ctrl+E"), tr("Exportieren", "Export")),
                (
                    tr("Strg+I", "Ctrl+I"),
                    tr("Bild importieren", "Import image"),
                ),
            ];
            for (key, what) in rows {
                ui.monospace(key);
                ui.label(what);
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    if ui.button(tr("Schließen", "Close")).clicked() {
        return Outcome::Close;
    }
    Outcome::Keep
}

fn about(app: &mut App, ui: &mut egui::Ui) -> Outcome {
    ui.heading("PixelArt Studio");
    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
    ui.add_space(6.0);
    ui.label(tr(
        "Ein Editor für echte Pixel-Art, den auch Claude über MCP bedienen kann.",
        "An editor for real pixel art that Claude can also use through MCP.",
    ));
    ui.label(tr(
        "Gespeichert wird automatisch nach jeder Änderung.",
        "Everything is saved automatically after each change.",
    ));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(tr("Arbeitsordner:", "Workspace folder:"));
        ui.monospace(app.ws.home().display().to_string());
    });
    ui.add_space(8.0);
    if ui.button(tr("Schließen", "Close")).clicked() {
        return Outcome::Close;
    }
    Outcome::Keep
}

/// Eine Reihe kleiner Farbfelder als Vorschau einer Palette.
fn palette_strip(ui: &mut egui::Ui, colors: &[Rgba]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
        for &c in colors.iter().take(256) {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.0, crate::app::c32(c));
        }
    });
}

// ---------------------------------------------------------------- Farbe ersetzen

pub struct ReplaceForm {
    from: Rgba,
    to: Rgba,
    all_frames: bool,
    all_layers: bool,
    used: Vec<Rgba>,
    frames: usize,
}

impl ReplaceForm {
    pub fn new(app: &App) -> Self {
        let s = app.sprite();
        let used = s.map(|s| s.used_colors()).unwrap_or_default();
        Self {
            from: used.first().copied().unwrap_or(Rgba::BLACK),
            to: app.brush.color,
            all_frames: true,
            all_layers: false,
            used,
            frames: s.map_or(1, |s| s.frames().len()),
        }
    }
}

fn color_choice(ui: &mut egui::Ui, id: &str, c: &mut Rgba, choices: &[Rgba]) {
    ui.horizontal(|ui| {
        let mut rgba = c.to_array();
        if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
            *c = Rgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
        ui.monospace(c.to_string());
    });
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(70.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                for &k in choices {
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 2.0, crate::app::c32(k));
                    if k == *c {
                        ui.painter().rect_stroke(
                            rect,
                            2.0,
                            egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if resp.on_hover_text(k.to_string()).clicked() {
                        *c = k;
                    }
                }
            });
        });
}

fn replace_color(app: &mut App, ui: &mut egui::Ui, f: &mut ReplaceForm) -> Outcome {
    ui.set_width(380.0);
    ui.heading(tr("Farbe ersetzen", "Replace color"));
    ui.label(tr(
        "Jeder Pixel der einen Farbe bekommt die andere.",
        "Every pixel of one color gets the other one.",
    ));
    section(ui, tr("Diese Farbe", "This color"));
    color_choice(ui, "from", &mut f.from, &f.used);
    let palette = app
        .sprite()
        .map(|s| s.palette.colors.clone())
        .unwrap_or_default();
    section(ui, tr("wird zu", "becomes"));
    color_choice(ui, "to", &mut f.to, &palette);
    section(ui, tr("Wo", "Where"));
    if f.frames > 1 {
        ui.radio_value(
            &mut f.all_frames,
            false,
            tr("Nur im aktuellen Frame", "Only in the current frame"),
        );
        ui.radio_value(
            &mut f.all_frames,
            true,
            trf!("In allen {} Frames", "In all {} frames", f.frames),
        );
    }
    ui.checkbox(
        &mut f.all_layers,
        tr(
            "Auf allen Ebenen (sonst nur auf der gewählten)",
            "On all layers (otherwise only the selected one)",
        ),
    );
    let (ok, cancel) = buttons(ui, tr("Ersetzen", "Replace"), f.from != f.to);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let (frame, layer, from, to) = (app.frame, app.layer, f.from, f.to);
    let (all_frames, all_layers) = (f.all_frames, f.all_layers);
    let changed = app.sel.and_then(|id| {
        let doc = app.doc.as_mut()?;
        Some(doc.edit(id, |s| {
            let frames: Vec<usize> = if all_frames {
                (0..s.frames().len()).collect()
            } else {
                vec![frame]
            };
            let auto = s.outline_layer();
            let layers: Vec<usize> = if all_layers {
                (0..s.layer_count()).filter(|&l| Some(l) != auto).collect()
            } else {
                vec![layer]
            };
            let mut n = 0;
            for &fr in &frames {
                for &l in &layers {
                    n += pixel_core::draw::replace_color(
                        s,
                        pixel_core::draw::Target::new(fr, l),
                        from,
                        to,
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            Ok(n)
        }))
    });
    match changed {
        Some(result) => finish(app, result, |app, n| {
            app.info(if n == 0 {
                tr(
                    "Die Farbe kommt dort nicht vor",
                    "The color does not occur there",
                )
                .to_string()
            } else {
                trf!("{n} Pixel ersetzt", "Replaced {n} pixels")
            });
        }),
        None => Outcome::Close,
    }
}

// ---------------------------------------------------------------- Leinwandgröße

pub struct ResizeForm {
    width: u32,
    height: u32,
    old: (u32, u32),
    /// Wo der Inhalt bleibt, je Achse −1, 0 oder 1.
    align: (i8, i8),
    /// Beim Zuschneiden auf den Inhalt: der Versatz.
    crop: Option<(i64, i64)>,
    content: Option<(i64, i64, u32, u32)>,
    /// Bei Figuren und Animationen: wie viele Assets mitgeändert werden.
    together: usize,
}

impl ResizeForm {
    pub fn new(app: &App) -> Self {
        let s = app.sprite();
        let old = s.map_or((32, 32), |s| (s.width(), s.height()));
        let together = app
            .sel
            .and_then(|id| {
                let doc = app.doc.as_ref()?;
                let c = doc.character_of(id)?;
                Some(doc.project().animations_of(c).count() + 1)
            })
            .unwrap_or(1);
        // Bei Figuren bleiben die Füße unten.
        let character = together > 1 || app.sel.is_some_and(|id| id.kind != AssetKind::Image);
        Self {
            width: old.0,
            height: old.1,
            old,
            align: if character { (0, 1) } else { (0, 0) },
            crop: None,
            content: s.and_then(|s| s.content_bounds()),
            together,
        }
    }
}

fn resize(app: &mut App, ui: &mut egui::Ui, f: &mut ResizeForm) -> Outcome {
    ui.set_width(360.0);
    ui.heading(tr("Leinwandgröße", "Canvas size"));
    ui.label(trf!(
        "Jetzt {}×{} Pixel. Der Inhalt wird nicht skaliert: es kommt Platz dazu oder Rand fällt weg.",
        "Now {}×{} pixels. The content is not scaled: room is added or edges are cut away.",
        f.old.0,
        f.old.1
    ));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let w = ui.add(
            egui::DragValue::new(&mut f.width)
                .range(1..=MAX_SIZE)
                .prefix(tr("Breite ", "Width ")),
        );
        let h = ui.add(
            egui::DragValue::new(&mut f.height)
                .range(1..=MAX_SIZE)
                .prefix(tr("Höhe ", "Height ")),
        );
        if w.changed() || h.changed() {
            f.crop = None;
        }
    });
    if f.crop.is_none() {
        section(ui, tr("Inhalt bleibt", "Content stays"));
        egui::Grid::new("anchor")
            .spacing(egui::vec2(2.0, 2.0))
            .show(ui, |ui| {
                for y in -1..=1i8 {
                    for x in -1..=1i8 {
                        let on = f.align == (x, y);
                        let resp = ui.add(
                            egui::Button::new("")
                                .min_size(egui::vec2(26.0, 22.0))
                                .selected(on),
                        );
                        if on {
                            let color = ui.visuals().strong_text_color();
                            ui.painter().circle_filled(resp.rect.center(), 4.0, color);
                        }
                        if resp.clicked() {
                            f.align = (x, y);
                        }
                    }
                    ui.end_row();
                }
            });
    }
    ui.add_space(4.0);
    if let Some((x, y, w, h)) = f.content {
        if ui
            .button(tr("Auf den Inhalt zuschneiden", "Crop to content"))
            .on_hover_text(tr(
                "Kleinstes Rechteck um alles Gezeichnete, über alle Frames und Ebenen",
                "Smallest box around everything drawn, over all frames and layers",
            ))
            .clicked()
        {
            f.width = w;
            f.height = h;
            f.crop = Some((-x, -y));
        }
    }
    if f.together > 1 {
        ui.add_space(4.0);
        ui.weak(if f.together == 2 {
            tr(
                "Gilt für die Figur und ihre Animation, damit beide gleich groß bleiben. Das lässt sich nicht rückgängig machen.",
                "Applies to the character and its animation so both stay the same size. This cannot be undone.",
            )
            .to_string()
        } else {
            trf!(
                "Gilt für die Figur und alle {} Animationen, damit alle gleich groß bleiben. Das lässt sich nicht rückgängig machen.",
                "Applies to the character and all {} animations so all stay the same size. This cannot be undone.",
                f.together - 1
            )
        });
    }
    let changed = (f.width, f.height) != f.old || f.crop.is_some_and(|c| c != (0, 0));
    let (ok, cancel) = buttons(ui, tr("Übernehmen", "Apply"), changed);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let (dx, dy) = f
        .crop
        .unwrap_or_else(|| pixel_core::sprite::canvas_offset(f.old, (f.width, f.height), f.align));
    let (w, h) = (f.width, f.height);
    let result = match (app.sel, app.doc.as_mut()) {
        (Some(id), Some(doc)) => doc.resize_canvas(id, w, h, dx, dy),
        _ => return Outcome::Close,
    };
    finish(app, result, |app, _| {
        app.deselect();
        app.fix_selection();
        app.view.fit = true;
        app.info(trf!(
            "Leinwand ist jetzt {w}×{h} Pixel",
            "Canvas is now {w}×{h} pixels"
        ));
    })
}

// ---------------------------------------------------------------- Form speichern

fn save_shape(app: &mut App, ui: &mut egui::Ui, name: &mut String) -> Outcome {
    ui.set_width(360.0);
    ui.heading(tr("Als Form speichern", "Save as shape"));
    let what = if app.selection.is_some() {
        tr(
            "Das Gezeichnete in der Auswahl",
            "What is drawn inside the selection",
        )
        .to_string()
    } else {
        let layer = app
            .sprite()
            .map(|s| s.layer_names())
            .and_then(|n| n.get(app.layer).cloned())
            .unwrap_or_default();
        trf!(
            "Alles auf der Ebene „{layer}“",
            "Everything on the layer \"{layer}\""
        )
    };
    ui.label(trf!(
        "{what} wird eine eigene Form. Sie steht danach beim Werkzeug Formen in allen Projekten bereit und lässt sich in jeder Größe aufziehen.",
        "{what} becomes a custom shape. It is then available with the Shapes tool in all projects and can be dragged out at any size."
    ));
    ui.add_space(6.0);
    name_field(ui, name);
    let exists = app
        .custom_shapes
        .iter()
        .any(|c| pixel_core::workspace::name_key(&c.name) == pixel_core::workspace::name_key(name));
    let builtin = pixel_core::shapes::Shape::ALL.iter().any(|b| {
        b.label().eq_ignore_ascii_case(name.trim()) || b.id() == name.trim().to_lowercase()
    });
    if exists {
        ui.weak(tr(
            "Eine Form mit diesem Namen wird ersetzt.",
            "A shape with this name will be replaced.",
        ));
    }
    if builtin {
        ui.colored_label(
            ui.visuals().error_fg_color,
            tr(
                "So heißt schon eine eingebaute Form.",
                "A built-in shape already has this name.",
            ),
        );
    }
    let ok_name = !name.trim().is_empty() && name.trim().chars().count() <= 40 && !builtin;
    let (ok, cancel) = buttons(ui, tr("Speichern", "Save"), ok_name);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let area = app.selection.map(|a| (a.x, a.y, a.w, a.h));
    let t = app.target();
    let shape = match app.sprite() {
        Some(s) => {
            pixel_core::shapes::CustomShape::from_layer(name, s, t, area).map_err(|e| e.to_string())
        }
        None => return Outcome::Close,
    };
    let saved = shape.and_then(|shape| {
        app.ws
            .save_shape(&shape)
            .map(|_| shape)
            .map_err(|e| e.to_string())
    });
    finish(app, saved, |app, shape| {
        app.reload_shapes(Some(&shape.name));
        app.brush.tool = Tool::Shape;
        app.info(trf!(
            "Form „{}“ gespeichert ({}×{} Pixel)",
            "Shape \"{}\" saved ({}×{} pixels)",
            shape.name,
            shape.width,
            shape.height
        ));
    })
}
