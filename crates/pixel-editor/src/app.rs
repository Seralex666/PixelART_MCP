//! Das Hauptfenster: Zustand des Editors und die Leisten rund um die Leinwand.

use crate::canvas;
use crate::dialogs::{self, Dialog, ExportForm, NewAnimation, NewSprite};
use crate::doc::{AssetEntry, AssetId, AssetKind, Doc, Res};
use crate::help;
use crate::tools::{Brush, Stroke, Tool, MAX_BRUSH};
use eframe::egui::{self, Color32, Key, RichText, Sense, StrokeKind};
use pixel_core::draw::{self, Target};
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
    shot: Option<Shot>,
}

/// Bildschirmfoto für automatische Prüfungen (`PIXELART_SHOT=<datei.png>`).
struct Shot {
    path: std::path::PathBuf,
    frames: u32,
    requested: bool,
    done: Arc<AtomicBool>,
}

impl App {
    pub fn new(ws: Workspace, project: Option<String>) -> Self {
        let mut app = Self {
            projects: ws.project_names(),
            ws,
            doc: None,
            sel: None,
            frame: 0,
            layer: 0,
            brush: Brush {
                tool: Tool::Pencil,
                color: Rgba::BLACK,
                size: 1,
                filled: false,
            },
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
            shot: std::env::var_os("PIXELART_SHOT").map(|p| Shot {
                path: p.into(),
                frames: 0,
                requested: false,
                done: Arc::new(AtomicBool::new(false)),
            }),
        };
        // Das gewünschte, sonst das zuletzt bearbeitete Projekt öffnen.
        let latest = project.or_else(|| {
            app.projects
                .iter()
                .max_by_key(|name| app.ws.modified(name))
                .cloned()
        });
        if let Some(name) = latest {
            app.open_project(&name);
        }
        match std::env::var("PIXELART_SHOT_DIALOG").as_deref() {
            Ok("connect") => app.dialog = Dialog::Connect,
            Ok("export") => app.dialog = Dialog::Export(ExportForm::default()),
            Ok("character") => app.dialog = Dialog::NewSprite(NewSprite::new(true)),
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
                self.stroke = None;
                self.playing = false;
                self.sel = None;
                self.fix_selection();
                self.fit_color();
                self.info(format!("Projekt „{name}“ geöffnet"));
            }
            Err(err) => self.error(format!("Projekt „{name}“ lässt sich nicht öffnen: {err}")),
        }
    }

    pub fn select(&mut self, id: AssetId) {
        if self.sel != Some(id) {
            self.sel = Some(id);
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

    fn target(&self) -> Target {
        Target::new(self.frame, self.layer)
    }

    pub fn undo(&mut self, redo: bool) {
        let Some(doc) = &mut self.doc else { return };
        let result = if redo { doc.redo() } else { doc.undo() };
        match result {
            Ok(Some(id)) => {
                self.select(id);
                self.info(if redo { "Wiederholt" } else { "Rückgängig" });
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

    // ------------------------------------------------------------ Takt

    fn tick(&mut self, ctx: &egui::Context) {
        let (now, dt) = ctx.input(|i| (i.time, i.stable_dt));
        if self.stroke.is_none() && now - self.last_poll >= POLL_SECONDS {
            self.last_poll = now;
            self.projects = self.ws.project_names();
            if let Some(doc) = &mut self.doc {
                if doc.poll(self.sel) {
                    self.fix_selection();
                    self.info("Von außen geändert (zum Beispiel durch Claude), neu geladen");
                }
            }
        }
        ctx.request_repaint_after_secs(POLL_SECONDS as f32);
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
        let (command, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        if command {
            if pressed(Key::Z) {
                self.undo(shift);
            } else if pressed(Key::Y) {
                self.undo(true);
            } else if pressed(Key::E) && self.sel.is_some() {
                self.dialog = Dialog::Export(ExportForm::default());
            }
            return;
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
            ui.menu_button("Datei", |ui| {
                if ui.button("Neues Projekt…").clicked() {
                    self.open_dialog(Dialog::NewProject {
                        name: String::new(),
                    });
                }
                ui.separator();
                if ui
                    .add_enabled(has_doc, egui::Button::new("Neue Figur…"))
                    .clicked()
                {
                    self.open_dialog(Dialog::NewSprite(NewSprite::new(true)));
                }
                if ui
                    .add_enabled(has_character, egui::Button::new("Neue Animation…"))
                    .clicked()
                {
                    self.new_animation_dialog(None);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new("Neues Bild…"))
                    .clicked()
                {
                    self.open_dialog(Dialog::NewSprite(NewSprite::new(false)));
                }
                ui.separator();
                if ui
                    .add_enabled(has_sel, egui::Button::new("Exportieren…  Strg+E"))
                    .clicked()
                {
                    self.open_dialog(Dialog::Export(ExportForm::default()));
                }
                if ui.button("Arbeitsordner öffnen").clicked() {
                    help::open_folder(self.ws.home());
                }
                ui.separator();
                if ui.button("Beenden").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Bearbeiten", |ui| {
                let (can_undo, can_redo) = self
                    .doc
                    .as_ref()
                    .map_or((false, false), |d| (d.can_undo(), d.can_redo()));
                if ui
                    .add_enabled(can_undo, egui::Button::new("Rückgängig  Strg+Z"))
                    .clicked()
                {
                    self.undo(false);
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("Wiederholen  Strg+Y"))
                    .clicked()
                {
                    self.undo(true);
                }
                ui.separator();
                if ui
                    .add_enabled(has_sel, egui::Button::new("Ebene spiegeln"))
                    .clicked()
                {
                    let t = self.target();
                    self.edit(|s| draw::flip_horizontal(s, t).map_err(|e| e.to_string()));
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("Ebene leeren"))
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
            ui.menu_button("Ansicht", |ui| {
                ui.checkbox(&mut self.grid, "Raster");
                ui.checkbox(&mut self.onion, "Zwiebelhaut (vorheriger Frame)");
                ui.separator();
                if ui.button("Vergrößern  +").clicked() {
                    self.set_zoom(1);
                }
                if ui.button("Verkleinern  −").clicked() {
                    self.set_zoom(-1);
                }
                if ui.button("Einpassen  0").clicked() {
                    self.view.fit = true;
                }
            });
            ui.menu_button("Hilfe", |ui| {
                if ui.button("Mit Claude verbinden…").clicked() {
                    self.open_dialog(Dialog::Connect);
                }
                if ui.button("Tastenkürzel").clicked() {
                    self.open_dialog(Dialog::Shortcuts);
                }
                if ui.button("Über PixelArt Studio").clicked() {
                    self.open_dialog(Dialog::About);
                }
            });

            ui.separator();
            ui.label("Projekt:");
            let current = self
                .doc
                .as_ref()
                .map_or("(keins)".to_string(), |d| d.name().to_string());
            let mut open = None;
            egui::ComboBox::from_id_salt("project")
                .selected_text(current.clone())
                .show_ui(ui, |ui| {
                    for name in &self.projects {
                        if ui.selectable_label(*name == current, name).clicked() {
                            open = Some(name.clone());
                        }
                    }
                });
            if let Some(name) = open {
                if name != current {
                    self.open_project(&name);
                }
            }
        });
    }

    pub fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog = dialog;
        self.dialog_error = None;
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

    fn tool_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in Tool::ALL {
                ui.selectable_value(&mut self.brush.tool, tool, tool.label())
                    .on_hover_text(format!("Taste {}", tool.key()));
            }
            ui.separator();
            ui.add_enabled(
                self.brush.tool.uses_size(),
                egui::DragValue::new(&mut self.brush.size)
                    .range(1..=MAX_BRUSH)
                    .prefix("Größe "),
            );
            ui.add_enabled(
                self.brush.tool.uses_fill(),
                egui::Checkbox::new(&mut self.brush.filled, "Gefüllt"),
            );
            ui.separator();
            if ui.button("−").on_hover_text("Verkleinern").clicked() {
                self.set_zoom(-1);
            }
            ui.label(format!("{}×", self.view.zoom));
            if ui.button("+").on_hover_text("Vergrößern").clicked() {
                self.set_zoom(1);
            }
            if ui.button("Einpassen").clicked() {
                self.view.fit = true;
            }
            ui.checkbox(&mut self.grid, "Raster");
            ui.checkbox(&mut self.onion, "Zwiebelhaut")
                .on_hover_text("Zeigt den vorherigen Frame blass unter dem aktuellen");
        });
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
                    let mut text = format!("{}×{} · {} Bit", s.width(), s.height(), s.depth.bits());
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
        if self.doc.is_none() {
            ui.label("Noch kein Projekt geöffnet.");
            if ui.button("Neues Projekt…").clicked() {
                self.open_dialog(Dialog::NewProject {
                    name: String::new(),
                });
            }
            return;
        }
        let assets = self.doc.as_ref().map(|d| d.assets()).unwrap_or_default();
        let has_character = assets.iter().any(|a| a.id.kind == AssetKind::Character);
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Figur").clicked() {
                self.open_dialog(Dialog::NewSprite(NewSprite::new(true)));
            }
            if ui
                .add_enabled(has_character, egui::Button::new("+ Animation"))
                .clicked()
            {
                self.new_animation_dialog(None);
            }
            if ui.button("+ Bild").clicked() {
                self.open_dialog(Dialog::NewSprite(NewSprite::new(false)));
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.strong("Figuren");
            let characters: Vec<&AssetEntry> = assets
                .iter()
                .filter(|a| a.id.kind == AssetKind::Character)
                .collect();
            if characters.is_empty() {
                ui.weak("Noch keine. Eine Figur hat eine feste Größe und Palette, damit sie in jeder Animation gleich aussieht.");
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
            ui.add_space(8.0);
            ui.strong("Bilder");
            let mut any = false;
            for i in assets.iter().filter(|a| a.id.kind == AssetKind::Image) {
                any = true;
                self.asset_row(ui, i, false);
            }
            if !any {
                ui.weak("Noch keine. Bilder sind frei: Tiles, Items, Szenen.");
            }
        });
    }

    fn asset_row(&mut self, ui: &mut egui::Ui, a: &AssetEntry, indent: bool) {
        ui.horizontal(|ui| {
            if indent {
                ui.add_space(14.0);
            }
            let selected = self.sel == Some(a.id);
            let frames = if a.frames == 1 {
                "1 Frame".to_string()
            } else {
                format!("{} Frames", a.frames)
            };
            let resp = ui
                .selectable_label(selected, &a.name)
                .on_hover_text(format!(
                    "{} · {}×{} · {} Bit · {frames}",
                    a.id.kind.label(),
                    a.width,
                    a.height,
                    a.bits
                ));
            if resp.clicked() {
                self.select(a.id);
            }
            resp.context_menu(|ui| {
                if a.id.kind == AssetKind::Character && ui.button("Neue Animation…").clicked() {
                    self.new_animation_dialog(Some(a.id.id));
                    ui.close();
                }
                if ui.button("Umbenennen…").clicked() {
                    self.open_dialog(Dialog::Rename {
                        id: a.id,
                        name: a.name.clone(),
                    });
                    ui.close();
                }
                if ui.button("Exportieren…").clicked() {
                    self.select(a.id);
                    self.open_dialog(Dialog::Export(ExportForm::default()));
                    ui.close();
                }
                if ui.button("Löschen…").clicked() {
                    self.open_dialog(Dialog::Delete(a.id));
                    ui.close();
                }
            });
        });
    }

    // ------------------------------------------------------------ Farbe, Palette, Ebenen

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let Some(id) = self.sel else {
            ui.weak("Kein Bild ausgewählt.");
            return;
        };
        let Some(s) = self.sprite() else { return };
        let palette = s.palette.clone();
        let depth = s.depth;
        let character = self.doc.as_ref().and_then(|d| d.character_of(id));

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.strong("Farbe");
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
                    ui.weak("Nicht in der Palette der Figur. Gezeichnet wird die ähnlichste Palettenfarbe.");
                    if let Some(character) = character {
                        if ui.button("In die Palette der Figur aufnehmen").clicked() {
                            let result = self
                                .doc
                                .as_mut()
                                .map(|d| d.add_character_color(character, exact));
                            if let Some(Ok(())) = result {
                                self.brush.color = exact;
                                self.info("Farbe in die Palette der Figur aufgenommen");
                            } else if let Some(Err(err)) = result {
                                self.error(err);
                            }
                        }
                    }
                }
            } else if exact != wanted {
                ui.weak(format!(
                    "Bei {} Bit wird daraus {exact}.",
                    depth.bits()
                ));
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.strong("Palette");
                ui.weak(format!("{} Farben", palette.len()));
                if palette.locked {
                    ui.weak("· fest")
                        .on_hover_text("Die Palette gehört zu einer Figur und kann nur wachsen. So bleibt die Figur überall gleich.");
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
                && ui.button("+ aktuelle Farbe").clicked()
            {
                self.edit(|s| s.palette.add(exact).map(|_| ()).map_err(|e| e.to_string()));
            }

            ui.add_space(8.0);
            self.layer_list(ui);
        });
    }

    fn layer_list(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.sprite() else { return };
        let layers: Vec<(String, bool)> = s
            .frame(self.frame)
            .map(|f| {
                f.layers
                    .iter()
                    .map(|l| (l.name.clone(), l.visible))
                    .collect()
            })
            .unwrap_or_default();
        ui.strong("Ebenen");
        // Die oberste Ebene steht oben, wie sie auch gezeichnet wird.
        for (i, (name, visible)) in layers.iter().enumerate().rev() {
            ui.horizontal(|ui| {
                let mut v = *visible;
                if ui.checkbox(&mut v, "").on_hover_text("Sichtbar").changed() {
                    self.edit(|s| {
                        for f in 0..s.frames().len() {
                            s.layer_mut(f, i).map_err(|e| e.to_string())?.visible = v;
                        }
                        Ok(())
                    });
                }
                let resp = ui.selectable_label(self.layer == i, name);
                if resp.clicked() {
                    self.layer = i;
                }
                if resp.double_clicked() {
                    self.open_dialog(Dialog::RenameLayer {
                        layer: i,
                        name: name.clone(),
                    });
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(layers.len() < MAX_LAYERS, egui::Button::new("+ Ebene"))
                .clicked()
            {
                // Der Name wird erst in der Änderung gewählt, also nach dem Abgleich mit
                // der Datei. Der MCP-Server findet Ebenen über ihren Namen.
                if let Some(i) = self.edit(|s| {
                    let name = (s.layer_count() + 1..)
                        .map(|n| format!("Ebene {n}"))
                        .find(|name| s.layer_index(name).is_none())
                        .unwrap_or_default();
                    Ok(s.add_layer(name))
                }) {
                    self.layer = i;
                }
            }
            if ui.button("Umbenennen").clicked() {
                if let Some((name, _)) = layers.get(self.layer) {
                    self.open_dialog(Dialog::RenameLayer {
                        layer: self.layer,
                        name: name.clone(),
                    });
                }
            }
            if ui
                .add_enabled(layers.len() > 1, egui::Button::new("Löschen"))
                .clicked()
            {
                let i = self.layer;
                self.edit(|s| s.remove_layer(i).map_err(|e| e.to_string()));
            }
        });
    }

    // ------------------------------------------------------------ Timeline

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.sprite() else {
            ui.weak("Wähle links ein Bild, eine Figur oder eine Animation.");
            return;
        };
        let durations: Vec<u32> = s.frames().iter().map(|f| f.duration_ms).collect();
        let n = durations.len();
        self.update_thumbs(ui.ctx());
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let label = if self.playing { "Pause" } else { "Abspielen" };
            if ui
                .add_enabled(n > 1, egui::Button::new(label))
                .on_hover_text("Eingabetaste")
                .clicked()
            {
                self.toggle_play();
            }
            ui.label(format!("Frame {} von {n}", self.frame + 1));
            if let (Some(mut looping), Some(id)) = (self.looping(), self.sel) {
                if ui
                    .checkbox(&mut looping, "Schleife")
                    .on_hover_text("Ob die Animation nach dem letzten Frame von vorn beginnt")
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
                .on_hover_text("Leerer Frame am Ende")
                .clicked()
            {
                if let Some(i) = self.edit(|s| Ok(s.add_frame())) {
                    self.frame = i;
                }
            }
            if ui
                .add_enabled(n < MAX_FRAMES, egui::Button::new("Duplizieren"))
                .on_hover_text("Kopie des aktuellen Frames direkt dahinter")
                .clicked()
            {
                let f = self.frame;
                if let Some(i) = self.edit(|s| s.duplicate_frame(f).map_err(|e| e.to_string())) {
                    self.frame = i;
                }
            }
            if ui
                .add_enabled(n > 1, egui::Button::new("Löschen"))
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
                    .prefix("Dauer "),
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
                .add_enabled(n > 1, egui::Button::new("für alle"))
                .on_hover_text("Diese Dauer für alle Frames übernehmen")
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
