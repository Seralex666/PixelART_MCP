//! Prüf-Hinweise: typische Versehen in Pixel-Art finden.
//!
//! Alles hier sind Hinweise, keine Fehler. Ein einzelner Pixel kann ein Stern
//! sein und eine Lücke im Umriss Absicht. Die Prüfungen sind deshalb vorsichtig
//! und schweigen lieber, als bei jedem Bild etwas zu melden.

use crate::{Rgba, Sprite};
use std::collections::HashMap;

/// Mehr Stellen nennt ein Hinweis nicht, die Zahl stimmt trotzdem.
pub const MAX_PLACES: usize = 6;
/// Mehr einzelne Pixel als so viele sind Absicht (Sterne, Funken, Rauschen).
const MAX_STRAY: usize = 12;
/// Mehr Einzelfarben als so viele sind Absicht (Verläufe, importierte Fotos).
const MAX_RARE: usize = 6;
/// Ab so vielen Pixeln lohnt die Suche nach Einzelfarben.
const MIN_PIXELS: usize = 16;
/// Ab so vielen Randpixeln lässt sich ein Umriss beurteilen.
const MIN_EDGE: usize = 12;
/// Geprüft werden höchstens so viele Pixel je Sprite, damit große Bilder mit
/// vielen Frames die Projektübersicht nicht aufhalten.
const BUDGET: usize = 4 * 1024 * 1024;

/// Was geprüft wird.
#[derive(Clone, Copy, Debug, Default)]
pub struct Checks {
    /// Die Umrissfarbe, falls das Projekt eine festlegt. Sonst gilt die Farbe,
    /// die am Rand am häufigsten vorkommt.
    pub outline: Option<Rgba>,
    /// Auch prüfen, ob sich das Bild nahtlos kacheln lässt. Nur für freie Bilder
    /// sinnvoll, die die ganze Fläche füllen.
    pub seams: bool,
}

/// Ein Befund in einem Frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    /// Einzelne Pixel ohne jeden Nachbarn.
    StrayPixels { count: usize, at: Vec<(u32, u32)> },
    /// Der Umriss hat fast überall die Farbe `color`, an diesen Stellen nicht.
    OutlineGaps {
        color: Rgba,
        count: usize,
        at: Vec<(u32, u32)>,
    },
    /// Farben, die nur ein einziger Pixel hat.
    SingleUseColors { colors: Vec<(Rgba, (u32, u32))> },
    /// Die gegenüberliegenden Ränder passen nicht zueinander. `horizontal`
    /// meint den Übergang vom rechten zum linken Rand.
    Seam { horizontal: bool },
}

/// Ein Befund und die Frames, in denen er vorkommt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub frames: Vec<usize>,
    pub finding: Finding,
}

/// Prüft alle Frames eines Sprites. Gleiche Befunde in mehreren Frames werden
/// zu einem Hinweis zusammengefasst.
pub fn check(s: &Sprite, checks: Checks) -> Vec<Hint> {
    let all: Vec<usize> = (0..s.frames().len()).collect();
    check_frames(s, &all, checks)
}

/// Wie [`check`], aber nur für die genannten Frames.
pub fn check_frames(s: &Sprite, frames: &[usize], checks: Checks) -> Vec<Hint> {
    let (w, h) = (s.width() as usize, s.height() as usize);
    let affordable = (BUDGET / (w * h).max(1)).max(1);
    let mut hints: Vec<Hint> = Vec::new();
    let mut seen: Vec<(Vec<Rgba>, usize)> = Vec::new();
    for &f in frames.iter().take(affordable) {
        let Ok(pixels) = s.composite(f) else {
            continue;
        };
        // Ein Frame, der einem früheren gleicht, hat auch dessen Befunde.
        if let Some((_, first)) = seen.iter().find(|(p, _)| *p == pixels) {
            for hint in hints.iter_mut().filter(|h| h.frames.contains(first)) {
                hint.frames.push(f);
            }
            continue;
        }
        for finding in check_pixels(&pixels, w, h, checks) {
            match hints.iter_mut().find(|h| h.finding == finding) {
                Some(hint) => hint.frames.push(f),
                None => hints.push(Hint {
                    frames: vec![f],
                    finding,
                }),
            }
        }
        seen.push((pixels, f));
    }
    hints
}

fn check_pixels(pixels: &[Rgba], w: usize, h: usize, checks: Checks) -> Vec<Finding> {
    let at = |x: i64, y: i64| -> Option<Rgba> {
        (x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h)
            .then(|| pixels[y as usize * w + x as usize])
    };
    let filled = |x: i64, y: i64| at(x, y).is_some_and(|c| !c.is_transparent());
    let opaque = pixels.iter().filter(|c| !c.is_transparent()).count();
    let mut out = Vec::new();
    if opaque == 0 {
        return out;
    }

    // Einzelne Pixel: keiner der acht Nachbarn ist gefüllt.
    let mut stray = Vec::new();
    // Randpixel: gefüllt, mit einem leeren Nachbarn innerhalb des Bildes.
    let mut edge: Vec<((u32, u32), Rgba)> = Vec::new();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !filled(x, y) {
                continue;
            }
            let around = (-1..=1)
                .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                .filter(|&d| d != (0, 0))
                .any(|(dx, dy)| filled(x + dx, y + dy));
            if !around {
                stray.push((x as u32, y as u32));
                continue;
            }
            let open = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dy)| at(x + dx, y + dy).is_some_and(|c| c.is_transparent()));
            if open {
                edge.push(((x as u32, y as u32), pixels[y as usize * w + x as usize]));
            }
        }
    }
    if !stray.is_empty() && stray.len() <= MAX_STRAY && opaque > stray.len() {
        out.push(Finding::StrayPixels {
            count: stray.len(),
            at: stray.iter().copied().take(MAX_PLACES).collect(),
        });
    }

    // Umriss: Hat fast der ganze Rand eine Farbe, fallen die übrigen Stellen auf.
    if edge.len() >= MIN_EDGE {
        let mut counts: HashMap<Rgba, usize> = HashMap::new();
        for (_, c) in &edge {
            *counts.entry(*c).or_insert(0) += 1;
        }
        let (color, share_needed) = match checks.outline {
            Some(c) => (c, 0.5),
            None => {
                let most = counts
                    .iter()
                    .max_by_key(|(c, n)| (**n, std::cmp::Reverse(c.to_array())))
                    .map(|(c, _)| *c)
                    .unwrap_or(Rgba::TRANSPARENT);
                (most, 0.8)
            }
        };
        let with = counts.get(&color).copied().unwrap_or(0);
        let share = with as f64 / edge.len() as f64;
        if with < edge.len() && share >= share_needed {
            let gaps: Vec<(u32, u32)> = edge
                .iter()
                .filter(|(_, c)| *c != color)
                .map(|(p, _)| *p)
                .collect();
            out.push(Finding::OutlineGaps {
                color,
                count: gaps.len(),
                at: gaps.into_iter().take(MAX_PLACES).collect(),
            });
        }
    }

    // Farben, die genau ein Pixel hat.
    if opaque >= MIN_PIXELS {
        let mut uses: HashMap<Rgba, (usize, (u32, u32))> = HashMap::new();
        for (i, &c) in pixels.iter().enumerate() {
            if !c.is_transparent() {
                let place = ((i % w) as u32, (i / w) as u32);
                uses.entry(c).or_insert((0, place)).0 += 1;
            }
        }
        let mut single: Vec<(Rgba, (u32, u32))> = uses
            .into_iter()
            .filter(|(_, (n, _))| *n == 1)
            .map(|(c, (_, place))| (c, place))
            .collect();
        single.sort_unstable_by_key(|(_, (x, y))| (*y, *x));
        if !single.is_empty() && single.len() <= MAX_RARE {
            out.push(Finding::SingleUseColors { colors: single });
        }
    }

    // Kacheln: Der Sprung über den Rand soll nicht größer sein als die Sprünge im Bild.
    if checks.seams && opaque == pixels.len() && w >= 8 && h >= 8 {
        let px = |x: usize, y: usize| pixels[y * w + x];
        let step = |a: Rgba, b: Rgba| (a.distance_sq(b) as f64).sqrt();
        // Mittlerer Sprung von Spalte `a` zu Spalte `b` und von Zeile `a` zu Zeile `b`.
        let columns =
            |a: usize, b: usize| (0..h).map(|y| step(px(a, y), px(b, y))).sum::<f64>() / h as f64;
        let rows =
            |a: usize, b: usize| (0..w).map(|x| step(px(x, a), px(x, b))).sum::<f64>() / w as f64;
        let inside = (0..w - 1).map(|x| columns(x, x + 1)).fold(0.0, f64::max);
        if seam(columns(w - 1, 0), inside) {
            out.push(Finding::Seam { horizontal: true });
        }
        let inside = (0..h - 1).map(|y| rows(y, y + 1)).fold(0.0, f64::max);
        if seam(rows(h - 1, 0), inside) {
            out.push(Finding::Seam { horizontal: false });
        }
    }
    out
}

/// Ein Rand fällt auf, wenn der Sprung darüber deutlich größer ist als der
/// größte Sprung zwischen Nachbarn im Bild und auch für sich genommen sichtbar.
/// Der größte, nicht der mittlere: Muster mit harten Kanten (Ziegel, Schachbrett)
/// dürfen am Rand genauso springen wie innen.
fn seam(across: f64, inside: f64) -> bool {
    across > 24.0 && across > inside * 2.0
}
