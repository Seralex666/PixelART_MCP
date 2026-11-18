//! Dialog „Autotile…“: aus einer kleinen Vorlage einen ganzen Kachelsatz bauen,
//! ihn auf einer Beispielkarte ansehen und für Godot oder Tiled exportieren.

use crate::app::{c32, section, App};
use crate::dialogs::Outcome;
use crate::doc::{AssetId, AssetKind};
use eframe::egui::{self, Color32, RichText};
use pixel_core::autotile::{self, TileSet, TileSetKind};
use pixel_core::export::MAX_SCALE;
use pixel_core::lang::tr;
use pixel_core::workspace::TileEngine;
use pixel_core::{trf, ColorDepth, Palette, Rgba, Sprite};
use std::path::PathBuf;

/// Breite der Vorschau.
const PREVIEW_W: f32 = 560.0;
const PREVIEW_H: f32 = 240.0;

/// Schlüssel, Textur und Größe der zuletzt gezeichneten Vorschau.
type Preview = (
    (u64, AssetId, TileSetKind, u64),
    egui::TextureHandle,
    (f32, f32),
);

pub struct TileForm {
    id: AssetId,
    kind: TileSetKind,
    /// Name für den Satz (beim Bauen) oder die neue Vorlage.
    name: String,
    tile: u32,
    seed: u64,
    scale: u32,
    cache: Option<Preview>,
    written: Vec<PathBuf>,
}

impl TileForm {
    pub fn new(app: &App, id: AssetId) -> Option<Self> {
        let doc = app.doc.as_ref()?;
        let s = doc.sprite(id)?;
        let name = doc.asset_name(id).unwrap_or("").to_string();
        // Gibt es schon einen Satz aus dieser Vorlage, baut der Dialog ihn neu.
        let built = doc.project().images.iter().find(|i| {
            i.sprite
                .tileset()
                .and_then(|t| t.source.as_deref())
                .is_some_and(|src| {
                    pixel_core::workspace::name_key(src) == pixel_core::workspace::name_key(&name)
                })
        });
        let kind = s
            .tileset()
            .or_else(|| built.and_then(|i| i.sprite.tileset()))
            .map_or(TileSetKind::Blob47, |t| t.kind);
        Some(Self {
            id,
            kind,
            name: match built {
                Some(i) => i.name.clone(),
                None if autotile::tile_of_template(s.width(), s.height()).is_ok() => {
                    trf!("{name} Kacheln", "{name} tiles")
                }
                None => tr("Gelände", "Terrain").to_string(),
            },
            tile: 16,
            seed: 1,
            scale: 1,
            cache: None,
            written: Vec::new(),
        })
    }
}

enum Mode {
    /// Das Bild ist ein fertiger Satz.
    Set(TileSet),
    /// Das Bild ist eine Vorlage.
    Template,
    /// Weder noch: eine Vorlage anlegen.
    New,
}

pub fn show(app: &mut App, ui: &mut egui::Ui, f: &mut TileForm) -> Outcome {
    let Some(sprite) = app.doc.as_ref().and_then(|d| d.sprite(f.id)).cloned() else {
        return Outcome::Close;
    };
    let mode = match sprite.tileset() {
        Some(t) => Mode::Set(t.clone()),
        None if f.id.kind == AssetKind::Image
            && autotile::tile_of_template(sprite.width(), sprite.height()).is_ok() =>
        {
            Mode::Template
        }
        None => Mode::New,
    };
    ui.set_max_width(PREVIEW_W + 20.0);
    ui.heading("Autotile");
    ui.label(tr(
        "Du zeichnest nur wenige Kacheln: einen Block aus 2 × 2 Kacheln mit Außenecken, Rändern und Mitte und eine Kachel mit den vier Innenecken. Daraus entstehen alle Kacheln, die sich auf einer Karte von selbst zusammensetzen.",
        "You draw only a few tiles: a block of 2 × 2 tiles with outer corners, edges and fill, and one tile with the four inner corners. From these come all the tiles that join up by themselves on a map.",
    ));
    ui.add_space(6.0);
    let mut outcome = Outcome::Keep;
    match mode {
        Mode::New => new_template(app, ui, f),
        Mode::Template => template(app, ui, f, &sprite),
        Mode::Set(info) => set(app, ui, f, &sprite, &info),
    }
    ui.add_space(8.0);
    if ui.button(tr("Schließen", "Close")).clicked() {
        outcome = Outcome::Close;
    }
    outcome
}

fn new_template(app: &mut App, ui: &mut egui::Ui, f: &mut TileForm) {
    ui.label(tr(
        "Das gewählte Bild ist keine Autotile-Vorlage. Eine Vorlage ist 2 Kacheln breit und 3 hoch; eine Hilfsebene zeigt, was wohin gehört.",
        "The selected image is not an autotile template. A template is 2 tiles wide and 3 tall; a guide layer shows what goes where.",
    ));
    section(ui, tr("Neue Vorlage", "New template"));
    ui.horizontal(|ui| {
        ui.label(tr("Name", "Name"));
        ui.text_edit_singleline(&mut f.name);
    });
    ui.horizontal(|ui| {
        ui.label(tr("Kachelgröße", "Tile size"));
        for t in [8, 16, 24, 32, 48, 64] {
            ui.radio_value(&mut f.tile, t, format!("{t}"));
        }
    });
    ui.weak(trf!(
        "Die Vorlage wird {}×{} Pixel groß.",
        "The template will be {}×{} pixels.",
        f.tile * 2,
        f.tile * 3
    ));
    ui.add_space(6.0);
    if ui
        .button(RichText::new(tr("Vorlage anlegen", "Create template")).strong())
        .clicked()
    {
        let made = autotile::template(f.tile, ColorDepth::Rgba32, Palette::default())
            .map_err(|e| e.to_string())
            .and_then(|s| {
                app.doc
                    .as_mut()
                    .ok_or_else(|| tr("Kein Projekt offen", "No project open").to_string())?
                    .create_image_from(&f.name, s)
            });
        match made {
            Ok(id) => {
                app.select(id);
                // Auf der Zeichenebene weitermalen, nicht auf der Hilfsebene.
                if let Some(s) = app.sprite() {
                    app.layer = s.layer_count().saturating_sub(1);
                }
                f.id = id;
                f.name = trf!("{} Kacheln", "{} tiles", f.name);
                f.cache = None;
                app.info(tr(
                    "Vorlage angelegt. Male auf der Ebene „Gelände“.",
                    "Template created. Paint on the layer \"Terrain\".",
                ));
            }
            Err(err) => app.dialog_error = Some(err),
        }
    }
}

fn template(app: &mut App, ui: &mut egui::Ui, f: &mut TileForm, sprite: &Sprite) {
    let source = app
        .doc
        .as_ref()
        .and_then(|d| d.asset_name(f.id))
        .unwrap_or("")
        .to_string();
    ui.horizontal(|ui| {
        for k in TileSetKind::ALL {
            ui.radio_value(&mut f.kind, k, k.label());
        }
    });
    let built = autotile::build(sprite, f.kind).ok();
    if let Some((set, info)) = &built {
        preview(app, ui, f, set, info);
    } else {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            tr(
                "Die Vorlage hat die falsche Größe.",
                "The template has the wrong size.",
            ),
        );
    }
    section(ui, tr("Satz anlegen", "Create set"));
    ui.horizontal(|ui| {
        ui.label(tr("Name des Satzes", "Name of the set"));
        ui.text_edit_singleline(&mut f.name);
    });
    let existing = app.doc.as_ref().and_then(|d| d.image_named(&f.name));
    let label = if existing.is_some() {
        tr("Satz neu bauen", "Rebuild set")
    } else {
        tr("Satz anlegen", "Create set")
    };
    if ui.button(RichText::new(label).strong()).clicked() {
        if let Some((mut set, mut info)) = built {
            info.source = Some(source);
            set.set_tileset(Some(info));
            store(app, f, existing, set);
        }
    }
    ui.weak(tr(
        "Ändert sich die Vorlage, baust du den Satz einfach neu. Mehrere Frames ergeben animierte Kacheln.",
        "When the template changes, simply rebuild the set. Several frames give animated tiles.",
    ));
}

/// Legt den Satz als Bild an oder ersetzt einen vorhandenen Satz.
fn store(app: &mut App, f: &mut TileForm, existing: Option<AssetId>, set: Sprite) {
    match existing {
        Some(id) => {
            let is_set = app
                .doc
                .as_ref()
                .and_then(|d| d.sprite(id))
                .is_some_and(|s| s.tileset().is_some());
            if !is_set {
                app.dialog_error = Some(trf!(
                    "Es gibt schon ein Bild „{}“, das kein Kachelsatz ist. Nimm einen anderen Namen.",
                    "There is already an image \"{}\" that is not a tile set. Pick another name.",
                    f.name
                ));
                return;
            }
            app.select(id);
            if app
                .edit(|s| {
                    *s = set;
                    Ok(())
                })
                .is_some()
            {
                f.id = id;
                app.info(tr("Kachelsatz neu gebaut", "Tile set rebuilt"));
            }
        }
        None => {
            let made = app
                .doc
                .as_mut()
                .ok_or_else(|| tr("Kein Projekt offen", "No project open").to_string())
                .and_then(|d| d.create_image_from(&f.name, set));
            match made {
                Ok(id) => {
                    app.select(id);
                    f.id = id;
                    app.info(tr("Kachelsatz angelegt", "Tile set created"));
                }
                Err(err) => app.dialog_error = Some(err),
            }
        }
    }
    f.cache = None;
}

fn set(app: &mut App, ui: &mut egui::Ui, f: &mut TileForm, sprite: &Sprite, info: &TileSet) {
    ui.label(trf!(
        "Kachelsatz mit {} Kacheln zu {}×{} Pixeln ({}).",
        "Tile set with {} tiles of {}×{} pixels ({}).",
        info.masks.len(),
        info.tile,
        info.tile,
        info.kind.label()
    ));
    preview(app, ui, f, sprite, info);
    if let Some(src) = &info.source {
        let template = app.doc.as_ref().and_then(|d| d.image_named(src));
        if let Some(tid) = template {
            if ui
                .button(trf!("Aus „{src}“ neu bauen", "Rebuild from \"{src}\""))
                .clicked()
            {
                let built = app
                    .doc
                    .as_ref()
                    .and_then(|d| d.sprite(tid))
                    .map(|t| autotile::build(t, info.kind));
                match built {
                    Some(Ok((mut set, mut fresh))) => {
                        fresh.source = Some(src.clone());
                        set.set_tileset(Some(fresh));
                        let id = f.id;
                        store(app, f, Some(id), set);
                    }
                    Some(Err(err)) => app.dialog_error = Some(err.to_string()),
                    None => {}
                }
            }
        }
    }
    section(ui, tr("Für die Spiel-Engine", "For the game engine"));
    ui.horizontal(|ui| {
        ui.label(tr("Vergrößerung", "Scale"));
        ui.add(
            egui::DragValue::new(&mut f.scale)
                .range(1..=MAX_SCALE)
                .suffix("×"),
        );
    });
    ui.horizontal(|ui| {
        for (engine, label) in [
            (
                TileEngine::Godot,
                tr("Für Godot 4 exportieren", "Export for Godot 4"),
            ),
            (
                TileEngine::Tiled,
                tr("Für Tiled exportieren", "Export for Tiled"),
            ),
        ] {
            if ui.button(label).clicked() {
                let done = app
                    .doc
                    .as_ref()
                    .map(|d| d.export_tileset(f.id, engine, f.scale));
                match done {
                    Some(Ok(files)) => {
                        f.written = files;
                        app.info(tr("Kachelsatz exportiert", "Tile set exported"));
                    }
                    Some(Err(err)) => app.dialog_error = Some(err),
                    None => {}
                }
            }
        }
        if let Some(dir) = f.written.first().and_then(|p| p.parent()) {
            if ui.button(tr("Ordner öffnen", "Open folder")).clicked() {
                crate::help::open_folder(dir);
            }
        }
    });
    ui.weak(tr(
        "Godot: beide Dateien in dasselbe Projektverzeichnis legen, die .tres als TileSet einer TileMapLayer laden und im Reiter Terrains malen. Tiled: die .tsx zur Karte hinzufügen und mit dem Terrain-Pinsel malen.",
        "Godot: put both files into the same project folder, load the .tres as the TileSet of a TileMapLayer and paint in the Terrains tab. Tiled: add the .tsx to the map and paint with the terrain brush.",
    ));
    for p in &f.written {
        ui.monospace(p.display().to_string());
    }
}

/// Links der Satz, rechts eine Beispielkarte damit.
fn preview(app: &App, ui: &mut egui::Ui, f: &mut TileForm, set: &Sprite, info: &TileSet) {
    let revision = app.doc.as_ref().map_or(0, |d| d.revision());
    let key = (revision, f.id, f.kind, f.seed);
    if f.cache.as_ref().map(|c| c.0) != Some(key) {
        f.cache = image(set, info, f.seed).map(|(img, size)| {
            let tex = ui
                .ctx()
                .load_texture("autotile-preview", img, egui::TextureOptions::NEAREST);
            (key, tex, size)
        });
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(PREVIEW_W, PREVIEW_H), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_rgb(36, 40, 52));
    if let Some((_, tex, (w, h))) = &f.cache {
        let scale = ((PREVIEW_W - 16.0) / w).min((PREVIEW_H - 16.0) / h);
        let scale = if scale >= 1.0 { scale.floor() } else { scale };
        let r = egui::Rect::from_center_size(rect.center(), egui::vec2(*w, *h) * scale);
        painter.image(
            tex.id(),
            r,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if ui
        .button(tr("Andere Beispielkarte", "Another sample map"))
        .clicked()
    {
        f.seed = f.seed.wrapping_add(1);
    }
}

fn image(set: &Sprite, info: &TileSet, seed: u64) -> Option<(egui::ColorImage, (f32, f32))> {
    let (map, mw, mh) = autotile::demo_map(set, info, 0, 12, 9, seed).ok()?;
    let px = set.composite(0).ok()?;
    let (sw, sh) = (set.width() as usize, set.height() as usize);
    let gap = info.tile as usize / 2;
    let (mw, mh) = (mw as usize, mh as usize);
    let (w, h) = (sw + gap + mw, sh.max(mh));
    let water = Rgba::rgb(52, 92, 160);
    let mut out = vec![Color32::TRANSPARENT; w * h];
    for y in 0..sh {
        for x in 0..sw {
            out[y * w + x] = c32(px[y * sw + x]);
        }
    }
    for y in 0..mh {
        for x in 0..mw {
            out[y * w + sw + gap + x] = c32(map[y * mw + x].over(water));
        }
    }
    Some((egui::ColorImage::new([w, h], out), (w as f32, h as f32)))
}
