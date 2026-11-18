//! Drehen in jedem Winkel ohne Pixelbrei, nach dem RotSprite-Verfahren:
//! Das Bild wird dreimal mit Scale2x vergrößert (8×), dort gedreht und beim
//! Verkleinern wird je Pixel genau eine Farbe genommen. So entstehen keine neuen
//! Farben, Linien bleiben geschlossen und eine gesperrte Palette passt weiter.

use crate::{Error, Result, Rgba, Sprite};

/// Vergrößerung vor dem Drehen (3 × Scale2x).
const UPSCALE: usize = 8;

/// Scale2x (EPX): verdoppelt das Bild und rundet Treppenstufen ab, ohne neue Farben.
fn scale2x(px: &[Rgba], w: usize, h: usize) -> Vec<Rgba> {
    let at = |x: isize, y: isize, p: Rgba| -> Rgba {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
            p
        } else {
            px[y as usize * w + x as usize]
        }
    };
    let mut out = vec![Rgba::TRANSPARENT; w * h * 4];
    let ow = w * 2;
    for y in 0..h {
        for x in 0..w {
            let p = px[y * w + x];
            let (xi, yi) = (x as isize, y as isize);
            let a = at(xi, yi - 1, p);
            let b = at(xi + 1, yi, p);
            let c = at(xi - 1, yi, p);
            let d = at(xi, yi + 1, p);
            let e0 = if c == a && c != d && a != b { a } else { p };
            let e1 = if a == b && a != c && b != d { b } else { p };
            let e2 = if d == c && d != b && c != a { c } else { p };
            let e3 = if b == d && b != a && d != c { d } else { p };
            out[(2 * y) * ow + 2 * x] = e0;
            out[(2 * y) * ow + 2 * x + 1] = e1;
            out[(2 * y + 1) * ow + 2 * x] = e2;
            out[(2 * y + 1) * ow + 2 * x + 1] = e3;
        }
    }
    out
}

/// Sinus und Kosinus, bei Vielfachen von 90° exakt.
fn sin_cos(angle: f32) -> (f64, f64) {
    let a = (angle as f64).rem_euclid(360.0);
    for (q, sc) in [
        (0.0, (0.0, 1.0)),
        (90.0, (1.0, 0.0)),
        (180.0, (0.0, -1.0)),
        (270.0, (-1.0, 0.0)),
    ] {
        if (a - q).abs() < 1e-6 || (a - q - 360.0).abs() < 1e-6 {
            return sc;
        }
    }
    a.to_radians().sin_cos()
}

/// Größe des Rechtecks, das ein um `angle` gedrehtes Bild ganz umfasst.
pub fn rotated_size(w: u32, h: u32, angle: f32) -> (u32, u32) {
    let (s, c) = sin_cos(angle);
    let (w, h) = (w as f64, h as f64);
    let nw = (w * c.abs() + h * s.abs() - 1e-6).ceil().max(1.0);
    let nh = (w * s.abs() + h * c.abs() - 1e-6).ceil().max(1.0);
    (nw as u32, nh as u32)
}

/// Dreht ein Pixelbild um `angle` Grad im Uhrzeigersinn um `pivot`.
///
/// Das Ergebnis ist `out` groß; `out_pivot` ist dort der Drehpunkt. Pixel
/// außerhalb des Bildes werden durchsichtig. Es kommen nur Farben aus `px` vor.
pub fn rotsprite(
    px: &[Rgba],
    (w, h): (u32, u32),
    angle: f32,
    pivot: (f32, f32),
    out: (u32, u32),
    out_pivot: (f32, f32),
) -> Vec<Rgba> {
    let (w, h) = (w as usize, h as usize);
    let (ow, oh) = (out.0 as usize, out.1 as usize);
    let mut result = vec![Rgba::TRANSPARENT; ow * oh];
    if px.iter().all(|p| p.is_transparent()) {
        return result;
    }
    let (s, c) = sin_cos(angle);
    let exact = s == 0.0 || c == 0.0;
    // Bei Vielfachen von 90° reicht das Original, sonst die 8-fache Vergrößerung.
    let (big, k) = if exact {
        (px.to_vec(), 1)
    } else {
        let mut big = px.to_vec();
        let (mut bw, mut bh) = (w, h);
        while bw < w * UPSCALE {
            big = scale2x(&big, bw, bh);
            bw *= 2;
            bh *= 2;
        }
        (big, UPSCALE)
    };
    let bw = w * k;
    let (px0, py0) = (pivot.0 as f64, pivot.1 as f64);
    let (qx, qy) = (out_pivot.0 as f64, out_pivot.1 as f64);
    for oy in 0..oh {
        for ox in 0..ow {
            // Pixelmitte zurückdrehen (gegen den Uhrzeigersinn) und im Original nachsehen.
            let dx = ox as f64 + 0.5 - qx;
            let dy = oy as f64 + 0.5 - qy;
            let sx = c * dx + s * dy + px0;
            let sy = -s * dx + c * dy + py0;
            if sx < 0.0 || sy < 0.0 || sx >= w as f64 || sy >= h as f64 {
                continue;
            }
            let bx = ((sx * k as f64).floor() as usize).min(w * k - 1);
            let by = ((sy * k as f64).floor() as usize).min(h * k - 1);
            result[oy * ow + ox] = big[by * bw + bx];
        }
    }
    result
}

/// Was gedreht wird.
#[derive(Clone, Debug, PartialEq)]
pub struct RotateOptions {
    /// Winkel in Grad im Uhrzeigersinn.
    pub angle: f32,
    /// Nur diese Ebene; sonst alle gezeichneten Ebenen (ohne Vorlagen und
    /// automatische Ebenen, die sich selbst neu zeichnen).
    pub layer: Option<usize>,
    /// Nur dieser Frame; sonst alle.
    pub frame: Option<usize>,
    /// Drehpunkt als Pixel; sonst die Bildmitte.
    pub pivot: Option<(u32, u32)>,
    /// Leinwand vergrößern, damit nichts abgeschnitten wird.
    pub expand: bool,
    /// Über die Frames verteilen: Frame i dreht um `angle · i / Frames`. Mit 360°
    /// ergibt das eine nahtlose Drehung im Kreis.
    pub spin: bool,
    /// Mindestens so viele Frames, fehlende entstehen als Kopien (für `spin`).
    pub frames: Option<usize>,
}

impl RotateOptions {
    pub fn new(angle: f32) -> Self {
        Self {
            angle,
            layer: None,
            frame: None,
            pivot: None,
            expand: false,
            spin: false,
            frames: None,
        }
    }
}

fn fail<T>(msg: String) -> Result<T> {
    Err(Error::Effect(msg))
}

impl Sprite {
    /// Dreht Ebenen dieses Sprites mit RotSprite (siehe [`RotateOptions`]).
    pub fn rotate(&mut self, o: &RotateOptions) -> Result<()> {
        if !o.angle.is_finite() || o.angle.abs() > 3600.0 {
            return fail(crate::trf!(
                "Ungültiger Winkel {}",
                "invalid angle {}",
                o.angle
            ));
        }
        if o.spin && o.frame.is_some() {
            return fail(
                crate::lang::tr(
                    "Verteilen über die Frames geht nur mit allen Frames",
                    "spin needs all frames; leave out `frame`",
                )
                .to_string(),
            );
        }
        if let Some(l) = o.layer {
            if l >= self.layer_count() {
                return Err(Error::NoSuchLayer(l));
            }
            if self.is_reference(l) || self.is_auto_layer(l) {
                return fail(
                    crate::lang::tr(
                        "Diese Ebene zeichnet sich selbst; drehe die gezeichneten Ebenen",
                        "this layer is drawn automatically or is a guide; rotate the drawn layers",
                    )
                    .to_string(),
                );
            }
        }
        if let Some(f) = o.frame {
            self.frame(f)?;
        }
        if let Some((x, y)) = o.pivot {
            if x >= self.width() || y >= self.height() {
                return fail(crate::trf!(
                    "Drehpunkt {x},{y} liegt außerhalb des Bildes",
                    "pivot {x},{y} is outside the image"
                ));
            }
        }
        if let Some(n) = o.frames {
            self.ensure_frames(n)?;
        }
        let frames: Vec<usize> = match o.frame {
            Some(f) => vec![f],
            None => (0..self.frames().len()).collect(),
        };
        let count = self.frames().len();
        let angle_of = |f: usize| {
            if o.spin {
                o.angle * f as f32 / count as f32
            } else {
                o.angle
            }
        };
        let mut pivot = match o.pivot {
            Some((x, y)) => (x as f32 + 0.5, y as f32 + 0.5),
            None => (self.width() as f32 / 2.0, self.height() as f32 / 2.0),
        };
        if o.expand {
            // Die Leinwand wächst gleichmäßig, bis jeder Winkel hineinpasst.
            let (w, h) = (self.width(), self.height());
            let (mut nw, mut nh) = (w, h);
            for &f in &frames {
                let (rw, rh) = rotated_size(w, h, angle_of(f));
                nw = nw.max(rw);
                nh = nh.max(rh);
            }
            if (nw, nh) != (w, h) {
                if nw > crate::MAX_SIZE || nh > crate::MAX_SIZE {
                    return fail(crate::trf!(
                        "Gedreht wäre das Bild {nw}×{nh} groß, erlaubt sind {} Pixel",
                        "rotated, the image would be {nw}×{nh}; the limit is {} pixels",
                        crate::MAX_SIZE
                    ));
                }
                let dx = (nw - w) / 2;
                let dy = (nh - h) / 2;
                self.resize_canvas(nw, nh, dx as i64, dy as i64)?;
                pivot = (pivot.0 + dx as f32, pivot.1 + dy as f32);
            }
        }
        let layers: Vec<usize> = match o.layer {
            Some(l) => vec![l],
            None => (0..self.layer_count())
                .filter(|&l| !self.is_reference(l) && !self.is_auto_layer(l))
                .collect(),
        };
        let size = (self.width(), self.height());
        for &f in &frames {
            let angle = angle_of(f);
            for &l in &layers {
                let layer = self.layer_mut(f, l)?;
                let turned = rotsprite(layer.pixels(), size, angle, pivot, size, pivot);
                layer.replace_pixels(turned)?;
            }
        }
        self.refresh_outline()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorDepth, Palette};

    const R: Rgba = Rgba::rgb(200, 40, 40);
    const B: Rgba = Rgba::rgb(20, 20, 30);

    fn bar() -> (Vec<Rgba>, (u32, u32)) {
        // Ein waagrechter Balken 9×3 mit dunklem Rand in einem 15×15-Bild.
        let (w, h) = (15u32, 15u32);
        let mut px = vec![Rgba::TRANSPARENT; (w * h) as usize];
        for y in 6..9 {
            for x in 3..12 {
                px[(y * w + x) as usize] = if y == 7 && (4..11).contains(&x) { R } else { B };
            }
        }
        (px, (w, h))
    }

    #[test]
    fn no_rotation_and_full_turns_keep_the_image() {
        let (px, size) = bar();
        let c = (7.5, 7.5);
        for a in [0.0, 360.0, -360.0] {
            assert_eq!(rotsprite(&px, size, a, c, size, c), px, "{a}");
        }
    }

    #[test]
    fn quarter_turns_are_exact() {
        let (px, size) = bar();
        let c = (7.5, 7.5);
        let q = rotsprite(&px, size, 90.0, c, size, c);
        // Aus dem waagrechten Balken wird ein senkrechter.
        let w = size.0 as usize;
        assert_eq!(q[3 * w + 7], B);
        assert_eq!(q[7 * w + 7], R);
        assert!(q[7 * w + 3].is_transparent());
        let back = rotsprite(&q, size, -90.0, c, size, c);
        assert_eq!(back, px);
        let half = rotsprite(
            &rotsprite(&px, size, 90.0, c, size, c),
            size,
            90.0,
            c,
            size,
            c,
        );
        assert_eq!(half, rotsprite(&px, size, 180.0, c, size, c));
    }

    #[test]
    fn any_angle_keeps_the_colors_and_roughly_the_area() {
        let (px, size) = bar();
        let c = (7.5, 7.5);
        let count = |v: &[Rgba]| v.iter().filter(|p| !p.is_transparent()).count() as f32;
        for a in [15.0, 30.0, 45.0, 72.5, 200.0] {
            let r = rotsprite(&px, size, a, c, size, c);
            assert!(
                r.iter().all(|p| p.is_transparent() || *p == R || *p == B),
                "{a}"
            );
            let ratio = count(&r) / count(&px);
            assert!((0.8..1.25).contains(&ratio), "{a}: {ratio}");
            // Die rote Mitte bleibt sichtbar.
            assert!(r.contains(&R), "{a}");
        }
    }

    #[test]
    fn rotated_size_fits_the_corners() {
        assert_eq!(rotated_size(10, 4, 0.0), (10, 4));
        assert_eq!(rotated_size(10, 4, 90.0), (4, 10));
        assert_eq!(rotated_size(10, 10, 45.0), (15, 15));
    }

    #[test]
    fn sprite_rotation_expands_spins_and_skips_guides() {
        let (px, (w, h)) = bar();
        let mut s = Sprite::new(w, h, ColorDepth::Rgba32, Palette::default()).unwrap();
        s.layer_mut(0, 0)
            .unwrap()
            .replace_pixels(px.clone())
            .unwrap();
        let mut o = RotateOptions::new(45.0);
        o.expand = true;
        let mut a = s.clone();
        a.rotate(&o).unwrap();
        assert_eq!((a.width(), a.height()), rotated_size(w, h, 45.0));

        let mut spin = RotateOptions::new(360.0);
        spin.spin = true;
        spin.frames = Some(8);
        let mut b = s.clone();
        b.rotate(&spin).unwrap();
        assert_eq!(b.frames().len(), 8);
        assert_eq!(
            b.layer(0, 0).unwrap().pixels(),
            &px[..],
            "frame 0 is not turned"
        );
        let quarter = rotsprite(&px, (w, h), 90.0, (7.5, 7.5), (w, h), (7.5, 7.5));
        assert_eq!(b.layer(2, 0).unwrap().pixels(), &quarter[..]);

        spin.frame = Some(1);
        assert!(s.clone().rotate(&spin).is_err());
        let mut bad = RotateOptions::new(10.0);
        bad.pivot = Some((99, 0));
        assert!(s.clone().rotate(&bad).is_err());
    }
}
