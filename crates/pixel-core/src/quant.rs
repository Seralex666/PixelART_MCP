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
