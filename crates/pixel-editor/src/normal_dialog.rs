//! Dialog „Normal Map…“: Normal Maps für Licht im Spiel ansehen und exportieren.
//! Links das Bild im Licht einer Lampe, die der Maus folgt, rechts die Normal Map.

use crate::app::{c32, section, App};
use crate::dialogs::Outcome;
use crate::doc::AssetId;
use eframe::egui::{self, Color32};
use pixel_core::export::MAX_SCALE;
use pixel_core::lang::tr;
use pixel_core::normalmap::{self, NormalOptions};
use pixel_core::trf;
use pixel_core::workspace::ExportFormat;
use pixel_core::Rgba;
use std::path::PathBuf;

/// Größe jeder der beiden Vorschauen.
const PREVIEW: f32 = 230.0;

/// Was zuletzt berechnet wurde, damit nicht jedes Bild neu entsteht.
struct Cache {
    options: NormalOptions,
    frame: usize,
    color: Vec<Rgba>,
    normals: Vec<Rgba>,
    map: egui::TextureHandle,
}

pub struct NormalForm {
    id: AssetId,
    size: (usize, usize),
    frames: usize,
    frame: usize,
    options: NormalOptions,
    /// Die Lampe in Pixeln des Bildes; die Höhe steht in `height`.
    lamp: (f32, f32),
    height: f32,
    sheet: bool,
    scale: u32,
    cache: Option<Cache>,
    written: Vec<PathBuf>,
}

impl NormalForm {
    pub fn new(app: &App, id: AssetId) -> Option<Self> {
        let s = app.doc.as_ref()?.sprite(id)?;
        let (w, h) = (s.width() as usize, s.height() as usize);
        Some(Self {
            id,
            size: (w, h),
            frames: s.frames().len(),
            frame: app.frame.min(s.frames().len().saturating_sub(1)),
            options: NormalOptions::default(),
            lamp: (-(w as f32) * 0.2, -(h as f32) * 0.2),
            height: w.max(h) as f32 * 0.6,
            sheet: s.frames().len() > 1,
            scale: 1,
            cache: None,
            written: Vec::new(),
        })
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, f: &mut NormalForm) -> Outcome {
    ui.set_max_width(560.0);
    ui.heading(tr("Normal Map", "Normal map"));
    ui.label(tr(
        "Eine Normal Map sagt einer Spiel-Engine wie Godot oder Unity, wohin jede Stelle des Bildes zeigt. Dann beleuchten Lampen im Spiel deine Pixel-Art live. Fahr mit der Maus über das linke Bild, um die Lampe zu bewegen.",
        "A normal map tells a game engine like Godot or Unity where each spot of the picture faces. Lamps in the game then light your pixel art live. Move the mouse over the left picture to move the lamp.",
    ));
    ui.add_space(6.0);
    previews(app, ui, f);
    if f.frames > 1 {
        ui.horizontal(|ui| {
            let mut shown = f.frame + 1;
            ui.add(egui::Slider::new(&mut shown, 1..=f.frames).text(tr("Frame", "Frame")));
            f.frame = shown - 1;
        });
    }

    section(ui, tr("Form", "Shape"));
    let o = &mut f.options;
    egui::Grid::new("normal-options")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label(tr("Wölbung", "Bevel"));
            ui.add(egui::Slider::new(&mut o.bevel, 1..=16).suffix(" px"))
                .on_hover_text(tr(
                    "So breit wölbt sich der Rand. Mehr wirkt runder.",
                    "How wide the rim curves. More reads rounder.",
                ));
            ui.end_row();
            ui.label(tr("Stärke", "Strength"));
            ui.add(egui::Slider::new(&mut o.strength, 0.1..=5.0).fixed_decimals(1));
            ui.end_row();
            ui.label(tr("Details aus Farben", "Detail from colors"));
            ui.add(egui::Slider::new(&mut o.detail, 0.0..=1.0).fixed_decimals(2))
                .on_hover_text(tr(
                    "Helle Farben stehen etwas vor, dunkle liegen tiefer, etwa Fugen zwischen Steinen.",
                    "Light colors stand out a little, dark ones lie deeper, like joints between stones.",
                ));
            ui.end_row();
            ui.label(tr("Lampenhöhe", "Lamp height"));
            ui.add(egui::Slider::new(&mut f.height, 1.0..=128.0).fixed_decimals(0));
            ui.end_row();
        });
    ui.checkbox(
        &mut o.flip_green,
        tr(
            "Grün umdrehen (DirectX-Art; Godot und Unity brauchen das nicht)",
            "Flip green (DirectX convention; Godot and Unity don't need it)",
        ),
    );

    section(ui, tr("Exportieren", "Export"));
    ui.horizontal(|ui| {
        if f.frames > 1 {
            ui.radio_value(&mut f.sheet, true, tr("Spritesheet", "Sprite sheet"));
            ui.radio_value(&mut f.sheet, false, tr("PNG je Frame", "PNG per frame"));
        }
        ui.label(tr("Vergrößerung", "Scale"));
        ui.add(
            egui::DragValue::new(&mut f.scale)
                .range(1..=MAX_SCALE)
                .suffix("×"),
        );
    });
    ui.weak(tr(
        "Die Dateien heißen wie beim normalen Export, nur mit _n vor der Endung, und liegen im Export-Ordner des Projekts.",
        "The files are named like the normal export, just with _n before the extension, and go into the project's export folder.",
    ));
    if !f.written.is_empty() {
        for path in &f.written {
            ui.monospace(path.display().to_string());
        }
    }
    ui.add_space(8.0);
    let mut close = false;
    ui.horizontal(|ui| {
        if ui
            .button(tr("Normal Map exportieren", "Export normal map"))
            .clicked()
        {
            export(app, f);
        }
        if !f.written.is_empty() {
            if let Some(dir) = f.written[0].parent() {
                if ui.button(tr("Ordner öffnen", "Open folder")).clicked() {
                    crate::help::open_folder(dir);
                }
            }
        }
        close = ui.button(tr("Schließen", "Close")).clicked();
    });
    if close {
        Outcome::Close
    } else {
        Outcome::Keep
    }
}

fn export(app: &mut App, f: &mut NormalForm) {
    let Some(doc) = app.doc.as_ref() else {
        return;
    };
    let format = if f.sheet && f.frames > 1 {
        ExportFormat::Sheet { columns: 0 }
    } else {
        ExportFormat::Png
    };
    match doc.export_normals(f.id, &f.options, format, f.scale) {
        Ok(files) => {
            let n = files.len();
            f.written = files;
            app.dialog_error = None;
            app.info(if n == 1 {
                tr("Normal Map exportiert", "Normal map exported").to_string()
            } else {
                trf!("{n} Normal Maps exportiert", "{n} normal maps exported")
            });
        }
        Err(err) => app.dialog_error = Some(err),
    }
}

fn previews(app: &App, ui: &mut egui::Ui, f: &mut NormalForm) {
    let frame = f.frame.min(f.frames.saturating_sub(1));
    let (w, h) = f.size;
    let fresh = f
        .cache
        .as_ref()
        .is_some_and(|c| c.options == f.options && c.frame == frame);
    if !fresh {
        let color = app
            .doc
            .as_ref()
            .and_then(|d| d.sprite(f.id))
            .and_then(|s| s.composite(frame).ok());
        f.cache = color.map(|color| {
            let normals = normalmap::normal_map(&color, w, h, &f.options);
            let img = egui::ColorImage::new([w, h], normals.iter().copied().map(c32).collect());
            let map = ui
                .ctx()
                .load_texture("normal-map", img, egui::TextureOptions::NEAREST);
            Cache {
                options: f.options,
                frame,
                color,
                normals,
                map,
            }
        });
    }
    let Some(cache) = &f.cache else {
        return;
    };
    let scale = ((PREVIEW - 24.0) / w as f32).min((PREVIEW - 24.0) / h as f32);
    let scale = if scale >= 1.0 { scale.floor() } else { scale };
    ui.horizontal(|ui| {
        // Links: das Bild im Licht der Lampe.
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(PREVIEW, PREVIEW), egui::Sense::hover());
        let img =
            egui::Rect::from_center_size(rect.center(), egui::vec2(w as f32, h as f32) * scale);
        if let Some(pos) = response.hover_pos() {
            let p = (pos - img.min) / scale;
            f.lamp = (p.x, p.y);
        }
        let shaded = normalmap::lit(
            &cache.color,
            &cache.normals,
            w,
            (f.lamp.0, f.lamp.1, f.height),
            0.25,
            f.options.flip_green,
        );
        let lit_img = egui::ColorImage::new([w, h], shaded.into_iter().map(c32).collect());
        let lit_tex = ui
            .ctx()
            .load_texture("normal-lit", lit_img, egui::TextureOptions::NEAREST);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_gray(24));
        painter.image(lit_tex.id(), img, full_uv(), Color32::WHITE);
        let lamp = img.min + egui::vec2(f.lamp.0, f.lamp.1) * scale;
        let inner = rect.shrink(6.0);
        let lamp = egui::pos2(
            lamp.x.clamp(inner.min.x, inner.max.x),
            lamp.y.clamp(inner.min.y, inner.max.y),
        );
        painter.circle(
            lamp,
            5.0,
            Color32::from_rgb(255, 230, 140),
            egui::Stroke::new(1.5, Color32::from_rgb(120, 90, 20)),
        );

        // Rechts: die Normal Map selbst.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(PREVIEW, PREVIEW), egui::Sense::hover());
        let img =
            egui::Rect::from_center_size(rect.center(), egui::vec2(w as f32, h as f32) * scale);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_gray(24));
        painter.image(cache.map.id(), img, full_uv(), Color32::WHITE);
    });
}

fn full_uv() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
}
