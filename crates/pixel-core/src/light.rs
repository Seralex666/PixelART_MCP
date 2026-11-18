//! Licht und Schatten, die einer Lichtquelle folgen.
//!
//! Eine Lichtquelle steht an einem Punkt der Leinwand (auch außerhalb) und kann
//! über die Frames einen Pfad entlangwandern, etwa eine Fackel, die vorbeizieht,
//! oder die Sonne, die über den Himmel läuft. Daraus entstehen auf eigenen Ebenen:
//!
//! - **Licht:** die Kanten, die zur Lichtquelle zeigen, eine Stufe heller,
//! - **Schatten:** die Kanten, die von ihr wegzeigen, eine Stufe dunkler,
//! - **Schlagschatten** (auf Wunsch): ein flacher Schatten am Boden, der vom Licht
//!   wegfällt.
//!
//! Wie die automatische Kontur wird alles nach jeder Änderung in allen Frames neu
//! berechnet ([`Sprite::refresh_outline`] erledigt beides). Die Pixel bleiben echte
//! Pixel-Art: heller und dunkler geht es entlang der Farbtreppen der Palette.

use crate::brush::tone_color;
use crate::quant::luminance;
use crate::{Error, Result, Rgba, Sprite};
use serde::{Deserialize, Serialize};

/// Breiter als so viele Pixel wird die Licht- und Schattenkante nicht.
pub const MAX_WIDTH: u32 = 4;
/// So viele Punkte darf ein Lichtpfad haben.
pub const MAX_PATH: usize = 32;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Effect(message.into()))
}

fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}

/// Ein Schatten am Boden, der vom Licht wegfällt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CastShadow {
    /// Name der Ebene, ganz unten unter der Figur.
    pub layer: String,
    /// Länge im Verhältnis zur Höhe der Figur, in Prozent (10 bis 300).
    #[serde(default = "half")]
    pub length: u32,
    /// Deckkraft der Schattenebene, 0 bis 255.
    #[serde(default = "shade_opacity")]
    pub opacity: u8,
    /// Farbe; ohne die dunkelste Farbe der Palette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgba>,
}

fn half() -> u32 {
    100
}
fn shade_opacity() -> u8 {
    110
}

impl CastShadow {
    pub fn new(layer: impl Into<String>) -> Self {
        Self {
            layer: layer.into(),
            length: half(),
            opacity: shade_opacity(),
            color: None,
        }
    }
}

/// Wie das automatische Licht aussieht.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoLight {
    /// Ebene mit den beleuchteten Kanten.
    pub light_layer: String,
    /// Ebene mit den Kanten im Schatten.
    pub shadow_layer: String,
    /// Wo die Lichtquelle steht, in Pixeln der Leinwand. Mit mehreren Punkten wandert
    /// sie über die Frames diesen Pfad entlang, vom ersten Frame bis zum letzten.
    pub path: Vec<(f32, f32)>,
    /// Wie breit die Kanten sind, 1 bis [`MAX_WIDTH`] Pixel.
    #[serde(default = "one")]
    pub width: u32,
    /// Wie viele Stufen heller und dunkler, 1 oder 2. Bei 2 bekommen die äußersten
    /// Kantenpixel die doppelte Stufe.
    #[serde(default = "one")]
    pub strength: u32,
    /// Farbe des Lichts, etwa Orange für eine Fackel. Ohne Farbe wird entlang der
    /// Palette heller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgba>,
    /// Auch die Schattenseite abdunkeln.
    #[serde(default = "yes")]
    pub shadows: bool,
    /// Schlagschatten am Boden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast: Option<CastShadow>,
}

impl AutoLight {
    /// Ein Licht an `at`, Kanten 1 Pixel breit, mit Eigenschatten, ohne Schlagschatten.
    pub fn new(at: (f32, f32)) -> Self {
        Self {
            light_layer: crate::lang::tr("Licht", "Light").into(),
            shadow_layer: crate::lang::tr("Schatten", "Shadow").into(),
            path: vec![at],
            width: 1,
            strength: 1,
            color: None,
            shadows: true,
            cast: None,
        }
    }

    fn check(&self) -> Result<()> {
        let names = [
            Some(&self.light_layer),
            Some(&self.shadow_layer),
            self.cast.as_ref().map(|c| &c.layer),
        ];
        let names: Vec<&String> = names.into_iter().flatten().collect();
        if names.iter().any(|n| n.trim().is_empty()) {
            return fail(crate::lang::tr(
                "Die Ebenen für Licht und Schatten brauchen Namen",
                "the light and shadow layers need names",
            ));
        }
        for (i, a) in names.iter().enumerate() {
            if names[..i].iter().any(|b| b.eq_ignore_ascii_case(a)) {
                return fail(crate::lang::tr(
                    "Licht, Schatten und Schlagschatten brauchen verschiedene Ebenen",
                    "light, shadow and cast shadow need different layers",
                ));
            }
        }
        if self.path.is_empty() || self.path.len() > MAX_PATH {
            return fail(crate::trf!(
                "Der Lichtpfad braucht 1 bis {MAX_PATH} Punkte",
                "the light path needs 1 to {MAX_PATH} points"
            ));
        }
        if self
            .path
            .iter()
            .any(|(x, y)| !x.is_finite() || !y.is_finite() || x.abs() > 1e5 || y.abs() > 1e5)
        {
            return fail(crate::lang::tr(
                "Der Lichtpfad hat ungültige Punkte",
                "the light path has invalid points",
            ));
        }
        if !(1..=MAX_WIDTH).contains(&self.width) {
            return fail(crate::trf!(
                "Die Breite muss 1 bis {MAX_WIDTH} Pixel sein",
                "the width must be 1 to {MAX_WIDTH} pixels"
            ));
        }
        if !(1..=2).contains(&self.strength) {
            return fail(crate::lang::tr(
                "Die Stärke ist 1 oder 2",
                "the strength is 1 or 2",
            ));
        }
        if self.color.is_some_and(|c| c.is_transparent()) {
            return fail(crate::lang::tr(
                "Das Licht braucht eine sichtbare Farbe",
                "the light needs a visible color",
            ));
        }
        if let Some(c) = &self.cast {
            if !(10..=300).contains(&c.length) {
                return fail(crate::lang::tr(
                    "Die Länge des Schlagschattens ist 10 bis 300 Prozent",
                    "the cast shadow length is 10 to 300 percent",
                ));
            }
        }
        Ok(())
    }

    /// Wo die Lichtquelle in Frame `frame` von `frames` steht.
    pub fn position(&self, frame: usize, frames: usize) -> (f32, f32) {
        let n = self.path.len();
        if n == 1 || frames <= 1 {
            return self.path[0];
        }
        let t = frame.min(frames - 1) as f32 / (frames - 1) as f32 * (n - 1) as f32;
        let i = (t.floor() as usize).min(n - 2);
        let k = t - i as f32;
        let (a, b) = (self.path[i], self.path[i + 1]);
        (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k)
    }

    fn layers(&self) -> Vec<&str> {
        let mut v = vec![self.light_layer.as_str(), self.shadow_layer.as_str()];
        if let Some(c) = &self.cast {
            v.push(c.layer.as_str());
        }
        v
    }
}

/// Eine Lichtquelle in Richtung `dir` (wie `"top-left"`), weit genug außerhalb der
/// Leinwand von `w × h`, damit das Licht fast parallel einfällt.
pub fn position_for(dir: &str, w: u32, h: u32) -> Option<(f32, f32)> {
    let (dx, dy) = crate::contour::light_vector(dir)?;
    let (w, h) = (w as f32, h as f32);
    let r = w.max(h) * 1.5;
    Some((w / 2.0 + f32::from(dx) * r, h / 2.0 + f32::from(dy) * r))
}

impl Sprite {
    /// Das eingerichtete Licht, falls es eins gibt.
    pub fn auto_light(&self) -> Option<&AutoLight> {
        self.light.as_deref()
    }

    /// Setzt die Einstellungen des Lichts, ohne neu zu zeichnen (zum Zusammenführen
    /// zweier Stände); danach [`Sprite::refresh_outline`] aufrufen.
    pub fn restore_light_spec(&mut self, spec: Option<AutoLight>) {
        self.light = spec.map(Box::new);
    }

    /// Ob eine Ebene vom Licht verwaltet wird (Licht, Schatten oder Schlagschatten).
    pub fn is_light_layer(&self, layer: usize) -> bool {
        let Some(spec) = &self.light else {
            return false;
        };
        let names = self.layer_names();
        names
            .get(layer)
            .is_some_and(|n| spec.layers().contains(&n.as_str()))
    }

    /// Index der Schlagschatten-Ebene.
    pub fn cast_shadow_layer(&self) -> Option<usize> {
        self.light
            .as_ref()
            .and_then(|l| l.cast.as_ref())
            .and_then(|c| self.layer_index(&c.layer))
    }

    /// Richtet das Licht ein (oder ändert es) und zeichnet es in allen Frames.
    /// Licht und Schatten kommen ganz nach oben, unter eine automatische Kontur;
    /// der Schlagschatten ganz nach unten, über Vorlagen. Vorhandene Ebenen dieser
    /// Namen werden verschoben und überschrieben. Eine Ebene, die vorher zum Licht
    /// gehörte und nun nicht mehr gebraucht wird, verschwindet.
    pub fn set_auto_light(&mut self, spec: AutoLight) -> Result<()> {
        spec.check()?;
        if let Some(old) = self.light.take() {
            for name in old.layers() {
                if !spec.layers().contains(&name) {
                    if let Some(i) = self.layer_index(name) {
                        if self.layer_count() > 1 {
                            self.remove_layer(i)?;
                        }
                    }
                }
            }
        }
        for name in [&spec.light_layer, &spec.shadow_layer] {
            if self.layer_index(name).is_none() {
                self.add_layer(name.clone());
            }
        }
        // Reihenfolge: … Inhalt, Schatten, Licht, Kontur.
        let outline = self.auto_outline().map(|o| o.layer.clone());
        for name in [
            Some(&spec.shadow_layer),
            Some(&spec.light_layer),
            outline.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(i) = self.layer_index(name) {
                self.move_layer(i, self.layer_count() - 1)?;
            }
        }
        if let Some(c) = &spec.cast {
            let i = match self.layer_index(&c.layer) {
                Some(i) => i,
                None => self.add_layer(c.layer.clone()),
            };
            let bottom = (0..self.layer_count())
                .take_while(|&l| self.is_reference(l))
                .count()
                .min(self.layer_count() - 1);
            self.move_layer(i, if i < bottom { bottom - 1 } else { bottom })?;
            let l = self.layer_index(&c.layer).ok_or(Error::NoSuchLayer(0))?;
            for f in 0..self.frames().len() {
                self.layer_mut(f, l)?.opacity = c.opacity;
            }
        }
        self.light = Some(Box::new(spec));
        self.refresh_light()?;
        Ok(())
    }

    /// Beendet das Licht. Mit `keep` bleiben die Ebenen als gewöhnliche Ebenen
    /// stehen, sonst verschwinden sie.
    pub fn stop_light(&mut self, keep: bool) -> Result<()> {
        let Some(spec) = self.light.take() else {
            return Ok(());
        };
        if !keep {
            for name in spec.layers() {
                if let Some(i) = self.layer_index(name) {
                    if self.layer_count() > 1 {
                        self.remove_layer(i)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Zeichnet Licht und Schatten in allen Frames neu. `true`, wenn sich dabei
    /// etwas geändert hat.
    pub fn refresh_light(&mut self) -> Result<bool> {
        let Some(spec) = self.light.clone() else {
            return Ok(false);
        };
        let (Some(lit), Some(dark)) = (
            self.layer_index(&spec.light_layer),
            self.layer_index(&spec.shadow_layer),
        ) else {
            // Eine der Ebenen wurde gelöscht oder umbenannt: Dann ist das Licht vorbei.
            self.light = None;
            return Ok(true);
        };
        let cast = spec.cast.as_ref().and_then(|c| self.layer_index(&c.layer));
        let skip: Vec<usize> = [Some(lit), Some(dark), cast, self.outline_layer()]
            .into_iter()
            .flatten()
            .collect();
        let frames = self.frames().len();
        let (w, h) = (self.width() as usize, self.height() as usize);
        let contents: Vec<Vec<Rgba>> = (0..frames)
            .map(|f| content(self, f, &skip))
            .collect::<Result<_>>()?;
        // Der Boden liegt für alle Frames gleich: unter dem tiefsten Pixel. So bleibt
        // der Schatten liegen, wenn die Figur springt.
        let ground = contents
            .iter()
            .filter_map(|c| {
                (0..h)
                    .rev()
                    .find(|&y| c[y * w..][..w].iter().any(|p| p.a > 0))
            })
            .max();
        let mut changed = false;
        for (f, colors) in contents.iter().enumerate() {
            let at = spec.position(f, frames);
            let (light, shadow) = edges(self, &spec, colors, at);
            changed |= self.put(f, lit, light)?;
            changed |= self.put(f, dark, shadow)?;
            if let (Some(l), Some(c), Some(g)) = (cast, &spec.cast, ground) {
                let color = c.color.unwrap_or_else(|| darkest(self));
                let pixels = cast_shadow(colors, w, h, g, at, c.length, self.conform(color));
                changed |= self.put(f, l, pixels)?;
            }
        }
        Ok(changed)
    }

    fn put(&mut self, frame: usize, layer: usize, pixels: Vec<Rgba>) -> Result<bool> {
        let pixels: Vec<Rgba> = pixels.into_iter().map(|c| self.conform(c)).collect();
        let l = self.layer_mut(frame, layer)?;
        if l.pixels() == pixels.as_slice() {
            return Ok(false);
        }
        l.replace_pixels(pixels)?;
        Ok(true)
    }
}

/// Die dunkelste Farbe der Palette (oder fast Schwarz).
fn darkest(s: &Sprite) -> Rgba {
    s.palette
        .colors
        .iter()
        .copied()
        .filter(|c| !c.is_transparent())
        .min_by_key(|c| luminance(*c))
        .unwrap_or(Rgba::rgb(16, 14, 24))
}

/// Was beleuchtet wird: alle sichtbaren Ebenen außer `skip` und Vorlagen, übereinander.
fn content(s: &Sprite, frame: usize, skip: &[usize]) -> Result<Vec<Rgba>> {
    let mut out = vec![Rgba::TRANSPARENT; (s.width() * s.height()) as usize];
    for (i, l) in s.frame(frame)?.layers.iter().enumerate() {
        if skip.contains(&i) || !l.visible || l.opacity == 0 || l.reference {
            continue;
        }
        for (o, &c) in out.iter_mut().zip(l.pixels()) {
            if c.a > 0 {
                *o = c.over(*o);
            }
        }
    }
    Ok(out)
}

/// Abstand jedes deckenden Pixels zum Rand (1 = Randpixel), höchstens `max + 1`.
fn edge_distance(solid: &[bool], w: usize, h: usize, max: u32) -> Vec<u32> {
    let mut d: Vec<u32> = solid.iter().map(|&s| if s { max + 1 } else { 0 }).collect();
    let at = |x: i64, y: i64| -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h)
            .then(|| y as usize * w + x as usize)
    };
    for step in 1..=max {
        let mut next = d.clone();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let i = y as usize * w + x as usize;
                if d[i] <= step {
                    continue;
                }
                let touches = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .any(|(dx, dy)| at(x + dx, y + dy).is_none_or(|j| d[j] == step - 1));
                if touches {
                    next[i] = step;
                }
            }
        }
        d = next;
    }
    d
}

/// Licht- und Schattenkanten eines Frames für eine Lichtquelle an `at`.
fn edges(s: &Sprite, spec: &AutoLight, colors: &[Rgba], at: (f32, f32)) -> (Vec<Rgba>, Vec<Rgba>) {
    let (w, h) = (s.width() as usize, s.height() as usize);
    let mut solid: Vec<bool> = colors.iter().map(|c| c.a >= 128).collect();
    // Eine dunkle Kontur bleibt, wie sie ist: Licht und Schatten fallen auf die
    // Pixel gleich dahinter. Als Kontur gilt ein sehr dunkler Randpixel.
    let rim = edge_distance(&solid, w, h, 1);
    for (i, s) in solid.iter_mut().enumerate() {
        if *s && rim[i] == 1 && luminance(colors[i]) < 60_000 {
            *s = false;
        }
    }
    let d = edge_distance(&solid, w, h, spec.width);
    let mut light = vec![Rgba::TRANSPARENT; w * h];
    let mut shadow = vec![Rgba::TRANSPARENT; w * h];
    let get = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            0.0
        } else {
            d[y as usize * w + x as usize] as f32
        }
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !solid[i] || d[i] > spec.width {
                continue;
            }
            let (xi, yi) = (x as i64, y as i64);
            // Die Richtung nach außen: dorthin, wo der Abstand zum Rand abnimmt,
            // geglättet über die acht Nachbarn.
            let mut nx = 0.0;
            let mut ny = 0.0;
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let fall = get(xi, yi) - get(xi + dx, yi + dy);
                nx += dx as f32 * fall;
                ny += dy as f32 * fall;
            }
            let len = (nx * nx + ny * ny).sqrt();
            if len < 1e-3 {
                continue;
            }
            let (lx, ly) = (at.0 - (x as f32 + 0.5), at.1 - (y as f32 + 0.5));
            let ll = (lx * lx + ly * ly).sqrt();
            let facing = if ll < 0.5 {
                1.0
            } else {
                (nx * lx + ny * ly) / (len * ll)
            };
            let base = colors[i];
            let double = spec.strength == 2 && d[i] == 1 && facing.abs() > 0.7;
            if facing > 0.3 {
                light[i] = brighten(s, spec, base, double);
            } else if facing < -0.3 && spec.shadows {
                let mut c = tone_color(s, base, false);
                if double {
                    c = tone_color(s, c, false);
                }
                if c != base {
                    shadow[i] = Rgba { a: base.a, ..c };
                }
            }
        }
    }
    (light, shadow)
}

/// Eine Stufe heller, oder zur Lichtfarbe hin.
fn brighten(s: &Sprite, spec: &AutoLight, base: Rgba, double: bool) -> Rgba {
    let c = match spec.color {
        None => {
            let c = tone_color(s, base, true);
            if double {
                tone_color(s, c, true)
            } else {
                c
            }
        }
        Some(k) => {
            let share = if double { 0.6 } else { 0.4 };
            let mix =
                |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * share).round() as u8;
            let mixed = Rgba::rgb(mix(base.r, k.r), mix(base.g, k.g), mix(base.b, k.b));
            if s.palette.locked {
                s.palette.nearest(mixed)
            } else {
                mixed
            }
        }
    };
    if c == base {
        Rgba::TRANSPARENT
    } else {
        Rgba { a: base.a, ..c }
    }
}

/// Der Schlagschatten eines Frames: die Figur flach auf den Boden in Zeile `ground`
/// gelegt, vom Licht weg verschoben. Liegt das Licht unter dem Boden, gibt es keinen.
fn cast_shadow(
    colors: &[Rgba],
    w: usize,
    h: usize,
    ground: usize,
    at: (f32, f32),
    length: u32,
    color: Rgba,
) -> Vec<Rgba> {
    let mut out = vec![Rgba::TRANSPARENT; w * h];
    let solid: Vec<(usize, usize)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| colors[y * w + x].a >= 128)
        .collect();
    if solid.is_empty() || at.1 >= ground as f32 + 0.5 {
        return out;
    }
    let cx = solid.iter().map(|&(x, _)| x as f32).sum::<f32>() / solid.len() as f32 + 0.5;
    // Wie weit der Schatten je Pixel Höhe zur Seite fällt (flaches Licht: lang).
    let skew = ((cx - at.0) / (ground as f32 + 1.0 - at.1)).clamp(-4.0, 4.0);
    let k = length as f32 / 100.0;
    // Der Boden ist flach: Eine Figur von 24 Pixeln wirft einen Schatten von etwa 3.
    const FLAT: f32 = 0.12;
    for &(x, y) in &solid {
        let height = (ground - y) as f32 + 0.5;
        let tx = (x as f32 + 0.5 + skew * height * k).floor();
        let ty = (ground as f32 + 0.5 - height * k * FLAT).floor();
        if tx >= 0.0 && ty >= 0.0 && (tx as usize) < w && (ty as usize) < h {
            out[ty as usize * w + tx as usize] = color;
        }
    }
    // Waagerechte Lücken schließen, die beim Verschieben entstehen.
    for y in 0..h {
        let row = &mut out[y * w..][..w];
        let filled: Vec<usize> = (0..w).filter(|&x| !row[x].is_transparent()).collect();
        for pair in filled.windows(2) {
            if pair[1] - pair[0] <= 3 {
                for c in &mut row[pair[0]..pair[1]] {
                    *c = color;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorDepth, Palette};

    fn ball() -> Sprite {
        let mut s = Sprite::new(
            16,
            16,
            ColorDepth::Rgba32,
            Palette::new("p", vec![Rgba::rgb(40, 60, 160)]),
        )
        .unwrap();
        for y in 3..13 {
            for x in 3..13 {
                s.set_pixel(0, 0, x, y, Rgba::rgb(40, 60, 160)).unwrap();
            }
        }
        s.add_frame();
        for y in 3..13 {
            for x in 3..13 {
                s.set_pixel(1, 0, x, y, Rgba::rgb(40, 60, 160)).unwrap();
            }
        }
        s
    }

    #[test]
    fn the_light_moves_along_its_path() {
        let mut s = ball();
        let mut spec = AutoLight::new((-20.0, 8.0));
        spec.path.push((36.0, 8.0));
        s.set_auto_light(spec).unwrap();
        let lit = s.layer_index("Licht").or(s.layer_index("Light")).unwrap();
        let dark = s
            .layer_index("Schatten")
            .or(s.layer_index("Shadow"))
            .unwrap();
        // Frame 0: Licht von links, Frame 1: von rechts.
        let px = |s: &Sprite, f: usize, l: usize, x: i64| s.layer(f, l).unwrap().get(x, 8).unwrap();
        assert!(!px(&s, 0, lit, 3).is_transparent());
        assert!(px(&s, 0, lit, 12).is_transparent());
        assert!(!px(&s, 0, dark, 12).is_transparent());
        assert!(!px(&s, 1, lit, 12).is_transparent());
        assert!(!px(&s, 1, dark, 3).is_transparent());
        // Das Licht ist heller, der Schatten dunkler als die Figur.
        let base = luminance(Rgba::rgb(40, 60, 160));
        assert!(luminance(px(&s, 0, lit, 3)) > base);
        assert!(luminance(px(&s, 0, dark, 12)) < base);
        // Ändert sich die Figur, folgt das Licht.
        s.set_pixel(0, 0, 2, 8, Rgba::rgb(40, 60, 160)).unwrap();
        assert!(s.refresh_light().unwrap());
        assert!(!px(&s, 0, lit, 2).is_transparent());
        assert_eq!(s.auto_light().unwrap().position(1, 3), (8.0, 8.0));
    }

    #[test]
    fn cast_shadows_fall_away_from_the_light_and_stop_cleanly() {
        let mut s = ball();
        let mut spec = AutoLight::new((-10.0, -10.0));
        spec.cast = Some(CastShadow::new("Boden"));
        s.set_auto_light(spec.clone()).unwrap();
        let l = s.cast_shadow_layer().unwrap();
        assert_eq!(l, 0, "der Schlagschatten liegt ganz unten");
        let row: Vec<bool> = (0..16)
            .map(|x| !s.layer(0, l).unwrap().get(x, 12).unwrap().is_transparent())
            .collect();
        assert!(
            row[14] && !row[2],
            "Licht oben links, Schatten nach rechts: {row:?}"
        );
        assert!(s.is_light_layer(l));
        // Ohne Schlagschatten verschwindet seine Ebene wieder.
        spec.cast = None;
        let layers = s.layer_count();
        s.set_auto_light(spec).unwrap();
        assert_eq!(s.layer_count(), layers - 1);
        s.stop_light(false).unwrap();
        assert_eq!(s.layer_count(), 1);
        assert!(s.auto_light().is_none());
        assert!(AutoLight {
            width: 9,
            ..AutoLight::new((0.0, 0.0))
        }
        .check()
        .is_err());
    }
}
