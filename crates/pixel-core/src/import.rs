//! Import vorhandener Bilder: PNG, JPG und GIF lesen, Spritesheets in Frames
//! teilen, hochskalierte Pixel-Art pixelgenau verkleinern und das Ergebnis als
//! Bild, Figur oder Animation ins Projekt holen.

use crate::{
    Animation, AnimationKind, Character, ColorDepth, Error, Palette, Project, Result, Rgba, Sprite,
};
use image::{AnimationDecoder, ImageDecoder};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use uuid::Uuid;

/// Größte Datei, die gelesen wird.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Größte Kantenlänge einer Quelldatei. Sie darf größer sein als ein Bild im
/// Projekt, weil sie ein Spritesheet oder hochskaliert sein kann.
pub const MAX_SOURCE_SIDE: u32 = 16384;
/// So viele Pixel darf eine Quelldatei über alle Frames zusammen haben.
pub const MAX_SOURCE_PIXELS: u64 = 64 * 1024 * 1024;
/// Mehr Frames übernimmt ein Import nicht.
pub const MAX_FRAMES: usize = 256;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Import(message.into()))
}

/// Ein Frame einer gelesenen Datei.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureFrame {
    /// Pixel zeilenweise von links oben.
    pub pixels: Vec<Rgba>,
    pub duration_ms: u32,
}

/// Eine gelesene Bilddatei, noch ohne Bezug zu einem Projekt.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<PictureFrame>,
    /// `true`, wenn die Frame-Dauern aus der Datei stammen (animiertes GIF).
    pub timed: bool,
}

/// Liest eine Bilddatei von der Platte.
pub fn read(path: &Path) -> Result<Picture> {
    let unreadable = |err: std::io::Error| {
        Error::Import(if err.kind() == std::io::ErrorKind::NotFound {
            crate::trf!(
                "Die Datei {} gibt es nicht",
                "The file {} does not exist",
                path.display()
            )
        } else {
            crate::trf!(
                "{} lässt sich nicht lesen: {err}",
                "{} cannot be read: {err}",
                path.display()
            )
        })
    };
    let size = std::fs::metadata(path).map_err(unreadable)?.len();
    if size > MAX_FILE_BYTES {
        return fail(crate::trf!(
            "Die Datei ist {} MB groß, erlaubt sind {} MB",
            "The file is {} MB, at most {} MB are allowed",
            size / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(unreadable)?;
    decode(&bytes)
}

/// Liest PNG, JPG oder GIF aus dem Speicher. Ein animiertes GIF ergibt einen
/// Frame je Bild samt Dauer, alles andere einen einzelnen Frame.
pub fn decode(bytes: &[u8]) -> Result<Picture> {
    let unreadable = |err: image::ImageError| {
        Error::Import(crate::trf!(
            "Die Datei ist kein lesbares PNG, JPG oder GIF ({err})",
            "The file is not a readable PNG, JPG or GIF ({err})"
        ))
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_SOURCE_PIXELS * 4);
    let format = image::guess_format(bytes).map_err(unreadable)?;
    let convert = |img: &image::RgbaImage| -> Vec<Rgba> {
        img.pixels()
            .map(|p| Rgba::new(p.0[0], p.0[1], p.0[2], p.0[3]))
            .collect()
    };
    if format == image::ImageFormat::Gif {
        let mut decoder =
            image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(unreadable)?;
        decoder.set_limits(limits).map_err(unreadable)?;
        let (width, height) = decoder.dimensions();
        let per_frame = u64::from(width) * u64::from(height);
        let mut frames = Vec::new();
        for frame in decoder.into_frames() {
            let frame = frame.map_err(unreadable)?;
            if frames.len() == MAX_FRAMES {
                return fail(crate::trf!(
                    "Das GIF hat mehr als {MAX_FRAMES} Frames, so viele übernimmt ein Import höchstens",
                    "The GIF has more than {MAX_FRAMES} frames, which is the most an import takes"
                ));
            }
            if per_frame * (frames.len() as u64 + 1) > MAX_SOURCE_PIXELS {
                return fail(crate::lang::tr(
                    "Das GIF ist zu groß: zu viele Frames für diese Bildgröße",
                    "The GIF is too large: too many frames for this picture size",
                ));
            }
            let (ms, per) = frame.delay().numer_denom_ms();
            let ms = ms.checked_div(per).unwrap_or(0);
            frames.push(PictureFrame {
                // Ein GIF ohne Zeitangabe spielt üblicherweise mit 100 ms.
                duration_ms: if ms == 0 {
                    Sprite::DEFAULT_FRAME_MS
                } else {
                    ms
                },
                pixels: convert(frame.buffer()),
            });
        }
        if frames.is_empty() {
            return fail(crate::lang::tr(
                "Das GIF enthält kein Bild",
                "The GIF contains no picture",
            ));
        }
        let timed = frames.len() > 1;
        return Ok(Picture {
            width,
            height,
            frames,
            timed,
        });
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let img = reader.decode().map_err(unreadable)?.to_rgba8();
    Ok(Picture {
        width: img.width(),
        height: img.height(),
        frames: vec![PictureFrame {
            pixels: convert(&img),
            duration_ms: Sprite::DEFAULT_FRAME_MS,
        }],
        timed: false,
    })
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

impl Picture {
    /// Erkennt hochskalierte Pixel-Art: der größte Faktor `k`, bei dem das Bild
    /// lückenlos aus einfarbigen Blöcken von `k × k` Pixeln besteht. 1 heißt,
    /// das Bild ist schon in Originalgröße. Ein ganz einfarbiges Bild gilt als
    /// Originalgröße, weil sich daraus nichts ablesen lässt.
    pub fn pixel_scale(&self) -> u32 {
        let (w, h) = (self.width as usize, self.height as usize);
        let single_color = self
            .frames
            .iter()
            .all(|f| f.pixels.iter().all(|p| *p == f.pixels[0]));
        if single_color {
            return 1;
        }
        let blocks_are_uniform = |k: usize| {
            self.frames.iter().all(|f| {
                (0..h).all(|y| {
                    let row = &f.pixels[y * w..][..w];
                    let top = &f.pixels[(y - y % k) * w..][..w];
                    (0..w).all(|x| row[x] == top[x - x % k])
                })
            })
        };
        let common = gcd(self.width, self.height);
        (2..=common)
            .rev()
            .filter(|k| common.is_multiple_of(*k))
            .find(|&k| blocks_are_uniform(k as usize))
            .unwrap_or(1)
    }

    /// Verkleinert um einen ganzen Faktor, indem aus jedem Block der Pixel
    /// links oben genommen wird. Für hochskalierte Pixel-Art ist das verlustfrei.
    pub fn shrink(&self, factor: u32) -> Result<Picture> {
        if factor <= 1 {
            return Ok(self.clone());
        }
        if !self.width.is_multiple_of(factor) || !self.height.is_multiple_of(factor) {
            return fail(crate::trf!(
                "{}×{} lässt sich nicht glatt durch {factor} teilen",
                "{}×{} cannot be divided evenly by {factor}",
                self.width,
                self.height
            ));
        }
        let (w, h) = (self.width / factor, self.height / factor);
        let frames = self
            .frames
            .iter()
            .map(|f| PictureFrame {
                duration_ms: f.duration_ms,
                pixels: (0..h)
                    .flat_map(|y| {
                        let row = &f.pixels[(y * factor * self.width) as usize..];
                        (0..w).map(move |x| row[(x * factor) as usize])
                    })
                    .collect(),
            })
            .collect();
        Ok(Picture {
            width: w,
            height: h,
            frames,
            timed: self.timed,
        })
    }

    /// Teilt ein Spritesheet in Frames der Größe `cell_w × cell_h`, zeilenweise
    /// von links oben.
    pub fn split(&self, cell_w: u32, cell_h: u32) -> Result<Picture> {
        if self.frames.len() != 1 {
            return fail(crate::lang::tr(
                "Die Datei hat schon mehrere Frames und lässt sich nicht zusätzlich als Spritesheet teilen",
                "The file already has several frames and cannot also be split as a sprite sheet",
            ));
        }
        if cell_w == 0
            || cell_h == 0
            || !self.width.is_multiple_of(cell_w)
            || !self.height.is_multiple_of(cell_h)
        {
            return fail(crate::trf!(
                "Ein Bild von {}×{} lässt sich nicht in Frames von {cell_w}×{cell_h} teilen",
                "A {}×{} picture cannot be split into frames of {cell_w}×{cell_h}",
                self.width,
                self.height
            ));
        }
        let (columns, rows) = (self.width / cell_w, self.height / cell_h);
        if (columns * rows) as usize > MAX_FRAMES {
            return fail(crate::trf!(
                "Das ergäbe {} Frames, erlaubt sind {MAX_FRAMES}",
                "That would give {} frames, {MAX_FRAMES} are allowed",
                columns * rows
            ));
        }
        let source = &self.frames[0];
        let mut frames = Vec::with_capacity((columns * rows) as usize);
        for row in 0..rows {
            for column in 0..columns {
                let mut pixels = Vec::with_capacity((cell_w * cell_h) as usize);
                for y in 0..cell_h {
                    let start = ((row * cell_h + y) * self.width + column * cell_w) as usize;
                    pixels.extend_from_slice(&source.pixels[start..start + cell_w as usize]);
                }
                frames.push(PictureFrame {
                    pixels,
                    duration_ms: source.duration_ms,
                });
            }
        }
        Ok(Picture {
            width: cell_w,
            height: cell_h,
            frames,
            timed: false,
        })
    }

    /// Entfernt ganz durchsichtige Frames am Ende, wie sie in Spritesheets als
    /// Füllung der letzten Zeile vorkommen. Gibt zurück, wie viele es waren.
    pub fn drop_empty_tail(&mut self) -> usize {
        let before = self.frames.len();
        while self.frames.len() > 1
            && self
                .frames
                .last()
                .is_some_and(|f| f.pixels.iter().all(|p| p.a == 0))
        {
            self.frames.pop();
        }
        before - self.frames.len()
    }

    /// Baut aus den Frames ein Sprite mit einer Ebene.
    pub fn to_sprite(&self, depth: ColorDepth, colors: Colors) -> Result<(Sprite, Report)> {
        if self.frames.len() > MAX_FRAMES {
            return fail(crate::trf!(
                "Die Datei hat {} Frames, erlaubt sind {MAX_FRAMES}",
                "The file has {} frames, {MAX_FRAMES} are allowed",
                self.frames.len()
            ));
        }
        // Wie oft jede Farbe vorkommt, schon auf die Farbtiefe gebracht.
        let mut source = HashMap::new();
        let mut counts: HashMap<Rgba, u64> = HashMap::new();
        for &p in self.frames.iter().flat_map(|f| &f.pixels) {
            let fitted = *source.entry(p).or_insert_with(|| depth.conform(p));
            if !fitted.is_transparent() {
                *counts.entry(fitted).or_insert(0) += 1;
            }
        }
        // Welche Farbe am Ende für jede dieser Farben steht.
        let limited = matches!(colors, Colors::Limited) || depth == ColorDepth::Indexed8;
        let mut target: HashMap<Rgba, Rgba> = HashMap::new();
        match colors {
            Colors::Locked(palette) => {
                for &c in counts.keys() {
                    target.insert(c, palette.nearest(c));
                }
            }
            _ if limited && counts.len() > Palette::MAX_COLORS => {
                let all = counts.iter().map(|(c, n)| (*c, *n)).collect();
                for group in crate::quant::median_cut(all, Palette::MAX_COLORS) {
                    let mean = depth.conform(group.mean);
                    for c in group.members {
                        target.insert(c, mean);
                    }
                }
            }
            _ => {
                for &c in counts.keys() {
                    target.insert(c, c);
                }
            }
        }
        let resolve = |p: Rgba| {
            let fitted = source[&p];
            target.get(&fitted).copied().unwrap_or(Rgba::TRANSPARENT)
        };
        // Die Palette: bei einer Figur ihre eigene, sonst die Farben des Bildes,
        // die häufigsten zuerst.
        let palette = match colors {
            Colors::Locked(palette) => palette.clone(),
            _ => {
                let mut used: HashMap<Rgba, u64> = HashMap::new();
                for (c, n) in &counts {
                    *used.entry(target[c]).or_insert(0) += n;
                }
                let mut used: Vec<(Rgba, u64)> = used.into_iter().collect();
                used.sort_unstable_by_key(|(c, n)| (std::cmp::Reverse(*n), c.to_array()));
                Palette::new("Import", used.into_iter().map(|(c, _)| c).collect())
            }
        };
        let mut sprite = Sprite::new(self.width, self.height, depth, palette)?;
        for (i, frame) in self.frames.iter().enumerate() {
            if i > 0 {
                sprite.add_frame();
            }
            let pixels = frame.pixels.iter().map(|&p| resolve(p)).collect();
            sprite.layer_mut(i, 0)?.replace_pixels(pixels)?;
            sprite.frame_mut(i)?.duration_ms = frame.duration_ms.max(1);
        }
        let report = Report {
            source_colors: source.keys().filter(|c| c.a > 0).count(),
            colors: sprite.used_colors().len(),
            changed: source
                .keys()
                .filter(|&&c| c.a > 0 && resolve(c) != c)
                .count(),
        };
        Ok((sprite, report))
    }
}

/// Welche Farben ein importiertes Sprite haben darf.
#[derive(Clone, Copy, Debug)]
pub enum Colors<'a> {
    /// So viele, wie die Farbtiefe erlaubt.
    Open,
    /// Höchstens 256, wie die Palette einer Figur fasst. Mehr werden auf
    /// ähnliche Farben reduziert.
    Limited,
    /// Nur Farben dieser Palette; jede andere wird zur ähnlichsten.
    Locked(&'a Palette),
}

/// Was beim Umrechnen der Farben geschah.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    /// Verschiedene Farben in der Datei (ohne ganz durchsichtige).
    pub source_colors: usize,
    /// Verschiedene Farben im Ergebnis.
    pub colors: usize,
    /// Wie viele Farben der Datei im Ergebnis anders aussehen, weil Farbtiefe
    /// oder Palette sie nicht fassen.
    pub changed: usize,
}

/// Wie eine Datei vor dem Import verkleinert wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shrink {
    /// Hochskalierte Pixel-Art erkennen und auf Originalgröße bringen.
    Auto,
    /// Um genau diesen Faktor; 1 lässt das Bild, wie es ist.
    By(u32),
}

/// Wie eine Datei in Frames geteilt wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grid {
    /// Gar nicht: die Datei ist ein Bild (oder ein GIF mit eigenen Frames).
    Whole,
    /// Frames dieser Größe, gemessen nach dem Verkleinern.
    Cells { width: u32, height: u32 },
    /// So viele Spalten und Zeilen.
    Count { columns: u32, rows: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub shrink: Shrink,
    pub grid: Grid,
    /// Ganz durchsichtige Frames am Ende eines Spritesheets behalten.
    pub keep_empty: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            shrink: Shrink::Auto,
            grid: Grid::Whole,
            keep_empty: false,
        }
    }
}

/// Eine Datei, fertig verkleinert und in Frames geteilt.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub picture: Picture,
    /// Um welchen Faktor verkleinert wurde (1: gar nicht).
    pub shrunk_by: u32,
    /// Wie viele leere Frames am Ende entfielen.
    pub dropped_empty: usize,
}

/// Verkleinert und teilt eine gelesene Datei nach den Vorgaben.
pub fn prepare(picture: &Picture, options: Options) -> Result<Prepared> {
    let factor = match options.shrink {
        Shrink::Auto => picture.pixel_scale(),
        Shrink::By(k) => k.max(1),
    };
    let small = picture.shrink(factor)?;
    let mut split = match options.grid {
        Grid::Whole => small,
        Grid::Cells { width, height } => small.split(width, height)?,
        Grid::Count { columns, rows } => {
            if columns == 0
                || rows == 0
                || !small.width.is_multiple_of(columns)
                || !small.height.is_multiple_of(rows)
            {
                return fail(crate::trf!(
                    "Ein Bild von {}×{} lässt sich nicht in {columns} Spalten und {rows} Zeilen teilen",
                    "A {}×{} picture cannot be split into {columns} columns and {rows} rows",
                    small.width, small.height
                ));
            }
            small.split(small.width / columns, small.height / rows)?
        }
    };
    let dropped_empty = if options.keep_empty || options.grid == Grid::Whole {
        0
    } else {
        split.drop_empty_tail()
    };
    Ok(Prepared {
        picture: split,
        shrunk_by: factor,
        dropped_empty,
    })
}

impl Prepared {
    /// Für Animationen: Ist die Datei ein ungeteiltes Spritesheet, wird sie in
    /// Frames der Größe `width × height` geschnitten, der Größe der Figur.
    /// Passt die Datei schon oder wurde sie ausdrücklich geteilt, geschieht nichts.
    pub fn cut_to_frames(&mut self, width: u32, height: u32, keep_empty: bool) -> Result<()> {
        let picture = &self.picture;
        if picture.frames.len() != 1 || (picture.width, picture.height) == (width, height) {
            return Ok(());
        }
        self.picture = picture.split(width, height)?;
        if !keep_empty {
            self.dropped_empty += self.picture.drop_empty_tail();
        }
        Ok(())
    }
}

/// Legt die Datei als freies Bild im Projekt an.
pub fn add_image(
    project: &mut Project,
    name: &str,
    picture: &Picture,
    depth: ColorDepth,
) -> Result<(Uuid, Report)> {
    let (sprite, report) = picture.to_sprite(depth, Colors::Open)?;
    Ok((project.add_image(name, sprite), report))
}

/// Die Assets, die beim Import einer Figur entstanden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportedCharacter {
    pub character: Uuid,
    /// Nur wenn die Datei mehrere Frames hatte und eine Animation gewünscht war.
    pub animation: Option<Uuid>,
}

/// Legt die Datei als Figur an. Der erste Frame wird die Grundpose, die Farben
/// aller Frames bilden die gesperrte Palette. Hat die Datei mehrere Frames und
/// ist `animation` gesetzt, entsteht daraus zusätzlich eine Animation der Figur.
pub fn add_character(
    project: &mut Project,
    name: &str,
    picture: &Picture,
    depth: ColorDepth,
    animation: Option<AnimationKind>,
) -> Result<(ImportedCharacter, Report)> {
    let (all_frames, report) = picture.to_sprite(depth, Colors::Limited)?;
    let mut base = all_frames.clone();
    while base.frames().len() > 1 {
        base.remove_frame(base.frames().len() - 1)?;
    }
    base.frame_mut(0)?.duration_ms = Sprite::DEFAULT_FRAME_MS;
    let character = Character::from_sprite(name, base)?;
    let animation = match animation {
        Some(kind) if all_frames.frames().len() > 1 => {
            let mut sprite = all_frames;
            sprite.palette = character.palette().clone();
            if !picture.timed {
                for i in 0..sprite.frames().len() {
                    sprite.frame_mut(i)?.duration_ms = kind.default_frame_ms();
                }
            }
            Some(Animation {
                id: Uuid::new_v4(),
                name: format!("{} {}", character.name, kind.name()),
                looping: kind.loops_by_default(),
                kind,
                character_id: character.id,
                sprite,
            })
        }
        _ => None,
    };
    let imported = ImportedCharacter {
        character: character.id,
        animation: animation.as_ref().map(|a| a.id),
    };
    project.add_character(character);
    project.animations.extend(animation);
    Ok((imported, report))
}

/// Was mit Farben geschieht, die die Figur noch nicht kennt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewColors {
    /// In die Palette der Figur aufnehmen. Das Bild bleibt pixelgenau.
    Add,
    /// Durch die ähnlichste Farbe der Figur ersetzen. Die Palette bleibt, wie sie ist.
    Snap,
}

/// Legt die Datei als Animation einer vorhandenen Figur an. Die Frames müssen
/// so groß sein wie die Figur. Gibt außerdem zurück, wie viele Farben neu in
/// die Palette der Figur kamen.
pub fn add_animation(
    project: &mut Project,
    character_id: Uuid,
    name: Option<&str>,
    kind: AnimationKind,
    picture: &Picture,
    new_colors: NewColors,
) -> Result<(Uuid, Report, usize)> {
    let character = project.character(character_id)?;
    let (w, h) = character.size();
    if (picture.width, picture.height) != (w, h) {
        return fail(crate::trf!(
            "Die Frames sind {}×{} Pixel groß, die Figur „{}“ aber {w}×{h}. Eine Animation hat immer die Größe ihrer Figur.",
            "The frames are {}×{} pixels, but the character \"{}\" is {w}×{h}. An animation always has the size of its character.",
            picture.width, picture.height, character.name
        ));
    }
    let depth = character.base.depth;
    let before = character.palette().len();
    if new_colors == NewColors::Add {
        let mut missing = Vec::new();
        for &p in picture.frames.iter().flat_map(|f| &f.pixels) {
            let c = depth.conform(p);
            if !c.is_transparent() && !character.palette().contains(c) && !missing.contains(&c) {
                missing.push(c);
                if before + missing.len() > Palette::MAX_COLORS {
                    return fail(crate::trf!(
                        "Die Animation bringt mehr neue Farben mit, als in die Palette der Figur passen (sie hat {before} von {} Farben). Lass die Farben stattdessen an die Palette der Figur anpassen.",
                        "The animation brings more new colors than fit into the character's palette (it has {before} of {} colors). Let the colors snap to the character's palette instead.",
                        Palette::MAX_COLORS
                    ));
                }
            }
        }
        project.add_character_colors(character_id, &missing)?;
    }
    let character = project.character(character_id)?;
    let added = character.palette().len() - before;
    let (mut sprite, report) = picture.to_sprite(depth, Colors::Locked(character.palette()))?;
    if !picture.timed {
        for i in 0..sprite.frames().len() {
            sprite.frame_mut(i)?.duration_ms = kind.default_frame_ms();
        }
    }
    let animation = Animation {
        id: Uuid::new_v4(),
        name: match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => name.to_string(),
            None => format!("{} {}", character.name, kind.name()),
        },
        looping: kind.loops_by_default(),
        kind,
        character_id,
        sprite,
    };
    let id = animation.id;
    project.animations.push(animation);
    Ok((id, report, added))
}
