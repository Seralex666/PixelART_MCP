//! Export von Einzelbildern, Spritesheets und animierten GIFs.
//!
//! Vergrößert wird nur ganzzahlig und ohne Glättung, damit jeder Pixel ein
//! sauberes Quadrat bleibt.

use crate::{Error, Result, Rgba, Sprite};
use image::{ImageFormat, RgbImage, RgbaImage};
use serde::Serialize;
use std::io::Cursor;
use std::path::Path;

/// Größter erlaubter Vergrößerungsfaktor beim Export.
pub const MAX_SCALE: u32 = 64;
/// Größte Kantenlänge einer Exportdatei in Pixeln.
pub const MAX_EXPORT_SIZE: u32 = 16384;

fn to_image(px: &[Rgba], width: u32, height: u32, scale: u32) -> Result<RgbaImage> {
    let scale = scale.clamp(1, MAX_SCALE);
    let (ow, oh) = (width * scale, height * scale);
    if ow > MAX_EXPORT_SIZE || oh > MAX_EXPORT_SIZE {
        return Err(Error::InvalidSize {
            width: ow,
            height: oh,
        });
    }
    Ok(RgbaImage::from_fn(ow, oh, |x, y| {
        image::Rgba(px[((y / scale) * width + x / scale) as usize].to_array())
    }))
}

/// Ein Frame als RGBA-Bild, Ebenen verrechnet.
pub fn frame_image(s: &Sprite, frame: usize, scale: u32) -> Result<RgbaImage> {
    to_image(&s.composite(frame)?, s.width(), s.height(), scale)
}

/// Ein Frame als PNG-Bytes.
pub fn frame_png(s: &Sprite, frame: usize, scale: u32) -> Result<Vec<u8>> {
    encode_png(&frame_image(s, frame, scale)?)
}

/// Ein Frame als JPEG-Bytes. JPEG kennt keine Transparenz, deshalb wird
/// das Bild auf `background` gelegt. `quality` liegt zwischen 1 und 100.
pub fn frame_jpeg(
    s: &Sprite,
    frame: usize,
    scale: u32,
    background: Rgba,
    quality: u8,
) -> Result<Vec<u8>> {
    let img = frame_image(s, frame, scale)?;
    let bg = Rgba {
        a: 255,
        ..background
    };
    let rgb = RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let c = Rgba::new(p[0], p[1], p[2], p[3]).over(bg);
        image::Rgb([c.r, c.g, c.b])
    });
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    rgb.write_with_encoder(enc)?;
    Ok(out)
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// Eine Farbtabelle und die Bilder als Verweise in diese Tabelle.
struct Indexed {
    colors: Vec<Rgba>,
    frames: Vec<Vec<u8>>,
}

/// Bringt Bilder auf eine gemeinsame Farbtabelle mit höchstens 256 Einträgen.
/// Hat das Bild mehr Farben, kommt deren Anzahl als Fehler zurück.
fn index_frames(frames: &[Vec<Rgba>]) -> std::result::Result<Indexed, usize> {
    let mut lookup = std::collections::HashMap::new();
    let mut colors = Vec::new();
    for &c in frames.iter().flatten() {
        let next = colors.len();
        if *lookup.entry(c).or_insert(next) == next {
            colors.push(c);
        }
    }
    if colors.len() > 256 {
        return Err(colors.len());
    }
    let frames = frames
        .iter()
        .map(|f| f.iter().map(|c| lookup[c] as u8).collect())
        .collect();
    Ok(Indexed { colors, frames })
}

/// Vergrößert ein Bild aus Tabellen-Verweisen ganzzahlig.
fn scale_indices(px: &[u8], width: u32, height: u32, scale: u32) -> Vec<u8> {
    let (ow, oh) = (width * scale, height * scale);
    let mut out = Vec::with_capacity((ow * oh) as usize);
    for y in 0..oh {
        let row = &px[((y / scale) * width) as usize..][..width as usize];
        for &i in row {
            out.extend(std::iter::repeat_n(i, scale as usize));
        }
    }
    out
}

/// Größe des vergrößerten Bildes, oder ein Fehler, wenn sie die Grenze sprengt.
pub(crate) fn check_export_size(s: &Sprite, scale: u32) -> Result<(u32, u32)> {
    let scale = scale.clamp(1, MAX_SCALE);
    let (ow, oh) = (s.width() * scale, s.height() * scale);
    if ow > MAX_EXPORT_SIZE || oh > MAX_EXPORT_SIZE {
        return Err(Error::InvalidSize {
            width: ow,
            height: oh,
        });
    }
    Ok((ow, oh))
}

/// Ein Frame als PNG mit Farbtabelle statt Vollfarbe ("indiziert", PNG-8).
///
/// Die Datei enthält exakt dieselben Pixel wie [`frame_png`], ist aber kleiner
/// und passt zu Werkzeugen, die eine Palette erwarten. Geht nur bei höchstens
/// 256 verschiedenen Farben; halbtransparente Farben zählen als eigene Farben.
pub fn frame_png_indexed(s: &Sprite, frame: usize, scale: u32) -> Result<Vec<u8>> {
    let scale = scale.clamp(1, MAX_SCALE);
    let (ow, oh) = check_export_size(s, scale)?;
    let indexed = index_frames(&[s.composite(frame)?])
        .map_err(|found| Error::TooManyColors { found, max: 256 })?;
    // Kleinste Bittiefe, in die die Tabelle passt: 2, 4, 16 oder 256 Farben.
    let (depth, bits) = match indexed.colors.len() {
        0..=2 => (png::BitDepth::One, 1),
        3..=4 => (png::BitDepth::Two, 2),
        5..=16 => (png::BitDepth::Four, 4),
        _ => (png::BitDepth::Eight, 8),
    };
    let pixels = scale_indices(&indexed.frames[0], s.width(), s.height(), scale);
    // Jede Zeile beginnt auf einer Bytegrenze, die Pixel stehen von links nach rechts.
    let row_bytes = (ow as usize * bits).div_ceil(8);
    let mut data = vec![0u8; row_bytes * oh as usize];
    for (y, row) in pixels.chunks(ow as usize).enumerate() {
        for (x, &i) in row.iter().enumerate() {
            let bit = x * bits;
            data[y * row_bytes + bit / 8] |= i << (8 - bits - bit % 8);
        }
    }
    let mut palette = Vec::with_capacity(indexed.colors.len() * 3);
    let mut alpha = Vec::with_capacity(indexed.colors.len());
    for c in &indexed.colors {
        palette.extend([c.r, c.g, c.b]);
        alpha.push(c.a);
    }
    // Deckende Einträge am Ende brauchen keinen Alpha-Wert.
    while alpha.last() == Some(&255) {
        alpha.pop();
    }
    if palette.is_empty() {
        palette.extend([0, 0, 0]);
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, ow, oh);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(depth);
    encoder.set_palette(palette);
    if !alpha.is_empty() {
        encoder.set_trns(alpha);
    }
    let encode = |e: png::EncodingError| Error::Encode(e.to_string());
    let mut writer = encoder.write_header().map_err(encode)?;
    writer.write_image_data(&data).map_err(encode)?;
    writer.finish().map_err(encode)?;
    Ok(out)
}

/// Was beim GIF-Export herauskam.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GifInfo {
    /// Anzahl der Farben in der Datei (ohne Transparenz).
    pub colors: usize,
    /// `false`, wenn das Bild mehr Farben hatte, als ein GIF fasst, und sie
    /// deshalb auf ähnliche Farben reduziert wurden.
    pub exact: bool,
}

/// Anzeigedauer eines Frames in GIF-Einheiten (Hundertstelsekunden). Unter
/// 20 ms spielen viele Programme stattdessen mit 100 ms ab, deshalb mindestens 2.
pub fn gif_delay(duration_ms: u32) -> u16 {
    ((duration_ms + 5) / 10).clamp(2, u32::from(u16::MAX)) as u16
}

/// Alle Frames als animiertes GIF.
///
/// GIF kennt höchstens 256 Farben und Transparenz nur als an oder aus. Pixel mit
/// weniger als halber Deckkraft werden durchsichtig, alle anderen deckend. Passen
/// die Farben in die Tabelle, bleiben sie exakt; sonst werden sie reduziert, und
/// [`GifInfo::exact`] ist `false`.
pub fn animation_gif(s: &Sprite, scale: u32, looping: bool) -> Result<(Vec<u8>, GifInfo)> {
    let scale = scale.clamp(1, MAX_SCALE);
    let (ow, oh) = check_export_size(s, scale)?;
    let mut frames = Vec::with_capacity(s.frames().len());
    for f in 0..s.frames().len() {
        let mut px = s.composite(f)?;
        for c in &mut px {
            *c = if c.a < 128 {
                Rgba::TRANSPARENT
            } else {
                Rgba::rgb(c.r, c.g, c.b)
            };
        }
        frames.push(px);
    }
    let (indexed, exact) = match index_frames(&frames) {
        Ok(indexed) => (indexed, true),
        Err(_) => (quantize(&frames), false),
    };
    let transparent = indexed
        .colors
        .iter()
        .position(|c| c.is_transparent())
        .map(|i| i as u8);
    let mut palette: Vec<u8> = indexed
        .colors
        .iter()
        .flat_map(|c| [c.r, c.g, c.b])
        .collect();
    if palette.is_empty() {
        palette.extend([0, 0, 0]);
    }
    let encode = |e: gif::EncodingError| Error::Encode(e.to_string());
    let mut out = Vec::new();
    {
        let mut encoder =
            gif::Encoder::new(&mut out, ow as u16, oh as u16, &palette).map_err(encode)?;
        if looping {
            encoder.set_repeat(gif::Repeat::Infinite).map_err(encode)?;
        }
        for (pixels, frame) in indexed.frames.iter().zip(s.frames()) {
            let pixels = scale_indices(pixels, s.width(), s.height(), scale);
            let mut gif_frame =
                gif::Frame::from_indexed_pixels(ow as u16, oh as u16, pixels, transparent);
            gif_frame.delay = gif_delay(frame.duration_ms);
            // Mit Transparenz muss das vorige Bild erst weg, sonst scheint es durch.
            gif_frame.dispose = if transparent.is_some() {
                gif::DisposalMethod::Background
            } else {
                gif::DisposalMethod::Keep
            };
            encoder.write_frame(&gif_frame).map_err(encode)?;
        }
    }
    let colors = indexed.colors.len() - usize::from(transparent.is_some());
    Ok((out, GifInfo { colors, exact }))
}

/// Reduziert Bilder mit mehr als 256 Farben auf eine gemeinsame Tabelle mit
/// höchstens 255 Farben und einem Eintrag für Transparenz.
///
/// Verfahren "Median Cut": Alle Farben liegen zuerst in einer Kiste. Die Kiste
/// mit der größten Farbspanne wird immer wieder an ihrer Mitte geteilt, bis es
/// 255 Kisten sind. Jede Kiste ergibt eine Farbe, den Durchschnitt ihres Inhalts.
fn quantize(frames: &[Vec<Rgba>]) -> Indexed {
    let mut counts = std::collections::HashMap::new();
    for &c in frames.iter().flatten().filter(|c| !c.is_transparent()) {
        *counts.entry(c).or_insert(0u64) += 1;
    }
    let mut all: Vec<(Rgba, u64)> = counts.into_iter().collect();
    // Feste Reihenfolge, damit derselbe Export immer dieselbe Datei ergibt.
    all.sort_unstable_by_key(|(c, _)| c.to_array());
    let channel = |c: Rgba, i: usize| c.to_array()[i];
    // Größte Spanne einer Kiste und der Farbkanal, in dem sie liegt.
    let spread = |b: &[(Rgba, u64)]| {
        (0..3)
            .map(|i| {
                let values = b.iter().map(|(c, _)| channel(*c, i));
                let (lo, hi) = values.fold((255, 0), |(lo, hi), v| (lo.min(v), hi.max(v)));
                (hi.saturating_sub(lo), i)
            })
            .max()
            .unwrap_or((0, 0))
    };
    let mut boxes = vec![all];
    while boxes.len() < 255 {
        let Some((at, (_, by))) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| (i, spread(b)))
            .max_by_key(|&(_, (range, _))| range)
        else {
            break;
        };
        let b = &mut boxes[at];
        b.sort_unstable_by_key(|(c, _)| (channel(*c, by), c.to_array()));
        // Teilen, wo die Hälfte der Pixel erreicht ist, aber nie am Rand.
        let half = b.iter().map(|(_, n)| n).sum::<u64>() / 2;
        let mut seen = 0;
        let mut cut = b.len() - 1;
        for (i, (_, n)) in b.iter().enumerate() {
            seen += n;
            if seen >= half {
                cut = (i + 1).min(b.len() - 1);
                break;
            }
        }
        let upper = b.split_off(cut.max(1));
        boxes.push(upper);
    }
    let mut colors = vec![Rgba::TRANSPARENT];
    let mut lookup = std::collections::HashMap::new();
    for (i, b) in boxes.iter().enumerate() {
        let total = b.iter().map(|(_, n)| n).sum::<u64>().max(1);
        let mean = |ch: usize| {
            let sum: u64 = b.iter().map(|(c, n)| u64::from(channel(*c, ch)) * n).sum();
            ((sum + total / 2) / total) as u8
        };
        colors.push(Rgba::rgb(mean(0), mean(1), mean(2)));
        for (c, _) in b {
            lookup.insert(*c, i as u8 + 1);
        }
    }
    let frames = frames
        .iter()
        .map(|f| {
            f.iter()
                .map(|c| lookup.get(c).copied().unwrap_or(0))
                .collect()
        })
        .collect();
    Indexed { colors, frames }
}

/// Lage eines Frames im Spritesheet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetFrame {
    pub index: usize,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub duration_ms: u32,
}

/// Metadaten zu einem Spritesheet, zum Einlesen in Game-Engines.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetMeta {
    pub image_width: u32,
    pub image_height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub columns: u32,
    pub rows: u32,
    pub scale: u32,
    pub frames: Vec<SheetFrame>,
}

/// Baut ein Spritesheet aus allen Frames. `columns = 0` legt alle Frames in eine Zeile.
pub fn sheet_image(s: &Sprite, columns: u32, scale: u32) -> Result<(RgbaImage, SheetMeta)> {
    let n = s.frames().len() as u32;
    let scale = scale.clamp(1, MAX_SCALE);
    let columns = if columns == 0 { n } else { columns.min(n) };
    let rows = n.div_ceil(columns);
    let (fw, fh) = (s.width() * scale, s.height() * scale);
    let (iw, ih) = (fw as u64 * columns as u64, fh as u64 * rows as u64);
    if iw > MAX_EXPORT_SIZE as u64 || ih > MAX_EXPORT_SIZE as u64 {
        return Err(Error::InvalidSize {
            width: iw.min(u32::MAX as u64) as u32,
            height: ih.min(u32::MAX as u64) as u32,
        });
    }
    let mut sheet = RgbaImage::new(iw as u32, ih as u32);
    let mut frames = Vec::with_capacity(n as usize);
    for (i, f) in s.frames().iter().enumerate() {
        let (x, y) = ((i as u32 % columns) * fw, (i as u32 / columns) * fh);
        let img = frame_image(s, i, scale)?;
        image::imageops::replace(&mut sheet, &img, x as i64, y as i64);
        frames.push(SheetFrame {
            index: i,
            x,
            y,
            w: fw,
            h: fh,
            duration_ms: f.duration_ms,
        });
    }
    let meta = SheetMeta {
        image_width: iw as u32,
        image_height: ih as u32,
        frame_width: fw,
        frame_height: fh,
        columns,
        rows,
        scale,
        frames,
    };
    Ok((sheet, meta))
}

/// Spritesheet als PNG-Bytes plus Metadaten.
pub fn sheet_png(s: &Sprite, columns: u32, scale: u32) -> Result<(Vec<u8>, SheetMeta)> {
    let (img, meta) = sheet_image(s, columns, scale)?;
    Ok((encode_png(&img)?, meta))
}

/// Schreibt ein Spritesheet als `<pfad>.png` und die Metadaten als `<pfad>.json`.
pub fn save_sheet(
    s: &Sprite,
    path_without_ext: &Path,
    columns: u32,
    scale: u32,
) -> Result<SheetMeta> {
    let (png, meta) = sheet_png(s, columns, scale)?;
    std::fs::write(path_without_ext.with_extension("png"), png)?;
    std::fs::write(
        path_without_ext.with_extension("json"),
        serde_json::to_vec_pretty(&meta)?,
    )?;
    Ok(meta)
}

/// Liest ein PNG oder JPEG als Pixel ein (z. B. um ein vorhandenes Bild zu importieren).
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<Rgba>)> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let px = img
        .pixels()
        .map(|p| Rgba::new(p.0[0], p.0[1], p.0[2], p.0[3]))
        .collect();
    Ok((img.width(), img.height(), px))
}
