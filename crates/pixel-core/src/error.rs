use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("ungültige Bildgröße {width}×{height} (erlaubt: 1 bis {max})", max = crate::MAX_SIZE)]
    InvalidSize { width: u32, height: u32 },
    #[error("Pixel ({x}, {y}) liegt außerhalb des Bildes {width}×{height}")]
    OutOfBounds {
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    },
    #[error("Ebene {0} existiert nicht")]
    NoSuchLayer(usize),
    #[error("Frame {0} existiert nicht")]
    NoSuchFrame(usize),
    #[error("{kind} '{id}' nicht gefunden")]
    NotFound { kind: &'static str, id: String },
    #[error("Palette ist voll (maximal {0} Farben)")]
    PaletteFull(usize),
    #[error("Palette ist gesperrt; Farbe {0} ist nicht enthalten")]
    ColorNotInPalette(crate::Rgba),
    #[error("Palette ist gesperrt, weil sie zu einer Figur gehört")]
    PaletteLocked,
    #[error("Eine Figur braucht eine Palette mit mindestens einer Farbe")]
    EmptyPalette,
    #[error("Koordinate oder Größe {0} liegt zu weit außerhalb des Bildes (erlaubt: ±{limit})", limit = crate::draw::COORD_LIMIT)]
    CoordOutOfRange(i64),
    #[error("Größe passt nicht: erwartet {expected_w}×{expected_h}, bekommen {got_w}×{got_h}")]
    SizeMismatch {
        expected_w: u32,
        expected_h: u32,
        got_w: u32,
        got_h: u32,
    },
    #[error("ungültige Farbe '{0}' (erwartet #RRGGBB oder #RRGGBBAA)")]
    InvalidColor(String),
    #[error("ungültige Pixeldaten: {0}")]
    InvalidPixelData(String),
    #[error("nicht unterstützte Projektversion {0}")]
    UnsupportedVersion(u32),
    #[error("E/A-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON-Fehler: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Bildfehler: {0}")]
    Image(#[from] image::ImageError),
}
