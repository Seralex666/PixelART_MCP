use crate::{ColorDepth, Error, Layer, Palette, Result, Rgba, MAX_SIZE};
use serde::{Deserialize, Serialize};

/// Ein Einzelbild einer Animation. Alle Frames eines Sprites haben dieselben Ebenen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Anzeigedauer in Millisekunden.
    pub duration_ms: u32,
    pub layers: Vec<Layer>,
}

/// Ein Pixelbild mit Ebenen und einem oder mehreren Frames.
///
/// Alle Schreibzugriffe sollten über [`Sprite::conform`] laufen, damit die
/// Farbtiefe des Dokuments eingehalten wird.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    width: u32,
    height: u32,
    pub depth: ColorDepth,
    pub palette: Palette,
    frames: Vec<Frame>,
}

impl Sprite {
    pub const DEFAULT_FRAME_MS: u32 = 100;

    pub fn new(width: u32, height: u32, depth: ColorDepth, palette: Palette) -> Result<Self> {
        check_size(width, height)?;
        Ok(Self {
            width,
            height,
            depth,
            palette,
            frames: vec![Frame {
                duration_ms: Self::DEFAULT_FRAME_MS,
                layers: vec![Layer::new("Ebene 1", width, height)],
            }],
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn frame(&self, i: usize) -> Result<&Frame> {
        self.frames.get(i).ok_or(Error::NoSuchFrame(i))
    }

    pub fn frame_mut(&mut self, i: usize) -> Result<&mut Frame> {
        self.frames.get_mut(i).ok_or(Error::NoSuchFrame(i))
    }

    pub fn layer_count(&self) -> usize {
        self.frames.first().map_or(0, |f| f.layers.len())
    }

    pub fn layer_names(&self) -> Vec<String> {
        self.frames
            .first()
            .map(|f| f.layers.iter().map(|l| l.name.clone()).collect())
            .unwrap_or_default()
    }

    pub fn layer(&self, frame: usize, layer: usize) -> Result<&Layer> {
        self.frame(frame)?
            .layers
            .get(layer)
            .ok_or(Error::NoSuchLayer(layer))
    }

    pub fn layer_mut(&mut self, frame: usize, layer: usize) -> Result<&mut Layer> {
        self.frame_mut(frame)?
            .layers
            .get_mut(layer)
            .ok_or(Error::NoSuchLayer(layer))
    }

    /// Index der Ebene mit diesem Namen (Groß- und Kleinschreibung egal).
    pub fn layer_index(&self, name: &str) -> Option<usize> {
        self.layer_names()
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
    }

    /// Ersetzt die Palette und passt sie und alle vorhandenen Pixel an die
    /// Farbtiefe an. Bei gesperrter Palette (Figuren) nicht erlaubt.
    pub fn set_palette(&mut self, palette: Palette) -> Result<()> {
        if self.palette.locked {
            return Err(Error::PaletteLocked);
        }
        self.palette = palette;
        self.palette.locked = false;
        self.palette.conform_to(self.depth);
        if self.depth == ColorDepth::Indexed8 {
            // Im 8-Bit-Modus müssen alle Pixel aus der neuen Palette stammen.
            let pal = self.palette.clone();
            for f in &mut self.frames {
                for l in &mut f.layers {
                    l.map_pixels(|p| pal.nearest(p));
                }
            }
        }
        Ok(())
    }

    /// Kopiert den Inhalt eines Frames in einen anderen (Dauer bleibt erhalten).
    pub fn copy_frame(&mut self, from: usize, to: usize) -> Result<()> {
        let layers = self.frame(from)?.layers.clone();
        self.frame_mut(to)?.layers = layers;
        Ok(())
    }

    /// Bringt eine Farbe auf die Farbtiefe des Sprites.
    ///
    /// - 8 Bit: Ist die Farbe nicht in der Palette, wird sie aufgenommen, solange
    ///   die Palette offen ist und Platz hat. Sonst wird die nächste Palettenfarbe genommen.
    /// - 16 Bit: Reduktion auf RGB565.
    /// - 32 Bit: unverändert.
    ///
    /// Bei gesperrter Palette (Figuren) wird in jeder Farbtiefe die nächste Palettenfarbe
    /// genommen. Die Farben einer gesperrten Palette liegen selbst in der Farbtiefe
    /// (siehe [`Palette::conform_to`]), das Ergebnis erfüllt also beide Bedingungen.
    pub fn conform(&mut self, c: Rgba) -> Rgba {
        let c = self.depth.conform(c);
        if c.is_transparent() {
            return Rgba::TRANSPARENT;
        }
        if self.palette.locked {
            return self.palette.nearest(c);
        }
        if self.depth == ColorDepth::Indexed8 && self.palette.add(c).is_err() {
            return self.palette.nearest(c);
        }
        c
    }

    /// Setzt einen Pixel mit Farbtiefen-Anpassung.
    pub fn set_pixel(
        &mut self,
        frame: usize,
        layer: usize,
        x: i64,
        y: i64,
        c: Rgba,
    ) -> Result<Rgba> {
        let c = self.conform(c);
        self.layer_mut(frame, layer)?.set(x, y, c)?;
        Ok(c)
    }

    /// Fügt allen Frames eine leere Ebene hinzu und gibt ihren Index zurück.
    pub fn add_layer(&mut self, name: impl Into<String>) -> usize {
        let name = name.into();
        for f in &mut self.frames {
            f.layers
                .push(Layer::new(name.clone(), self.width, self.height));
        }
        self.layer_count() - 1
    }

    pub fn remove_layer(&mut self, layer: usize) -> Result<()> {
        if layer >= self.layer_count() || self.layer_count() == 1 {
            return Err(Error::NoSuchLayer(layer));
        }
        for f in &mut self.frames {
            f.layers.remove(layer);
        }
        Ok(())
    }

    pub fn rename_layer(&mut self, layer: usize, name: impl Into<String>) -> Result<()> {
        let name = name.into();
        if layer >= self.layer_count() {
            return Err(Error::NoSuchLayer(layer));
        }
        for f in &mut self.frames {
            f.layers[layer].name = name.clone();
        }
        Ok(())
    }

    /// Hängt einen leeren Frame an (mit denselben Ebenen) und gibt seinen Index zurück.
    pub fn add_frame(&mut self) -> usize {
        let layers = self
            .layer_names()
            .into_iter()
            .map(|n| Layer::new(n, self.width, self.height))
            .collect();
        self.frames.push(Frame {
            duration_ms: Self::DEFAULT_FRAME_MS,
            layers,
        });
        self.frames.len() - 1
    }

    /// Kopiert einen Frame und fügt die Kopie direkt dahinter ein.
    pub fn duplicate_frame(&mut self, i: usize) -> Result<usize> {
        let f = self.frame(i)?.clone();
        self.frames.insert(i + 1, f);
        Ok(i + 1)
    }

    pub fn remove_frame(&mut self, i: usize) -> Result<()> {
        if i >= self.frames.len() || self.frames.len() == 1 {
            return Err(Error::NoSuchFrame(i));
        }
        self.frames.remove(i);
        Ok(())
    }

    /// Stellt die Farbtiefe um und passt die Palette und alle vorhandenen Pixel an.
    pub fn set_depth(&mut self, depth: ColorDepth) {
        self.depth = depth;
        self.palette.conform_to(depth);
        let mut frames = std::mem::take(&mut self.frames);
        for f in &mut frames {
            for l in &mut f.layers {
                l.map_pixels(|p| self.conform(p));
            }
        }
        self.frames = frames;
    }

    /// Alle Farben, die tatsächlich in irgendeiner Ebene vorkommen (ohne Transparenz).
    pub fn used_colors(&self) -> Vec<Rgba> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for f in &self.frames {
            for l in &f.layers {
                for &p in l.pixels() {
                    if !p.is_transparent() && seen.insert(p) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    /// Alle Pixel eines Frames, Ebenen von unten nach oben verrechnet.
    pub fn composite(&self, frame: usize) -> Result<Vec<Rgba>> {
        let f = self.frame(frame)?;
        let mut out = vec![Rgba::TRANSPARENT; (self.width * self.height) as usize];
        for l in f.layers.iter().filter(|l| l.visible && l.opacity > 0) {
            for (dst, &src) in out.iter_mut().zip(l.pixels()) {
                if src.a == 0 {
                    continue;
                }
                let src = if l.opacity == 255 {
                    src
                } else {
                    Rgba {
                        a: (src.a as u32 * l.opacity as u32 / 255) as u8,
                        ..src
                    }
                };
                *dst = src.over(*dst);
            }
        }
        Ok(out)
    }

    /// Prüft die innere Konsistenz (z. B. nach dem Laden einer Datei).
    pub fn check(&self) -> Result<()> {
        check_size(self.width, self.height)?;
        if self.frames.is_empty() {
            return Err(Error::NoSuchFrame(0));
        }
        let names = self.layer_names();
        for f in &self.frames {
            if f.layers.len() != names.len() {
                return Err(Error::InvalidPixelData(
                    "Frames haben unterschiedlich viele Ebenen".into(),
                ));
            }
            for l in &f.layers {
                // Erst die Maße prüfen: sie stammen aus der Datei und dürfen die
                // Pixelzahl-Prüfung nicht mit Riesenwerten füttern.
                if l.width() != self.width || l.height() != self.height {
                    return Err(Error::SizeMismatch {
                        expected_w: self.width,
                        expected_h: self.height,
                        got_w: l.width(),
                        got_h: l.height(),
                    });
                }
                l.check_consistent()?;
            }
        }
        Ok(())
    }
}

pub(crate) fn check_size(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 || width > MAX_SIZE || height > MAX_SIZE {
        return Err(Error::InvalidSize { width, height });
    }
    Ok(())
}
