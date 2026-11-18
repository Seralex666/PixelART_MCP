//! Auswahl: kopieren, ausschneiden, einfügen, spiegeln, leeren und verschieben.
//!
//! Eine Auswahl ist ein Rechteck auf der Leinwand. Wird sie verschoben oder etwas
//! eingefügt, schwebt der Inhalt, bis etwas anderes geschieht: so lässt er sich
//! mehrmals verschieben, ohne zu übermalen, was darunter liegt.

use crate::app::App;
use crate::doc::AssetId;
use crate::tools::{Lifted, Tool};
use pixel_core::brush;
use pixel_core::effects::{self, Area, Patch};
use pixel_core::lang::tr;
use pixel_core::trf;

/// Wozu ein schwebender Ausschnitt gehört: Asset, Frame, Ebene und Stand des Projekts.
pub type FloatKey = (AssetId, usize, usize, u64);

impl App {
    fn float_key(&self) -> Option<FloatKey> {
        Some((
            self.sel?,
            self.frame,
            self.layer,
            self.doc.as_ref()?.revision(),
        ))
    }

    /// Der schwebende Ausschnitt, falls er noch zum aktuellen Stand passt.
    fn valid_floating(&self) -> Option<&Lifted> {
        let key = self.float_key()?;
        self.floating
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, l)| l)
    }

    /// Merkt sich einen schwebenden Ausschnitt für den aktuellen Stand.
    pub fn set_floating(&mut self, lifted: Option<Lifted>) {
        self.floating = lifted.and_then(|l| Some((self.float_key()?, l)));
    }

    /// Was ein Strich mit dem Auswahl-Werkzeug bei `at` anhebt: nichts, wenn außerhalb
    /// der Auswahl begonnen wird.
    pub fn lift_at(&self, at: (i64, i64)) -> Option<Lifted> {
        let area = self.selection?;
        let inside = at.0 >= area.x
            && at.1 >= area.y
            && at.0 < area.x + i64::from(area.w)
            && at.1 < area.y + i64::from(area.h);
        if !inside {
            return None;
        }
        if let Some(l) = self.valid_floating() {
            return Some(l.clone());
        }
        let s = self.sprite()?;
        Lifted::lift(s, self.target(), area).ok()
    }

    /// Ob auf der aktuellen Ebene von Hand geändert werden darf.
    fn layer_editable(&mut self) -> bool {
        if self.sprite().and_then(|s| s.outline_layer()) == Some(self.layer) {
            self.error(tr(
                "Diese Ebene ist die automatische Kontur. Wähle eine andere Ebene.",
                "This layer is the automatic outline. Pick another layer.",
            ));
            return false;
        }
        if self.sprite().is_some_and(|s| s.is_light_layer(self.layer)) {
            self.error(tr(
                "Diese Ebene gehört zum automatischen Licht. Wähle eine andere Ebene.",
                "This layer belongs to the automatic light. Pick another layer.",
            ));
            return false;
        }
        if self.sprite().is_some_and(|s| s.is_effect_layer(self.layer)) {
            self.error(tr(
                "Diese Ebene gehört zu einem Effekt. Wähle eine andere Ebene.",
                "This layer belongs to an effect. Pick another layer.",
            ));
            return false;
        }
        if self.sprite().is_some_and(|s| s.is_reference(self.layer)) {
            self.error(tr(
                "Diese Ebene ist eine Vorlage zum Nachzeichnen. Wähle eine andere Ebene.",
                "This layer is a reference to trace over. Pick another layer.",
            ));
            return false;
        }
        true
    }

    fn need_selection(&mut self) -> Option<Area> {
        let area = self
            .selection
            .and_then(|a| brush::clip_area(self.sprite()?, a));
        if area.is_none() {
            self.info(tr(
                "Erst mit dem Auswahl-Werkzeug (S) einen Bereich aufziehen",
                "First drag an area with the select tool (S)",
            ));
        }
        area
    }

    pub fn select_all(&mut self) {
        let Some(s) = self.sprite() else { return };
        self.selection = Some(Area {
            x: 0,
            y: 0,
            w: s.width(),
            h: s.height(),
        });
        self.floating = None;
        self.brush.tool = Tool::Select;
    }

    pub fn deselect(&mut self) {
        self.selection = None;
        self.floating = None;
    }

    pub fn copy_selection(&mut self) -> bool {
        let Some(area) = self.need_selection() else {
            return false;
        };
        let t = self.target();
        let patch = self
            .sprite()
            .map(|s| Patch::cut(s, t.frame, Some(t.layer), Some(area)));
        match patch {
            Some(Ok(p)) => {
                let (w, h) = (p.width, p.height);
                self.clipboard = Some(p);
                self.info(trf!("{w}×{h} Pixel kopiert", "Copied {w}×{h} pixels"));
                true
            }
            Some(Err(err)) => {
                self.error(err.to_string());
                false
            }
            None => false,
        }
    }

    pub fn delete_selection(&mut self) {
        let Some(area) = self.need_selection() else {
            return;
        };
        if !self.layer_editable() {
            return;
        }
        let t = self.target();
        // Schwebt etwas, verschwindet es, und darunter erscheint wieder, was dort war.
        let base = self.valid_floating().map(|l| l.base.clone());
        self.edit(|s| match base {
            Some(base) => s
                .layer_mut(t.frame, t.layer)
                .and_then(|l| l.replace_pixels(base))
                .map_err(|e| e.to_string()),
            None => brush::clear_area(s, t, area)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        });
        self.floating = None;
    }

    /// Text, der beim Kopieren in die Zwischenablage des Systems kommt. Steht er beim
    /// Einfügen noch dort, wird das Kopierte eingefügt; sonst hat jemand inzwischen
    /// etwas anderes kopiert.
    pub fn clipboard_mark(&self) -> String {
        let (w, h) = self
            .clipboard
            .as_ref()
            .map_or((0, 0), |p| (p.width, p.height));
        format!("PixelArt Studio: {w}×{h} Pixel")
    }

    /// Fügt ein Bild aus der Zwischenablage des Systems ein, etwa einen Bildschirmausschnitt.
    pub fn paste_image(&mut self, img: &eframe::egui::ColorImage) {
        let [w, h] = img.size;
        let max = pixel_core::MAX_SIZE as usize;
        if w == 0 || h == 0 || w > max || h > max {
            self.error(trf!(
                "Das Bild in der Zwischenablage ist zu groß (höchstens {max}×{max} Pixel)",
                "The image on the clipboard is too large (at most {max}×{max} pixels)"
            ));
            return;
        }
        let pixels = img
            .pixels
            .iter()
            .map(|c| {
                let [r, g, b, a] = c.to_srgba_unmultiplied();
                pixel_core::Rgba::new(r, g, b, a)
            })
            .collect();
        self.clipboard = Some(Patch {
            width: w as u32,
            height: h as u32,
            pixels,
        });
        self.paste();
    }

    pub fn paste(&mut self) {
        let Some(patch) = self.clipboard.clone() else {
            self.info(tr(
                "Die Zwischenablage ist leer. Erst eine Auswahl kopieren (Strg+C).",
                "The clipboard is empty. Copy a selection first (Ctrl+C).",
            ));
            return;
        };
        if !self.layer_editable() {
            return;
        }
        let Some((w, h)) = self
            .sprite()
            .map(|s| (i64::from(s.width()), i64::from(s.height())))
        else {
            return;
        };
        // An die Stelle der Auswahl, sonst oben links; immer sichtbar.
        let (x, y) = self.selection.map_or((0, 0), |a| {
            (a.x.clamp(0, (w - 1).max(0)), a.y.clamp(0, (h - 1).max(0)))
        });
        let t = self.target();
        let pasted = self.edit(|s| {
            let base = s
                .layer(t.frame, t.layer)
                .map_err(|e| e.to_string())?
                .pixels()
                .to_vec();
            effects::stamp(s, t, &patch, x, y).map_err(|e| e.to_string())?;
            Ok(base)
        });
        if let Some(base) = pasted {
            let area = Area {
                x,
                y,
                w: patch.width,
                h: patch.height,
            };
            self.selection = Some(area);
            self.set_floating(Some(Lifted { patch, area, base }));
            self.brush.tool = Tool::Select;
            self.info(tr(
                "Eingefügt. Mit der Maus an die richtige Stelle ziehen.",
                "Pasted. Drag it to the right place with the mouse.",
            ));
        }
    }

    pub fn flip_selection(&mut self, horizontal: bool, vertical: bool) {
        let Some(area) = self.need_selection() else {
            return;
        };
        if !self.layer_editable() {
            return;
        }
        let t = self.target();
        let floating = self.valid_floating().cloned();
        let done = match floating {
            // Schwebt der Inhalt, wird nur er gespiegelt, nicht was darunter liegt.
            Some(l) => {
                let patch = l.patch.flipped(horizontal, vertical);
                let ok = self.edit(|s| {
                    s.layer_mut(t.frame, t.layer)
                        .and_then(|layer| layer.replace_pixels(l.base.clone()))
                        .map_err(|e| e.to_string())?;
                    effects::stamp(s, t, &patch, l.area.x, l.area.y).map_err(|e| e.to_string())
                });
                ok.map(|_| Lifted { patch, ..l })
            }
            None => self
                .edit(|s| {
                    brush::flip_area(s, t, area, horizontal, vertical).map_err(|e| e.to_string())
                })
                .and_then(|()| self.sprite().and_then(|s| Lifted::lift(s, t, area).ok())),
        };
        self.set_floating(done);
    }
}
