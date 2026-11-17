//! Formen zum Aufziehen: Herz, Raute, Stern und andere, dazu eigene Formen,
//! die aus Gezeichnetem gespeichert werden.
//!
//! Eine Form ist eine Maske, die auf jede Größe passt. Gezeichnet wird sie wie
//! Rechteck und Ellipse: gefüllt oder nur ihr Rand, pixelgenau.

use crate::draw::Target;
use crate::{Error, Result, Rgba, Sprite};

/// Die eingebauten Formen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Heart,
    Diamond,
    Star,
    Triangle,
    Cross,
    Hexagon,
    Arrow,
    Circle,
}

/// Ab diesem Anteil in Prozent gehört ein Pixel zur Form.
const COVERAGE: u32 = 40;

impl Shape {
    pub const ALL: [Shape; 8] = [
        Shape::Heart,
        Shape::Diamond,
        Shape::Star,
        Shape::Triangle,
        Shape::Cross,
        Shape::Hexagon,
        Shape::Arrow,
        Shape::Circle,
    ];

    /// Name für Claude und Dateien.
    pub fn id(self) -> &'static str {
        match self {
            Shape::Heart => "heart",
            Shape::Diamond => "diamond",
            Shape::Star => "star",
            Shape::Triangle => "triangle",
            Shape::Cross => "cross",
            Shape::Hexagon => "hexagon",
            Shape::Arrow => "arrow",
            Shape::Circle => "circle",
        }
    }

    pub fn label(self) -> &'static str {
        use crate::lang::tr;
        match self {
            Shape::Heart => tr("Herz", "Heart"),
            Shape::Diamond => tr("Raute", "Diamond"),
            Shape::Star => tr("Stern", "Star"),
            Shape::Triangle => tr("Dreieck", "Triangle"),
            Shape::Cross => tr("Kreuz", "Cross"),
            Shape::Hexagon => tr("Sechseck", "Hexagon"),
            Shape::Arrow => tr("Pfeil", "Arrow"),
            Shape::Circle => tr("Kreis", "Circle"),
        }
    }

    pub fn parse(name: &str) -> Option<Shape> {
        let name = name.trim().to_ascii_lowercase();
        Shape::ALL.into_iter().find(|s| s.id() == name)
    }

    /// Ob der Punkt `(u, v)` in der Form liegt, beide von 0 bis 1 über die Fläche.
    fn contains(self, u: f64, v: f64) -> bool {
        // Mittig um 0, von −1 bis 1, y nach oben.
        let (x, y) = (u * 2.0 - 1.0, 1.0 - v * 2.0);
        match self {
            Shape::Heart => {
                // Zwei Kreise oben, darunter ein Keil bis zur Spitze.
                let lobe = |cx: f64| (x - cx).powi(2) + (y - 0.5).powi(2) <= 0.25;
                lobe(-0.5) || lobe(0.5) || (y <= 0.5 && x.abs() <= (y + 1.15) / 1.65)
            }
            Shape::Diamond => x.abs() + y.abs() <= 1.0,
            Shape::Star => inside_polygon(&star_points(), x, y),
            Shape::Triangle => y >= -1.0 && x.abs() <= (1.0 - y) / 2.0,
            Shape::Cross => x.abs() <= 1.0 / 3.0 || y.abs() <= 1.0 / 3.0,
            Shape::Hexagon => x.abs() <= 1.0 && x.abs() * 0.5 + y.abs() <= 1.0,
            Shape::Arrow => {
                // Spitze nach rechts: Schaft links, Kopf rechts.
                (x <= 0.1 && y.abs() <= 0.35) || (x > 0.1 && y.abs() <= (1.0 - x) / 0.9)
            }
            Shape::Circle => x * x + y * y <= 1.0,
        }
    }

    /// Die Maske in `w × h` Pixeln, zeilenweise.
    pub fn mask(self, w: u32, h: u32) -> Vec<bool> {
        // Jeder Pixel wird an 4×4 Stellen geprüft; reicht die Abdeckung, gehört er
        // dazu. So bleiben auch dünne Spitzen sichtbar.
        const N: u32 = 4;
        let mut out = Vec::with_capacity((w * h) as usize);
        for py in 0..h {
            for px in 0..w {
                let mut hits = 0;
                for sy in 0..N {
                    for sx in 0..N {
                        let sub = |p: u32, s: u32, size: u32| {
                            (f64::from(p) + (f64::from(s) + 0.5) / f64::from(N)) / f64::from(size)
                        };
                        hits += u32::from(self.contains(sub(px, sx, w), sub(py, sy, h)));
                    }
                }
                out.push(hits * 100 >= N * N * COVERAGE);
            }
        }
        // Sehr kleine Formen verlieren sonst alle Pixel; dann wenigstens die Mitte.
        if !out.contains(&true) && w > 0 && h > 0 {
            out[(h / 2 * w + w / 2) as usize] = true;
        }
        out
    }
}

fn star_points() -> Vec<(f64, f64)> {
    (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 1.05 } else { 0.47 };
            let a = std::f64::consts::FRAC_PI_2 + f64::from(i) * std::f64::consts::PI / 5.0;
            (r * a.cos(), r * a.sin() - 0.1)
        })
        .collect()
}

fn inside_polygon(points: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Eine eigene Form: ein kleines Bild, dessen gefüllte Pixel die Form ergeben.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomShape {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Rgba>,
}

impl CustomShape {
    /// Die Form aus einem Ausschnitt einer Ebene, auf den gezeichneten Teil zugeschnitten.
    pub fn from_layer(
        name: &str,
        s: &Sprite,
        t: Target,
        area: Option<(i64, i64, u32, u32)>,
    ) -> Result<Self> {
        let layer = s.layer(t.frame, t.layer)?;
        let (w, h) = (s.width() as i64, s.height() as i64);
        let (ax, ay, aw, ah) = area.unwrap_or((0, 0, s.width(), s.height()));
        let (x0, y0) = (ax.clamp(0, w), ay.clamp(0, h));
        let (x1, y1) = (
            (ax + i64::from(aw)).clamp(0, w),
            (ay + i64::from(ah)).clamp(0, h),
        );
        let mut bounds: Option<(i64, i64, i64, i64)> = None;
        for y in y0..y1 {
            for x in x0..x1 {
                if !layer.get(x, y)?.is_transparent() {
                    bounds = Some(match bounds {
                        None => (x, y, x + 1, y + 1),
                        Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x + 1), d.max(y + 1)),
                    });
                }
            }
        }
        let Some((bx0, by0, bx1, by1)) = bounds else {
            return Err(Error::Effect(
                crate::lang::tr(
                    "Dort ist nichts gezeichnet, aus dem eine Form werden könnte",
                    "nothing is drawn there that could become a shape",
                )
                .into(),
            ));
        };
        let mut pixels = Vec::new();
        for y in by0..by1 {
            for x in bx0..bx1 {
                pixels.push(layer.get(x, y)?);
            }
        }
        Ok(Self {
            name: name.trim().to_string(),
            width: (bx1 - bx0) as u32,
            height: (by1 - by0) as u32,
            pixels,
        })
    }

    /// Farbe des Pixels, der bei Größe `w × h` an `(px, py)` liegt (nächster Nachbar).
    fn sample(&self, px: u32, py: u32, w: u32, h: u32) -> Rgba {
        let sx = (u64::from(px) * u64::from(self.width) / u64::from(w.max(1))) as u32;
        let sy = (u64::from(py) * u64::from(self.height) / u64::from(h.max(1))) as u32;
        let i = (sy.min(self.height - 1) * self.width + sx.min(self.width - 1)) as usize;
        self.pixels[i]
    }

    pub fn mask(&self, w: u32, h: u32) -> Vec<bool> {
        let mut out = Vec::with_capacity((w * h) as usize);
        for py in 0..h {
            for px in 0..w {
                out.push(!self.sample(px, py, w, h).is_transparent());
            }
        }
        out
    }

    /// Die Farben der Form in `w × h`, für „eigene Farben behalten“.
    pub fn colors(&self, w: u32, h: u32) -> Vec<Rgba> {
        let mut out = Vec::with_capacity((w * h) as usize);
        for py in 0..h {
            for px in 0..w {
                out.push(self.sample(px, py, w, h));
            }
        }
        out
    }
}

/// Nur der Rand einer Maske: gefüllte Pixel mit einem leeren Nachbarn oder am Rand der Fläche.
pub fn edge(mask: &[bool], w: u32, h: u32) -> Vec<bool> {
    let (w, h) = (w as i64, h as i64);
    let at = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h && mask[(y * w + x) as usize];
    let mut out = vec![false; mask.len()];
    for y in 0..h {
        for x in 0..w {
            let open = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dy)| !at(x + dx, y + dy));
            if at(x, y) && open {
                out[(y * w + x) as usize] = true;
            }
        }
    }
    out
}

/// Spiegelt eine Maske oder ein Farbraster der Größe `w × h`.
pub fn flip<T: Copy>(cells: &[T], w: u32, h: u32, horizontal: bool, vertical: bool) -> Vec<T> {
    let mut out = Vec::with_capacity(cells.len());
    for y in 0..h {
        for x in 0..w {
            let sx = if horizontal { w - 1 - x } else { x };
            let sy = if vertical { h - 1 - y } else { y };
            out.push(cells[(sy * w + sx) as usize]);
        }
    }
    out
}

/// Zeichnet eine Maske der Größe `w × h` mit der linken oberen Ecke auf `(x, y)`.
/// Mit `colors` behält jeder Pixel seine eigene Farbe, sonst gilt `c`.
#[allow(clippy::too_many_arguments)]
pub fn draw_mask(
    s: &mut Sprite,
    t: Target,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    mask: &[bool],
    c: Rgba,
    colors: Option<&[Rgba]>,
) -> Result<usize> {
    let mut points = Vec::new();
    for py in 0..h {
        for px in 0..w {
            let i = (py * w + px) as usize;
            if mask[i] {
                let color = colors.map_or(c, |cs| cs[i]);
                points.push((x + i64::from(px), y + i64::from(py), color));
            }
        }
    }
    let mut conformed = std::collections::HashMap::new();
    for &(_, _, color) in &points {
        if !conformed.contains_key(&color) {
            let k = s.conform(color);
            conformed.insert(color, k);
        }
    }
    let layer = s.layer_mut(t.frame, t.layer)?;
    let mut n = 0;
    for (px, py, color) in points {
        if layer.in_bounds(px, py) {
            layer.set_clipped(px, py, conformed[&color]);
            n += 1;
        }
    }
    Ok(n)
}

/// Zeichnet eine eingebaute Form in das Rechteck `(x, y, w, h)`.
#[allow(clippy::too_many_arguments)]
pub fn draw_shape(
    s: &mut Sprite,
    t: Target,
    shape: Shape,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    c: Rgba,
    filled: bool,
) -> Result<usize> {
    check_size(w, h)?;
    let mut mask = shape.mask(w, h);
    if !filled {
        mask = edge(&mask, w, h);
    }
    draw_mask(s, t, x, y, w, h, &mask, c, None)
}

/// Zeichnet eine eigene Form in das Rechteck `(x, y, w, h)`.
#[allow(clippy::too_many_arguments)]
pub fn draw_custom(
    s: &mut Sprite,
    t: Target,
    shape: &CustomShape,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    c: Rgba,
    filled: bool,
    keep_colors: bool,
) -> Result<usize> {
    check_size(w, h)?;
    let mut mask = shape.mask(w, h);
    if !filled {
        mask = edge(&mask, w, h);
    }
    let colors = keep_colors.then(|| shape.colors(w, h));
    draw_mask(s, t, x, y, w, h, &mask, c, colors.as_deref())
}

fn check_size(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 || w > crate::MAX_SIZE || h > crate::MAX_SIZE {
        return Err(Error::Effect(crate::trf!(
            "Eine Form braucht 1 bis {} Pixel Breite und Höhe",
            "a shape needs a width and height of 1 to {} pixels",
            crate::MAX_SIZE
        )));
    }
    Ok(())
}
