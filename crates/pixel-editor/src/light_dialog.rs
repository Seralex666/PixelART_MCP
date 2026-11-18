//! Dialog „Licht…“: Licht und Schatten, die einer Lichtquelle folgen, einrichten,
//! ändern, festschreiben oder entfernen. Die Lichtquelle kann über die Frames
//! von einem Punkt zu einem anderen wandern.

use crate::app::{section, App};
use crate::dialogs::Outcome;
use crate::doc::AssetId;
use eframe::egui::{self, Color32, RichText};
use pixel_core::lang::tr;
use pixel_core::light::{position_for, AutoLight, CastShadow, MAX_WIDTH};
use pixel_core::project::LIGHT_DIRS;
use pixel_core::Rgba;

/// Größe der Vorschau im Dialog.
const PREVIEW: f32 = 180.0;

pub struct LightForm {
    id: AssetId,
    existing: bool,
    size: (u32, u32),
    frames: usize,
    start: (f32, f32),
    moving: bool,
    end: (f32, f32),
    width: u32,
    strength: u32,
    tinted: bool,
    color: Rgba,
    shadows: bool,
    cast: bool,
    cast_length: u32,
    cast_opacity: u8,
    /// Welcher Frame in der Vorschau zu sehen ist.
    frame: usize,
    names: (String, String, String),
    preview: Option<(AutoLight, usize, egui::TextureHandle)>,
}

impl LightForm {
    pub fn new(app: &App, id: AssetId) -> Option<Self> {
        let doc = app.doc.as_ref()?;
        let s = doc.sprite(id)?;
        let (w, h) = (s.width(), s.height());
        let dir = doc
            .project()
            .style
            .light_dir
            .clone()
            .unwrap_or_else(|| "top-left".into());
        let start = position_for(&dir, w, h).unwrap_or((-(w as f32), -(h as f32)));
        let mut form = Self {
            id,
            existing: false,
            size: (w, h),
            frames: s.frames().len(),
            start,
            moving: s.frames().len() > 1,
            end: position_for("top-right", w, h).unwrap_or(start),
            width: 1,
            strength: 1,
            tinted: false,
            color: Rgba::rgb(255, 180, 90),
            shadows: true,
            cast: false,
            cast_length: 100,
            cast_opacity: 110,
            frame: app.frame,
            names: (
                tr("Licht", "Light").into(),
                tr("Schatten", "Shadow").into(),
                tr("Schlagschatten", "Ground shadow").into(),
            ),
            preview: None,
        };
        if let Some(l) = s.auto_light() {
            form.existing = true;
            form.start = l.path[0];
            form.moving = l.path.len() > 1;
            form.end = *l.path.last().unwrap_or(&l.path[0]);
            form.width = l.width;
            form.strength = l.strength;
            form.tinted = l.color.is_some();
            form.color = l.color.unwrap_or(form.color);
            form.shadows = l.shadows;
            form.names.0 = l.light_layer.clone();
            form.names.1 = l.shadow_layer.clone();
            if let Some(c) = &l.cast {
                form.cast = true;
                form.cast_length = c.length;
                form.cast_opacity = c.opacity;
                form.names.2 = c.layer.clone();
            }
        }
        Some(form)
    }

    fn spec(&self) -> AutoLight {
        let mut path = vec![self.start];
        if self.moving {
            path.push(self.end);
        }
        AutoLight {
            light_layer: self.names.0.clone(),
            shadow_layer: self.names.1.clone(),
            path,
            width: self.width,
            strength: self.strength,
            color: self.tinted.then_some(self.color),
            shadows: self.shadows,
            cast: self.cast.then(|| CastShadow {
                length: self.cast_length,
                opacity: self.cast_opacity,
                ..CastShadow::new(self.names.2.clone())
            }),
        }
    }
}

/// Neun Knöpfe für die Richtung, aus der das Licht kommt; der mittlere setzt die
/// Lichtquelle in die Mitte des Bildes. Daneben die genaue Position in Pixeln.
fn position(ui: &mut egui::Ui, id: &str, at: &mut (f32, f32), (w, h): (u32, u32)) {
    ui.horizontal(|ui| {
        egui::Grid::new(id).spacing([2.0, 2.0]).show(ui, |ui| {
            for i in 0..9 {
                let target = match i {
                    4 => Some((w as f32 / 2.0, h as f32 / 2.0)),
                    _ => {
                        let d = if i < 4 { i } else { i - 1 };
                        position_for(LIGHT_DIRS[d], w, h)
                    }
                };
                let on =
                    target.is_some_and(|t| (t.0 - at.0).abs() < 0.5 && (t.1 - at.1).abs() < 0.5);
                let tip = if i == 4 {
                    tr("In der Mitte des Bildes", "In the middle of the picture")
                } else {
                    tr(
                        "Von weit außen aus dieser Richtung",
                        "From far outside in this direction",
                    )
                };
                let button = ui.add(
                    egui::Button::new("")
                        .selected(on)
                        .min_size(egui::vec2(22.0, 20.0)),
                );
                arrow(ui, button.rect, i, on);
                if button.on_hover_text(tip).clicked() {
                    if let Some(t) = target {
                        *at = t;
                    }
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        ui.vertical(|ui| {
            let range = -(4.0 * w.max(h) as f32)..=(5.0 * w.max(h) as f32);
            ui.add(
                egui::DragValue::new(&mut at.0)
                    .range(range.clone())
                    .speed(0.5)
                    .prefix("x "),
            );
            ui.add(
                egui::DragValue::new(&mut at.1)
                    .range(range)
                    .speed(0.5)
                    .prefix("y "),
            );
        });
    });
}

/// Draws the arrow of a direction button by hand, because the default fonts
/// have no arrow glyphs. Button 4 (the middle) gets a dot.
fn arrow(ui: &egui::Ui, rect: egui::Rect, i: usize, on: bool) {
    let color = if on {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    let c = rect.center();
    let painter = ui.painter_at(rect);
    if i == 4 {
        painter.circle_filled(c, 2.5, color);
        return;
    }
    let d = egui::vec2((i % 3) as f32 - 1.0, (i / 3) as f32 - 1.0).normalized();
    let tip = c + d * 5.5;
    let tail = c - d * 5.5;
    let stroke = egui::Stroke::new(1.5, color);
    painter.line_segment([tail, tip], stroke);
    let side = egui::vec2(-d.y, d.x);
    for s in [side, -side] {
        painter.line_segment([tip, tip - d * 4.0 + s * 3.5], stroke);
    }
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, f: &mut LightForm) -> Outcome {
    ui.set_max_width(520.0);
    ui.heading(tr("Licht und Schatten", "Light and shadow"));
    ui.weak(tr(
        "Kanten zur Lichtquelle werden heller, Kanten von ihr weg dunkler, jeweils auf einer eigenen Ebene. Beides folgt jeder Änderung, in allen Frames. Die Lichtquelle kann über die Frames wandern, etwa eine Fackel, die vorbeizieht.",
        "Edges facing the light source get lighter, edges facing away darker, each on its own layer. Both follow every change, in every frame. The light source can move over the frames, like a torch passing by.",
    ));

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            section(ui, tr("Lichtquelle", "Light source"));
            if f.frames > 1 {
                ui.checkbox(
                    &mut f.moving,
                    tr("wandert über die Frames", "moves over the frames"),
                );
            }
            if f.moving && f.frames > 1 {
                ui.weak(tr("Im ersten Frame", "In the first frame"));
            }
            position(ui, "light-start", &mut f.start, f.size);
            if f.moving && f.frames > 1 {
                ui.weak(tr("Im letzten Frame", "In the last frame"));
                position(ui, "light-end", &mut f.end, f.size);
            }

            section(ui, tr("Licht", "Light"));
            ui.horizontal(|ui| {
                ui.label(tr("Breite", "Width"));
                ui.add(
                    egui::DragValue::new(&mut f.width)
                        .range(1..=MAX_WIDTH)
                        .suffix(" px"),
                );
                ui.label(tr("Stärke", "Strength"));
                ui.radio_value(&mut f.strength, 1, "1");
                ui.radio_value(&mut f.strength, 2, "2").on_hover_text(tr(
                    "Die äußersten Kantenpixel zwei Stufen heller oder dunkler",
                    "The outermost edge pixels two steps lighter or darker",
                ));
            });
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut f.tinted,
                    false,
                    tr("Heller entlang der Palette", "Lighter along the palette"),
                );
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.tinted, true, tr("Farbiges Licht", "Colored light"));
                let mut rgba = f.color.to_array();
                if ui
                    .add_enabled_ui(f.tinted, |ui| {
                        ui.color_edit_button_srgba_unmultiplied(&mut rgba)
                    })
                    .inner
                    .changed()
                {
                    f.color = Rgba::new(rgba[0], rgba[1], rgba[2], 255);
                }
            });
            ui.checkbox(
                &mut f.shadows,
                tr("Schattenseite abdunkeln", "Darken the shadow side"),
            );

            section(ui, tr("Schlagschatten", "Ground shadow"));
            ui.checkbox(
                &mut f.cast,
                tr(
                    "Schatten am Boden, vom Licht weg",
                    "Shadow on the ground, away from the light",
                ),
            );
            if f.cast {
                ui.horizontal(|ui| {
                    ui.label(tr("Länge", "Length"));
                    ui.add(egui::Slider::new(&mut f.cast_length, 10..=300).suffix(" %"));
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Deckkraft", "Opacity"));
                    let mut pct = u32::from(f.cast_opacity) * 100 / 255;
                    if ui
                        .add(egui::Slider::new(&mut pct, 10..=100).suffix(" %"))
                        .changed()
                    {
                        f.cast_opacity = (pct * 255 / 100) as u8;
                    }
                });
            }
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            preview(app, ui, f);
            if f.frames > 1 {
                ui.add(
                    egui::Slider::new(&mut f.frame, 0..=f.frames - 1)
                        .text("Frame")
                        .custom_formatter(|v, _| format!("{}", v as usize + 1)),
                );
            }
        });
    });

    ui.add_space(10.0);
    let mut outcome = Outcome::Keep;
    let mut action: Option<(Option<AutoLight>, bool)> = None;
    ui.horizontal(|ui| {
        let apply = if f.existing {
            tr("Übernehmen", "Apply")
        } else {
            tr("Licht anlegen", "Add light")
        };
        if ui.button(RichText::new(apply).strong()).clicked()
            || ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            action = Some((Some(f.spec()), false));
        }
        if f.existing {
            if ui
                .button(tr("Festschreiben", "Freeze"))
                .on_hover_text(tr(
                    "Licht und Schatten bleiben als normale Ebenen stehen und lassen sich von Hand ändern, folgen aber nicht mehr",
                    "Light and shadow stay as normal layers you can edit by hand, but no longer follow",
                ))
                .clicked()
            {
                action = Some((None, true));
            }
            if ui.button(tr("Entfernen", "Remove")).clicked() {
                action = Some((None, false));
            }
        }
        if ui.button(tr("Abbrechen", "Cancel")).clicked() {
            outcome = Outcome::Close;
        }
    });
    if let Some((spec, keep)) = action {
        let id = f.id;
        if app.sel != Some(id) {
            app.select(id);
        }
        let done = app.edit(|s| {
            match spec.clone() {
                Some(spec) => s.set_auto_light(spec),
                None => s.stop_light(keep),
            }
            .map_err(|e| e.to_string())
        });
        if done.is_some() {
            // Die Ebenen haben sich verschoben: auf einer Zeichenebene bleiben.
            if let Some(s) = app.sprite() {
                if app.layer >= s.layer_count() || s.is_light_layer(app.layer) {
                    app.layer = (0..s.layer_count())
                        .rev()
                        .find(|&l| {
                            !s.is_light_layer(l)
                                && !s.is_reference(l)
                                && s.outline_layer() != Some(l)
                        })
                        .unwrap_or(0);
                }
            }
            app.info(match (spec, keep) {
                (Some(_), _) => tr("Licht angelegt", "Light set"),
                (None, true) => tr("Licht festgeschrieben", "Light frozen"),
                (None, false) => tr("Licht entfernt", "Light removed"),
            });
            outcome = Outcome::Close;
        }
    }
    outcome
}

/// Zeigt einen Frame mit Licht und Schatten, wie sie mit diesen Einstellungen würden,
/// und wo die Lichtquelle dann steht.
fn preview(app: &App, ui: &mut egui::Ui, f: &mut LightForm) {
    let spec = f.spec();
    let frame = f.frame.min(f.frames.saturating_sub(1));
    let fresh = f
        .preview
        .as_ref()
        .is_some_and(|(s, fr, _)| *s == spec && *fr == frame);
    if !fresh {
        let image = app.doc.as_ref().and_then(|d| d.sprite(f.id)).and_then(|s| {
            let mut s = s.clone();
            s.set_auto_light(spec.clone()).ok()?;
            s.refresh_outline().ok()?;
            crate::canvas::sprite_image(&s, frame)
        });
        f.preview = image.map(|img| {
            let tex = ui
                .ctx()
                .load_texture("light-preview", img, egui::TextureOptions::NEAREST);
            (spec.clone(), frame, tex)
        });
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(PREVIEW, PREVIEW), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    let Some((_, _, tex)) = &f.preview else {
        return;
    };
    let size = tex.size_vec2();
    let scale = ((PREVIEW - 40.0) / size.x).min((PREVIEW - 40.0) / size.y);
    let scale = if scale >= 1.0 { scale.floor() } else { scale };
    let img = egui::Rect::from_center_size(rect.center(), size * scale);
    painter.rect_filled(img, 0.0, Color32::from_gray(200));
    painter.image(
        tex.id(),
        img,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    // Die Lichtquelle; liegt sie weit draußen, steht sie am Rand der Vorschau.
    let to_screen = |(x, y): (f32, f32)| img.min + egui::vec2(x, y) * scale;
    let inner = rect.shrink(8.0);
    let clamp = |p: egui::Pos2| {
        egui::pos2(
            p.x.clamp(inner.min.x, inner.max.x),
            p.y.clamp(inner.min.y, inner.max.y),
        )
    };
    if spec.path.len() > 1 {
        let (a, b) = (
            clamp(to_screen(spec.path[0])),
            clamp(to_screen(spec.path[1])),
        );
        painter.add(egui::Shape::dashed_line(
            &[a, b],
            egui::Stroke::new(1.0, Color32::from_rgb(242, 193, 78)),
            4.0,
            3.0,
        ));
    }
    let sun = clamp(to_screen(spec.position(frame, f.frames)));
    painter.circle(
        sun,
        6.0,
        Color32::from_rgb(255, 214, 102),
        egui::Stroke::new(1.5, Color32::from_rgb(120, 80, 20)),
    );
}
