//! Farbreduktion: viele Farben auf wenige ähnliche bringen.

use crate::Rgba;

/// Eine Gruppe ähnlicher Farben und die Farbe, die sie ersetzt.
pub(crate) struct ColorBox {
    /// Durchschnitt der Gruppe, gewichtet nach Häufigkeit.
    pub mean: Rgba,
    pub members: Vec<Rgba>,
}

/// Teilt Farben in höchstens `max` Gruppen ähnlicher Farben ("Median Cut").
///
/// Alle Farben liegen zuerst in einer Kiste. Die Kiste mit der größten
/// Farbspanne wird immer wieder dort geteilt, wo die Hälfte ihrer Pixel
/// erreicht ist, bis es `max` Kisten sind. `colors` nennt jede Farbe einmal,
/// zusammen mit der Zahl der Pixel, die sie haben.
pub(crate) fn median_cut(mut colors: Vec<(Rgba, u64)>, max: usize) -> Vec<ColorBox> {
    // Feste Reihenfolge, damit dieselbe Eingabe immer dasselbe Ergebnis gibt.
    colors.sort_unstable_by_key(|(c, _)| c.to_array());
    let channel = |c: Rgba, i: usize| c.to_array()[i];
    // Größte Spanne einer Kiste und der Farbkanal, in dem sie liegt.
    let spread = |b: &[(Rgba, u64)]| {
        (0..4)
            .map(|i| {
                let values = b.iter().map(|(c, _)| channel(*c, i));
                let (lo, hi) = values.fold((255, 0), |(lo, hi), v| (lo.min(v), hi.max(v)));
                (hi.saturating_sub(lo), i)
            })
            .max()
            .unwrap_or((0, 0))
    };
    let mut boxes = vec![colors];
    while boxes.len() < max {
        let Some((at, (_, by))) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| (i, spread(b)))
            .max_by_key(|&(_, (range, _))| range)
        else {
            break;
        };
        let b = &mut boxes[at];
        b.sort_unstable_by_key(|(c, _)| (channel(*c, by), c.to_array()));
        // Teilen, wo die Hälfte der Pixel erreicht ist, aber nie am Rand.
        let half = b.iter().map(|(_, n)| n).sum::<u64>() / 2;
        let mut seen = 0;
        let mut cut = b.len() - 1;
        for (i, (_, n)) in b.iter().enumerate() {
            seen += n;
            if seen >= half {
                cut = (i + 1).min(b.len() - 1);
                break;
            }
        }
        let upper = b.split_off(cut.max(1));
        boxes.push(upper);
    }
    boxes
        .into_iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let total = b.iter().map(|(_, n)| n).sum::<u64>().max(1);
            let mean = |ch: usize| {
                let sum: u64 = b.iter().map(|(c, n)| u64::from(channel(*c, ch)) * n).sum();
                ((sum + total / 2) / total) as u8
            };
            ColorBox {
                mean: Rgba::new(mean(0), mean(1), mean(2), mean(3)),
                members: b.into_iter().map(|(c, _)| c).collect(),
            }
        })
        .collect()
}

/// Über so viele Farben sucht [`similar`] nicht mehr paarweise: das würde zu
/// lange dauern. Was darüber liegt, fasst vorher [`median_cut`] grob zusammen.
const MAX_PAIRWISE: usize = 512;

/// Teilt Farben in höchstens `max` Gruppen, indem immer wieder die zwei
/// ähnlichsten zusammenfallen. Für die Gruppe steht ihre häufigste Farbe, es
/// entsteht also keine neue Mischfarbe: das Ergebnis ist eine Auswahl aus den
/// Farben, die es schon gab. Deutlich verschiedene Farben bleiben getrennt,
/// auch wenn sie selten sind. Genau das braucht Pixel-Art, wo ein einzelner
/// heller Pixel ein Glanzlicht sein kann.
pub(crate) fn similar(colors: Vec<(Rgba, u64)>, max: usize) -> Vec<ColorBox> {
    let max = max.max(1);
    // Je Gruppe: die Farbe, die für sie steht, ihre Pixelzahl und ihre Mitglieder.
    let mut groups: Vec<(Rgba, u64, Vec<Rgba>)> = if colors.len() > MAX_PAIRWISE {
        let counts: std::collections::HashMap<Rgba, u64> = colors.iter().copied().collect();
        median_cut(colors, MAX_PAIRWISE)
            .into_iter()
            .map(|b| {
                let n = b.members.iter().map(|c| counts[c]).sum();
                (b.mean, n, b.members)
            })
            .collect()
    } else {
        colors.into_iter().map(|(c, n)| (c, n, vec![c])).collect()
    };
    // Feste Reihenfolge, damit dieselbe Eingabe immer dasselbe Ergebnis gibt.
    groups.sort_unstable_by_key(|(c, _, _)| c.to_array());
    while groups.len() > max {
        let mut best = (u32::MAX, u64::MAX, 0, 1);
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                let d = groups[i].0.distance_sq(groups[j].0);
                let fewer = groups[i].1.min(groups[j].1);
                if (d, fewer) < (best.0, best.1) {
                    best = (d, fewer, i, j);
                }
            }
        }
        let (_, _, i, j) = best;
        let (color, n, members) = groups.remove(j);
        let into = &mut groups[i];
        if n > into.1 {
            into.0 = color;
        }
        into.1 += n;
        into.2.extend(members);
    }
    groups
        .into_iter()
        .map(|(mean, _, members)| ColorBox { mean, members })
        .collect()
}

/// Helligkeit einer Farbe, 0 (schwarz) bis 255000 (weiß).
pub fn luminance(c: Rgba) -> u32 {
    299 * u32::from(c.r) + 587 * u32::from(c.g) + 114 * u32::from(c.b)
}

/// Höchstens `max` Farben, die ein Bild wiedergeben, von dunkel nach hell.
/// Ähnliche Farben fallen zusammen, die häufigste bleibt. Durchsichtige Pixel zählen nicht, halb durchsichtige gelten als
/// deckend. Gibt außerdem zurück, wie viele Farben das Bild hatte.
pub fn palette_of(pixels: impl IntoIterator<Item = Rgba>, max: usize) -> (Vec<Rgba>, usize) {
    let mut counts: std::collections::HashMap<Rgba, u64> = std::collections::HashMap::new();
    for p in pixels {
        if p.a >= 128 {
            *counts.entry(Rgba::rgb(p.r, p.g, p.b)).or_insert(0) += 1;
        }
    }
    let found = counts.len();
    let mut colors: Vec<Rgba> = similar(counts.into_iter().collect(), max)
        .into_iter()
        .map(|b| b.mean)
        .collect();
    colors.sort_unstable_by_key(|c| (luminance(*c), c.to_array()));
    colors.dedup();
    (colors, found)
}

/// Was [`reduce`] getan hat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reduced {
    /// Zahl der Farben vorher und nachher.
    pub before: usize,
    pub after: usize,
    /// Die Farben danach, die häufigsten zuerst.
    pub colors: Vec<Rgba>,
}

/// Bringt Sprites gemeinsam auf höchstens `max` Farben. Ähnliche Farben fallen
/// zusammen, jeder Pixel bekommt die häufigste Farbe seiner Gruppe. Alle Sprites erhalten
/// danach dieselbe Palette aus genau diesen Farben; eine Figur und ihre
/// Animationen bleiben so einheitlich. Ob eine Palette gesperrt ist, ändert
/// sich nicht.
pub fn reduce(sprites: &mut [&mut crate::Sprite], max: usize) -> crate::Result<Reduced> {
    use std::collections::HashMap;
    let max = max.clamp(1, crate::Palette::MAX_COLORS);
    let Some(depth) = sprites.first().map(|s| s.depth) else {
        return Ok(Reduced {
            before: 0,
            after: 0,
            colors: Vec::new(),
        });
    };
    let mut counts: HashMap<Rgba, u64> = HashMap::new();
    for s in sprites.iter() {
        for l in s
            .frames()
            .iter()
            .flat_map(|f| &f.layers)
            .filter(|l| !l.reference)
        {
            for &p in l.pixels() {
                if !p.is_transparent() {
                    *counts.entry(p).or_insert(0) += 1;
                }
            }
        }
    }
    let before = counts.len();
    if before <= max {
        // Nichts zu tun. Auch die Palette bleibt, samt ihrer unbenutzten Farben.
        let mut colors: Vec<(Rgba, u64)> = counts.into_iter().collect();
        colors.sort_unstable_by_key(|(c, n)| (std::cmp::Reverse(*n), c.to_array()));
        return Ok(Reduced {
            before,
            after: before,
            colors: colors.into_iter().map(|(c, _)| c).collect(),
        });
    }
    let mut target: HashMap<Rgba, Rgba> = HashMap::new();
    {
        for group in similar(counts.iter().map(|(c, n)| (*c, *n)).collect(), max) {
            // Der Durchschnitt darf nicht durchsichtig werden: sonst verschwänden Pixel.
            let mut mean = depth.conform(group.mean);
            if mean.is_transparent() {
                mean = group
                    .members
                    .iter()
                    .copied()
                    .max_by_key(|c| (counts[c], c.to_array()))
                    .unwrap_or(mean);
            }
            for c in group.members {
                target.insert(c, mean);
            }
        }
    }
    let mut used: HashMap<Rgba, u64> = HashMap::new();
    for (c, n) in &counts {
        *used
            .entry(target.get(c).copied().unwrap_or(*c))
            .or_insert(0) += n;
    }
    let mut used: Vec<(Rgba, u64)> = used.into_iter().collect();
    used.sort_unstable_by_key(|(c, n)| (std::cmp::Reverse(*n), c.to_array()));
    let colors: Vec<Rgba> = used.into_iter().map(|(c, _)| c).collect();
    for s in sprites.iter_mut() {
        for i in 0..s.frames().len() {
            for l in s.frame_mut(i)?.layers.iter_mut().filter(|l| !l.reference) {
                l.map_pixels(|p| target.get(&p).copied().unwrap_or(p));
            }
        }
        s.palette.colors = colors.clone();
    }
    Ok(Reduced {
        before,
        after: colors.len(),
        colors,
    })
}
