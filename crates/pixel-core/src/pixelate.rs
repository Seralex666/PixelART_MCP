//! Bilder in Pixel-Art umwandeln: ein Foto oder eine Zeichnung auf eine kleine
//! Zielgröße bringen, die Farben auf wenige reduzieren (oder auf eine vorhandene
//! Palette), wahlweise mit Dithering, freigestelltem Hintergrund und Umriss.
//!
//! Außerdem die Vorlage zum Nachzeichnen: ein Bild in Größe der Leinwand, ohne
//! Farbreduktion, für eine halb durchsichtige Vorlage-Ebene.

use crate::quant::{luminance, median_cut};
use crate::{Error, Result, Rgba, MAX_SIZE};
use std::collections::HashMap;

/// Höchstens so viele Farben erzeugt die Umwandlung selbst.
pub const MAX_COLORS: usize = 256;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Import(message.into()))
}

/// Wie Farbübergänge mit wenigen Farben nachgebildet werden.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dither {
    /// Jeder Pixel bekommt die ähnlichste Farbe: klare Flächen, typisch für Pixel-Art.
    #[default]
    None,
    /// Ein gleichmäßiges Schachbrett-Raster (Bayer 4×4), ruhig und gut für Verläufe.
    Ordered,
    /// Fehlerverteilung (Floyd-Steinberg): am genauesten, wirkt aber körniger.
    Diffusion,
}

impl Dither {
    pub const ALL: [Dither; 3] = [Dither::None, Dither::Ordered, Dither::Diffusion];

    pub fn id(self) -> &'static str {
        match self {
            Dither::None => "none",
            Dither::Ordered => "ordered",
            Dither::Diffusion => "diffusion",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Dither::None => crate::lang::tr("Aus", "Off"),
            Dither::Ordered => crate::lang::tr("Raster", "Pattern"),
            Dither::Diffusion => crate::lang::tr("Streuung", "Diffusion"),
        }
    }

    pub fn parse(s: &str) -> Option<Dither> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "" => Some(Dither::None),
            "ordered" | "bayer" | "pattern" => Some(Dither::Ordered),
            "diffusion" | "floyd" | "floyd-steinberg" | "error" => Some(Dither::Diffusion),
            _ => None,
        }
    }
}

/// Was mit dem Hintergrund geschieht.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Background {
    #[default]
    Keep,
    /// Die häufigste Farbe am Rand gilt als Hintergrund.
    Auto,
    /// Diese Farbe ist der Hintergrund.
    Color(Rgba),
}

/// Ein Umriss um alles, was nicht durchsichtig ist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outline {
    #[default]
    None,
    /// Die dunkelste Farbe der Palette (oder ein sehr dunkler Ton davon).
    Auto,
    Color(Rgba),
}

/// Alle Einstellungen einer Umwandlung.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixelate {
    /// Zielgröße in Pixeln.
    pub width: u32,
    pub height: u32,
    /// Wie viele Farben entstehen (2 bis 256), wenn keine Palette vorgegeben ist.
    pub colors: usize,
    /// Nur diese Farben benutzen, etwa die Palette des Projekts oder einer Figur.
    pub palette: Option<Vec<Rgba>>,
    pub dither: Dither,
    pub background: Background,
    /// Wie ähnlich eine Farbe dem Hintergrund sein muss, in Prozent (0 bis 100).
    pub tolerance: u8,
    pub outline: Outline,
    /// Farben vor dem Reduzieren kräftiger und kontrastreicher machen.
    pub vivid: bool,
}

impl Pixelate {
    /// Gute Voreinstellungen für ein Bild von `w × h`: die längere Seite wird 64 Pixel.
    pub fn for_source(w: u32, h: u32) -> Self {
        let (width, height) = fit_size(w, h, 64);
        Self {
            width,
            height,
            colors: 16,
            palette: None,
            dither: Dither::None,
            background: Background::Keep,
            tolerance: 12,
            outline: Outline::None,
            vivid: false,
        }
    }

    fn check(&self) -> Result<()> {
        if self.width == 0 || self.height == 0 || self.width > MAX_SIZE || self.height > MAX_SIZE {
            return fail(crate::trf!(
                "Die Zielgröße muss zwischen 1 und {MAX_SIZE} Pixeln liegen",
                "The target size must be between 1 and {MAX_SIZE} pixels"
            ));
        }
        if self.palette.is_none() && !(2..=MAX_COLORS).contains(&self.colors) {
            return fail(crate::trf!(
                "Die Farbanzahl muss zwischen 2 und {MAX_COLORS} liegen",
                "The number of colors must be between 2 and {MAX_COLORS}"
            ));
        }
        if let Some(p) = &self.palette {
            if !p.iter().any(|c| !c.is_transparent()) {
                return fail(crate::lang::tr(
                    "Die vorgegebene Palette hat keine Farben",
                    "The given palette has no colors",
                ));
            }
        }
        if self.tolerance > 100 {
            return fail(crate::lang::tr(
                "Die Toleranz liegt zwischen 0 und 100 Prozent",
                "The tolerance lies between 0 and 100 percent",
            ));
        }
        Ok(())
    }
}

/// Eine Größe mit dem Seitenverhältnis von `w × h`, deren längere Seite `longest` ist.
pub fn fit_size(w: u32, h: u32, longest: u32) -> (u32, u32) {
    let (w, h, longest) = (w.max(1) as f64, h.max(1) as f64, longest.max(1) as f64);
    let k = longest / w.max(h);
    let side = |v: f64| ((v * k).round() as u32).clamp(1, MAX_SIZE);
    (side(w), side(h))
}

// ---------------------------------------------------------------- Hintergrund

/// Macht den Hintergrund durchsichtig: von den Rändern aus alles, was zusammenhängt
/// und der Hintergrundfarbe ähnlich ist. Gibt die Anzahl freigestellter Pixel zurück.
pub fn remove_background(
    pixels: &mut [Rgba],
    w: u32,
    h: u32,
    background: Background,
    tolerance: u8,
) -> usize {
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || pixels.len() != w * h {
        return 0;
    }
    let border: Vec<usize> = (0..w)
        .flat_map(|x| [x, (h - 1) * w + x])
        .chain((0..h).flat_map(|y| [y * w, y * w + w - 1]))
        .collect();
    let target = match background {
        Background::Keep => return 0,
        Background::Color(c) => c,
        Background::Auto => {
            // Ist der Rand schon größtenteils durchsichtig, gibt es nichts zu tun.
            let clear = border.iter().filter(|&&i| pixels[i].a < 128).count();
            if clear * 2 >= border.len() {
                return 0;
            }
            let mut counts: HashMap<[u8; 3], (u32, [u64; 3])> = HashMap::new();
            for &i in &border {
                let c = pixels[i];
                if c.a < 128 {
                    continue;
                }
                let e = counts
                    .entry([c.r >> 4, c.g >> 4, c.b >> 4])
                    .or_insert((0, [0; 3]));
                e.0 += 1;
                e.1[0] += u64::from(c.r);
                e.1[1] += u64::from(c.g);
                e.1[2] += u64::from(c.b);
            }
            let Some((_, (n, sum))) = counts.into_iter().max_by_key(|(k, (n, _))| (*n, *k)) else {
                return 0;
            };
            let n = u64::from(n);
            Rgba::rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8)
        }
    };
    // Größter Abstand im RGB-Würfel ist √(3·255²) ≈ 441.
    let limit = (f64::from(tolerance) * 4.42).powi(2) as i64;
    let near = |c: Rgba| {
        if c.a < 128 {
            return true;
        }
        let d = |a: u8, b: u8| i64::from(a) - i64::from(b);
        let (dr, dg, db) = (d(c.r, target.r), d(c.g, target.g), d(c.b, target.b));
        dr * dr + dg * dg + db * db <= limit
    };
    let mut seen = vec![false; w * h];
    let mut stack: Vec<usize> = border.into_iter().filter(|&i| near(pixels[i])).collect();
    for &i in &stack {
        seen[i] = true;
    }
    let mut n = 0;
    while let Some(i) = stack.pop() {
        if pixels[i].a > 0 {
            n += 1;
        }
        pixels[i] = Rgba::TRANSPARENT;
        let (x, y) = (i % w, i / w);
        let mut visit = |j: usize| {
            if !seen[j] && near(pixels[j]) {
                seen[j] = true;
                stack.push(j);
            }
        };
        if x > 0 {
            visit(i - 1);
        }
        if x + 1 < w {
            visit(i + 1);
        }
        if y > 0 {
            visit(i - w);
        }
        if y + 1 < h {
            visit(i + w);
        }
    }
    n
}

// ---------------------------------------------------------------- Verkleinern

/// Bringt ein Bild auf `tw × th`: jeder Zielpixel ist der Durchschnitt der Fläche,
/// die er im Original bedeckt (beim Vergrößern der nächste Pixel). Mit `solid` wird
/// jeder Pixel ganz deckend oder ganz durchsichtig, wie in Pixel-Art üblich.
pub fn resample(pixels: &[Rgba], w: u32, h: u32, tw: u32, th: u32, solid: bool) -> Vec<Rgba> {
    let (w, h, tw, th) = (w as usize, h as usize, tw as usize, th as usize);
    let mut out = Vec::with_capacity(tw * th);
    if w == 0 || h == 0 || pixels.len() != w * h {
        return vec![Rgba::TRANSPARENT; tw * th];
    }
    let span = |t: usize, src: usize, dst: usize| {
        let a = t * src / dst;
        let b = ((t + 1) * src / dst).max(a + 1).min(src);
        (a, b)
    };
    for ty in 0..th {
        let (y0, y1) = span(ty, h, th);
        for tx in 0..tw {
            let (x0, x1) = span(tx, w, tw);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for &c in &pixels[y * w + x0..y * w + x1] {
                    let ca = u64::from(c.a);
                    r += u64::from(c.r) * ca;
                    g += u64::from(c.g) * ca;
                    b += u64::from(c.b) * ca;
                    a += ca;
                    n += 1;
                }
            }
            let alpha = a / n.max(1);
            let c = if a == 0 || (solid && alpha < 128) {
                Rgba::TRANSPARENT
            } else {
                let ch = |v: u64| (v / a) as u8;
                Rgba::new(ch(r), ch(g), ch(b), if solid { 255 } else { alpha as u8 })
            };
            out.push(c);
        }
    }
    out
}

/// Kräftigere Farben: mehr Sättigung und etwas mehr Kontrast.
fn vivid(c: Rgba) -> Rgba {
    let gray = luminance(c) as f64 / 1000.0;
    let ch = |v: u8| {
        let s = gray + (f64::from(v) - gray) * 1.35;
        ((s - 128.0) * 1.1 + 128.0).round().clamp(0.0, 255.0) as u8
    };
    Rgba::new(ch(c.r), ch(c.g), ch(c.b), c.a)
}

// ---------------------------------------------------------------- Farben

/// Abstand zweier Farben, wie das Auge ihn etwa wahrnimmt ("redmean").
fn distance(a: [i32; 3], b: Rgba) -> i64 {
    let rm = (a[0] + i32::from(b.r)) / 2;
    let dr = i64::from(a[0] - i32::from(b.r));
    let dg = i64::from(a[1] - i32::from(b.g));
    let db = i64::from(a[2] - i32::from(b.b));
    let rm = i64::from(rm);
    (((512 + rm) * dr * dr) >> 8) + 4 * dg * dg + (((767 - rm) * db * db) >> 8)
}

fn nearest(c: [i32; 3], palette: &[Rgba]) -> Rgba {
    palette
        .iter()
        .copied()
        .min_by_key(|&p| distance(c, p))
        .unwrap_or(Rgba::TRANSPARENT)
}

fn rgb(c: Rgba) -> [i32; 3] {
    [i32::from(c.r), i32::from(c.g), i32::from(c.b)]
}

/// Höchstens `n` Farben, die die deckenden Pixel gut wiedergeben: Median Cut,
/// danach einige Runden k-Means zum Feinschliff. Von dunkel nach hell sortiert.
pub fn build_palette<'a>(frames: impl IntoIterator<Item = &'a [Rgba]>, n: usize) -> Vec<Rgba> {
    let mut counts: HashMap<Rgba, u64> = HashMap::new();
    for frame in frames {
        for &c in frame {
            if c.a >= 128 {
                *counts.entry(Rgba::rgb(c.r, c.g, c.b)).or_insert(0) += 1;
            }
        }
    }
    if counts.is_empty() {
        return Vec::new();
    }
    let colors: Vec<(Rgba, u64)> = counts.into_iter().collect();
    let mut palette: Vec<Rgba> = median_cut(colors.clone(), n.max(1))
        .into_iter()
        .map(|b| Rgba::rgb(b.mean.r, b.mean.g, b.mean.b))
        .collect();
    for _ in 0..4 {
        let mut sums = vec![([0u64; 3], 0u64); palette.len()];
        for &(c, k) in &colors {
            let i = (0..palette.len())
                .min_by_key(|&i| distance(rgb(c), palette[i]))
                .unwrap_or(0);
            sums[i].0[0] += u64::from(c.r) * k;
            sums[i].0[1] += u64::from(c.g) * k;
            sums[i].0[2] += u64::from(c.b) * k;
            sums[i].1 += k;
        }
        for (p, (s, k)) in palette.iter_mut().zip(sums) {
            if k > 0 {
                *p = Rgba::rgb((s[0] / k) as u8, (s[1] / k) as u8, (s[2] / k) as u8);
            }
        }
    }
    palette.sort_unstable_by_key(|c| (luminance(*c), c.to_array()));
    palette.dedup();
    palette
}

const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Ersetzt jeden deckenden Pixel durch eine Farbe aus `palette`.
pub fn map_colors(pixels: &[Rgba], w: u32, palette: &[Rgba], dither: Dither) -> Vec<Rgba> {
    let w = w.max(1) as usize;
    let palette: Vec<Rgba> = palette
        .iter()
        .filter(|c| !c.is_transparent())
        .map(|c| Rgba::rgb(c.r, c.g, c.b))
        .collect();
    let mut out = vec![Rgba::TRANSPARENT; pixels.len()];
    if palette.is_empty() {
        return out;
    }
    match dither {
        Dither::None => {
            let mut cache: HashMap<Rgba, Rgba> = HashMap::new();
            for (o, &c) in out.iter_mut().zip(pixels) {
                if c.a >= 128 {
                    *o = *cache
                        .entry(Rgba::rgb(c.r, c.g, c.b))
                        .or_insert_with(|| nearest(rgb(c), &palette));
                }
            }
        }
        Dither::Ordered => {
            // Die Stärke richtet sich nach dem Abstand der Farben: wenige Farben, mehr Raster.
            let strength = (96.0 / (palette.len() as f64).sqrt()).clamp(10.0, 48.0);
            for (i, (o, &c)) in out.iter_mut().zip(pixels).enumerate() {
                if c.a < 128 {
                    continue;
                }
                let t = BAYER[(i / w) % 4][(i % w) % 4];
                let shift = ((f64::from(t) + 0.5) / 16.0 - 0.5) * strength;
                let v = rgb(c).map(|v| (f64::from(v) + shift).round() as i32);
                *o = nearest(v, &palette);
            }
        }
        Dither::Diffusion => {
            // Der Fehler wird etwas gedämpft weitergegeben, damit Flächen ruhig bleiben.
            let mut err = vec![[0i32; 3]; pixels.len()];
            let h = pixels.len() / w;
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let c = pixels[i];
                    if c.a < 128 {
                        continue;
                    }
                    let want = [0, 1, 2].map(|k| (rgb(c)[k] + err[i][k] / 16).clamp(0, 255));
                    let got = nearest(want, &palette);
                    out[i] = got;
                    let e = [0, 1, 2].map(|k| (want[k] - rgb(got)[k]) * 3 / 4);
                    let mut spread = |dx: isize, dy: usize, f: i32| {
                        let nx = x as isize + dx;
                        if nx < 0 || nx as usize >= w || y + dy >= h {
                            return;
                        }
                        let j = (y + dy) * w + nx as usize;
                        for k in 0..3 {
                            err[j][k] += e[k] * f;
                        }
                    };
                    spread(1, 0, 7);
                    spread(-1, 1, 3);
                    spread(0, 1, 5);
                    spread(1, 1, 1);
                }
            }
        }
    }
    out
}

/// Zieht einen 1-Pixel-Umriss außen um alles Deckende. Gibt die Anzahl neuer Pixel zurück.
pub fn add_outline(pixels: &mut [Rgba], w: u32, h: u32, color: Rgba) -> usize {
    let (w, h) = (w as usize, h as usize);
    let solid: Vec<bool> = pixels.iter().map(|c| c.a >= 128).collect();
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if solid[i] {
                continue;
            }
            let touches = (x > 0 && solid[i - 1])
                || (x + 1 < w && solid[i + 1])
                || (y > 0 && solid[i - w])
                || (y + 1 < h && solid[i + w]);
            if touches {
                pixels[i] = color;
                n += 1;
            }
        }
    }
    n
}

/// Die Farbe für einen automatischen Umriss: die dunkelste der Palette, und ist
/// sie nicht dunkel genug, ein sehr dunkler Ton von ihr.
pub fn outline_color(palette: &[Rgba], fixed: bool) -> Rgba {
    let darkest = palette
        .iter()
        .copied()
        .filter(|c| !c.is_transparent())
        .min_by_key(|c| (luminance(*c), c.to_array()))
        .unwrap_or(Rgba::rgb(20, 16, 28));
    if fixed || luminance(darkest) <= 60_000 {
        return Rgba::rgb(darkest.r, darkest.g, darkest.b);
    }
    let ch = |v: u8| (u32::from(v) * 3 / 10) as u8;
    Rgba::rgb(ch(darkest.r), ch(darkest.g), ch(darkest.b))
}

/// Das Ergebnis einer Umwandlung.
#[derive(Clone, Debug, PartialEq)]
pub struct Converted {
    /// Die Frames in Zielgröße, jeweils `width × height` Pixel.
    pub frames: Vec<Vec<Rgba>>,
    /// Die benutzten Farben, von dunkel nach hell.
    pub palette: Vec<Rgba>,
    /// Wie viele Pixel des Originals als Hintergrund entfielen (über alle Frames).
    pub background_removed: usize,
}

/// Wandelt Frames von `w × h` Pixeln in Pixel-Art um. Alle Frames teilen sich eine
/// Palette, damit eine Animation einheitlich bleibt.
pub fn convert(frames: &[&[Rgba]], w: u32, h: u32, options: &Pixelate) -> Result<Converted> {
    options.check()?;
    if frames
        .iter()
        .any(|f| f.len() != (w as usize) * (h as usize))
    {
        return fail("the frames do not match the picture size");
    }
    let (tw, th) = (options.width, options.height);
    let mut removed = 0;
    let small: Vec<Vec<Rgba>> = frames
        .iter()
        .map(|f| {
            let mut px = f.to_vec();
            removed += remove_background(&mut px, w, h, options.background, options.tolerance);
            let mut out = resample(&px, w, h, tw, th, true);
            if options.vivid {
                out.iter_mut()
                    .filter(|c| !c.is_transparent())
                    .for_each(|c| *c = vivid(*c));
            }
            out
        })
        .collect();
    let fixed = options.palette.is_some();
    let mut palette = match &options.palette {
        Some(p) => {
            let mut p: Vec<Rgba> = p
                .iter()
                .filter(|c| !c.is_transparent())
                .map(|c| Rgba::rgb(c.r, c.g, c.b))
                .collect();
            p.dedup();
            p
        }
        None => {
            // Mit Umriss eine Farbe weniger, damit die Anzahl insgesamt stimmt.
            let n = options.colors - usize::from(options.outline != Outline::None);
            build_palette(small.iter().map(Vec::as_slice), n.max(1))
        }
    };
    let mut out: Vec<Vec<Rgba>> = small
        .iter()
        .map(|f| map_colors(f, tw, &palette, options.dither))
        .collect();
    let line = match options.outline {
        Outline::None => None,
        Outline::Auto => Some(outline_color(&palette, fixed)),
        Outline::Color(c) if fixed => Some(nearest(rgb(c), &palette)),
        Outline::Color(c) => Some(Rgba::rgb(c.r, c.g, c.b)),
    };
    if let Some(c) = line {
        for f in &mut out {
            add_outline(f, tw, th, c);
        }
    }
    // Nur die Farben behalten, die wirklich vorkommen (eine vorgegebene Palette bleibt ganz).
    if !fixed {
        let used: std::collections::HashSet<Rgba> = out.iter().flatten().copied().collect();
        palette.retain(|c| used.contains(c));
        if let Some(c) = line {
            if used.contains(&c) && !palette.contains(&c) {
                palette.push(c);
            }
        }
        palette.sort_unstable_by_key(|c| (luminance(*c), c.to_array()));
    }
    Ok(Converted {
        frames: out,
        palette,
        background_removed: removed,
    })
}

/// Eine Vorlage zum Nachzeichnen: das Bild auf eine Leinwand von `cw × ch`
/// eingepasst (Seitenverhältnis bleibt, mittig), ohne Farbreduktion.
pub fn reference(pixels: &[Rgba], w: u32, h: u32, cw: u32, ch: u32) -> Vec<Rgba> {
    let k = (f64::from(cw) / f64::from(w.max(1))).min(f64::from(ch) / f64::from(h.max(1)));
    let fw = ((f64::from(w) * k).round() as u32).clamp(1, cw.max(1));
    let fh = ((f64::from(h) * k).round() as u32).clamp(1, ch.max(1));
    let fitted = resample(pixels, w, h, fw, fh, false);
    let (ox, oy) = ((cw - fw) / 2, (ch - fh) / 2);
    let mut out = vec![Rgba::TRANSPARENT; (cw * ch) as usize];
    for y in 0..fh {
        let src = &fitted[(y * fw) as usize..][..fw as usize];
        let at = ((y + oy) * cw + ox) as usize;
        out[at..at + fw as usize].copy_from_slice(src);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein Verlauf von links (rot) nach rechts (blau) auf weißem Rand.
    fn photo(w: u32, h: u32) -> Vec<Rgba> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let edge = x < 4 || y < 4 || x >= w - 4 || y >= h - 4;
                if edge {
                    Rgba::rgb(250, 250, 248)
                } else {
                    let t = (x * 255 / w) as u8;
                    Rgba::rgb(255 - t, 40, t)
                }
            })
            .collect()
    }

    #[test]
    fn a_photo_becomes_small_with_few_colors() {
        let px = photo(120, 80);
        let mut opts = Pixelate::for_source(120, 80);
        assert_eq!((opts.width, opts.height), (64, 43));
        opts.width = 30;
        opts.height = 20;
        opts.colors = 6;
        let out = convert(&[&px], 120, 80, &opts).unwrap();
        assert_eq!(out.frames[0].len(), 600);
        let used: std::collections::HashSet<_> = out.frames[0].iter().collect();
        assert!(used.len() <= 6, "{}", used.len());
        assert!(out.palette.len() <= 6);

        // Hintergrund weg, Umriss dran: der Rand ist durchsichtig, die Figur umrandet.
        opts.background = Background::Auto;
        opts.outline = Outline::Auto;
        let out = convert(&[&px], 120, 80, &opts).unwrap();
        let f = &out.frames[0];
        assert!(out.background_removed > 0);
        assert!(f[0].is_transparent());
        let line = outline_color(&out.palette, false);
        assert_eq!(f[30], line, "Umriss direkt über der Fläche");
        assert!(out.palette.contains(&line));
        assert!(out.palette.len() <= 6);
    }

    #[test]
    fn dithering_and_fixed_palettes() {
        let px = photo(64, 64);
        let palette = vec![Rgba::rgb(255, 40, 0), Rgba::rgb(0, 40, 255)];
        for dither in Dither::ALL {
            let opts = Pixelate {
                width: 32,
                height: 32,
                palette: Some(palette.clone()),
                dither,
                ..Pixelate::for_source(64, 64)
            };
            let out = convert(&[&px], 64, 64, &opts).unwrap();
            assert!(out.frames[0].iter().all(|c| palette.contains(c)));
            assert_eq!(out.palette, palette);
            // Mit Dithering mischen sich in der Mitte beide Farben.
            let row: std::collections::HashSet<_> =
                out.frames[0][16 * 32 + 12..16 * 32 + 20].iter().collect();
            if dither == Dither::None {
                assert!(row.len() <= 2);
            } else {
                assert_eq!(row.len(), 2, "{dither:?}");
            }
        }
        assert_eq!(Dither::parse("floyd-steinberg"), Some(Dither::Diffusion));
        assert!(convert(
            &[&px],
            64,
            64,
            &Pixelate {
                colors: 1,
                ..Pixelate::for_source(64, 64)
            }
        )
        .is_err());
    }

    #[test]
    fn references_keep_the_aspect_ratio() {
        let px = vec![Rgba::new(10, 20, 30, 200); 40 * 20];
        let r = reference(&px, 40, 20, 16, 16);
        assert_eq!(r.len(), 256);
        assert!(r[0].is_transparent(), "oben leer, das Bild ist breiter");
        assert_eq!(r[8 * 16], Rgba::new(10, 20, 30, 200));
        assert_eq!(fit_size(1000, 250, 64), (64, 16));
    }
}
