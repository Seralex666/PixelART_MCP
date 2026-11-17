//! Pinsel und Bereichswerkzeuge, die Editor und Claude gleich benutzen:
//! Sprühdose, Aufhellen und Abdunkeln entlang der Palette, Spiegeln beim Zeichnen
//! und Ausschnitte verschieben, spiegeln und leeren.

use crate::draw::Target;
use crate::effects::{Area, Patch};
use crate::expr::{hash, unit};
use crate::{Error, Result, Rgba, Sprite};

/// Helligkeit von 0 bis 255.
fn luminance(c: Rgba) -> u32 {
    crate::quant::luminance(c) / 1000
}

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Effect(message.into()))
}

// ---------------------------------------------------------------- Sprühdose

/// Die Pixel, die ein Sprühstoß um `(cx, cy)` trifft: zufällig verteilt in einem
/// Kreis mit `radius`, jeder Pixel mit der Wahrscheinlichkeit `density` (0 bis 1).
/// Gleicher `seed`, gleiches Muster.
pub fn spray_points(cx: i64, cy: i64, radius: u32, density: f64, seed: u64) -> Vec<(i64, i64)> {
    let r = i64::from(radius);
    let limit = (r * r + r) as f64;
    let mut out = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            if (dx * dx + dy * dy) as f64 > limit {
                continue;
            }
            let (x, y) = (cx + dx, cy + dy);
            if unit(hash(seed, x, y, 11)) < density {
                out.push((x, y));
            }
        }
    }
    out
}

/// Ein Sprühstoß in der Farbe `c`. Gibt die Anzahl getroffener Pixel im Bild zurück.
#[allow(clippy::too_many_arguments)]
pub fn spray(
    s: &mut Sprite,
    t: Target,
    cx: i64,
    cy: i64,
    radius: u32,
    density: f64,
    c: Rgba,
    seed: u64,
) -> Result<usize> {
    if !(0.0..=1.0).contains(&density) {
        return fail("`density` must be between 0 and 1");
    }
    if radius > 64 {
        return fail("`radius` must be at most 64");
    }
    let c = s.conform(c);
    let layer = s.layer_mut(t.frame, t.layer)?;
    let mut n = 0;
    for (x, y) in spray_points(cx, cy, radius, density, seed) {
        if layer.in_bounds(x, y) {
            layer.set_clipped(x, y, c);
            n += 1;
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------- Aufhellen und Abdunkeln

/// Die nächste hellere (`lighten`) oder dunklere Stufe von `c`.
///
/// Zuerst wird in der Palette gesucht: die ähnlichste Farbe, die heller oder dunkler
/// ist. So wandert der Pinsel die Farbtreppen der Palette entlang. Gibt es dort
/// keine, wird die Farbe selbst um eine Stufe verschoben, außer die Palette ist
/// gesperrt. Leere Pixel bleiben leer.
pub fn tone_color(s: &Sprite, c: Rgba, lighten: bool) -> Rgba {
    if c.is_transparent() {
        return c;
    }
    let l = luminance(c);
    // Ein kleiner Abstand, damit fast gleich helle Farben nicht als Stufe zählen.
    let step = 6;
    let candidates = s.palette.colors.iter().copied().filter(|k| {
        !k.is_transparent()
            && if lighten {
                luminance(*k) > l + step
            } else {
                luminance(*k) + step < l
            }
    });
    // Ähnlichkeit zählt mehr als der Helligkeitsabstand: so bleibt der Farbton erhalten.
    let shift = |k: &Rgba| {
        let dl = luminance(*k).abs_diff(l);
        hue_distance(c, *k) + dl * dl / 4
    };
    let best = candidates.min_by_key(shift);
    match best {
        Some(k) if s.palette.locked || hue_distance(c, k) < 3 * 60 * 60 => {
            Rgba::new(k.r, k.g, k.b, c.a)
        }
        _ if s.palette.locked => c,
        _ => {
            let f = if lighten { 1.18 } else { 0.8 };
            let add = if lighten { 14.0 } else { 0.0 };
            let ch = |v: u8| (f64::from(v) * f + add).round().clamp(0.0, 255.0) as u8;
            s.depth.conform(Rgba::new(ch(c.r), ch(c.g), ch(c.b), c.a))
        }
    }
}

/// Abstand zweier Farben ohne ihre Helligkeit: wie verschieden ihr Farbton ist.
fn hue_distance(a: Rgba, b: Rgba) -> u32 {
    let la = luminance(a) as i32;
    let lb = luminance(b) as i32;
    let d = |x: u8, y: u8| {
        let v = (i32::from(x) - la) - (i32::from(y) - lb);
        (v * v) as u32
    };
    d(a.r, b.r) + d(a.g, b.g) + d(a.b, b.b)
}

/// Hellt oder dunkelt die Pixel an `points` je eine Stufe ab. Jeder Pixel zählt
/// einmal, auch wenn er mehrfach in `points` steht. Gibt die Anzahl geänderter Pixel zurück.
pub fn tone(s: &mut Sprite, t: Target, points: &[(i64, i64)], lighten: bool) -> Result<usize> {
    let layer = s.layer(t.frame, t.layer)?;
    let mut seen = std::collections::HashSet::new();
    let mut changes = Vec::new();
    for &(x, y) in points {
        if !layer.in_bounds(x, y) || !seen.insert((x, y)) {
            continue;
        }
        let c = layer.get(x, y)?;
        let k = tone_color(s, c, lighten);
        if k != c {
            changes.push((x, y, k));
        }
    }
    let n = changes.len();
    for (x, y, k) in changes {
        // Über conform, damit 8 Bit die Farbe in die Palette aufnimmt.
        s.set_pixel(t.frame, t.layer, x, y, k)?;
    }
    Ok(n)
}

// ---------------------------------------------------------------- Spiegeln beim Zeichnen

/// Überträgt alles, was sich gegenüber `before` auf der Ebene geändert hat, auf die
/// gespiegelte Seite: an der senkrechten Mitte (`horizontal`, links und rechts),
/// der waagerechten (`vertical`, oben und unten) oder beiden.
pub fn mirror_changes(
    s: &mut Sprite,
    t: Target,
    before: &[Rgba],
    horizontal: bool,
    vertical: bool,
) -> Result<usize> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let layer = s.layer_mut(t.frame, t.layer)?;
    if before.len() != layer.pixels().len() {
        return fail("the layer changed size while drawing");
    }
    let now = layer.pixels().to_vec();
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if now[i] == before[i] {
                continue;
            }
            for (mx, my) in mirror_positions(x, y, w, h, horizontal, vertical) {
                layer.set_clipped(mx, my, now[i]);
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Die gespiegelten Gegenstücke eines Pixels, ohne den Pixel selbst.
pub fn mirror_positions(
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    horizontal: bool,
    vertical: bool,
) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    if horizontal {
        out.push((w - 1 - x, y));
    }
    if vertical {
        out.push((x, h - 1 - y));
    }
    if horizontal && vertical {
        out.push((w - 1 - x, h - 1 - y));
    }
    out.retain(|&p| p != (x, y));
    out
}

// ---------------------------------------------------------------- Ausschnitte

/// Begrenzt einen Ausschnitt auf das Bild. `None`, wenn nichts davon im Bild liegt.
pub fn clip_area(s: &Sprite, a: Area) -> Option<Area> {
    let (w, h) = (i64::from(s.width()), i64::from(s.height()));
    let (x0, y0) = (a.x.clamp(0, w), a.y.clamp(0, h));
    let x1 = a.x.saturating_add(i64::from(a.w)).clamp(0, w);
    let y1 = a.y.saturating_add(i64::from(a.h)).clamp(0, h);
    (x1 > x0 && y1 > y0).then(|| Area {
        x: x0,
        y: y0,
        w: (x1 - x0) as u32,
        h: (y1 - y0) as u32,
    })
}

/// Leert einen Ausschnitt einer Ebene. Gibt die Anzahl geleerter Pixel zurück.
pub fn clear_area(s: &mut Sprite, t: Target, a: Area) -> Result<usize> {
    let Some(a) = clip_area(s, a) else {
        return Ok(0);
    };
    let layer = s.layer_mut(t.frame, t.layer)?;
    let mut n = 0;
    for y in a.y..a.y + i64::from(a.h) {
        for x in a.x..a.x + i64::from(a.w) {
            if !layer.get(x, y)?.is_transparent() {
                layer.set(x, y, Rgba::TRANSPARENT)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Hebt einen Ausschnitt aus der Ebene und setzt ihn um `(dx, dy)` versetzt wieder ab.
/// Gibt den neuen Ausschnitt zurück (kann teils außerhalb des Bildes liegen).
pub fn move_area(s: &mut Sprite, t: Target, a: Area, dx: i64, dy: i64) -> Result<Area> {
    let Some(a) = clip_area(s, a) else {
        return fail("the area lies outside the canvas");
    };
    let patch = Patch::cut(s, t.frame, Some(t.layer), Some(a))?;
    clear_area(s, t, a)?;
    crate::effects::stamp(s, t, &patch, a.x + dx, a.y + dy)?;
    Ok(Area {
        x: a.x + dx,
        y: a.y + dy,
        ..a
    })
}

/// Spiegelt einen Ausschnitt an Ort und Stelle, links-rechts und/oder oben-unten.
pub fn flip_area(
    s: &mut Sprite,
    t: Target,
    a: Area,
    horizontal: bool,
    vertical: bool,
) -> Result<()> {
    let Some(a) = clip_area(s, a) else {
        return fail("the area lies outside the canvas");
    };
    let patch = Patch::cut(s, t.frame, Some(t.layer), Some(a))?.flipped(horizontal, vertical);
    clear_area(s, t, a)?;
    crate::effects::stamp(s, t, &patch, a.x, a.y)?;
    Ok(())
}
