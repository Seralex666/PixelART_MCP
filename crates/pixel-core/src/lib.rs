//! # pixel-core
//!
//! Das gemeinsame Datenmodell von PixelART MCP. Editor und MCP-Server
//! arbeiten beide ausschließlich über diese Bibliothek, damit ein Bild,
//! das Claude zeichnet, exakt dasselbe ist, das du im Editor siehst.
//!
//! Grundprinzipien:
//! - **Echte Pixel:** Jede Ebene ist ein Raster aus `width × height` Pixeln.
//!   Es gibt keine Skalierung, kein Anti-Aliasing, keine Unterpixel.
//! - **Farbtiefe wird erzwungen:** Jede Farbe, die geschrieben wird, wird
//!   auf die Farbtiefe des Dokuments gebracht (8 Bit indiziert, 16 Bit
//!   RGB565 oder 32 Bit RGBA).
//! - **Konstanz:** Figuren sind [`Character`]-Assets mit gesperrter Palette
//!   und fester Größe. Animationen verweisen auf die Figur, statt sie zu
//!   kopieren, und [`Project::validate`] meldet jede Abweichung.

pub mod animation;
pub mod autotile;
pub mod brush;
pub mod character;
pub mod color;
pub mod contour;
pub mod draw;
pub mod effects;
pub mod error;
pub mod export;
pub mod expr;
pub mod fx;
pub mod import;
pub mod lang;
pub mod layer;
pub mod light;
pub mod lint;
pub mod normalmap;
pub mod palette;
pub mod pixelate;
pub mod project;
pub mod quant;
pub mod rotate;
pub mod shapes;
pub mod sprite;
pub mod stylebook;
pub mod template;
pub mod variants;
pub mod video;
pub mod workspace;

pub use animation::{Animation, AnimationKind};
pub use character::Character;
pub use color::{ColorDepth, Rgba};
pub use error::{Error, Result};
pub use layer::Layer;
pub use palette::Palette;
pub use project::{Project, Style, ValidationIssue};
pub use sprite::{Frame, Sprite};
pub use template::{CanvasTemplate, TEMPLATES};
pub use workspace::Workspace;

/// Größte erlaubte Kantenlänge eines Bildes in Pixeln.
pub const MAX_SIZE: u32 = 1024;
