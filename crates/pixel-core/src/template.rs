use serde::Serialize;

/// Eine Vorlage für gängige Bildformate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CanvasTemplate {
    pub id: &'static str,
    /// Name und Zweck auf Deutsch.
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub purpose: &'static str,
    /// Name und Zweck auf Englisch.
    pub name_en: &'static str,
    pub purpose_en: &'static str,
}

impl CanvasTemplate {
    /// Der Name in der eingestellten Sprache.
    pub fn label(&self) -> &'static str {
        crate::lang::tr(self.name, self.name_en)
    }

    /// Wofür die Vorlage gedacht ist, in der eingestellten Sprache.
    pub fn use_text(&self) -> &'static str {
        crate::lang::tr(self.purpose, self.purpose_en)
    }
}

const fn t(
    id: &'static str,
    (name, name_en): (&'static str, &'static str),
    width: u32,
    height: u32,
    (purpose, purpose_en): (&'static str, &'static str),
) -> CanvasTemplate {
    CanvasTemplate {
        id,
        name,
        width,
        height,
        purpose,
        name_en,
        purpose_en,
    }
}

pub const TEMPLATES: &[CanvasTemplate] = &[
    t(
        "icon-8",
        ("Mini-Icon 8×8", "Mini icon 8×8"),
        8,
        8,
        ("Winzige Icons, Partikel", "tiny icons, particles"),
    ),
    t(
        "icon-16",
        ("Icon 16×16", "Icon 16×16"),
        16,
        16,
        (
            "Items, Icons, klassische Tiles",
            "items, icons, classic tiles",
        ),
    ),
    t(
        "tile-16",
        ("Tile 16×16", "Tile 16×16"),
        16,
        16,
        ("Kacheln für Tilemaps", "tiles for tile maps"),
    ),
    t(
        "char-16x24",
        ("Figur 16×24", "Character 16×24"),
        16,
        24,
        ("Kleine Spielfigur", "small game character"),
    ),
    t(
        "tile-32",
        ("Tile 32×32", "Tile 32×32"),
        32,
        32,
        ("Kacheln, Figuren, Items", "tiles, characters, items"),
    ),
    t(
        "char-32x48",
        ("Figur 32×48", "Character 32×48"),
        32,
        48,
        ("Mittelgroße Spielfigur", "medium game character"),
    ),
    t(
        "char-48",
        ("Figur 48×48", "Character 48×48"),
        48,
        48,
        (
            "Figur mit Waffe oder Umhang",
            "character with weapon or cape",
        ),
    ),
    t(
        "char-64",
        ("Figur 64×64", "Character 64×64"),
        64,
        64,
        ("Detaillierte Figur, Gegner", "detailed character, enemy"),
    ),
    t(
        "portrait-96",
        ("Porträt 96×96", "Portrait 96×96"),
        96,
        96,
        ("Dialog-Porträts", "dialog portraits"),
    ),
    t(
        "boss-128",
        ("Boss 128×128", "Boss 128×128"),
        128,
        128,
        ("Große Gegner, Bosse", "large enemies, bosses"),
    ),
    t(
        "scene-160x144",
        ("Handheld-Bildschirm 160×144", "Handheld screen 160×144"),
        160,
        144,
        (
            "Szene im Format klassischer Handhelds",
            "scene in the format of classic handhelds",
        ),
    ),
    t(
        "scene-256x224",
        ("Konsolen-Bildschirm 256×224", "Console screen 256×224"),
        256,
        224,
        (
            "Szene im Format klassischer Heimkonsolen",
            "scene in the format of classic home consoles",
        ),
    ),
    t(
        "scene-320x180",
        ("Szene 320×180 (16:9)", "Scene 320×180 (16:9)"),
        320,
        180,
        (
            "Moderne Pixel-Szene, skaliert sauber auf 1080p",
            "modern 16:9 pixel scene, scales cleanly to 1080p",
        ),
    ),
    t(
        "scene-480x270",
        ("Szene 480×270 (16:9)", "Scene 480×270 (16:9)"),
        480,
        270,
        ("Größere 16:9-Szene", "larger 16:9 scene"),
    ),
    t(
        "art-256",
        ("Artwork 256×256", "Artwork 256×256"),
        256,
        256,
        ("Freies Pixel-Artwork", "free pixel artwork"),
    ),
    t(
        "art-512",
        ("Artwork 512×512", "Artwork 512×512"),
        512,
        512,
        ("Großes Artwork", "large artwork"),
    ),
    t(
        "art-1024",
        ("Artwork 1024×1024", "Artwork 1024×1024"),
        1024,
        1024,
        ("Maximale Größe", "maximum size"),
    ),
];

pub fn find(id: &str) -> Option<&'static CanvasTemplate> {
    TEMPLATES.iter().find(|t| t.id == id)
}
