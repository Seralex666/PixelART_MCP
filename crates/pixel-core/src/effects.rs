//! Pixel-Art-Techniken für eine Ebene: Kontur, Licht und Schatten, Verläufe in
//! harten Stufen oder gerastert, Körnung, Formeln je Pixel und Stempeln.
//!
//! Alles arbeitet mit ganzen Pixeln und ohne Glättung. Jede geschriebene Farbe
//! läuft über [`Sprite::conform`], hält also Farbtiefe und gesperrte Paletten
//! ein. Zufall kommt aus einem festen Startwert: gleiche Eingaben ergeben auf
//! jedem Rechner dasselbe Bild.

use crate::draw::Target;
use crate::expr::{bayer_threshold, hash, unit, Env, Expr};
use crate::{Error, Result, Rgba, Sprite};
use std::collections::HashMap;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Effect(message.into()))
}

/// Rechteckiger Ausschnitt; was über die Leinwand hinausragt, entfällt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// Welche Pixel eines Ausschnitts eine Technik anfasst.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mask {
    All,
    /// Nur Pixel, auf denen schon etwas gezeichnet ist.
    Opaque,
    /// Nur leere Pixel.
    Transparent,
    /// Nur Pixel genau dieser Farbe.
    Color(Rgba),
}

/// Die ausgewählten Pixel einer Ebene.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    width: usize,
    height: usize,
    inside: Vec<bool>,
    /// Kleinstes Rechteck um die Auswahl: `(x0, y0, x1, y1)`, die oberen Grenzen exklusiv.
    bounds: Option<(usize, usize, usize, usize)>,
}

impl Selection {
    pub fn new(s: &Sprite, t: Target, area: Option<Area>, mask: Mask) -> Result<Self> {
        let layer = s.layer(t.frame, t.layer)?;
        let (width, height) = (s.width() as usize, s.height() as usize);
        let (x0, y0, x1, y1) = match area {
            None => (0, 0, width, height),
            Some(a) => {
                let clip = |v: i64, max: usize| v.clamp(0, max as i64) as usize;
                (
                    clip(a.x, width),
                    clip(a.y, height),
                    clip(a.x.saturating_add(a.w as i64), width),
                    clip(a.y.saturating_add(a.h as i64), height),
                )
            }
        };
        let wanted = match mask {
            Mask::Color(c) => Some(s.depth.conform(c)),
            _ => None,
        };
        let pixels = layer.pixels();
        let mut inside = vec![false; width * height];
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = pixels[y * width + x];
                let hit = match mask {
                    Mask::All => true,
                    Mask::Opaque => !p.is_transparent(),
                    Mask::Transparent => p.is_transparent(),
                    Mask::Color(c) => p == c || Some(p) == wanted,
                };
                if hit {
                    inside[y * width + x] = true;
                    bounds = Some(match bounds {
                        None => (x, y, x + 1, y + 1),
                        Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x + 1), d.max(y + 1)),
                    });
                }
            }
        }
        Ok(Self {
            width,
            height,
            inside,
            bounds,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.bounds.is_none()
    }

    pub fn len(&self) -> usize {
        self.inside.iter().filter(|&&b| b).count()
    }

    /// Ob der Pixel `(x, y)` ausgewählt ist.
    pub fn contains_point(&self, x: i64, y: i64) -> bool {
        self.contains(x, y)
    }

    fn contains(&self, x: i64, y: i64) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.width
            && (y as usize) < self.height
            && self.inside[y as usize * self.width + x as usize]
    }

    /// Alle ausgewählten Pixel, zeilenweise.
    fn points(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let (x0, y0, x1, y1) = self.bounds.unwrap_or((0, 0, 0, 0));
        (y0..y1)
            .flat_map(move |y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| self.inside[y * self.width + x])
    }
}

/// Schreibt Pixel und bringt jede Farbe vorher auf Farbtiefe und Palette.
fn write(s: &mut Sprite, t: Target, pixels: Vec<(usize, usize, Rgba)>) -> Result<usize> {
    let mut conformed: HashMap<Rgba, Rgba> = HashMap::new();
    for &(_, _, c) in &pixels {
        conformed.entry(c).or_insert_with(|| s.conform(c));
    }
    let layer = s.layer_mut(t.frame, t.layer)?;
    for &(x, y, c) in &pixels {
        layer.set(x as i64, y as i64, conformed[&c])?;
    }
    Ok(pixels.len())
}

// ---------------------------------------------------------------- Kontur

/// Zieht eine Kontur um alles, was auf der Ebene gezeichnet ist. Die Kontur
/// liegt außen auf bisher leeren Pixeln. Mit `diagonals` zählen auch
/// Eck-Nachbarn, die Kontur wird dann an Schrägen dicker.
pub fn outline(
    s: &mut Sprite,
    t: Target,
    color: Rgba,
    thickness: u32,
    diagonals: bool,
) -> Result<usize> {
    if color.is_transparent() {
        return fail("an outline needs a visible color");
    }
    let (w, h) = (s.width() as i64, s.height() as i64);
    let mut filled: Vec<bool> = s
        .layer(t.frame, t.layer)?
        .pixels()
        .iter()
        .map(|p| !p.is_transparent())
        .collect();
    let mut added = Vec::new();
    for _ in 0..thickness.clamp(1, 16) {
        let before = filled.clone();
        let at =
            |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h && before[(y * w + x) as usize];
        for y in 0..h {
            for x in 0..w {
                if at(x, y) {
                    continue;
                }
                let straight = at(x - 1, y) || at(x + 1, y) || at(x, y - 1) || at(x, y + 1);
                let corner = diagonals
                    && (at(x - 1, y - 1)
                        || at(x + 1, y - 1)
                        || at(x - 1, y + 1)
                        || at(x + 1, y + 1));
                if straight || corner {
                    filled[(y * w + x) as usize] = true;
                    added.push((x as usize, y as usize, color));
                }
            }
        }
    }
    write(s, t, added)
}

// ---------------------------------------------------------------- Licht

/// Licht und Schatten für eine Fläche. `light` zeigt zur Lichtquelle, zum
/// Beispiel `(-1, -1)` für oben links.
///
/// `ramp` läuft von dunkel nach hell. Die erste Hälfte färbt die Ränder, die
/// vom Licht wegzeigen (die dunkelste Farbe ganz außen), die zweite Hälfte die
/// Ränder zum Licht hin (die hellste ganz außen). Bei ungerader Anzahl ist die
/// mittlere Farbe die Grundfarbe und füllt den Rest der Fläche.
pub fn shade(
    s: &mut Sprite,
    t: Target,
    sel: &Selection,
    light: (i64, i64),
    ramp: &[Rgba],
) -> Result<usize> {
    if ramp.len() < 2 {
        return fail("`ramp` needs at least 2 colors, from dark to light");
    }
    if light == (0, 0) {
        return fail("the light needs a direction");
    }
    let (lx, ly) = (light.0.signum(), light.1.signum());
    let depth = (ramp.len() / 2) as i64;
    let base = (ramp.len() % 2 == 1).then(|| ramp[ramp.len() / 2]);
    // Wie viele Schritte es in einer Richtung bis zum Rand der Fläche sind (1 = Randpixel).
    let steps = |x: usize, y: usize, dx: i64, dy: i64| -> i64 {
        let dirs = [(dx, 0), (0, dy), (dx, dy)];
        dirs.iter()
            .filter(|d| **d != (0, 0))
            .map(|&(ax, ay)| {
                (1..=depth)
                    .find(|k| !sel.contains(x as i64 + k * ax, y as i64 + k * ay))
                    .unwrap_or(depth + 1)
            })
            .min()
            .unwrap_or(depth + 1)
    };
    let mut out = Vec::new();
    for (x, y) in sel.points() {
        let lit = steps(x, y, lx, ly);
        let dark = steps(x, y, -lx, -ly);
        // Dünne Stellen liegen an beiden Rändern zugleich: dort gewinnt das Licht.
        let color = if lit <= depth && lit <= dark {
            Some(ramp[ramp.len() - lit as usize])
        } else if dark <= depth {
            Some(ramp[dark as usize - 1])
        } else {
            base
        };
        if let Some(c) = color {
            out.push((x, y, c));
        }
    }
    write(s, t, out)
}

// ---------------------------------------------------------------- Verläufe

/// Wie ein Verlauf über die Fläche läuft.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flow {
    /// Gerade in Richtung `(dx, dy)`, über die ganze Auswahl.
    Linear { dx: f64, dy: f64 },
    /// Vom Mittelpunkt nach außen. Ohne `radius` bis zur fernsten Ecke der Auswahl.
    Radial {
        cx: f64,
        cy: f64,
        radius: Option<f64>,
    },
}

/// Rastermuster für Übergänge mit wenigen Farben.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Bayer2,
    Bayer4,
    Bayer8,
    /// Nur drei Stufen: erste Farbe, Schachbrett, zweite Farbe.
    Checker,
}

impl Pattern {
    fn threshold(self, x: usize, y: usize) -> f64 {
        let (x, y) = (x as i64, y as i64);
        match self {
            Pattern::Bayer2 => bayer_threshold(x, y, 1),
            Pattern::Bayer4 => bayer_threshold(x, y, 2),
            Pattern::Bayer8 => bayer_threshold(x, y, 3),
            Pattern::Checker => {
                if (x + y) % 2 == 0 {
                    1.0 / 3.0
                } else {
                    2.0 / 3.0
                }
            }
        }
    }
}

/// Lage jedes ausgewählten Pixels im Verlauf, von 0 (Anfang) bis 1 (Ende).
fn positions(sel: &Selection, flow: Flow) -> Result<Vec<(usize, usize, f64)>> {
    let Some((x0, y0, x1, y1)) = sel.bounds else {
        return Ok(Vec::new());
    };
    let corners = [
        (x0 as f64, y0 as f64),
        ((x1 - 1) as f64, y0 as f64),
        (x0 as f64, (y1 - 1) as f64),
        ((x1 - 1) as f64, (y1 - 1) as f64),
    ];
    let place: Box<dyn Fn(f64, f64) -> f64> = match flow {
        Flow::Linear { dx, dy } => {
            if !(dx.is_finite() && dy.is_finite()) || (dx == 0.0 && dy == 0.0) {
                return fail("the gradient needs a direction");
            }
            let along = move |x: f64, y: f64| x * dx + y * dy;
            let values = corners.map(|(x, y)| along(x, y));
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let span = max - min;
            Box::new(move |x, y| {
                if span <= 0.0 {
                    0.0
                } else {
                    (along(x, y) - min) / span
                }
            })
        }
        Flow::Radial { cx, cy, radius } => {
            if !(cx.is_finite() && cy.is_finite()) {
                return fail("the gradient center is not a number");
            }
            let far = move |x: f64, y: f64| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            let radius = match radius {
                Some(r) if r.is_finite() && r > 0.0 => r,
                Some(_) => return fail("`radius` must be greater than 0"),
                None => corners.iter().map(|&(x, y)| far(x, y)).fold(0.0, f64::max),
            };
            Box::new(move |x, y| {
                if radius <= 0.0 {
                    0.0
                } else {
                    far(x, y) / radius
                }
            })
        }
    };
    Ok(sel
        .points()
        .map(|(x, y)| (x, y, place(x as f64, y as f64).clamp(0.0, 1.0)))
        .collect())
}

/// Verteilt `colors` gleichmäßig auf `steps` Stufen. Zwischenfarben werden gemischt.
fn bands(colors: &[Rgba], steps: Option<usize>) -> Vec<Rgba> {
    let n = steps.unwrap_or(colors.len()).clamp(1, 256);
    if n == colors.len() || colors.len() == 1 {
        return colors.to_vec();
    }
    (0..n)
        .map(|i| {
            let at = if n == 1 {
                0.0
            } else {
                i as f64 / (n - 1) as f64 * (colors.len() - 1) as f64
            };
            let lower = (at.floor() as usize).min(colors.len() - 1);
            let upper = (lower + 1).min(colors.len() - 1);
            let part = at - lower as f64;
            let blend = |a: u8, b: u8| {
                (a as f64 + (b as f64 - a as f64) * part)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            let (a, b) = (colors[lower], colors[upper]);
            Rgba {
                r: blend(a.r, b.r),
                g: blend(a.g, b.g),
                b: blend(a.b, b.b),
                a: blend(a.a, b.a),
            }
        })
        .collect()
}

/// Verlauf durch `colors`. Ohne `pattern` in harten Stufen, mit `pattern`
/// gerastert: die Farben selbst bleiben rein, nur ihr Anteil ändert sich.
/// `steps` legt die Zahl der Stufen fest; sind es mehr als Farben, werden
/// Zwischenfarben gemischt.
pub fn gradient(
    s: &mut Sprite,
    t: Target,
    sel: &Selection,
    colors: &[Rgba],
    flow: Flow,
    steps: Option<usize>,
    pattern: Option<Pattern>,
) -> Result<usize> {
    if colors.is_empty() {
        return fail("a gradient needs at least one color");
    }
    let colors = bands(colors, steps);
    let n = colors.len();
    let mut out = Vec::new();
    for (x, y, at) in positions(sel, flow)? {
        let color = match pattern {
            None => colors[((at * n as f64) as usize).min(n - 1)],
            Some(_) if n == 1 => colors[0],
            Some(pattern) => {
                let scaled = at * (n - 1) as f64;
                let lower = (scaled.floor() as usize).min(n - 2);
                let part = scaled - lower as f64;
                if part > pattern.threshold(x, y) {
                    colors[lower + 1]
                } else {
                    colors[lower]
                }
            }
        };
        out.push((x, y, color));
    }
    write(s, t, out)
}

// ---------------------------------------------------------------- Körnung

/// Streut Pixel in zufällig gewählten Farben aus `colors` über die Auswahl.
/// `density` ist der Anteil der getroffenen Pixel, von 0 bis 1.
pub fn noise(
    s: &mut Sprite,
    t: Target,
    sel: &Selection,
    colors: &[Rgba],
    density: f64,
    seed: u64,
) -> Result<usize> {
    if colors.is_empty() {
        return fail("noise needs at least one color");
    }
    if !(0.0..=1.0).contains(&density) {
        return fail("`density` must be between 0 and 1");
    }
    let out = sel
        .points()
        .filter(|&(x, y)| unit(hash(seed, x as i64, y as i64, 1)) < density)
        .map(|(x, y)| {
            let pick = hash(seed, x as i64, y as i64, 2) % colors.len() as u64;
            (x, y, colors[pick as usize])
        })
        .collect();
    write(s, t, out)
}

// ---------------------------------------------------------------- Formel

/// Rechnet `formula` für jeden ausgewählten Pixel aus. Das Ergebnis, abgerundet,
/// wählt eine Farbe aus `colors`: 0 die erste, 1 die zweite und so weiter.
/// Werte über dem Ende nehmen die letzte Farbe, negative lassen den Pixel
/// unverändert.
pub fn shader(
    s: &mut Sprite,
    t: Target,
    sel: &Selection,
    formula: &Expr,
    colors: &[Rgba],
    seed: u64,
) -> Result<usize> {
    if colors.is_empty() {
        return fail("a shader needs at least one color");
    }
    let env = Env::new(s.width(), s.height(), t.frame, s.frames().len(), seed);
    let mut out = Vec::new();
    for (x, y) in sel.points() {
        let value = formula.eval(&env.at(x as u32, y as u32));
        if value.is_nan() || value < 0.0 {
            continue;
        }
        let index = (value.floor() as usize).min(colors.len() - 1);
        out.push((x, y, colors[index]));
    }
    write(s, t, out)
}

// ---------------------------------------------------------------- Stempel

/// Ein ausgeschnittenes Stück Bild.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Rgba>,
}

impl Patch {
    /// Schneidet `area` aus einem Frame: aus einer Ebene oder, ohne `layer`,
    /// aus dem fertigen Bild aller sichtbaren Ebenen.
    pub fn cut(s: &Sprite, frame: usize, layer: Option<usize>, area: Option<Area>) -> Result<Self> {
        let source: Vec<Rgba> = match layer {
            Some(l) => s.layer(frame, l)?.pixels().to_vec(),
            None => s.composite(frame)?,
        };
        let (w, h) = (s.width() as i64, s.height() as i64);
        let a = area.unwrap_or(Area {
            x: 0,
            y: 0,
            w: s.width(),
            h: s.height(),
        });
        let (x0, y0) = (a.x.clamp(0, w), a.y.clamp(0, h));
        let x1 = a.x.saturating_add(a.w as i64).clamp(0, w);
        let y1 = a.y.saturating_add(a.h as i64).clamp(0, h);
        if x1 <= x0 || y1 <= y0 {
            return fail("the source area lies outside the canvas");
        }
        let mut pixels = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                pixels.push(source[(y * w + x) as usize]);
            }
        }
        Ok(Self {
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
            pixels,
        })
    }

    fn get(&self, x: u32, y: u32) -> Rgba {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn flipped(&self, horizontal: bool, vertical: bool) -> Self {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for y in 0..self.height {
            for x in 0..self.width {
                let sx = if horizontal { self.width - 1 - x } else { x };
                let sy = if vertical { self.height - 1 - y } else { y };
                pixels.push(self.get(sx, sy));
            }
        }
        Self {
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    /// Dreht im Uhrzeigersinn um 0, 90, 180 oder 270 Grad.
    pub fn rotated(&self, degrees: u32) -> Result<Self> {
        let (w, h) = (self.width, self.height);
        let (nw, nh) = match degrees {
            0 => return Ok(self.clone()),
            180 => (w, h),
            90 | 270 => (h, w),
            other => return fail(format!("`rotate` must be 0, 90, 180 or 270, got {other}")),
        };
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for y in 0..nh {
            for x in 0..nw {
                pixels.push(match degrees {
                    90 => self.get(y, h - 1 - x),
                    180 => self.get(w - 1 - x, h - 1 - y),
                    _ => self.get(w - 1 - y, x),
                });
            }
        }
        Ok(Self {
            width: nw,
            height: nh,
            pixels,
        })
    }

    /// Tauscht Farben aus: jedes Paar ist `(von, nach)`.
    pub fn recolored(mut self, map: &[(Rgba, Rgba)]) -> Self {
        if !map.is_empty() {
            let lookup: HashMap<Rgba, Rgba> = map.iter().copied().collect();
            for p in &mut self.pixels {
                if let Some(&to) = lookup.get(p) {
                    *p = to;
                }
            }
        }
        self
    }
}

/// Setzt ein Stück Bild mit der linken oberen Ecke auf `(x, y)`. Leere Pixel
/// des Stücks lassen stehen, was darunter liegt.
pub fn stamp(s: &mut Sprite, t: Target, patch: &Patch, x: i64, y: i64) -> Result<usize> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let mut out = Vec::new();
    for py in 0..patch.height {
        for px in 0..patch.width {
            let c = patch.get(px, py);
            let (tx, ty) = (x.saturating_add(px as i64), y.saturating_add(py as i64));
            if !c.is_transparent() && tx >= 0 && ty >= 0 && tx < w && ty < h {
                out.push((tx as usize, ty as usize, c));
            }
        }
    }
    write(s, t, out)
}
