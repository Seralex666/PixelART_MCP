//! Effekte, die über die Frames laufen: Leuchten, Blitz, Explosion, Spiegelung,
//! Funken, Rauch, Magie und Farbzyklen.
//!
//! Jeder Effekt liegt auf einer eigenen Ebene und wird wie Kontur und Licht nach
//! jeder Änderung neu gezeichnet, in allen Frames. Wer den Effekt von Hand
//! nachbessern will, schreibt ihn fest ([`Sprite::remove_effect`] mit `keep`).
//! Zufall kommt aus einem festen Startwert: gleiche Eingaben ergeben auf jedem
//! Rechner dasselbe Bild. Wiederkehrende Effekte (Leuchten, Partikel, Farbzyklen,
//! Wellen) laufen nahtlos im Kreis, wenn die Animation von vorn beginnt.

use crate::expr::{bayer_threshold, hash, unit, value_noise};
use crate::lang::tr;
use crate::{ColorDepth, Error, Result, Rgba, Sprite};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// Mehr Effekte hat ein Bild nicht.
pub const MAX_EFFECTS: usize = 8;
/// So viele Frames legt [`Sprite::ensure_frames`] höchstens an.
pub const MAX_FRAMES: usize = 64;
/// Höchstens so viele Farben in einer Farbliste eines Effekts.
pub const MAX_COLORS: usize = 16;
/// Höchstens so viele Partikel.
pub const MAX_PARTICLES: u32 = 300;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Effect(message.into()))
}

/// Wie sich Partikel bewegen und aussehen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticleStyle {
    /// Funken: schnell nach oben und außen, fallen zurück, glühen aus.
    Sparks,
    /// Rauch: steigt langsam, wird größer und löst sich auf.
    Smoke,
    /// Magie: funkelnde Sterne, die aufleuchten und vergehen.
    Magic,
}

impl ParticleStyle {
    pub const ALL: [ParticleStyle; 3] = [Self::Sparks, Self::Smoke, Self::Magic];

    pub fn id(self) -> &'static str {
        match self {
            Self::Sparks => "sparks",
            Self::Smoke => "smoke",
            Self::Magic => "magic",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sparks => tr("Funken", "Sparks"),
            Self::Smoke => tr("Rauch", "Smoke"),
            Self::Magic => tr("Magie", "Magic"),
        }
    }

    /// Die Farben, wenn keine angegeben sind: vom Anfang bis zum Ende des Lebens.
    pub fn colors(self) -> Vec<Rgba> {
        let hex: &[u32] = match self {
            Self::Sparks => &[0xfff8d0, 0xffd24a, 0xff8a2a, 0xc8401e],
            Self::Smoke => &[0xb4b4bc, 0x96969e, 0x787882, 0x5a5a64],
            Self::Magic => &[0xffffff, 0xc8f0ff, 0xa078ff, 0xff78dc],
        };
        hex.iter().map(|&c| rgb(c)).collect()
    }

    /// Welcher Anteil der Animation ein Partikel lebt.
    fn life(self) -> f32 {
        match self {
            Self::Sparks => 0.4,
            Self::Smoke => 0.85,
            Self::Magic => 0.6,
        }
    }
}

fn rgb(c: u32) -> Rgba {
    Rgba::rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Fertige Farbtreppen für Farbzyklen. Wer damit malt und dann den Zyklus
/// einrichtet, bekommt fließendes Wasser, flackerndes Feuer oder glühende Lava.
pub const CYCLE_PRESETS: [&str; 3] = ["water", "fire", "lava"];

/// Die Farben einer fertigen Farbtreppe (`water`, `fire`, `lava`, auch deutsch).
pub fn cycle_preset(name: &str) -> Option<Vec<Rgba>> {
    let hex: &[u32] = match name.trim().to_ascii_lowercase().as_str() {
        "water" | "wasser" => &[0x1b3b6f, 0x2a5ca8, 0x3f8fd8, 0x7cc4f2],
        "fire" | "feuer" => &[0x7a1d12, 0xc8401e, 0xf08a24, 0xffd25a],
        "lava" => &[0x4a0f0f, 0x8f1e12, 0xd94a14, 0xff9a2e],
        _ => return None,
    };
    Some(hex.iter().map(|&c| rgb(c)).collect())
}

/// Die Farben einer Explosion ohne eigene Farben: von weißglühend bis Rauch.
pub fn explosion_colors() -> Vec<Rgba> {
    [
        0xffffff, 0xfff2a0, 0xffb43c, 0xf0642d, 0xa02d28, 0x6e6464, 0x463c46,
    ]
    .iter()
    .map(|&c| rgb(c))
    .collect()
}

fn no_colors(v: &[Rgba]) -> bool {
    v.is_empty()
}
fn yes() -> bool {
    true
}

/// Was ein Effekt tut, mit seinen Einstellungen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EffectKind {
    /// Ein Schein um die Figur (hinter ihr) oder, mit `colors`, nur um Pixel dieser
    /// Farben, etwa leuchtende Augen oder eine Klinge (dann über der Figur).
    Glow {
        color: Rgba,
        /// Wie weit der Schein reicht, 1 bis 8 Pixel.
        radius: u32,
        /// Um so viele Pixel wird der Schein im Takt kleiner und wieder größer.
        #[serde(default)]
        pulse: u32,
        #[serde(default, skip_serializing_if = "no_colors")]
        colors: Vec<Rgba>,
        /// Gerastert statt halb durchsichtig (bei 8 und 16 Bit immer).
        #[serde(default)]
        dither: bool,
    },
    /// Ein gezackter Blitz von `from` nach `to`, in jedem Frame neu geformt.
    Lightning {
        from: (f32, f32),
        to: (f32, f32),
        color: Rgba,
        /// Schein um den Blitz; ohne Schein bleibt er eine dünne Linie.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        glow: Option<Rgba>,
        /// Seitenäste, 0 bis 8.
        #[serde(default)]
        branches: u32,
        /// In manchen Frames ist der Blitz aus.
        #[serde(default = "yes")]
        flicker: bool,
    },
    /// Eine Explosion um `at`: Blitz, Feuerball, Trümmer, Rauch. Läuft einmal ab,
    /// von Frame `start` an über `length` Frames (0 = bis zum Ende).
    Explosion {
        at: (f32, f32),
        radius: u32,
        #[serde(default)]
        start: u32,
        #[serde(default)]
        length: u32,
        #[serde(default, skip_serializing_if = "no_colors")]
        colors: Vec<Rgba>,
    },
    /// Spiegelbild im Wasser unter der Figur, mit Wellen über die Frames.
    Reflection {
        /// Erste Zeile des Wassers; ohne gleich unter dem tiefsten Pixel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        /// Farbe des Wassers, in die das Spiegelbild getönt wird.
        color: Rgba,
        /// Wie stark getönt, 0 bis 100 Prozent.
        mix: u8,
        /// Deckkraft des Spiegelbilds, 0 bis 255.
        opacity: u8,
        /// Wie weit die Wellen die Zeilen verschieben, 0 bis 8 Pixel.
        #[serde(default)]
        wave: u32,
    },
    /// Funken, Rauch oder Magie aus einer Quelle bei `at`.
    Particles {
        style: ParticleStyle,
        at: (f32, f32),
        /// Halbe Breite der Quelle in Pixeln.
        #[serde(default)]
        spread: u32,
        count: u32,
        /// Wie weit die Partikel fliegen, 0.1 bis 5 (1 = normal).
        speed: f32,
        #[serde(default, skip_serializing_if = "no_colors")]
        colors: Vec<Rgba>,
    },
    /// Palettenanimation: Pixel dieser Farben wandern Frame für Frame zur nächsten
    /// Farbe der Liste weiter (Wasser, Feuer, Lava, Leuchtschrift).
    ColorCycle {
        colors: Vec<Rgba>,
        /// Alle so viele Frames ein Schritt, 1 bis 16.
        speed: u32,
        #[serde(default)]
        reverse: bool,
    },
}

/// Wo die Ebene eines Effekts liegt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Hinter der Figur (über Vorlagen und Schlagschatten).
    Below,
    /// Direkt über dem Inhalt, unter Licht und Kontur.
    Middle,
    /// Ganz oben.
    Top,
}

impl EffectKind {
    /// Kurzer Name für Claude: `glow`, `lightning`, `explosion`, `reflection`,
    /// `sparks`, `smoke`, `magic`, `color_cycle`.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Glow { .. } => "glow",
            Self::Lightning { .. } => "lightning",
            Self::Explosion { .. } => "explosion",
            Self::Reflection { .. } => "reflection",
            Self::Particles { style, .. } => style.id(),
            Self::ColorCycle { .. } => "color_cycle",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Glow { .. } => tr("Leuchten", "Glow"),
            Self::Lightning { .. } => tr("Blitz", "Lightning"),
            Self::Explosion { .. } => tr("Explosion", "Explosion"),
            Self::Reflection { .. } => tr("Spiegelung", "Reflection"),
            Self::Particles { style, .. } => style.label(),
            Self::ColorCycle { .. } => tr("Farbzyklus", "Color cycle"),
        }
    }

    /// Alle Arten, wie sie Claude kennt.
    pub const IDS: [&'static str; 8] = [
        "glow",
        "lightning",
        "explosion",
        "reflection",
        "sparks",
        "smoke",
        "magic",
        "color_cycle",
    ];

    /// Ein Effekt mit Einstellungen, die zu einer Leinwand von `w × h` passen.
    pub fn default_for(id: &str, w: u32, h: u32) -> Option<Self> {
        let (fw, fh) = (w as f32, h as f32);
        let center = (fw / 2.0, fh / 2.0);
        let low = (fw / 2.0, fh * 0.8);
        Some(match id.trim().to_ascii_lowercase().as_str() {
            "glow" | "leuchten" => Self::Glow {
                color: rgb(0xffe68a),
                radius: 3,
                pulse: 1,
                colors: Vec::new(),
                dither: false,
            },
            "lightning" | "blitz" => Self::Lightning {
                from: (fw * 0.3, 0.0),
                to: (fw * 0.6, fh - 1.0),
                color: rgb(0xfffbe6),
                glow: Some(rgb(0x8fd3ff)),
                branches: 2,
                flicker: true,
            },
            "explosion" => Self::Explosion {
                at: center,
                radius: (w.min(h) / 2).clamp(2, 256),
                start: 0,
                length: 0,
                colors: Vec::new(),
            },
            "reflection" | "spiegelung" => Self::Reflection {
                line: None,
                color: rgb(0x2a5ca8),
                mix: 35,
                opacity: 170,
                wave: 1,
            },
            "sparks" | "funken" => Self::Particles {
                style: ParticleStyle::Sparks,
                at: low,
                spread: (w / 8).max(1),
                count: 24,
                speed: 1.0,
                colors: Vec::new(),
            },
            "smoke" | "rauch" => Self::Particles {
                style: ParticleStyle::Smoke,
                at: low,
                spread: (w / 10).max(1),
                count: 16,
                speed: 1.0,
                colors: Vec::new(),
            },
            "magic" | "magie" => Self::Particles {
                style: ParticleStyle::Magic,
                at: center,
                spread: (w / 3).max(1),
                count: 10,
                speed: 1.0,
                colors: Vec::new(),
            },
            "color_cycle" | "colorcycle" | "cycle" | "farbzyklus" => Self::ColorCycle {
                colors: cycle_preset("water").unwrap_or_default(),
                speed: 1,
                reverse: false,
            },
            _ => return None,
        })
    }

    fn place(&self) -> Place {
        match self {
            Self::Glow { colors, .. } if colors.is_empty() => Place::Below,
            Self::Reflection { .. } => Place::Below,
            Self::ColorCycle { .. } => Place::Middle,
            _ => Place::Top,
        }
    }

    /// Ob der Effekt erst über mehrere Frames lebendig wird.
    pub fn animates(&self) -> bool {
        match self {
            Self::Glow { pulse, .. } => *pulse > 0,
            Self::Reflection { wave, .. } => *wave > 0,
            _ => true,
        }
    }

    fn check(&self) -> Result<()> {
        let finite = |p: (f32, f32)| p.0.is_finite() && p.1.is_finite();
        let colors = |v: &[Rgba]| -> Result<()> {
            if v.len() > MAX_COLORS {
                return fail(crate::trf!(
                    "Höchstens {MAX_COLORS} Farben",
                    "at most {MAX_COLORS} colors"
                ));
            }
            Ok(())
        };
        match self {
            Self::Glow {
                radius,
                pulse,
                colors: c,
                ..
            } => {
                if !(1..=8).contains(radius) {
                    return fail(tr(
                        "Der Schein reicht 1 bis 8 Pixel weit",
                        "the glow radius must be 1 to 8 pixels",
                    ));
                }
                if pulse >= radius {
                    return fail(tr(
                        "Das Pulsieren muss kleiner als die Reichweite sein",
                        "the pulse must be smaller than the radius",
                    ));
                }
                colors(c)?;
            }
            Self::Lightning {
                from, to, branches, ..
            } => {
                if !finite(*from) || !finite(*to) {
                    return fail(tr("Ungültiger Punkt", "invalid point"));
                }
                if *branches > 8 {
                    return fail(tr("Höchstens 8 Äste", "at most 8 branches"));
                }
            }
            Self::Explosion {
                at,
                radius,
                colors: c,
                ..
            } => {
                if !finite(*at) {
                    return fail(tr("Ungültiger Punkt", "invalid point"));
                }
                if !(2..=256).contains(radius) {
                    return fail(tr(
                        "Die Explosion braucht einen Radius von 2 bis 256",
                        "the explosion radius must be 2 to 256",
                    ));
                }
                if !c.is_empty() && c.len() < 2 {
                    return fail(tr(
                        "Eine Explosion braucht mindestens 2 Farben",
                        "an explosion needs at least 2 colors",
                    ));
                }
                colors(c)?;
            }
            Self::Reflection { mix, wave, .. } => {
                if *mix > 100 {
                    return fail(tr(
                        "Die Tönung geht von 0 bis 100 Prozent",
                        "mix must be 0 to 100 percent",
                    ));
                }
                if *wave > 8 {
                    return fail(tr(
                        "Wellen verschieben höchstens 8 Pixel",
                        "wave must be 0 to 8 pixels",
                    ));
                }
            }
            Self::Particles {
                at,
                spread,
                count,
                speed,
                colors: c,
                ..
            } => {
                if !finite(*at) {
                    return fail(tr("Ungültiger Punkt", "invalid point"));
                }
                if !(1..=MAX_PARTICLES).contains(count) {
                    return fail(crate::trf!(
                        "1 bis {MAX_PARTICLES} Partikel",
                        "count must be 1 to {MAX_PARTICLES}"
                    ));
                }
                if *spread > crate::MAX_SIZE {
                    return fail(tr("Die Quelle ist zu breit", "spread is too wide"));
                }
                if !(0.1..=5.0).contains(speed) {
                    return fail(tr("Das Tempo geht von 0.1 bis 5", "speed must be 0.1 to 5"));
                }
                colors(c)?;
            }
            Self::ColorCycle {
                colors: c, speed, ..
            } => {
                if c.len() < 2 {
                    return fail(tr(
                        "Ein Farbzyklus braucht mindestens 2 Farben",
                        "a color cycle needs at least 2 colors",
                    ));
                }
                colors(c)?;
                if !(1..=16).contains(speed) {
                    return fail(tr(
                        "Ein Schritt alle 1 bis 16 Frames",
                        "speed must be 1 to 16 frames per step",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Ein eingerichteter Effekt auf seiner Ebene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoEffect {
    /// Name der Ebene; zugleich der Name, unter dem der Effekt geändert wird.
    pub layer: String,
    #[serde(flatten)]
    pub kind: EffectKind,
    /// Startwert für den Zufall: andere Zahl, andere Funken und Blitze.
    #[serde(default)]
    pub seed: u64,
}

impl AutoEffect {
    /// Der Effekt auf einer Ebene, die so heißt wie er.
    pub fn new(kind: EffectKind) -> Self {
        Self {
            layer: kind.label().into(),
            kind,
            seed: 0,
        }
    }

    pub fn check(&self) -> Result<()> {
        if self.layer.trim().is_empty() {
            return fail(tr(
                "Die Ebene des Effekts braucht einen Namen",
                "the effect layer needs a name",
            ));
        }
        self.kind.check()
    }
}

impl Sprite {
    /// Die eingerichteten Effekte, von unten nach oben in der Reihenfolge ihrer Einrichtung.
    pub fn effects(&self) -> &[AutoEffect] {
        &self.effects
    }

    /// Der Effekt auf der Ebene `layer`.
    pub fn effect(&self, layer: &str) -> Option<&AutoEffect> {
        self.effects
            .iter()
            .find(|e| e.layer.eq_ignore_ascii_case(layer))
    }

    /// Setzt die Effekte, ohne neu zu zeichnen (zum Zusammenführen zweier Stände);
    /// danach [`Sprite::refresh_outline`] aufrufen.
    pub fn restore_effects(&mut self, effects: Vec<AutoEffect>) {
        self.effects = effects;
    }

    /// Ob eine Ebene von einem Effekt gezeichnet wird.
    pub fn is_effect_layer(&self, layer: usize) -> bool {
        let names = self.layer_names();
        names
            .get(layer)
            .is_some_and(|n| self.effects.iter().any(|e| &e.layer == n))
    }

    /// Ob eine Ebene automatisch gezeichnet wird: Kontur, Licht, Schatten oder Effekt.
    pub fn is_auto_layer(&self, layer: usize) -> bool {
        self.outline_layer() == Some(layer)
            || self.is_light_layer(layer)
            || self.is_effect_layer(layer)
    }

    /// Ein freier Ebenenname, der mit `base` beginnt.
    pub fn free_layer_name(&self, base: &str) -> String {
        if self.layer_index(base).is_none() {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|n| self.layer_index(n).is_none())
            .unwrap_or_else(|| base.to_string())
    }

    /// Richtet einen Effekt ein oder ändert den Effekt auf derselben Ebene, und
    /// zeichnet ihn in allen Frames.
    pub fn set_effect(&mut self, spec: AutoEffect) -> Result<()> {
        spec.check()?;
        match self
            .effects
            .iter()
            .position(|e| e.layer.eq_ignore_ascii_case(&spec.layer))
        {
            Some(i) => {
                let old = self.effects[i].layer.clone();
                if old != spec.layer {
                    if let Some(l) = self.layer_index(&old) {
                        self.rename_layer(l, spec.layer.clone())?;
                    }
                }
                self.effects[i] = spec;
            }
            None => {
                if self.effects.len() >= MAX_EFFECTS {
                    return fail(crate::trf!(
                        "Höchstens {MAX_EFFECTS} Effekte je Bild",
                        "at most {MAX_EFFECTS} effects per image"
                    ));
                }
                if self.layer_index(&spec.layer).is_some() {
                    return fail(crate::trf!(
                        "Es gibt schon eine Ebene „{}“; nimm einen anderen Namen",
                        "there is already a layer '{}'; pick another name",
                        spec.layer
                    ));
                }
                self.add_layer(spec.layer.clone());
                self.effects.push(spec);
            }
        }
        self.order_effect_layers()?;
        self.refresh_effects()?;
        Ok(())
    }

    /// Entfernt den Effekt auf der Ebene `layer`. Mit `keep` bleibt die Ebene als
    /// gewöhnliche Ebene stehen, sonst verschwindet sie.
    pub fn remove_effect(&mut self, layer: &str, keep: bool) -> Result<()> {
        let Some(i) = self
            .effects
            .iter()
            .position(|e| e.layer.eq_ignore_ascii_case(layer))
        else {
            return fail(crate::trf!(
                "Kein Effekt auf der Ebene „{layer}“",
                "no effect on layer '{layer}'"
            ));
        };
        let spec = self.effects.remove(i);
        if !keep {
            if let Some(l) = self.layer_index(&spec.layer) {
                if self.layer_count() > 1 {
                    self.remove_layer(l)?;
                }
            }
        }
        Ok(())
    }

    /// Wie hoch die Leinwand für ein ganzes Spiegelbild sein muss: Wasserlinie
    /// (`line` oder gleich unter dem tiefsten Pixel der Figur) plus die Höhe der
    /// Figur. `None` ohne Figur.
    pub fn reflection_height(&self, line: Option<u32>) -> Option<u32> {
        let w = self.width() as usize;
        let cast = self.cast_shadow_layer();
        let mut rows: Option<(usize, usize)> = None;
        for f in self.frames() {
            for (i, l) in f.layers.iter().enumerate() {
                if !l.visible
                    || l.opacity == 0
                    || l.reference
                    || Some(i) == cast
                    || self.is_effect_layer(i)
                {
                    continue;
                }
                for (p, c) in l.pixels().iter().enumerate() {
                    if c.a > 0 {
                        let y = p / w;
                        rows = Some(rows.map_or((y, y), |(a, b)| (a.min(y), b.max(y))));
                    }
                }
            }
        }
        let (top, bottom) = rows?;
        let water = line.map_or(bottom + 1, |l| l as usize);
        Some((water + water.saturating_sub(top)) as u32)
    }

    /// Sorgt für mindestens `n` Frames: Fehlende entstehen als Kopien der
    /// vorhandenen, reihum. Gibt zurück, wie viele dazukamen.
    pub fn ensure_frames(&mut self, n: usize) -> Result<usize> {
        if n > MAX_FRAMES {
            return fail(crate::trf!(
                "Höchstens {MAX_FRAMES} Frames",
                "at most {MAX_FRAMES} frames"
            ));
        }
        let have = self.frames().len();
        for i in have..n {
            self.insert_frame_copy(i % have, i)?;
        }
        Ok(n.saturating_sub(have))
    }

    /// Bringt die Effekt-Ebenen an ihren Platz: Schein und Spiegelung hinter die
    /// Figur, Farbzyklen direkt über den Inhalt, alles andere ganz nach oben.
    pub(crate) fn order_effect_layers(&mut self) -> Result<()> {
        if self.effects.is_empty() {
            return Ok(());
        }
        let cast = self.cast_shadow_layer();
        let base = (0..self.layer_count())
            .take_while(|&l| self.is_reference(l) || Some(l) == cast)
            .count();
        let specs = self.effects.clone();
        let mut k = 0;
        for e in specs.iter().filter(|e| e.kind.place() == Place::Below) {
            if let Some(i) = self.layer_index(&e.layer) {
                let to = (base + k).min(self.layer_count() - 1);
                if i != to {
                    self.move_layer(i, to)?;
                }
                k += 1;
            }
        }
        for e in specs.iter().filter(|e| e.kind.place() == Place::Top) {
            if let Some(i) = self.layer_index(&e.layer) {
                self.move_layer(i, self.layer_count() - 1)?;
            }
        }
        for e in specs.iter().filter(|e| e.kind.place() == Place::Middle) {
            let Some(i) = self.layer_index(&e.layer) else {
                continue;
            };
            // Unter die unterste Ebene von Licht, Kontur und oberen Effekten.
            let above = (0..self.layer_count()).filter(|&l| l != i).find(|&l| {
                self.is_light_layer(l) && Some(l) != cast
                    || self.outline_layer() == Some(l)
                    || self.layer_names().get(l).is_some_and(|n| {
                        specs
                            .iter()
                            .any(|t| &t.layer == n && t.kind.place() == Place::Top)
                    })
            });
            let to = match above {
                Some(a) if a > i => a - 1,
                Some(a) => a,
                None => self.layer_count() - 1,
            };
            if i != to {
                self.move_layer(i, to)?;
            }
        }
        Ok(())
    }

    /// Zeichnet alle Effekte in allen Frames neu. `true`, wenn sich dabei etwas
    /// geändert hat. Ein Effekt, dessen Ebene fehlt, endet.
    pub fn refresh_effects(&mut self) -> Result<bool> {
        if self.effects.is_empty() {
            return Ok(false);
        }
        let mut changed = false;
        let kept: Vec<AutoEffect> = self
            .effects
            .iter()
            .filter(|e| self.layer_index(&e.layer).is_some())
            .cloned()
            .collect();
        if kept.len() != self.effects.len() {
            self.effects = kept.clone();
            changed = true;
        }
        let frames = self.frames().len();
        let (w, h) = (self.width() as usize, self.height() as usize);
        let fx_layers: Vec<usize> = kept
            .iter()
            .filter_map(|e| self.layer_index(&e.layer))
            .collect();
        let mut skip_fx = fx_layers.clone();
        skip_fx.extend(self.cast_shadow_layer());
        let mut skip_all = skip_fx.clone();
        skip_all.extend((0..self.layer_count()).filter(|&l| self.is_auto_layer(l)));
        // Was hinter den Effekten liegt, je Frame; erst ausrechnen, wenn es gebraucht wird.
        let mut figure: Option<Vec<Vec<Rgba>>> = None;
        let mut base: Option<Vec<Vec<Rgba>>> = None;
        let soft = self.depth == ColorDepth::Rgba32 && !self.palette.locked;
        for e in &kept {
            let Some(l) = self.layer_index(&e.layer) else {
                continue;
            };
            let src = match e.kind {
                EffectKind::Glow { .. } | EffectKind::Reflection { .. } => {
                    if figure.is_none() {
                        figure = Some(
                            (0..frames)
                                .map(|f| crate::light::content(self, f, &skip_fx))
                                .collect::<Result<_>>()?,
                        );
                    }
                    figure.as_deref()
                }
                EffectKind::ColorCycle { .. } => {
                    if base.is_none() {
                        base = Some(
                            (0..frames)
                                .map(|f| crate::light::content(self, f, &skip_all))
                                .collect::<Result<_>>()?,
                        );
                    }
                    base.as_deref()
                }
                _ => None,
            };
            let line = match (&e.kind, src) {
                (EffectKind::Reflection { line: None, .. }, Some(src)) => src
                    .iter()
                    .filter_map(|c| {
                        (0..h)
                            .rev()
                            .find(|&y| c[y * w..][..w].iter().any(|p| p.a > 0))
                    })
                    .max()
                    .map(|y| y + 1),
                (EffectKind::Reflection { line: Some(y), .. }, _) => Some(*y as usize),
                _ => None,
            };
            for f in 0..frames {
                let ctx = Ctx {
                    w,
                    h,
                    f,
                    frames,
                    seed: e.seed,
                    soft,
                };
                let pixels = match &e.kind {
                    EffectKind::Glow {
                        color,
                        radius,
                        pulse,
                        colors,
                        dither,
                    } => glow(
                        &ctx,
                        src.map(|s| &s[f][..]).unwrap_or(&[]),
                        *color,
                        *radius,
                        *pulse,
                        colors,
                        *dither,
                    ),
                    EffectKind::Lightning {
                        from,
                        to,
                        color,
                        glow,
                        branches,
                        flicker,
                    } => lightning(&ctx, *from, *to, *color, *glow, *branches, *flicker),
                    EffectKind::Explosion {
                        at,
                        radius,
                        start,
                        length,
                        colors,
                    } => {
                        let colors = if colors.is_empty() {
                            explosion_colors()
                        } else {
                            colors.clone()
                        };
                        explosion(&ctx, *at, *radius, *start, *length, &colors)
                    }
                    EffectKind::Reflection {
                        color,
                        mix,
                        opacity,
                        wave,
                        ..
                    } => reflection(
                        &ctx,
                        src.map(|s| &s[f][..]).unwrap_or(&[]),
                        line,
                        *color,
                        *mix,
                        *opacity,
                        *wave,
                    ),
                    EffectKind::Particles {
                        style,
                        at,
                        spread,
                        count,
                        speed,
                        colors,
                    } => {
                        let colors = if colors.is_empty() {
                            style.colors()
                        } else {
                            colors.clone()
                        };
                        particles(&ctx, *style, *at, *spread, *count, *speed, &colors)
                    }
                    EffectKind::ColorCycle {
                        colors,
                        speed,
                        reverse,
                    } => {
                        let colors: Vec<Rgba> = colors.iter().map(|&c| self.conform(c)).collect();
                        color_cycle(
                            &ctx,
                            src.map(|s| &s[f][..]).unwrap_or(&[]),
                            &colors,
                            *speed,
                            *reverse,
                        )
                    }
                };
                changed |= self.put(f, l, pixels)?;
            }
        }
        Ok(changed)
    }
}

/// Was jeder Effekt über den Frame wissen muss.
struct Ctx {
    w: usize,
    h: usize,
    f: usize,
    frames: usize,
    seed: u64,
    /// Halbe Deckkraft erlaubt (32 Bit, offene Palette); sonst wird gerastert.
    soft: bool,
}

impl Ctx {
    fn blank(&self) -> Vec<Rgba> {
        vec![Rgba::TRANSPARENT; self.w * self.h]
    }

    /// Wo der Frame in der Schleife steht, 0 bis unter 1.
    fn phase(&self) -> f32 {
        self.f as f32 / self.frames.max(1) as f32
    }

    fn set(&self, out: &mut [Rgba], x: i64, y: i64, c: Rgba) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            out[y as usize * self.w + x as usize] = c;
        }
    }

    /// `c` mit der Sichtbarkeit `k` (0 bis 1): halb durchsichtig in Vierteln, oder
    /// gerastert, wo das nicht geht.
    fn fade(&self, c: Rgba, k: f32, x: usize, y: usize, dither: bool) -> Rgba {
        let k = k.clamp(0.0, 1.0);
        if self.soft && !dither {
            let a = ((k * 4.0).ceil() / 4.0 * f32::from(c.a)).round() as u8;
            Rgba { a, ..c }
        } else if (bayer_threshold(x as i64, y as i64, 2) as f32) < k {
            c
        } else {
            Rgba::TRANSPARENT
        }
    }
}

/// Abstand jedes Pixels zum nächsten `source`-Pixel in Pixeln (3-4-Chamfer).
fn distance(source: &[bool], w: usize, h: usize) -> Vec<f32> {
    const FAR: u32 = u32::MAX / 4;
    let mut d: Vec<u32> = source.iter().map(|&s| if s { 0 } else { FAR }).collect();
    let relax = |d: &mut Vec<u32>, x: usize, y: usize, dx: i64, dy: i64, cost: u32| {
        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
        if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
            let v = d[ny as usize * w + nx as usize] + cost;
            let i = y * w + x;
            if v < d[i] {
                d[i] = v;
            }
        }
    };
    for y in 0..h {
        for x in 0..w {
            relax(&mut d, x, y, -1, 0, 3);
            relax(&mut d, x, y, 0, -1, 3);
            relax(&mut d, x, y, -1, -1, 4);
            relax(&mut d, x, y, 1, -1, 4);
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            relax(&mut d, x, y, 1, 0, 3);
            relax(&mut d, x, y, 0, 1, 3);
            relax(&mut d, x, y, 1, 1, 4);
            relax(&mut d, x, y, -1, 1, 4);
        }
    }
    d.into_iter().map(|v| v as f32 / 3.0).collect()
}

fn glow(
    ctx: &Ctx,
    src: &[Rgba],
    color: Rgba,
    radius: u32,
    pulse: u32,
    colors: &[Rgba],
    dither: bool,
) -> Vec<Rgba> {
    let mut out = ctx.blank();
    if src.len() != out.len() {
        return out;
    }
    let source: Vec<bool> = src
        .iter()
        .map(|c| c.a > 0 && (colors.is_empty() || colors.contains(c)))
        .collect();
    if !source.contains(&true) {
        return out;
    }
    let d = distance(&source, ctx.w, ctx.h);
    let breathe = (1.0 - (ctx.phase() * TAU).cos()) / 2.0;
    let r = radius as f32 - pulse as f32 * breathe;
    for (i, &di) in d.iter().enumerate() {
        if source[i] || di > r + 0.01 {
            continue;
        }
        // Drei Stufen: innen kräftig, außen schwach.
        let band = (((di - 1.0).max(0.0) / r) * 3.0).floor().min(2.0);
        let k = [1.0, 0.7, 0.4][band as usize];
        out[i] = ctx.fade(color, k, i % ctx.w, i / ctx.w, dither);
    }
    out
}

/// Eine gezackte Linie von `a` nach `b`, durch Verschieben der Mitten.
fn jagged(seed: u64, k: u64, a: (f32, f32), b: (f32, f32), rough: f32) -> Vec<(f32, f32)> {
    let mut pts = vec![a, b];
    for level in 0..8 {
        let mut next = Vec::with_capacity(pts.len() * 2);
        let mut longest = 0.0f32;
        for (j, pair) in pts.windows(2).enumerate() {
            let (p, q) = (pair[0], pair[1]);
            let (dx, dy) = (q.0 - p.0, q.1 - p.1);
            let len = (dx * dx + dy * dy).sqrt();
            longest = longest.max(len);
            next.push(p);
            if len >= 3.0 {
                let u = unit(hash(seed, level, j as i64, k)) as f32 - 0.5;
                let off = u * len * rough;
                next.push((
                    (p.0 + q.0) / 2.0 - dy / len * off,
                    (p.1 + q.1) / 2.0 + dx / len * off,
                ));
            }
        }
        next.push(*pts.last().unwrap_or(&b));
        pts = next;
        if longest < 3.0 {
            break;
        }
    }
    pts
}

/// Die Pixel einer Linie (Bresenham).
fn line_pixels(a: (f32, f32), b: (f32, f32), mut put: impl FnMut(i64, i64)) {
    let (mut x0, mut y0) = (a.0.round() as i64, a.1.round() as i64);
    let (x1, y1) = (b.0.round() as i64, b.1.round() as i64);
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let mut err = dx + dy;
    for _ in 0..4096 {
        put(x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn lightning(
    ctx: &Ctx,
    from: (f32, f32),
    to: (f32, f32),
    color: Rgba,
    glow: Option<Rgba>,
    branches: u32,
    flicker: bool,
) -> Vec<Rgba> {
    let mut out = ctx.blank();
    let f = ctx.f as u64;
    if flicker && ctx.frames > 1 && f > 0 && unit(hash(ctx.seed, f as i64, 0, 7)) < 0.3 {
        return out;
    }
    let mut core = vec![false; ctx.w * ctx.h];
    let mut mark = |pts: &[(f32, f32)]| {
        for pair in pts.windows(2) {
            line_pixels(pair[0], pair[1], |x, y| {
                if x >= 0 && y >= 0 && (x as usize) < ctx.w && (y as usize) < ctx.h {
                    core[y as usize * ctx.w + x as usize] = true;
                }
            });
        }
    };
    let main = jagged(ctx.seed.wrapping_add(f * 101), 1, from, to, 0.45);
    mark(&main);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    for b in 0..u64::from(branches) {
        let r = |k: u64| unit(hash(ctx.seed.wrapping_add(f * 101), b as i64, 3, k)) as f32;
        let at = main[((0.2 + r(1) * 0.6) * (main.len() - 1) as f32) as usize];
        let turn = (0.35 + r(2) * 0.5) * if r(3) < 0.5 { -1.0 } else { 1.0 };
        let (c, s) = (turn.cos(), turn.sin());
        let reach = len * (0.2 + r(4) * 0.25);
        let dir = (dx / len, dy / len);
        let end = (
            at.0 + (dir.0 * c - dir.1 * s) * reach,
            at.1 + (dir.0 * s + dir.1 * c) * reach,
        );
        mark(&jagged(
            ctx.seed.wrapping_add(f * 101 + b + 1),
            2,
            at,
            end,
            0.5,
        ));
    }
    if let Some(g) = glow {
        let soft = Rgba {
            a: if ctx.soft { 170 } else { 255 },
            ..g
        };
        for y in 0..ctx.h {
            for x in 0..ctx.w {
                if core[y * ctx.w + x] {
                    continue;
                }
                let near = [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .any(|(ox, oy)| {
                        let (nx, ny) = (x as i64 + ox, y as i64 + oy);
                        nx >= 0
                            && ny >= 0
                            && (nx as usize) < ctx.w
                            && (ny as usize) < ctx.h
                            && core[ny as usize * ctx.w + nx as usize]
                    });
                if near {
                    out[y * ctx.w + x] = soft;
                }
            }
        }
    }
    for (o, &c) in out.iter_mut().zip(&core) {
        if c {
            *o = color;
        }
    }
    out
}

fn explosion(
    ctx: &Ctx,
    at: (f32, f32),
    radius: u32,
    start: u32,
    length: u32,
    colors: &[Rgba],
) -> Vec<Rgba> {
    let mut out = ctx.blank();
    let start = start as usize;
    let length = if length == 0 {
        ctx.frames.saturating_sub(start).max(1)
    } else {
        length as usize
    };
    if ctx.f < start || ctx.f >= start + length || colors.is_empty() {
        return out;
    }
    let t = (ctx.f - start) as f32 / length as f32 + 0.5 / length as f32;
    let r = radius as f32;
    let n = colors.len();
    let grow = 1.0 - (1.0 - (t / 0.45).min(1.0)).powi(2);
    let rf = (r * (0.25 + 0.75 * grow)).max(1.0);
    let rise = t * t * r * 0.35;
    let seed = ctx.seed;
    let reach = (r * 1.4) as i64 + 2;
    let (cx, cy) = (at.0.round() as i64, at.1.round() as i64);
    for y in cy - reach..=cy + reach {
        for x in cx - reach..=cx + reach {
            if x < 0 || y < 0 || x as usize >= ctx.w || y as usize >= ctx.h {
                continue;
            }
            let (dx, dy) = (x as f32 - at.0, y as f32 - at.1 + rise);
            let d = (dx * dx + dy * dy).sqrt();
            let angle = dy.atan2(dx);
            let edge = rf
                * (0.78 + 0.35 * value_noise(seed, f64::from(angle) * 1.6 + 10.0, 0.0, 2) as f32);
            if d > edge {
                continue;
            }
            let grain = value_noise(
                seed,
                f64::from(x as f32 * 0.45),
                f64::from(y as f32 * 0.45 + t * 3.0),
                1,
            ) as f32;
            if t > 0.5 {
                let holes = (t - 0.5) / 0.5;
                if grain < holes * 0.9 + (d / edge) * holes * 0.4 {
                    continue;
                }
            }
            let heat =
                (1.0 - d / edge) * (1.25 - 1.15 * t) + (grain - 0.5) * 0.45 + 0.15 * (1.0 - t);
            let idx = (((1.0 - heat.clamp(0.0, 1.0)) * n as f32) as usize).min(n - 1);
            out[y as usize * ctx.w + x as usize] = colors[idx];
        }
    }
    // Trümmer und Funken fliegen nach außen und fallen.
    if t < 0.75 {
        let pieces = 6 + radius / 2;
        for i in 0..i64::from(pieces.min(40)) {
            let a = unit(hash(seed, i, 0, 11)) as f32 * TAU;
            let v = 0.6 + unit(hash(seed, i, 0, 12)) as f32 * 0.8;
            let pos = |t: f32| {
                (
                    at.0 + a.cos() * v * r * 1.3 * t,
                    at.1 + a.sin() * v * r * 1.3 * t + r * 0.9 * t * t,
                )
            };
            let (x, y) = pos(t);
            let (px, py) = pos((t - 0.08).max(0.0));
            let hot = colors[1.min(n - 1)];
            let cool = colors[(n / 2).min(n - 1)];
            ctx.set(&mut out, px.round() as i64, py.round() as i64, cool);
            ctx.set(&mut out, x.round() as i64, y.round() as i64, hot);
        }
    }
    out
}

fn reflection(
    ctx: &Ctx,
    src: &[Rgba],
    line: Option<usize>,
    color: Rgba,
    mix: u8,
    opacity: u8,
    wave: u32,
) -> Vec<Rgba> {
    let mut out = ctx.blank();
    let Some(line) = line else {
        return out;
    };
    if src.len() != out.len() || line == 0 || line >= ctx.h {
        return out;
    }
    let rows = line.min(ctx.h - line);
    let m = f32::from(mix) / 100.0;
    let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * m).round() as u8;
    let phase = ctx.phase() * TAU;
    for dist in 0..rows {
        let ty = line + dist;
        let sy = line - 1 - dist;
        let dx = if wave > 0 {
            (wave as f32 * (phase + dist as f32 * 0.9).sin()).round() as i64
        } else {
            0
        };
        // Drei Stufen nach unten schwächer; jede vierte Zeile ist eine helle Lücke.
        let step = (dist * 3 / rows.max(1)).min(2);
        let mut k = [1.0, 0.66, 0.36][step] * f32::from(opacity) / 255.0;
        if wave > 0 && (dist + ctx.f) % 4 == 3 {
            k *= 0.5;
        }
        for tx in 0..ctx.w {
            let sx = tx as i64 - dx;
            if sx < 0 || sx as usize >= ctx.w {
                continue;
            }
            let c = src[sy * ctx.w + sx as usize];
            if c.a == 0 {
                continue;
            }
            let tinted = Rgba::new(
                lerp(c.r, color.r),
                lerp(c.g, color.g),
                lerp(c.b, color.b),
                c.a,
            );
            out[ty * ctx.w + tx] = ctx.fade(tinted, k, tx, ty, false);
        }
    }
    out
}

fn particles(
    ctx: &Ctx,
    style: ParticleStyle,
    at: (f32, f32),
    spread: u32,
    count: u32,
    speed: f32,
    colors: &[Rgba],
) -> Vec<Rgba> {
    let mut out = ctx.blank();
    if colors.is_empty() {
        return out;
    }
    let n = ctx.frames.max(1);
    let life = ((n as f32 * style.life()).round() as usize).max(2);
    let (w, h) = (ctx.w as f32, ctx.h as f32);
    let size = w.max(h);
    let spread = spread as f32;
    for i in 0..i64::from(count) {
        let r = |k: u64| unit(hash(ctx.seed, i, 0, k)) as f32;
        // Alter im Leben, 0 bis unter 1. Ein Standbild zeigt alle Alter gemischt.
        let age = if n == 1 {
            r(1)
        } else {
            let birth = (r(1) * n as f32) as usize;
            let a = (ctx.f + n - birth % n) % n;
            if a >= life {
                continue;
            }
            a as f32 / life as f32
        };
        let x0 = at.0 + (r(2) - 0.5) * 2.0 * spread;
        let color = |a: f32| colors[((a * colors.len() as f32) as usize).min(colors.len() - 1)];
        match style {
            ParticleStyle::Sparks => {
                let angle = -std::f32::consts::FRAC_PI_2 + (r(3) - 0.5) * 2.1;
                let v = speed * size * (0.45 + r(4) * 0.4);
                let pos = |a: f32| {
                    (
                        x0 + angle.cos() * v * a,
                        at.1 + angle.sin() * v * a + size * 0.5 * speed * a * a,
                    )
                };
                let (x, y) = pos(age);
                if age > 0.0 {
                    let (px, py) = pos((age - 1.0 / life as f32).max(0.0));
                    let trail = color((age + 0.3).min(0.99));
                    ctx.set(&mut out, px.round() as i64, py.round() as i64, trail);
                }
                ctx.set(&mut out, x.round() as i64, y.round() as i64, color(age));
            }
            ParticleStyle::Smoke => {
                let rise = speed * size * (0.35 + r(4) * 0.25) * age;
                let sway = (age * TAU + r(5) * TAU).sin() * (1.0 + spread * 0.3).min(4.0);
                let (x, y) = (x0 + sway + age * size * 0.08, at.1 - rise);
                let rad = 1.0 + age * (2.0 + r(6) * 1.5);
                let k = 1.0 - age * 0.7;
                let c = color(age);
                let (cx, cy) = (x.round() as i64, y.round() as i64);
                let ri = rad.ceil() as i64;
                for py in cy - ri..=cy + ri {
                    for px in cx - ri..=cx + ri {
                        let (dx, dy) = (px as f32 - x, py as f32 - y);
                        if dx * dx + dy * dy > rad * rad
                            || px < 0
                            || py < 0
                            || px as usize >= ctx.w
                            || py as usize >= ctx.h
                        {
                            continue;
                        }
                        let p = ctx.fade(c, k, px as usize, py as usize, true);
                        if p.a > 0 {
                            ctx.set(&mut out, px, py, p);
                        }
                    }
                }
            }
            ParticleStyle::Magic => {
                let y0 = at.1 + (r(3) - 0.5) * 2.0 * spread;
                let (x, y) = (x0, y0 - speed * size * 0.12 * age);
                let (cx, cy) = (x.round() as i64, y.round() as i64);
                // Aufleuchten: Punkt, kleiner Stern, großer Stern, Punkt.
                let arm = match (age * 4.0) as u32 {
                    0 | 3 => 0,
                    1 => 1,
                    _ => 2,
                };
                let c = color(age);
                ctx.set(&mut out, cx, cy, colors[0]);
                for d in 1..=arm {
                    let tip = if d == arm && arm > 1 { color(0.99) } else { c };
                    for (ox, oy) in [(d, 0), (-d, 0), (0, d), (0, -d)] {
                        ctx.set(&mut out, cx + ox, cy + oy, tip);
                    }
                }
            }
        }
    }
    out
}

fn color_cycle(ctx: &Ctx, src: &[Rgba], colors: &[Rgba], speed: u32, reverse: bool) -> Vec<Rgba> {
    let mut out = ctx.blank();
    let n = colors.len();
    if src.len() != out.len() || n < 2 {
        return out;
    }
    let step = (ctx.f / speed.max(1) as usize) % n;
    let shift = if reverse { n - step } else { step };
    for (o, c) in out.iter_mut().zip(src) {
        if c.a == 0 {
            continue;
        }
        if let Some(i) = colors
            .iter()
            .position(|k| k.r == c.r && k.g == c.g && k.b == c.b)
        {
            *o = Rgba {
                a: c.a,
                ..colors[(i + shift) % n]
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{rect, Target};
    use crate::Palette;

    fn figure() -> Sprite {
        let mut s = Sprite::new(24, 24, ColorDepth::Rgba32, Palette::default()).unwrap();
        rect(
            &mut s,
            Target::new(0, 0),
            8,
            6,
            8,
            8,
            Rgba::rgb(200, 60, 60),
            true,
        )
        .unwrap();
        s
    }

    #[test]
    fn glow_sits_behind_the_figure_and_pulses() {
        let mut s = figure();
        s.ensure_frames(4).unwrap();
        let kind = EffectKind::default_for("glow", 24, 24).unwrap();
        s.set_effect(AutoEffect::new(kind)).unwrap();
        let l = s.layer_index("Leuchten").or(s.layer_index("Glow")).unwrap();
        assert_eq!(l, 0, "glow goes below the figure");
        let lit = |f: usize| {
            s.layer(f, l)
                .unwrap()
                .pixels()
                .iter()
                .filter(|p| p.a > 0)
                .count()
        };
        assert!(lit(0) > 0);
        assert!(
            lit(2) < lit(0),
            "the glow shrinks halfway through the pulse"
        );
        // Unter der Figur bleibt nichts.
        assert_eq!(
            s.layer(0, l).unwrap().get(10, 10).unwrap(),
            Rgba::TRANSPARENT
        );
    }

    #[test]
    fn color_cycles_move_each_color_one_step_per_frame() {
        let water = cycle_preset("water").unwrap();
        let mut s = Sprite::new(4, 1, ColorDepth::Rgba32, Palette::default()).unwrap();
        for (x, &c) in water.iter().enumerate() {
            s.set_pixel(0, 0, x as i64, 0, c).unwrap();
        }
        s.ensure_frames(4).unwrap();
        s.set_effect(AutoEffect::new(EffectKind::ColorCycle {
            colors: water.clone(),
            speed: 1,
            reverse: false,
        }))
        .unwrap();
        let l = s.effects()[0].layer.clone();
        let l = s.layer_index(&l).unwrap();
        assert_eq!(s.layer(1, l).unwrap().get(0, 0).unwrap(), water[1]);
        assert_eq!(s.layer(3, l).unwrap().get(2, 0).unwrap(), water[1]);
        assert_eq!(s.composite(0).unwrap()[0], water[0]);
    }

    #[test]
    fn effects_are_drawn_the_same_every_time_and_end_with_their_layer() {
        let mut s = figure();
        s.ensure_frames(6).unwrap();
        for id in EffectKind::IDS {
            let kind = EffectKind::default_for(id, 24, 24).unwrap();
            let mut e = AutoEffect::new(kind);
            e.layer = id.into();
            s.set_effect(e).unwrap();
        }
        let before = s.clone();
        assert!(!s.refresh_effects().unwrap(), "same input, same pixels");
        assert_eq!(s, before);
        let any = |s: &Sprite, name: &str| {
            let l = s.layer_index(name).unwrap();
            (0..6).any(|f| s.layer(f, l).unwrap().pixels().iter().any(|p| p.a > 0))
        };
        for id in EffectKind::IDS {
            if id != "color_cycle" {
                assert!(any(&s, id), "{id} draws something");
            }
        }
        let l = s.layer_index("sparks").unwrap();
        s.remove_layer(l).unwrap();
        s.refresh_effects().unwrap();
        assert!(s.effect("sparks").is_none());
        s.remove_effect("smoke", true).unwrap();
        assert!(s.layer_index("smoke").is_some());
        assert!(s.effect("smoke").is_none());
        let json = serde_json::to_string(&s).unwrap();
        let back: Sprite = serde_json::from_str(&json).unwrap();
        assert_eq!(back.effects(), s.effects());
    }
}
