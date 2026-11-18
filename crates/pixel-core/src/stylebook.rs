//! Die Stil-Bibliothek: feste Stile wie Comic, Manga oder 8 Bit und eigene,
//! die im Speicherordner unter `styles` liegen und in allen Projekten gelten.
//!
//! Ein Stil beschreibt, wie Bilder aussehen sollen: Palette, Farbtiefe,
//! empfohlene Größen, Umriss, Schattierung und Regeln in Worten. Der Editor
//! zeigt die Stile zum Nachlesen, und Claude holt sich einen Stil per Namen,
//! etwa nur „Manga“, und weiß damit genau, wie das Bild werden soll.

use crate::lang::tr;
use crate::{Error, Palette, Result, Rgba};
use serde::{Deserialize, Serialize};

/// So viele Regeln darf ein Stil haben.
pub const MAX_RULES: usize = 20;
/// So lang darf eine Regel sein (Zeichen).
pub const MAX_RULE_LEN: usize = 300;
/// So lang darf die Beschreibung sein (Zeichen).
pub const MAX_DESCRIPTION: usize = 500;

/// Wie die Umrisse in einem Stil aussehen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutlineRule {
    /// Keine Umrisse, Formen grenzen nur über Farbe aneinander.
    None,
    /// Schwarze Umrisse.
    Black,
    /// Umrisse in der dunkelsten Farbe der Palette.
    #[default]
    Dark,
    /// Selektiv: jeder Umrisspixel ist eine dunklere Stufe der Farbe daneben.
    Selective,
}

impl OutlineRule {
    pub const ALL: [OutlineRule; 4] = [
        OutlineRule::None,
        OutlineRule::Black,
        OutlineRule::Dark,
        OutlineRule::Selective,
    ];

    pub fn id(self) -> &'static str {
        match self {
            OutlineRule::None => "none",
            OutlineRule::Black => "black",
            OutlineRule::Dark => "dark",
            OutlineRule::Selective => "selective",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OutlineRule::None => tr("Keine", "None"),
            OutlineRule::Black => tr("Schwarz", "Black"),
            OutlineRule::Dark => tr("Dunkelste Palettenfarbe", "Darkest palette color"),
            OutlineRule::Selective => tr(
                "Selektiv (dunklere Stufe der Farbe daneben)",
                "Selective (a darker step of the color next to it)",
            ),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|o| o.id() == t)
    }
}

fn one() -> u8 {
    1
}

fn three() -> u8 {
    3
}

fn full_depth() -> u8 {
    32
}

/// Ein Stil aus der Bibliothek.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StylePreset {
    /// Kurzer Name, unter dem man den Stil findet, etwa „Manga“.
    pub name: String,
    /// Wofür der Stil gedacht ist und wie er wirkt.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Die Farben des Stils.
    #[serde(default)]
    pub palette: Vec<Rgba>,
    /// Farbtiefe für neue Bilder: 8, 16 oder 32 Bit.
    #[serde(default = "full_depth")]
    pub color_depth: u8,
    /// Empfohlene Größen in Pixeln (Breite, Höhe), die erste ist die übliche.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<[u32; 2]>,
    /// Höchstens so viele Farben pro Figur oder Bild, wenn der Stil das vorgibt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors_per_sprite: Option<u32>,
    /// Wie die Umrisse aussehen.
    #[serde(default)]
    pub outline: OutlineRule,
    /// Umrissdicke in Pixeln.
    #[serde(default = "one")]
    pub outline_width: u8,
    /// Stufen je Farbe, vom Schatten bis zum Licht (mit der Grundfarbe).
    #[serde(default = "three")]
    pub shades: u8,
    /// Ob Dithering (Raster aus zwei Farben) zum Stil gehört.
    #[serde(default)]
    pub dithering: bool,
    /// Ob Schatten zu kühleren und Lichter zu wärmeren Farbtönen wandern.
    #[serde(default)]
    pub hue_shift: bool,
    /// Ob Kanten von Hand mit Zwischenfarben geglättet werden dürfen.
    #[serde(default)]
    pub anti_alias: bool,
    /// Woher das Licht kommt, einer der Werte aus [`crate::project::LIGHT_DIRS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_dir: Option<String>,
    /// Regeln in Worten, an die sich jeder hält, der in diesem Stil zeichnet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// Fest eingebaut (nicht änderbar) oder selbst angelegt.
    #[serde(skip)]
    pub builtin: bool,
    /// Kurzname der festen Stile (`manga`, `8bit` …), bei eigenen leer.
    #[serde(skip)]
    pub id: &'static str,
}

/// Vergleichsschlüssel für Stilnamen: nur Buchstaben und Ziffern, klein.
/// So findet „8 Bit“, „8bit“ und „8-Bit“ denselben Stil.
pub fn key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn hex(colors: &[&str]) -> Vec<Rgba> {
    colors
        .iter()
        .map(|c| c.parse().expect("feste Stilfarbe"))
        .collect()
}

impl StylePreset {
    /// Ein leerer eigener Stil mit sinnvollen Grundwerten.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            palette: Palette::ramps_32().colors,
            color_depth: 32,
            sizes: vec![[32, 32]],
            colors_per_sprite: None,
            outline: OutlineRule::Dark,
            outline_width: 1,
            shades: 3,
            dithering: false,
            hue_shift: false,
            anti_alias: false,
            light_dir: Some("top-left".into()),
            rules: Vec::new(),
            builtin: false,
            id: "",
        }
    }

    /// Ob `name` diesen Stil meint: Name, Kurzname oder ein Zweitname.
    pub fn matches(&self, name: &str) -> bool {
        let k = key(name);
        if k.is_empty() {
            return false;
        }
        k == key(&self.name) || (!self.id.is_empty() && k == key(self.id)) || {
            aliases(self.id).iter().any(|a| key(a) == k)
        }
    }

    /// Die Umrissfarbe, die der Stil vorgibt (keine bei selektiv oder ohne Umriss).
    pub fn outline_color(&self) -> Option<Rgba> {
        match self.outline {
            OutlineRule::Black => Some(Rgba::rgb(0, 0, 0)),
            OutlineRule::Dark => self
                .palette
                .iter()
                .copied()
                .min_by_key(|c| crate::quant::luminance(*c)),
            OutlineRule::None | OutlineRule::Selective => None,
        }
    }

    /// Prüft die Angaben, bevor ein eigener Stil gespeichert wird.
    pub fn check(&self) -> Result<()> {
        let bad = |de: &'static str, en: &'static str| Err(Error::Effect(tr(de, en).to_string()));
        if key(&self.name).is_empty() {
            return bad(
                "Der Stil braucht einen Namen mit Buchstaben oder Ziffern",
                "The style needs a name with letters or digits",
            );
        }
        if self.name.chars().count() > 40 {
            return bad(
                "Der Name darf höchstens 40 Zeichen lang sein",
                "The name can have at most 40 characters",
            );
        }
        if self.palette.is_empty() || self.palette.len() > Palette::MAX_COLORS {
            return bad(
                "Ein Stil braucht 1 bis 256 Farben",
                "A style needs 1 to 256 colors",
            );
        }
        if !matches!(self.color_depth, 8 | 16 | 32) {
            return bad(
                "Die Farbtiefe muss 8, 16 oder 32 Bit sein",
                "The color depth must be 8, 16 or 32 bits",
            );
        }
        if self
            .sizes
            .iter()
            .any(|[w, h]| *w == 0 || *h == 0 || *w > 1024 || *h > 1024)
        {
            return bad(
                "Größen müssen zwischen 1 und 1024 Pixeln liegen",
                "Sizes must be between 1 and 1024 pixels",
            );
        }
        if !(1..=4).contains(&self.outline_width) {
            return bad(
                "Die Umrissdicke muss 1 bis 4 Pixel sein",
                "The outline width must be 1 to 4 pixels",
            );
        }
        if !(1..=8).contains(&self.shades) {
            return bad(
                "Ein Stil hat 1 bis 8 Stufen je Farbe",
                "A style has 1 to 8 shades per color",
            );
        }
        if let Some(dir) = &self.light_dir {
            if !crate::project::LIGHT_DIRS.contains(&dir.as_str()) {
                return bad("Unbekannte Lichtrichtung", "Unknown light direction");
            }
        }
        if self.description.chars().count() > MAX_DESCRIPTION
            || self.rules.len() > MAX_RULES
            || self.rules.iter().any(|r| r.chars().count() > MAX_RULE_LEN)
        {
            return Err(Error::Effect(crate::trf!(
                "Höchstens {MAX_DESCRIPTION} Zeichen Beschreibung und {MAX_RULES} Regeln mit je {MAX_RULE_LEN} Zeichen",
                "At most {MAX_DESCRIPTION} characters of description and {MAX_RULES} rules of {MAX_RULE_LEN} characters each"
            )));
        }
        Ok(())
    }

    /// Übernimmt Palette, Licht, Umriss und Notizen in den Stil eines Projekts.
    pub fn apply_to(&self, style: &mut crate::Style) {
        style.palette = self.palette.clone();
        style.light_dir = self.light_dir.clone();
        style.outline = self.outline_color();
        style.preset = Some(self.name.clone());
    }

    /// Ein eigener Stil aus dem Stil eines Projekts.
    pub fn from_project(name: impl Into<String>, style: &crate::Style) -> Self {
        let mut s = Self::new(name);
        if !style.palette.is_empty() {
            s.palette = style.palette.clone();
        }
        s.light_dir = style.light_dir.clone().or(s.light_dir);
        if let Some(c) = style.outline {
            s.outline = if c == Rgba::rgb(0, 0, 0) {
                OutlineRule::Black
            } else {
                OutlineRule::Dark
            };
        }
        if !style.notes.is_empty() {
            s.rules = vec![style.notes.clone()];
        }
        s
    }
}

fn aliases(id: &str) -> &'static [&'static str] {
    match id {
        "castle" => &["Burgen", "Burg", "Castles", "Medieval", "Mittelalter"],
        "hq" => &[
            "HQ",
            "High Quality",
            "HQ Pixel Art",
            "HQ Pixel-Art",
            "Hi-Bit",
        ],
        "8bit" => &["8 Bit", "NES", "Retro"],
        "16bit" => &["16 Bit", "SNES"],
        "comic" => &["Comics", "Cartoon"],
        "manga" => &["Anime"],
        _ => &[],
    }
}

/// Die fest eingebauten Stile, in der aktuellen Sprache.
pub fn builtin() -> Vec<StylePreset> {
    let base = |id: &'static str, name: &str| StylePreset {
        builtin: true,
        id,
        ..StylePreset::new(name)
    };
    let rules = |list: &[(&'static str, &'static str)]| -> Vec<String> {
        list.iter().map(|(de, en)| tr(de, en).to_string()).collect()
    };
    vec![
        StylePreset {
            description: tr(
                "Klassische Konsolen-Optik der 80er: wenige kräftige Farben, kleine Figuren, klare Formen.",
                "Classic 80s console look: few strong colors, small sprites, clear shapes.",
            )
            .into(),
            palette: hex(&[
                "#000000", "#545454", "#a8a8a8", "#fcfcfc", "#7c0800", "#d82800", "#fc7460",
                "#fcbcb0", "#503000", "#ac7c00", "#f8b800", "#fce0a8", "#005800", "#00a800",
                "#58d854", "#b8f8b8", "#00008c", "#0058f8", "#6888fc", "#b8b8f8", "#940084",
                "#d800cc", "#f878f8", "#007878", "#00b8b8",
            ]),
            color_depth: 8,
            sizes: vec![[16, 16], [16, 24], [32, 32]],
            colors_per_sprite: Some(4),
            outline: OutlineRule::Black,
            shades: 2,
            rules: rules(&[
                ("Höchstens 3 Farben plus Transparenz pro Figur oder Bauteil.", "At most 3 colors plus transparency per sprite or part."),
                ("Keine Kantenglättung und keine Verläufe, nur harte Pixelkanten.", "No anti-aliasing and no gradients, only hard pixel edges."),
                ("Schattierung mit höchstens einer dunkleren Stufe.", "Shading with at most one darker step."),
                ("Große, lesbare Formen: Kopf etwa ein Drittel der Figur, dicke Augen aus 1 bis 2 Pixeln.", "Big readable shapes: head about a third of the sprite, eyes of 1 to 2 pixels."),
                ("Hintergrund-Kacheln 16×16, Muster dürfen sich sichtbar wiederholen.", "Background tiles 16×16, patterns may repeat visibly."),
            ]),
            ..base("8bit", "8 Bit")
        },
        StylePreset {
            description: tr(
                "Die Optik der 16-Bit-Konsolen: mehr Farben, weichere Schattierung, selektive Umrisse.",
                "The 16-bit console look: more colors, softer shading, selective outlines.",
            )
            .into(),
            palette: Palette::ramps_32().colors,
            color_depth: 16,
            sizes: vec![[32, 32], [24, 32], [48, 48], [64, 64]],
            colors_per_sprite: Some(16),
            outline: OutlineRule::Selective,
            shades: 4,
            hue_shift: true,
            rules: rules(&[
                ("Höchstens 16 Farben pro Figur, Farben aus Farbtreppen.", "At most 16 colors per sprite, colors from ramps."),
                ("3 bis 4 Stufen je Material, Schatten etwas kühler, Lichter etwas wärmer.", "3 to 4 shades per material, shadows slightly cooler, highlights slightly warmer."),
                ("Selektive Umrisse: außen dunkel, zur Lichtseite heller oder offen.", "Selective outlines: dark outside, lighter or open on the lit side."),
                ("Dithering nur sparsam für große Flächen wie Himmel.", "Dithering only sparingly for large areas like skies."),
                ("Glanzlichter als einzelne helle Pixel auf Metall und Augen.", "Highlights as single bright pixels on metal and eyes."),
            ]),
            ..base("16bit", "16 Bit")
        },
        StylePreset {
            description: tr(
                "Moderne, detaillierte Pixel-Art mit vielen Farbstufen, Farbton-Verschiebung und sauber geglätteten Kanten.",
                "Modern detailed pixel art with many shades, hue shifting and carefully anti-aliased edges.",
            )
            .into(),
            palette: Palette::ramps_64().colors,
            color_depth: 32,
            sizes: vec![[64, 64], [48, 64], [96, 96], [128, 128]],
            outline: OutlineRule::Selective,
            shades: 5,
            hue_shift: true,
            anti_alias: true,
            rules: rules(&[
                ("4 bis 6 Stufen je Material mit deutlicher Farbton-Verschiebung.", "4 to 6 shades per material with clear hue shifting."),
                ("Kantenglättung von Hand an Rundungen, nie an der Außenkante gegen Transparenz.", "Hand-placed anti-aliasing on curves, never on the outer edge against transparency."),
                ("Keine Banding-Treppen: Stufen versetzt und unterschiedlich breit setzen.", "No banding: offset the shade steps and vary their width."),
                ("Materialien klar trennen: Metall mit hartem Glanz, Stoff weich, Haut warm.", "Separate materials clearly: metal with hard shine, cloth soft, skin warm."),
                ("Umgebungslicht und Reflexlicht auf der Schattenseite.", "Ambient and reflected light on the shadow side."),
            ]),
            ..base("hq", tr("HQ Pixel-Art", "HQ Pixel Art"))
        },
        StylePreset {
            description: tr(
                "Wie gezeichnete Comics: dicke schwarze Linien, flache kräftige Farben, harte Schatten.",
                "Like drawn comics: thick black lines, flat bold colors, hard shadows.",
            )
            .into(),
            palette: hex(&[
                "#000000", "#ffffff", "#f4d6b5", "#d9a37f", "#e83b3b", "#9e1f2c", "#f7c936",
                "#c7861a", "#3b7de8", "#1f3f9e", "#4cc35a", "#1f7a3a", "#a05ad6", "#5c2a8a",
                "#ff8c2e", "#8a8f99", "#4a4e57",
            ]),
            sizes: vec![[64, 64], [48, 64], [96, 96]],
            outline: OutlineRule::Black,
            outline_width: 2,
            shades: 2,
            dithering: true,
            rules: rules(&[
                ("Schwarze Umrisse, außen 2 Pixel, Innenlinien 1 Pixel.", "Black outlines, 2 pixels outside, 1 pixel for inner lines."),
                ("Flache Farben mit genau einem harten Schatten, keine Verläufe.", "Flat colors with exactly one hard shadow, no gradients."),
                ("Rasterpunkte (Dithering) als Comic-Punkte in Schatten und Hintergründen.", "Dithering as comic halftone dots in shadows and backgrounds."),
                ("Übertriebene Posen und Mimik, Bewegungslinien bei Aktion.", "Exaggerated poses and faces, speed lines for action."),
                ("Weiße Glanzlichter als kleine Striche.", "White highlights as small strokes."),
            ]),
            ..base("comic", "Comic")
        },
        StylePreset {
            description: tr(
                "Schwarz-weiß wie gedruckter Manga: Graustufen, Rasterfolie statt Farbe, große Augen, ein Akzent in Rot.",
                "Black and white like printed manga: grays, screentone instead of color, big eyes, one red accent.",
            )
            .into(),
            palette: hex(&[
                "#000000", "#1e1e1e", "#3c3c3c", "#6b6b6b", "#9c9c9c", "#cfcfcf", "#ffffff",
                "#f3e3d3", "#c8102e",
            ]),
            sizes: vec![[64, 96], [64, 64], [96, 128]],
            outline: OutlineRule::Black,
            shades: 3,
            dithering: true,
            rules: rules(&[
                ("Fast nur Schwarz, Weiß und Grau; Rot nur als einzelner Akzent.", "Almost only black, white and gray; red only as a single accent."),
                ("Schatten als Rasterfolie: gleichmäßiges Dithering statt Grauflächen.", "Shadows as screentone: even dithering instead of flat grays."),
                ("Große Augen mit weißem Glanzpunkt, kleiner Mund, spitzes Kinn.", "Big eyes with a white shine, small mouth, pointed chin."),
                ("Haare als große Strähnen mit schwarzen Linien und weißen Glanzstreifen.", "Hair as big strands with black lines and white shine streaks."),
                ("Starke Kontraste, viel Weiß als Papierfarbe.", "Strong contrast, lots of white as paper."),
            ]),
            ..base("manga", "Manga")
        },
        StylePreset {
            description: tr(
                "Burgen und Mittelalter: Stein, Holz, Moos, Banner und Fackellicht, für Kacheln und Szenen.",
                "Castles and the middle ages: stone, wood, moss, banners and torchlight, for tiles and scenes.",
            )
            .into(),
            palette: hex(&[
                "#1b1c22", "#33343d", "#4f515c", "#6f727d", "#959aa3", "#c0c4c9", "#2e1f16",
                "#5a3a24", "#8a5a35", "#b98a55", "#26381f", "#46632d", "#7a9a3e", "#5e1a1e",
                "#a32a2a", "#d8483a", "#1e2a5e", "#3550a3", "#e0761f", "#ffc04a", "#fff1a8",
                "#7fa6c9",
            ]),
            color_depth: 16,
            sizes: vec![[32, 32], [16, 16], [64, 64]],
            outline: OutlineRule::Selective,
            shades: 4,
            dithering: true,
            hue_shift: true,
            rules: rules(&[
                ("Steine kühl grau, jeder Stein mit heller Oberkante und dunkler Fuge.", "Stones cool gray, each stone with a light top edge and a dark joint."),
                ("Holz warm braun mit Maserung aus 1-Pixel-Linien.", "Wood warm brown with grain made of 1-pixel lines."),
                ("Moos und Risse als kleine unregelmäßige Flecken, sparsam.", "Moss and cracks as small irregular spots, sparingly."),
                ("Fackeln als warmes Licht mit Leuchten in Orange und Gelb.", "Torches as warm light with an orange and yellow glow."),
                ("Kacheln 32×32, nahtlos kachelbar; mit `view` und `tile` prüfen.", "Tiles 32×32, seamless; check with `view` and `tile`."),
            ]),
            ..base("castle", tr("Burgen", "Castles"))
        },
    ]
}

/// Sucht einen Stil zuerst unter den festen, dann unter den eigenen.
pub fn find<'a>(styles: &'a [StylePreset], name: &str) -> Option<&'a StylePreset> {
    styles.iter().find(|s| s.matches(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_styles_are_valid_and_found_by_many_names() {
        let all = builtin();
        assert_eq!(all.len(), 6);
        for s in &all {
            s.check().unwrap();
            assert!(!s.rules.is_empty());
        }
        for (name, id) in [
            ("manga", "manga"),
            ("8bit", "8bit"),
            ("8-Bit", "8bit"),
            ("16 bit", "16bit"),
            ("Burgen", "castle"),
            ("castle", "castle"),
            ("hq pixel art", "hq"),
            ("Comic", "comic"),
        ] {
            assert_eq!(find(&all, name).map(|s| s.id), Some(id), "{name}");
        }
        assert!(find(&all, "aquarell").is_none());
        let eight = find(&all, "8bit").unwrap();
        assert_eq!(eight.outline_color(), Some(Rgba::rgb(0, 0, 0)));
        assert!(find(&all, "16bit").unwrap().outline_color().is_none());
    }

    #[test]
    fn own_styles_are_checked_and_keep_their_settings_through_json() {
        let mut s = StylePreset::new("Sumpf");
        s.rules = vec!["grün und braun".into()];
        s.check().unwrap();
        let json = serde_json::to_string(&s).unwrap();
        let back: StylePreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        s.shades = 0;
        assert!(s.check().is_err());
        s.shades = 3;
        s.light_dir = Some("oben".into());
        assert!(s.check().is_err());
    }
}
