//! Normal Maps für Licht im Spiel (Godot, Unity und andere).
//!
//! Aus jedem Frame entsteht ein Bild, dessen Farben die Ausrichtung der
//! Oberfläche verschlüsseln: Rot ist die Neigung nach rechts, Grün nach oben,
//! Blau zum Betrachter (OpenGL-Art, wie Godot und Unity sie erwarten). Die
//! Form kommt aus dem Abstand zum Rand: Kanten wölben sich, die Mitte ist
//! flach. Auf Wunsch tragen helle und dunkle Farben feine Details bei.

use crate::light::edge_distance;
use crate::quant::luminance;
use crate::{ColorDepth, Error, Palette, Result, Rgba, Sprite};

/// Einstellungen einer Normal Map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalOptions {
    /// So viele Pixel breit wölbt sich der Rand (1 bis 16).
    pub bevel: u32,
    /// Wie steil die Wölbung wirkt (0.1 bis 5).
    pub strength: f32,
    /// Anteil der Details aus hellen und dunklen Farben (0 bis 1).
    pub detail: f32,
    /// Grün umdrehen (DirectX-Art) statt OpenGL-Art.
    pub flip_green: bool,
}

impl Default for NormalOptions {
    fn default() -> Self {
        Self {
            bevel: 3,
            strength: 1.0,
            detail: 0.3,
            flip_green: false,
        }
    }
}

impl NormalOptions {
    pub fn check(&self) -> Result<()> {
        if !(1..=16).contains(&self.bevel)
            || !(0.1..=5.0).contains(&self.strength)
            || !(0.0..=1.0).contains(&self.detail)
        {
            return Err(Error::Effect(
                crate::lang::tr(
                    "Normal Map: Wölbung 1 bis 16 Pixel, Stärke 0,1 bis 5, Details 0 bis 1",
                    "Normal map: bevel 1 to 16 pixels, strength 0.1 to 5, detail 0 to 1",
                )
                .into(),
            ));
        }
        Ok(())
    }
}

/// Die Höhe jedes Pixels: gewölbter Rand plus Details aus der Helligkeit.
fn heights(px: &[Rgba], w: usize, h: usize, o: &NormalOptions) -> Vec<f32> {
    let solid: Vec<bool> = px.iter().map(|c| c.a >= 128).collect();
    let dist = edge_distance(&solid, w, h, o.bevel);
    let bevel = o.bevel as f32;
    px.iter()
        .zip(&dist)
        .map(|(c, &d)| {
            if d == 0 {
                return 0.0;
            }
            // Viertelkreis: am Rand steil, zur Mitte hin flach.
            let t = (d as f32 / bevel).min(1.0);
            let dome = (1.0 - (1.0 - t) * (1.0 - t)).sqrt() * bevel;
            let light = luminance(*c) as f32 / 255_000.0;
            dome + o.detail * (light - 0.5) * 2.0
        })
        .collect()
}

/// Die Normal Map eines Bildes. Durchsichtige Pixel bleiben durchsichtig.
pub fn normal_map(px: &[Rgba], w: usize, h: usize, o: &NormalOptions) -> Vec<Rgba> {
    let hs = heights(px, w, h, o);
    let at = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0.0
        } else {
            hs[y as usize * w + x as usize]
        }
    };
    let mut out = vec![Rgba::TRANSPARENT; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let i = y as usize * w + x as usize;
            if px[i].a < 128 {
                continue;
            }
            let gx = (at(x + 1, y) - at(x - 1, y)) * 0.5 * o.strength;
            // Bildzeilen laufen nach unten, die Normale zeigt mit Grün nach oben.
            let gy = (at(x, y + 1) - at(x, y - 1)) * 0.5 * o.strength;
            let (nx, mut ny, nz) = (-gx, gy, 1.0f32);
            if o.flip_green {
                ny = -ny;
            }
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            let enc = |v: f32| {
                ((v / len) * 0.5 + 0.5)
                    .mul_add(255.0, 0.5)
                    .clamp(0.0, 255.0) as u8
            };
            out[i] = Rgba::new(enc(nx), enc(ny), enc(nz), 255);
        }
    }
    out
}

/// Ein Sprite mit der Normal Map jedes Frames auf einer Ebene, bereit zum Export.
pub fn sprite_normals(s: &Sprite, o: &NormalOptions) -> Result<Sprite> {
    o.check()?;
    let (w, h) = (s.width() as usize, s.height() as usize);
    let mut out = Sprite::new(
        s.width(),
        s.height(),
        ColorDepth::Rgba32,
        Palette::default(),
    )?;
    for f in 0..s.frames().len() {
        if f > 0 {
            out.add_frame();
        }
        let normals = normal_map(&s.composite(f)?, w, h, o);
        out.layer_mut(f, 0)?.replace_pixels(normals)?;
    }
    Ok(out)
}

/// Beleuchtet ein Bild mit seiner Normal Map, für die Vorschau.
/// `light` ist die Lampe in Pixeln (x, y) und Höhe über dem Bild (z).
pub fn lit(
    px: &[Rgba],
    normals: &[Rgba],
    w: usize,
    light: (f32, f32, f32),
    ambient: f32,
    flip_green: bool,
) -> Vec<Rgba> {
    px.iter()
        .zip(normals)
        .enumerate()
        .map(|(i, (c, n))| {
            if c.a == 0 {
                return *c;
            }
            let dec = |v: u8| f32::from(v) / 255.0 * 2.0 - 1.0;
            let (nx, mut ny, nz) = (dec(n.r), dec(n.g), dec(n.b));
            if flip_green {
                ny = -ny;
            }
            let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
            // Richtung zur Lampe, mit Grün nach oben wie in der Normal Map.
            let (lx, ly, lz) = (light.0 - x, y - light.1, light.2);
            let len = (lx * lx + ly * ly + lz * lz).sqrt().max(1e-3);
            let diffuse = ((nx * lx + ny * ly + nz * lz) / len).max(0.0);
            let k = ambient + (1.0 - ambient) * diffuse;
            let ch = |v: u8| (f32::from(v) * k * 1.15).round().clamp(0.0, 255.0) as u8;
            Rgba::new(ch(c.r), ch(c.g), ch(c.b), c.a)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_tilt_outward_and_the_middle_faces_the_viewer() {
        let (w, h) = (12usize, 12usize);
        let mut px = vec![Rgba::TRANSPARENT; w * h];
        for y in 2..10 {
            for x in 2..10 {
                px[y * w + x] = Rgba::rgb(128, 128, 128);
            }
        }
        let o = NormalOptions {
            detail: 0.0,
            ..Default::default()
        };
        let n = normal_map(&px, w, h, &o);
        let at = |x: usize, y: usize| n[y * w + x];
        assert!(at(0, 0).is_transparent());
        // Linker Rand zeigt nach links (wenig Rot), rechter nach rechts.
        assert!(at(2, 5).r < 100, "{:?}", at(2, 5));
        assert!(at(9, 5).r > 156, "{:?}", at(9, 5));
        // Oberer Rand zeigt nach oben (viel Grün), unterer nach unten.
        assert!(at(5, 2).g > 156, "{:?}", at(5, 2));
        assert!(at(5, 9).g < 100, "{:?}", at(5, 9));
        // Die Mitte schaut gerade nach vorn.
        let mid = at(5, 5);
        assert!(
            mid.r.abs_diff(128) <= 2 && mid.g.abs_diff(128) <= 2 && mid.b > 250,
            "{mid:?}"
        );
        // DirectX-Art dreht nur Grün um.
        let dx = normal_map(
            &px,
            w,
            h,
            &NormalOptions {
                flip_green: true,
                ..o
            },
        );
        assert_eq!(dx[2 * w + 5].r, at(5, 2).r);
        assert!(dx[2 * w + 5].g < 100);
        // Licht von links macht den linken Rand heller als den rechten.
        let shaded = lit(&px, &n, w, (-10.0, 6.0, 4.0), 0.3, false);
        assert!(shaded[5 * w + 2].r > shaded[5 * w + 9].r);
    }
}
