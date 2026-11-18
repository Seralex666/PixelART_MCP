//! Automatische Kontur für Figuren, Animationen und Bilder.
//!
//! Die Kontur liegt auf einer eigenen Ebene und entsteht aus allen anderen
//! sichtbaren Ebenen. Ist sie einmal eingerichtet, wird sie nach jeder Änderung
//! neu berechnet ([`Sprite::refresh_outline`]), in allen Frames. Wer sie von Hand
//! nachbessern will, schreibt sie fest ([`Sprite::stop_outline`]); dann bleibt
//! sie eine gewöhnliche Ebene.

use crate::quant::luminance;
use crate::{Error, Palette, Result, Rgba, Sprite};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dicker als so viele Pixel wird eine Kontur nicht.
pub const MAX_THICKNESS: u32 = 4;

/// Wie die automatische Kontur aussieht.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoOutline {
    /// Name der Ebene, auf der die Kontur liegt.
    pub layer: String,
    /// Feste Farbe. Ohne Farbe ist die Kontur selektiv: jeder Konturpixel nimmt
    /// eine dunklere Stufe der Farbe, an die er grenzt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgba>,
    /// Innen: Die Kontur färbt die Randpixel der Figur, statt außen anzusetzen.
    /// Die Figur wird so nicht größer.
    #[serde(default)]
    pub inside: bool,
    /// Auch über Eck: Die Kontur schließt diagonale Lücken und wirkt an Schrägen dicker.
    #[serde(default)]
    pub corners: bool,
    /// Dicke in Pixeln, 1 bis [`MAX_THICKNESS`].
    #[serde(default = "one")]
    pub thickness: u32,
    /// Innenlinien, wo eine obere Ebene an eine untere grenzt, etwa ein Arm vor dem Körper.
    #[serde(default)]
    pub inner: bool,
    /// Richtung zur Lichtquelle, etwa `(-1, -1)` für oben links. Eine selektive
    /// Kontur ist auf dieser Seite heller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<(i8, i8)>,
}

fn one() -> u32 {
    1
}

/// Die Lichtrichtung zu einem Namen aus [`crate::project::LIGHT_DIRS`], etwa
/// `"top-left"` zu `(-1, -1)`.
pub fn light_vector(name: &str) -> Option<(i8, i8)> {
    const VECTORS: [(i8, i8); 8] = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    let i = crate::project::LIGHT_DIRS.iter().position(|&d| d == name)?;
    Some(VECTORS[i])
}

impl AutoOutline {
    /// Eine selektive Kontur außen, ein Pixel dick, auf der Ebene `layer`.
    pub fn new(layer: impl Into<String>) -> Self {
        Self {
            layer: layer.into(),
            color: None,
            inside: false,
            corners: false,
            thickness: 1,
            inner: false,
            light: None,
        }
    }

    fn check(&self) -> Result<()> {
        let fail = |m: &str| Err(Error::Effect(m.into()));
        if self.layer.trim().is_empty() {
            return fail(crate::lang::tr(
                "Die Konturebene braucht einen Namen",
                "the outline layer needs a name",
            ));
        }
        if !(1..=MAX_THICKNESS).contains(&self.thickness) {
            return Err(Error::Effect(crate::trf!(
                "Die Dicke der Kontur muss 1 bis {MAX_THICKNESS} sein",
                "outline thickness must be 1 to {MAX_THICKNESS}"
            )));
        }
        if self.color.is_some_and(|c| c.is_transparent()) {
            return fail(crate::lang::tr(
                "Eine Kontur braucht eine sichtbare Farbe",
                "an outline needs a visible color",
            ));
        }
        Ok(())
    }
}

impl Sprite {
    /// Die eingerichtete automatische Kontur, falls es eine gibt.
    pub fn auto_outline(&self) -> Option<&AutoOutline> {
        self.outline.as_ref()
    }

    /// Setzt die Einstellungen der Kontur, ohne neu zu zeichnen. Für das
    /// Zusammenführen zweier Stände; danach [`Sprite::refresh_outline`] aufrufen.
    pub fn restore_outline_spec(&mut self, spec: Option<AutoOutline>) {
        self.outline = spec;
    }

    /// Index der Ebene mit der automatischen Kontur.
    pub fn outline_layer(&self) -> Option<usize> {
        let spec = self.outline.as_ref()?;
        self.layer_index(&spec.layer)
    }

    /// Richtet die automatische Kontur ein (oder ändert sie) und zeichnet sie in
    /// allen Frames. Fehlt die Ebene, kommt sie ganz nach oben; eine vorhandene
    /// Ebene dieses Namens wird nach oben geschoben und überschrieben.
    /// Gibt die Zahl der Konturpixel im ersten Frame zurück.
    pub fn set_auto_outline(&mut self, spec: AutoOutline) -> Result<usize> {
        spec.check()?;
        let top = self.layer_count() - 1;
        match self.layer_index(&spec.layer) {
            Some(i) => self.move_layer(i, top)?,
            None => {
                self.add_layer(spec.layer.clone());
            }
        }
        self.outline = Some(spec);
        self.order_effect_layers()?;
        self.refresh_contour()?;
        let l = self.outline_layer().ok_or(Error::NoSuchLayer(0))?;
        Ok(self
            .layer(0, l)?
            .pixels()
            .iter()
            .filter(|c| !c.is_transparent())
            .count())
    }

    /// Beendet die Automatik. Mit `keep` bleibt die Kontur als gewöhnliche Ebene
    /// stehen und lässt sich von Hand ändern, sonst verschwindet die Ebene.
    pub fn stop_outline(&mut self, keep: bool) -> Result<()> {
        let layer = self.outline_layer();
        self.outline = None;
        if let (Some(l), false) = (layer, keep) {
            if self.layer_count() > 1 {
                self.remove_layer(l)?;
            } else {
                for f in 0..self.frames().len() {
                    self.layer_mut(f, l)?.clear();
                }
            }
        }
        Ok(())
    }

    /// Zeichnet die automatischen Ebenen in allen Frames neu: erst Licht und
    /// Schatten ([`crate::light`]), dann die Kontur, zuletzt die Effekte
    /// ([`crate::fx`]). `true`, wenn sich dabei etwas geändert hat. Ohne Licht,
    /// Kontur und Effekte passiert nichts.
    pub fn refresh_outline(&mut self) -> Result<bool> {
        let lit = self.refresh_light()?;
        let outlined = self.refresh_contour()?;
        Ok(self.refresh_effects()? || outlined || lit)
    }

    fn refresh_contour(&mut self) -> Result<bool> {
        let Some(spec) = self.outline.clone() else {
            return Ok(false);
        };
        let l = match self.layer_index(&spec.layer) {
            Some(l) => l,
            // Die Ebene wurde umbenannt oder gelöscht: Dann ist auch die Automatik vorbei.
            None => {
                self.outline = None;
                return Ok(true);
            }
        };
        let mut changed = false;
        let mut cache = HashMap::new();
        for f in 0..self.frames().len() {
            let raw = render(self, &spec, f, l, &mut cache)?;
            let pixels: Vec<Rgba> = raw.into_iter().map(|c| self.conform(c)).collect();
            let layer = self.layer_mut(f, l)?;
            if layer.pixels() != pixels.as_slice() {
                layer.replace_pixels(pixels)?;
                changed = true;
            }
        }
        Ok(changed)
    }
}

/// Was unter der Kontur liegt: je Pixel die oberste Ebene und ihre Farbe.
struct Content {
    owner: Vec<Option<usize>>,
    color: Vec<Rgba>,
}

fn content(s: &Sprite, frame: usize, skip: usize) -> Result<Content> {
    let n = (s.width() * s.height()) as usize;
    let mut out = Content {
        owner: vec![None; n],
        color: vec![Rgba::TRANSPARENT; n],
    };
    for (i, layer) in s.frame(frame)?.layers.iter().enumerate() {
        // Der Schlagschatten liegt am Boden und bekommt keine Kontur.
        if i == skip
            || !layer.visible
            || layer.opacity == 0
            || layer.reference
            || Some(i) == s.cast_shadow_layer()
            || s.is_effect_layer(i)
        {
            continue;
        }
        for (p, &c) in layer.pixels().iter().enumerate() {
            if !c.is_transparent() {
                out.owner[p] = Some(i);
                out.color[p] = c;
            }
        }
    }
    Ok(out)
}

const ORTHOGONAL: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
const ALL: [(i64, i64); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Die Konturpixel eines Frames, noch ohne Anpassung an Farbtiefe und Palette.
fn render(
    s: &Sprite,
    spec: &AutoOutline,
    frame: usize,
    skip: usize,
    cache: &mut HashMap<(Rgba, bool), Rgba>,
) -> Result<Vec<Rgba>> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let c = content(s, frame, skip)?;
    let n = c.color.len();
    let at = |x: i64, y: i64| (x >= 0 && y >= 0 && x < w && y < h).then(|| (y * w + x) as usize);
    let around: &[(i64, i64)] = if spec.corners { &ALL } else { &ORTHOGONAL };

    // Je Konturpixel: die Farbe, an die er grenzt, und wohin er vom Inneren aus zeigt.
    let mut base: Vec<Option<(Rgba, (i64, i64))>> = vec![None; n];

    if spec.inside {
        // Ringe nach innen: gefüllte Pixel, die an Leeres (oder an den vorigen Ring) grenzen.
        let mut done = vec![false; n];
        for _ in 0..spec.thickness {
            let mut ring = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    if c.owner[i].is_none() || done[i] {
                        continue;
                    }
                    let mut normal = (0, 0);
                    let mut edge = false;
                    for &(dx, dy) in around {
                        if let Some(j) = at(x + dx, y + dy) {
                            if c.owner[j].is_none() || done[j] {
                                normal = (normal.0 + dx, normal.1 + dy);
                                edge = true;
                            }
                        }
                    }
                    if edge {
                        base[i] = Some((c.color[i], normal));
                        ring.push(i);
                    }
                }
            }
            if ring.is_empty() {
                break;
            }
            for i in ring {
                done[i] = true;
            }
        }
    } else {
        // Ringe nach außen: leere Pixel, die an die Figur (oder den vorigen Ring) grenzen.
        let mut filled: Vec<Option<Rgba>> = c
            .color
            .iter()
            .map(|&k| (!k.is_transparent()).then_some(k))
            .collect();
        for _ in 0..spec.thickness {
            let mut ring = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    if filled[i].is_some() {
                        continue;
                    }
                    let mut normal = (0, 0);
                    let mut seen: Vec<Rgba> = Vec::new();
                    for &(dx, dy) in around {
                        if let Some(k) = at(x + dx, y + dy).and_then(|j| filled[j]) {
                            normal = (normal.0 - dx, normal.1 - dy);
                            seen.push(k);
                        }
                    }
                    if let Some(k) = most_common(&seen) {
                        ring.push((i, k, normal));
                    }
                }
            }
            if ring.is_empty() {
                break;
            }
            for (i, k, normal) in ring {
                filled[i] = Some(k);
                base[i] = Some((k, normal));
            }
        }
    }

    // Innenlinien: Wo eine obere Ebene an eine untere grenzt, bekommt die untere
    // eine Linie in der Farbe der oberen. So hebt sich ein Arm vom Körper ab.
    if spec.inner {
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let Some(mine) = c.owner[i] else { continue };
                if base[i].is_some() {
                    continue;
                }
                for &(dx, dy) in &ORTHOGONAL {
                    let Some(j) = at(x + dx, y + dy) else {
                        continue;
                    };
                    if c.owner[j].is_some_and(|theirs| theirs > mine) {
                        base[i] = Some((c.color[j], (-dx, -dy)));
                        break;
                    }
                }
            }
        }
    }

    let light = spec.light.map(|(lx, ly)| (i64::from(lx), i64::from(ly)));
    Ok(base
        .into_iter()
        .map(|b| match (b, spec.color) {
            (None, _) => Rgba::TRANSPARENT,
            (Some(_), Some(fixed)) => fixed,
            (Some((k, normal)), None) => {
                let lit = light.is_some_and(|(lx, ly)| normal.0 * lx + normal.1 * ly > 0);
                *cache
                    .entry((k, lit))
                    .or_insert_with(|| darker(k, lit, &s.palette))
            }
        })
        .collect())
}

/// Die häufigste Farbe, bei Gleichstand die dunkelste.
fn most_common(colors: &[Rgba]) -> Option<Rgba> {
    let mut best: Option<(usize, Rgba)> = None;
    for &k in colors {
        let n = colors.iter().filter(|&&o| o == k).count();
        let better = match best {
            None => true,
            Some((m, b)) => n > m || (n == m && luminance(k) < luminance(b)),
        };
        if better {
            best = Some((n, k));
        }
    }
    best.map(|(_, k)| k)
}

/// Eine dunklere Stufe von `k` für eine selektive Kontur, auf der Lichtseite
/// weniger dunkel. Passt eine dunklere Palettenfarbe, wird sie genommen; bei
/// gesperrter Palette immer eine Palettenfarbe.
pub fn darker(k: Rgba, lit: bool, palette: &Palette) -> Rgba {
    let keep = if lit { 0.72 } else { 0.45 };
    // Schatten wandern leicht ins Kühle, das wirkt lebendiger als reines Abdunkeln.
    let tint = [40.0, 28.0, 72.0];
    let mix = |v: u8, t: f64| (f64::from(v) * keep * 0.85 + t * 0.15).round() as u8;
    let target = Rgba::new(mix(k.r, tint[0]), mix(k.g, tint[1]), mix(k.b, tint[2]), 255);
    let limit = luminance(k) * 9 / 10;
    let darker_ones = palette
        .colors
        .iter()
        .copied()
        .filter(|c| !c.is_transparent() && luminance(*c) < limit);
    let best = darker_ones.min_by_key(|c| c.distance_sq(target));
    match best {
        Some(c) if palette.locked || c.distance_sq(target) <= 3 * 36 * 36 => c,
        None if palette.locked => palette
            .colors
            .iter()
            .copied()
            .filter(|c| !c.is_transparent())
            .min_by_key(|c| luminance(*c))
            .unwrap_or(target),
        _ => target,
    }
}

impl crate::Project {
    /// Zeichnet alle automatischen Konturen im Projekt neu.
    pub fn refresh_outlines(&mut self) -> Result<bool> {
        let mut changed = false;
        for c in &mut self.characters {
            changed |= c.base.refresh_outline()?;
        }
        for a in &mut self.animations {
            changed |= a.sprite.refresh_outline()?;
        }
        for i in &mut self.images {
            changed |= i.sprite.refresh_outline()?;
        }
        Ok(changed)
    }

    /// Richtet die Kontur für eine Figur ein, auf Wunsch auch für alle ihre
    /// Animationen. Gibt zurück, für wie viele Animationen sie mit eingerichtet wurde.
    pub fn set_character_outline(
        &mut self,
        id: uuid::Uuid,
        spec: Option<AutoOutline>,
        keep: bool,
        with_animations: bool,
    ) -> Result<usize> {
        let apply = |s: &mut Sprite| -> Result<()> {
            match &spec {
                Some(spec) => s.set_auto_outline(spec.clone()).map(|_| ()),
                None => s.stop_outline(keep),
            }
        };
        apply(&mut self.character_mut(id)?.base)?;
        let mut n = 0;
        if with_animations {
            for a in self.animations.iter_mut().filter(|a| a.character_id == id) {
                apply(&mut a.sprite)?;
                n += 1;
            }
        }
        Ok(n)
    }
}
