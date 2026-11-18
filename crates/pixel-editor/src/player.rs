//! Kleine Vorschau, die die Frames eines Sprites abspielt. Die Dialoge für
//! Drehen und Farbvarianten benutzen sie.

use eframe::egui::{self, Color32};
use pixel_core::Sprite;
use std::hash::{DefaultHasher, Hash, Hasher};

pub struct Player {
    key: u64,
    frames: Vec<egui::TextureHandle>,
    durations: Vec<u32>,
    size: (u32, u32),
    started: f64,
}

/// Schlüssel aus allem, was die Vorschau bestimmt (über die Debug-Ausgabe,
/// damit auch Kommazahlen zählen).
pub fn key(parts: &impl std::fmt::Debug) -> u64 {
    let mut h = DefaultHasher::new();
    format!("{parts:?}").hash(&mut h);
    h.finish()
}

/// Wo und wie groß das Bild in der Vorschau liegt.
pub struct Shown {
    pub image: egui::Rect,
    pub scale: f32,
}

impl Player {
    /// Baut die Vorschau neu, wenn sich `key` geändert hat. `make` liefert das Sprite.
    pub fn ensure(
        slot: &mut Option<Player>,
        ui: &egui::Ui,
        name: &str,
        key: u64,
        make: impl FnOnce() -> Option<Sprite>,
    ) {
        if slot.as_ref().map(|p| p.key) == Some(key) {
            return;
        }
        let started = slot
            .as_ref()
            .map_or_else(|| ui.input(|i| i.time), |p| p.started);
        *slot = make().map(|s| Player {
            key,
            frames: (0..s.frames().len())
                .filter_map(|i| crate::canvas::sprite_image(&s, i))
                .enumerate()
                .map(|(i, img)| {
                    ui.ctx()
                        .load_texture(format!("{name}-{i}"), img, egui::TextureOptions::NEAREST)
                })
                .collect(),
            durations: s.frames().iter().map(|f| f.duration_ms.max(20)).collect(),
            size: (s.width(), s.height()),
            started,
        });
    }

    /// Zeichnet den aktuellen Frame in `rect`.
    pub fn paint(&self, ui: &egui::Ui, painter: &egui::Painter, rect: egui::Rect) -> Shown {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let scale = ((rect.width() - 12.0) / w).min((rect.height() - 12.0) / h);
        let scale = if scale >= 1.0 { scale.floor() } else { scale };
        let image = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h) * scale);
        let total: u32 = self.durations.iter().sum::<u32>().max(1);
        let now =
            ((ui.input(|i| i.time) - self.started) * 1000.0).max(0.0) as u64 % u64::from(total);
        let mut acc = 0u64;
        let mut frame = 0;
        for (i, d) in self.durations.iter().enumerate() {
            acc += u64::from(*d);
            if now < acc {
                frame = i;
                break;
            }
        }
        if let Some(tex) = self.frames.get(frame) {
            painter.image(
                tex.id(),
                image,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if self.frames.len() > 1 {
            ui.ctx().request_repaint();
            painter.text(
                rect.left_bottom() + egui::vec2(6.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{}/{}", frame + 1, self.frames.len()),
                egui::FontId::monospace(11.0),
                Color32::from_gray(170),
            );
        }
        Shown { image, scale }
    }
}

/// Ein dunkles Feld für die Vorschau.
pub fn frame(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    sense: egui::Sense,
) -> (egui::Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(size, sense);
    ui.painter_at(rect)
        .rect_filled(rect, 4.0, Color32::from_rgb(36, 40, 52));
    (rect, response)
}
