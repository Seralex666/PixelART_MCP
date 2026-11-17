//! Dialog „Kontur…“: automatische Kontur einrichten, ändern, festschreiben oder entfernen.

use crate::app::{section, App};
use crate::dialogs::Outcome;
use crate::doc::{AssetId, AssetKind};
use eframe::egui::{self, Color32, RichText};
use pixel_core::contour::{light_vector, AutoOutline, MAX_THICKNESS};
use pixel_core::lang::tr;
use pixel_core::project::LIGHT_DIRS;
use pixel_core::trf;
use pixel_core::Rgba;

/// Größe der Vorschau im Dialog.
const PREVIEW: f32 = 160.0;

pub struct OutlineForm {
    id: AssetId,
    existing: bool,
    layer: String,
    selective: bool,
    color: Rgba,
    inside: bool,
    corners: bool,
    thickness: u32,
    inner: bool,
    light: Option<(i8, i8)>,
    with_animations: bool,
    animations: usize,
    preview: Option<(AutoOutline, usize, egui::TextureHandle)>,
}

impl OutlineForm {
    pub fn new(app: &App, id: AssetId) -> Option<Self> {
        let doc = app.doc.as_ref()?;
        let s = doc.sprite(id)?;
        let style = &doc.project().style;
        let animations = match id.kind {
            AssetKind::Character => doc.project().animations_of(id.id).count(),
            _ => 0,
        };
        // Vorschlag für die feste Farbe: die des Stils, sonst die dunkelste der Palette.
        let darkest = s
            .palette
            .colors
            .iter()
            .copied()
            .filter(|c| !c.is_transparent())
            .min_by_key(|&c| pixel_core::quant::luminance(c))
            .unwrap_or(Rgba::BLACK);
        let mut form = Self {
            id,
            existing: false,
            layer: tr("Kontur", "Outline").to_string(),
            selective: style.outline.is_none(),
            color: style.outline.unwrap_or(darkest),
            inside: false,
            corners: false,
            thickness: 1,
            inner: false,
            light: style.light_dir.as_deref().and_then(light_vector),
            with_animations: true,
            animations,
            preview: None,
        };
        if let Some(o) = s.auto_outline() {
            form.existing = true;
            form.layer = o.layer.clone();
            form.selective = o.color.is_none();
            form.color = o.color.unwrap_or(form.color);
            form.inside = o.inside;
            form.corners = o.corners;
            form.thickness = o.thickness;
            form.inner = o.inner;
            form.light = o.light;
        }
        Some(form)
    }

    fn spec(&self) -> AutoOutline {
        AutoOutline {
            layer: self.layer.clone(),
            color: (!self.selective).then_some(self.color),
            inside: self.inside,
            corners: self.corners,
            thickness: self.thickness,
            inner: self.inner,
            light: self.light,
        }
    }
}

fn light_label(light: Option<(i8, i8)>) -> &'static str {
    let names = [
        tr("oben links", "top left"),
        tr("oben", "top"),
        tr("oben rechts", "top right"),
        tr("links", "left"),
        tr("rechts", "right"),
        tr("unten links", "bottom left"),
        tr("unten", "bottom"),
        tr("unten rechts", "bottom right"),
    ];
    light
        .and_then(|v| LIGHT_DIRS.iter().position(|d| light_vector(d) == Some(v)))
        .map_or(tr("überall gleich", "even all around"), |i| names[i])
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, f: &mut OutlineForm) -> Outcome {
    ui.set_max_width(460.0);
    ui.heading(tr("Kontur", "Outline"));
    ui.weak(tr(
        "Die Kontur liegt auf einer eigenen Ebene ganz oben und folgt jeder Änderung, in allen Frames. Zeichne einfach auf den anderen Ebenen weiter.",
        "The outline sits on its own layer at the top and follows every change, in every frame. Just keep drawing on the other layers.",
    ));

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            section(ui, tr("Farbe", "Color"));
            ui.radio_value(
                &mut f.selective,
                true,
                tr(
                    "Selektiv: dunklere Stufe der Farbe daneben",
                    "Selective: darker shade of the color next to it",
                ),
            );
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.selective, false, tr("Eine Farbe", "One color"));
                let mut rgba = f.color.to_array();
                if ui
                    .add_enabled_ui(!f.selective, |ui| {
                        ui.color_edit_button_srgba_unmultiplied(&mut rgba)
                    })
                    .inner
                    .changed()
                {
                    f.color = Rgba::new(rgba[0], rgba[1], rgba[2], 255);
                }
            });
            if f.selective {
                egui::ComboBox::from_label(tr("Licht von", "Light from"))
                    .selected_text(light_label(f.light))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.light, None, light_label(None));
                        for d in LIGHT_DIRS {
                            let v = light_vector(d);
                            ui.selectable_value(&mut f.light, v, light_label(v));
                        }
                    })
                    .response
                    .on_hover_text(tr(
                        "Auf dieser Seite wird die Kontur heller",
                        "The outline gets lighter on this side",
                    ));
            }

            section(ui, tr("Form", "Shape"));
            ui.radio_value(
                &mut f.inside,
                false,
                tr("Außen um die Figur", "Outside around the shape"),
            );
            ui.radio_value(
                &mut f.inside,
                true,
                tr(
                    "Innen auf dem Rand (Figur wird nicht größer)",
                    "Inside on the edge (shape does not grow)",
                ),
            );
            ui.horizontal(|ui| {
                ui.label(tr("Dicke", "Thickness"));
                ui.add(
                    egui::DragValue::new(&mut f.thickness)
                        .range(1..=MAX_THICKNESS)
                        .suffix(" px"),
                );
            });
            ui.checkbox(&mut f.corners, tr("Auch über Eck", "Include corners"))
                .on_hover_text(tr(
                    "Schließt diagonale Lücken, wirkt an Schrägen dicker",
                    "Closes diagonal gaps, looks thicker on slopes",
                ));
            ui.checkbox(
                &mut f.inner,
                tr("Innenlinien zwischen Ebenen", "Inner lines between layers"),
            )
            .on_hover_text(tr(
                "Wo eine obere Ebene an eine untere grenzt, etwa ein Arm vor dem Körper",
                "Where an upper layer meets a lower one, like an arm in front of the body",
            ));
            if f.animations > 0 {
                ui.add_space(4.0);
                let text = if f.animations == 1 {
                    tr(
                        "Auch für die Animation der Figur",
                        "Also for the character's animation",
                    )
                    .to_string()
                } else {
                    trf!(
                        "Auch für alle {} Animationen der Figur",
                        "Also for all {} animations of the character",
                        f.animations
                    )
                };
                ui.checkbox(&mut f.with_animations, text);
            }
        });
        ui.add_space(12.0);
        preview(app, ui, f);
    });

    ui.add_space(10.0);
    let mut outcome = Outcome::Keep;
    let mut action: Option<(Option<AutoOutline>, bool)> = None;
    ui.horizontal(|ui| {
        let apply = if f.existing {
            tr("Übernehmen", "Apply")
        } else {
            tr("Kontur anlegen", "Add outline")
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
                    "Die Kontur bleibt als normale Ebene stehen und lässt sich von Hand ändern, folgt aber nicht mehr",
                    "The outline stays as a normal layer you can edit by hand, but no longer follows",
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
        let with = f.with_animations && f.animations > 0;
        let result = app
            .doc
            .as_mut()
            .map(|d| d.set_outline(f.id, spec.clone(), keep, with));
        match result {
            Some(Ok(n)) => {
                app.fix_selection();
                let also = match n {
                    0 => String::new(),
                    1 => tr(" (auch in der Animation)", " (also in the animation)").to_string(),
                    n => trf!(" (auch in {n} Animationen)", " (also in {n} animations)"),
                };
                app.info(match (spec, keep) {
                    (Some(_), _) => trf!("Kontur angelegt{also}", "Outline set{also}"),
                    (None, true) => trf!("Kontur festgeschrieben{also}", "Outline frozen{also}"),
                    (None, false) => trf!("Kontur entfernt{also}", "Outline removed{also}"),
                });
                outcome = Outcome::Close;
            }
            Some(Err(err)) => app.dialog_error = Some(err),
            None => outcome = Outcome::Close,
        }
    }
    outcome
}

/// Zeigt den aktuellen Frame mit der Kontur, wie sie mit diesen Einstellungen würde.
fn preview(app: &App, ui: &mut egui::Ui, f: &mut OutlineForm) {
    let spec = f.spec();
    let frame = app.frame;
    let fresh = f
        .preview
        .as_ref()
        .is_some_and(|(s, fr, _)| *s == spec && *fr == frame);
    if !fresh {
        let image = app.doc.as_ref().and_then(|d| d.sprite(f.id)).and_then(|s| {
            let mut s = s.clone();
            s.set_auto_outline(spec.clone()).ok()?;
            crate::canvas::sprite_image(&s, frame.min(s.frames().len() - 1))
        });
        f.preview = image.map(|img| {
            let tex = ui
                .ctx()
                .load_texture("outline-preview", img, egui::TextureOptions::NEAREST);
            (spec, frame, tex)
        });
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(PREVIEW, PREVIEW), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    if let Some((_, _, tex)) = &f.preview {
        let size = tex.size_vec2();
        let scale = ((PREVIEW - 8.0) / size.x).min((PREVIEW - 8.0) / size.y);
        // Ganze Vielfache halten die Pixel scharf, solange das Bild hineinpasst.
        let scale = if scale >= 1.0 { scale.floor() } else { scale };
        let img = egui::Rect::from_center_size(rect.center(), size * scale);
        ui.painter().rect_filled(img, 0.0, Color32::from_gray(200));
        ui.painter().image(
            tex.id(),
            img,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}
