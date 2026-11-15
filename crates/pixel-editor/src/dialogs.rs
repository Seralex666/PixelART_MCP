//! Dialoge: Neues Projekt, neue Figur, neues Bild, neue Animation, Export,
//! Umbenennen, Löschen und das Menü „Hilfe“.

use crate::app::{section, App, MAX_FRAMES};
use crate::doc::{AssetId, AssetKind, Res};
use crate::help::{self, ConnectInfo};
use crate::tools::Tool;
use eframe::egui::{self, RichText};
use pixel_core::export::MAX_SCALE;
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
    Connect,
    Shortcuts,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PalettePreset {
    Default16,
    Gray4,
    Gray8,
    Gray16,
}

impl PalettePreset {
    const ALL: [Self; 4] = [Self::Default16, Self::Gray4, Self::Gray8, Self::Gray16];

    fn label(self) -> &'static str {
        match self {
            Self::Default16 => "Standard (16 Farben)",
            Self::Gray4 => "Graustufen (4)",
            Self::Gray8 => "Graustufen (8)",
            Self::Gray16 => "Graustufen (16)",
        }
    }

    fn palette(self) -> Palette {
        match self {
            Self::Default16 => Palette::default_16(),
            Self::Gray4 => Palette::grayscale(4),
            Self::Gray8 => Palette::grayscale(8),
            Self::Gray16 => Palette::grayscale(16),
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
            palette: PalettePreset::Default16,
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
    Jpg,
    Sheet,
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
        }
    }
}

/// Was nach dem Zeichnen eines Dialogs geschehen soll.
enum Outcome {
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
            Dialog::Connect => connect(app, ui),
            Dialog::Shortcuts => shortcuts(ui),
            Dialog::About => about(app, ui),
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
fn buttons(ui: &mut egui::Ui, confirm: &str, enabled: bool) -> (bool, bool) {
    ui.add_space(10.0);
    let mut ok = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ok = ui
            .add_enabled(enabled, egui::Button::new(RichText::new(confirm).strong()))
            .clicked();
        cancel = ui.button("Abbrechen").clicked();
    });
    // Die Eingabetaste bestätigt, wie man es von Dialogen gewohnt ist.
    if enabled && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ok = true;
    }
    (ok, cancel)
}

/// Schließt den Dialog bei Erfolg, sonst bleibt er mit der Fehlermeldung offen.
fn finish<T>(app: &mut App, result: Res<T>, done: impl FnOnce(&mut App, T)) -> Outcome {
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
    ui.heading("Neues Projekt");
    ui.label("Ein Projekt sammelt Figuren, Animationen und Bilder in einer Datei.");
    ui.add_space(6.0);
    name_field(ui, name);
    ui.weak("Buchstaben, Ziffern, Leerzeichen, - und _");
    let (ok, cancel) = buttons(ui, "Anlegen", !name.trim().is_empty());
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
        ui.heading("Neue Figur");
        ui.label("Größe, Farbtiefe und Palette einer Figur stehen danach fest. Alle Animationen übernehmen sie, deshalb sieht die Figur überall gleich aus. Farben lassen sich später ergänzen.");
    } else {
        ui.heading("Neues Bild");
        ui.label("Ein freies Bild, zum Beispiel ein Tile, ein Item oder eine Szene.");
    }
    ui.add_space(6.0);
    name_field(ui, &mut f.name);

    section(ui, "Größe");
    let selected = f
        .template
        .and_then(|i| TEMPLATES.get(i))
        .map_or("Eigene Größe", |t| t.name);
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
                .selectable_label(f.template.is_none(), "Eigene Größe")
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
                .prefix("Breite "),
        );
        ui.add_enabled(
            custom,
            egui::DragValue::new(&mut f.height)
                .range(1..=MAX_SIZE)
                .prefix("Höhe "),
        );
        ui.weak(format!("Pixel, höchstens {MAX_SIZE}"));
    });

    section(ui, "Farbtiefe");
    ui.horizontal(|ui| {
        ui.radio_value(&mut f.bits, 8, "8 Bit")
            .on_hover_text("Nur Farben aus der Palette, höchstens 256");
        ui.radio_value(&mut f.bits, 16, "16 Bit")
            .on_hover_text("65 536 Farben, Pixel sind deckend oder durchsichtig");
        ui.radio_value(&mut f.bits, 32, "32 Bit")
            .on_hover_text("Alle Farben mit Transparenz");
    });

    section(ui, "Palette");
    egui::ComboBox::from_id_salt("palette")
        .selected_text(f.palette.label())
        .width(260.0)
        .show_ui(ui, |ui| {
            for p in PalettePreset::ALL {
                ui.selectable_value(&mut f.palette, p, p.label());
            }
        });

    let (ok, cancel) = buttons(ui, "Anlegen", !f.name.trim().is_empty());
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let depth = ColorDepth::from_bits(f.bits).unwrap_or(ColorDepth::Rgba32);
    let Some(doc) = &mut app.doc else {
        return Outcome::Close;
    };
    let created = if f.character {
        doc.create_character(&f.name, f.width, f.height, depth, f.palette.palette())
    } else {
        doc.create_image(&f.name, f.width, f.height, depth, f.palette.palette())
    };
    finish(app, created, |app, id| {
        app.select(id);
        app.info(format!("„{}“ angelegt", f.name.trim()));
    })
}

fn new_animation(app: &mut App, ui: &mut egui::Ui, f: &mut NewAnimation) -> Outcome {
    ui.set_width(380.0);
    ui.heading("Neue Animation");
    ui.label("Jeder Frame startet als Kopie der Grundpose der Figur. Du änderst dann nur, was sich bewegt.");
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

    section(ui, "Figur");
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

    section(ui, "Art");
    let label = |k: &AnimationKind| match k {
        AnimationKind::Idle => "Stehen (idle)",
        AnimationKind::Walk => "Gehen (walk)",
        AnimationKind::Run => "Rennen (run)",
        AnimationKind::Jump => "Springen (jump)",
        AnimationKind::Attack => "Angriff (attack)",
        AnimationKind::Hurt => "Treffer (hurt)",
        AnimationKind::Death => "Sterben (death)",
        AnimationKind::Custom(_) => "Eigene",
    };
    let selected = f
        .kind
        .and_then(|i| AnimationKind::PRESETS.get(i))
        .map_or("Eigene", label);
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
            if ui.selectable_label(f.kind.is_none(), "Eigene").clicked() {
                f.kind = None;
            }
        });
    if f.kind.is_none() {
        ui.add(
            egui::TextEdit::singleline(&mut f.custom)
                .hint_text("Bezeichnung, zum Beispiel winken")
                .desired_width(260.0),
        );
    }
    ui.weak(f.kind().guide().to_string());

    section(ui, "Frames");
    ui.add(egui::DragValue::new(&mut f.frames).range(1..=MAX_FRAMES));

    section(ui, "Name (optional)");
    let default_name = format!("{current} {}", f.kind().name());
    ui.add(
        egui::TextEdit::singleline(&mut f.name)
            .hint_text(default_name)
            .desired_width(260.0),
    );

    let valid = f.kind.is_some() || !f.custom.trim().is_empty();
    let (ok, cancel) = buttons(ui, "Anlegen", valid);
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
        app.info("Animation angelegt");
    })
}

fn rename(app: &mut App, ui: &mut egui::Ui, id: AssetId, name: &mut String) -> Outcome {
    ui.heading(format!("{} umbenennen", id.kind.label()));
    ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
    let (ok, cancel) = buttons(ui, "Umbenennen", !name.trim().is_empty());
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
    finish(app, renamed, |app, ()| app.info("Umbenannt"))
}

fn rename_layer(app: &mut App, ui: &mut egui::Ui, layer: usize, name: &mut String) -> Outcome {
    ui.heading("Ebene umbenennen");
    ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
    let (ok, cancel) = buttons(ui, "Umbenennen", !name.trim().is_empty());
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
            return Err("Der Name darf höchstens 64 Zeichen lang sein".into());
        }
        if s.layer_index(&new_name).is_some_and(|other| other != layer) {
            return Err(format!("Eine Ebene namens „{new_name}“ gibt es schon"));
        }
        s.rename_layer(layer, new_name.clone())
            .map_err(|e| e.to_string())
    });
    finish(app, renamed, |app, ()| {
        app.fix_selection();
        app.info("Ebene umbenannt");
    })
}

fn delete(app: &mut App, ui: &mut egui::Ui, id: AssetId) -> Outcome {
    let Some(doc) = &mut app.doc else {
        return Outcome::Close;
    };
    let name = doc.asset_name(id).unwrap_or_default().to_string();
    ui.heading(format!("„{name}“ löschen?"));
    if id.kind == AssetKind::Character {
        let n = doc.project().animations_of(id.id).count();
        if n > 0 {
            ui.label(format!(
                "Mit der Figur werden auch ihre {n} Animationen gelöscht."
            ));
        }
    }
    ui.label("Das lässt sich nicht rückgängig machen.");
    let (ok, cancel) = buttons(ui, "Löschen", true);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let deleted = doc.delete(id);
    finish(app, deleted, |app, ()| {
        app.fix_selection();
        app.info(format!("„{name}“ gelöscht"));
    })
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
    ui.heading(format!("„{name}“ exportieren"));

    section(ui, "Format");
    ui.horizontal(|ui| {
        ui.radio_value(&mut f.kind, ExportKind::Png, "PNG")
            .on_hover_text("Verlustfrei, mit Transparenz");
        ui.radio_value(&mut f.kind, ExportKind::Jpg, "JPG")
            .on_hover_text("Ohne Transparenz, leicht verlustbehaftet");
        ui.radio_value(&mut f.kind, ExportKind::Sheet, "Spritesheet")
            .on_hover_text("Alle Frames in einem PNG, dazu eine JSON-Datei mit den Positionen");
    });

    section(ui, "Vergrößerung");
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut f.scale)
                .range(1..=MAX_SCALE)
                .suffix("×"),
        );
        ui.weak(format!(
            "ein Frame wird {}×{} Pixel groß, ohne Weichzeichnen",
            w * f.scale,
            h * f.scale
        ));
    });

    match f.kind {
        ExportKind::Sheet => {
            section(ui, "Spalten");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut f.columns).range(0..=frames as u32));
                ui.weak("0 legt alle Frames in eine Zeile");
            });
        }
        ExportKind::Png | ExportKind::Jpg => {
            if frames > 1 {
                section(ui, "Frames");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut f.all_frames, true, format!("alle {frames} einzeln"));
                    ui.radio_value(
                        &mut f.all_frames,
                        false,
                        format!("nur Frame {}", app.frame + 1),
                    );
                });
            }
        }
    }
    if f.kind == ExportKind::Jpg {
        section(ui, "JPG");
        ui.horizontal(|ui| {
            ui.label("Hintergrund");
            ui.color_edit_button_srgb(&mut f.background);
            ui.add(
                egui::DragValue::new(&mut f.quality)
                    .range(1..=100)
                    .prefix("Qualität "),
            );
        });
    }

    if !f.written.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("Exportiert:").strong());
        for path in f.written.iter().take(6) {
            ui.monospace(path.display().to_string());
        }
        if f.written.len() > 6 {
            ui.weak(format!("und {} weitere", f.written.len() - 6));
        }
        if let Some(dir) = f.written.first().and_then(|p| p.parent()) {
            if ui.button("Ordner öffnen").clicked() {
                help::open_folder(dir);
            }
        }
    }

    ui.add_space(10.0);
    let mut go = false;
    let mut close = false;
    ui.horizontal(|ui| {
        go = ui.button(RichText::new("Exportieren").strong()).clicked();
        close = ui.button("Schließen").clicked();
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
        };
        let frame =
            (f.kind != ExportKind::Sheet && !f.all_frames && frames > 1).then_some(app.frame);
        match doc.export(id, format, frame, f.scale) {
            Ok(out) => {
                app.dialog_error = None;
                let n = out.files.len();
                f.written = out.files;
                app.info(if n == 1 {
                    "1 Datei exportiert".to_string()
                } else {
                    format!("{n} Dateien exportiert")
                });
            }
            Err(err) => app.dialog_error = Some(err),
        }
    }
    Outcome::Keep
}

/// Mehrzeiliger Text zum Markieren und Kopieren. Der Knopf steht darüber,
/// damit er auch bei langen Texten sofort zu sehen ist.
fn copy_block(app: &mut App, ui: &mut egui::Ui, text: &str, button: &str, rows: usize) {
    if ui.button(button).clicked() {
        ui.ctx().copy_text(text.to_string());
        app.info("In die Zwischenablage kopiert");
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
    ui.heading("Mit Claude verbinden");
    ui.label("Claude zeichnet über den MCP-Server „pixelart-mcp“. Das ist ein kleines Programm, das Claude selbst startet, sobald es einmal eingetragen ist. Es arbeitet im selben Arbeitsordner wie dieser Editor. Deshalb siehst du hier nach kurzer Zeit, was Claude zeichnet, und Claude sieht, was du zeichnest.");
    ui.add_space(6.0);
    egui::Grid::new("connect-facts")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("MCP-Server:");
            ui.horizontal(|ui| {
                ui.monospace(info.server.display().to_string());
                if info.server_found {
                    ui.colored_label(egui::Color32::from_rgb(60, 160, 80), "gefunden");
                } else {
                    ui.colored_label(ui.visuals().error_fg_color, "fehlt");
                }
            });
            ui.end_row();
            ui.label("Arbeitsordner:");
            ui.horizontal(|ui| {
                ui.monospace(info.home.display().to_string());
                if ui.small_button("öffnen").clicked() {
                    help::open_folder(&info.home);
                }
            });
            ui.end_row();
        });
    if !info.server_found {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Lege die Datei „pixelart-mcp“ in denselben Ordner wie dieses Programm. Erst dann stimmen die Angaben unten.",
        );
    }
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .max_height(430.0)
        .show(ui, |ui| {
            egui::CollapsingHeader::new("1. Am einfachsten: Claude richtet es selbst ein")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label("Kopiere diesen Text und schick ihn Claude. Er enthält alle Angaben. Das klappt überall, wo Claude auf deinem PC Befehle ausführen oder Dateien bearbeiten darf, zum Beispiel in Claude Code. Im normalen Chat ohne Zugriff auf deinen PC nimm stattdessen Weg 2.");
                    copy_block(app, ui, &info.setup_prompt(), "Prompt kopieren", 8);
                });
            egui::CollapsingHeader::new("2. Claude Desktop von Hand").show(ui, |ui| {
                ui.label("1. In Claude Desktop: Einstellungen → Entwickler → Konfiguration bearbeiten. Es öffnet sich die Datei claude_desktop_config.json.");
                ui.label("2. Diesen Inhalt eintragen. Steht dort schon ein Abschnitt \"mcpServers\", nur den Eintrag \"pixelart\" dazusetzen, mit einem Komma getrennt.");
                copy_block(app, ui, &info.desktop_json(), "Eintrag kopieren", 9);
                ui.label("3. Claude Desktop ganz beenden, auch im Infobereich neben der Uhr, und neu starten.");
            });
            egui::CollapsingHeader::new("3. Claude Code von Hand").show(ui, |ui| {
                ui.label("Diesen Befehl einmal in der Eingabeaufforderung ausführen:");
                copy_block(app, ui, &info.claude_code_command(), "Befehl kopieren", 2);
            });
            egui::CollapsingHeader::new("4. Andere MCP-Programme").show(ui, |ui| {
                ui.label("Jedes Programm, das lokale MCP-Server über stdio starten kann, funktioniert. Einzutragen sind immer dieselben Angaben:");
                let text = format!(
                    "Programm:  {}\nArgumente: --home \"{}\"\nÜbertragung: stdio",
                    info.server.display(),
                    info.home.display()
                );
                copy_block(app, ui, &text, "Angaben kopieren", 3);
            });
            egui::CollapsingHeader::new("5. Prompt zum Loslegen, wenn die Verbindung steht")
                .show(ui, |ui| {
                    ui.label("Dieser Text erklärt Claude, wie es mit dem Studio arbeitet. Schick ihn am Anfang eines Gesprächs und schreib darunter, was gezeichnet werden soll.");
                    copy_block(app, ui, &info.usage_prompt(), "Prompt kopieren", 8);
                });
        });
    ui.add_space(8.0);
    if ui.button("Schließen").clicked() {
        return Outcome::Close;
    }
    Outcome::Keep
}

fn shortcuts(ui: &mut egui::Ui) -> Outcome {
    ui.heading("Tastenkürzel");
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
                ("Rechte Maustaste", "Farbe aufnehmen"),
                ("Mausrad", "Vergrößern und verkleinern"),
                ("+  −  0", "Vergrößern, verkleinern, einpassen"),
                ("Leertaste + Ziehen", "Ausschnitt verschieben"),
                ("Mittlere Maustaste", "Ausschnitt verschieben"),
                ("←  →", "Vorheriger und nächster Frame"),
                ("Eingabetaste", "Animation abspielen und anhalten"),
                ("Strg+Z", "Rückgängig"),
                ("Strg+Y", "Wiederholen"),
                ("Strg+E", "Exportieren"),
            ];
            for (key, what) in rows {
                ui.monospace(key);
                ui.label(what);
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    if ui.button("Schließen").clicked() {
        return Outcome::Close;
    }
    Outcome::Keep
}

fn about(app: &mut App, ui: &mut egui::Ui) -> Outcome {
    ui.heading("PixelArt Studio");
    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
    ui.add_space(6.0);
    ui.label("Ein Editor für echte Pixel-Art, den auch Claude über MCP bedienen kann.");
    ui.label("Gespeichert wird automatisch nach jeder Änderung.");
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Arbeitsordner:");
        ui.monospace(app.ws.home().display().to_string());
    });
    ui.add_space(8.0);
    if ui.button("Schließen").clicked() {
        return Outcome::Close;
    }
    Outcome::Keep
}
