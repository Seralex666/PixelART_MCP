//! Werkzeugleiste: Pinselgröße, Werkzeuge, Spiegeln, Kontur, Ansicht, und darunter
//! die Einstellungen des gewählten Werkzeugs.

use crate::app::App;
use crate::dialogs::Dialog;
use crate::icons::Icon;
use crate::tools::{ShapeChoice, Tool, MAX_BRUSH};
use eframe::egui;
use pixel_core::lang::tr;
use pixel_core::shapes::Shape;
use pixel_core::trf;

const ICON: f32 = 22.0;

impl App {
    pub(crate) fn tool_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            // Pinselgröße ganz vorn.
            let brush = self.icons.icon(ui.ctx(), Icon::Brush);
            ui.add_enabled(
                self.brush.tool.uses_size(),
                egui::Image::new((brush, egui::vec2(ICON, ICON))),
            )
            .on_hover_text(if self.brush.tool == Tool::Spray {
                tr("Sprühradius", "Spray radius")
            } else {
                tr("Pinselgröße", "Brush size")
            });
            ui.add_enabled(
                self.brush.tool.uses_size(),
                egui::DragValue::new(&mut self.brush.size)
                    .range(1..=MAX_BRUSH)
                    .suffix(" px"),
            );
            ui.separator();

            for tool in Tool::ALL {
                self.tool_button(ui, tool);
                if tool == Tool::Shape {
                    // „Gefüllt“ gleich hinter Rechteck, Ellipse und Formen.
                    ui.add_enabled(
                        self.brush.tool.uses_fill(),
                        egui::Checkbox::new(&mut self.brush.filled, tr("Gefüllt", "Filled")),
                    );
                    ui.separator();
                }
                if tool == Tool::Shade {
                    ui.separator();
                }
            }
            ui.separator();

            let mirrors = self.brush.tool.mirrors();
            for (icon, on, tip) in [
                (
                    Icon::MirrorX,
                    self.brush.mirror_x,
                    tr(
                        "Spiegeln links und rechts: alles erscheint auch auf der anderen Seite",
                        "Mirror left and right: everything also appears on the other side",
                    ),
                ),
                (
                    Icon::MirrorY,
                    self.brush.mirror_y,
                    tr("Spiegeln oben und unten", "Mirror top and bottom"),
                ),
            ] {
                let tex = self.icons.icon(ui.ctx(), icon);
                let button = egui::Button::image((tex, egui::vec2(ICON, ICON))).selected(on);
                if ui.add_enabled(mirrors, button).on_hover_text(tip).clicked() {
                    match icon {
                        Icon::MirrorX => self.brush.mirror_x = !self.brush.mirror_x,
                        _ => self.brush.mirror_y = !self.brush.mirror_y,
                    }
                }
            }
            ui.separator();

            // Kontur mit eigenem Symbol, etwas abgesetzt.
            let has_outline = self.sprite().is_some_and(|s| s.auto_outline().is_some());
            let tex = self.icons.icon(ui.ctx(), Icon::Outline);
            let label = if has_outline {
                tr("Kontur bearbeiten…", "Edit outline…")
            } else {
                tr("Kontur…", "Outline…")
            };
            if ui
                .add_enabled(
                    self.sel.is_some(),
                    egui::Button::image_and_text((tex, egui::vec2(ICON, ICON)), label),
                )
                .on_hover_text(tr(
                    "Automatische Kontur auf eigener Ebene, die jeder Änderung folgt",
                    "Automatic outline on its own layer that follows every change",
                ))
                .clicked()
            {
                self.outline_dialog();
            }
            ui.separator();

            if ui
                .button("−")
                .on_hover_text(tr("Verkleinern", "Zoom out"))
                .clicked()
            {
                self.set_zoom(-1);
            }
            ui.label(format!("{}×", self.view.zoom));
            if ui
                .button("+")
                .on_hover_text(tr("Vergrößern", "Zoom in"))
                .clicked()
            {
                self.set_zoom(1);
            }
            if ui
                .button(tr("Einpassen", "Fit"))
                .on_hover_text(tr(
                    "Ganzes Bild in das Fenster einpassen (Taste 0)",
                    "Fit the whole image into the window (key 0)",
                ))
                .clicked()
            {
                self.view.fit = true;
            }
        });
        self.tool_options(ui);
        ui.add_space(2.0);
    }

    fn tool_button(&mut self, ui: &mut egui::Ui, tool: Tool) {
        let tex = self.icons.get(ui.ctx(), tool);
        let button =
            egui::Button::image((tex, egui::vec2(ICON, ICON))).selected(self.brush.tool == tool);
        if ui
            .add(button)
            .on_hover_text(format!("{} ({})", tool.label(), tool.key()))
            .clicked()
        {
            self.brush.tool = tool;
        }
    }

    /// Die Einstellungen des gewählten Werkzeugs, in einer eigenen Zeile.
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        let tool = self.brush.tool;
        let has_options = matches!(
            tool,
            Tool::Pencil | Tool::Spray | Tool::Shade | Tool::Shape | Tool::Select
        );
        if !has_options {
            return;
        }
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.weak(tool.label());
            ui.add_space(6.0);
            match tool {
                Tool::Pencil => {
                    ui.checkbox(
                        &mut self.brush.pixel_perfect,
                        tr("Pixel-perfekt", "Pixel-perfect"),
                    )
                    .on_hover_text(tr(
                        "Entfernt beim Zeichnen doppelte Pixel in Ecken, für saubere 1-Pixel-Linien (nur bei 1 px)",
                        "Removes doubled corner pixels while drawing, for clean 1-pixel lines (1 px only)",
                    ));
                }
                Tool::Spray => {
                    ui.label(tr("Dichte", "Density"));
                    ui.add(egui::Slider::new(&mut self.brush.density, 5..=100).suffix(" %"));
                    ui.weak(tr(
                        "Gedrückt halten sprüht weiter",
                        "Hold the button to keep spraying",
                    ));
                }
                Tool::Shade => {
                    ui.radio_value(&mut self.brush.lighten, true, tr("Aufhellen", "Lighten"));
                    ui.radio_value(&mut self.brush.lighten, false, tr("Abdunkeln", "Darken"));
                    ui.weak(tr(
                        "Eine Stufe je Strich, entlang der Farbtreppen der Palette",
                        "One step per stroke, along the color ramps of the palette",
                    ));
                }
                Tool::Shape => self.shape_options(ui),
                Tool::Select => self.select_options(ui),
                _ => {}
            }
        });
    }

    fn shape_options(&mut self, ui: &mut egui::Ui) {
        let current = match self.brush.shape {
            ShapeChoice::Builtin(s) => s.label().to_string(),
            ShapeChoice::Custom(i) => self
                .custom_shapes
                .get(i)
                .map_or_else(|| Shape::Heart.label().to_string(), |c| c.name.clone()),
        };
        egui::ComboBox::from_id_salt("shape")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for s in Shape::ALL {
                    ui.selectable_value(&mut self.brush.shape, ShapeChoice::Builtin(s), s.label());
                }
                if !self.custom_shapes.is_empty() {
                    ui.separator();
                    ui.weak(tr("Eigene Formen", "Custom shapes"));
                    for (i, c) in self.custom_shapes.iter().enumerate() {
                        ui.selectable_value(&mut self.brush.shape, ShapeChoice::Custom(i), &c.name);
                    }
                }
            });
        if matches!(self.brush.shape, ShapeChoice::Custom(_)) {
            ui.checkbox(
                &mut self.brush.keep_colors,
                tr("Eigene Farben", "Own colors"),
            )
            .on_hover_text(tr(
                "Die Form in ihren gespeicherten Farben zeichnen statt in der aktuellen Farbe",
                "Draw the shape in its saved colors instead of the current color",
            ));
            if ui
                .button(tr("Form löschen", "Delete shape"))
                .on_hover_text(tr(
                    "Entfernt die eigene Form aus allen Projekten",
                    "Removes the custom shape from all projects",
                ))
                .clicked()
            {
                self.delete_custom_shape();
            }
        }
        if ui
            .button(tr("Als Form speichern…", "Save as shape…"))
            .on_hover_text(tr(
                "Macht das Gezeichnete (die Auswahl oder die ganze Ebene) zu einer eigenen Form, die es in allen Projekten gibt",
                "Turns what is drawn (the selection or the whole layer) into a custom shape available in all projects",
            ))
            .clicked()
        {
            self.open_dialog(Dialog::SaveShape {
                name: String::new(),
            });
        }
        ui.weak(tr(
            "Aufziehen wie ein Rechteck",
            "Drag it out like a rectangle",
        ));
    }

    fn select_options(&mut self, ui: &mut egui::Ui) {
        let has = self.selection.is_some();
        if let Some(a) = self.selection {
            ui.weak(trf!("{}×{} Pixel", "{}×{} pixels", a.w, a.h));
        }
        if ui
            .add_enabled(has, egui::Button::new(tr("Kopieren", "Copy")))
            .on_hover_text(tr("Strg+C", "Ctrl+C"))
            .clicked()
            && self.copy_selection()
        {
            ui.ctx().copy_text(self.clipboard_mark());
        }
        if ui
            .add_enabled(has, egui::Button::new(tr("Ausschneiden", "Cut")))
            .on_hover_text(tr("Strg+X", "Ctrl+X"))
            .clicked()
            && self.copy_selection()
        {
            ui.ctx().copy_text(self.clipboard_mark());
            self.delete_selection();
        }
        if ui
            .add_enabled(
                self.clipboard.is_some(),
                egui::Button::new(tr("Einfügen", "Paste")),
            )
            .on_hover_text(tr("Strg+V", "Ctrl+V"))
            .clicked()
        {
            self.paste();
        }
        let flip_x = self.icons.icon(ui.ctx(), Icon::MirrorX);
        let flip_y = self.icons.icon(ui.ctx(), Icon::MirrorY);
        if ui
            .add_enabled(has, egui::Button::image((flip_x, egui::vec2(18.0, 18.0))))
            .on_hover_text(tr(
                "Auswahl links-rechts spiegeln",
                "Flip selection left-right",
            ))
            .clicked()
        {
            self.flip_selection(true, false);
        }
        if ui
            .add_enabled(has, egui::Button::image((flip_y, egui::vec2(18.0, 18.0))))
            .on_hover_text(tr(
                "Auswahl oben-unten spiegeln",
                "Flip selection top-bottom",
            ))
            .clicked()
        {
            self.flip_selection(false, true);
        }
        if ui
            .add_enabled(has, egui::Button::new(tr("Leeren", "Clear")))
            .on_hover_text(tr("Entf", "Delete"))
            .clicked()
        {
            self.delete_selection();
        }
        if ui
            .add_enabled(
                has,
                egui::Button::new(tr("Als Form speichern…", "Save as shape…")),
            )
            .clicked()
        {
            self.open_dialog(Dialog::SaveShape {
                name: String::new(),
            });
        }
        if ui
            .button(tr("Alles", "All"))
            .on_hover_text(tr("Strg+A", "Ctrl+A"))
            .clicked()
        {
            self.select_all();
        }
        if ui
            .add_enabled(has, egui::Button::new(tr("Aufheben", "Deselect")))
            .on_hover_text(tr("Esc", "Esc"))
            .clicked()
        {
            self.deselect();
        }
    }
}
