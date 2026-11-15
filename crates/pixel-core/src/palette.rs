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
