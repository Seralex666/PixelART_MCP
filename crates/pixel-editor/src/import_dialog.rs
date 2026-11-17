//! Dialog „Bild importieren“: holt PNG, JPG und GIF als Bild, als neue Figur,
//! als Animation einer vorhandenen Figur oder als Vorlage zum Nachzeichnen ins
//! Projekt. Fotos und Zeichnungen lassen sich dabei in Pixel-Art umwandeln.

use crate::app::{c32, section, App};
use crate::dialogs::{buttons, finish, kind_label, Outcome};
use crate::doc::ImportAs;
use eframe::egui::{self, RichText};
use pixel_core::import::{self, Grid, NewColors, Options, Picture, PictureFrame, Prepared, Shrink};
use pixel_core::lang::tr;
use pixel_core::pixelate::{self, Background, Dither, Outline, Pixelate};
use pixel_core::trf;
use pixel_core::{AnimationKind, ColorDepth, Rgba, MAX_SIZE};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// So viele Frames zeigt die Vorschau höchstens.
const PREVIEW_FRAMES: usize = 12;
/// Kantenlänge eines Vorschaubilds in Punkten.
const PREVIEW_SIZE: f32 = 72.0;
/// Kantenlänge der Vorher-Nachher-Vorschau beim Umwandeln.
const COMPARE_SIZE: f32 = 180.0;
/// Größere Fotos werden beim Laden einmal auf diese Kantenlänge gebracht, damit
/// die Vorschau beim Verstellen flüssig bleibt.
const WORK_SIDE: u32 = 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Image,
    Character,
    Animation,
    /// Vorlage-Ebene im gerade geöffneten Bild.
    Reference,
}

/// Woher die Farben einer Umwandlung kommen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteFrom {
    /// Die besten Farben des Bildes selbst.
    Image,
    /// Die Palette des Projekt-Stils.
    Project,
    /// Die Palette der Figur, zu der die Animation gehört.
    Character,
}

/// Einstellungen für „Foto oder Zeichnung umwandeln“.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Convert {
    on: bool,
    width: u32,
    height: u32,
    keep_ratio: bool,
    colors: u32,
    from: PaletteFrom,
    dither: Dither,
    remove_bg: bool,
    tolerance: u8,
    outline: bool,
    vivid: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Split {
    Whole,
    Count,
    Cells,
}

/// Alles, wovon das zugeschnittene Ergebnis abhängt. Ändert sich etwas davon,
/// wird neu gerechnet, sonst nicht.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cut {
    auto_shrink: bool,
    factor: u32,
    split: Split,
    columns: u32,
    rows: u32,
    cell_w: u32,
    cell_h: u32,
    keep_empty: bool,
    /// Bei Animationen: die Größe der Figur, in die die Frames passen müssen.
    /// Bei einer Vorlage: die Größe der Leinwand.
    fit: Option<(u32, u32)>,
    convert: Convert,
    /// Als Vorlage, mit so vielen Frames wie das Bild hat.
    reference: Option<usize>,
}

pub struct ImportForm {
    path: String,
    /// Der Pfad, zu dem `file` gehört.
    loaded_path: String,
    /// Die gelesene Datei oder der Grund, warum sie sich nicht lesen ließ.
    file: Option<Result<Picture, String>>,
    name: String,
    animation_name: String,
    target: Target,
    character: Option<Uuid>,
    /// Index in [`AnimationKind::PRESETS`]; `None` heißt eigene Art.
    kind: Option<usize>,
    custom: String,
    /// Bei einer neuen Figur mit mehreren Frames: alle auch als Animation anlegen.
    with_animation: bool,
    bits: u8,
    snap_colors: bool,
    cut: Cut,
    ready: Option<(Cut, Result<Prepared, String>)>,
    thumbs: Vec<egui::TextureHandle>,
    /// Bei großen Fotos eine verkleinerte Arbeitskopie für die Umwandlung.
    work: Option<Picture>,
    /// Das Original für die Vorher-Ansicht.
    before: Option<egui::TextureHandle>,
    /// Deckkraft einer Vorlage, 0 bis 255.
    opacity: u8,
}

impl ImportForm {
    /// `path`: eine Datei, die gleich gelesen wird (zum Beispiel ins Fenster gezogen).
    /// `character`: die Figur, an der gerade gearbeitet wird.
    pub fn new(path: Option<PathBuf>, character: Option<Uuid>) -> Self {
        let mut form = Self {
            path: path.map(|p| p.display().to_string()).unwrap_or_default(),
            loaded_path: String::new(),
            file: None,
            name: String::new(),
            animation_name: String::new(),
            target: Target::Image,
            character,
            kind: Some(0),
            custom: String::new(),
            with_animation: true,
            bits: 32,
            snap_colors: false,
            cut: Cut {
                auto_shrink: true,
                factor: 1,
                split: Split::Whole,
                columns: 4,
                rows: 1,
                cell_w: 32,
                cell_h: 32,
                keep_empty: false,
                fit: None,
                convert: Convert {
                    on: false,
                    width: 64,
                    height: 64,
                    keep_ratio: true,
                    colors: 16,
                    from: PaletteFrom::Image,
                    dither: Dither::None,
                    remove_bg: false,
                    tolerance: 12,
                    outline: false,
                    vivid: false,
                },
                reference: None,
            },
            ready: None,
            thumbs: Vec::new(),
            work: None,
            before: None,
            opacity: 128,
        };
        if !form.path.is_empty() {
            form.load();
        }
        form
    }

    fn load(&mut self) {
        // „Als Pfad kopieren“ im Windows-Explorer setzt Anführungszeichen um den Pfad.
        let path = self.path.trim().trim_matches('"').trim().to_string();
        self.path = path.clone();
        self.loaded_path = path.clone();
        self.ready = None;
        self.thumbs.clear();
        self.work = None;
        self.before = None;
        let file = import::read(Path::new(&path)).map_err(|e| e.to_string());
        if let Ok(picture) = &file {
            self.prepare_convert(picture);
        }
        if file.is_ok() {
            let stem = Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Steht die Art schon im Dateinamen („ritter-walk.png“), gleich vorwählen.
            let lower = stem.to_lowercase();
            if let Some(i) = AnimationKind::PRESETS
                .iter()
                .position(|k| lower.contains(k.name()))
            {
                self.kind = Some(i);
            }
            self.name = stem;
            self.animation_name.clear();
            self.cut.split = Split::Whole;
        }
        self.file = Some(file);
    }

    /// Stellt die Umwandlung passend zur Datei ein: Ein großes Bild mit vielen
    /// Farben ist wohl ein Foto oder eine Zeichnung und wird gleich umgewandelt.
    fn prepare_convert(&mut self, picture: &Picture) {
        let (w, h) = (picture.width, picture.height);
        (self.cut.convert.width, self.cut.convert.height) = pixelate::fit_size(w, h, 64);
        let colors: std::collections::HashSet<Rgba> = picture
            .frames
            .first()
            .map(|f| f.pixels.iter().copied().take(1 << 20).collect())
            .unwrap_or_default();
        self.cut.convert.on = w.max(h) > 256 && colors.len() > 256 && picture.pixel_scale() == 1;
        if w.max(h) > WORK_SIDE {
            let (ww, wh) = pixelate::fit_size(w, h, WORK_SIDE);
            self.work = Some(Picture {
                width: ww,
                height: wh,
                frames: picture
                    .frames
                    .iter()
                    .map(|f| PictureFrame {
                        pixels: pixelate::resample(&f.pixels, w, h, ww, wh, false),
                        duration_ms: f.duration_ms,
                    })
                    .collect(),
                timed: picture.timed,
            });
        }
    }

    fn kind(&self) -> AnimationKind {
        match self.kind.and_then(|i| AnimationKind::PRESETS.get(i)) {
            Some(k) => k.clone(),
            None => AnimationKind::Custom(self.custom.trim().to_string()),
        }
    }

    fn kind_is_valid(&self) -> bool {
        self.kind.is_some() || !self.custom.trim().is_empty()
    }
}

/// Die Vorlage: jedes Bild auf die Leinwand eingepasst, ohne Farbreduktion.
fn reference_picture(picture: &Picture, cut: Cut, frames: usize) -> Result<Prepared, String> {
    let (cw, ch) = cut.fit.unwrap_or((picture.width, picture.height));
    let (w, h) = (picture.width, picture.height);
    // Hat die Datei so viele Frames wie das Bild, bekommt jeder Frame seine Vorlage.
    let take = if picture.frames.len() == frames {
        frames
    } else {
        1
    };
    let frames = picture
        .frames
        .iter()
        .take(take)
        .map(|f| {
            let mut px = f.pixels.clone();
            if cut.convert.remove_bg {
                pixelate::remove_background(&mut px, w, h, Background::Auto, cut.convert.tolerance);
            }
            PictureFrame {
                pixels: pixelate::reference(&px, w, h, cw, ch),
                duration_ms: f.duration_ms,
            }
        })
        .collect();
    Ok(Prepared {
        picture: Picture {
            width: cw,
            height: ch,
            frames,
            timed: false,
        },
        shrunk_by: 1,
        dropped_empty: 0,
        background_removed: 0,
    })
}

/// Die Einstellungen der Umwandlung für den Kern.
fn pixelate_options(cut: &Cut, palette: Option<&[Rgba]>) -> Pixelate {
    let c = cut.convert;
    let (width, height) = cut.fit.unwrap_or((c.width, c.height));
    Pixelate {
        width,
        height,
        colors: c.colors as usize,
        palette: palette.map(<[Rgba]>::to_vec),
        dither: c.dither,
        background: if c.remove_bg {
            Background::Auto
        } else {
            Background::Keep
        },
        tolerance: c.tolerance,
        outline: if c.outline {
            Outline::Auto
        } else {
            Outline::None
        },
        vivid: c.vivid,
    }
}

/// Verkleinert und teilt die Datei so, wie es im Dialog eingestellt ist.
/// `palette`: feste Farben für die Umwandlung.
fn cut_picture(picture: &Picture, cut: Cut, palette: Option<&[Rgba]>) -> Result<Prepared, String> {
    if let Some(frames) = cut.reference {
        return reference_picture(picture, cut, frames);
    }
    // Ein GIF bringt seine Frames mit und wird nicht noch einmal geteilt.
    let grid = match cut.split {
        _ if picture.frames.len() > 1 => Grid::Whole,
        Split::Whole => Grid::Whole,
        Split::Count => Grid::Count {
            columns: cut.columns,
            rows: cut.rows,
        },
        Split::Cells => Grid::Cells {
            width: cut.cell_w,
            height: cut.cell_h,
        },
    };
    let options = Options {
        shrink: if cut.auto_shrink {
            Shrink::Auto
        } else {
            Shrink::By(cut.factor)
        },
        grid,
        keep_empty: cut.keep_empty,
        pixelate: cut.convert.on.then(|| pixelate_options(&cut, palette)),
    };
    let mut ready = import::prepare(picture, options).map_err(|e| e.to_string())?;
    if let Some((w, h)) = cut.fit {
        let (fw, fh) = (ready.picture.width, ready.picture.height);
        if grid == Grid::Whole {
            ready.cut_to_frames(w, h, cut.keep_empty).map_err(|_| {
                trf!(
                    "Die Datei ist {fw}×{fh} Pixel groß. Das lässt sich nicht in Frames von {w}×{h} Pixeln teilen, der Größe der Figur. Passe das Verkleinern oder die Aufteilung an.", "The file is {fw}×{fh} pixels. That cannot be split into frames of {w}×{h} pixels, the size of the character. Adjust the shrinking or the split."
                )
            })?;
        }
        let (fw, fh) = (ready.picture.width, ready.picture.height);
        if (fw, fh) != (w, h) {
            return Err(trf!(
                "Die Frames wären {fw}×{fh} Pixel groß, die Figur ist {w}×{h}. Eine Animation muss so groß sein wie ihre Figur.", "The frames would be {fw}×{fh} pixels, the character is {w}×{h}. An animation must be the size of its character."
            ));
        }
    }
    let (w, h) = (ready.picture.width, ready.picture.height);
    if w > MAX_SIZE || h > MAX_SIZE {
        return Err(trf!(
            "{w}×{h} Pixel ist größer als die erlaubten {MAX_SIZE}×{MAX_SIZE}. Verkleinere das Bild oder teile es in Frames.", "{w}×{h} pixels is larger than the allowed {MAX_SIZE}×{MAX_SIZE}. Shrink the picture or split it into frames."
        ));
    }
    Ok(ready)
}

fn frames_text(n: usize) -> String {
    if n == 1 {
        tr("1 Frame", "1 frame").into()
    } else {
        trf!("{n} Frames", "{n} frames")
    }
}

fn kind_picker(ui: &mut egui::Ui, kind: &mut Option<usize>, custom: &mut String) {
    let selected = kind
        .and_then(|i| AnimationKind::PRESETS.get(i))
        .map_or(tr("Eigene", "Custom"), kind_label);
    egui::ComboBox::from_id_salt("import-kind")
        .selected_text(selected)
        .width(200.0)
        .show_ui(ui, |ui| {
            for (i, k) in AnimationKind::PRESETS.iter().enumerate() {
                ui.selectable_value(kind, Some(i), kind_label(k));
            }
            ui.selectable_value(kind, None, tr("Eigene", "Custom"));
        });
    if kind.is_none() {
        ui.add(
            egui::TextEdit::singleline(custom)
                .hint_text(tr(
                    "Bezeichnung, zum Beispiel winken",
                    "Name, for example wave",
                ))
                .desired_width(200.0),
        );
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, f: &mut ImportForm) -> Outcome {
    ui.set_width(460.0);
    let Some(doc) = &app.doc else {
        return Outcome::Close;
    };
    let characters: Vec<(Uuid, String, (u32, u32))> = doc
        .project()
        .characters
        .iter()
        .map(|c| (c.id, c.name.clone(), c.size()))
        .collect();

    let style_palette = doc.project().style.palette.clone();
    // Das offene Bild, das eine Vorlage bekommen kann.
    let open_asset = app.sel.and_then(|id| {
        let s = doc.sprite(id)?;
        Some((
            doc.asset_name(id)?.to_string(),
            (s.width(), s.height()),
            s.frames().len(),
        ))
    });
    let character_palette = |id: Option<Uuid>| {
        doc.project()
            .characters
            .iter()
            .find(|c| Some(c.id) == id)
            .map(|c| c.palette().colors.clone())
    };
    let hero_palette = character_palette(f.character);

    ui.heading(tr("Bild importieren", "Import image"));
    ui.label(tr("Holt eine PNG-, JPG- oder GIF-Datei ins Projekt. Vergrößerte Pixel-Art wird auf ihre echte Größe gebracht, Spritesheets werden in Frames geteilt, Fotos und Zeichnungen lassen sich in Pixel-Art umwandeln. Du kannst die Datei auch einfach ins Fenster ziehen.", "Brings a PNG, JPG or GIF file into the project. Enlarged pixel art is brought back to its true size, sprite sheets are split into frames, photos and drawings can be turned into pixel art. You can also just drag the file into the window."));

    section(ui, tr("Datei", "File"));
    let mut loaded_now = false;
    ui.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut f.path)
                .hint_text(tr("Pfad zur Datei", "Path to the file"))
                .desired_width(290.0),
        );
        let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        // Beim Öffnen ohne Datei gleich einen Pfad tippen oder einfügen können.
        if f.path.is_empty() && !resp.has_focus() && ui.memory(|m| m.focused().is_none()) {
            resp.request_focus();
        }
        if ui.button(tr("Durchsuchen…", "Browse…")).clicked() {
            let picked = rfd::FileDialog::new()
                .set_title(tr("Bild importieren", "Import image"))
                .add_filter(
                    tr("Bilder (PNG, JPG, GIF)", "Images (PNG, JPG, GIF)"),
                    &["png", "jpg", "jpeg", "gif"],
                )
                .pick_file();
            if let Some(path) = picked {
                f.path = path.display().to_string();
                f.load();
                loaded_now = true;
            }
        }
        let changed = f.path.trim() != f.loaded_path;
        let load = changed && !f.path.trim().is_empty();
        if (load && ui.button(tr("Laden", "Load")).clicked()) || (load && entered) {
            f.load();
            loaded_now = true;
        }
    });
    if loaded_now {
        app.dialog_error = None;
    }
    // Eine Datei, die auf den offenen Dialog gezogen wird, ersetzt die bisherige.
    if let Some(path) = ui.input(|i| i.raw.dropped_files.first().map(|d| d.path().to_path_buf())) {
        f.path = path.display().to_string();
        f.load();
        loaded_now = true;
    }

    let picture = match &f.file {
        Some(Ok(picture)) if f.path.trim() == f.loaded_path => picture,
        other => {
            if let Some(Err(err)) = other {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            let (_, cancel) = buttons(ui, tr("Importieren", "Import"), false);
            return if cancel {
                Outcome::Close
            } else {
                Outcome::Keep
            };
        }
    };
    let animated = picture.frames.len() > 1;
    ui.weak(trf!(
        "{}×{} Pixel{}",
        "{}×{} pixels{}",
        picture.width,
        picture.height,
        if animated {
            format!(", {}", frames_text(picture.frames.len()))
        } else {
            String::new()
        }
    ));

    section(ui, tr("Importieren als", "Import as"));
    ui.horizontal(|ui| {
        ui.radio_value(&mut f.target, Target::Image, tr("Bild", "Image"))
            .on_hover_text(tr(
                "Ein freies Bild: Tile, Item oder Szene",
                "A free image: tile, item or scene",
            ));
        ui.radio_value(
            &mut f.target,
            Target::Character,
            tr("Neue Figur", "New character"),
        )
        .on_hover_text(tr(
            "Die Datei legt Größe und Palette der Figur fest",
            "The file sets size and palette of the character",
        ));
        ui.add_enabled_ui(!characters.is_empty(), |ui| {
            ui.radio_value(
                &mut f.target,
                Target::Animation,
                tr("Animation einer Figur", "Animation of a character"),
            )
            .on_hover_text(tr(
                "Die Frames übernehmen Größe und Palette der Figur",
                "The frames take over size and palette of the character",
            ))
            .on_disabled_hover_text(tr(
                "Dafür braucht das Projekt erst eine Figur",
                "For this the project needs a character first",
            ));
        });
        ui.add_enabled_ui(open_asset.is_some(), |ui| {
            ui.radio_value(&mut f.target, Target::Reference, tr("Vorlage", "Reference"))
                .on_hover_text(tr(
                    "Eine halb durchsichtige Ebene im offenen Bild zum Nachzeichnen. Sie wird nicht exportiert.",
                    "A semi-transparent layer in the open image to trace over. It is not exported.",
                ))
                .on_disabled_hover_text(tr(
                    "Dafür erst ein Bild oder eine Figur öffnen",
                    "Open an image or a character first",
                ));
        });
    });
    if f.target == Target::Reference && open_asset.is_none() {
        f.target = Target::Image;
    }
    if f.target == Target::Animation {
        if !characters.iter().any(|(id, ..)| Some(*id) == f.character) {
            f.character = characters.first().map(|(id, ..)| *id);
        }
        f.cut.fit = characters
            .iter()
            .find(|(id, ..)| Some(*id) == f.character)
            .map(|(.., size)| *size);
        f.cut.reference = None;
    } else if let (Target::Reference, Some((_, size, frames))) = (f.target, &open_asset) {
        f.cut.fit = Some(*size);
        f.cut.reference = Some(*frames);
    } else {
        f.cut.fit = None;
        f.cut.reference = None;
    }
    let reference = f.target == Target::Reference;
    // Die Palette der Figur gibt es nur bei einer Animation.
    if f.cut.convert.from == PaletteFrom::Character && f.target != Target::Animation
        || f.cut.convert.from == PaletteFrom::Project && style_palette.is_empty()
    {
        f.cut.convert.from = PaletteFrom::Image;
    }

    let several = animated_or_split(picture, &f.cut);
    let kind_name = f.kind().name().to_string();
    egui::Grid::new("import-target")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| match f.target {
            Target::Reference => {
                ui.label(tr("Für", "For"));
                ui.label(open_asset.as_ref().map_or(String::new(), |(name, (w, h), _)| format!("{name} ({w}×{h})")));
                ui.end_row();
                ui.label(tr("Deckkraft", "Opacity"));
                let mut pct = u32::from(f.opacity) * 100 / 255;
                if ui.add(egui::Slider::new(&mut pct, 10..=90).suffix(" %")).changed() {
                    f.opacity = (pct * 255 / 100) as u8;
                }
                ui.end_row();
                ui.label(tr("Hintergrund", "Background"));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.cut.convert.remove_bg, tr("entfernen", "remove"));
                    if f.cut.convert.remove_bg {
                        ui.add(egui::Slider::new(&mut f.cut.convert.tolerance, 1..=60).text(tr("Toleranz", "Tolerance")));
                    }
                });
                ui.end_row();
            }
            Target::Image | Target::Character => {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(200.0));
                ui.end_row();
                ui.label(tr("Farbtiefe", "Color depth"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut f.bits, 8, "8 Bit")
                        .on_hover_text(tr("Höchstens 256 Farben. Hat die Datei mehr, werden ähnliche zusammengelegt.", "At most 256 colors. If the file has more, similar ones are merged."));
                    ui.radio_value(&mut f.bits, 16, "16 Bit")
                        .on_hover_text(tr("65 536 Farben, Pixel sind deckend oder durchsichtig", "65,536 colors, pixels are opaque or transparent"));
                    ui.radio_value(&mut f.bits, 32, "32 Bit")
                        .on_hover_text(tr("Alle Farben mit Transparenz", "All colors with transparency"));
                });
                ui.end_row();
                if f.target == Target::Character && several {
                    ui.label("Frames");
                    ui.checkbox(&mut f.with_animation, tr("auch als Animation anlegen", "also create an animation"))
                        .on_hover_text(tr("Der erste Frame wird die Grundpose. Mit Haken werden alle Frames zusätzlich eine Animation der neuen Figur.", "The first frame becomes the base pose. When checked, all frames also become an animation of the new character."));
                    ui.end_row();
                    if f.with_animation {
                        ui.label(tr("Art", "Kind"));
                        ui.vertical(|ui| kind_picker(ui, &mut f.kind, &mut f.custom));
                        ui.end_row();
                    }
                }
            }
            Target::Animation => {
                ui.label(tr("Figur", "Character"));
                let current = characters
                    .iter()
                    .find(|(id, ..)| Some(*id) == f.character)
                    .map_or(String::new(), |(_, name, (w, h))| format!("{name} ({w}×{h})"));
                egui::ComboBox::from_id_salt("import-character")
                    .selected_text(current)
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (id, name, (w, h)) in &characters {
                            ui.selectable_value(
                                &mut f.character,
                                Some(*id),
                                format!("{name} ({w}×{h})"),
                            );
                        }
                    });
                ui.end_row();
                ui.label(tr("Art", "Kind"));
                ui.vertical(|ui| kind_picker(ui, &mut f.kind, &mut f.custom));
                ui.end_row();
                ui.label("Name");
                let hero = characters
                    .iter()
                    .find(|(id, ..)| Some(*id) == f.character)
                    .map_or("", |(_, name, _)| name.as_str());
                ui.add(
                    egui::TextEdit::singleline(&mut f.animation_name)
                        .hint_text(format!("{hero} {kind_name}"))
                        .desired_width(200.0),
                );
                ui.end_row();
                ui.label(tr("Neue Farben", "New colors"));
                ui.vertical(|ui| {
                    ui.radio_value(&mut f.snap_colors, false, tr("in die Palette der Figur aufnehmen", "add to the character's palette"));
                    ui.radio_value(&mut f.snap_colors, true, tr("an die vorhandenen Farben anpassen", "snap to the existing colors"));
                });
                ui.end_row();
            }
        });

    if !reference {
        section(ui, tr("Art der Datei", "Kind of file"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut f.cut.convert.on, false, tr("Pixel-Art", "Pixel art"))
                .on_hover_text(tr(
                    "Jeder Pixel wird genau übernommen",
                    "Every pixel is taken over exactly",
                ));
            ui.radio_value(
                &mut f.cut.convert.on,
                true,
                tr(
                    "Foto oder Zeichnung umwandeln",
                    "Convert a photo or drawing",
                ),
            )
            .on_hover_text(tr(
                "Auf eine kleine Größe und wenige Farben bringen, wie echte Pixel-Art",
                "Bring it to a small size and few colors, like real pixel art",
            ));
        });
    }
    if f.cut.convert.on && !reference {
        convert_options(
            ui,
            &mut f.cut,
            f.target,
            (picture.width, picture.height),
            !style_palette.is_empty(),
        );
    }
    if !f.cut.convert.on && !reference {
        section(ui, tr("Größe", "Size"));
        ui.horizontal(|ui| {
        ui.checkbox(&mut f.cut.auto_shrink, tr("Vergrößerte Pixel-Art erkennen", "Detect enlarged pixel art"))
            .on_hover_text(tr("Besteht das Bild aus gleich großen einfarbigen Blöcken, wird jeder Block zu einem Pixel.", "If the picture consists of equally sized single-color blocks, each block becomes one pixel."));
        if !f.cut.auto_shrink {
            ui.add(
                egui::DragValue::new(&mut f.cut.factor)
                    .range(1..=64)
                    .prefix(tr("verkleinern um ", "shrink by "))
                    .suffix("×"),
            );
        }
    });
    }

    if !animated && !f.cut.convert.on && !reference {
        section(ui, "Frames");
        ui.horizontal(|ui| {
            let whole = if f.cut.fit.is_some() {
                tr("Nach Größe der Figur", "By character size")
            } else {
                tr("Ein Bild", "One picture")
            };
            ui.radio_value(&mut f.cut.split, Split::Whole, whole);
            ui.radio_value(
                &mut f.cut.split,
                Split::Count,
                tr("Spalten × Zeilen", "Columns × rows"),
            );
            ui.radio_value(
                &mut f.cut.split,
                Split::Cells,
                tr("Framegröße", "Frame size"),
            );
        });
        match f.cut.split {
            Split::Whole => {}
            Split::Count => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut f.cut.columns)
                            .range(1..=256)
                            .suffix(tr(" Spalten", " columns")),
                    );
                    ui.add(
                        egui::DragValue::new(&mut f.cut.rows)
                            .range(1..=256)
                            .suffix(tr(" Zeilen", " rows")),
                    );
                });
            }
            Split::Cells => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut f.cut.cell_w)
                            .range(1..=MAX_SIZE)
                            .prefix(tr("Breite ", "Width ")),
                    );
                    ui.add(
                        egui::DragValue::new(&mut f.cut.cell_h)
                            .range(1..=MAX_SIZE)
                            .prefix(tr("Höhe ", "Height ")),
                    );
                    ui.weak(tr("Pixel, nach dem Verkleinern", "pixels, after shrinking"));
                });
            }
        }
        if f.cut.split != Split::Whole || f.cut.fit.is_some() {
            ui.checkbox(
                &mut f.cut.keep_empty,
                tr(
                    "Leere Frames am Ende behalten",
                    "Keep empty frames at the end",
                ),
            );
        }
    }

    // Erst nach den Eingaben rechnen, damit die Vorschau zu ihnen passt.
    if f.ready.as_ref().map(|(cut, _)| *cut) != Some(f.cut) {
        let fixed = match f.cut.convert.from {
            PaletteFrom::Image => None,
            PaletteFrom::Project => Some(style_palette.as_slice()),
            PaletteFrom::Character => hero_palette.as_deref(),
        };
        let mut cut = f.cut;
        if cut.convert.on {
            cut.split = Split::Whole;
        }
        // Umwandlung und Vorlage rechnen mit der verkleinerten Arbeitskopie.
        let source = match &f.work {
            Some(work) if cut.convert.on || reference => work,
            _ => picture,
        };
        let ready = cut_picture(source, cut, fixed);
        f.thumbs = match &ready {
            Ok(ready) => ready
                .picture
                .frames
                .iter()
                .take(PREVIEW_FRAMES)
                .enumerate()
                .map(|(i, frame)| {
                    let size = [ready.picture.width as usize, ready.picture.height as usize];
                    let pixels = frame.pixels.iter().map(|&p| c32(p)).collect();
                    ui.ctx().load_texture(
                        format!("import-{i}"),
                        egui::ColorImage::new(size, pixels),
                        egui::TextureOptions::NEAREST,
                    )
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        f.ready = Some((f.cut, ready));
    }

    section(ui, tr("Ergebnis", "Result"));
    let mut summary = None;
    match f.ready.as_ref().map(|(_, ready)| ready) {
        Some(Ok(ready)) => {
            let p = &ready.picture;
            let mut text = trf!(
                "{} mit {}×{} Pixeln",
                "{} of {}×{} pixels",
                frames_text(p.frames.len()),
                p.width,
                p.height
            );
            if ready.shrunk_by > 1 {
                text.push_str(&trf!(", {}× verkleinert", ", shrunk {}×", ready.shrunk_by));
            }
            if ready.dropped_empty > 0 {
                text.push_str(&trf!(
                    ", {} am Ende weggelassen",
                    ", {} at the end left out",
                    match ready.dropped_empty {
                        1 => tr("1 leerer Frame", "1 empty frame").to_string(),
                        n => trf!("{n} leere Frames", "{n} empty frames"),
                    }
                ));
            }
            ui.label(text);
            if p.timed {
                ui.weak(tr(
                    "Die Dauer jedes Frames kommt aus dem GIF.",
                    "The duration of each frame comes from the GIF.",
                ));
            }
            let only_first =
                f.target == Target::Character && p.frames.len() > 1 && !f.with_animation;
            if only_first {
                ui.weak(tr(
                    "Nur der erste Frame wird übernommen, als Grundpose der Figur.",
                    "Only the first frame is used, as the character's base pose.",
                ));
            }
            if f.cut.convert.on || reference {
                if f.before.is_none() {
                    let src = f.work.as_ref().unwrap_or(picture);
                    let size = [src.width as usize, src.height as usize];
                    let pixels = src.frames[0].pixels.iter().map(|&p| c32(p)).collect();
                    f.before = Some(ui.ctx().load_texture(
                        "import-before",
                        egui::ColorImage::new(size, pixels),
                        egui::TextureOptions::LINEAR,
                    ));
                }
                compare(
                    ui,
                    f.before.as_ref(),
                    f.thumbs.first(),
                    (picture.width, picture.height),
                    (p.width, p.height),
                );
            } else {
                preview(ui, &f.thumbs, p.width, p.height, p.frames.len());
            }
            summary = Some((p.frames.len(), ready.shrunk_by));
        }
        Some(Err(err)) => {
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
        None => {}
    }

    let named =
        matches!(f.target, Target::Animation | Target::Reference) || !f.name.trim().is_empty();
    let needs_kind = match f.target {
        Target::Image | Target::Reference => false,
        Target::Character => f.with_animation && summary.is_some_and(|(n, _)| n > 1),
        Target::Animation => true,
    };
    let valid = summary.is_some() && named && (!needs_kind || f.kind_is_valid());
    let (ok, cancel) = buttons(ui, tr("Importieren", "Import"), valid && !loaded_now);
    if cancel {
        return Outcome::Close;
    }
    if !ok {
        return Outcome::Keep;
    }
    let (Some((_, Ok(ready))), Some(doc)) = (&f.ready, &mut app.doc) else {
        return Outcome::Keep;
    };
    if f.target == Target::Reference {
        let frames: Vec<Vec<Rgba>> = ready
            .picture
            .frames
            .iter()
            .map(|fr| fr.pixels.clone())
            .collect();
        let opacity = f.opacity;
        let wanted = f.name.trim().to_string();
        let added = app.edit(|s| {
            let base = if wanted.is_empty() {
                tr("Vorlage", "Reference").to_string()
            } else {
                wanted
            };
            let mut name = base.clone();
            let mut n = 2;
            while s.layer_index(&name).is_some() {
                name = format!("{base} {n}");
                n += 1;
            }
            s.add_reference_layer(name.clone(), &frames, opacity)
                .map_err(|e| e.to_string())?;
            Ok(name)
        });
        return match added {
            Some(name) => {
                app.layer += 1;
                app.info(trf!(
                    "Vorlage „{name}“ angelegt. Zeichne auf einer Ebene darüber; beim Export fehlt sie.",
                    "Reference \"{name}\" added. Draw on a layer above it; it is left out when exporting."
                ));
                Outcome::Close
            }
            None => Outcome::Keep,
        };
    }
    let depth = ColorDepth::from_bits(f.bits).unwrap_or(ColorDepth::Rgba32);
    let (name, target) = match f.target {
        Target::Image => (f.name.as_str(), ImportAs::Image { depth }),
        Target::Character => (
            f.name.as_str(),
            ImportAs::Character {
                depth,
                animation: f.with_animation.then(|| f.kind()),
            },
        ),
        Target::Reference => return Outcome::Keep,
        Target::Animation => {
            let Some(character) = f.character else {
                return Outcome::Keep;
            };
            (
                f.animation_name.as_str(),
                ImportAs::Animation {
                    character,
                    kind: f.kind(),
                    new_colors: if f.snap_colors {
                        NewColors::Snap
                    } else {
                        NewColors::Add
                    },
                },
            )
        }
    };
    let shrunk_by = ready.shrunk_by;
    let imported = doc.import(name, &ready.picture, target);
    finish(app, imported, |app, done| {
        app.select(done.id);
        let name = app
            .doc
            .as_ref()
            .and_then(|d| d.asset_name(done.id))
            .unwrap_or_default()
            .to_string();
        let mut text = trf!("„{name}“ importiert", "\"{name}\" imported");
        if shrunk_by > 1 {
            text.push_str(&trf!(", {shrunk_by}× verkleinert", ", shrunk {shrunk_by}×"));
        }
        if done.added_colors > 0 {
            text.push_str(&trf!(
                ", {} neue Farbe(n) in der Palette der Figur",
                ", {} new color(s) in the character's palette",
                done.added_colors
            ));
        }
        if done.report.changed > 0 {
            text.push_str(&trf!(
                ", {} von {} Farben angepasst",
                ", {} of {} colors adjusted",
                done.report.changed,
                done.report.source_colors
            ));
        }
        app.info(text);
    })
}

/// Die Einstellungen für „Foto oder Zeichnung umwandeln“.
fn convert_options(
    ui: &mut egui::Ui,
    cut: &mut Cut,
    target: Target,
    (sw, sh): (u32, u32),
    has_style: bool,
) {
    let animation = target == Target::Animation;
    let fit = cut.fit;
    let c = &mut cut.convert;
    section(ui, tr("Umwandeln", "Convert"));
    egui::Grid::new("import-convert")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(tr("Größe", "Size"));
            if let (true, Some((w, h))) = (animation, fit) {
                ui.label(trf!(
                    "{w}×{h} Pixel, wie die Figur",
                    "{w}×{h} pixels, like the character"
                ));
            } else {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let w = ui.add(egui::DragValue::new(&mut c.width).range(1..=MAX_SIZE));
                        ui.label("×");
                        let h = ui.add(egui::DragValue::new(&mut c.height).range(1..=MAX_SIZE));
                        ui.label(tr("Pixel", "pixels"));
                        if c.keep_ratio {
                            let ratio = |v: u32, a: u32, b: u32| {
                                ((f64::from(v) * f64::from(a) / f64::from(b.max(1))).round() as u32)
                                    .clamp(1, MAX_SIZE)
                            };
                            if w.changed() {
                                c.height = ratio(c.width, sh, sw);
                            } else if h.changed() {
                                c.width = ratio(c.height, sw, sh);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        for side in [32, 48, 64, 96, 128] {
                            if ui
                                .small_button(side.to_string())
                                .on_hover_text(tr(
                                    "Längere Seite in Pixeln",
                                    "Longer side in pixels",
                                ))
                                .clicked()
                            {
                                (c.width, c.height) = pixelate::fit_size(sw, sh, side);
                            }
                        }
                        if ui
                            .checkbox(&mut c.keep_ratio, tr("Seitenverhältnis", "Aspect ratio"))
                            .changed()
                            && c.keep_ratio
                        {
                            c.height = pixelate::fit_size(sw, sh, c.width.max(1)).1.max(1);
                            (c.width, c.height) = pixelate::fit_size(sw, sh, c.width.max(c.height));
                        }
                    });
                });
            }
            ui.end_row();

            ui.label(tr("Farben", "Colors"));
            ui.horizontal(|ui| {
                let label = |from: PaletteFrom| match from {
                    PaletteFrom::Image => tr("aus dem Bild", "from the picture"),
                    PaletteFrom::Project => tr("Palette des Projekts", "project palette"),
                    PaletteFrom::Character => tr("Palette der Figur", "character palette"),
                };
                egui::ComboBox::from_id_salt("import-palette")
                    .selected_text(label(c.from))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut c.from,
                            PaletteFrom::Image,
                            label(PaletteFrom::Image),
                        );
                        if has_style {
                            ui.selectable_value(
                                &mut c.from,
                                PaletteFrom::Project,
                                label(PaletteFrom::Project),
                            );
                        }
                        if animation {
                            ui.selectable_value(
                                &mut c.from,
                                PaletteFrom::Character,
                                label(PaletteFrom::Character),
                            );
                        }
                    });
                if c.from == PaletteFrom::Image {
                    ui.add(egui::Slider::new(&mut c.colors, 2..=64));
                }
            });
            ui.end_row();

            ui.label(tr("Dithering", "Dithering"));
            ui.horizontal(|ui| {
                for d in Dither::ALL {
                    let tip = match d {
                        Dither::None => tr(
                            "Klare Flächen, typisch für Pixel-Art",
                            "Clean areas, typical for pixel art",
                        ),
                        Dither::Ordered => tr(
                            "Gleichmäßiges Raster, ruhig bei Verläufen",
                            "Even pattern, calm in gradients",
                        ),
                        Dither::Diffusion => tr(
                            "Am genauesten, wirkt körniger",
                            "Most accurate, looks grainier",
                        ),
                    };
                    ui.radio_value(&mut c.dither, d, d.label())
                        .on_hover_text(tip);
                }
            });
            ui.end_row();

            ui.label(tr("Hintergrund", "Background"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut c.remove_bg, tr("entfernen", "remove"))
                    .on_hover_text(tr(
                        "Die häufigste Farbe am Rand wird von außen her durchsichtig",
                        "The most common edge color turns transparent from the outside in",
                    ));
                if c.remove_bg {
                    ui.add(
                        egui::Slider::new(&mut c.tolerance, 1..=60)
                            .text(tr("Toleranz", "Tolerance")),
                    );
                }
            });
            ui.end_row();

            ui.label(tr("Extras", "Extras"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut c.outline, tr("Umriss", "Outline"))
                    .on_hover_text(tr(
                        "Dunkler 1-Pixel-Rand um alles Deckende",
                        "Dark 1-pixel edge around everything opaque",
                    ));
                ui.checkbox(&mut c.vivid, tr("Kräftigere Farben", "Vivid colors"))
                    .on_hover_text(tr(
                        "Mehr Sättigung und Kontrast, gut für blasse Fotos",
                        "More saturation and contrast, good for pale photos",
                    ));
            });
            ui.end_row();
        });
}

/// Vorher und nachher nebeneinander, das Ergebnis pixelgenau vergrößert.
fn compare(
    ui: &mut egui::Ui,
    before: Option<&egui::TextureHandle>,
    after: Option<&egui::TextureHandle>,
    (sw, sh): (u32, u32),
    (w, h): (u32, u32),
) {
    // Hellgrau, damit Umriss und durchsichtige Stellen gut zu erkennen sind.
    let background = egui::Color32::from_gray(190);
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let fit = |w: u32, h: u32| {
        let k = COMPARE_SIZE / w.max(h).max(1) as f32;
        egui::vec2(w as f32 * k, h as f32 * k)
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (tex, size, label) in [
            (before, fit(sw, sh), tr("Vorher", "Before")),
            (after, fit(w, h), tr("Nachher", "After")),
        ] {
            ui.vertical(|ui| {
                ui.weak(label);
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(COMPARE_SIZE, COMPARE_SIZE),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rect, 2.0, background);
                if let Some(tex) = tex {
                    let img = egui::Rect::from_center_size(rect.center(), size);
                    ui.painter().image(tex.id(), img, uv, egui::Color32::WHITE);
                }
            });
        }
    });
}

/// Ob aus der Datei mehrere Frames werden können (GIF oder geteiltes Spritesheet).
fn animated_or_split(picture: &Picture, cut: &Cut) -> bool {
    picture.frames.len() > 1 || cut.split != Split::Whole
}

/// Die ersten Frames nebeneinander, pixelgenau vergrößert.
fn preview(ui: &mut egui::Ui, thumbs: &[egui::TextureHandle], w: u32, h: u32, frames: usize) {
    if thumbs.is_empty() {
        return;
    }
    let longest = w.max(h) as f32;
    // Kleine Sprites ganzzahlig vergrößern, große passend verkleinern.
    let scale = if longest <= PREVIEW_SIZE {
        (PREVIEW_SIZE / longest).floor()
    } else {
        PREVIEW_SIZE / longest
    };
    let size = egui::vec2(w as f32 * scale, h as f32 * scale);
    let background = ui.visuals().extreme_bg_color;
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for tex in thumbs {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, background);
            ui.painter().image(tex.id(), rect, uv, egui::Color32::WHITE);
        }
    });
    if frames > thumbs.len() {
        ui.weak(
            RichText::new(trf!("und {} weitere", "and {} more", frames - thumbs.len())).small(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixel_core::import::PictureFrame;
    use pixel_core::Rgba;

    /// Ein Streifen aus `frames` Feldern von 2×2 Pixeln, jeder Pixel `scale`-fach vergrößert.
    fn strip(frames: u32, scale: u32) -> Picture {
        let (w, h) = (frames * 2 * scale, 2 * scale);
        let pixels = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x / scale, y / scale)))
            .map(|(x, y)| Rgba::rgb((x * 40) as u8, (y * 90) as u8, 200))
            .collect();
        Picture {
            width: w,
            height: h,
            frames: vec![PictureFrame {
                pixels,
                duration_ms: 100,
            }],
            timed: false,
        }
    }

    fn cut() -> Cut {
        ImportForm::new(None, None).cut
    }

    #[test]
    fn an_animation_is_cut_to_the_size_of_its_character() {
        let sheet = strip(3, 4);
        let whole = cut_picture(&sheet, cut(), None).unwrap();
        assert_eq!((whole.picture.width, whole.picture.frames.len()), (6, 1));
        assert_eq!(whole.shrunk_by, 4);

        let fitted = Cut {
            fit: Some((2, 2)),
            ..cut()
        };
        let frames = cut_picture(&sheet, fitted, None).unwrap().picture;
        assert_eq!(
            (frames.width, frames.height, frames.frames.len()),
            (2, 2, 3)
        );

        // Passt die Größe nicht, sagt die Meldung, woran es liegt.
        let wrong = Cut {
            fit: Some((4, 2)),
            ..cut()
        };
        let err = cut_picture(&sheet, wrong, None).unwrap_err();
        assert!(err.contains("6×2") && err.contains("4×2"), "{err}");
        let cells = Cut {
            split: Split::Cells,
            cell_w: 3,
            cell_h: 2,
            fit: Some((2, 2)),
            ..cut()
        };
        let err = cut_picture(&sheet, cells, None).unwrap_err();
        assert!(err.contains("3×2") && err.contains("so groß sein"), "{err}");
    }

    #[test]
    fn sheets_split_by_count_and_too_large_pictures_are_refused() {
        let by_count = Cut {
            split: Split::Count,
            columns: 3,
            rows: 1,
            ..cut()
        };
        let frames = cut_picture(&strip(3, 1), by_count, None).unwrap().picture;
        assert_eq!((frames.width, frames.frames.len()), (2, 3));

        let unscaled = Cut {
            auto_shrink: false,
            factor: 1,
            ..cut()
        };
        let err = cut_picture(&strip(300, 2), unscaled, None).unwrap_err();
        assert!(err.contains("1200×4"), "{err}");
    }
}
