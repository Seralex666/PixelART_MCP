//! Dialog „Drehen…“: in jedem Winkel drehen, ohne dass die Pixel verschwimmen
//! (RotSprite), auf Wunsch über alle Frames verteilt als Drehung im Kreis.

use crate::app::{section, App};
use crate::dialogs::Outcome;
use crate::doc::{AssetId, AssetKind};
use crate::player::{self, Player};
use eframe::egui::{self, Color32, RichText};
use pixel_core::lang::tr;
use pixel_core::rotate::RotateOptions;
use pixel_core::trf;

const PREVIEW: f32 = 240.0;

pub struct RotateForm {
    id: AssetId,
    angle: f32,
    all_layers: bool,
    all_frames: bool,
    spin: bool,
    frames: u32,
    expand: bool,
    pivot: Option<(u32, u32)>,
    player: Option<Player>,
}

impl RotateForm {
    pub fn new(app: &App, id: AssetId) -> Option<Self> {
        let s = app.doc.as_ref()?.sprite(id)?;
        Some(Self {
            id,
            angle: 15.0,
            all_layers: true,
            all_frames: true,
            spin: false,
            frames: s.frames().len().max(8) as u32,
            expand: id.kind == AssetKind::Image,
            pivot: None,
            player: None,
        })
    }

    fn options(&self, app: &App) -> RotateOptions {
        let mut o = RotateOptions::new(self.angle);
        o.layer = (!self.all_layers).then_some(app.layer);
        o.frame = (!self.all_frames && !self.spin).then_some(app.frame);
        o.pivot = self.pivot;
        o.expand = self.expand && self.id.kind == AssetKind::Image;
        o.spin = self.spin;
        o.frames =
            (self.spin && self.id.kind != AssetKind::Character).then_some(self.frames as usize);
        o
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, f: &mut RotateForm) -> Outcome {
    let Some(sprite) = app.doc.as_ref().and_then(|d| d.sprite(f.id)).cloned() else {
        return Outcome::Close;
    };
    let image = f.id.kind == AssetKind::Image;
    let character = f.id.kind == AssetKind::Character;
    ui.heading(tr("Drehen", "Rotate"));
    ui.label(tr(
        "Dreht in jedem Winkel, ohne dass Pixel verschwimmen: Es entstehen keine neuen Farben, Linien bleiben geschlossen. Kontur, Licht und Effekte zeichnen sich danach neu.",
        "Rotates by any angle without blurring the pixels: no new colors appear and lines stay closed. Outline, light and effects redraw themselves afterwards.",
    ));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(PREVIEW);
            preview(app, ui, f, &sprite);
            ui.weak(tr(
                "Klick setzt den Drehpunkt, Rechtsklick nimmt wieder die Mitte.",
                "Click sets the pivot, right-click goes back to the center.",
            ));
        });
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.set_width(300.0);
            section(ui, tr("Winkel", "Angle"));
            let toggled = ui.checkbox(
                &mut f.spin,
                tr("Über die Frames verteilen (Drehung im Kreis)", "Spread over the frames (spinning)"),
            )
            .on_hover_text(tr(
                "Frame für Frame ein Stück weiter. Mit 360° dreht sich der Gegenstand nahtlos einmal im Kreis, etwa eine Axt oder Münze.",
                "A little further every frame. With 360° the object turns seamlessly once around, like an axe or a coin.",
            ));
            if toggled.changed() {
                f.angle = if f.spin { 360.0 } else { 15.0 };
            }
            let range = if f.spin { -720.0..=720.0 } else { -180.0..=180.0 };
            ui.add(egui::Slider::new(&mut f.angle, range).suffix("°").step_by(1.0));
            ui.horizontal(|ui| {
                if f.spin {
                    for a in [360.0, -360.0, 180.0] {
                        if ui.button(format!("{a}°")).clicked() {
                            f.angle = a;
                        }
                    }
                } else {
                    for a in [-90.0, -45.0, -15.0, 15.0, 45.0, 90.0, 180.0] {
                        if ui.button(format!("{a}°")).clicked() {
                            f.angle = a;
                        }
                    }
                }
            });
            if f.spin && !character {
                ui.horizontal(|ui| {
                    ui.label(tr("Frames mindestens", "Frames at least"));
                    ui.add(egui::DragValue::new(&mut f.frames).range(2..=64));
                });
            }
            if f.spin && f.frames > 0 {
                let n = if character { sprite.frames().len() as u32 } else { f.frames.max(sprite.frames().len() as u32) };
                ui.weak(trf!(
                    "{:.1}° je Frame bei {n} Frames",
                    "{:.1}° per frame with {n} frames",
                    f.angle / n as f32
                ));
            }
            section(ui, tr("Was", "What"));
            let layer_name = sprite.layer_names().get(app.layer).cloned().unwrap_or_default();
            ui.radio_value(&mut f.all_layers, true, tr("Alle gezeichneten Ebenen", "All drawn layers"));
            ui.radio_value(
                &mut f.all_layers,
                false,
                trf!("Nur Ebene „{layer_name}“", "Only layer \"{layer_name}\""),
            );
            if sprite.frames().len() > 1 && !f.spin {
                ui.radio_value(&mut f.all_frames, true, tr("Alle Frames", "All frames"));
                ui.radio_value(
                    &mut f.all_frames,
                    false,
                    trf!("Nur Frame {}", "Only frame {}", app.frame + 1),
                );
            }
            if image {
                ui.checkbox(
                    &mut f.expand,
                    tr("Leinwand vergrößern, damit nichts abgeschnitten wird", "Grow the canvas so nothing is cut off"),
                );
            } else {
                ui.weak(tr(
                    "Figuren und Animationen behalten ihre Größe.",
                    "Characters and animations keep their size.",
                ));
            }
            ui.label(match f.pivot {
                Some((x, y)) => trf!("Drehpunkt: Pixel {x}, {y}", "Pivot: pixel {x}, {y}"),
                None => tr("Drehpunkt: Bildmitte", "Pivot: center of the image").to_string(),
            });
        });
    });
    ui.add_space(8.0);
    let mut outcome = Outcome::Keep;
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new(tr("Drehen", "Rotate")).strong())
            .clicked()
        {
            let o = f.options(app);
            app.select(f.id);
            if app
                .edit(|s| s.rotate(&o).map_err(|e| e.to_string()))
                .is_some()
            {
                app.info(if f.spin {
                    tr(
                        "Drehung über die Frames verteilt",
                        "Rotation spread over the frames",
                    )
                } else {
                    tr("Gedreht", "Rotated")
                });
                outcome = Outcome::Close;
            }
        }
        if ui.button(tr("Abbrechen", "Cancel")).clicked() {
            outcome = Outcome::Close;
        }
    });
    outcome
}

fn preview(app: &App, ui: &mut egui::Ui, f: &mut RotateForm, sprite: &pixel_core::Sprite) {
    let revision = app.doc.as_ref().map_or(0, |d| d.revision());
    let o = f.options(app);
    let key = player::key(&(revision, f.id, &o, app.frame));
    let shown_frame = app.frame.min(sprite.frames().len().saturating_sub(1));
    Player::ensure(&mut f.player, ui, "rotate-preview", key, || {
        let mut s = sprite.clone();
        s.rotate(&o).ok()?;
        // Ohne Verteilen auf die Frames reicht der aktuelle Frame.
        if !o.spin {
            let keep = o.frame.unwrap_or(shown_frame).min(s.frames().len() - 1);
            while s.frames().len() > keep + 1 {
                s.remove_frame(s.frames().len() - 1).ok()?;
            }
            while s.frames().len() > 1 {
                s.remove_frame(0).ok()?;
            }
        }
        Some(s)
    });
    let (rect, response) = player::frame(ui, egui::vec2(PREVIEW, PREVIEW), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let Some(p) = &f.player else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr("Keine Vorschau", "No preview"),
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
        return;
    };
    let shown = p.paint(ui, &painter, rect);
    // Der Drehpunkt liegt in der Vorschau um den Zuwachs der Leinwand versetzt.
    let (w, h) = (sprite.width() as f32, sprite.height() as f32);
    let grown = (shown.image.width() / shown.scale - w) / 2.0;
    let grown_y = (shown.image.height() / shown.scale - h) / 2.0;
    let (px, py) = match f.pivot {
        Some((x, y)) => (x as f32 + 0.5, y as f32 + 0.5),
        None => (w / 2.0, h / 2.0),
    };
    let at = shown.image.min + egui::vec2(px + grown, py + grown_y) * shown.scale;
    let stroke = egui::Stroke::new(1.5, Color32::from_rgb(255, 200, 80));
    painter.line_segment(
        [at - egui::vec2(6.0, 0.0), at + egui::vec2(6.0, 0.0)],
        stroke,
    );
    painter.line_segment(
        [at - egui::vec2(0.0, 6.0), at + egui::vec2(0.0, 6.0)],
        stroke,
    );
    if response.secondary_clicked() {
        f.pivot = None;
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let v = (pos - shown.image.min) / shown.scale - egui::vec2(grown, grown_y);
            if v.x >= 0.0 && v.y >= 0.0 && v.x < w && v.y < h {
                f.pivot = Some((v.x as u32, v.y as u32));
            }
        }
    }
}
