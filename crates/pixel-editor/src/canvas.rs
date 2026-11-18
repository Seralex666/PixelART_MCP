//! Die Leinwand: zeigt das Bild pixelgenau und setzt Mausstriche um.

use crate::app::{c32, App, ZOOM_LEVELS};
use crate::dialogs::Dialog;
use crate::tools::{ShapeChoice, Stroke, Tool};
use eframe::egui::{self, Color32, Key, PointerButton, Rect, Sense, StrokeKind};
use pixel_core::draw::Target;
use pixel_core::lang::tr;
use pixel_core::trf;
use pixel_core::Sprite;

/// Ab dieser Vergrößerung lohnt sich das Pixelraster.
const GRID_MIN_ZOOM: u32 = 6;
/// Kantenlänge eines Karos im Transparenz-Muster, in Bildschirmpunkten.
const CHECKER: f32 = 8.0;

/// Ein Frame als Bild für die Grafikkarte, ohne Vorlage-Ebenen.
pub fn sprite_image(s: &Sprite, frame: usize) -> Option<egui::ColorImage> {
    to_image(s, s.composite(frame).ok()?)
}

/// Ein Frame so, wie die Leinwand ihn zeigt: mit Vorlage-Ebenen zum Nachzeichnen.
fn canvas_image(s: &Sprite, frame: usize) -> Option<egui::ColorImage> {
    to_image(s, s.composite_with_references(frame).ok()?)
}

fn to_image(s: &Sprite, pixels: Vec<pixel_core::Rgba>) -> Option<egui::ColorImage> {
    Some(egui::ColorImage::new(
        [s.width() as usize, s.height() as usize],
        pixels.into_iter().map(c32).collect(),
    ))
}

const UV: Rect = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let area = resp.rect;
    app.hover_cell = None;

    let Some((w, h, frames)) = app
        .sprite()
        .map(|s| (s.width(), s.height(), s.frames().len()))
    else {
        welcome(app, ui, area);
        return;
    };
    let ppp = ui.ctx().pixels_per_point();

    if app.view.fit {
        // Größte Stufe, bei der das ganze Bild mit etwas Rand hineinpasst.
        let room = area.size() * ppp * 0.9;
        app.view.zoom = ZOOM_LEVELS
            .iter()
            .rev()
            .copied()
            .find(|&z| (w * z) as f32 <= room.x && (h * z) as f32 <= room.y)
            .unwrap_or(1);
        app.view.pan = egui::Vec2::ZERO;
        app.view.fit = false;
    }

    // Ein Bildpixel belegt immer eine ganze Zahl von Bildschirmpixeln, auch bei
    // Windows-Skalierung wie 125 %. Nur so bleiben die Pixel gleich groß.
    let cell = app.view.zoom as f32 / ppp;
    let size = egui::vec2(w as f32 * cell, h as f32 * cell);
    let snap = |v: f32| (v * ppp).round() / ppp;
    let center = area.center() + app.view.pan;
    let origin = egui::pos2(snap(center.x - size.x / 2.0), snap(center.y - size.y / 2.0));
    let image_rect = Rect::from_min_size(origin, size);

    // --- Zeichnen ---
    let checker = app.textures.checker.get_or_insert_with(|| {
        let (a, b) = (Color32::from_gray(190), Color32::from_gray(225));
        ui.ctx().load_texture(
            "checker",
            egui::ColorImage::new([2, 2], vec![a, b, b, a]),
            egui::TextureOptions::NEAREST_REPEAT,
        )
    });
    let tiles = size / (2.0 * CHECKER);
    painter.image(
        checker.id(),
        image_rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(tiles.x, tiles.y)),
        Color32::WHITE,
    );

    if let (Some(doc), Some(id)) = (&app.doc, app.sel) {
        let rev = doc.revision();
        if let Some(s) = doc.sprite(id) {
            if app.onion && !app.playing && frames > 1 {
                let prev = (app.frame + frames - 1) % frames;
                let key = (rev, id, prev);
                if app.textures.onion.as_ref().is_none_or(|(k, _)| *k != key) {
                    if let Some(img) = sprite_image(s, prev) {
                        let tex =
                            ui.ctx()
                                .load_texture("onion", img, egui::TextureOptions::NEAREST);
                        app.textures.onion = Some((key, tex));
                    }
                }
                if let Some((_, tex)) = &app.textures.onion {
                    painter.image(tex.id(), image_rect, UV, Color32::from_white_alpha(90));
                }
            }
            let key = (rev, id, app.frame);
            if app.textures.main.as_ref().is_none_or(|(k, _)| *k != key) {
                if let Some(img) = canvas_image(s, app.frame) {
                    let tex = ui
                        .ctx()
                        .load_texture("canvas", img, egui::TextureOptions::NEAREST);
                    app.textures.main = Some((key, tex));
                }
            }
            if let Some((_, tex)) = &app.textures.main {
                painter.image(tex.id(), image_rect, UV, Color32::WHITE);
            }
        }
    }

    if app.grid && app.view.zoom >= GRID_MIN_ZOOM {
        let line = egui::Stroke::new(1.0 / ppp, Color32::from_black_alpha(45));
        let visible = image_rect.intersect(area);
        let first_x = ((visible.min.x - origin.x) / cell).floor().max(0.0) as u32;
        let last_x = (((visible.max.x - origin.x) / cell).ceil() as u32).min(w);
        for x in first_x..=last_x {
            painter.vline(origin.x + x as f32 * cell, visible.y_range(), line);
        }
        let first_y = ((visible.min.y - origin.y) / cell).floor().max(0.0) as u32;
        let last_y = (((visible.max.y - origin.y) / cell).ceil() as u32).min(h);
        for y in first_y..=last_y {
            painter.hline(visible.x_range(), origin.y + y as f32 * cell, line);
        }
    }
    painter.rect_stroke(
        image_rect,
        0.0,
        egui::Stroke::new(1.0, Color32::from_gray(90)),
        StrokeKind::Outside,
    );

    // --- Eingabe ---
    let cell_at = |pos: egui::Pos2| {
        (
            ((pos.x - origin.x) / cell).floor() as i64,
            ((pos.y - origin.y) / cell).floor() as i64,
        )
    };
    let input = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.secondary_pressed(),
            i.pointer.latest_pos(),
            i.key_down(Key::Space),
            i.smooth_scroll_delta.y,
        )
    });
    let (pressed, down, secondary, pos, space, scroll) = input;
    let over = resp.contains_pointer() && matches!(app.dialog, Dialog::None);
    let hover = pos.filter(|_| over).map(cell_at);
    app.hover_cell =
        hover.filter(|&(x, y)| x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h));

    // Auswahl: gestrichelter Rahmen.
    if let Some(a) = app.selection {
        let r = Rect::from_min_size(
            origin + egui::vec2(a.x as f32 * cell, a.y as f32 * cell),
            egui::vec2(a.w as f32 * cell, a.h as f32 * cell),
        );
        let clip = painter.with_clip_rect(area);
        let corners = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        clip.add(egui::Shape::line(
            corners.to_vec(),
            egui::Stroke::new(1.0, Color32::WHITE),
        ));
        clip.extend(egui::Shape::dashed_line(
            &corners,
            egui::Stroke::new(1.0, Color32::BLACK),
            4.0,
            4.0,
        ));
    }

    // Spiegelachsen.
    if app.brush.tool.mirrors() && (app.brush.mirror_x || app.brush.mirror_y) {
        let axis = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 200, 230, 160));
        if app.brush.mirror_x {
            painter.vline(image_rect.center().x, image_rect.y_range(), axis);
        }
        if app.brush.mirror_y {
            painter.hline(image_rect.x_range(), image_rect.center().y, axis);
        }
    }

    // Pinsel-Umriss unter dem Mauszeiger; bei der Sprühdose ein Kreis.
    if let (Some((x, y)), None, Tool::Spray) = (hover, &app.stroke, app.brush.tool) {
        let r = app.brush.size as f32 + 0.5;
        let c = origin + egui::vec2((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
        let clip = painter.with_clip_rect(area);
        clip.circle_stroke(c, r * cell + 1.0, egui::Stroke::new(1.0, Color32::BLACK));
        clip.circle_stroke(c, r * cell, egui::Stroke::new(1.0, Color32::WHITE));
    } else if let (Some((x, y)), None) = (hover, &app.stroke) {
        let n = if app.brush.tool.uses_size() {
            i64::from(app.brush.size)
        } else {
            1
        };
        let offset = (n - 1) / 2;
        let outline = Rect::from_min_size(
            origin + egui::vec2((x - offset) as f32 * cell, (y - offset) as f32 * cell),
            egui::Vec2::splat(n as f32 * cell),
        );
        let clip = painter.with_clip_rect(area);
        clip.rect_stroke(
            outline,
            0.0,
            egui::Stroke::new(1.0, Color32::BLACK),
            StrokeKind::Outside,
        );
        clip.rect_stroke(
            outline,
            0.0,
            egui::Stroke::new(1.0, Color32::WHITE),
            StrokeKind::Inside,
        );
    }

    // Verschieben des Ausschnitts: mittlere Maustaste oder Leertaste + Ziehen.
    let panning = resp.dragged_by(PointerButton::Middle)
        || (space && resp.dragged_by(PointerButton::Primary) && app.stroke.is_none());
    if panning {
        app.view.pan += resp.drag_delta();
    }

    // Mausrad vergrößert um die Stelle unter dem Zeiger.
    if over && scroll != 0.0 && app.stroke.is_none() {
        app.view.scroll += scroll;
        let steps = (app.view.scroll / 40.0).trunc() as i32;
        if steps != 0 {
            app.view.scroll -= steps as f32 * 40.0;
            let before = app.view.zoom;
            app.set_zoom(steps);
            if let Some(p) = pos {
                // Der Bildpunkt unter dem Zeiger soll dort bleiben.
                let factor = app.view.zoom as f32 / before as f32;
                let from_center = p - center;
                app.view.pan += from_center - from_center * factor;
            }
        }
    }

    // Rechte Maustaste: Farbe aufnehmen, egal welches Werkzeug gewählt ist.
    if over && secondary {
        if let Some(c) = hover {
            pick(app, c);
        }
    }

    if pressed && over && !space && app.stroke.is_none() {
        if let Some(c) = hover {
            begin(app, c);
        }
    }
    if app.stroke.is_some() {
        if down {
            if let Some(p) = pos {
                drag(app, cell_at(p));
            }
            // Die Sprühdose sprüht weiter, solange die Taste gedrückt bleibt.
            let now = ui.input(|i| i.time);
            if now - app.last_puff > 0.04 {
                app.last_puff = now;
                tick(app);
            }
        } else {
            end(app);
        }
        ui.ctx().request_repaint();
    }
}

fn welcome(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let text = if app.doc.is_none() {
        tr("Willkommen im PixelArt Studio.\n\nLege über „Datei › Neues Projekt…“ ein Projekt an.\nWie Claude hier mitzeichnet, steht unter „Hilfe › Mit Claude verbinden…“.", "Welcome to PixelArt Studio.\n\nCreate a project with \"File › New project…\".\nHow Claude draws along is explained under \"Help › Connect to Claude…\".")
    } else {
        tr("Dieses Projekt ist noch leer.\n\nLege links eine Figur oder ein Bild an,\noder lass Claude über den MCP-Server zeichnen.", "This project is still empty.\n\nCreate a character or an image on the left,\nor let Claude draw through the MCP server.")
    };
    ui.painter().text(
        area.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(16.0),
        ui.visuals().weak_text_color(),
    );
}

/// Nimmt die Farbe des sichtbaren Bildes an einer Zelle auf.
fn pick(app: &mut App, (x, y): (i64, i64)) {
    let Some(s) = app.sprite() else { return };
    if x < 0 || y < 0 || x >= i64::from(s.width()) || y >= i64::from(s.height()) {
        return;
    }
    let Ok(pixels) = s.composite(app.frame) else {
        return;
    };
    let c = pixels[y as usize * s.width() as usize + x as usize];
    if c.is_transparent() {
        app.info(tr("Dort ist nichts gezeichnet", "Nothing is drawn there"));
    } else {
        app.brush.color = c;
        app.info(trf!("Farbe {c} aufgenommen", "Picked color {c}"));
    }
}

fn begin(app: &mut App, at: (i64, i64)) {
    if app.brush.tool == Tool::Picker {
        pick(app, at);
        return;
    }
    let Some(id) = app.sel else { return };
    app.playing = false;
    if app.sprite().and_then(|s| s.outline_layer()) == Some(app.layer) {
        app.error(tr(
            "Diese Ebene ist die automatische Kontur und folgt den anderen Ebenen. Zeichne auf einer anderen Ebene, oder schreibe die Kontur fest (Kontur bearbeiten… › Festschreiben), um sie von Hand zu ändern.",
            "This layer is the automatic outline and follows the other layers. Draw on another layer, or freeze the outline (Edit outline… › Freeze) to change it by hand.",
        ));
        return;
    }
    if app.sprite().is_some_and(|s| s.is_light_layer(app.layer)) {
        app.error(tr(
            "Diese Ebene gehört zum automatischen Licht und folgt den anderen Ebenen. Zeichne auf einer anderen Ebene, oder schreibe das Licht fest (Licht und Schatten… › Festschreiben).",
            "This layer belongs to the automatic light and follows the other layers. Draw on another layer, or freeze the light (Light and shadow… › Freeze).",
        ));
        return;
    }
    if app.sprite().is_some_and(|s| s.is_effect_layer(app.layer)) {
        app.error(tr(
            "Diese Ebene gehört zu einem Effekt und wird nach jeder Änderung neu gezeichnet. Zeichne auf einer anderen Ebene, oder schreibe den Effekt fest (Effekte… › Festschreiben).",
            "This layer belongs to an effect and is redrawn after every change. Draw on another layer, or freeze the effect (Effects… › Freeze).",
        ));
        return;
    }
    if app.sprite().is_some_and(|s| s.is_reference(app.layer)) {
        app.error(tr(
            "Diese Ebene ist eine Vorlage zum Nachzeichnen. Zeichne auf einer anderen Ebene darüber.",
            "This layer is a reference to trace over. Draw on another layer above it.",
        ));
        return;
    }
    let Some(doc) = &mut app.doc else { return };
    if let Err(err) = doc.begin_stroke(id) {
        app.error(err);
        return;
    }
    // Der Abgleich beim Beginn kann das Projekt neu geladen haben.
    app.fix_selection();
    if app.sel != Some(id) {
        return;
    }
    let target = Target::new(app.frame, app.layer);
    let brush = app.brush;
    let custom = match brush.shape {
        ShapeChoice::Custom(i) if brush.tool == Tool::Shape => app.custom_shapes.get(i).cloned(),
        _ => None,
    };
    let lifted = if brush.tool == Tool::Select {
        app.lift_at(at)
    } else {
        None
    };
    if brush.tool == Tool::Select && lifted.is_none() {
        // Eine neue Auswahl beginnt; was schwebte, bleibt liegen.
        app.floating = None;
    }
    let Some(doc) = &mut app.doc else { return };
    let hidden = doc
        .sprite(id)
        .and_then(|s| s.layer(target.frame, target.layer).ok())
        .is_some_and(|l| !l.visible);
    let started = if hidden {
        Err(tr(
            "Die gewählte Ebene ist ausgeblendet. Blende sie ein, um darauf zu zeichnen.",
            "The selected layer is hidden. Show it to draw on it.",
        )
        .into())
    } else {
        doc.stroke_sprite(id)
            .and_then(|s| Stroke::begin(s, target, brush, at, custom, lifted))
    };
    match started {
        Ok(stroke) => app.stroke = Some((id, stroke)),
        Err(err) => {
            let _ = doc.end_stroke(id);
            app.error(err);
        }
    }
}

fn drag(app: &mut App, at: (i64, i64)) {
    let (Some(doc), Some((id, stroke))) = (&mut app.doc, &mut app.stroke) else {
        return;
    };
    if stroke.last() == at {
        return;
    }
    let result = doc.stroke_sprite(*id).and_then(|s| stroke.drag(s, at));
    if stroke.tool() == Tool::Select {
        app.selection = stroke.selection();
    }
    if let Err(err) = result {
        app.error(err);
    }
}

fn tick(app: &mut App) {
    let (Some(doc), Some((id, stroke))) = (&mut app.doc, &mut app.stroke) else {
        return;
    };
    if stroke.tool() != Tool::Spray {
        return;
    }
    if let Err(err) = doc.stroke_sprite(*id).and_then(|s| stroke.tick(s)) {
        app.error(err);
    }
}

fn end(app: &mut App) {
    let Some((id, stroke)) = app.stroke.take() else {
        return;
    };
    if let Some(doc) = &mut app.doc {
        if let Err(err) = doc.end_stroke(id) {
            app.error(trf!(
                "Speichern fehlgeschlagen: {err}",
                "Saving failed: {err}"
            ));
        }
    }
    if stroke.tool() == Tool::Select {
        if !stroke.dragged() && !stroke.is_moving() {
            // Ein einfacher Klick neben die Auswahl hebt sie auf.
            app.deselect();
        } else {
            app.selection = stroke.selection();
            let floating = stroke.finish();
            app.set_floating(floating);
        }
    }
}
