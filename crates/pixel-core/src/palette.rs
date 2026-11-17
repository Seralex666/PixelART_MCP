use crate::{ColorDepth, Error, Result, Rgba};
use serde::{Deserialize, Serialize};

/// Eine Farbpalette. Im 8-Bit-Modus ist sie die einzige Farbquelle,
/// in den anderen Modi dient sie als Farbvorrat des Projekts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<Rgba>,
    /// Gesperrte Paletten nehmen keine neuen Farben auf. Figuren nutzen das,
    /// damit sie in allen Animationen dieselben Farben behalten.
    #[serde(default)]
    pub locked: bool,
}

impl Palette {
    pub const MAX_COLORS: usize = 256;

    pub fn new(name: impl Into<String>, colors: Vec<Rgba>) -> Self {
        let mut colors = colors;
        colors.truncate(Self::MAX_COLORS);
        Self {
            name: name.into(),
            colors,
            locked: false,
        }
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    pub fn contains(&self, c: Rgba) -> bool {
        c.is_transparent() || self.colors.contains(&c)
    }

    /// Fügt eine Farbe hinzu, falls sie noch fehlt, und gibt ihren Index zurück.
    pub fn add(&mut self, c: Rgba) -> Result<usize> {
        if let Some(i) = self.colors.iter().position(|&p| p == c) {
            return Ok(i);
        }
        if self.locked {
            return Err(Error::ColorNotInPalette(c));
        }
        if self.colors.len() >= Self::MAX_COLORS {
            return Err(Error::PaletteFull(Self::MAX_COLORS));
        }
        self.colors.push(c);
        Ok(self.colors.len() - 1)
    }

    /// Die Palettenfarbe, die `c` am nächsten kommt. Transparenz bleibt transparent.
    /// Eine leere Palette kennt keine Farbe, das Ergebnis ist dann transparent.
    pub fn nearest(&self, c: Rgba) -> Rgba {
        if c.a < 128 {
            return Rgba::TRANSPARENT;
        }
        self.colors
            .iter()
            .min_by_key(|p| p.distance_sq(c))
            .copied()
            .unwrap_or(Rgba::TRANSPARENT)
    }

    /// Bringt alle Palettenfarben auf die Farbtiefe und entfernt dabei entstehende
    /// Doppelte, damit jede Palettenfarbe in dieser Farbtiefe exakt zeichenbar ist.
    pub fn conform_to(&mut self, depth: ColorDepth) {
        let mut out: Vec<Rgba> = Vec::with_capacity(self.colors.len());
        for &c in &self.colors {
            let c = depth.conform(c);
            if !c.is_transparent() && !out.contains(&c) {
                out.push(c);
            }
        }
        self.colors = out;
    }

    pub fn is_full(&self) -> bool {
        self.colors.len() >= Self::MAX_COLORS
    }

    /// Standard-Startpalette mit 16 Farben (eigene Auswahl, für Figuren und Umgebung).
    pub fn default_16() -> Self {
        let hex = [
            "#1a1c2c", "#5d275d", "#b13e53", "#ef7d57", "#ffcd75", "#a7f070", "#38b764", "#257179",
            "#29366f", "#3b5dc9", "#41a6f6", "#73eff7", "#f4f4f4", "#94b0c2", "#566c86", "#333c57",
        ];
        Self::new(
            "Standard 16",
            hex.iter().map(|h| h.parse().unwrap()).collect(),
        )
    }

    /// Palette aus Farbtreppen: `hues` Farbtöne mit je `steps` Stufen von dunkel nach hell,
    /// dazu `grays` Grautöne. Die Stufen wandern wie in klassischer Pixel-Art im Farbton:
    /// Schatten ins Kühle, Lichter ins Warme.
    pub fn ramps(name: impl Into<String>, hues: usize, steps: usize, grays: usize) -> Self {
        let mut colors = Vec::new();
        for g in 0..grays {
            let v = if grays < 2 {
                0.5
            } else {
                g as f64 / (grays - 1) as f64
            };
            colors.push(hsl(230.0, 0.12, 0.08 + 0.86 * v));
        }
        for k in 0..hues {
            let hue = k as f64 * 360.0 / hues as f64;
            for i in 0..steps {
                let t = if steps < 2 {
                    0.5
                } else {
                    i as f64 / (steps - 1) as f64
                };
                let target = if t < 0.5 { 250.0 } else { 60.0 };
                let h = toward(hue, target, (t - 0.5).abs() * 2.0 * 0.22);
                let sat = 0.78 - (t - 0.5).abs() * 0.3;
                let light = 0.16 + 0.68 * t;
                colors.push(hsl(h, sat, light));
            }
        }
        colors.truncate(Self::MAX_COLORS);
        Self::new(name, colors)
    }

    /// 32 Farben: 7 Farbtreppen mit 4 Stufen und 4 Grautöne.
    pub fn ramps_32() -> Self {
        Self::ramps(crate::lang::tr("Farbtreppen 32", "Ramps 32"), 7, 4, 4)
    }

    /// 64 Farben: 12 Farbtreppen mit 5 Stufen und 4 Grautöne.
    pub fn ramps_64() -> Self {
        Self::ramps(crate::lang::tr("Farbtreppen 64", "Ramps 64"), 12, 5, 4)
    }

    /// Graustufen mit `n` Stufen (2 bis 256).
    pub fn grayscale(n: usize) -> Self {
        let n = n.clamp(2, Self::MAX_COLORS);
        let colors = (0..n)
            .map(|i| {
                let v = (i * 255 / (n - 1)) as u8;
                Rgba::rgb(v, v, v)
            })
            .collect();
        Self::new(format!("Graustufen {n}"), colors)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::default_16()
    }
}

/// Dreht den Farbton `h` um den Anteil `share` des Abstands auf `target` zu, auf dem
/// kürzeren Weg. Verschiedene Farbtöne bleiben so verschieden.
fn toward(h: f64, target: f64, share: f64) -> f64 {
    let diff = (target - h + 540.0).rem_euclid(360.0) - 180.0;
    (h + diff * share).rem_euclid(360.0)
}

/// Farbe aus Farbton (Grad), Sättigung und Helligkeit (je 0 bis 1).
fn hsl(h: f64, s: f64, l: f64) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let v = |f: f64| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba::rgb(v(r), v(g), v(b))
}
