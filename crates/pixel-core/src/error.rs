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
    #[error("{0}")]
    InvalidName(String),
    #[error("das Bild hat {found} Farben, erlaubt sind hier höchstens {max}")]
    TooManyColors { found: usize, max: usize },
    #[error("Fehler beim Schreiben der Bilddatei: {0}")]
    Encode(String),
    #[error("{0}")]
    Video(String),
    #[error("{0}")]
    Import(String),
    /// Fehler in einer Formel der Zeichen-Op `shader` (englisch, für MCP-Clients).
    #[error("formula: {0}")]
    Formula(String),
    /// Unbrauchbare Angabe für eine Zeichentechnik (englisch, für MCP-Clients).
    #[error("{0}")]
    Effect(String),
    #[error("WebM braucht eine gerade Vergrößerung (2, 4, 6 … bis {max}), damit die Pixel scharf bleiben", max = crate::export::MAX_SCALE)]
    OddVideoScale,
    #[error("ffmpeg wurde nicht gefunden. {help}", help = crate::video::FFMPEG_HELP)]
    FfmpegMissing,
    #[error("nicht unterstützte Projektversion {0}")]
    UnsupportedVersion(u32),
    #[error("E/A-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON-Fehler: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Bildfehler: {0}")]
    Image(#[from] image::ImageError),
}

impl Error {
    /// Dieselbe Meldung auf Englisch, für MCP-Clients. Meldungen, die als fertiger
    /// Text ankommen (Import, Video), bleiben, wie sie sind.
    pub fn english(&self) -> String {
        match self {
            Error::InvalidSize { width, height } => format!(
                "invalid size {width}×{height} (allowed: 1 to {})",
                crate::MAX_SIZE
            ),
            Error::OutOfBounds {
                x,
                y,
                width,
                height,
            } => format!("pixel ({x}, {y}) is outside the {width}×{height} canvas"),
            Error::NoSuchLayer(i) => format!("layer {i} does not exist"),
            Error::NoSuchFrame(i) => format!("frame {i} does not exist"),
            Error::NotFound { kind, id } => {
                let kind = match *kind {
                    "Figur" => "character",
                    "Animation" => "animation",
                    "Bild" => "image",
                    other => other,
                };
                format!("{kind} '{id}' not found")
            }
            Error::PaletteFull(max) => format!("the palette is full (at most {max} colors)"),
            Error::ColorNotInPalette(c) => {
                format!("the palette is locked and does not contain {c}")
            }
            Error::PaletteLocked => {
                "the palette is locked because it belongs to a character; use add_character_colors to extend it".into()
            }
            Error::EmptyPalette => "a character needs a palette with at least one color".into(),
            Error::CoordOutOfRange(v) => format!(
                "coordinate or size {v} is too far outside the canvas (allowed: ±{})",
                crate::draw::COORD_LIMIT
            ),
            Error::SizeMismatch {
                expected_w,
                expected_h,
                got_w,
                got_h,
            } => format!("size mismatch: expected {expected_w}×{expected_h}, got {got_w}×{got_h}"),
            Error::InvalidColor(c) => {
                format!("invalid color '{c}' (expected #RRGGBB or #RRGGBBAA)")
            }
            Error::TooManyColors { found, max } => {
                format!("the picture has {found} colors, at most {max} are allowed here")
            }
            Error::OddVideoScale => format!(
                "webm needs an even scale (2, 4, 6 … up to {}) so the pixels stay sharp",
                crate::export::MAX_SCALE
            ),
            Error::FfmpegMissing => "ffmpeg was not found".into(),
            Error::UnsupportedVersion(v) => format!("unsupported project version {v}"),
            Error::Io(err) => format!("file error: {err}"),
            Error::Json(err) => format!("JSON error: {err}"),
            Error::Image(err) => format!("image error: {err}"),
            Error::InvalidPixelData(_)
            | Error::InvalidName(_)
            | Error::Encode(_)
            | Error::Video(_)
            | Error::Import(_)
            | Error::Formula(_)
            | Error::Effect(_) => self.to_string(),
        }
    }
}
