use crate::{Error, Result, Rgba};
use base64::Engine;
use serde::{Deserialize, Serialize};

/// Eine Ebene: ein echtes Pixelraster mit genau `width × height` Pixeln.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Deckkraft der Ebene, 0 bis 255.
    #[serde(default = "full")]
    pub opacity: u8,
    width: u32,
    height: u32,
    /// Pixel zeilenweise von links oben, gespeichert als Base64 von RGBA-Bytes.
    #[serde(with = "pixel_data")]
    pixels: Vec<Rgba>,
}

fn yes() -> bool {
    true
}
fn full() -> u8 {
    255
}

impl Layer {
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 255,
            width,
            height,
            pixels: vec![Rgba::TRANSPARENT; (width * height) as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[Rgba] {
        &self.pixels
    }

    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && (x as u64) < self.width as u64 && (y as u64) < self.height as u64
    }

    fn index(&self, x: i64, y: i64) -> Result<usize> {
        if !self.in_bounds(x, y) {
            return Err(Error::OutOfBounds {
                x,
                y,
                width: self.width,
                height: self.height,
            });
        }
        Ok(y as usize * self.width as usize + x as usize)
    }

    pub fn get(&self, x: i64, y: i64) -> Result<Rgba> {
        Ok(self.pixels[self.index(x, y)?])
    }

    /// Setzt einen Pixel. Die Farbe muss vorher an die Farbtiefe angepasst sein
    /// (siehe [`crate::Sprite::conform`]).
    pub fn set(&mut self, x: i64, y: i64, c: Rgba) -> Result<()> {
        let i = self.index(x, y)?;
        self.pixels[i] = c;
        Ok(())
    }

    /// Wie [`Layer::set`], ignoriert aber Pixel außerhalb des Bildes (für Zeichenwerkzeuge).
    pub fn set_clipped(&mut self, x: i64, y: i64, c: Rgba) {
        if self.in_bounds(x, y) {
            let i = y as usize * self.width as usize + x as usize;
            self.pixels[i] = c;
        }
    }

    pub fn clear(&mut self) {
        self.pixels.fill(Rgba::TRANSPARENT);
    }

    pub fn fill(&mut self, c: Rgba) {
        self.pixels.fill(c);
    }

    /// Wendet `f` auf jeden Pixel an (z. B. um eine neue Farbtiefe zu erzwingen).
    pub fn map_pixels(&mut self, mut f: impl FnMut(Rgba) -> Rgba) {
        for p in &mut self.pixels {
            *p = f(*p);
        }
    }

    /// Ersetzt den Inhalt durch rohe Pixel (zeilenweise, `width × height` Einträge).
    pub fn replace_pixels(&mut self, pixels: Vec<Rgba>) -> Result<()> {
        if pixels.len() != self.pixels.len() {
            return Err(Error::InvalidPixelData(format!(
                "erwartet {} Pixel, bekommen {}",
                self.pixels.len(),
                pixels.len()
            )));
        }
        self.pixels = pixels;
        Ok(())
    }

    pub(crate) fn check_consistent(&self) -> Result<()> {
        if self.pixels.len() as u64 != self.width as u64 * self.height as u64 {
            return Err(Error::InvalidPixelData(format!(
                "Ebene '{}' hat {} Pixel, erwartet {}",
                self.name,
                self.pixels.len(),
                self.width as u64 * self.height as u64
            )));
        }
        Ok(())
    }
}

mod pixel_data {
    use super::*;

    pub fn serialize<S: serde::Serializer>(
        px: &[Rgba],
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_array()).collect();
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<Vec<Rgba>, D::Error> {
        let s = String::deserialize(d)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .map_err(serde::de::Error::custom)?;
        if bytes.len() % 4 != 0 {
            return Err(serde::de::Error::custom(
                "Pixeldaten sind kein Vielfaches von 4 Bytes",
            ));
        }
        Ok(bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| Rgba::new(r, g, b, a))
            .collect())
    }
}
