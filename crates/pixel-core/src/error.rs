use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Fehler des Kerns. Die Meldung erscheint in der eingestellten Sprache
/// (siehe [`crate::lang`]); [`Error::english`] gibt sie immer auf Englisch.
#[derive(Debug)]
pub enum Error {
    InvalidSize {
        width: u32,
        height: u32,
    },
    OutOfBounds {
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    },
    NoSuchLayer(usize),
    NoSuchFrame(usize),
    NotFound {
        kind: &'static str,
        id: String,
    },
    PaletteFull(usize),
    ColorNotInPalette(crate::Rgba),
    PaletteLocked,
    EmptyPalette,
    CoordOutOfRange(i64),
    SizeMismatch {
        expected_w: u32,
        expected_h: u32,
        got_w: u32,
        got_h: u32,
    },
    InvalidColor(String),
    InvalidPixelData(String),
    /// Fertiger Text in der eingestellten Sprache.
    InvalidName(String),
    TooManyColors {
        found: usize,
        max: usize,
    },
    Encode(String),
    /// Fertiger Text in der eingestellten Sprache.
    Video(String),
    /// Fertiger Text in der eingestellten Sprache.
    Import(String),
    /// Fehler in einer Formel der Zeichen-Op `shader` (englisch, für MCP-Clients).
    Formula(String),
    /// Unbrauchbare Angabe für eine Zeichentechnik (englisch, für MCP-Clients).
    Effect(String),
    OddVideoScale,
    FfmpegMissing,
    UnsupportedVersion(u32),
    Io(std::io::Error),
    Json(serde_json::Error),
    Image(image::ImageError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if crate::lang::is_en() {
            f.write_str(&self.english())
        } else {
            f.write_str(&self.german())
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Image(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Error::Image(e)
    }
}

impl Error {
    /// Die Meldung auf Deutsch.
    pub fn german(&self) -> String {
        match self {
            Error::InvalidSize { width, height } => format!(
                "ungültige Bildgröße {width}×{height} (erlaubt: 1 bis {})",
                crate::MAX_SIZE
            ),
            Error::OutOfBounds {
                x,
                y,
                width,
                height,
            } => format!("Pixel ({x}, {y}) liegt außerhalb des Bildes {width}×{height}"),
            Error::NoSuchLayer(i) => format!("Ebene {i} existiert nicht"),
            Error::NoSuchFrame(i) => format!("Frame {i} existiert nicht"),
            Error::NotFound { kind, id } => format!("{kind} '{id}' nicht gefunden"),
            Error::PaletteFull(max) => format!("Palette ist voll (maximal {max} Farben)"),
            Error::ColorNotInPalette(c) => {
                format!("Palette ist gesperrt; Farbe {c} ist nicht enthalten")
            }
            Error::PaletteLocked => "Palette ist gesperrt, weil sie zu einer Figur gehört".into(),
            Error::EmptyPalette => "Eine Figur braucht eine Palette mit mindestens einer Farbe".into(),
            Error::CoordOutOfRange(v) => format!(
                "Koordinate oder Größe {v} liegt zu weit außerhalb des Bildes (erlaubt: ±{})",
                crate::draw::COORD_LIMIT
            ),
            Error::SizeMismatch {
                expected_w,
                expected_h,
                got_w,
                got_h,
            } => format!(
                "Größe passt nicht: erwartet {expected_w}×{expected_h}, bekommen {got_w}×{got_h}"
            ),
            Error::InvalidColor(c) => {
                format!("ungültige Farbe '{c}' (erwartet #RRGGBB oder #RRGGBBAA)")
            }
            Error::InvalidPixelData(m) => format!("ungültige Pixeldaten: {m}"),
            Error::TooManyColors { found, max } => {
                format!("das Bild hat {found} Farben, erlaubt sind hier höchstens {max}")
            }
            Error::Encode(m) => format!("Fehler beim Schreiben der Bilddatei: {m}"),
            Error::Formula(m) => format!("formula: {m}"),
            Error::InvalidName(m) | Error::Video(m) | Error::Import(m) | Error::Effect(m) => {
                m.clone()
            }
            Error::OddVideoScale => format!(
                "WebM braucht eine gerade Vergrößerung (2, 4, 6 … bis {}), damit die Pixel scharf bleiben",
                crate::export::MAX_SCALE
            ),
            Error::FfmpegMissing => format!(
                "ffmpeg wurde nicht gefunden. {}",
                crate::video::ffmpeg_help_de()
            ),
            Error::UnsupportedVersion(v) => format!("nicht unterstützte Projektversion {v}"),
            Error::Io(e) => format!("E/A-Fehler: {e}"),
            Error::Json(e) => format!("JSON-Fehler: {e}"),
            Error::Image(e) => format!("Bildfehler: {e}"),
        }
    }

    /// Die Meldung auf Englisch. Meldungen, die als fertiger Text ankommen
    /// (Import, Video, Namen), stehen schon in der eingestellten Sprache.
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
            Error::FfmpegMissing => format!("ffmpeg was not found. {}", crate::video::ffmpeg_help_en()),
            Error::UnsupportedVersion(v) => format!("unsupported project version {v}"),
            Error::Io(err) => format!("file error: {err}"),
            Error::Json(err) => format!("JSON error: {err}"),
            Error::Image(err) => format!("image error: {err}"),
            Error::InvalidPixelData(m) => format!("invalid pixel data: {m}"),
            Error::Encode(m) => format!("cannot write the image file: {m}"),
            Error::Formula(m) => format!("formula: {m}"),
            Error::InvalidName(m) | Error::Video(m) | Error::Import(m) | Error::Effect(m) => {
                m.clone()
            }
        }
    }
}
