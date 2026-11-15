use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Eine Farbe mit 8 Bit pro Kanal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    pub fn is_transparent(self) -> bool {
        self.a == 0
    }

    pub fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Quadratischer Abstand im RGBA-Raum, für die Suche nach der nächsten Palettenfarbe.
    pub fn distance_sq(self, other: Rgba) -> u32 {
        let d = |a: u8, b: u8| {
            let v = a as i32 - b as i32;
            (v * v) as u32
        };
        d(self.r, other.r) * 3 + d(self.g, other.g) * 4 + d(self.b, other.b) * 2 + d(self.a, other.a) * 3
    }

    /// Normal-Blending (Quelle über Ziel), ganzzahlig.
    pub fn over(self, below: Rgba) -> Rgba {
        if self.a == 255 || below.a == 0 {
            return self;
        }
        if self.a == 0 {
            return below;
        }
        let sa = self.a as u32;
        let da = below.a as u32 * (255 - sa) / 255;
        let out_a = sa + da;
        let mix = |s: u8, d: u8| ((s as u32 * sa + d as u32 * da) / out_a) as u8;
        Rgba::new(mix(self.r, below.r), mix(self.g, below.g), mix(self.b, below.b), out_a as u8)
    }

    /// Reduziert die Farbe auf RGB565 (16 Bit). Alpha wird auf 0 oder 255 gesetzt,
    /// weil RGB565 keinen Alphakanal hat und Pixel nur sichtbar oder unsichtbar sind.
    pub fn to_rgb565(self) -> Rgba {
        if self.a < 128 {
            return Rgba::TRANSPARENT;
        }
        let r5 = (self.r as u32 * 31 + 127) / 255;
        let g6 = (self.g as u32 * 63 + 127) / 255;
        let b5 = (self.b as u32 * 31 + 127) / 255;
        Rgba::rgb(
            ((r5 * 255 + 15) / 31) as u8,
            ((g6 * 255 + 31) / 63) as u8,
            ((b5 * 255 + 15) / 31) as u8,
        )
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(f, "#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

impl FromStr for Rgba {
    type Err = crate::Error;

    fn from_str(s: &str) -> crate::Result<Self> {
        let err = || crate::Error::InvalidColor(s.to_string());
        let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
        if !hex.is_ascii() || !(hex.len() == 6 || hex.len() == 8) {
            return Err(err());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err());
        let a = if hex.len() == 8 { byte(6)? } else { 255 };
        Ok(Rgba::new(byte(0)?, byte(2)?, byte(4)?, a))
    }
}

impl Serialize for Rgba {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Farbtiefe eines Dokuments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorDepth {
    /// 8 Bit indiziert: nur Farben aus der Palette (höchstens 256) plus Transparenz.
    Indexed8,
    /// 16 Bit „High Color“ (RGB565): 65.536 Farben, Pixel sind sichtbar oder transparent.
    Rgb565,
    /// 32 Bit RGBA: volle Farben mit stufenlosem Alphakanal.
    #[default]
    Rgba32,
}

impl ColorDepth {
    pub fn bits(self) -> u8 {
        match self {
            ColorDepth::Indexed8 => 8,
            ColorDepth::Rgb565 => 16,
            ColorDepth::Rgba32 => 32,
        }
    }

    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            8 => Some(ColorDepth::Indexed8),
            16 => Some(ColorDepth::Rgb565),
            32 => Some(ColorDepth::Rgba32),
            _ => None,
        }
    }
}
