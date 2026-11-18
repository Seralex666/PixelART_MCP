//! Farbvarianten einer Figur (Skins, Teams): eine Liste „Farbe → Ersatzfarbe“.
//!
//! Eine Variante ist keine Kopie. Sie gilt beim Ansehen und Exportieren für die
//! Figur und alle ihre Animationen, darum bleibt jede Änderung an der Figur in
//! allen Varianten gleich.

use crate::workspace::name_key;
use crate::{Character, Error, Result, Rgba, Sprite};
use serde::{Deserialize, Serialize};

/// Höchstens so viele Varianten je Figur.
pub const MAX_VARIANTS: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorVariant {
    pub name: String,
    /// Paare aus Originalfarbe und Ersatz. Verglichen wird ohne Deckkraft, die
    /// Deckkraft des Pixels bleibt.
    #[serde(default)]
    pub colors: Vec<(Rgba, Rgba)>,
}

fn same_rgb(a: Rgba, b: Rgba) -> bool {
    (a.r, a.g, a.b) == (b.r, b.g, b.b)
}

impl ColorVariant {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            colors: Vec::new(),
        }
    }

    /// Die Ersatzfarbe für `c`, sonst `c` selbst.
    pub fn map(&self, c: Rgba) -> Rgba {
        if c.is_transparent() {
            return c;
        }
        match self.colors.iter().find(|(from, _)| same_rgb(*from, c)) {
            Some((_, to)) => Rgba::new(to.r, to.g, to.b, c.a),
            None => c,
        }
    }

    /// Ersatz für eine Farbe setzen; derselbe Ersatz wie das Original entfernt ihn.
    pub fn set(&mut self, from: Rgba, to: Rgba) {
        self.colors.retain(|(f, _)| !same_rgb(*f, from));
        if !same_rgb(from, to) {
            self.colors.push((from, to));
        }
    }

    /// Ersetzt die Farbe jedes Pixels und der Palette in einer Kopie des Sprites.
    pub fn apply(&self, s: &Sprite) -> Sprite {
        let mut out = s.clone();
        for f in 0..out.frames().len() {
            for l in 0..out.layer_count() {
                if let Ok(layer) = out.layer_mut(f, l) {
                    layer.map_pixels(|c| self.map(c));
                }
            }
        }
        let mut palette = out.palette.clone();
        for c in &mut palette.colors {
            *c = self.map(*c);
        }
        // Doppelte Farben nach dem Tausch zusammenfassen.
        let mut seen = Vec::new();
        palette.colors.retain(|c| {
            let new = !seen.contains(c);
            seen.push(*c);
            new
        });
        out.palette = palette;
        out
    }

    /// Ersatz für `colors` durch Drehen des Farbtons (Grad), etwa für Teamfarben.
    pub fn shift_hue(&mut self, colors: &[Rgba], degrees: f32) {
        for &c in colors {
            self.set(c, hue_shift(c, degrees));
        }
    }
}

/// Ob eine Farbe deutlich bunt ist: Grau, fast Schwarz und fast Weiß (etwa
/// Umriss und Glanzlichter) zählen nicht. Der Farbton-Schieber dreht nur diese.
pub fn is_colorful(c: Rgba) -> bool {
    let max = c.r.max(c.g).max(c.b) as f32 / 255.0;
    let min = c.r.min(c.g).min(c.b) as f32 / 255.0;
    let l = (max + min) / 2.0;
    let d = max - min;
    if !(0.15..=0.92).contains(&l) || d < 1e-6 {
        return false;
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    s >= 0.25
}

/// Dreht den Farbton einer Farbe, Helligkeit und Sättigung bleiben.
pub fn hue_shift(c: Rgba, degrees: f32) -> Rgba {
    let (r, g, b) = (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return c;
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    let h = (h + degrees).rem_euclid(360.0);
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let ch = |t: f32| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let hn = h / 360.0;
    Rgba::new(ch(hn + 1.0 / 3.0), ch(hn), ch(hn - 1.0 / 3.0), c.a)
}

impl Character {
    pub fn variant(&self, name: &str) -> Option<&ColorVariant> {
        let key = name_key(name);
        self.variants.iter().find(|v| name_key(&v.name) == key)
    }

    /// Legt eine Variante an oder ersetzt die gleichnamige.
    pub fn set_variant(&mut self, v: ColorVariant) -> Result<()> {
        let name = v.name.trim();
        if name.is_empty() {
            return Err(Error::InvalidName(
                crate::lang::tr(
                    "Die Variante braucht einen Namen",
                    "the variant needs a name",
                )
                .to_string(),
            ));
        }
        let key = name_key(name);
        match self.variants.iter_mut().find(|x| name_key(&x.name) == key) {
            Some(x) => {
                *x = ColorVariant {
                    name: name.to_string(),
                    ..v
                }
            }
            None => {
                if self.variants.len() >= MAX_VARIANTS {
                    return Err(Error::Effect(crate::trf!(
                        "Höchstens {MAX_VARIANTS} Varianten je Figur",
                        "at most {MAX_VARIANTS} variants per character"
                    )));
                }
                self.variants.push(ColorVariant {
                    name: name.to_string(),
                    ..v
                });
            }
        }
        Ok(())
    }

    /// Entfernt eine Variante; `false`, wenn es sie nicht gab.
    pub fn remove_variant(&mut self, name: &str) -> bool {
        let key = name_key(name);
        let before = self.variants.len();
        self.variants.retain(|v| name_key(&v.name) != key);
        self.variants.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorDepth, Palette};

    #[test]
    fn a_variant_swaps_colors_but_keeps_alpha() {
        let mut v = ColorVariant::new("Rot");
        let blue = Rgba::rgb(40, 60, 200);
        let red = Rgba::rgb(200, 40, 40);
        v.set(blue, red);
        assert_eq!(v.map(blue), red);
        assert_eq!(
            v.map(Rgba::new(40, 60, 200, 100)),
            Rgba::new(200, 40, 40, 100)
        );
        assert_eq!(v.map(Rgba::rgb(1, 2, 3)), Rgba::rgb(1, 2, 3));
        v.set(blue, blue);
        assert!(v.colors.is_empty());
    }

    #[test]
    fn apply_changes_pixels_and_palette() {
        let blue = Rgba::rgb(40, 60, 200);
        let pal = Palette::new("p", vec![blue, Rgba::rgb(0, 0, 0)]);
        let mut c = Character::new("Held", 4, 4, ColorDepth::Indexed8, pal).unwrap();
        c.base.layer_mut(0, 0).unwrap().fill(blue);
        let mut v = ColorVariant::new("Grün");
        v.shift_hue(&[blue], -120.0);
        let green = v.map(blue);
        assert!(green.g > green.r && green.g > green.b, "{green:?}");
        let s = v.apply(&c.base);
        assert!(s.layer(0, 0).unwrap().pixels().iter().all(|&p| p == green));
        assert!(s.palette.contains(green) && !s.palette.contains(blue));
        s.check().unwrap();

        c.set_variant(v.clone()).unwrap();
        c.set_variant(ColorVariant::new(" grün ")).unwrap();
        assert_eq!(c.variants.len(), 1);
        assert!(c.variant("GRÜN").is_some());
        assert!(c.set_variant(ColorVariant::new("  ")).is_err());
        assert!(c.remove_variant("grün"));
        assert!(!c.remove_variant("grün"));
    }

    #[test]
    fn only_clearly_colorful_colors_count() {
        assert!(is_colorful(Rgba::rgb(59, 93, 201)));
        assert!(!is_colorful(Rgba::rgb(26, 28, 44)), "outline");
        assert!(!is_colorful(Rgba::rgb(244, 244, 244)));
        assert!(!is_colorful(Rgba::rgb(120, 125, 130)));
    }

    #[test]
    fn hue_shift_keeps_grays_and_turns_full_circle() {
        let g = Rgba::rgb(90, 90, 90);
        assert_eq!(hue_shift(g, 77.0), g);
        let c = Rgba::rgb(180, 70, 30);
        let back = hue_shift(hue_shift(c, 200.0), 160.0);
        assert!(c.distance_sq(back) <= 3, "{back:?}");
    }
}
