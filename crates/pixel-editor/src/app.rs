//! Das Hauptfenster: Zustand des Editors und die Leisten rund um die Leinwand.

use crate::canvas;
use crate::dialogs::{self, Dialog, ExportForm, NewAnimation, NewSprite, SettingsForm};
use crate::doc::{AssetEntry, AssetId, AssetKind, Doc, Res};
use crate::help;
use crate::icons::Icons;
use crate::import_dialog::ImportForm;
use crate::outline_dialog::OutlineForm;
use crate::selection::FloatKey;
use crate::settings::Settings;
use crate::tools::{Brush, Lifted, ShapeChoice, Stroke, Tool};
use eframe::egui::{self, Color32, Key, RichText, Sense, StrokeKind};
use pixel_core::draw::{self, Target};
use pixel_core::effects::{Area, Patch};
use pixel_core::lang::{tr, Lang};
use pixel_core::shapes::CustomShape;
use pixel_core::trf;
use pixel_core::{Rgba, Sprite, Workspace};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const MAX_FRAMES: usize = 256;
pub const MAX_LAYERS: usize = 64;
/// Vergrößerungsstufen der Leinwand (Bildschirmpixel je Bildpixel).
pub const ZOOM_LEVELS: [u32; 12] = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64];
/// So oft wird nachgesehen, ob die Projektdatei von außen geändert wurde.
const POLL_SECONDS: f64 = 0.5;
/// Bis zu dieser Pixelmenge bekommt die Timeline kleine Vorschaubilder.
const THUMB_BUDGET: usize = 2_000_000;
const THUMB_SIZE: f32 = 48.0;
/// Höhe einer Zeile in der Ebenenliste und Größe ihres Vorschaubilds.
const LAYER_ROW: f32 = 40.0;
const LAYER_THUMB: f32 = 32.0;
const UV: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

pub fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Ausschnitt und Vergrößerung der Leinwand.
pub struct View {
    pub zoom: u32,
    pub pan: egui::Vec2,
    /// Beim nächsten Zeichnen so einpassen, dass das ganze Bild sichtbar ist.
    pub fit: bool,
    pub scroll: f32,
}

type TexKey = (u64, AssetId, usize);

#[derive(Default)]
pub struct Textures {
    pub checker: Option<egui::TextureHandle>,
    pub main: Option<(TexKey, egui::TextureHandle)>,
    pub onion: Option<(TexKey, egui::TextureHandle)>,
    thumbs: Option<((u64, AssetId), Vec<egui::TextureHandle>)>,
    layers: Option<(TexKey, Vec<egui::TextureHandle>)>,
}

pub struct App {
    pub ws: Workspace,
    pub projects: Vec<String>,
    pub doc: Option<Doc>,
    pub sel: Option<AssetId>,
    pub frame: usize,
    pub layer: usize,
    pub brush: Brush,
    pub view: View,
    pub grid: bool,
    pub onion: bool,
    pub stroke: Option<(AssetId, Stroke)>,
    pub playing: bool,
    play_clock: f32,
    pub textures: Textures,
    pub dialog: Dialog,
    pub dialog_error: Option<String>,
    status: String,
    status_is_error: bool,
    last_poll: f64,
    /// Zelle unter dem Mauszeiger, für die Statusleiste.
    pub hover_cell: Option<(i64, i64)>,
    duration_edit: Option<u32>,
    /// Läuft gerade eine Installation von ffmpeg, kommt hier ihr Ergebnis an.
    ffmpeg_install: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    shot: Option<Shot>,
    pub settings: Settings,
    pub(crate) icons: Icons,
    /// Die Auswahl auf der Leinwand.
    pub selection: Option<Area>,
    /// Ein angehobener Ausschnitt, der noch verschoben werden kann.
    pub floating: Option<(FloatKey, Lifted)>,
    pub clipboard: Option<Patch>,
    /// Die eigenen Formen aus dem Speicherordner.
    pub custom_shapes: Vec<CustomShape>,
    /// Zeitpunkt des letzten Sprühstoßes.
    pub last_puff: f64,
    /// Deckkraft, während der Regler gezogen wird.
    opacity_edit: Option<u32>,
}

/// Bildschirmfoto für automatische Prüfungen (`PIXELART_SHOT=<datei.png>`).
struct Shot {
    path: std::path::PathBuf,
    frames: u32,
    requested: bool,
    done: Arc<AtomicBool>,
}

impl App {
    pub fn new(ws: Workspace, project: Option<String>, settings: Settings) -> Self {
        let shapes = ws.custom_shapes();
        let mut app = Self {
            projects: ws.project_names(),
            ws,
            doc: None,
            sel: None,
            frame: 0,
            layer: 0,
            brush: Brush::default(),
            view: View {
                zoom: 8,
                pan: egui::Vec2::ZERO,
                fit: true,
                scroll: 0.0,
            },
            grid: true,
            onion: false,
            stroke: None,
            playing: false,
            play_clock: 0.0,
            textures: Textures::default(),
            dialog: Dialog::None,
            dialog_error: None,
            status: String::new(),
            status_is_error: false,
            last_poll: 0.0,
            hover_cell: None,
            duration_edit: None,
            ffmpeg_install: None,
            shot: std::env::var_os("PIXELART_SHOT").map(|p| Shot {
                path: p.into(),
                frames: 0,
                requested: false,
                done: Arc::new(AtomicBool::new(false)),
            }),
            custom_shapes: shapes,
            settings,
            icons: Icons::default(),
            selection: None,
            floating: None,
            clipboard: None,
            last_puff: 0.0,
            opacity_edit: None,
        };
        app.open_latest(project);
        match std::env::var("PIXELART_SHOT_DIALOG").as_deref() {
            Ok("connect") => app.dialog = Dialog::Connect,
            Ok("export") => app.dialog = Dialog::Export(ExportForm::default()),
            Ok("character") => app.dialog = Dialog::NewSprite(NewSprite::new(true)),
            Ok("settings") => app.dialog = Dialog::Settings(SettingsForm::new(&app)),
            Ok("outline") => app.outline_dialog(),
            Ok("resize") => app.open_dialog(Dialog::Resize(dialogs::ResizeForm::new(&app))),
            Ok("replace") => app.open_dialog(Dialog::ReplaceColor(dialogs::ReplaceForm::new(&app))),
            Ok("shape") => app.open_dialog(Dialog::SaveShape {
                name: "Stern".into(),
            }),
            Ok(other) => {
                // „import:<datei>“ öffnet den Import-Dialog mit dieser Datei.
                if let Some(path) = other.strip_prefix("import:") {
                    app.import_dialog(Some(path.into()));
                }
            }
            _ => {}
        }
        app
    }

    // ------------------------------------------------------------ Zustand

    pub fn info(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_is_error = false;
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_is_error = true;
    }

    /// Zeigt einen Fehler in der Statusleiste und gibt das Ergebnis weiter.
    pub fn check<T>(&mut self, result: Res<T>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(err) => {
                self.error(err);
                None
            }
        }
    }

    pub fn sprite(&self) -> Option<&Sprite> {
        self.doc.as_ref()?.sprite(self.sel?)
    }

    pub fn open_project(&mut self, name: &str) {
        match Doc::open(&self.ws, name) {
            Ok(doc) => {
                self.doc = Some(doc);
                // Ein neu geöffnetes Projekt zählt seine Änderungen wieder von vorn, und
                // übernommene Figuren behalten ihre Kennung. Alte Bilder im Zwischenspeicher
                // könnten deshalb für die des neuen Projekts gehalten werden.
                self.textures.main = None;
                self.textures.onion = None;
                self.textures.thumbs = None;
                self.textures.layers = None;
                self.stroke = None;
                self.playing = false;
                self.sel = None;
                self.fix_selection();
                self.fit_color();
                self.info(trf!(
                    "Projekt „{name}“ geöffnet",
                    "Project \"{name}\" opened"
                ));
            }
            Err(err) => self.error(trf!(
                "Projekt „{name}“ lässt sich nicht öffnen: {err}",
                "Project \"{name}\" cannot be opened: {err}"
            )),
        }
    }

    /// Übernimmt Speicherordner und Sprache aus dem Einstellungs-Dialog und speichert sie.
    pub fn apply_settings(&mut self, home: std::path::PathBuf, lang: Lang) -> Res<()> {
        if home != self.ws.home() {
            let ws = Workspace::open(&home).map_err(|err| {
                trf!(
                    "Der Ordner {} lässt sich nicht als Speicherordner nutzen: {err}",
                    "The folder {} cannot be used as save folder: {err}",
                    home.display()
                )
            })?;
            self.ws = ws;
            self.custom_shapes = self.ws.custom_shapes();
            self.brush.shape = ShapeChoice::Builtin(pixel_core::shapes::Shape::Heart);
            self.doc = None;
            self.sel = None;
            self.projects = self.ws.project_names();
            self.open_latest(None);
        }
        pixel_core::lang::set(lang);
        self.settings.lang = lang;
        // Nur ein Ordner, der vom Standard abweicht, wird gemerkt. So folgt das
        // Programm dem Standard, falls der sich ändert (etwa ein „Projekte“-Ordner
        // neben der exe).
        let default = Settings { home: None, lang }.effective_home();
        self.settings.home =
            (default.as_deref() != Some(self.ws.home())).then(|| self.ws.home().to_path_buf());
        self.settings.save()
    }

    /// Ein Projekt ist weg (hier gelöscht oder von außen). War es offen, wird es
    /// geschlossen und das zuletzt bearbeitete andere geöffnet.
    pub fn project_deleted(&mut self, name: &str) {
        self.projects = self.ws.project_names();
        if self.doc.as_ref().is_some_and(|d| d.name() == name) {
            self.doc = None;
            self.sel = None;
            self.stroke = None;
            self.playing = false;
            self.open_latest(None);
        }
        self.info(trf!(
            "Projekt „{name}“ gelöscht",
            "Project \"{name}\" deleted"
        ));
    }

    /// Öffnet das gewünschte, sonst das zuletzt bearbeitete Projekt.
    fn open_latest(&mut self, project: Option<String>) {
        let latest = project.or_else(|| {
            self.projects
                .iter()
                .max_by_key(|name| self.ws.modified(name))
                .cloned()
        });
        if let Some(name) = latest {
            self.open_project(&name);
        }
    }

    pub fn select(&mut self, id: AssetId) {
        if self.sel != Some(id) {
            self.sel = Some(id);
            self.selection = None;
            self.floating = None;
            self.frame = 0;
            self.layer = 0;
            self.view.fit = true;
            self.playing = false;
        }
        self.fix_selection();
        self.fit_color();
    }

    /// Bei fester Palette mit einer Farbe starten, die es dort gibt.
    fn fit_color(&mut self) {
        let Some(s) = self.sprite() else { return };
        let palette = &s.palette;
        let exact = s.depth.conform(self.brush.color);
        if (palette.locked || s.depth == pixel_core::ColorDepth::Indexed8)
            && !palette.contains(exact)
        {
            if let Some(&first) = palette.colors.first() {
                self.brush.color = first;
            }
        }
    }

    /// Hält Auswahl, Frame und Ebene gültig, auch wenn sich das Projekt von
    /// außen geändert hat.
    pub fn fix_selection(&mut self) {
        let Some(doc) = &self.doc else {
            self.sel = None;
            return;
        };
        if self.sel.is_none_or(|id| doc.sprite(id).is_none()) {
            self.sel = doc.assets().first().map(|a| a.id);
            self.frame = 0;
            self.layer = 0;
            self.view.fit = true;
        }
        if let Some(s) = self.sel.and_then(|id| doc.sprite(id)) {
            self.frame = self.frame.min(s.frames().len().saturating_sub(1));
            self.layer = self.layer.min(s.layer_count().saturating_sub(1));
        }
    }

    /// Ändert das ausgewählte Sprite, mit Rückgängig und Speichern.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Sprite) -> Res<T>) -> Option<T> {
        let id = self.sel?;
        let result = self.doc.as_mut()?.edit(id, f);
        self.fix_selection();
        self.check(result)
    }

    pub(crate) fn target(&self) -> Target {
        Target::new(self.frame, self.layer)
    }

    pub fn undo(&mut self, redo: bool) {
        let Some(doc) = &mut self.doc else { return };
        let result = if redo { doc.redo() } else { doc.undo() };
        match result {
            Ok(Some(id)) => {
                self.select(id);
                self.info(if redo {
                    tr("Wiederholt", "Redone")
                } else {
                    tr("Rückgängig", "Undone")
                });
            }
            Ok(None) => {}
            Err(err) => self.error(err),
        }
    }

    pub fn set_zoom(&mut self, step: i32) {
        let i = ZOOM_LEVELS
            .iter()
            .position(|&z| z >= self.view.zoom)
            .unwrap_or(ZOOM_LEVELS.len() - 1) as i32;
        let i = (i + step).clamp(0, ZOOM_LEVELS.len() as i32 - 1);
        self.view.zoom = ZOOM_LEVELS[i as usize];
        self.view.fit = false;
    }

    // ------------------------------------------------------------ ffmpeg

    pub fn installing_ffmpeg(&self) -> bool {
        self.ffmpeg_install.is_some()
    }

    /// Installiert ffmpeg im Hintergrund, damit das Fenster bedienbar bleibt.
    pub fn install_ffmpeg(&mut self, ctx: &egui::Context) {
        if self.ffmpeg_install.is_some() {
            return;
        }
        let (result, receiver) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = pixel_core::video::install_ffmpeg().map_err(|err| err.to_string());
            let _ = result.send(outcome);
            ctx.request_repaint();
        });
        self.ffmpeg_install = Some(receiver);
        self.dialog_error = None;
        self.info(tr("ffmpeg wird installiert …", "Installing ffmpeg …"));
    }

    fn poll_ffmpeg_install(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let Some(receiver) = &self.ffmpeg_install else {
            return;
        };
        let outcome = match receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(tr(
                "Die Installation von ffmpeg wurde unerwartet beendet.",
                "The installation of ffmpeg ended unexpectedly.",
            )
            .to_string()),
        };
        self.ffmpeg_install = None;
        // Der Export-Dialog soll danach neu nachsehen, ob ffmpeg da ist.
        if let Dialog::Export(form) = &mut self.dialog {
            form.ffmpeg = None;
        }
        match outcome {
            Ok(path) => self.info(trf!(
                "ffmpeg ist installiert: {}",
                "ffmpeg is installed: {}",
                path.display()
            )),
            Err(err) => {
                if matches!(self.dialog, Dialog::Export(_)) {
                    self.dialog_error = Some(err.clone());
                }
                self.error(err);
            }
        }
    }

    // ------------------------------------------------------------ Takt

    fn tick(&mut self, ctx: &egui::Context) {
        let (now, dt) = ctx.input(|i| (i.time, i.stable_dt));
        if self.stroke.is_none() && now - self.last_poll >= POLL_SECONDS {
            self.last_poll = now;
            self.projects = self.ws.project_names();
            // Von außen gelöscht (etwa durch Claude): nicht durch Speichern wieder anlegen.
            let gone = self
                .doc
                .as_ref()
                .filter(|d| !self.ws.exists(d.name()))
                .map(|d| d.name().to_string());
            if let Some(name) = gone {
                self.project_deleted(&name);
            }
            if let Some(doc) = &mut self.doc {
                if doc.poll(self.sel) {
                    self.fix_selection();
                    self.info(tr(
                        "Von außen geändert (zum Beispiel durch Claude), neu geladen",
                        "Changed from outside (for example by Claude), reloaded",
                    ));
                }
            }
        }
        ctx.request_repaint_after_secs(POLL_SECONDS as f32);
        self.poll_ffmpeg_install();
        if let Some(notice) = self.doc.as_mut().and_then(|d| d.take_notice()) {
            self.error(notice);
        }

        if self.playing {
            let durations: Vec<u32> = self
                .sprite()
                .map(|s| s.frames().iter().map(|f| f.duration_ms.max(10)).collect())
                .unwrap_or_default();
            if durations.len() < 2 {
                self.playing = false;
            } else {
                // Freie Bilder und Figuren laufen in der Vorschau im Kreis. Animationen
                // richten sich danach, ob sie wiederholt werden sollen.
                let looping = self.looping().unwrap_or(true);
                let last = durations.len() - 1;
                self.play_clock += dt.min(0.25) * 1000.0;
                self.frame = self.frame.min(last);
                while self.playing && self.play_clock >= durations[self.frame] as f32 {
                    self.play_clock -= durations[self.frame] as f32;
                    if self.frame < last {
                        self.frame += 1;
                    } else if looping {
                        self.frame = 0;
                    } else {
                        self.playing = false;
                    }
                }
                ctx.request_repaint();
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !matches!(self.dialog, Dialog::None)
            || self.stroke.is_some()
            || ctx.egui_wants_keyboard_input()
        {
            return;
        }
        let pressed = |key: Key| ctx.input(|i| i.key_pressed(key));
        // Kopieren, Ausschneiden und Einfügen kommen je nach System als eigene Ereignisse.
        let clip = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Copy => Some(0),
                egui::Event::Cut => Some(1),
                egui::Event::Paste(_) => Some(2),
                _ => None,
            })
        });
        match clip {
            Some(0) => {
                self.copy_selection();
                return;
            }
            Some(1) => {
                self.cut_selection();
                return;
            }
            Some(2) => {
                self.paste();
                return;
            }
            _ => {}
        }
        let (command, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        if command {
            if pressed(Key::Z) {
                self.undo(shift);
            } else if pressed(Key::Y) {
                self.undo(true);
            } else if pressed(Key::E) && self.sel.is_some() {
                self.dialog = Dialog::Export(ExportForm::default());
            } else if pressed(Key::I) {
                self.import_dialog(None);
            } else if pressed(Key::C) {
                self.copy_selection();
            } else if pressed(Key::X) {
                self.cut_selection();
            } else if pressed(Key::V) {
                self.paste();
            } else if pressed(Key::A) {
                self.select_all();
            }
            return;
        }
        if pressed(Key::Delete) && self.selection.is_some() {
            self.delete_selection();
        }
        if pressed(Key::Escape) && self.selection.is_some() {
            self.deselect();
        }
        for tool in Tool::ALL {
            if Key::from_name(&tool.key().to_string()).is_some_and(pressed) {
                self.brush.tool = tool;
            }
        }
        if pressed(Key::Plus) || pressed(Key::Equals) {
            self.set_zoom(1);
        }
        if pressed(Key::Minus) {
            self.set_zoom(-1);
        }
        if pressed(Key::Num0) {
            self.view.fit = true;
        }
        let frames = self.sprite().map_or(0, |s| s.frames().len());
        if frames > 1 {
            if pressed(Key::ArrowRight) {
                self.playing = false;
                self.frame = (self.frame + 1) % frames;
            }
            if pressed(Key::ArrowLeft) {
                self.playing = false;
                self.frame = (self.frame + frames - 1) % frames;
            }
            if pressed(Key::Enter) {
                self.toggle_play();
            }
        }
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        self.play_clock = 0.0;
        // Eine Animation ohne Wiederholung steht nach dem Abspielen am Ende:
        // dann von vorn beginnen.
        let frames = self.sprite().map_or(0, |s| s.frames().len());
        if self.playing && self.looping() == Some(false) && self.frame + 1 >= frames {
            self.frame = 0;
        }
    }

    /// Ob die gewählte Animation wiederholt wird; `None` bei Figuren und Bildern.
    fn looping(&self) -> Option<bool> {
        self.doc.as_ref()?.looping(self.sel?)
    }

    fn take_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        shot.frames += 1;
        ctx.request_repaint();
        if shot.done.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if shot.frames > 20 && !shot.requested {
            shot.requested = true;
            let path = shot.path.clone();
            let done = shot.done.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
                let _ =
                    image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8);
                done.store(true, Ordering::Relaxed);
            });
        }
    }

    // ------------------------------------------------------------ Leisten

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let has_doc = self.doc.is_some();
        let has_sel = self.sel.is_some();
        let has_character = self
            .doc
            .as_ref()
            .is_some_and(|d| !d.project().characters.is_empty());
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("Datei", "File"), |ui| {
                if ui.button(tr("Neues Projekt…", "New project…")).clicked() {
                    self.open_dialog(Dialog::NewProject {
                        name: String::new(),
                    });
                }
                ui.separator();
                if ui
                    .add_enabled(
                        has_doc,
                        egui::Button::new(tr("Neue Figur…", "New character…")),
                    )
                    .clicked()
                {
                    self.open_dialog(Dialog::NewSprite(NewSprite::new(true)));
                }
                if ui
                    .add_enabled(
                        has_character,
                        egui::Button::new(tr("Neue Animation…", "New animation…")),
                    )
                    .clicked()
                {
                    self.new_animation_dialog(None);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new(tr("Neues Bild…", "New image…")))
                    .clicked()
                {
                    self.open_dialog(Dialog::NewSprite(NewSprite::new(false)));
                }
                ui.separator();
                if ui
                    .add_enabled(
                        has_doc,
                        egui::Button::new(tr("Bild importieren…  Strg+I", "Import image…  Ctrl+I")),
                    )
                    .clicked()
                {
                    self.import_dialog(None);
                }
                if ui
                    .add_enabled(
                        has_sel,
                        egui::Button::new(tr("Exportieren…  Strg+E", "Export…  Ctrl+E")),
                    )
                    .clicked()
                {
                    self.open_dialog(Dialog::Export(ExportForm::default()));
                }
                if ui
                    .add_enabled(
                        has_doc,
                        egui::Button::new(tr("Projekt löschen…", "Delete project…")),
                    )
                    .clicked()
                {
                    if let Some(name) = self.doc.as_ref().map(|d| d.name().to_string()) {
                        self.open_dialog(Dialog::DeleteProject {
                            name,
                            exports: false,
                        });
                    }
                }
                ui.separator();
                if ui
                    .button(tr("Arbeitsordner öffnen", "Open workspace folder"))
                    .clicked()
                {
                    help::open_folder(self.ws.home());
                }
                if ui.button(tr("Einstellungen…", "Settings…")).clicked() {
                    let form = SettingsForm::new(self);
                    self.open_dialog(Dialog::Settings(form));
                }
                ui.separator();
                if ui.button(tr("Beenden", "Quit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(tr("Bearbeiten", "Edit"), |ui| {
                let (can_undo, can_redo) = self
                    .doc
                    .as_ref()
                    .map_or((false, false), |d| (d.can_undo(), d.can_redo()));
                if ui
                    .add_enabled(
                        can_undo,
                        egui::Button::new(tr("Rückgängig  Strg+Z", "Undo  Ctrl+Z")),
                    )
                    .clicked()
                {
                    self.undo(false);
                }
                if ui
                    .add_enabled(
                        can_redo,
                        egui::Button::new(tr("Wiederholen  Strg+Y", "Redo  Ctrl+Y")),
                    )
                    .clicked()
                {
                    self.undo(true);
                }
                ui.separator();
                if ui
                    .add_enabled(
                        has_sel,
                        egui::Button::new(tr("Alles auswählen  Strg+A", "Select all  Ctrl+A")),
                    )
                    .clicked()
                {
                    self.select_all();
                }
                let has_area = self.selection.is_some();
                if ui
                    .add_enabled(
                        has_area,
                        egui::Button::new(tr("Kopieren  Strg+C", "Copy  Ctrl+C")),
                    )
                    .clicked()
                {
                    self.copy_selection();
                }
                if ui
                    .add_enabled(
                        has_area,
                        egui::Button::new(tr("Ausschneiden  Strg+X", "Cut  Ctrl+X")),
                    )
                    .clicked()
                {
                    self.cut_selection();
                }
                if ui
                    .add_enabled(
                        has_sel && self.clipboard.is_some(),
                        egui::Button::new(tr("Einfügen  Strg+V", "Paste  Ctrl+V")),
                    )
                    .clicked()
                {
                    self.paste();
                }
                ui.separator();
                if ui
                    .add_enabled(has_sel, egui::Button::new(tr("Kontur…", "Outline…")))
                    .clicked()
                {
                    self.outline_dialog();
                }
                if ui
                    .add_enabled(
                        has_sel,
                        egui::Button::new(tr("Farbe ersetzen…", "Replace color…")),
                    )
                    .clicked()
                {
                    let form = dialogs::ReplaceForm::new(self);
                    self.open_dialog(Dialog::ReplaceColor(form));
                }
                if ui
                    .add_enabled(
                        has_sel,
                        egui::Button::new(tr("Leinwandgröße…", "Canvas size…")),
                    )
                    .clicked()
                {
                    let form = dialogs::ResizeForm::new(self);
                    self.open_dialog(Dialog::Resize(form));
                }
                if ui
                    .add_enabled(
                        has_sel,
                        egui::Button::new(tr("Ebene spiegeln", "Mirror layer")),
                    )
                    .clicked()
                {
                    let t = self.target();
                    self.edit(|s| draw::flip_horizontal(s, t).map_err(|e| e.to_string()));
                }
                if ui
                    .add_enabled(
                        has_sel,
                        egui::Button::new(tr("Ebene leeren", "Clear layer")),
                    )
                    .clicked()
                {
                    let t = self.target();
                    self.edit(|s| {
                        s.layer_mut(t.frame, t.layer)
                            .map(|l| l.clear())
                            .map_err(|e| e.to_string())
                    });
                }
            });
            ui.menu_button(tr("Ansicht", "View"), |ui| {
                ui.checkbox(&mut self.grid, tr("Raster", "Grid"));
                ui.checkbox(
                    &mut self.onion,
                    tr(
                        "Zwiebelhaut (vorheriger Frame)",
                        "Onion skin (previous frame)",
                    ),
                );
                ui.separator();
                if ui.button(tr("Vergrößern  +", "Zoom in  +")).clicked() {
                    self.set_zoom(1);
                }
                if ui.button(tr("Verkleinern  −", "Zoom out  −")).clicked() {
                    self.set_zoom(-1);
                }
                if ui.button(tr("Einpassen  0", "Fit  0")).clicked() {
                    self.view.fit = true;
                }
            });
            ui.menu_button(tr("Hilfe", "Help"), |ui| {
                if ui
                    .button(tr("Mit Claude verbinden…", "Connect to Claude…"))
                    .clicked()
                {
                    self.open_dialog(Dialog::Connect);
                }
                if ui
                    .button(tr("Tastenkürzel", "Keyboard shortcuts"))
                    .clicked()
                {
                    self.open_dialog(Dialog::Shortcuts);
                }
                if ui
                    .button(tr("Über PixelArt Studio", "About PixelArt Studio"))
                    .clicked()
                {
                    self.open_dialog(Dialog::About);
                }
            });

            ui.separator();
            ui.label(tr("Projekt:", "Project:"));
            let current = self
                .doc
                .as_ref()
                .map_or(tr("(keins)", "(none)").to_string(), |d| {
                    d.name().to_string()
                });
            let mut open = None;
            let mut delete = None;
            egui::ComboBox::from_id_salt("project")
                .selected_text(current.clone())
                .show_ui(ui, |ui| {
                    for name in &self.projects {
                        let resp = ui
                            .selectable_label(*name == current, name)
                            .on_hover_text(tr("Rechtsklick zum Löschen", "Right-click to delete"));
                        if resp.clicked() {
                            open = Some(name.clone());
                        }
                        resp.context_menu(|ui| {
                            if ui.button(tr("Löschen…", "Delete…")).clicked() {
                                delete = Some(name.clone());
                                ui.close();
                            }
                        });
                    }
                });
            if let Some(name) = open {
                if name != current {
                    self.open_project(&name);
                }
            }
            if let Some(name) = delete {
                self.open_dialog(Dialog::DeleteProject {
                    name,
                    exports: false,
                });
            }
        });
    }

    pub fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog = dialog;
        self.dialog_error = None;
    }

    /// Öffnet den Import-Dialog, auf Wunsch gleich mit einer Datei.
    pub fn import_dialog(&mut self, path: Option<std::path::PathBuf>) {
        let Some(doc) = &self.doc else {
            self.error(tr(
                "Lege erst ein Projekt an, dann lassen sich Bilder importieren",
                "Create a project first, then images can be imported",
            ));
            return;
        };
        // Vorauswahl für Animationen: die Figur, an der gerade gearbeitet wird.
        let character = self.sel.and_then(|id| doc.character_of(id));
        self.open_dialog(Dialog::Import(Box::new(ImportForm::new(path, character))));
    }

    /// Eine Bilddatei, die ins Fenster gezogen wurde, öffnet den Import-Dialog.
    fn dropped_files(&mut self, ctx: &egui::Context) {
        if !matches!(self.dialog, Dialog::None) || self.stroke.is_some() {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.import_dialog(Some(path));
        }
    }

    fn new_animation_dialog(&mut self, character: Option<uuid::Uuid>) {
        let Some(doc) = &self.doc else { return };
        // Vorauswahl: die Figur, an der gerade gearbeitet wird.
        let character = character
            .or_else(|| self.sel.and_then(|id| doc.character_of(id)))
            .or_else(|| doc.project().characters.first().map(|c| c.id));
        if let Some(character) = character {
            self.open_dialog(Dialog::NewAnimation(NewAnimation::new(character)));
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let color = if self.status_is_error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.colored_label(color, &self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(s) = self.sprite() {
                    let mut text = trf!(
                        "{}×{} · {} Bit",
                        "{}×{} · {} bit",
                        s.width(),
                        s.height(),
                        s.depth.bits()
                    );
                    if let Some((x, y)) = self.hover_cell {
                        text = format!("x {x}, y {y} · {text}");
                    }
                    ui.label(text);
                }
            });
        });
    }

    // ------------------------------------------------------------ Assets

    fn asset_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);
        if self.doc.is_none() {
            ui.label(tr("Noch kein Projekt geöffnet.", "No project open yet."));
            ui.add_space(4.0);
            if ui.button(tr("Neues Projekt…", "New project…")).clicked() {
                self.open_dialog(Dialog::NewProject {
                    name: String::new(),
                });
            }
            return;
        }
        let assets = self.doc.as_ref().map(|d| d.assets()).unwrap_or_default();
        let has_character = assets.iter().any(|a| a.id.kind == AssetKind::Character);

        // Anlegen steht unten, die Liste bekommt den Platz darüber.
        egui::Panel::bottom("asset_add")
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui.add_space(6.0);
                let width = ui.available_width();
                let wide = |text: &str| egui::Button::new(text).min_size(egui::vec2(width, 0.0));
                if ui.add(wide(tr("+ Figur", "+ Character"))).clicked() {
                    self.open_dialog(Dialog::NewSprite(NewSprite::new(true)));
                }
                if has_character
                    && ui
                        .add(wide("+ Animation"))
                        .on_hover_text(tr(
                            "Neue Animation für eine Figur",
                            "New animation for a character",
                        ))
                        .clicked()
                {
                    self.new_animation_dialog(None);
                }
                if ui.add(wide(tr("+ Bild", "+ Image"))).clicked() {
                    self.open_dialog(Dialog::NewSprite(NewSprite::new(false)));
                }
                if ui
                    .add(wide(tr("Bild importieren…", "Import image…")))
                    .clicked()
                {
                    self.import_dialog(None);
                }
                ui.add_space(6.0);
            });

        egui::ScrollArea::vertical().show(ui, |ui| {
            if assets.is_empty() {
                ui.weak(tr(
                    "Das Projekt ist noch leer. Lege unten eine Figur oder ein Bild an, oder ziehe eine Bilddatei ins Fenster.",
                    "The project is still empty. Add a character or an image below, or drag an image file into the window.",
                ));
                ui.add_space(4.0);
                ui.weak(tr(
                    "Eine Figur hat eine feste Größe und Palette, damit sie in jeder Animation gleich aussieht. Bilder sind frei: Tiles, Items, Szenen.",
                    "A character has a fixed size and palette so it looks the same in every animation. Images are free: tiles, items, scenes.",
                ));
            }
            let characters: Vec<&AssetEntry> = assets
                .iter()
                .filter(|a| a.id.kind == AssetKind::Character)
                .collect();
            if !characters.is_empty() {
                section(ui, tr("Figuren", "Characters"));
            }
            for c in &characters {
                self.asset_row(ui, c, false);
                for a in assets
                    .iter()
                    .filter(|a| a.id.kind == AssetKind::Animation && a.character == Some(c.id.id))
                {
                    self.asset_row(ui, a, true);
                }
            }
            // Animationen, deren Figur fehlt (sollte nicht vorkommen, darf aber nicht verschwinden).
            for a in assets.iter().filter(|a| {
                a.id.kind == AssetKind::Animation
                    && !characters.iter().any(|c| Some(c.id.id) == a.character)
            }) {
                self.asset_row(ui, a, false);
            }
            let images: Vec<&AssetEntry> = assets
                .iter()
                .filter(|a| a.id.kind == AssetKind::Image)
                .collect();
            if !images.is_empty() {
                ui.add_space(6.0);
                section(ui, tr("Bilder", "Images"));
            }
            for i in images {
                self.asset_row(ui, i, false);
            }
        });
    }

    fn asset_row(&mut self, ui: &mut egui::Ui, a: &AssetEntry, indent: bool) {
        ui.horizontal(|ui| {
            if indent {
                ui.add_space(16.0);
            }
            let selected = self.sel == Some(a.id);
            let frames = if a.frames == 1 {
                "1 Frame".to_string()
            } else {
                format!("{} Frames", a.frames)
            };
            let width = ui.available_width();
            // Rechts die Zahl der Frames bei Animationen; das schiebt den Namen nach links.
            let count = if a.id.kind == AssetKind::Animation {
                a.frames.to_string()
            } else {
                String::new()
            };
            let button = egui::Button::selectable(selected, &a.name)
                .right_text(RichText::new(count).weak().small())
                .min_size(egui::vec2(width, 26.0))
                .truncate();
            let resp = ui.add(button).on_hover_text(trf!(
                "{} · {}×{} · {} Bit · {frames}",
                "{} · {}×{} · {} bit · {frames}",
                a.id.kind.label(),
                a.width,
                a.height,
                a.bits
            ));
            if resp.clicked() {
                self.select(a.id);
            }
            resp.context_menu(|ui| {
                if a.id.kind == AssetKind::Character
                    && ui.button(tr("Neue Animation…", "New animation…")).clicked()
                {
                    self.new_animation_dialog(Some(a.id.id));
                    ui.close();
                }
                if ui.button(tr("Kontur…", "Outline…")).clicked() {
                    self.select(a.id);
                    self.outline_dialog();
                    ui.close();
                }
                if ui.button(tr("Umbenennen…", "Rename…")).clicked() {
                    self.open_dialog(Dialog::Rename {
                        id: a.id,
                        name: a.name.clone(),
                    });
                    ui.close();
                }
                if ui.button(tr("Exportieren…", "Export…")).clicked() {
                    self.select(a.id);
                    self.open_dialog(Dialog::Export(ExportForm::default()));
                    ui.close();
                }
                if ui.button(tr("Löschen…", "Delete…")).clicked() {
                    self.open_dialog(Dialog::Delete(a.id));
                    ui.close();
                }
            });
        });
    }

    /// Öffnet den Kontur-Dialog für das ausgewählte Asset.
    /// Liest die eigenen Formen neu ein und wählt auf Wunsch eine aus.
    pub fn reload_shapes(&mut self, select: Option<&str>) {
        self.custom_shapes = self.ws.custom_shapes();
        let key = |n: &str| pixel_core::workspace::name_key(n);
        self.brush.shape = select
            .and_then(|name| {
                self.custom_shapes
                    .iter()
                    .position(|c| key(&c.name) == key(name))
            })
            .map_or(
                ShapeChoice::Builtin(pixel_core::shapes::Shape::Heart),
                ShapeChoice::Custom,
            );
    }

    pub fn delete_custom_shape(&mut self) {
        let ShapeChoice::Custom(i) = self.brush.shape else {
            return;
        };
        let Some(name) = self.custom_shapes.get(i).map(|c| c.name.clone()) else {
            return;
        };
        match self.ws.delete_shape(&name) {
            Ok(_) => {
                self.reload_shapes(None);
                self.info(trf!("Form „{name}“ gelöscht", "Shape \"{name}\" deleted"));
            }
            Err(err) => self.error(err.to_string()),
        }
    }

    pub fn outline_dialog(&mut self) {
        let Some(id) = self.sel else { return };
        if let Some(form) = OutlineForm::new(self, id) {
            self.open_dialog(Dialog::Outline(Box::new(form)));
        }
    }

    // ------------------------------------------------------------ Farbe, Palette, Ebenen

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let Some(id) = self.sel else {
            ui.weak(tr("Kein Bild ausgewählt.", "No image selected."));
            return;
        };
        let Some(s) = self.sprite() else { return };
        let palette = s.palette.clone();
        let depth = s.depth;
        let character = self.doc.as_ref().and_then(|d| d.character_of(id));

        // Die Knöpfe der Ebenen stehen unten, die Liste bekommt den Platz darüber.
        egui::Panel::bottom("layer_buttons")
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.layer_buttons(ui));

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.strong(tr("Farbe", "Color"));
            ui.horizontal(|ui| {
                let mut rgba = self.brush.color.to_array();
                if ui
                    .color_edit_button_srgba_unmultiplied(&mut rgba)
                    .changed()
                {
                    self.brush.color = Rgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
                }
                ui.monospace(self.brush.color.to_string());
            });
            // Was wirklich gezeichnet wird, hängt von Farbtiefe und Palette ab.
            let wanted = self.brush.color;
            let exact = depth.conform(wanted);
            if palette.locked {
                if !palette.contains(exact) && !exact.is_transparent() {
                    ui.weak(tr("Nicht in der Palette der Figur. Gezeichnet wird die ähnlichste Palettenfarbe.", "Not in the character's palette. The most similar palette color is drawn."));
                    if let Some(character) = character {
                        if ui.button(tr("In die Palette der Figur aufnehmen", "Add to the character's palette")).clicked() {
                            let result = self
                                .doc
                                .as_mut()
                                .map(|d| d.add_character_color(character, exact));
                            if let Some(Ok(())) = result {
                                self.brush.color = exact;
                                self.info(tr("Farbe in die Palette der Figur aufgenommen", "Color added to the character's palette"));
                            } else if let Some(Err(err)) = result {
                                self.error(err);
                            }
                        }
                    }
                }
            } else if exact != wanted {
                ui.weak(trf!(
                    "Bei {} Bit wird daraus {exact}.", "At {} bit this becomes {exact}.",
                    depth.bits()
                ));
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.strong("Palette");
                ui.weak(trf!("{} Farben", "{} colors", palette.len()));
                if palette.locked {
                    ui.weak(tr("· fest", "· fixed"))
                        .on_hover_text(tr("Die Palette gehört zu einer Figur und kann nur wachsen. So bleibt die Figur überall gleich.", "The palette belongs to a character and can only grow. That keeps the character the same everywhere."));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                for &c in &palette.colors {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                    ui.painter().rect_filled(rect, 2.0, c32(c));
                    if c == self.brush.color {
                        ui.painter().rect_stroke(
                            rect,
                            2.0,
                            egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
                            StrokeKind::Outside,
                        );
                    }
                    if resp.on_hover_text(c.to_string()).clicked() {
                        self.brush.color = c;
                        if self.brush.tool == Tool::Eraser || self.brush.tool == Tool::Picker {
                            self.brush.tool = Tool::Pencil;
                        }
                    }
                }
            });
            if !palette.locked
                && !palette.contains(exact)
                && !exact.is_transparent()
                && !palette.is_full()
                && ui.button(tr("+ aktuelle Farbe", "+ current color")).clicked()
            {
                self.edit(|s| s.palette.add(exact).map(|_| ()).map_err(|e| e.to_string()));
            }

            ui.add_space(8.0);
            self.layer_list(ui);
        });
    }

    fn layer_list(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.sprite() else { return };
        let auto = s.outline_layer();
        let layers: Vec<(String, bool)> = s
            .frame(self.frame)
            .map(|f| {
                f.layers
                    .iter()
                    .map(|l| (l.name.clone(), l.visible))
                    .collect()
            })
            .unwrap_or_default();
        self.update_layer_thumbs(ui.ctx());
        ui.strong(tr("Ebenen", "Layers"));
        ui.add_space(4.0);
        // Die oberste Ebene steht oben, wie sie auch gezeichnet wird.
        for (i, (name, visible)) in layers.iter().enumerate().rev() {
            let width = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, LAYER_ROW), Sense::click());
            let selected = self.layer == i;
            let visuals = ui.visuals().clone();
            let fill = if selected {
                visuals.selection.bg_fill
            } else if resp.hovered() {
                visuals.widgets.hovered.bg_fill
            } else {
                visuals.faint_bg_color
            };
            ui.painter().rect_filled(rect, 4.0, fill);

            let check = egui::Rect::from_center_size(
                egui::pos2(rect.min.x + 14.0, rect.center().y),
                egui::vec2(18.0, 18.0),
            );
            let mut v = *visible;
            if ui
                .put(check, egui::Checkbox::without_text(&mut v))
                .on_hover_text(tr("Sichtbar", "Visible"))
                .changed()
            {
                self.edit(|s| {
                    for f in 0..s.frames().len() {
                        s.layer_mut(f, i).map_err(|e| e.to_string())?.visible = v;
                    }
                    Ok(())
                });
            }

            let thumb = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + 28.0, rect.center().y - LAYER_THUMB / 2.0),
                egui::vec2(LAYER_THUMB, LAYER_THUMB),
            );
            let painter = ui.painter();
            painter.rect_filled(thumb, 2.0, Color32::from_gray(200));
            let tex = self
                .textures
                .layers
                .as_ref()
                .and_then(|(_, t)| t.get(i))
                .map(|t| (t.id(), t.size_vec2()));
            if let Some((tex, size)) = tex {
                let scale = (LAYER_THUMB / size.x).min(LAYER_THUMB / size.y);
                let img = egui::Rect::from_center_size(thumb.center(), size * scale);
                painter.image(tex, img, UV, Color32::WHITE);
            }

            let text_color = if selected {
                visuals.selection.stroke.color
            } else {
                visuals.text_color()
            };
            let x = thumb.max.x + 8.0;
            let is_auto = auto == Some(i);
            let y = if is_auto {
                rect.center().y - 7.0
            } else {
                rect.center().y
            };
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(14.0),
                text_color,
            );
            if is_auto {
                painter.text(
                    egui::pos2(x, rect.center().y + 8.0),
                    egui::Align2::LEFT_CENTER,
                    tr("automatische Kontur", "automatic outline"),
                    egui::FontId::proportional(11.0),
                    visuals.weak_text_color(),
                );
            }
            if resp.clicked() {
                self.layer = i;
            }
            if resp.double_clicked() {
                self.open_dialog(Dialog::RenameLayer {
                    layer: i,
                    name: name.clone(),
                });
            }
            ui.add_space(2.0);
        }
    }

    fn layer_buttons(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.sprite() else { return };
        let count = s.layer_count();
        let auto = s.outline_layer();
        let layer = self.layer;
        let names = s.layer_names();
        let opacity = s.layer(self.frame, layer).map_or(255, |l| l.opacity);
        ui.add_space(6.0);
        ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);
        let width = ui.available_width();
        let gap = ui.spacing().item_spacing.x;
        let half = (width - gap) / 2.0;

        ui.horizontal(|ui| {
            ui.label(tr("Deckkraft", "Opacity"));
            let mut percent = self.opacity_edit.unwrap_or(u32::from(opacity) * 100 / 255);
            let resp = ui.add(egui::Slider::new(&mut percent, 0..=100).suffix(" %"));
            if resp.dragged() {
                self.opacity_edit = Some(percent);
            } else if resp.changed() || resp.drag_stopped() {
                self.opacity_edit = None;
                let value = (percent * 255 / 100) as u8;
                self.edit(|s| {
                    for f in 0..s.frames().len() {
                        s.layer_mut(f, layer).map_err(|e| e.to_string())?.opacity = value;
                    }
                    Ok(())
                });
            }
        });
        ui.add_space(2.0);

        // Die automatische Kontur bleibt ganz oben.
        let top = auto.unwrap_or(count);
        ui.horizontal(|ui| {
            let up = egui::Button::new(tr("Nach oben", "Move up")).min_size(egui::vec2(half, 0.0));
            if ui
                .add_enabled(auto != Some(layer) && layer + 1 < top, up)
                .clicked()
            {
                self.move_layer(layer, layer + 1);
            }
            let down =
                egui::Button::new(tr("Nach unten", "Move down")).min_size(egui::vec2(half, 0.0));
            if ui
                .add_enabled(auto != Some(layer) && layer > 0, down)
                .clicked()
            {
                self.move_layer(layer, layer - 1);
            }
        });
        ui.horizontal(|ui| {
            let rename =
                egui::Button::new(tr("Umbenennen", "Rename")).min_size(egui::vec2(half, 0.0));
            if ui.add(rename).clicked() {
                if let Some(name) = names.get(layer) {
                    self.open_dialog(Dialog::RenameLayer {
                        layer,
                        name: name.clone(),
                    });
                }
            }
            let delete = egui::Button::new(tr("Löschen", "Delete")).min_size(egui::vec2(half, 0.0));
            if ui.add_enabled(count > 1, delete).clicked() {
                self.edit(|s| s.remove_layer(layer).map_err(|e| e.to_string()));
            }
        });
        let add = egui::Button::new(tr("+ Ebene", "+ Layer")).min_size(egui::vec2(width, 0.0));
        if ui.add_enabled(count < MAX_LAYERS, add).clicked() {
            // Der Name wird erst in der Änderung gewählt, also nach dem Abgleich mit
            // der Datei. Der MCP-Server findet Ebenen über ihren Namen.
            if let Some(i) = self.edit(|s| {
                let name = (s.layer_count() + 1..)
                    .map(|n| trf!("Ebene {n}", "Layer {n}"))
                    .find(|name| s.layer_index(name).is_none())
                    .unwrap_or_default();
                let i = s.add_layer(name);
                match s.outline_layer() {
                    Some(top) if top + 1 == i => {
                        s.move_layer(i, top).map_err(|e| e.to_string())?;
                        Ok(top)
                    }
                    _ => Ok(i),
                }
            }) {
                self.layer = i;
            }
        }
        ui.add_space(6.0);
    }

    fn move_layer(&mut self, from: usize, to: usize) {
        if self
            .edit(|s| s.move_layer(from, to).map_err(|e| e.to_string()))
            .is_some()
        {
            self.layer = to;
        }
    }

    /// Kleine Vorschaubilder der Ebenen im aktuellen Frame.
    fn update_layer_thumbs(&mut self, ctx: &egui::Context) {
        let (Some(doc), Some(id)) = (&self.doc, self.sel) else {
            self.textures.layers = None;
            return;
        };
        let key = (doc.revision(), id, self.frame);
        let fresh = self
            .textures
            .layers
            .as_ref()
            .is_some_and(|(k, _)| *k == key);
        if fresh || (self.stroke.is_some() && self.textures.layers.is_some()) {
            return;
        }
        let Some(s) = doc.sprite(id) else { return };
        let Ok(frame) = s.frame(self.frame) else {
            return;
        };
        if s.width() as usize * s.height() as usize * frame.layers.len() > THUMB_BUDGET {
            self.textures.layers = Some((key, Vec::new()));
            return;
        }
        let size = [s.width() as usize, s.height() as usize];
        let thumbs = frame
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let img = egui::ColorImage::new(size, l.pixels().iter().map(|&c| c32(c)).collect());
                ctx.load_texture(format!("layer-{i}"), img, egui::TextureOptions::NEAREST)
            })
            .collect();
        self.textures.layers = Some((key, thumbs));
    }

    // ------------------------------------------------------------ Timeline

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.sprite() else {
            ui.weak(tr(
                "Wähle links ein Bild, eine Figur oder eine Animation.",
                "Choose an image, a character or an animation on the left.",
            ));
            return;
        };
        let durations: Vec<u32> = s.frames().iter().map(|f| f.duration_ms).collect();
        let n = durations.len();
        self.update_thumbs(ui.ctx());
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let label = if self.playing {
                "Pause"
            } else {
                tr("Abspielen", "Play")
            };
            if ui
                .add_enabled(n > 1, egui::Button::new(label))
                .on_hover_text(tr("Eingabetaste", "Enter"))
                .clicked()
            {
                self.toggle_play();
            }
            ui.label(trf!("Frame {} von {n}", "Frame {} of {n}", self.frame + 1));
            if let (Some(mut looping), Some(id)) = (self.looping(), self.sel) {
                if ui
                    .checkbox(&mut looping, tr("Schleife", "Loop"))
                    .on_hover_text(tr(
                        "Ob die Animation nach dem letzten Frame von vorn beginnt",
                        "Whether the animation starts over after the last frame",
                    ))
                    .changed()
                {
                    let result = self.doc.as_mut().map(|d| d.set_looping(id.id, looping));
                    if let Some(result) = result {
                        self.check(result);
                    }
                }
            }
            ui.separator();
            if ui
                .add_enabled(n < MAX_FRAMES, egui::Button::new("+ Frame"))
                .on_hover_text(tr("Leerer Frame am Ende", "Empty frame at the end"))
                .clicked()
            {
                if let Some(i) = self.edit(|s| Ok(s.add_frame())) {
                    self.frame = i;
                }
            }
            if ui
                .add_enabled(
                    n < MAX_FRAMES,
                    egui::Button::new(tr("Duplizieren", "Duplicate")),
                )
                .on_hover_text(tr(
                    "Kopie des aktuellen Frames direkt dahinter",
                    "Copy of the current frame right after it",
                ))
                .clicked()
            {
                let f = self.frame;
                if let Some(i) = self.edit(|s| s.duplicate_frame(f).map_err(|e| e.to_string())) {
                    self.frame = i;
                }
            }
            if ui
                .add_enabled(n > 1, egui::Button::new(tr("Löschen", "Delete")))
                .clicked()
            {
                let f = self.frame;
                self.edit(|s| s.remove_frame(f).map_err(|e| e.to_string()));
            }
            ui.separator();
            // Die Dauer wird erst übernommen, wenn das Ziehen oder Tippen fertig ist,
            // damit nicht jeder Zwischenwert ein eigener Rückgängig-Schritt wird.
            let current = durations.get(self.frame).copied().unwrap_or(100);
            let mut value = self.duration_edit.unwrap_or(current);
            let resp = ui.add(
                egui::DragValue::new(&mut value)
                    .range(10..=10_000)
                    .suffix(" ms")
                    .prefix(tr("Dauer ", "Duration ")),
            );
            if resp.changed() {
                self.duration_edit = Some(value);
            }
            if !resp.dragged() && !resp.has_focus() {
                if let Some(value) = self.duration_edit.take() {
                    if value != current {
                        let f = self.frame;
                        self.edit(|s| {
                            s.frame_mut(f).map_err(|e| e.to_string())?.duration_ms = value;
                            Ok(())
                        });
                    }
                }
            }
            if ui
                .add_enabled(n > 1, egui::Button::new(tr("für alle", "for all")))
                .on_hover_text(tr(
                    "Diese Dauer für alle Frames übernehmen",
                    "Use this duration for all frames",
                ))
                .clicked()
            {
                self.edit(|s| {
                    for f in 0..s.frames().len() {
                        s.frame_mut(f).map_err(|e| e.to_string())?.duration_ms = current;
                    }
                    Ok(())
                });
            }
        });
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, ms) in durations.iter().enumerate() {
                    let thumb = self
                        .textures
                        .thumbs
                        .as_ref()
                        .and_then(|(_, t)| t.get(i))
                        .map(|t| (t.id(), t.size_vec2()));
                    let size = egui::vec2(THUMB_SIZE + 8.0, THUMB_SIZE + 22.0);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                    let selected = i == self.frame;
                    let visuals = ui.visuals();
                    let fill = if selected {
                        visuals.selection.bg_fill
                    } else if resp.hovered() {
                        visuals.widgets.hovered.bg_fill
                    } else {
                        visuals.extreme_bg_color
                    };
                    let painter = ui.painter();
                    painter.rect_filled(rect, 4.0, fill);
                    let area = egui::Rect::from_min_size(
                        rect.min + egui::vec2(4.0, 4.0),
                        egui::vec2(THUMB_SIZE, THUMB_SIZE),
                    );
                    if let Some((tex, tex_size)) = thumb {
                        let scale = (THUMB_SIZE / tex_size.x).min(THUMB_SIZE / tex_size.y);
                        let img = egui::Rect::from_center_size(area.center(), tex_size * scale);
                        painter.rect_filled(img, 0.0, Color32::from_gray(200));
                        painter.image(
                            tex,
                            img,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    painter.text(
                        egui::pos2(rect.center().x, rect.max.y - 9.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{} · {ms} ms", i + 1),
                        egui::FontId::proportional(10.0),
                        visuals.text_color(),
                    );
                    if resp.clicked() {
                        self.frame = i;
                        self.playing = false;
                    }
                }
            });
        });
        ui.add_space(2.0);
    }

    /// Erneuert die Vorschaubilder der Timeline, wenn sich etwas geändert hat.
    fn update_thumbs(&mut self, ctx: &egui::Context) {
        let (Some(doc), Some(id)) = (&self.doc, self.sel) else {
            self.textures.thumbs = None;
            return;
        };
        let key = (doc.revision(), id);
        let fresh = self
            .textures
            .thumbs
            .as_ref()
            .is_some_and(|(k, _)| *k == key);
        // Während eines Strichs warten, sonst würde jeder Mausschritt alle Frames neu rechnen.
        if fresh || (self.stroke.is_some() && self.textures.thumbs.is_some()) {
            return;
        }
        let Some(s) = doc.sprite(id) else { return };
        let pixels =
            s.width() as usize * s.height() as usize * s.frames().len() * s.layer_count().max(1);
        if pixels > THUMB_BUDGET {
            self.textures.thumbs = Some((key, Vec::new()));
            return;
        }
        let thumbs = (0..s.frames().len())
            .filter_map(|f| {
                let img = canvas::sprite_image(s, f)?;
                Some(ctx.load_texture(format!("thumb-{f}"), img, egui::TextureOptions::NEAREST))
            })
            .collect();
        self.textures.thumbs = Some((key, thumbs));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        self.shortcuts(&ctx);
        self.dropped_files(&ctx);
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("tools").show(ui, |ui| self.tool_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("timeline").show(ui, |ui| self.timeline(ui));
        egui::Panel::left("assets")
            .resizable(true)
            .default_size(210.0)
            .show(ui, |ui| self.asset_panel(ui));
        egui::Panel::right("side")
            .resizable(true)
            .default_size(230.0)
            .show(ui, |ui| self.side_panel(ui));
        let background = ui.visuals().extreme_bg_color;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(background))
            .show(ui, |ui| canvas::show(self, ui));
        dialogs::show(self, &ctx);
        self.take_shot(&ctx);
    }
}

/// Text mit etwas Abstand, für Abschnitte in Dialogen.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong());
}
