//! Export von Einzelbildern und Spritesheets.
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
