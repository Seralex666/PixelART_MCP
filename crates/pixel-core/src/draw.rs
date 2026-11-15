//! Zeichenwerkzeuge. Alle arbeiten pixelgenau, ohne Anti-Aliasing, und
//! bringen die Farbe vor dem Schreiben auf die Farbtiefe des Sprites.
//! Koordinaten außerhalb des Bildes werden abgeschnitten.

use crate::{Result, Rgba, Sprite};

/// Größter Betrag, den eine Koordinate oder Größe haben darf. Formen dürfen weit
/// über den Bildrand hinausragen, aber nicht beliebig weit, damit keine Operation
/// Milliarden unsichtbarer Pixel durchläuft.
pub const COORD_LIMIT: i64 = 16 * crate::MAX_SIZE as i64;

fn check_coords(values: &[i64]) -> Result<()> {
    match values.iter().find(|v| v.abs() > COORD_LIMIT) {
        Some(&v) => Err(crate::Error::CoordOutOfRange(v)),
        None => Ok(()),
    }
}

/// Wohin gezeichnet wird: Frame und Ebene eines Sprites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub frame: usize,
    pub layer: usize,
}

impl Target {
    pub const fn new(frame: usize, layer: usize) -> Self {
        Self { frame, layer }
    }
}

/// Setzt mehrere Pixel in einer Farbe.
pub fn pixels(s: &mut Sprite, t: Target, points: &[(i64, i64)], c: Rgba) -> Result<()> {
    let c = s.conform(c);
    let l = s.layer_mut(t.frame, t.layer)?;
    for &(x, y) in points {
        l.set_clipped(x, y, c);
    }
    Ok(())
}

/// Linie nach Bresenham, 1 Pixel breit.
pub fn line(s: &mut Sprite, t: Target, from: (i64, i64), to: (i64, i64), c: Rgba) -> Result<()> {
    check_coords(&[from.0, from.1, to.0, to.1])?;
    let c = s.conform(c);
    let l = s.layer_mut(t.frame, t.layer)?;
    let (mut x, mut y) = from;
    let dx = (to.0 - x).abs();
    let dy = -(to.1 - y).abs();
    let sx = if x < to.0 { 1 } else { -1 };
    let sy = if y < to.1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        l.set_clipped(x, y, c);
        if (x, y) == to {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    Ok(())
}

/// Rechteck. `w` und `h` sind die Größe in Pixeln; `filled` füllt die Fläche.
#[allow(clippy::too_many_arguments)]
pub fn rect(
    s: &mut Sprite,
    t: Target,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    c: Rgba,
    filled: bool,
) -> Result<()> {
    check_coords(&[x, y, w as i64, h as i64])?;
    let c = s.conform(c);
    let l = s.layer_mut(t.frame, t.layer)?;
    if w == 0 || h == 0 {
        return Ok(());
    }
    // Auf das Bild begrenzen, damit riesige Rechtecke nicht unnötig iterieren.
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w as i64 - 1).min(l.width() as i64 - 1);
    let y1 = (y + h as i64 - 1).min(l.height() as i64 - 1);
    let (rx1, ry1) = (x + w as i64 - 1, y + h as i64 - 1);
    for py in y0..=y1 {
        for px in x0..=x1 {
            if filled || px == x || px == rx1 || py == y || py == ry1 {
                l.set_clipped(px, py, c);
            }
        }
    }
    Ok(())
}

/// Ellipse, die genau in das Rechteck `x, y, w, h` passt.
#[allow(clippy::too_many_arguments)]
pub fn ellipse(
    s: &mut Sprite,
    t: Target,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    c: Rgba,
    filled: bool,
) -> Result<()> {
    check_coords(&[x, y, w as i64, h as i64])?;
    let c = s.conform(c);
    let l = s.layer_mut(t.frame, t.layer)?;
    if w == 0 || h == 0 {
        return Ok(());
    }
    // Pro Zeile wird die Spanne der Ellipse berechnet (relativ zu x), symmetrisch
    // für gerade und ungerade Größen.
    let (wf, hf) = (w as f64, h as f64);
    let span = |row: i64| -> Option<(i64, i64)> {
        if row < 0 || row >= h as i64 {
            return None;
        }
        let cy = (row as f64 + 0.5) / hf * 2.0 - 1.0;
        let half = (1.0 - cy * cy).max(0.0).sqrt() * wf / 2.0;
        let left = (wf / 2.0 - half).round() as i64;
        let right = (wf / 2.0 + half).round() as i64 - 1;
        (right >= left).then_some((left, right))
    };
    // Nur sichtbare Zeilen und Spalten durchlaufen, egal wie groß die Ellipse ist.
    let first_row = (-y).max(0);
    let last_row = (l.height() as i64 - 1 - y).min(h as i64 - 1);
    let (min_px, max_px) = (-x, l.width() as i64 - 1 - x);
    for row in first_row..=last_row {
        let Some((left, right)) = span(row) else {
            continue;
        };
        let py = y + row;
        let (above, below) = (span(row - 1), span(row + 1));
        for px in left.max(min_px)..=right.min(max_px) {
            // Umriss: Randpixel der Zeile plus alles, was über die Nachbarzeilen
            // hinausragt, damit der Umriss oben und unten geschlossen ist.
            let inside = |n: Option<(i64, i64)>| n.is_some_and(|(nl, nr)| px > nl && px < nr);
            if filled || px == left || px == right || !inside(above) || !inside(below) {
                l.set_clipped(x + px, py, c);
            }
        }
    }
    Ok(())
}

/// Füllt die zusammenhängende Fläche gleicher Farbe ab `(x, y)` (4er-Nachbarschaft).
/// Gibt die Anzahl gefüllter Pixel zurück.
pub fn flood_fill(s: &mut Sprite, t: Target, x: i64, y: i64, c: Rgba) -> Result<usize> {
    let c = s.conform(c);
    let l = s.layer_mut(t.frame, t.layer)?;
    let target = l.get(x, y)?;
    if target == c {
        return Ok(0);
    }
    let mut stack = vec![(x, y)];
    let mut count = 0;
    while let Some((px, py)) = stack.pop() {
        if !l.in_bounds(px, py) || l.get(px, py)? != target {
            continue;
        }
        l.set(px, py, c)?;
        count += 1;
        stack.extend([(px + 1, py), (px - 1, py), (px, py + 1), (px, py - 1)]);
    }
    Ok(count)
}

/// Ersetzt eine Farbe überall auf der Ebene. Gibt die Anzahl geänderter Pixel zurück.
pub fn replace_color(s: &mut Sprite, t: Target, from: Rgba, to: Rgba) -> Result<usize> {
    let to = s.conform(to);
    let l = s.layer_mut(t.frame, t.layer)?;
    let mut count = 0;
    l.map_pixels(|p| {
        if p == from {
            count += 1;
            to
        } else {
            p
        }
    });
    Ok(count)
}

/// Verschiebt den Inhalt einer Ebene um `(dx, dy)` Pixel. Was aus dem Bild fällt, geht verloren;
/// zurückgegeben wird die Anzahl der so verlorenen Pixel.
/// Praktisch für Animationen (z. B. Oberkörper beim Atmen 1 Pixel anheben).
pub fn shift(s: &mut Sprite, t: Target, dx: i64, dy: i64) -> Result<usize> {
    let l = s.layer_mut(t.frame, t.layer)?;
    let (w, h) = (l.width() as i64, l.height() as i64);
    let old = l.pixels().to_vec();
    l.clear();
    let mut lost = 0;
    for y in 0..h {
        for x in 0..w {
            let p = old[(y * w + x) as usize];
            if p.is_transparent() {
                continue;
            }
            // saturating_add: auch absurde Verschiebungen dürfen nicht überlaufen.
            let (nx, ny) = (x.saturating_add(dx), y.saturating_add(dy));
            if l.in_bounds(nx, ny) {
                l.set_clipped(nx, ny, p);
            } else {
                lost += 1;
            }
        }
    }
    Ok(lost)
}

/// Spiegelt eine Ebene horizontal (z. B. für die Blickrichtung).
pub fn flip_horizontal(s: &mut Sprite, t: Target) -> Result<()> {
    let l = s.layer_mut(t.frame, t.layer)?;
    let w = l.width() as usize;
    let mut px = l.pixels().to_vec();
    for row in px.chunks_exact_mut(w) {
        row.reverse();
    }
    l.replace_pixels(px)
}
