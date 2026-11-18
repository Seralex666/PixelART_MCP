//! Dialog „Effekte…“: Leuchten, Blitz, Explosion, Spiegelung, Funken, Rauch,
//! Magie und Farbzyklen einrichten, ändern, festschreiben oder entfernen. Die
//! Vorschau spielt die Frames ab, wie sie mit diesen Einstellungen würden.

use crate::app::{c32, section, App};
use crate::dialogs::Outcome;
use crate::doc::{AssetId, AssetKind};
use eframe::egui::{self, Color32, RichText};
use pixel_core::fx::{self, AutoEffect, EffectKind, ParticleStyle};
use pixel_core::lang::tr;
use pixel_core::{trf, Rgba, Sprite};

/// Größe der Vorschau.
const PREVIEW: f32 = 220.0;

/// Die Arten in der Reihenfolge der Auswahl.
const KINDS: [&str; 8] = [
    "glow",
    "lightning",
    "explosion",
    "reflection",
    "sparks",
    "smoke",
    "magic",
    "color_cycle",
];

struct Preview {
    key: (AutoEffect, u32, bool, u64),
    frames: Vec<egui::TextureHandle>,
    durations: Vec<u32>,
    size: (u32, u32),
}

pub struct FxForm {
    id: AssetId,
    /// Ebene des Effekts, der gerade bearbeitet wird; `None` für einen neuen.
    editing: Option<String>,
    draft: AutoEffect,
    /// So viele Frames soll das Bild mindestens haben (aus Kopien).
    frames: u32,
    /// Ein Bild darf für die Spiegelung nach unten wachsen.
    grow: bool,
    preview: Option<Preview>,
    started: f64,
}

impl FxForm {
    pub fn new(app: &App, id: AssetId) -> Option<Self> {
        let s = app.doc.as_ref()?.sprite(id)?;
        let mut form = Self {
            id,
            editing: None,
            draft: new_effect(s, "glow"),
            frames: s.frames().len() as u32,
            grow: true,
            preview: None,
            started: 0.0,
        };
        if let Some(e) = s.effects().first() {
            form.edit(e.clone());
        } else if s.frames().len() == 1 && id.kind != AssetKind::Character {
            form.frames = 8;
        }
        Some(form)
    }

    fn edit(&mut self, e: AutoEffect) {
        self.editing = Some(e.layer.clone());
        self.draft = e;
    }
}

fn new_effect(s: &Sprite, id: &str) -> AutoEffect {
    let kind = EffectKind::default_for(id, s.width(), s.height())
        .unwrap_or_else(|| EffectKind::default_for("glow", 16, 16).expect("glow"));
    let mut e = AutoEffect::new(kind);
    e.layer = s.free_layer_name(&e.layer);
    e
}

pub fn show(app: &mut App, ui: &mut egui::Ui, f: &mut FxForm) -> Outcome {
    let Some(sprite) = app.doc.as_ref().and_then(|d| d.sprite(f.id)).cloned() else {
        return Outcome::Close;
    };
    let character = f.id.kind == AssetKind::Character;
    ui.set_max_width(640.0);
    ui.heading(tr("Effekte", "Effects"));
    ui.label(tr(
        "Jeder Effekt liegt auf einer eigenen Ebene, läuft über alle Frames und folgt jeder Änderung am Bild, wie die Kontur.",
        "Each effect sits on its own layer, runs over all frames and follows every change to the picture, like the outline.",
    ));
    ui.add_space(6.0);

    // Die Effekte des Bildes und ein neuer.
    ui.horizontal_wrapped(|ui| {
        for e in sprite.effects() {
            let on = f.editing.as_deref() == Some(e.layer.as_str());
            if ui.selectable_label(on, &e.layer).clicked() && !on {
                f.edit(e.clone());
            }
        }
        if ui
            .selectable_label(f.editing.is_none(), tr("+ Neuer Effekt", "+ New effect"))
            .clicked()
            && f.editing.is_some()
        {
            f.editing = None;
            f.draft = new_effect(&sprite, f.draft.kind.id());
        }
    });
    ui.add_space(6.0);

    ui.horizontal_top(|ui| {
        preview(app, ui, f, &sprite, character);
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.set_width(360.0);
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| settings(ui, f, &sprite, character));
        });
    });

    ui.add_space(10.0);
    let mut outcome = Outcome::Keep;
    let mut action: Option<Action> = None;
    ui.horizontal(|ui| {
        let apply = if f.editing.is_some() {
            tr("Übernehmen", "Apply")
        } else {
            tr("Effekt anlegen", "Add effect")
        };
        if ui.button(RichText::new(apply).strong()).clicked() {
            action = Some(Action::Apply);
        }
        if f.editing.is_some() {
            if ui
                .button(tr("Festschreiben", "Freeze"))
                .on_hover_text(tr(
                    "Die Ebene bleibt als normale Ebene stehen und lässt sich von Hand ändern, folgt aber nicht mehr",
                    "The layer stays as a normal layer you can edit by hand, but no longer follows",
                ))
                .clicked()
            {
                action = Some(Action::Remove { keep: true });
            }
            if ui.button(tr("Entfernen", "Remove")).clicked() {
                action = Some(Action::Remove { keep: false });
            }
        }
        if ui.button(tr("Schließen", "Close")).clicked() {
            outcome = Outcome::Close;
        }
    });
    if let Some(action) = action {
        run(app, f, action, character);
    }
    outcome
}

enum Action {
    Apply,
    Remove { keep: bool },
}

fn run(app: &mut App, f: &mut FxForm, action: Action, character: bool) {
    let id = f.id;
    if app.sel != Some(id) {
        app.select(id);
    }
    let draft = f.draft.clone();
    let editing = f.editing.clone();
    let frames = if character { 1 } else { f.frames as usize };
    let grow = f.grow && id.kind == AssetKind::Image;
    let done = app.edit(|s| {
        let result = (|| -> pixel_core::Result<()> {
            match &action {
                Action::Apply => {
                    s.ensure_frames(frames)?;
                    // Den alten Effekt zuerst umbenennen, wenn der Name geändert wurde.
                    if let Some(old) = &editing {
                        if !old.eq_ignore_ascii_case(&draft.layer) && s.effect(old).is_some() {
                            if s.layer_index(&draft.layer).is_some() {
                                return Err(pixel_core::Error::Effect(trf!(
                                    "Es gibt schon eine Ebene „{}“",
                                    "there is already a layer '{}'",
                                    draft.layer
                                )));
                            }
                            if let Some(l) = s.layer_index(old) {
                                s.rename_layer(l, draft.layer.clone())?;
                            }
                        }
                    }
                    if let EffectKind::Reflection { line, .. } = &draft.kind {
                        if let Some(need) = s.reflection_height(*line) {
                            if grow && need > s.height() {
                                let w = s.width();
                                s.resize_canvas(w, need.min(pixel_core::MAX_SIZE), 0, 0)?;
                            }
                        }
                    }
                    s.set_effect(draft.clone())
                }
                Action::Remove { keep } => s.remove_effect(editing.as_deref().unwrap_or(""), *keep),
            }?;
            s.refresh_outline()?;
            Ok(())
        })();
        result.map_err(|e| e.to_string())
    });
    if done.is_none() {
        return;
    }
    // Auf einer Zeichenebene bleiben.
    if let Some(s) = app.sprite() {
        if app.layer >= s.layer_count() || s.is_auto_layer(app.layer) {
            app.layer = (0..s.layer_count())
                .rev()
                .find(|&l| !s.is_auto_layer(l) && !s.is_reference(l))
                .unwrap_or(0);
        }
    }
    match action {
        Action::Apply => {
            app.info(if editing.is_some() {
                tr("Effekt geändert", "Effect changed")
            } else {
                tr("Effekt angelegt", "Effect added")
            });
            f.editing = Some(f.draft.layer.clone());
        }
        Action::Remove { keep } => {
            app.info(if keep {
                tr("Effekt festgeschrieben", "Effect frozen")
            } else {
                tr("Effekt entfernt", "Effect removed")
            });
            f.editing = None;
            if let Some(s) = app.sprite() {
                f.draft = new_effect(s, f.draft.kind.id());
            }
        }
    }
}

/// Spielt die Frames ab, wie sie mit diesem Effekt würden. Ein Klick setzt den
/// Punkt des Effekts (beim Blitz: links Anfang, rechts Ende).
fn preview(app: &App, ui: &mut egui::Ui, f: &mut FxForm, sprite: &Sprite, character: bool) {
    let revision = app.doc.as_ref().map_or(0, |d| d.revision());
    let frames = if character { 1 } else { f.frames };
    let key = (f.draft.clone(), frames, f.grow, revision);
    if f.preview.as_ref().map(|p| &p.key) != Some(&key) {
        let mut s = sprite.clone();
        // Die bisherige Fassung dieses Effekts weg, damit der Entwurf an ihre Stelle tritt.
        if let Some(old) = &f.editing {
            if !old.eq_ignore_ascii_case(&f.draft.layer) {
                let _ = s.remove_effect(old, false);
            }
        }
        let ok = s.ensure_frames(frames as usize).is_ok()
            && {
                if let EffectKind::Reflection { line, .. } = &f.draft.kind {
                    if let Some(need) = s.reflection_height(*line) {
                        if f.grow && f.id.kind == AssetKind::Image && need > s.height() {
                            let w = s.width();
                            let _ = s.resize_canvas(w, need.min(pixel_core::MAX_SIZE), 0, 0);
                        }
                    }
                }
                true
            }
            && s.set_effect(f.draft.clone()).is_ok()
            && s.refresh_outline().is_ok();
        f.preview = ok.then(|| {
            let textures = (0..s.frames().len())
                .filter_map(|i| crate::canvas::sprite_image(&s, i))
                .enumerate()
                .map(|(i, img)| {
                    ui.ctx().load_texture(
                        format!("fx-preview-{i}"),
                        img,
                        egui::TextureOptions::NEAREST,
                    )
                })
                .collect();
            Preview {
                key: key.clone(),
                frames: textures,
                durations: s.frames().iter().map(|fr| fr.duration_ms.max(20)).collect(),
                size: (s.width(), s.height()),
            }
        });
        f.started = ui.input(|i| i.time);
    }
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(PREVIEW, PREVIEW), egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_rgb(36, 40, 52));
    let Some(p) = &f.preview else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr("Keine Vorschau", "No preview"),
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
        return;
    };
    let (w, h) = (p.size.0 as f32, p.size.1 as f32);
    let scale = ((PREVIEW - 16.0) / w).min((PREVIEW - 16.0) / h);
    let scale = if scale >= 1.0 { scale.floor() } else { scale };
    let img = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h) * scale);
    // Welcher Frame gerade dran ist.
    let total: u32 = p.durations.iter().sum::<u32>().max(1);
    let now = ((ui.input(|i| i.time) - f.started) * 1000.0) as u64 % u64::from(total);
    let mut acc = 0u64;
    let mut frame = 0;
    for (i, d) in p.durations.iter().enumerate() {
        acc += u64::from(*d);
        if now < acc {
            frame = i;
            break;
        }
    }
    if let Some(tex) = p.frames.get(frame) {
        painter.image(
            tex.id(),
            img,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if p.frames.len() > 1 {
        ui.ctx().request_repaint();
    }
    painter.text(
        rect.left_bottom() + egui::vec2(6.0, -6.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{}/{}", frame + 1, p.frames.len()),
        egui::FontId::monospace(11.0),
        Color32::from_gray(170),
    );
    // Klick in die Vorschau setzt den Punkt.
    let at = |pos: egui::Pos2| {
        let v = (pos - img.min) / scale;
        (v.x.round(), v.y.round())
    };
    if let Some(pos) = response.interact_pointer_pos() {
        let secondary = response.secondary_clicked();
        if response.clicked() || secondary {
            let pt = at(pos);
            match &mut f.draft.kind {
                EffectKind::Lightning { from, to, .. } => {
                    if secondary {
                        *to = pt;
                    } else {
                        *from = pt;
                    }
                }
                EffectKind::Explosion { at, .. } | EffectKind::Particles { at, .. } => *at = pt,
                _ => {}
            }
        }
    }
    let hint = match &f.draft.kind {
        EffectKind::Lightning { .. } => Some(tr(
            "Klick: Anfang, Rechtsklick: Ende",
            "Click: start, right-click: end",
        )),
        EffectKind::Explosion { .. } | EffectKind::Particles { .. } => {
            Some(tr("Klick setzt die Quelle", "Click sets the source"))
        }
        _ => None,
    };
    if let Some(hint) = hint {
        response.on_hover_text(hint);
    }
}

fn settings(ui: &mut egui::Ui, f: &mut FxForm, sprite: &Sprite, character: bool) {
    let (w, h) = (sprite.width(), sprite.height());
    ui.horizontal(|ui| {
        ui.label(tr("Art", "Type"));
        let current = f.draft.kind.id();
        egui::ComboBox::from_id_salt("fx-kind")
            .selected_text(f.draft.kind.label())
            .show_ui(ui, |ui| {
                for id in KINDS {
                    let label = EffectKind::default_for(id, w, h).map_or(id, |k| k.label());
                    if ui.selectable_label(current == id, label).clicked() && current != id {
                        let fresh = new_effect(sprite, id);
                        f.draft.kind = fresh.kind;
                        if f.editing.is_none() {
                            f.draft.layer = fresh.layer;
                        }
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label(tr("Ebene", "Layer"));
        ui.text_edit_singleline(&mut f.draft.layer);
    });
    if !character {
        ui.horizontal(|ui| {
            ui.label(tr("Frames mindestens", "At least frames"));
            ui.add(egui::DragValue::new(&mut f.frames).range(1..=fx::MAX_FRAMES as u32))
                .on_hover_text(tr(
                    "Fehlende Frames entstehen als Kopien der vorhandenen, damit sich der Effekt bewegen kann.",
                    "Missing frames are copies of the existing ones, so the effect can move.",
                ));
        });
    } else if f.draft.kind.animates() {
        ui.weak(tr(
            "Die Grundpose einer Figur hat einen Frame. Bewegte Effekte zeigen sich in ihren Animationen.",
            "A character's base pose has one frame. Moving effects show in its animations.",
        ));
    }

    section(ui, tr("Einstellungen", "Settings"));
    let max_r = w.max(h).clamp(2, 256);
    let used = sprite.used_colors();
    match &mut f.draft.kind {
        EffectKind::Glow {
            color,
            radius,
            pulse,
            colors,
            dither,
        } => {
            color_row(ui, tr("Farbe", "Color"), color);
            ui.add(egui::Slider::new(radius, 1..=8).text(tr("Reichweite", "Reach")));
            *pulse = (*pulse).min(*radius - 1);
            ui.add(egui::Slider::new(pulse, 0..=*radius - 1).text(tr("Pulsieren", "Pulse")));
            ui.checkbox(
                dither,
                tr(
                    "Gerastert statt durchscheinend",
                    "Dithered instead of see-through",
                ),
            );
            ui.label(tr(
                "Nur um diese Farben leuchten (leer: um die ganze Figur):",
                "Glow only around these colors (empty: around the whole figure):",
            ));
            toggle_colors(ui, &used, colors);
        }
        EffectKind::Lightning {
            from,
            to,
            color,
            glow,
            branches,
            flicker,
        } => {
            color_row(ui, tr("Farbe", "Color"), color);
            let mut has_glow = glow.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_glow, tr("Schein", "Glow"));
                let mut g = glow.unwrap_or(Rgba::rgb(143, 211, 255));
                if has_glow {
                    color_button(ui, &mut g);
                }
                *glow = has_glow.then_some(g);
            });
            ui.add(egui::Slider::new(branches, 0..=8).text(tr("Äste", "Branches")));
            ui.checkbox(flicker, tr("Flackern", "Flicker"));
            point_row(ui, tr("Anfang", "Start"), from);
            point_row(ui, tr("Ende", "End"), to);
        }
        EffectKind::Explosion {
            at,
            radius,
            start,
            length,
            ..
        } => {
            *radius = (*radius).min(max_r);
            ui.add(egui::Slider::new(radius, 2..=max_r).text(tr("Radius", "Radius")));
            point_row(ui, tr("Mitte", "Center"), at);
            ui.horizontal(|ui| {
                ui.label(tr("Ab Frame", "From frame"));
                let mut shown = *start + 1;
                ui.add(egui::DragValue::new(&mut shown).range(1..=fx::MAX_FRAMES as u32));
                *start = shown - 1;
                ui.label(tr("Dauer", "Length"));
                ui.add(egui::DragValue::new(length).range(0..=fx::MAX_FRAMES as u32))
                    .on_hover_text(tr("0 = bis zum letzten Frame", "0 = until the last frame"));
            });
        }
        EffectKind::Reflection {
            line,
            color,
            mix,
            opacity,
            wave,
        } => {
            color_row(ui, tr("Wasserfarbe", "Water color"), color);
            ui.add(
                egui::Slider::new(mix, 0..=100)
                    .suffix(" %")
                    .text(tr("Tönung", "Tint")),
            );
            ui.add(egui::Slider::new(opacity, 0..=255).text(tr("Deckkraft", "Opacity")));
            ui.add(
                egui::Slider::new(wave, 0..=8)
                    .suffix(" px")
                    .text(tr("Wellen", "Waves")),
            );
            let mut auto = line.is_none();
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut auto,
                    tr("Wasserlinie unter der Figur", "Water line below the figure"),
                );
                if !auto {
                    let mut l = line.unwrap_or(h / 2);
                    ui.add(egui::DragValue::new(&mut l).range(1..=h.saturating_sub(1)));
                    *line = Some(l);
                } else {
                    *line = None;
                }
            });
            if f.id.kind == AssetKind::Image {
                ui.checkbox(
                    &mut f.grow,
                    tr(
                        "Leinwand nach unten vergrößern, wenn das Spiegelbild nicht passt",
                        "Make the canvas taller when the reflection does not fit",
                    ),
                );
            }
        }
        EffectKind::Particles {
            style,
            at,
            spread,
            count,
            speed,
            ..
        } => {
            ui.horizontal(|ui| {
                for s in ParticleStyle::ALL {
                    ui.radio_value(style, s, s.label());
                }
            });
            point_row(ui, tr("Quelle", "Source"), at);
            ui.add(
                egui::Slider::new(spread, 0..=w.max(1))
                    .suffix(" px")
                    .text(tr("Breite", "Width")),
            );
            ui.add(egui::Slider::new(count, 1..=fx::MAX_PARTICLES).text(tr("Anzahl", "Count")));
            ui.add(
                egui::Slider::new(speed, 0.1..=5.0)
                    .fixed_decimals(1)
                    .text(tr("Weite", "Reach")),
            );
        }
        EffectKind::ColorCycle {
            colors,
            speed,
            reverse,
        } => {
            ui.label(tr(
                "Pixel dieser Farben wandern Frame für Frame zur nächsten Farbe der Reihe. Male eine Fläche mit den Farben, dann fließt sie.",
                "Pixels of these colors move one color along the row each step. Paint an area with the colors and it flows.",
            ));
            ui.horizontal(|ui| {
                ui.label(tr("Vorlagen", "Presets"));
                for (id, label) in [
                    ("water", tr("Wasser", "Water")),
                    ("fire", tr("Feuer", "Fire")),
                    ("lava", "Lava"),
                ] {
                    if ui.button(label).clicked() {
                        *colors = fx::cycle_preset(id).unwrap_or_default();
                    }
                }
            });
            ui.label(tr("Reihe (Klick entfernt):", "Row (click removes):"));
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for (i, c) in colors.iter().enumerate() {
                    if swatch(ui, *c, true).clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    colors.remove(i);
                }
            });
            ui.label(tr(
                "Farben im Bild (Klick hängt an):",
                "Colors in the picture (click appends):",
            ));
            ui.horizontal_wrapped(|ui| {
                for c in used.iter().take(48) {
                    if swatch(ui, *c, false).clicked() && colors.len() < fx::MAX_COLORS {
                        colors.push(*c);
                    }
                }
            });
            ui.add(
                egui::Slider::new(speed, 1..=16).text(tr("Frames je Schritt", "Frames per step")),
            );
            ui.checkbox(reverse, tr("Andersherum", "Reverse"));
            if !colors.is_empty() && !colors.iter().any(|c| used.contains(c)) {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    tr(
                        "Keine dieser Farben ist im Bild. Male zuerst mit ihnen.",
                        "None of these colors is in the picture. Paint with them first.",
                    ),
                );
            }
        }
    }
    if !matches!(
        f.draft.kind,
        EffectKind::ColorCycle { .. } | EffectKind::Reflection { .. } | EffectKind::Glow { .. }
    ) {
        ui.horizontal(|ui| {
            ui.label(tr("Zufall", "Seed"));
            ui.add(egui::DragValue::new(&mut f.draft.seed));
        })
        .response
        .on_hover_text(tr(
            "Eine andere Zahl ergibt andere Funken, Blitze und Explosionen.",
            "Another number gives other sparks, bolts and explosions.",
        ));
    }
}

fn color_row(ui: &mut egui::Ui, label: &str, c: &mut Rgba) {
    ui.horizontal(|ui| {
        ui.label(label);
        color_button(ui, c);
    });
}

fn color_button(ui: &mut egui::Ui, c: &mut Rgba) {
    let mut rgba = [c.r, c.g, c.b, c.a];
    if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
        *c = Rgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
    }
}

fn point_row(ui: &mut egui::Ui, label: &str, p: &mut (f32, f32)) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(&mut p.0).speed(0.5).prefix("x "));
        ui.add(egui::DragValue::new(&mut p.1).speed(0.5).prefix("y "));
    });
}

fn swatch(ui: &mut egui::Ui, c: Rgba, framed: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    ui.painter().rect_filled(rect, 3.0, c32(c));
    let stroke = if framed || response.hovered() {
        egui::Stroke::new(1.5, ui.visuals().strong_text_color())
    } else {
        egui::Stroke::new(1.0, Color32::from_gray(90))
    };
    ui.painter()
        .rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
    response.on_hover_text(c.to_string())
}

fn toggle_colors(ui: &mut egui::Ui, used: &[Rgba], chosen: &mut Vec<Rgba>) {
    ui.horizontal_wrapped(|ui| {
        for c in used.iter().take(48) {
            let on = chosen.contains(c);
            if swatch(ui, *c, on).clicked() {
                if on {
                    chosen.retain(|k| k != c);
                } else if chosen.len() < fx::MAX_COLORS {
                    chosen.push(*c);
                }
            }
        }
    });
}
