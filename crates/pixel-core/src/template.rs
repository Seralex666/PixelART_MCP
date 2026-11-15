use serde::Serialize;

/// Eine Vorlage für gängige Bildformate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CanvasTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub purpose: &'static str,
}

const fn t(id: &'static str, name: &'static str, width: u32, height: u32, purpose: &'static str) -> CanvasTemplate {
    CanvasTemplate { id, name, width, height, purpose }
}

pub const TEMPLATES: &[CanvasTemplate] = &[
    t("icon-8", "Mini-Icon 8×8", 8, 8, "Winzige Icons, Partikel"),
    t("icon-16", "Icon 16×16", 16, 16, "Items, Icons, klassische Tiles"),
    t("tile-16", "Tile 16×16", 16, 16, "Kacheln für Tilemaps"),
    t("char-16x24", "Figur 16×24", 16, 24, "Kleine Spielfigur"),
    t("tile-32", "Tile 32×32", 32, 32, "Kacheln, Figuren, Items"),
    t("char-32x48", "Figur 32×48", 32, 48, "Mittelgroße Spielfigur"),
    t("char-48", "Figur 48×48", 48, 48, "Figur mit Waffe oder Umhang"),
    t("char-64", "Figur 64×64", 64, 64, "Detaillierte Figur, Gegner"),
    t("portrait-96", "Porträt 96×96", 96, 96, "Dialog-Porträts"),
    t("boss-128", "Boss 128×128", 128, 128, "Große Gegner, Bosse"),
    t("scene-160x144", "Handheld-Bildschirm 160×144", 160, 144, "Szene im Format klassischer Handhelds"),
    t("scene-256x224", "Konsolen-Bildschirm 256×224", 256, 224, "Szene im Format klassischer Heimkonsolen"),
    t("scene-320x180", "Szene 320×180 (16:9)", 320, 180, "Moderne Pixel-Szene, skaliert sauber auf 1080p"),
    t("scene-480x270", "Szene 480×270 (16:9)", 480, 270, "Größere 16:9-Szene"),
    t("art-256", "Artwork 256×256", 256, 256, "Freies Pixel-Artwork"),
    t("art-512", "Artwork 512×512", 512, 512, "Großes Artwork"),
    t("art-1024", "Artwork 1024×1024", 1024, 1024, "Maximale Größe"),
];

pub fn find(id: &str) -> Option<&'static CanvasTemplate> {
    TEMPLATES.iter().find(|t| t.id == id)
}
