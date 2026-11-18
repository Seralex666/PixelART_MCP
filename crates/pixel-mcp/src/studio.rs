//! Die eigentliche Logik hinter den MCP-Werkzeugen, ohne Protokoll-Details.
//!
//! Jede Operation lädt das Projekt aus dem Arbeitsordner, ändert es und
//! speichert es sofort wieder. So sieht ein geöffneter Editor jede Änderung,
//! und nach einem Absturz geht nichts verloren.

use pixel_core::autotile::{self, TileSetKind};
use pixel_core::brush;
use pixel_core::contour::{self, AutoOutline};
use pixel_core::draw::{self, Target};
use pixel_core::effects::{self, Area, Flow, Mask, Patch, Pattern, Selection};
use pixel_core::expr::Expr;
use pixel_core::fx::{self, AutoEffect, EffectKind};
use pixel_core::import::{self, Grid, NewColors, Picture, Shrink};
use pixel_core::light;
use pixel_core::lint::{self, Checks, Finding};
use pixel_core::normalmap::{self, NormalOptions};
use pixel_core::pixelate::{self, Background, Dither, Outline, Pixelate};
use pixel_core::project::{CharacterBundle, Style};
use pixel_core::quant;
use pixel_core::rotate::RotateOptions;
use pixel_core::shapes::{self, CustomShape, Shape};
use pixel_core::stylebook::{self, OutlineRule, StylePreset};
use pixel_core::variants::{self, ColorVariant};
use pixel_core::workspace::{
    self, ExportFormat, NameClash, ProjectLock, ProjectNameError, TileEngine, Workspace,
};
use pixel_core::{
    export, template, Animation, AnimationKind, Character, ColorDepth, Palette, Project, Rgba,
    Sprite, TEMPLATES,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

/// Ergebnis eines Werkzeugs: Text für das Modell und optional ein Vorschaubild (PNG).
#[derive(Debug, Default)]
pub struct Reply {
    pub text: String,
    pub image_png: Option<Vec<u8>>,
}

impl Reply {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            image_png: None,
        }
    }

    fn json(v: Value) -> Self {
        Self::text(serde_json::to_string_pretty(&v).unwrap_or_default())
    }
}

pub type Res<T = Reply> = Result<T, String>;

/// Fehler als Text für das Modell. Meldungen aus dem Kern kommen auf Englisch,
/// wie alles, was der Server sagt.
fn e(err: impl std::fmt::Display + 'static) -> String {
    match (&err as &dyn std::any::Any).downcast_ref::<pixel_core::Error>() {
        Some(core) => core.english(),
        None => err.to_string(),
    }
}

/// Größte Seitenlänge, bis zu der `get_pixels` eine Region als Text ausgibt.
const MAX_TEXT_REGION: u32 = 128;
/// Zeichen für die Textdarstellung von Pixeln. `.` steht immer für transparent.
const SYMBOLS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// Zielgröße der längeren Seite eines Vorschaubildes in Bildschirm-Pixeln.
/// Größte Vorschau in Bildpunkten. Breiter als hoch, weil Frames nebeneinander liegen.
const PREVIEW_WIDTH: u32 = 1024;
const PREVIEW_HEIGHT: u32 = 512;
const MAX_OPS: usize = 5000;
const MAX_FRAMES: usize = 256;
const MAX_LAYERS: usize = 64;
/// So viele `draw`-Aufrufe merkt sich der Server für `undo`.
const MAX_UNDO: usize = 20;
/// Speichergrenze dafür: große Bilder mit vielen Frames belegen sonst sehr viel.
const MAX_UNDO_BYTES: usize = 256 * 1024 * 1024;
/// Längste Notiz im Projekt-Stil, in Zeichen.
const MAX_STYLE_NOTES: usize = 500;

pub struct Studio {
    ws: Workspace,
    /// Die letzten `draw`-Aufrufe dieser Sitzung, der jüngste zuletzt.
    history: Mutex<Vec<Drawn>>,
}

/// Ein `draw`-Aufruf, der sich zurücknehmen lässt.
struct Drawn {
    project: String,
    asset: AssetRef,
    before: Sprite,
    after: Sprite,
}

/// Verweis auf ein Asset im Projekt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssetRef {
    Character(Uuid),
    Animation(Uuid),
    Image(Uuid),
}

impl AssetRef {
    fn kind(self) -> &'static str {
        match self {
            AssetRef::Character(_) => "character",
            AssetRef::Animation(_) => "animation",
            AssetRef::Image(_) => "image",
        }
    }
}

/// One drawing operation. The `op` field selects the kind.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DrawOp {
    /// Set individual pixels to one color.
    Pixels {
        color: String,
        /// List of [x, y] coordinates.
        points: Vec<[i64; 2]>,
    },
    /// 1-pixel-wide line without anti-aliasing.
    Line {
        color: String,
        from: [i64; 2],
        to: [i64; 2],
    },
    /// Rectangle with top-left corner (x, y) and size w × h pixels.
    Rect {
        color: String,
        x: i64,
        y: i64,
        w: u32,
        h: u32,
        #[serde(default)]
        filled: bool,
    },
    /// Ellipse that exactly fits the rectangle (x, y, w, h).
    Ellipse {
        color: String,
        x: i64,
        y: i64,
        w: u32,
        h: u32,
        #[serde(default)]
        filled: bool,
    },
    /// Flood fill the connected area of equal color starting at (x, y).
    Fill { color: String, x: i64, y: i64 },
    /// Paint a block of pixels from text rows. Each character is looked up in `legend`
    /// (character → color). `.` and space leave the pixel unchanged unless the legend
    /// defines them. This is the most precise way to draw sprites.
    Grid {
        /// Top-left corner where the first character of the first row is placed.
        #[serde(default)]
        x: i64,
        #[serde(default)]
        y: i64,
        /// One string per pixel row, top to bottom.
        rows: Vec<String>,
        /// Maps a single character to a color, e.g. {"a": "#1a1c2c", "_": "transparent"}.
        legend: HashMap<String, String>,
    },
    /// Replace every pixel of color `from` with color `to` on the layer.
    ReplaceColor { from: String, to: String },
    /// Move the layer content by (dx, dy) pixels. Content pushed outside is lost.
    Shift { dx: i64, dy: i64 },
    /// Mirror the layer horizontally.
    FlipH,
    /// Mirror the layer vertically.
    FlipV,
    /// Erase the whole layer.
    Clear,
    /// Outline around everything drawn on the layer, placed on the empty pixels next to it.
    /// Example: {"op": "outline", "color": "#1a1c2c"}
    Outline {
        /// Outline color. May be left out when the project style sets one (see set_style).
        color: Option<String>,
        /// Width in pixels, 1 to 16. Default 1.
        thickness: Option<u32>,
        /// Also count corner neighbors. Makes the outline solid on diagonals. Default false.
        #[serde(default)]
        diagonals: bool,
    },
    /// Automatic light and shadow on a filled shape: edges facing the light get the light
    /// end of `ramp`, edges facing away get the dark end. Default mask is "opaque"; pass a
    /// color as `mask` to shade one flat area only.
    /// Example: {"op": "shade", "light_dir": "top-left", "ramp": ["#333c57", "#566c86", "#94b0c2"], "mask": "#566c86"}
    Shade {
        /// Where the light comes from: top, bottom, left, right, top-left, top-right, bottom-left or bottom-right.
        /// Default: the light direction of the project style, else top-left.
        light_dir: Option<String>,
        /// Colors from darkest to lightest. The first half shades the edges away from the
        /// light (darkest at the rim), the second half lights the edges toward it (lightest
        /// at the rim). With an odd number of colors the middle one is the base color and
        /// fills the rest of the shape; with an even number the inside stays as it is.
        ramp: Vec<String>,
        #[serde(flatten)]
        area: Where,
    },
    /// Ordered dithering between two colors: only these two colors are used, their share
    /// changes across the area. Linear along `direction` or radial around `center`.
    /// Example: {"op": "dither", "from_color": "#b13e53", "to_color": "#ef7d57", "direction": "down", "pattern": "bayer4", "x": 8, "y": 8, "w": 16, "h": 16}
    Dither {
        from_color: String,
        to_color: String,
        /// Direction the transition runs toward: right, left, down (default), up or a diagonal like "down-right".
        direction: Option<String>,
        /// [x, y] of the center for a radial transition. Overrides `direction`.
        center: Option<[f64; 2]>,
        /// Radius of a radial transition. Default: up to the farthest corner of the area.
        radius: Option<f64>,
        /// "bayer2", "bayer4" (default), "bayer8" or "checker" (three levels: from, checkerboard, to).
        pattern: Option<String>,
        #[serde(flatten)]
        area: Where,
    },
    /// Gradient in hard steps, without smoothing. With `pattern` the steps are dithered into each other.
    /// Example: {"op": "gradient", "colors": ["#1a1c2c", "#5d275d", "#b13e53", "#ef7d57", "#ffcd75"], "center": [32, 32], "radius": 20}
    Gradient {
        /// Two or more colors, from start to end.
        colors: Vec<String>,
        /// Direction the gradient runs toward: right, left, down (default), up or a diagonal like "down-right".
        direction: Option<String>,
        /// [x, y] of the center for a radial gradient. Overrides `direction`.
        center: Option<[f64; 2]>,
        /// Radius of a radial gradient. Default: up to the farthest corner of the area.
        radius: Option<f64>,
        /// Number of bands. Default: one band per color. With more steps than colors the
        /// colors in between are mixed (and snapped to a locked palette).
        steps: Option<usize>,
        /// Dither the bands into each other: "bayer2", "bayer4", "bayer8" or "checker". Default: hard bands.
        pattern: Option<String>,
        #[serde(flatten)]
        area: Where,
    },
    /// Grain: scatter pixels in randomly chosen colors. The same `seed` always gives the same picture.
    /// Example: {"op": "noise", "colors": ["#333c57", "#94b0c2"], "density": 0.12, "seed": 7, "mask": "#566c86"}
    Noise {
        colors: Vec<String>,
        /// Share of pixels that are hit, 0 to 1.
        density: f64,
        #[serde(default)]
        seed: u64,
        #[serde(flatten)]
        area: Where,
    },
    /// Per-pixel formula. It is evaluated for every pixel of the area; the result, rounded
    /// down, picks a color from `colors` (0 = first). Results above the end take the last
    /// color, negative results leave the pixel unchanged.
    /// Variables: x, y, w, h (canvas size), frame, frames, t (= frame / frames), cx, cy (canvas center), pi.
    /// Operators: + - * / % ^, < <= > >= == !=, && || !, cond ? a : b.
    /// Functions: abs floor ceil round sqrt sin cos sign min max pow mod clamp(v, lo, hi) mix(a, b, t)
    /// step(edge, v) if(c, a, b), dist(px, py) and angle(px, py) (to a point; without arguments to the
    /// center), rand() / rand(k) (fixed random 0..1 per pixel), noise(scale) / noise(scale, k) (smooth
    /// random 0..1), bayer(4) (dither threshold 0..1). Math only: no files, no loops, no code.
    /// Example, a glow that pulses over 4 frames:
    /// {"op": "shader", "colors": ["#b13e53", "#ef7d57", "#ffcd75"], "formula": "dist(32, 40) > 9 + 2 * sin(t * 2 * pi) ? -1 : 2.9 - dist(32, 40) / 4 + (noise(3, frame) - 0.5)"}
    Shader {
        formula: String,
        colors: Vec<String>,
        #[serde(default)]
        seed: u64,
        #[serde(flatten)]
        area: Where,
    },
    /// Copy a piece of a picture and paste it at (x, y). Empty pixels of the piece do not
    /// overwrite. Good for repeated stones, teeth, windows.
    /// Example: {"op": "stamp", "src": [0, 0, 8, 8], "x": 16, "y": 0, "flip_h": true}
    Stamp {
        /// Where the top-left corner of the piece goes.
        x: i64,
        y: i64,
        /// Source area [x, y, w, h]. Default: the whole canvas of the source.
        src: Option<[i64; 4]>,
        /// Name or id of another asset to copy from. Default: the asset being drawn on.
        from_asset: Option<String>,
        /// Source frame. Default: the frame being drawn on (0 for another asset).
        from_frame: Option<usize>,
        /// Source layer (index or name), or "all" for the merged picture. Default: the layer
        /// being drawn on ("all" for another asset).
        from_layer: Option<Value>,
        #[serde(default)]
        flip_h: bool,
        #[serde(default)]
        flip_v: bool,
        /// Rotate clockwise by 0, 90, 180 or 270 degrees (after flipping).
        rotate: Option<u32>,
        /// Replace colors of the piece, e.g. {"#3b5dc9": "#b13e53"}.
        color_map: Option<HashMap<String, String>>,
    },
    /// A shape fitted into the rectangle (x, y, w, h): heart, diamond, star, triangle,
    /// cross, hexagon, arrow (pointing right), circle, or the name of a saved custom shape
    /// (see get_studio_info, custom_shapes). Filled or only its 1-pixel edge.
    /// Example: {"op": "shape", "shape": "heart", "color": "#b13e53", "x": 4, "y": 4, "w": 9, "h": 8, "filled": true}
    Shape {
        shape: String,
        /// Color of the shape. With `keep_colors` on a custom shape it may be left out.
        color: Option<String>,
        x: i64,
        y: i64,
        w: u32,
        h: u32,
        #[serde(default)]
        filled: bool,
        /// Custom shapes only: paint them in their own saved colors instead of `color`.
        #[serde(default)]
        keep_colors: bool,
        /// Mirror the shape left-right (e.g. an arrow pointing left).
        #[serde(default)]
        flip_h: bool,
        /// Mirror the shape top-bottom.
        #[serde(default)]
        flip_v: bool,
    },
    /// Spray can: scatter pixels randomly in a circle around (x, y). `density` is the share
    /// of pixels hit (0 to 1). Several puffs with different `seed` build up texture.
    /// Example: {"op": "spray", "color": "#94b0c2", "x": 20, "y": 12, "radius": 4, "density": 0.3, "seed": 2}
    Spray {
        color: String,
        x: i64,
        y: i64,
        /// Radius in pixels, 0 to 64. Default 3.
        radius: Option<u32>,
        /// Share of pixels hit, 0 to 1. Default 0.25.
        density: Option<f64>,
        #[serde(default)]
        seed: u64,
    },
    /// Make drawn pixels one step lighter, walking along the palette's color ramps (the
    /// most similar lighter color). Without a fitting palette color the color itself is
    /// lightened, except on locked palettes. Empty pixels stay empty.
    /// Example: {"op": "lighten", "x": 4, "y": 2, "w": 6, "h": 3, "mask": "#3b5dc9"}
    Lighten {
        /// How many steps, 1 to 8. Default 1.
        steps: Option<u32>,
        #[serde(flatten)]
        area: Where,
    },
    /// Make drawn pixels one step darker along the palette's color ramps. See lighten.
    Darken {
        /// How many steps, 1 to 8. Default 1.
        steps: Option<u32>,
        #[serde(flatten)]
        area: Where,
    },
    /// Cut the area [x, y, w, h] out of the layer and put it down moved by (dx, dy).
    /// Empty pixels of the piece leave what is below. Content pushed outside is lost.
    MoveArea { area: [i64; 4], dx: i64, dy: i64 },
    /// Mirror the area [x, y, w, h] in place, left-right and/or top-bottom.
    FlipArea {
        area: [i64; 4],
        #[serde(default)]
        horizontal: bool,
        #[serde(default)]
        vertical: bool,
    },
    /// Erase the area [x, y, w, h] of the layer.
    ClearArea { area: [i64; 4] },
}

/// Area an op works on. Without x, y, w, h it is the whole canvas.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct Where {
    /// Left edge of the area. Default 0.
    x: Option<i64>,
    /// Top edge of the area. Default 0.
    y: Option<i64>,
    /// Width of the area. Default: up to the right edge.
    w: Option<u32>,
    /// Height of the area. Default: up to the bottom edge.
    h: Option<u32>,
    /// Which pixels inside the area are touched: "all", "opaque" (only drawn pixels),
    /// "transparent" (only empty pixels) or a color such as "#566c86" (only pixels of
    /// exactly that color). Default "all" (for shade: "opaque").
    mask: Option<String>,
}

/// Per-frame part of a draw call.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PerFrame {
    /// Frame this entry is for. Omit to go by position: the first entry belongs to the
    /// first selected frame, the second to the second, and so on.
    frame: Option<usize>,
    /// Color substitutions for this frame: wherever the shared `ops` use a key color, this
    /// frame uses the value instead, e.g. {"#ef7d57": "#ffcd75"} for a brighter glow.
    colors: Option<HashMap<String, String>>,
    /// Extra ops for this frame only, applied after the shared `ops`, e.g. a shift.
    ops: Option<Vec<DrawOp>>,
}

/// Options for the preview image of `view`.
#[derive(Debug, Default)]
pub struct ViewOptions {
    pub grid: bool,
    pub zoom_region: Option<[i64; 4]>,
    pub tile: bool,
    pub background: Option<String>,
}

/// Angaben für [`Studio::import_image`].
#[derive(Default)]
pub struct ImportArgs<'a> {
    pub project: &'a str,
    pub path: &'a str,
    pub name: Option<&'a str>,
    /// "image", "character" oder "animation".
    pub kind: Option<&'a str>,
    pub character: Option<&'a str>,
    pub animation: Option<&'a str>,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    pub downscale: Option<u32>,
    pub depth_bits: Option<u8>,
    pub new_colors: Option<&'a str>,
    pub keep_empty_frames: bool,
    /// Ein Foto oder eine Zeichnung in Pixel-Art umwandeln.
    pub pixelate: Option<PixelateArgs<'a>>,
    /// Bei as="reference": das Bild, das die Vorlage bekommt.
    pub asset: Option<&'a str>,
    /// Bei as="reference": Deckkraft der Vorlage, 0 bis 255.
    pub opacity: Option<u8>,
}

/// Wie [`Studio::import_image`] ein Foto in Pixel-Art umwandelt. Leere Felder
/// nehmen Voreinstellungen.
#[derive(Default, Clone, Copy)]
pub struct PixelateArgs<'a> {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size: Option<u32>,
    pub colors: Option<usize>,
    pub palette: Option<&'a str>,
    pub dither: Option<&'a str>,
    pub background: Option<&'a str>,
    pub tolerance: Option<u8>,
    pub outline: Option<&'a str>,
    pub vivid: Option<bool>,
}

/// Was `set_light` ändern soll. Leere Felder behalten den bisherigen Wert.
#[derive(Default)]
pub struct LightArgs<'a> {
    pub project: &'a str,
    pub asset: &'a str,
    pub action: Option<&'a str>,
    pub from: Option<&'a Value>,
    pub to: Option<&'a Value>,
    pub path: Option<&'a [[f32; 2]]>,
    pub width: Option<u32>,
    pub strength: Option<u32>,
    pub color: Option<&'a str>,
    pub shadows: Option<bool>,
    pub cast_shadow: Option<bool>,
    pub cast_length: Option<u32>,
    pub cast_opacity: Option<u8>,
    pub cast_color: Option<&'a str>,
}

/// Was `set_outline` ändern soll. Leere Felder behalten den bisherigen Wert.
#[derive(Default)]
pub struct OutlineArgs<'a> {
    pub project: &'a str,
    pub asset: &'a str,
    pub action: Option<&'a str>,
    pub color: Option<&'a str>,
    pub inside: Option<bool>,
    pub corners: Option<bool>,
    pub thickness: Option<u32>,
    pub inner: Option<bool>,
    pub light_dir: Option<&'a str>,
    pub layer: Option<&'a str>,
    pub with_animations: Option<bool>,
}

/// Angaben für [`Studio::set_style`]. Was fehlt, bleibt, wie es ist.
#[derive(Default)]
pub struct StyleArgs<'a> {
    pub style: Option<&'a str>,
    pub palette_preset: Option<&'a str>,
    pub palette: Option<&'a [String]>,
    pub light_dir: Option<&'a str>,
    pub outline_color: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub clear: bool,
}

/// Options of `export` for a normal map.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct NormalMapOpts {
    /// How many pixels wide the rim curves (1 to 16). Default 3. Larger reads rounder.
    pub bevel: Option<u32>,
    /// How steep the curve looks, 0.1 to 5. Default 1.
    pub strength: Option<f32>,
    /// How much light and dark colors add fine detail, 0 to 1. Default 0.3.
    pub detail: Option<f32>,
    /// true: green flipped (DirectX convention). Default false: OpenGL convention as Godot and Unity expect.
    pub directx: Option<bool>,
}

/// Parameters of the `autotile` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct AutotileParams {
    /// Project name.
    pub project: String,
    /// "template" creates an empty template image to draw the few source tiles on, "build" (default) makes the whole tile set from a drawn template, "preview" shows a set on a small sample map.
    pub action: Option<String>,
    /// build: the template image (2 tiles wide, 3 tall). preview: the tile set image.
    pub asset: Option<String>,
    /// template: name of the new template image (default "Terrain"). build: name of the tile set image (default "<template> tiles"); an existing set of that name is rebuilt.
    pub name: Option<String>,
    /// template: tile size in pixels, even, 4 to 256 (default 16). The template is 2×tile wide and 3×tile high.
    pub tile: Option<u32>,
    /// build: "blob47" (default: 47 tiles, corners and sides, with inner corners; Godot "Match Corners and Sides", Tiled "mixed") or "sides16" (16 tiles, sides only; Godot "Match Sides", Tiled "edge").
    pub kind: Option<String>,
    /// template: color depth 8, 16 or 32 (default 32).
    pub color_depth_bits: Option<u8>,
    /// template: palette preset (see get_studio_info). Default: the project style's palette, else default16.
    pub palette_preset: Option<String>,
    /// preview: another number gives another sample map. Default 1.
    pub seed: Option<u64>,
}

/// Parameters of the `rotate` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct RotateParams {
    /// Project name.
    pub project: String,
    /// Name or id of an image, an animation or a character (its base pose).
    pub asset: String,
    /// Angle in degrees, clockwise. Any angle works; multiples of 90 are exact. With `spin` the total turn over all frames (360 = one full turn).
    pub angle: f32,
    /// Only this layer (index or name). Default: every drawn layer; guide layers and automatic layers (outline, light, effects) are skipped because they redraw themselves.
    pub layer: Option<Value>,
    /// Only this frame (0-based). Default: all frames.
    pub frame: Option<usize>,
    /// Pivot pixel [x, y]. Default: the center of the canvas.
    pub pivot: Option<[u32; 2]>,
    /// Images only: grow the canvas so that nothing is cut off. Characters and animations keep their size.
    pub expand: Option<bool>,
    /// Spread the turn over the frames: frame i turns by angle·i/frames, e.g. a spinning axe or coin with angle 360. Needs all frames.
    pub spin: Option<bool>,
    /// With spin: make sure the asset has at least this many frames (2 to 64) by repeating its frames. Not for a character's base pose.
    pub frames: Option<u32>,
}

/// Parameters of the `variants` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct VariantsParams {
    /// Project name.
    pub project: String,
    /// Name or id of a character, or one of its animations (the variants belong to the character).
    pub character: String,
    /// "list" (default: variants and the character's colors), "set" (create or change the variant `name`), "remove" or "preview" (the character and the first frame of each animation in the original colors and in every variant).
    pub action: Option<String>,
    /// Variant name, e.g. "red team", "ice", "gold".
    pub name: Option<String>,
    /// set: replacement colors as {"#from": "#to"}, e.g. {"#3b5dc9": "#b13e53"}. Pixels keep their own opacity. Map a color to itself to drop its replacement.
    pub colors: Option<BTreeMap<String, String>>,
    /// set: turn the hue of these colors by `hue` degrees, keeping lightness and saturation (quick team colors). Default with `hue`: every color of the character except grays.
    pub hue_colors: Option<Vec<String>>,
    /// set: hue turn in degrees for `hue_colors`, e.g. 120 or -90.
    pub hue: Option<f32>,
    /// set: start the variant from scratch instead of changing the existing one.
    pub replace: Option<bool>,
}

/// Parameters of the `effects` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct EffectsParams {
    /// Project name.
    pub project: String,
    /// Name or id of an image, an animation or a character (its base pose).
    pub asset: String,
    /// "add" (default: add an effect, or change the one on `layer`), "list" (the effects of the asset), "remove" (delete an effect with its layer) or "freeze" (keep its layer as a normal layer you can edit by hand, no longer redrawn).
    pub action: Option<String>,
    /// Effect type for "add": "glow", "lightning", "explosion", "reflection", "sparks", "smoke", "magic" or "color_cycle". Not needed to change an existing effect named by `layer`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Layer of the effect. For "add" a new name (default: the type, e.g. "glow", "glow 2") or the layer of an existing effect to change it. For "remove"/"freeze" the effect to remove (or give `type` when there is only one of it).
    pub layer: Option<String>,
    /// Make sure the asset has at least this many frames (2 to 64) by repeating its frames, so a still image can glow, spark or flow. Effects loop over all frames. Not for a character's base pose (use an animation).
    pub frames: Option<u32>,
    /// Main color: glow color, lightning core color, reflection water color. Default per type.
    pub color: Option<String>,
    /// Color list. glow: glow only around pixels of these colors (eyes, a blade, a lamp) instead of around the whole figure. explosion/sparks/smoke/magic: color ramp from start to end of life. color_cycle: the ramp that cycles, in order.
    pub colors: Option<Vec<String>>,
    /// color_cycle: a ready ramp "water", "fire" or "lava" instead of `colors`; paint with these colors (see the reply) and they flow.
    pub preset: Option<String>,
    /// glow: reach in pixels, 1 to 8 (default 3). explosion: radius of the fireball, 2 to 256 (default half the canvas).
    pub radius: Option<u32>,
    /// glow: the glow shrinks by this many pixels and grows back over the loop (0 = steady, default 1; smaller than radius).
    pub pulse: Option<u32>,
    /// glow: dithered instead of semi-transparent (always dithered at 8 and 16 bit). Default false.
    pub dither: Option<bool>,
    /// lightning: start point [x, y] in canvas pixels (default near the top).
    pub from: Option<[f32; 2]>,
    /// lightning: end point [x, y] (default near the bottom).
    pub to: Option<[f32; 2]>,
    /// explosion: center [x, y]. sparks/smoke/magic: where the particles come from [x, y]. Defaults: center / lower middle.
    pub at: Option<[f32; 2]>,
    /// lightning: color of the 1-pixel glow around the bolt, or "none". Default light blue.
    pub glow_color: Option<String>,
    /// lightning: side branches, 0 to 8 (default 2).
    pub branches: Option<u32>,
    /// lightning: off in some frames (default true).
    pub flicker: Option<bool>,
    /// explosion: first frame (default 0).
    pub start: Option<u32>,
    /// explosion: frames it lasts (default 0 = until the last frame).
    pub length: Option<u32>,
    /// reflection: first row of the water (default: right below the lowest pixel of the figure).
    pub line: Option<u32>,
    /// reflection: how strongly tinted towards `color`, 0 to 100 percent (default 35).
    pub mix: Option<u8>,
    /// reflection: opacity 0 to 255 (default 170).
    pub opacity: Option<u8>,
    /// reflection: how far waves shift the rows, 0 to 8 pixels (default 1).
    pub wave: Option<u32>,
    /// reflection on an image: make the canvas taller when the reflection does not fit (default true). Animations and characters keep their size.
    pub grow: Option<bool>,
    /// sparks/smoke/magic: half width of the source in pixels (magic: also half height).
    pub spread: Option<u32>,
    /// sparks/smoke/magic: number of particles, 1 to 300.
    pub count: Option<u32>,
    /// sparks/smoke/magic: how far particles travel, 0.1 to 5 (default 1). color_cycle: frames per step, 1 to 16 (default 1).
    pub speed: Option<f32>,
    /// color_cycle: cycle the other way (default false).
    pub reverse: Option<bool>,
    /// Random seed: another number gives other sparks, bolts and explosions. Default 0.
    pub seed: Option<u64>,
}

/// Parameters of the `styles` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct StylesParams {
    /// "list" (default): all styles with their descriptions. "get": everything about one style,
    /// including palette and rules. "save": create or change an own style. "delete": remove an own style.
    pub action: Option<String>,
    /// Style name, e.g. "Manga", "8 Bit", "Castles" or an own style. Needed for get, save and delete.
    pub name: Option<String>,
    /// save: start from a copy of this style (built-in or own), then apply the other fields.
    pub based_on: Option<String>,
    /// save: take palette, light direction, outline color and notes from this project's style.
    pub from_project: Option<String>,
    /// save: what the style is for and how it looks (up to 500 characters).
    pub description: Option<String>,
    /// save: the style's colors, e.g. ["#1a1c2c", "#5d275d"].
    pub palette: Option<Vec<String>>,
    /// save: a palette preset from get_studio_info instead of `palette`.
    pub palette_preset: Option<String>,
    /// save: color depth for new images, 8, 16 or 32.
    pub color_depth_bits: Option<u8>,
    /// save: recommended sizes as [width, height], the usual one first.
    pub sizes: Option<Vec<[u32; 2]>>,
    /// save: at most this many colors per sprite; 0 removes the limit.
    pub colors_per_sprite: Option<u32>,
    /// save: "none", "black", "dark" (darkest palette color) or "selective".
    pub outline: Option<String>,
    /// save: outline width 1 to 4.
    pub outline_width: Option<u8>,
    /// save: shades per color from shadow to light, 1 to 8.
    pub shades: Option<u8>,
    /// save: whether dithering belongs to the style.
    pub dithering: Option<bool>,
    /// save: whether shadows shift to cooler and lights to warmer hues.
    pub hue_shift: Option<bool>,
    /// save: whether hand-placed anti-aliasing is allowed.
    pub anti_alias: Option<bool>,
    /// save: where the light comes from, e.g. "top-left"; "none" removes it.
    pub light_dir: Option<String>,
    /// save: rules in words for everyone drawing in this style (replaces the old ones).
    pub rules: Option<Vec<String>>,
}

impl Studio {
    pub fn new(home: PathBuf) -> Res<Self> {
        // Alles, was der Server sagt, ist Englisch, auch Meldungen aus dem Kern.
        pixel_core::lang::set(pixel_core::lang::Lang::En);
        let ws = Workspace::open(&home)
            .map_err(|err| format!("cannot create workspace {}: {err}", home.display()))?;
        Ok(Self {
            ws,
            history: Mutex::new(Vec::new()),
        })
    }

    pub fn home(&self) -> &Path {
        self.ws.home()
    }

    // ---------------------------------------------------------------- Dateien

    fn project_path(&self, name: &str) -> Res<PathBuf> {
        let name = name.trim();
        match workspace::check_project_name(name) {
            Ok(()) => self.ws.project_path(name).map_err(e),
            Err(ProjectNameError::Invalid) => Err(format!(
                "invalid project name '{name}': use 1 to 64 letters, digits, spaces, '-' or '_'"
            )),
            Err(ProjectNameError::Reserved) => Err(format!(
                "invalid project name '{name}': this name is reserved by Windows"
            )),
        }
    }

    fn load(&self, name: &str) -> Res<Project> {
        let path = self.project_path(name)?;
        if !path.exists() {
            return Err(format!(
                "project '{name}' does not exist. Existing projects: {}. Use create_project to make a new one.",
                self.ws.project_names().join(", ")
            ));
        }
        Project::load(&path).map_err(e)
    }

    /// Lädt ein Projekt, um es zu ändern. Die Sperre hält den Editor und andere
    /// Server davon ab, zwischen Laden und Speichern dazwischenzuschreiben.
    fn load_locked(&self, name: &str) -> Res<(ProjectLock, Project)> {
        self.project_path(name)?;
        let lock = self.ws.lock(name).map_err(e)?;
        Ok((lock, self.load(name)?))
    }

    /// Speichert. Automatische Konturen werden vorher auf den neuen Stand gebracht.
    fn save(&self, name: &str, p: &mut Project) -> Res<()> {
        p.refresh_outlines().map_err(e)?;
        p.save(&self.project_path(name)?).map_err(e)
    }

    // ---------------------------------------------------------------- Infos

    pub fn studio_info(&self) -> Res {
        Ok(Reply::json(json!({
            "workspace": self.ws.home(),
            "projects": self.ws.project_names(),
            "max_size": pixel_core::MAX_SIZE,
            "color_depths": {
                "8": "indexed: only palette colors (max 256) plus transparency",
                "16": "RGB565 high color: 65536 colors, pixels are opaque or transparent",
                "32": "RGBA: full color with alpha",
            },
            "templates": TEMPLATES.iter().map(|t| json!({
                "id": t.id,
                "width": t.width,
                "height": t.height,
                "purpose": t.purpose_en,
            })).collect::<Vec<_>>(),
            "palette_presets": {
                "default16": "16 hand-picked colors",
                "ramps32": "32 colors: 7 hue ramps with 4 shades (shadows cooler, lights warmer) plus 4 grays",
                "ramps64": "64 colors: 12 hue ramps with 5 shades plus 4 grays",
                "gray4": "4 grays",
                "gray8": "8 grays",
                "gray16": "16 grays",
            },
            "styles": self.ws.styles().iter().map(|s| json!({
                "name": s.name,
                "description": s.description,
            })).collect::<Vec<_>>(),
            "effects": fx::EffectKind::IDS,
            "color_cycle_presets": fx::CYCLE_PRESETS,
            "shapes": Shape::ALL.iter().map(|s| s.id()).collect::<Vec<_>>(),
            "custom_shapes": self.ws.custom_shapes().iter().map(|s| json!({
                "name": s.name,
                "width": s.width,
                "height": s.height,
            })).collect::<Vec<_>>(),
            "animation_presets": AnimationKind::PRESETS.iter().map(|k| json!({
                "kind": k.name(),
                "frames": k.default_frames(),
                "frame_ms": k.default_frame_ms(),
                "loops": k.loops_by_default(),
                "guide": k.guide(),
            })).collect::<Vec<_>>(),
        })))
    }

    pub fn create_project(&self, name: &str) -> Res {
        let path = self.project_path(name)?;
        let _lock = self.ws.lock(name).map_err(e)?;
        if path.exists() {
            return Err(format!("project '{name}' already exists"));
        }
        let mut p = Project::new(name.trim());
        self.save(name, &mut p)?;
        Ok(Reply::text(format!(
            "Created project '{}' at {}",
            name.trim(),
            path.display()
        )))
    }

    pub fn delete_project(&self, name: &str, confirm: &str, with_exports: bool) -> Res {
        if confirm.trim() != name.trim() {
            return Err(format!(
                "`confirm` must repeat the project name exactly ('{}'); nothing was deleted",
                name.trim()
            ));
        }
        let removed = self.ws.delete(name, with_exports).map_err(e)?;
        let exports = if removed {
            " Its exported files were deleted too."
        } else if self.ws.exports_dir(name).is_dir() {
            " Its exported files were kept."
        } else {
            ""
        };
        Ok(Reply::text(format!(
            "Deleted project '{}'.{exports}",
            name.trim()
        )))
    }

    pub fn get_project(&self, name: &str) -> Res {
        let p = self.load(name)?;
        let sprite_info = |s: &Sprite| {
            json!({
                "size": [s.width(), s.height()],
                "color_depth_bits": s.depth.bits(),
                "layers": s.layer_names(),
                "frames": s.frames().len(),
                "palette_colors": s.palette.len(),
            })
        };
        // Hinweise auf typische Versehen, nur wo es welche gibt.
        let outline = p.style.outline;
        let with_hints = |mut v: Value, s: &Sprite, seams: bool| {
            let hints = hint_texts(s, &lint::check(s, Checks { outline, seams }));
            if !hints.is_empty() {
                v["hints"] = json!(hints);
            }
            if let Some(o) = s.auto_outline() {
                v["auto_outline"] = outline_json(o);
            }
            if let Some(l) = s.auto_light() {
                v["auto_light"] = light_json(l);
            }
            if !s.effects().is_empty() {
                v["effects"] = json!(s.effects());
            }
            let refs: Vec<usize> = (0..s.layer_count())
                .filter(|&i| s.is_reference(i))
                .collect();
            if !refs.is_empty() {
                v["reference_layers"] = json!(refs);
            }
            v
        };
        let characters: Vec<Value> = p
            .characters
            .iter()
            .map(|c| {
                let v = json!({
                    "name": c.name,
                    "id": c.id,
                    "description": c.description,
                    "anchor": [c.anchor.0, c.anchor.1],
                    "base": sprite_info(&c.base),
                    "palette": c.palette().colors,
                    "animations": p.animations_of(c.id).map(|a| with_hints(json!({
                        "name": a.name,
                        "id": a.id,
                        "kind": a.kind.name(),
                        "frames": a.sprite.frames().len(),
                        "frame_ms": a.sprite.frames().iter().map(|f| f.duration_ms).collect::<Vec<_>>(),
                        "looping": a.looping,
                        "layers": a.sprite.layer_names(),
                    }), &a.sprite, false)).collect::<Vec<_>>(),
                });
                with_hints(v, &c.base, false)
            })
            .collect();
        let images: Vec<Value> = p
            .images
            .iter()
            .map(|i| {
                let mut v = sprite_info(&i.sprite);
                v["name"] = json!(i.name);
                v["id"] = json!(i.id);
                with_hints(v, &i.sprite, true)
            })
            .collect();
        let mut overview = json!({
            "project": p.name,
            "characters": characters,
            "images": images,
            "consistency_issues": p.validate(),
        });
        if !p.style.is_empty() {
            overview["style"] = style_json(&p.style);
            if let Some(st) = self.project_preset(&p) {
                overview["style_guide"] = preset_json(&st);
            }
        }
        Ok(Reply::json(overview))
    }

    /// Sucht einen Stil der Bibliothek, mit einer hilfreichen Meldung, wenn es ihn nicht gibt.
    fn find_style(&self, name: &str) -> Res<StylePreset> {
        self.ws.style(name).ok_or_else(|| {
            let names: Vec<String> = self.ws.styles().into_iter().map(|s| s.name).collect();
            format!("unknown style '{name}'. Styles: {}", names.join(", "))
        })
    }

    /// Der Stil der Bibliothek, auf dem der Projekt-Stil beruht.
    fn project_preset(&self, p: &Project) -> Option<StylePreset> {
        p.style.preset.as_deref().and_then(|n| self.ws.style(n))
    }

    /// Die Stil-Bibliothek: auflisten, lesen, eigene speichern und löschen.
    pub fn styles(&self, a: &StylesParams) -> Res {
        let action = a
            .action
            .as_deref()
            .unwrap_or("list")
            .trim()
            .to_ascii_lowercase();
        let name = || {
            a.name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .ok_or_else(|| format!("`{action}` needs `name`"))
        };
        match action.as_str() {
            "list" => Ok(Reply::json(json!({
                "styles": self.ws.styles().iter().map(|s| json!({
                    "name": s.name,
                    "built_in": s.builtin,
                    "description": s.description,
                })).collect::<Vec<_>>(),
                "note": "Use styles(action: \"get\") for the palette and rules of one style. set_style(style) gives a whole project a style, create_image and create_character take `style` too.",
            }))),
            "get" => Ok(Reply::json(preset_json(&self.find_style(name()?)?))),
            "delete" => {
                let n = name()?;
                if stylebook::find(&stylebook::builtin(), n).is_some() {
                    return Err(format!("'{n}' is a built-in style and cannot be deleted"));
                }
                if self.ws.delete_style(n).map_err(e)? {
                    Ok(Reply::text(format!("Deleted the style '{n}'.")))
                } else {
                    Err(format!("there is no own style '{n}'"))
                }
            }
            "save" => {
                let n = name()?;
                let mut st = match (&a.based_on, &a.from_project) {
                    (Some(_), Some(_)) => {
                        return Err("give `based_on` or `from_project`, not both".into())
                    }
                    (Some(base), None) => {
                        let mut st = self.find_style(base)?;
                        st.name = n.to_string();
                        st.builtin = false;
                        st.id = "";
                        st
                    }
                    (None, Some(project)) => {
                        let p = self.load(project)?;
                        StylePreset::from_project(n, &p.style)
                    }
                    (None, None) => self
                        .ws
                        .style(n)
                        .filter(|s| !s.builtin)
                        .unwrap_or_else(|| StylePreset::new(n)),
                };
                st.name = n.to_string();
                if let Some(d) = &a.description {
                    st.description = d.trim().to_string();
                }
                if a.palette.is_some() || a.palette_preset.is_some() {
                    st.palette = Self::palette(
                        a.palette_preset.as_deref(),
                        a.palette.as_deref(),
                        ColorDepth::Rgba32,
                    )?
                    .colors;
                }
                if let Some(bits) = a.color_depth_bits {
                    st.color_depth = bits;
                }
                if let Some(sizes) = &a.sizes {
                    st.sizes = sizes.clone();
                }
                if let Some(n) = a.colors_per_sprite {
                    st.colors_per_sprite = (n > 0).then_some(n);
                }
                if let Some(o) = &a.outline {
                    st.outline = OutlineRule::parse(o).ok_or_else(|| {
                        format!("`outline` must be none, black, dark or selective, got '{o}'")
                    })?;
                }
                if let Some(w) = a.outline_width {
                    st.outline_width = w;
                }
                if let Some(n) = a.shades {
                    st.shades = n;
                }
                if let Some(b) = a.dithering {
                    st.dithering = b;
                }
                if let Some(b) = a.hue_shift {
                    st.hue_shift = b;
                }
                if let Some(b) = a.anti_alias {
                    st.anti_alias = b;
                }
                if let Some(dir) = &a.light_dir {
                    st.light_dir = if dir.trim().eq_ignore_ascii_case("none") {
                        None
                    } else {
                        Some(direction_name(parse_direction(dir, "light_dir")?).to_string())
                    };
                }
                if let Some(rules) = &a.rules {
                    st.rules = rules
                        .iter()
                        .map(|r| r.trim().to_string())
                        .filter(|r| !r.is_empty())
                        .collect();
                }
                let path = self.ws.save_style(&st).map_err(e)?;
                let mut v = preset_json(&st);
                v["saved_to"] = json!(path.display().to_string());
                Ok(Reply::json(v))
            }
            other => Err(format!(
                "unknown action '{other}', use list, get, save or delete"
            )),
        }
    }

    /// Legt den Stil des Projekts fest: Palette, Lichtrichtung, Umrissfarbe, Notizen.
    pub fn set_style(&self, project: &str, a: &StyleArgs) -> Res {
        let nothing = a.style.is_none()
            && a.palette_preset.is_none()
            && a.palette.is_none()
            && a.light_dir.is_none()
            && a.outline_color.is_none()
            && a.notes.is_none();
        if nothing && !a.clear {
            return Err(
                "give at least one of `style`, `palette`, `palette_preset`, `light_dir`, `outline_color`, `notes` or `clear`"
                    .into(),
            );
        }
        let (_lock, mut p) = self.load_locked(project)?;
        if a.clear {
            p.style = Style::default();
        }
        let preset = a.style.map(|name| self.find_style(name)).transpose()?;
        if let Some(st) = &preset {
            st.apply_to(&mut p.style);
        }
        if a.palette_preset.is_some() || a.palette.is_some() {
            p.style.palette =
                Self::palette(a.palette_preset, a.palette, ColorDepth::Rgba32)?.colors;
        }
        let unset = |text: &str| matches!(text.trim().to_ascii_lowercase().as_str(), "" | "none");
        if let Some(dir) = a.light_dir {
            p.style.light_dir = if unset(dir) {
                None
            } else {
                Some(direction_name(parse_direction(dir, "light_dir")?).to_string())
            };
        }
        if let Some(color) = a.outline_color {
            p.style.outline = if unset(color) {
                None
            } else {
                Some(parse_color(color)?).filter(|c| !c.is_transparent())
            };
        }
        if let Some(notes) = a.notes {
            let notes = notes.trim();
            if notes.chars().count() > MAX_STYLE_NOTES {
                return Err(format!(
                    "`notes` may have at most {MAX_STYLE_NOTES} characters"
                ));
            }
            p.style.notes = notes.to_string();
        }
        self.save(project, &mut p)?;
        if p.style.is_empty() {
            return Ok(Reply::text(format!(
                "Project '{}' has no style now. New images start with the default palette again.",
                p.name
            )));
        }
        let mut v = style_json(&p.style);
        if let Some(st) = self.project_preset(&p) {
            v["style_guide"] = preset_json(&st);
        }
        v["note"] = json!("Used from now on: create_image and create_character start with this palette unless they name one, `shade` takes this light direction unless it gives `light_dir`, `outline` takes this color unless it gives `color`, and the outline hints of view and get_project check against it. Existing assets are not changed.");
        Ok(Reply::json(v))
    }

    /// Die Palette für ein neues Asset: die genannte, sonst die des Projekt-Stils,
    /// sonst die Standardpalette. `true`, wenn sie aus dem Stil kommt.
    /// Der Stil für ein neues Asset: der genannte, sonst der des Projekts.
    fn new_asset_style(&self, p: &Project, style: Option<&str>) -> Res<Option<StylePreset>> {
        match style {
            Some(name) => self.find_style(name).map(Some),
            None => Ok(self.project_preset(p)),
        }
    }

    /// Größe eines neuen Assets; ohne Angabe die übliche Größe des Stils.
    fn size_for_new(
        template_id: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        style: Option<&StylePreset>,
    ) -> Res<(u32, u32)> {
        if template_id.is_none() && width.is_none() && height.is_none() {
            if let Some([w, h]) = style.and_then(|s| s.sizes.first().copied()) {
                return Ok((w, h));
            }
        }
        Self::size(template_id, width, height)
    }

    /// Wie [`Self::palette_for_new`], aber ein ausdrücklich genannter Stil bringt seine Palette mit.
    fn palette_for_new_styled(
        p: &Project,
        preset: Option<&str>,
        colors: Option<&[String]>,
        depth: ColorDepth,
        style: Option<&StylePreset>,
    ) -> Res<(Palette, bool)> {
        if let (None, None, Some(st)) = (preset, colors, style) {
            let mut pal = Palette::new(st.name.clone(), st.palette.clone());
            pal.conform_to(depth);
            return Ok((pal, false));
        }
        Self::palette_for_new(p, preset, colors, depth)
    }

    fn palette_for_new(
        p: &Project,
        preset: Option<&str>,
        colors: Option<&[String]>,
        depth: ColorDepth,
    ) -> Res<(Palette, bool)> {
        if preset.is_none() && colors.is_none() && !p.style.palette.is_empty() {
            let mut pal = Palette::new("Projekt-Stil", p.style.palette.clone());
            pal.conform_to(depth);
            return Ok((pal, true));
        }
        Ok((Self::palette(preset, colors, depth)?, false))
    }

    // ---------------------------------------------------------------- Assets

    fn resolve(p: &Project, asset: &str) -> Res<AssetRef> {
        let key = asset.trim();
        let mut hits = Vec::new();
        let is = |name: &str, id: Uuid| name.eq_ignore_ascii_case(key) || id.to_string() == key;
        hits.extend(
            p.characters
                .iter()
                .filter(|c| is(&c.name, c.id))
                .map(|c| AssetRef::Character(c.id)),
        );
        hits.extend(
            p.animations
                .iter()
                .filter(|a| is(&a.name, a.id))
                .map(|a| AssetRef::Animation(a.id)),
        );
        hits.extend(
            p.images
                .iter()
                .filter(|i| is(&i.name, i.id))
                .map(|i| AssetRef::Image(i.id)),
        );
        match hits.len() {
            1 => Ok(hits[0]),
            0 => Err(format!(
                "no asset named '{key}'. Available: {}",
                Self::asset_names(p).join(", ")
            )),
            _ => Err(format!("'{key}' is ambiguous, use the asset id instead")),
        }
    }

    fn asset_names(p: &Project) -> Vec<String> {
        p.characters
            .iter()
            .map(|c| c.name.clone())
            .chain(p.animations.iter().map(|a| a.name.clone()))
            .chain(p.images.iter().map(|i| i.name.clone()))
            .collect()
    }

    fn check_new_name(p: &Project, name: &str) -> Res<String> {
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return Err("asset name must have 1 to 64 characters".into());
        }
        Self::check_name_free(&Self::asset_names(p), name)?;
        Ok(name.to_string())
    }

    /// Ein Name ist frei, wenn kein vorhandenes Asset gleich heißt und auch keines
    /// denselben Dateinamen beim Export ergäbe (z. B. "Tile A" und "Tile_A").
    fn check_name_free(taken: &[String], name: &str) -> Res<()> {
        match workspace::check_asset_name(taken, name) {
            Ok(()) => Ok(()),
            Err(NameClash::Same(t)) => {
                Err(format!("an asset named '{t}' already exists in this project"))
            }
            Err(NameClash::Similar(t)) => Err(format!(
                "the name '{name}' is too similar to the existing asset '{t}': both would export to the same file name"
            )),
        }
    }

    fn sprite(p: &Project, r: AssetRef) -> Res<&Sprite> {
        Ok(match r {
            AssetRef::Character(id) => &p.character(id).map_err(e)?.base,
            AssetRef::Animation(id) => &p.animation(id).map_err(e)?.sprite,
            AssetRef::Image(id) => &p.image(id).map_err(e)?.sprite,
        })
    }

    fn sprite_mut(p: &mut Project, r: AssetRef) -> Res<&mut Sprite> {
        Ok(match r {
            AssetRef::Character(id) => &mut p.character_mut(id).map_err(e)?.base,
            AssetRef::Animation(id) => &mut p.animation_mut(id).map_err(e)?.sprite,
            AssetRef::Image(id) => &mut p.image_mut(id).map_err(e)?.sprite,
        })
    }

    fn asset_name(p: &Project, r: AssetRef) -> String {
        match r {
            AssetRef::Character(id) => p.character(id).map(|c| c.name.clone()),
            AssetRef::Animation(id) => p.animation(id).map(|a| a.name.clone()),
            AssetRef::Image(id) => p.image(id).map(|i| i.name.clone()),
        }
        .unwrap_or_default()
    }

    fn size(template_id: Option<&str>, width: Option<u32>, height: Option<u32>) -> Res<(u32, u32)> {
        match (template_id, width, height) {
            (Some(t), None, None) => {
                template::find(t)
                    .map(|t| (t.width, t.height))
                    .ok_or_else(|| {
                        format!("unknown template '{t}'. See get_studio_info for the list.")
                    })
            }
            (None, Some(w), Some(h)) => Ok((w, h)),
            _ => Err("give either `template` or both `width` and `height`".into()),
        }
    }

    fn depth(bits: Option<u8>) -> Res<ColorDepth> {
        let bits = bits.unwrap_or(32);
        ColorDepth::from_bits(bits)
            .ok_or_else(|| format!("color_depth_bits must be 8, 16 or 32, got {bits}"))
    }

    fn palette(preset: Option<&str>, colors: Option<&[String]>, depth: ColorDepth) -> Res<Palette> {
        let mut pal = match (preset, colors) {
            (Some(_), Some(_)) => {
                return Err("give either `palette_preset` or `palette`, not both".into())
            }
            (None, Some(colors)) => {
                if colors.len() > Palette::MAX_COLORS {
                    return Err(format!(
                        "a palette holds at most {} colors",
                        Palette::MAX_COLORS
                    ));
                }
                let mut pal = Palette::new("Eigene Palette", Vec::new());
                for c in colors {
                    let c = parse_color(c)?;
                    if !c.is_transparent() {
                        pal.add(c).map_err(e)?;
                    }
                }
                pal
            }
            (Some(name), None) => match name.to_ascii_lowercase().as_str() {
                "default16" => Palette::default_16(),
                "gray4" => Palette::grayscale(4),
                "gray8" => Palette::grayscale(8),
                "gray16" => Palette::grayscale(16),
                "ramps32" => Palette::ramps_32(),
                "ramps64" => Palette::ramps_64(),
                other => return Err(format!("unknown palette preset '{other}'")),
            },
            (None, None) => Palette::default_16(),
        };
        // Palettenfarben selbst an die Farbtiefe anpassen, damit sie exakt zeichenbar sind.
        pal.conform_to(depth);
        Ok(pal)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_image(
        &self,
        project: &str,
        name: &str,
        template_id: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        depth_bits: Option<u8>,
        palette_preset: Option<&str>,
        palette: Option<&[String]>,
        style: Option<&str>,
        layers: Option<&[String]>,
        frames: Option<usize>,
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let name = Self::check_new_name(&p, name)?;
        let st = self.new_asset_style(&p, style)?;
        let (w, h) = Self::size_for_new(template_id, width, height, st.as_ref())?;
        let depth = Self::depth(depth_bits.or(st.as_ref().map(|s| s.color_depth)))?;
        let (pal, from_style) = Self::palette_for_new_styled(
            &p,
            palette_preset,
            palette,
            depth,
            style.and(st.as_ref()),
        )?;
        let mut style_note = if from_style {
            format!(" Palette: project style ({} colors).", pal.len())
        } else {
            String::new()
        };
        if let Some(st) = &st {
            style_note.push_str(&format!(
                "\n\nStyle '{}' – follow it while drawing:\n{}",
                st.name,
                serde_json::to_string_pretty(&preset_json(st)).unwrap_or_default()
            ));
        }
        let mut sprite = Sprite::new(w, h, depth, pal).map_err(e)?;
        Self::name_layers(&mut sprite, layers)?;
        let frames = frames.unwrap_or(1);
        if !(1..=MAX_FRAMES).contains(&frames) {
            return Err(format!("`frames` must be 1 to {MAX_FRAMES}, got {frames}"));
        }
        for _ in 1..frames {
            sprite.add_frame();
        }
        let layer_names = sprite.layer_names().join(", ");
        let id = p.add_image(name.clone(), sprite);
        self.save(project, &mut p)?;
        Ok(Reply::text(format!(
            "Created image '{name}' ({w}×{h}, {} bit), id {id}. Layers bottom to top: {layer_names}. Frames: {frames}, all empty.{style_note}",
            depth.bits()
        )))
    }

    /// Benennt die erste Ebene und legt die weiteren an, von unten nach oben.
    fn name_layers(sprite: &mut Sprite, layers: Option<&[String]>) -> Res<()> {
        let Some(layers) = layers.filter(|l| !l.is_empty()) else {
            return Ok(());
        };
        if layers.len() > MAX_LAYERS {
            return Err(format!("at most {MAX_LAYERS} layers"));
        }
        for (i, name) in layers.iter().enumerate() {
            if name.trim().is_empty() {
                return Err("layer names must not be empty".into());
            }
            if layers[..i]
                .iter()
                .any(|other| other.eq_ignore_ascii_case(name))
            {
                return Err(format!("layer name '{name}' is given twice"));
            }
        }
        sprite.rename_layer(0, layers[0].clone()).map_err(e)?;
        for l in &layers[1..] {
            sprite.add_layer(l.clone());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_character(
        &self,
        project: &str,
        name: &str,
        description: Option<&str>,
        template_id: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        depth_bits: Option<u8>,
        palette_preset: Option<&str>,
        palette: Option<&[String]>,
        style: Option<&str>,
        layers: Option<&[String]>,
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let name = Self::check_new_name(&p, name)?;
        let st = self.new_asset_style(&p, style)?;
        let (w, h) = Self::size_for_new(template_id, width, height, st.as_ref())?;
        let depth = Self::depth(depth_bits.or(st.as_ref().map(|s| s.color_depth)))?;
        let (pal, from_style) = Self::palette_for_new_styled(
            &p,
            palette_preset,
            palette,
            depth,
            style.and(st.as_ref()),
        )?;
        if pal.is_empty() {
            return Err("a character needs a palette with at least one color".into());
        }
        let mut ch = Character::new(name.clone(), w, h, depth, pal).map_err(e)?;
        ch.description = description.unwrap_or_default().to_string();
        Self::name_layers(&mut ch.base, layers)?;
        let mut info = json!({
            "created": name,
            "id": ch.id,
            "size": [w, h],
            "color_depth_bits": depth.bits(),
            "layers": ch.base.layer_names(),
            "palette": ch.palette().colors,
            "note": "The palette is now locked: every color you draw on this character or its animations snaps to the nearest palette color. Use add_character_colors to extend it. Draw the base pose next, then create animations from it.",
        });
        if from_style {
            info["palette_source"] = json!("project style");
        }
        if let Some(st) = &st {
            info["style_guide"] = preset_json(st);
            info["style_note"] = json!(format!(
                "Draw this character in the style '{}': follow its rules, shades and outline.",
                st.name
            ));
        }
        p.add_character(ch);
        self.save(project, &mut p)?;
        Ok(Reply::json(info))
    }

    pub fn create_animation(
        &self,
        project: &str,
        character: &str,
        kind: &str,
        frames: Option<usize>,
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let AssetRef::Character(cid) = Self::resolve(&p, character)? else {
            return Err(format!("'{character}' is not a character"));
        };
        let kind = AnimationKind::parse(kind.trim());
        if frames.is_some_and(|n| n == 0 || n > MAX_FRAMES) {
            return Err(format!("frames must be between 1 and {MAX_FRAMES}"));
        }
        let anim = Animation::from_character(p.character(cid).map_err(e)?, kind.clone(), frames)
            .map_err(e)?;
        let name = Self::check_new_name(&p, &anim.name)?;
        let info = json!({
            "created": name,
            "id": anim.id,
            "kind": kind.name(),
            "frames": anim.sprite.frames().len(),
            "frame_ms": kind.default_frame_ms(),
            "looping": anim.looping,
            "layers": anim.sprite.layer_names(),
            "guide": kind.guide(),
            "note": "Every frame starts as an exact copy of the character's base pose. Change only what moves, frame by frame, e.g. with the shift op on single layers or small grid patches.",
        });
        p.animations.push(anim);
        self.save(project, &mut p)?;
        Ok(Reply::json(info))
    }

    /// Die Einstellungen einer Umwandlung aus den Angaben für `import_image`.
    fn pixelate_options(
        p: &Project,
        a: &PixelateArgs,
        picture: &Picture,
        hero: Option<&Character>,
    ) -> Res<Pixelate> {
        let mut o = Pixelate::for_source(picture.width, picture.height);
        let ratio = |v: u32, num: u32, den: u32| {
            ((f64::from(v) * f64::from(num) / f64::from(den.max(1))).round() as u32).max(1)
        };
        (o.width, o.height) = match (a.width, a.height, hero) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, _) => (w, ratio(w, picture.height, picture.width)),
            (None, Some(h), _) => (ratio(h, picture.width, picture.height), h),
            (None, None, Some(c)) => c.size(),
            (None, None, None) => {
                pixelate::fit_size(picture.width, picture.height, a.size.unwrap_or(64))
            }
        };
        if o.width > pixel_core::MAX_SIZE || o.height > pixel_core::MAX_SIZE {
            return Err(format!(
                "the target size {}×{} is larger than {max}×{max}",
                o.width,
                o.height,
                max = pixel_core::MAX_SIZE
            ));
        }
        if let Some(n) = a.colors {
            o.colors = n;
        }
        o.palette = match a.palette.map(str::trim) {
            None if hero.is_some() => hero.map(|c| c.palette().colors.clone()),
            None | Some("") => None,
            Some(k) if k.eq_ignore_ascii_case("image") || k.eq_ignore_ascii_case("own") => None,
            Some(k) if k.eq_ignore_ascii_case("project") || k.eq_ignore_ascii_case("style") => {
                if p.style.palette.is_empty() {
                    return Err("the project style has no palette; use set_style first or palette \"image\"".into());
                }
                Some(p.style.palette.clone())
            }
            Some(k) => Some(
                Self::sprite(p, Self::resolve(p, k)?)?
                    .palette
                    .colors
                    .clone(),
            ),
        };
        if let Some(d) = a.dither {
            o.dither = Dither::parse(d)
                .ok_or_else(|| format!("unknown dither '{d}', use none, ordered or diffusion"))?;
        }
        if let Some(b) = a.background.map(str::trim) {
            o.background = match b.to_ascii_lowercase().as_str() {
                "keep" | "none" | "" => Background::Keep,
                "auto" | "remove" => Background::Auto,
                _ => Background::Color(parse_color(b).map_err(|_| {
                    format!("invalid background '{b}': use keep, auto or a color like #ffffff")
                })?),
            };
        }
        if let Some(t) = a.tolerance {
            o.tolerance = t;
        }
        if let Some(l) = a.outline.map(str::trim) {
            o.outline = match l.to_ascii_lowercase().as_str() {
                "none" | "off" | "" => Outline::None,
                "auto" => Outline::Auto,
                _ => Outline::Color(parse_color(l).map_err(|_| {
                    format!("invalid outline '{l}': use none, auto or a color like #1a1020")
                })?),
            };
        }
        o.vivid = a.vivid.unwrap_or(false);
        Ok(o)
    }

    /// Legt eine Datei als Vorlage-Ebene zum Nachzeichnen in ein vorhandenes Bild.
    fn import_reference(
        &self,
        a: &ImportArgs,
        mut p: Project,
        picture: &Picture,
        file: &str,
    ) -> Res {
        let asset = a.asset.ok_or(
            "as=\"reference\" needs `asset`: the character, animation or image to trace on",
        )?;
        let r = Self::resolve(&p, asset)?;
        let mut frames: Vec<Vec<Rgba>> = picture.frames.iter().map(|f| f.pixels.clone()).collect();
        if let Some(px) = &a.pixelate {
            let o = Self::pixelate_options(&p, px, picture, None)?;
            for f in &mut frames {
                pixelate::remove_background(
                    f,
                    picture.width,
                    picture.height,
                    o.background,
                    o.tolerance,
                );
            }
        }
        let s = Self::sprite_mut(&mut p, r)?;
        if s.layer_count() >= MAX_LAYERS {
            return Err(format!("at most {MAX_LAYERS} layers"));
        }
        let (cw, ch) = (s.width(), s.height());
        // Hat die Datei so viele Frames wie das Bild, bekommt jeder Frame seine Vorlage.
        if frames.len() != s.frames().len() {
            frames.truncate(1);
        }
        let refs: Vec<Vec<Rgba>> = frames
            .iter()
            .map(|f| pixelate::reference(f, picture.width, picture.height, cw, ch))
            .collect();
        let name = a
            .name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or("Reference")
            .to_string();
        if s.layer_index(&name).is_some() {
            return Err(format!("a layer named '{name}' already exists"));
        }
        let opacity = a.opacity.unwrap_or(128);
        s.add_reference_layer(name.clone(), &refs, opacity)
            .map_err(e)?;
        let png = render_plain(s, &[0], Some(0))?;
        let layers = s.layer_names().join(", ");
        let target = Self::asset_name(&p, r);
        self.save(a.project, &mut p)?;
        Ok(Reply {
            text: format!(
                "Added '{file}' as reference layer '{name}' (layer 0, opacity {opacity}/255) to '{target}', fitted into its {cw}×{ch} canvas{}. It is shown in the editor to trace over, but it is not part of the picture: export, palette, outline and render leave it out (render with layer=0 to look at it). All other layers moved up by one: {layers}. Draw on another layer and remove the reference with edit_layers when done.",
                if refs.len() > 1 { ", one picture per frame" } else { "" }
            ),
            image_png: Some(png),
        })
    }

    /// Holt eine Bilddatei von der Platte als Bild, Figur oder Animation ins Projekt.
    pub fn import_image(&self, a: &ImportArgs) -> Res {
        let (_lock, mut p) = self.load_locked(a.project)?;
        let path = Path::new(a.path.trim());
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let picture = import::read(path).map_err(|err| format!("cannot import: {err}"))?;
        let kind = a.kind.unwrap_or("image").trim().to_ascii_lowercase();
        if kind == "reference" {
            return self.import_reference(a, p, &picture, &file);
        }
        // Bei einer Animation geben Größe und Palette der Figur die Umwandlung vor.
        let hero = match (&a.pixelate, kind.as_str(), a.character) {
            (Some(_), "animation", Some(c)) => match Self::resolve(&p, c)? {
                AssetRef::Character(id) => Some(p.character(id).map_err(e)?),
                _ => None,
            },
            _ => None,
        };
        let pixelate = match &a.pixelate {
            Some(px) => Some(Self::pixelate_options(&p, px, &picture, hero)?),
            None => None,
        };
        let grid = match (a.frame_width, a.frame_height, a.columns, a.rows) {
            (None, None, None, None) => Grid::Whole,
            (Some(width), Some(height), None, None) => Grid::Cells { width, height },
            (None, None, columns, rows) => Grid::Count {
                columns: columns.unwrap_or(1),
                rows: rows.unwrap_or(1),
            },
            _ => {
                return Err("to split a sprite sheet give either frame_width and frame_height, or columns and/or rows".into())
            }
        };
        let shrink = match a.downscale {
            None | Some(0) => Shrink::Auto,
            Some(k) => Shrink::By(k),
        };
        let options = import::Options {
            shrink,
            grid,
            keep_empty: a.keep_empty_frames,
            pixelate: pixelate.clone(),
        };
        let mut ready = import::prepare(&picture, options).map_err(e)?;
        let default_name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let wanted = a.name.map(str::trim).filter(|n| !n.is_empty());
        let depth = Self::depth(a.depth_bits)?;
        let mut notes = Vec::new();
        if ready.shrunk_by > 1 {
            notes.push(format!(
                "The file was {}×{} px; it was scaled down {}× to its true pixel size.",
                picture.width, picture.height, ready.shrunk_by
            ));
        }
        if let Some(px) = &pixelate {
            notes.push(format!(
                "Converted from {}×{} px to {}×{} pixel art ({}, dither {}{}{}).",
                picture.width,
                picture.height,
                px.width,
                px.height,
                match &px.palette {
                    Some(c) => format!("{} given palette colors", c.len()),
                    None => format!("up to {} colors", px.colors),
                },
                px.dither.id(),
                if ready.background_removed > 0 {
                    ", background removed"
                } else {
                    ""
                },
                if px.outline != Outline::None {
                    ", outlined"
                } else {
                    ""
                },
            ));
        }

        let (created, report) = match kind.as_str() {
            "image" => {
                let name = Self::check_new_name(&p, wanted.unwrap_or(&default_name))?;
                let (id, report) =
                    import::add_image(&mut p, &name, &ready.picture, depth).map_err(e)?;
                (AssetRef::Image(id), report)
            }
            "character" => {
                let name = Self::check_new_name(&p, wanted.unwrap_or(&default_name))?;
                let animation = a.animation.map(|k| AnimationKind::parse(k.trim()));
                if let (Some(kind), true) = (&animation, ready.picture.frames.len() > 1) {
                    Self::check_new_name(&p, &format!("{name} {}", kind.name()))?;
                }
                let (made, report) =
                    import::add_character(&mut p, &name, &ready.picture, depth, animation)
                        .map_err(e)?;
                match made.animation {
                    Some(id) => notes.push(format!(
                        "Frame 0 became the base pose; all {} frames became the animation '{}'.",
                        ready.picture.frames.len(),
                        p.animation(id).map_err(e)?.name
                    )),
                    None if ready.picture.frames.len() > 1 => notes.push(
                        "Only frame 0 was used as the base pose. Pass `animation` (e.g. \"walk\") to also keep all frames as an animation.".into(),
                    ),
                    None => {}
                }
                (AssetRef::Character(made.character), report)
            }
            "animation" => {
                let character = a.character.ok_or(
                    "importing an animation needs `character`: the character it belongs to",
                )?;
                let AssetRef::Character(cid) = Self::resolve(&p, character)? else {
                    return Err(format!("'{character}' is not a character"));
                };
                let hero = p.character(cid).map_err(e)?;
                let (w, h) = hero.size();
                // Ohne Angabe wird ein Spritesheet in Frames von der Größe der Figur geteilt.
                if grid == Grid::Whole {
                    let (fw, fh) = (ready.picture.width, ready.picture.height);
                    let shrunk = ready.shrunk_by > 1;
                    ready.cut_to_frames(w, h, a.keep_empty_frames).map_err(|_| {
                        format!(
                            "the file is {fw}×{fh} px{}, which is not a whole number of {w}×{h} frames of character '{}'. Give frame_width/frame_height, columns/rows or downscale.",
                            if shrunk { " after scaling down" } else { "" },
                            hero.name
                        )
                    })?;
                }
                let kind = AnimationKind::parse(a.animation.unwrap_or(&default_name).trim());
                let name = match wanted {
                    Some(name) => name.to_string(),
                    None => format!("{} {}", hero.name, kind.name()),
                };
                let name = Self::check_new_name(&p, &name)?;
                let new_colors = match a.new_colors.unwrap_or("add").to_ascii_lowercase().as_str() {
                    "add" => NewColors::Add,
                    "snap" => NewColors::Snap,
                    other => return Err(format!("unknown new_colors '{other}', use add or snap")),
                };
                let (id, report, added) = import::add_animation(
                    &mut p,
                    cid,
                    Some(&name),
                    kind,
                    &ready.picture,
                    new_colors,
                )
                .map_err(e)?;
                if added > 0 {
                    notes.push(format!(
                        "{added} new color(s) were added to the character's palette."
                    ));
                }
                (AssetRef::Animation(id), report)
            }
            other => {
                return Err(format!(
                    "unknown `as` value '{other}', use image, character, animation or reference"
                ))
            }
        };
        if report.changed > 0 {
            notes.push(format!(
                "{} of {} colors were changed to fit the color depth or palette.",
                report.changed, report.source_colors
            ));
        }
        self.save(a.project, &mut p)?;
        let s = Self::sprite(&p, created)?;
        let shown: Vec<usize> = (0..s.frames().len().min(16)).collect();
        notes.insert(
            0,
            format!(
                "Imported '{file}' as {} '{}': {} frame(s) of {}×{} px, {} bit, {} colors.",
                created.kind(),
                Self::asset_name(&p, created),
                s.frames().len(),
                s.width(),
                s.height(),
                s.depth.bits(),
                report.colors,
            ),
        );
        if shown.len() < s.frames().len() {
            notes.push(format!(
                "The preview shows the first {} frames.",
                shown.len()
            ));
        }
        Ok(Reply {
            text: notes.join(" "),
            image_png: Some(render_plain(s, &shown, None)?),
        })
    }

    pub fn delete_asset(&self, project: &str, asset: &str) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;
        let name = Self::asset_name(&p, r);
        let extra = match r {
            AssetRef::Character(id) => {
                let n = p.animations_of(id).count();
                p.remove_character(id).map_err(e)?;
                format!(" and its {n} animation(s)")
            }
            AssetRef::Animation(id) => {
                p.remove_animation(id).map_err(e)?;
                String::new()
            }
            AssetRef::Image(id) => {
                p.remove_image(id).map_err(e)?;
                String::new()
            }
        };
        self.save(project, &mut p)?;
        Ok(Reply::text(format!(
            "Deleted {} '{name}'{extra}.",
            r.kind()
        )))
    }

    pub fn copy_character(&self, from_project: &str, character: &str, to_project: &str) -> Res {
        let src = self.load(from_project)?;
        let AssetRef::Character(cid) = Self::resolve(&src, character)? else {
            return Err(format!("'{character}' is not a character"));
        };
        let bundle: CharacterBundle = src.export_character(cid).map_err(e)?;
        let (_lock, mut dst) = self.load_locked(to_project)?;
        let replaced = dst.characters.iter().any(|c| c.id == cid);
        // Namen prüfen gegen alles, was im Zielprojekt bestehen bleibt. Die Figur
        // selbst und ihre bisherigen Animationen werden ersetzt und zählen nicht.
        let surviving: Vec<String> = dst
            .characters
            .iter()
            .filter(|c| c.id != cid)
            .map(|c| c.name.clone())
            .chain(
                dst.animations
                    .iter()
                    .filter(|a| a.character_id != cid)
                    .map(|a| a.name.clone()),
            )
            .chain(dst.images.iter().map(|i| i.name.clone()))
            .collect();
        for incoming in
            std::iter::once(&bundle.character.name).chain(bundle.animations.iter().map(|a| &a.name))
        {
            Self::check_name_free(&surviving, incoming)
                .map_err(|err| format!("cannot copy into project '{to_project}': {err}"))?;
        }
        let n = bundle.animations.len();
        let name = bundle.character.name.clone();
        dst.import_character(bundle).map_err(e)?;
        self.save(to_project, &mut dst)?;
        Ok(Reply::text(format!(
            "{} character '{name}' with {n} animation(s) into project '{to_project}'. It is pixel-identical to the one in '{from_project}'.",
            if replaced { "Updated" } else { "Copied" }
        )))
    }

    // ---------------------------------------------------------------- Zeichnen

    fn layer_index(s: &Sprite, layer: Option<&Value>) -> Res<usize> {
        let idx = match layer {
            None | Some(Value::Null) => 0,
            Some(Value::Number(n)) => {
                n.as_u64()
                    .ok_or("layer index must be a non-negative integer")? as usize
            }
            Some(Value::String(name)) => match name.parse::<usize>() {
                Ok(i) if s.layer_index(name).is_none() => i,
                _ => s.layer_index(name).ok_or_else(|| {
                    format!(
                        "no layer named '{name}'. Layers: {}",
                        s.layer_names().join(", ")
                    )
                })?,
            },
            Some(_) => return Err("layer must be an index or a layer name".into()),
        };
        if idx >= s.layer_count() {
            return Err(format!(
                "layer {idx} does not exist. Layers: {}",
                s.layer_names()
                    .iter()
                    .enumerate()
                    .map(|(i, n)| format!("{i}={n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(idx)
    }

    fn frame_index(s: &Sprite, frame: Option<usize>) -> Res<usize> {
        let f = frame.unwrap_or(0);
        if f >= s.frames().len() {
            return Err(format!(
                "frame {f} does not exist, the asset has {} frame(s) (0-based)",
                s.frames().len()
            ));
        }
        Ok(f)
    }

    /// Welche Frames gemeint sind: eine Zahl, eine Liste oder "all".
    fn frame_list(s: &Sprite, frame: Option<&Value>) -> Res<Vec<usize>> {
        let n = s.frames().len();
        let one = |v: &Value| -> Res<usize> {
            let f = match v {
                Value::Number(num) => num.as_u64().map(|f| f as usize),
                Value::String(text) => text.trim().parse::<usize>().ok(),
                _ => None,
            }
            .ok_or("`frame` must be a frame index, a list of indexes or \"all\"")?;
            Self::frame_index(s, Some(f))
        };
        match frame {
            None | Some(Value::Null) => Ok(vec![0]),
            Some(Value::String(text)) if text.trim().eq_ignore_ascii_case("all") => {
                Ok((0..n).collect())
            }
            Some(Value::Array(list)) => {
                if list.is_empty() {
                    return Err("`frame` is an empty list".into());
                }
                let mut out = Vec::new();
                for v in list {
                    let f = one(v)?;
                    if out.contains(&f) {
                        return Err(format!("frame {f} is listed twice"));
                    }
                    out.push(f);
                }
                Ok(out)
            }
            Some(v) => Ok(vec![one(v)?]),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        project: &str,
        asset: &str,
        frame: Option<&Value>,
        layer: Option<&Value>,
        ops: Vec<DrawOp>,
        per_frame: Vec<PerFrame>,
        mirror: Option<&str>,
    ) -> Res {
        let (mirror_h, mirror_v) = match mirror.map(|m| m.trim().to_ascii_lowercase()).as_deref() {
            None | Some("" | "none") => (false, false),
            Some("left_right" | "left-right" | "x" | "horizontal") => (true, false),
            Some("top_bottom" | "top-bottom" | "y" | "vertical") => (false, true),
            Some("both" | "xy") => (true, true),
            Some(other) => {
                return Err(format!(
                    "unknown mirror '{other}', use left_right, top_bottom or both"
                ))
            }
        };
        let extra: usize = per_frame
            .iter()
            .map(|f| f.ops.as_ref().map_or(0, Vec::len))
            .sum();
        if ops.is_empty() && extra == 0 {
            return Err("ops is empty".into());
        }
        if ops.len() > MAX_OPS || extra > MAX_OPS {
            return Err(format!("at most {MAX_OPS} ops per call"));
        }
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;

        // Stempel aus anderen Assets vorab holen: danach ist das Projekt zum Ändern geliehen.
        let mut sources: HashMap<String, Sprite> = HashMap::new();
        let all_ops = ops
            .iter()
            .chain(per_frame.iter().flat_map(|f| f.ops.iter().flatten()));
        for op in all_ops {
            if let DrawOp::Stamp {
                from_asset: Some(name),
                ..
            } = op
            {
                let other = Self::resolve(&p, name)?;
                if other != r && !sources.contains_key(name) {
                    sources.insert(name.clone(), Self::sprite(&p, other)?.clone());
                }
            }
        }

        // Eigene Formen nur laden, wenn eine Op sie braucht.
        let custom = ops
            .iter()
            .chain(per_frame.iter().flat_map(|f| f.ops.iter().flatten()))
            .any(|op| matches!(op, DrawOp::Shape { shape, .. } if Shape::parse(shape).is_none()));
        let custom_shapes = if custom {
            self.ws.custom_shapes()
        } else {
            Vec::new()
        };

        let style = p.style.clone();
        let s = Self::sprite_mut(&mut p, r)?;
        let before = s.clone();
        let frames = Self::frame_list(s, frame)?;
        let l = Self::layer_index(s, layer)?;
        if s.outline_layer() == Some(l) {
            return Err(format!(
                "layer {l} ('{}') holds the automatic outline and is redrawn after every change. Draw on another layer, or call set_outline with action \"freeze\" to keep it as a normal layer you can edit.",
                s.layer_names()[l]
            ));
        }
        if s.is_effect_layer(l) {
            return Err(format!(
                "layer {l} ('{}') holds an effect and is redrawn after every change. Draw on another layer, or call effects with action \"freeze\" to keep it as a normal layer you can edit.",
                s.layer_names()[l]
            ));
        }

        // Jeden Eintrag von per_frame seinem Frame zuordnen.
        let mut extras: HashMap<usize, &PerFrame> = HashMap::new();
        for (i, entry) in per_frame.iter().enumerate() {
            let f = match entry.frame {
                Some(f) if frames.contains(&f) => f,
                Some(f) => {
                    return Err(format!(
                        "per_frame entry {i} is for frame {f}, which is not among the frames drawn on ({})",
                        list_frames(&frames)
                    ))
                }
                None => *frames.get(i).ok_or_else(|| {
                    format!(
                        "per_frame has {} entries but only {} frame(s) are drawn on",
                        per_frame.len(),
                        frames.len()
                    )
                })?,
            };
            if extras.insert(f, entry).is_some() {
                return Err(format!("per_frame has two entries for frame {f}"));
            }
        }

        let mut ctx = DrawCtx {
            snapped: BTreeMap::new(),
            warnings: Vec::new(),
            swap: HashMap::new(),
            sources: &sources,
            style,
            shapes: &custom_shapes,
        };
        for &f in &frames {
            let t = Target::new(f, l);
            let entry = extras.get(&f);
            let untouched = (mirror_h || mirror_v)
                .then(|| s.layer(f, l).map(|layer| layer.pixels().to_vec()))
                .transpose()
                .map_err(e)?;
            ctx.swap.clear();
            for (from, to) in entry.and_then(|e| e.colors.as_ref()).into_iter().flatten() {
                ctx.swap.insert(parse_color(from)?, parse_color(to)?);
            }
            for (i, op) in ops.iter().enumerate() {
                apply_op(s, t, op, &mut ctx)
                    .map_err(|err| format!("op {i} on frame {f}: {err} (nothing was saved)"))?;
            }
            ctx.swap.clear();
            for (i, op) in entry
                .and_then(|e| e.ops.as_ref())
                .into_iter()
                .flatten()
                .enumerate()
            {
                apply_op(s, t, op, &mut ctx).map_err(|err| {
                    format!("per_frame op {i} on frame {f}: {err} (nothing was saved)")
                })?;
            }
            if let Some(untouched) = untouched {
                brush::mirror_changes(s, t, &untouched, mirror_h, mirror_v).map_err(e)?;
            }
        }
        s.refresh_outline().map_err(e)?;
        let (png, _) = render(s, &frames, None, &ViewOptions::default())?;
        let after = s.clone();
        let mut text = format!(
            "Applied {} op(s) to '{}', {}, layer {l} ('{}').",
            ops.len() * frames.len() + extra,
            Self::asset_name(&p, r),
            if frames.len() == 1 {
                format!("frame {}", frames[0])
            } else {
                format!("frames {} (preview left to right)", list_frames(&frames))
            },
            Self::sprite(&p, r)?.layer_names()[l],
        );
        if !ctx.snapped.is_empty() {
            let list: Vec<String> = ctx
                .snapped
                .iter()
                .map(|(a, b)| format!("{a}→{b}"))
                .collect();
            text.push_str(&format!(
                " Colors adjusted to the palette or color depth: {}.",
                list.join(", ")
            ));
        }
        ctx.warnings.dedup();
        for w in &ctx.warnings {
            text.push_str(&format!(" Warning: {w}."));
        }
        if matches!(r, AssetRef::Character(id) if p.animations_of(id).next().is_some()) {
            text.push_str(
                " Note: existing animations of this character keep the earlier base pose.",
            );
        }
        self.save(project, &mut p)?;
        self.remember(project, r, before, after)?;
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    /// Merkt sich eine Änderung an einem Asset, damit `undo` sie zurücknehmen kann.
    fn remember(&self, project: &str, asset: AssetRef, before: Sprite, after: Sprite) -> Res<()> {
        if before == after {
            return Ok(());
        }
        let mut history = self
            .history
            .lock()
            .map_err(|_| "the undo history is unavailable".to_string())?;
        history.push(Drawn {
            project: workspace::name_key(project),
            asset,
            before,
            after,
        });
        // Älteste Einträge fallen weg, wenn es zu viele oder zu große werden.
        let bytes = |h: &[Drawn]| -> usize {
            h.iter()
                .map(|d| sprite_bytes(&d.before) + sprite_bytes(&d.after))
                .sum()
        };
        while history.len() > MAX_UNDO || (history.len() > 1 && bytes(&history) > MAX_UNDO_BYTES) {
            history.remove(0);
        }
        Ok(())
    }

    /// Nimmt die letzten `draw`-Aufrufe dieser Sitzung zurück, den jüngsten zuerst.
    pub fn undo(&self, project: &str, steps: Option<usize>) -> Res {
        let steps = steps.unwrap_or(1).clamp(1, MAX_UNDO);
        let key = workspace::name_key(project);
        let (_lock, mut p) = self.load_locked(project)?;
        let mut history = self
            .history
            .lock()
            .map_err(|_| "the undo history is unavailable".to_string())?;
        let mut undone: Vec<String> = Vec::new();
        let mut stopped = None;
        let mut last = None;
        while undone.len() < steps {
            let Some(i) = history.iter().rposition(|d| d.project == key) else {
                break;
            };
            let entry = history.remove(i);
            let Ok(s) = Self::sprite_mut(&mut p, entry.asset) else {
                stopped = Some("the asset no longer exists".to_string());
                break;
            };
            if *s != entry.after {
                stopped = Some(format!(
                    "'{}' was changed after that draw call (in the editor or by another tool), so it was left alone",
                    Self::asset_name(&p, entry.asset)
                ));
                break;
            }
            *s = entry.before;
            undone.push(Self::asset_name(&p, entry.asset));
            last = Some(entry.asset);
        }
        if undone.is_empty() {
            return Err(stopped.unwrap_or_else(|| {
                "nothing to undo: no draw call of this session is left for this project".into()
            }));
        }
        self.save(project, &mut p)?;
        let left = history.iter().filter(|d| d.project == key).count();
        let mut text = format!(
            "Undid {} draw call(s), newest first: {}. {left} more can be undone.",
            undone.len(),
            undone.join(", ")
        );
        if let Some(why) = stopped {
            text.push_str(&format!(" Stopped early: {why}."));
        }
        let image_png = match last {
            Some(r) => {
                let s = Self::sprite(&p, r)?;
                let frames: Vec<usize> = (0..s.frames().len()).collect();
                Some(render(s, &frames, None, &ViewOptions::default())?.0)
            }
            None => None,
        };
        Ok(Reply { text, image_png })
    }

    pub fn view(
        &self,
        project: &str,
        asset: &str,
        frame: Option<&Value>,
        layer: Option<&Value>,
        options: &ViewOptions,
    ) -> Res {
        let p = self.load(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite(&p, r)?;
        let frames: Vec<usize> = match frame {
            None | Some(Value::Null) => (0..s.frames().len()).collect(),
            some => Self::frame_list(s, some)?,
        };
        let layer = match layer {
            None | Some(Value::Null) => None,
            some => Some(Self::layer_index(s, some)?),
        };
        let (png, notes) = render(s, &frames, layer, options)?;
        let mut text = format!(
            "'{}' ({}), {}×{} px, {} bit, layers: {}. Showing {}{}.",
            Self::asset_name(&p, r),
            r.kind(),
            s.width(),
            s.height(),
            s.depth.bits(),
            s.layer_names().join(", "),
            if frames.len() == 1 {
                format!("frame {}", frames[0])
            } else {
                format!("frames {} left to right", list_frames(&frames))
            },
            layer
                .map(|l| format!(", only layer {l}"))
                .unwrap_or_default(),
        );
        for note in notes {
            text.push(' ');
            text.push_str(&note);
        }
        if layer.is_none() {
            let checks = Checks {
                outline: p.style.outline,
                seams: matches!(r, AssetRef::Image(_)),
            };
            let hints = hint_texts(s, &lint::check_frames(s, &frames, checks));
            if !hints.is_empty() {
                text.push_str(&format!(" Hints: {}.", hints.join("; ")));
            }
        }
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_pixels(
        &self,
        project: &str,
        asset: &str,
        frame: Option<usize>,
        layer: Option<&Value>,
        x: Option<u32>,
        y: Option<u32>,
        w: Option<u32>,
        h: Option<u32>,
    ) -> Res {
        let p = self.load(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite(&p, r)?;
        let f = Self::frame_index(s, frame)?;
        let px: Vec<Rgba> = match layer {
            None | Some(Value::Null) => s.composite(f).map_err(e)?,
            some => s
                .layer(f, Self::layer_index(s, some)?)
                .map_err(e)?
                .pixels()
                .to_vec(),
        };
        let (x, y) = (x.unwrap_or(0), y.unwrap_or(0));
        if x >= s.width() || y >= s.height() {
            return Err(format!(
                "region start ({x}, {y}) is outside the {}×{} image",
                s.width(),
                s.height()
            ));
        }
        let w = w.unwrap_or(s.width() - x).min(s.width() - x);
        let h = h.unwrap_or(s.height() - y).min(s.height() - y);
        if w > MAX_TEXT_REGION || h > MAX_TEXT_REGION {
            return Err(format!(
                "region {w}×{h} is too large for text output (max {MAX_TEXT_REGION}×{MAX_TEXT_REGION}). Pass x, y, w, h to read a smaller region, or use view."
            ));
        }
        let mut legend: Vec<Rgba> = Vec::new();
        let mut rows = Vec::with_capacity(h as usize);
        for row in y..y + h {
            let mut line = String::with_capacity(w as usize);
            for col in x..x + w {
                let c = px[(row * s.width() + col) as usize];
                if c.is_transparent() {
                    line.push('.');
                    continue;
                }
                let i = legend.iter().position(|&l| l == c).unwrap_or_else(|| {
                    legend.push(c);
                    legend.len() - 1
                });
                match SYMBOLS.chars().nth(i) {
                    Some(ch) => line.push(ch),
                    None => {
                        return Err(format!(
                            "region has more than {} different colors, too many for text output. Read a smaller region or use view.",
                            SYMBOLS.len()
                        ))
                    }
                }
            }
            rows.push(line);
        }
        let legend: BTreeMap<String, String> = legend
            .iter()
            .enumerate()
            .map(|(i, c)| (SYMBOLS.chars().nth(i).unwrap().to_string(), c.to_string()))
            // Mit Eintrag für '.', damit ein zurückgeschriebenes Raster auch löscht.
            .chain(std::iter::once((
                ".".to_string(),
                "transparent".to_string(),
            )))
            .collect();
        Ok(Reply::json(json!({
            "asset": Self::asset_name(&p, r),
            "frame": f,
            "region": {"x": x, "y": y, "w": w, "h": h},
            "legend": legend,
            "rows": rows,
            "hint": "rows and legend can be edited and passed back to the grid op of draw. '.' is transparent: with this legend it erases, without a '.' entry the grid op leaves those pixels unchanged.",
        })))
    }

    // ---------------------------------------------------------------- Ebenen und Frames

    /// Eigene Formen: auflisten, aus Gezeichnetem speichern oder löschen.
    #[allow(clippy::too_many_arguments)]
    pub fn custom_shapes(
        &self,
        action: &str,
        name: Option<&str>,
        project: Option<&str>,
        asset: Option<&str>,
        frame: Option<usize>,
        layer: Option<&Value>,
        area: Option<[i64; 4]>,
    ) -> Res {
        let list = |ws: &Workspace| -> String {
            let all = ws.custom_shapes();
            if all.is_empty() {
                "No custom shapes saved yet.".into()
            } else {
                let names: Vec<String> = all
                    .iter()
                    .map(|s| format!("'{}' ({}×{})", s.name, s.width, s.height))
                    .collect();
                format!("Custom shapes: {}.", names.join(", "))
            }
        };
        let need_name = || {
            name.map(str::trim)
                .filter(|n| !n.is_empty() && n.chars().count() <= 40)
                .ok_or("this action needs `name` (1 to 40 characters)".to_string())
        };
        match action.trim() {
            "" | "list" => Ok(Reply::text(list(&self.ws))),
            "save" => {
                let n = need_name()?;
                if Shape::parse(n).is_some() {
                    return Err(format!("'{n}' is a built-in shape, pick another name"));
                }
                let project = project.ok_or("action 'save' needs `project` and `asset`")?;
                let asset = asset.ok_or("action 'save' needs `asset`")?;
                let p = self.load(project)?;
                let r = Self::resolve(&p, asset)?;
                let s = Self::sprite(&p, r)?;
                let f = Self::frame_index(s, frame)?;
                let l = Self::layer_index(s, layer)?;
                let area = area.map(|[x, y, w, h]| {
                    (
                        x,
                        y,
                        w.clamp(0, i64::from(u32::MAX)) as u32,
                        h.clamp(0, i64::from(u32::MAX)) as u32,
                    )
                });
                let shape = CustomShape::from_layer(n, s, Target::new(f, l), area).map_err(e)?;
                let replaced = self.ws.custom_shape(n).is_some();
                self.ws.save_shape(&shape).map_err(e)?;
                Ok(Reply::text(format!(
                    "{} custom shape '{n}' ({}×{} pixels, trimmed to what is drawn). Draw it with the `shape` op, also in other projects. {}",
                    if replaced { "Replaced" } else { "Saved" },
                    shape.width,
                    shape.height,
                    list(&self.ws)
                )))
            }
            "delete" => {
                let n = need_name()?;
                if !self.ws.delete_shape(n).map_err(e)? {
                    return Err(format!(
                        "there is no custom shape '{n}'. {}",
                        list(&self.ws)
                    ));
                }
                Ok(Reply::text(format!(
                    "Deleted custom shape '{n}'. {}",
                    list(&self.ws)
                )))
            }
            other => Err(format!(
                "unknown action '{other}', use list, save or delete"
            )),
        }
    }

    /// Ändert die Leinwandgröße eines Assets. Bei Figuren und Animationen wachsen die
    /// Figur und alle ihre Animationen gemeinsam, damit sie gleich groß bleiben.
    pub fn resize_canvas(
        &self,
        project: &str,
        asset: &str,
        width: Option<u32>,
        height: Option<u32>,
        anchor: Option<&str>,
        crop_to_content: bool,
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite(&p, r)?;
        let old = (s.width(), s.height());
        let (w, h, dx, dy) = if crop_to_content {
            if width.is_some() || height.is_some() {
                return Err("give either `crop_to_content` or a new size, not both".into());
            }
            let (x, y, w, h) = s
                .content_bounds()
                .ok_or("the asset is empty, there is nothing to crop to")?;
            (w, h, -x, -y)
        } else {
            let (w, h) = (width.unwrap_or(old.0), height.unwrap_or(old.1));
            let align = match anchor.map(|a| a.trim().to_ascii_lowercase()).as_deref() {
                None | Some("center" | "middle") => (0, 0),
                Some(a) => {
                    let (dx, dy) = parse_direction(a, "anchor")?;
                    (dx as i8, dy as i8)
                }
            };
            let (dx, dy) = pixel_core::sprite::canvas_offset(old, (w, h), align);
            (w, h, dx, dy)
        };
        let max = pixel_core::MAX_SIZE;
        if w == 0 || h == 0 || w > max || h > max {
            return Err(format!("width and height must be 1 to {max}"));
        }
        let character = match r {
            AssetRef::Character(id) => Some(id),
            AssetRef::Animation(id) => Some(p.animation(id).map_err(e)?.character_id),
            AssetRef::Image(_) => None,
        };
        let also = match character {
            Some(id) => {
                let n = p.animations_of(id).count();
                p.resize_character(id, w, h, dx, dy).map_err(e)?;
                let name = p.character(id).map_err(e)?.name.clone();
                match n {
                    0 => format!(" (character '{name}')"),
                    1 => format!(" (character '{name}' and its animation)"),
                    n => format!(" (character '{name}' and all {n} animations)"),
                }
            }
            None => {
                let s = Self::sprite_mut(&mut p, r)?;
                s.resize_canvas(w, h, dx, dy).map_err(e)?;
                s.refresh_outline().map_err(e)?;
                String::new()
            }
        };
        let s = Self::sprite(&p, r)?;
        let frames: Vec<usize> = (0..s.frames().len()).collect();
        let (png, _) = render(s, &frames, None, &ViewOptions::default())?;
        let text = format!(
            "Canvas of '{}' is now {w}×{h} (was {}×{}){also}; content moved by ({dx}, {dy}).",
            Self::asset_name(&p, r),
            old.0,
            old.1
        );
        self.save(project, &mut p)?;
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_layers(
        &self,
        project: &str,
        asset: &str,
        action: &str,
        layer: Option<&Value>,
        name: Option<&str>,
        visible: Option<bool>,
        opacity: Option<u8>,
        to: Option<usize>,
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite_mut(&mut p, r)?;
        let need_name = || {
            name.map(str::trim)
                .filter(|n| !n.is_empty() && n.len() <= 64)
                .ok_or("this action needs `name` (1 to 64 characters)".to_string())
        };
        let need_layer = |s: &Sprite| match layer {
            None | Some(Value::Null) => Err("this action needs `layer`".to_string()),
            some => Self::layer_index(s, some),
        };
        let unique = |s: &Sprite, n: &str| match s.layer_index(n) {
            Some(_) => Err(format!("a layer named '{n}' already exists")),
            None => Ok(()),
        };
        let msg = match action {
            "add" => {
                let n = need_name()?;
                unique(s, n)?;
                if s.layer_count() >= MAX_LAYERS {
                    return Err(format!("at most {MAX_LAYERS} layers"));
                }
                let i = s.add_layer(n);
                format!("Added layer {i} ('{n}') on top, in all frames.")
            }
            "remove" => {
                let i = need_layer(s)?;
                s.remove_layer(i)
                    .map_err(|_| "cannot remove the last remaining layer".to_string())?;
                format!("Removed layer {i} from all frames.")
            }
            "rename" => {
                let i = need_layer(s)?;
                let n = need_name()?;
                unique(s, n)?;
                s.rename_layer(i, n).map_err(e)?;
                format!("Renamed layer {i} to '{n}'.")
            }
            "set" => {
                let i = need_layer(s)?;
                if visible.is_none() && opacity.is_none() {
                    return Err("action 'set' needs `visible` and/or `opacity`".into());
                }
                for f in 0..s.frames().len() {
                    let l = s.layer_mut(f, i).map_err(e)?;
                    if let Some(v) = visible {
                        l.visible = v;
                    }
                    if let Some(o) = opacity {
                        l.opacity = o;
                    }
                }
                format!("Updated layer {i} in all frames.")
            }
            "move" => {
                let i = need_layer(s)?;
                let to = to.ok_or("action 'move' needs `to` (the new index, 0 = bottom)")?;
                if to >= s.layer_count() {
                    return Err(format!("`to` must be 0 to {}", s.layer_count() - 1));
                }
                s.move_layer(i, to).map_err(e)?;
                format!("Moved layer {i} to position {to} in all frames.")
            }
            other => {
                return Err(format!(
                    "unknown action '{other}', use add, remove, rename, set or move"
                ))
            }
        };
        let layers = s.layer_names();
        self.save(project, &mut p)?;
        Ok(Reply::text(format!(
            "{msg} Layers bottom to top: {}",
            layers.join(", ")
        )))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn edit_frames(
        &self,
        project: &str,
        asset: &str,
        action: &str,
        frame: Option<usize>,
        from_frame: Option<usize>,
        duration_ms: Option<u32>,
        looping: Option<bool>,
        count: Option<usize>,
        insert_at: Option<usize>,
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;
        if matches!(r, AssetRef::Character(_)) && matches!(action, "add" | "duplicate") {
            return Err(
                "a character's base pose has a single frame. Use create_animation for motion."
                    .into(),
            );
        }
        let s = Self::sprite_mut(&mut p, r)?;
        let need = |f: Option<usize>, what: &str| f.ok_or(format!("this action needs `{what}`"));
        let exists = |s: &Sprite, f: usize| Self::frame_index(s, Some(f));
        let msg = match action {
            "add" => {
                let count = Self::frame_count(s, count)?;
                let first = s.frames().len();
                for _ in 0..count {
                    s.add_frame();
                }
                if count == 1 {
                    format!("Added empty frame {first} at the end.")
                } else {
                    format!("Added {count} empty frames at the end ({first} to {}).", first + count - 1)
                }
            }
            "duplicate" => {
                let count = Self::frame_count(s, count)?;
                let f = exists(s, need(frame, "frame")?)?;
                let at = insert_at.unwrap_or(f + 1);
                if at > s.frames().len() {
                    return Err(format!(
                        "`insert_at` {at} is past the end, the asset has {} frame(s)",
                        s.frames().len()
                    ));
                }
                // Liegt die Einfügestelle vor dem Original, rückt es mit jeder Kopie weiter.
                for i in 0..count {
                    let from = if at <= f { f + i } else { f };
                    s.insert_frame_copy(from, at + i).map_err(e)?;
                }
                if count == 1 {
                    format!("Duplicated frame {f}; the copy is frame {at}, later frames moved up by one.")
                } else {
                    format!(
                        "Duplicated frame {f} {count} times; the copies are frames {at} to {}, later frames moved up.",
                        at + count - 1
                    )
                }
            }
            "remove" => {
                let f = exists(s, need(frame, "frame")?)?;
                s.remove_frame(f).map_err(|_| "cannot remove the last remaining frame".to_string())?;
                format!("Removed frame {f}; later frames moved down by one.")
            }
            "copy" => {
                let from = exists(s, need(from_frame, "from_frame")?)?;
                let to = exists(s, need(frame, "frame")?)?;
                s.copy_frame(from, to).map_err(e)?;
                format!("Copied the content of frame {from} into frame {to}.")
            }
            "set_duration" => {
                let ms = duration_ms.filter(|ms| (10..=60_000).contains(ms)).ok_or("`duration_ms` must be 10 to 60000")?;
                match frame {
                    Some(f) => {
                        let f = exists(s, f)?;
                        s.frame_mut(f).map_err(e)?.duration_ms = ms;
                        format!("Frame {f} now lasts {ms} ms.")
                    }
                    None => {
                        for f in 0..s.frames().len() {
                            s.frame_mut(f).map_err(e)?.duration_ms = ms;
                        }
                        format!("All frames now last {ms} ms.")
                    }
                }
            }
            "set_looping" => {
                let AssetRef::Animation(id) = r else {
                    return Err("only animations have a looping flag".into());
                };
                let v = looping.ok_or("this action needs `looping`")?;
                p.animation_mut(id).map_err(e)?.looping = v;
                format!("Looping is now {v}.")
            }
            other => {
                return Err(format!(
                    "unknown action '{other}', use add, duplicate, remove, copy, set_duration or set_looping"
                ))
            }
        };
        let n = Self::sprite(&p, r)?.frames().len();
        self.save(project, &mut p)?;
        Ok(Reply::text(format!(
            "{msg} The asset now has {n} frame(s)."
        )))
    }

    /// Wie viele Frames `add` und `duplicate` anlegen sollen, mit Blick auf die Obergrenze.
    fn frame_count(s: &Sprite, count: Option<usize>) -> Res<usize> {
        let count = count.unwrap_or(1);
        let room = MAX_FRAMES.saturating_sub(s.frames().len());
        if count == 0 {
            return Err("`count` must be at least 1".into());
        }
        if count > room {
            return Err(format!(
                "at most {MAX_FRAMES} frames: the asset has {}, so only {room} more fit",
                s.frames().len()
            ));
        }
        Ok(count)
    }

    // ---------------------------------------------------------------- Paletten

    pub fn set_palette(
        &self,
        project: &str,
        asset: &str,
        preset: Option<&str>,
        colors: Option<&[String]>,
    ) -> Res {
        if preset.is_none() && colors.is_none() {
            return Err("give `palette_preset` or `palette`".into());
        }
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;
        if !matches!(r, AssetRef::Image(_)) {
            return Err("character palettes are fixed to keep the character consistent. Use add_character_colors to extend one.".into());
        }
        let s = Self::sprite_mut(&mut p, r)?;
        let pal = Self::palette(preset, colors, s.depth)?;
        let n = pal.len();
        s.set_palette(pal).map_err(e)?;
        let note = if s.depth == ColorDepth::Indexed8 {
            " Existing pixels were remapped to the nearest new color."
        } else {
            ""
        };
        self.save(project, &mut p)?;
        Ok(Reply::text(format!(
            "Palette replaced, it now has {n} color(s).{note}"
        )))
    }

    pub fn add_character_colors(&self, project: &str, character: &str, colors: &[String]) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let AssetRef::Character(cid) = Self::resolve(&p, character)? else {
            return Err(format!("'{character}' is not a character"));
        };
        let colors: Vec<Rgba> = colors.iter().map(|c| parse_color(c)).collect::<Res<_>>()?;
        p.add_character_colors(cid, &colors).map_err(e)?;
        let pal = p.character(cid).map_err(e)?.palette().colors.clone();
        self.save(project, &mut p)?;
        Ok(Reply::json(json!({
            "palette": pal,
            "note": "New colors were appended; existing colors and pixels are unchanged. The extended palette also applies to all animations of this character.",
        })))
    }

    /// Liest die Farben einer Bilddatei oder eines Assets als Palette aus.
    pub fn palette_from_image(
        &self,
        path: Option<&str>,
        project: Option<&str>,
        asset: Option<&str>,
        max_colors: Option<usize>,
    ) -> Res {
        let max = max_colors.unwrap_or(16);
        if !(1..=Palette::MAX_COLORS).contains(&max) {
            return Err(format!(
                "`max_colors` must be 1 to {}, got {max}",
                Palette::MAX_COLORS
            ));
        }
        let (source, (colors, found)) = match (path, project, asset) {
            (Some(path), None, None) => {
                let path = Path::new(path.trim());
                let picture =
                    import::read(path).map_err(|err| format!("cannot read the image: {err}"))?;
                let pixels = picture.frames.iter().flat_map(|f| f.pixels.iter().copied());
                (path.display().to_string(), quant::palette_of(pixels, max))
            }
            (None, Some(project), Some(asset)) => {
                let p = self.load(project)?;
                let r = Self::resolve(&p, asset)?;
                let s = Self::sprite(&p, r)?;
                let mut pixels = Vec::new();
                for f in 0..s.frames().len() {
                    pixels.extend(s.composite(f).map_err(e)?);
                }
                (Self::asset_name(&p, r), quant::palette_of(pixels, max))
            }
            _ => {
                return Err(
                    "give either `path` (an image file) or `project` and `asset`, not both".into(),
                )
            }
        };
        if colors.is_empty() {
            return Err(format!("'{source}' has no visible pixels, so no colors"));
        }
        let note = if found > colors.len() {
            format!(
                "The source has {found} colors; similar ones were merged and the most used color of each group kept."
            )
        } else {
            "These are all colors of the source.".to_string()
        };
        Ok(Reply {
            text: serde_json::to_string_pretty(&json!({
                "source": source,
                "colors": colors,
                "count": colors.len(),
                "source_colors": found,
                "note": format!("{note} Sorted dark to light, as in the picture. Pass the list as `palette` to create_image, create_character or set_style."),
            }))
            .unwrap_or_default(),
            image_png: Some(swatches(&colors)?),
        })
    }

    /// Bringt ein Bild oder eine Figur samt Animationen auf höchstens `colors` Farben.
    pub fn reduce_colors(&self, project: &str, asset: &str, colors: usize) -> Res {
        if !(1..=Palette::MAX_COLORS).contains(&colors) {
            return Err(format!(
                "`colors` must be 1 to {}, got {colors}",
                Palette::MAX_COLORS
            ));
        }
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;
        let name = Self::asset_name(&p, r);
        let before = Self::sprite(&p, r)?.clone();
        let (report, tail) = match r {
            AssetRef::Image(_) => {
                let s = Self::sprite_mut(&mut p, r)?;
                let report = quant::reduce(&mut [s], colors).map_err(e)?;
                (report, " `undo` takes this back.".to_string())
            }
            AssetRef::Character(id) => {
                let report = p.reduce_character_colors(id, colors).map_err(e)?;
                let n = p.animations_of(id).count();
                let tail = if n > 0 {
                    format!(" Its {n} animation(s) were reduced with it and share the new palette. This cannot be undone with `undo`.")
                } else {
                    " `undo` takes this back.".to_string()
                };
                (report, tail)
            }
            AssetRef::Animation(id) => {
                let owner = p.animation(id).map_err(e)?.character_id;
                let owner = p
                    .character(owner)
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                return Err(format!(
                    "an animation shares the palette of its character. Reduce the character '{owner}' instead; that covers all its animations."
                ));
            }
        };
        let s = Self::sprite(&p, r)?;
        let frames: Vec<usize> = (0..s.frames().len()).collect();
        let png = render_plain(s, &frames, None)?;
        if report.after == report.before {
            return Ok(Reply {
                text: format!(
                    "'{name}' already uses only {} color(s), nothing was changed.",
                    report.before
                ),
                image_png: Some(png),
            });
        }
        let after = s.clone();
        self.save(project, &mut p)?;
        // Nur was `undo` allein wiederherstellen kann, kommt in die Liste.
        let alone = match r {
            AssetRef::Character(id) => p.animations_of(id).next().is_none(),
            _ => true,
        };
        if alone {
            self.remember(project, r, before, after)?;
        }
        let list: Vec<String> = report.colors.iter().map(Rgba::to_string).collect();
        Ok(Reply {
            text: format!(
                "Reduced '{name}' from {} to {} colors. Similar colors were merged into the most used color of each group, so no new mixed colors appear. The palette is now: {}.{tail}",
                report.before,
                report.after,
                list.join(", ")
            ),
            image_png: Some(png),
        })
    }

    // ---------------------------------------------------------------- Kontur

    pub fn set_outline(&self, a: &OutlineArgs) -> Res {
        let action = a.action.unwrap_or("set").trim().to_ascii_lowercase();
        if !matches!(action.as_str(), "set" | "remove" | "freeze") {
            return Err(format!(
                "unknown action '{action}', use \"set\", \"remove\" or \"freeze\""
            ));
        }
        if let Some(t) = a.thickness {
            if !(1..=contour::MAX_THICKNESS).contains(&t) {
                return Err(format!(
                    "`thickness` must be 1 to {}, got {t}",
                    contour::MAX_THICKNESS
                ));
            }
        }
        let (_lock, mut p) = self.load_locked(a.project)?;
        let r = Self::resolve(&p, a.asset)?;
        let name = Self::asset_name(&p, r);
        let before = Self::sprite(&p, r)?.clone();

        let spec = if action == "set" {
            let unset = |t: &str| matches!(t.trim().to_ascii_lowercase().as_str(), "none" | "");
            let old = before.auto_outline().cloned();
            let mut spec = old
                .clone()
                .unwrap_or_else(|| AutoOutline::new(a.layer.unwrap_or("outline").trim()));
            if old.is_none() {
                spec.color = p.style.outline;
                spec.light = p
                    .style
                    .light_dir
                    .as_deref()
                    .and_then(|d| parse_direction(d, "light_dir").ok())
                    .map(|(x, y)| (x as i8, y as i8));
            }
            if let Some(layer) = a.layer {
                spec.layer = layer.trim().to_string();
            }
            if let Some(color) = a.color {
                spec.color = match color.trim().to_ascii_lowercase().as_str() {
                    "selective" | "auto" => None,
                    _ => Some(parse_color(color)?),
                };
            }
            if let Some(dir) = a.light_dir {
                spec.light = if unset(dir) {
                    None
                } else {
                    let (x, y) = parse_direction(dir, "light_dir")?;
                    Some((x as i8, y as i8))
                };
            }
            spec.inside = a.inside.unwrap_or(spec.inside);
            spec.corners = a.corners.unwrap_or(spec.corners);
            spec.thickness = a.thickness.unwrap_or(spec.thickness);
            spec.inner = a.inner.unwrap_or(spec.inner);
            Some(spec)
        } else {
            if before.auto_outline().is_none() {
                return Err(format!("'{name}' has no automatic outline"));
            }
            None
        };
        let keep = action == "freeze";

        let mut animations = 0;
        match r {
            AssetRef::Character(id) => {
                let with = a.with_animations.unwrap_or(true);
                animations = p
                    .set_character_outline(id, spec.clone(), keep, with)
                    .map_err(e)?;
            }
            _ => {
                let s = Self::sprite_mut(&mut p, r)?;
                match spec.clone() {
                    Some(spec) => {
                        s.set_auto_outline(spec).map_err(e)?;
                    }
                    None => s.stop_outline(keep).map_err(e)?,
                }
            }
        }
        let s = Self::sprite(&p, r)?;
        let frames: Vec<usize> = (0..s.frames().len()).collect();
        let png = render_plain(s, &frames, None)?;
        let after = s.clone();
        self.save(a.project, &mut p)?;
        if animations == 0 {
            self.remember(a.project, r, before, after)?;
        }
        let with = if animations > 0 {
            format!(" and its {animations} animation(s)")
        } else {
            String::new()
        };
        let text = match spec {
            Some(spec) => format!(
                "'{name}'{with} now has an automatic outline on layer '{}': {}. It is redrawn from all other visible layers after every change, in every frame, so keep drawing on the other layers. New animations of a character inherit it. {}",
                spec.layer,
                describe_outline(&spec),
                if animations > 0 { "This cannot be undone with `undo`; call set_outline with action \"remove\" instead." } else { "`undo` takes this back." }
            ),
            None if keep => format!(
                "The outline of '{name}'{with} is now a normal layer: it stays as it is and can be edited by hand, but no longer follows changes."
            ),
            None => format!("Removed the automatic outline and its layer from '{name}'{with}."),
        };
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    /// Licht und Schatten einrichten, ändern, festschreiben oder entfernen.
    pub fn set_light(&self, a: &LightArgs) -> Res {
        let action = a.action.unwrap_or("set").trim().to_ascii_lowercase();
        if !matches!(action.as_str(), "set" | "remove" | "freeze") {
            return Err(format!(
                "unknown action '{action}', use \"set\", \"remove\" or \"freeze\""
            ));
        }
        let (_lock, mut p) = self.load_locked(a.project)?;
        let r = Self::resolve(&p, a.asset)?;
        let name = Self::asset_name(&p, r);
        let before = Self::sprite(&p, r)?.clone();
        let (w, h) = (before.width(), before.height());
        // Ein Punkt ist eine Richtung wie "top-left" (weit außerhalb) oder [x, y] in Pixeln.
        let point = |v: &Value, what: &str| -> Res<(f32, f32)> {
            match v {
                Value::String(d) => {
                    let (x, y) = parse_direction(d, what)?;
                    if (x, y) == (0, 0) {
                        return Err(format!("`{what}` needs a direction like top-left"));
                    }
                    light::position_for(direction_name((x, y)), w, h)
                        .ok_or_else(|| format!("unknown {what} '{d}'"))
                }
                Value::Array(xy) if xy.len() == 2 => {
                    let n = |v: &Value| v.as_f64().map(|f| f as f32);
                    match (n(&xy[0]), n(&xy[1])) {
                        (Some(x), Some(y)) => Ok((x, y)),
                        _ => Err(format!("`{what}` must be a direction or [x, y]")),
                    }
                }
                _ => Err(format!(
                    "`{what}` must be a direction like \"top-left\" or a point [x, y]"
                )),
            }
        };
        let spec = if action == "set" {
            let old = before.auto_light().cloned();
            let mut spec = old.clone().unwrap_or_else(|| {
                let dir = p.style.light_dir.as_deref().unwrap_or("top-left");
                let at = light::position_for(dir, w, h).unwrap_or((-(w as f32), -(h as f32)));
                light::AutoLight {
                    light_layer: "light".into(),
                    shadow_layer: "shadow".into(),
                    ..light::AutoLight::new(at)
                }
            });
            match (a.path, a.from, a.to) {
                (Some(path), None, None) => {
                    spec.path = path.iter().map(|[x, y]| (*x, *y)).collect();
                }
                (Some(_), _, _) => return Err("give either `path` or `from`/`to`, not both".into()),
                (None, Some(from), to) => {
                    spec.path = vec![point(from, "from")?];
                    if let Some(to) = to {
                        spec.path.push(point(to, "to")?);
                    }
                }
                (None, None, Some(to)) => {
                    let start = spec.path[0];
                    spec.path = vec![start, point(to, "to")?];
                }
                (None, None, None) => {}
            }
            spec.width = a.width.unwrap_or(spec.width);
            spec.strength = a.strength.unwrap_or(spec.strength);
            spec.shadows = a.shadows.unwrap_or(spec.shadows);
            if let Some(c) = a.color {
                spec.color = match c.trim().to_ascii_lowercase().as_str() {
                    "palette" | "none" | "" => None,
                    _ => Some(parse_color(c)?),
                };
            }
            let wants_cast = a.cast_shadow.unwrap_or(
                spec.cast.is_some()
                    || a.cast_length.is_some()
                    || a.cast_opacity.is_some()
                    || a.cast_color.is_some(),
            );
            spec.cast = if wants_cast {
                let mut c = spec
                    .cast
                    .clone()
                    .unwrap_or_else(|| light::CastShadow::new("ground shadow"));
                c.length = a.cast_length.unwrap_or(c.length);
                c.opacity = a.cast_opacity.unwrap_or(c.opacity);
                if let Some(k) = a.cast_color {
                    c.color = match k.trim().to_ascii_lowercase().as_str() {
                        "auto" | "darkest" | "" => None,
                        _ => Some(parse_color(k)?),
                    };
                }
                Some(c)
            } else {
                None
            };
            Some(spec)
        } else {
            if before.auto_light().is_none() {
                return Err(format!("'{name}' has no automatic light"));
            }
            None
        };
        let keep = action == "freeze";
        let s = Self::sprite_mut(&mut p, r)?;
        match spec.clone() {
            Some(spec) => s.set_auto_light(spec).map_err(e)?,
            None => s.stop_light(keep).map_err(e)?,
        }
        s.refresh_outline().map_err(e)?;
        let frames: Vec<usize> = (0..s.frames().len().min(16)).collect();
        let png = render_plain(s, &frames, None)?;
        let layers = s.layer_names().join(", ");
        let after = s.clone();
        self.save(a.project, &mut p)?;
        self.remember(a.project, r, before, after)?;
        let text = match spec {
            Some(spec) => format!(
                "'{name}' now has automatic light and shadow: {}. Layers '{}' (lit edges, one palette step lighter) and '{}' (edges facing away, one step darker){} are redrawn from the other visible layers after every change, in every frame; keep drawing on the other layers. Layers bottom to top: {layers}. `undo` takes this back.",
                describe_light(&spec, after_frames(&p, r)),
                spec.light_layer,
                spec.shadow_layer,
                spec.cast
                    .as_ref()
                    .map_or(String::new(), |c| format!(", plus '{}' at the bottom with the ground shadow", c.layer)),
            ),
            None if keep => format!(
                "The light of '{name}' is now made of normal layers: they stay as they are and can be edited by hand, but no longer follow changes."
            ),
            None => format!("Removed the automatic light and its layers from '{name}'."),
        };
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    // ---------------------------------------------------------------- Autotile

    /// Autotile: Vorlage anlegen, Satz bauen, Satz auf einer Beispielkarte zeigen.
    pub fn autotile(&self, a: &AutotileParams) -> Res {
        let action = a
            .action
            .as_deref()
            .unwrap_or("build")
            .trim()
            .to_ascii_lowercase();
        let (_lock, mut p) = self.load_locked(&a.project)?;
        match action.as_str() {
            "template" => {
                let tile = a.tile.unwrap_or(16);
                let name = Self::check_new_name(&p, a.name.as_deref().unwrap_or("Terrain"))?;
                let depth = Self::depth(a.color_depth_bits)?;
                let (pal, _) = Self::palette_for_new_styled(
                    &p,
                    a.palette_preset.as_deref(),
                    None,
                    depth,
                    None,
                )?;
                let s = autotile::template(tile, depth, pal).map_err(e)?;
                let (w, h) = (s.width(), s.height());
                let png = render_plain(&s, &[0], None)?;
                p.add_image(name.clone(), s);
                self.save(&a.project, &mut p)?;
                let q = tile / 2;
                Ok(Reply {
                    text: format!(
                        "Created the autotile template '{name}' ({w}×{h}: 2 tiles of {tile} px wide, 3 tall). Draw on layer 'Terrain' (the bottom layer is only a guide and is not exported):\n\
                         - Bottom 2×2 tiles (x 0-{w1}, y {tile}-{h1}): one block of terrain as an island, with its outer corners at the four corners, its edges along the four sides and plain terrain inside. Keep the edge within the outer {q} px of the block; the middle {tile}×{tile} is the fill and must tile seamlessly.\n\
                         - Top right tile (x {tile}-{w1}, y 0-{t1}): terrain everywhere except the four inner corners: in each corner the edge turns inwards, so it continues the left/right edge coming from above or below and the top/bottom edge coming from the side.\n\
                         - Top left tile: free, e.g. a preview of the terrain; it is not used.\n\
                         Every tile is built from {q}×{q} quarters of these. Then call autotile with action \"build\" and asset '{name}'. Several frames give animated tiles (e.g. water with a color cycle).",
                        w1 = w - 1,
                        h1 = h - 1,
                        t1 = tile - 1,
                    ),
                    image_png: Some(png),
                })
            }
            "build" => {
                let src_name = a
                    .asset
                    .as_deref()
                    .ok_or("`build` needs `asset`: the template image")?;
                let r = Self::resolve(&p, src_name)?;
                let template_name = Self::asset_name(&p, r);
                let src = Self::sprite(&p, r)?.clone();
                let kind = match &a.kind {
                    Some(k) => TileSetKind::parse(k)
                        .ok_or_else(|| format!("unknown kind '{k}', use blob47 or sides16"))?,
                    None => TileSetKind::Blob47,
                };
                let (mut set, mut info) = autotile::build(&src, kind).map_err(e)?;
                info.source = Some(template_name.clone());
                set.set_tileset(Some(info.clone()));
                let set_name = a
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("{template_name} tiles"));
                let existing = p
                    .images
                    .iter()
                    .find(|i| workspace::name_key(&i.name) == workspace::name_key(&set_name))
                    .map(|i| (i.id, i.sprite.tileset().is_some()));
                let verb = match existing {
                    Some((_, false)) => {
                        return Err(format!(
                            "there is already an image '{set_name}' that is not a tile set; give another `name`"
                        ))
                    }
                    Some((id, true)) => {
                        let r = AssetRef::Image(id);
                        let before = Self::sprite(&p, r)?.clone();
                        *Self::sprite_mut(&mut p, r)? = set.clone();
                        self.save(&a.project, &mut p)?;
                        self.remember(&a.project, r, before, set.clone())?;
                        "Rebuilt"
                    }
                    None => {
                        let name = Self::check_new_name(&p, &set_name)?;
                        p.add_image(name, set.clone());
                        self.save(&a.project, &mut p)?;
                        "Built"
                    }
                };
                let png = tileset_preview(&set, &info, 0, a.seed.unwrap_or(1)).map_err(e)?;
                Ok(Reply {
                    text: format!(
                        "{verb} the tile set '{set_name}' from '{template_name}': {} tiles of {t}×{t} ({}), {} columns, {} frame(s). Preview: the set, and a sample map built with it. Rebuild after changing the template; export it with export format \"godot\" (TileSet .tres with terrain, ready for painting terrains in a TileMapLayer) or \"tiled\" (.tsx with a Wang set).",
                        info.masks.len(),
                        kind.id(),
                        info.columns,
                        set.frames().len(),
                        t = info.tile,
                    ),
                    image_png: Some(png),
                })
            }
            "preview" => {
                let name = a
                    .asset
                    .as_deref()
                    .ok_or("`preview` needs `asset`: the tile set image")?;
                let r = Self::resolve(&p, name)?;
                let s = Self::sprite(&p, r)?;
                let info = s.tileset().ok_or_else(|| {
                    format!("'{name}' is not a tile set; build one with action \"build\"")
                })?;
                let png = tileset_preview(s, info, 0, a.seed.unwrap_or(1)).map_err(e)?;
                Ok(Reply {
                    text: format!(
                        "Tile set '{name}' ({} tiles, {}) and a sample map. Another `seed` gives another map.",
                        info.masks.len(),
                        info.kind.id()
                    ),
                    image_png: Some(png),
                })
            }
            other => Err(format!(
                "unknown action '{other}', use \"template\", \"build\" or \"preview\""
            )),
        }
    }

    // ---------------------------------------------------------------- Effekte

    /// Effekte über die Frames: einrichten, ändern, auflisten, entfernen.
    pub fn rotate(&self, a: &RotateParams) -> Res {
        let (_lock, mut p) = self.load_locked(&a.project)?;
        let r = Self::resolve(&p, &a.asset)?;
        let name = Self::asset_name(&p, r);
        let before = Self::sprite(&p, r)?.clone();
        let expand = a.expand.unwrap_or(false);
        let spin = a.spin.unwrap_or(false);
        if expand && !matches!(r, AssetRef::Image(_)) {
            return Err("characters and animations keep their size; leave out `expand` (or resize the canvas first with resize_canvas)".into());
        }
        if a.frames.is_some() && !spin {
            return Err("`frames` goes with `spin`".into());
        }
        if a.frames.is_some() && matches!(r, AssetRef::Character(_)) {
            return Err(
                "a character's base pose keeps its frames; spin an animation or an image".into(),
            );
        }
        if let Some(n) = a.frames {
            if !(2..=64).contains(&n) {
                return Err("`frames` must be 2 to 64".into());
            }
        }
        let mut o = RotateOptions::new(a.angle);
        o.layer = match &a.layer {
            Some(l) => Some(Self::layer_index(&before, Some(l))?),
            None => None,
        };
        o.frame = match a.frame {
            Some(_) => Some(Self::frame_index(&before, a.frame)?),
            None => None,
        };
        o.pivot = a.pivot.map(|[x, y]| (x, y));
        o.expand = expand;
        o.spin = spin;
        o.frames = a.frames.map(|n| n as usize);
        let s = Self::sprite_mut(&mut p, r)?;
        s.rotate(&o).map_err(e)?;
        let after = s.clone();
        let frames: Vec<usize> = match o.frame {
            Some(f) => vec![f],
            None => (0..after.frames().len()).collect(),
        };
        let (png, _) = render(&after, &frames, None, &ViewOptions::default())?;
        self.save(&a.project, &mut p)?;
        self.remember(&a.project, r, before.clone(), after.clone())?;
        let layers = match o.layer {
            Some(l) => format!("layer '{}'", after.layer_names()[l]),
            None => "all drawn layers".to_string(),
        };
        let how = if spin {
            format!(
                "spun by {}° over {} frames ({}° per frame)",
                a.angle,
                after.frames().len(),
                a.angle / after.frames().len() as f32
            )
        } else {
            format!("rotated by {}°", a.angle)
        };
        let mut text = format!(
            "'{name}' {how} with RotSprite, {layers}, {}. No new colors; outline, light and effects were redrawn.",
            match frames.as_slice() {
                [f] if after.frames().len() > 1 => format!("frame {f}"),
                [_] => "the only frame".to_string(),
                _ => format!("all {} frames", frames.len()),
            }
        );
        if (after.width(), after.height()) != (before.width(), before.height()) {
            text += &format!(
                " The canvas grew from {}×{} to {}×{}.",
                before.width(),
                before.height(),
                after.width(),
                after.height()
            );
        }
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    pub fn variants(&self, a: &VariantsParams) -> Res {
        let action = a
            .action
            .as_deref()
            .unwrap_or("list")
            .trim()
            .to_ascii_lowercase();
        let (_lock, mut p) = self.load_locked(&a.project)?;
        let r = Self::resolve(&p, &a.character)?;
        let ch = Self::character_of(&p, r)
            .ok_or(
                "color variants belong to characters; give a character or one of its animations",
            )?
            .clone();
        let colors = ch.base.used_colors();
        let describe = |c: &Character| -> Value {
            json!(c
                .variants
                .iter()
                .map(|v| json!({
                    "name": v.name,
                    "colors": v
                        .colors
                        .iter()
                        .map(|(f, t)| (f.to_string(), Value::String(t.to_string())))
                        .collect::<serde_json::Map<_, _>>(),
                }))
                .collect::<Vec<_>>())
        };
        match action.as_str() {
            "list" => Ok(Reply::json(json!({
                "character": ch.name,
                "colors": colors.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                "variants": describe(&ch),
                "note": "Variants apply to the character and all its animations when previewing and exporting (export with `variant`); drawing on the character keeps every variant up to date.",
            }))),
            "set" => {
                let name = a.name.as_deref().ok_or("`set` needs `name`")?;
                let mut v = match ch.variant(name) {
                    Some(old) if !a.replace.unwrap_or(false) => old.clone(),
                    _ => ColorVariant::new(name.trim()),
                };
                v.name = name.trim().to_string();
                if a.colors.is_none() && a.hue.is_none() {
                    return Err("`set` needs `colors` ({\"#from\": \"#to\"}) or `hue`".into());
                }
                if a.hue_colors.is_some() && a.hue.is_none() {
                    return Err("`hue_colors` goes with `hue` (degrees)".into());
                }
                if let Some(deg) = a.hue {
                    let list = match &a.hue_colors {
                        Some(list) => list
                            .iter()
                            .map(|c| parse_color(c))
                            .collect::<Res<Vec<_>>>()?,
                        None => colors
                            .iter()
                            .copied()
                            .filter(|c| variants::hue_shift(*c, 90.0) != *c)
                            .collect(),
                    };
                    v.shift_hue(&list, deg);
                }
                let mut unused = Vec::new();
                for (from, to) in a.colors.iter().flatten() {
                    let (f, t) = (parse_color(from)?, parse_color(to)?);
                    if !colors.iter().any(|c| (c.r, c.g, c.b) == (f.r, f.g, f.b)) {
                        unused.push(from.clone());
                    }
                    v.set(f, t);
                }
                let id = ch.id;
                let vname = v.name.clone();
                p.character_mut(id).map_err(e)?.set_variant(v).map_err(e)?;
                self.save(&a.project, &mut p)?;
                let ch = p.character(id).map_err(e)?;
                let png = variants_preview(&p, ch, Some(&vname))?;
                let mut text = format!(
                    "Variant '{vname}' of '{}' saved. It applies to the character and all its animations; export with `variant`.",
                    ch.name
                );
                if !unused.is_empty() {
                    text += &format!(
                        " Note: {} not used by the character (its colors: {}).",
                        unused.join(", "),
                        colors
                            .iter()
                            .map(|c| c.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                Ok(Reply {
                    text,
                    image_png: Some(png),
                })
            }
            "remove" => {
                let name = a.name.as_deref().ok_or("`remove` needs `name`")?;
                let id = ch.id;
                if !p.character_mut(id).map_err(e)?.remove_variant(name) {
                    return Err(format!(
                        "'{}' has no variant '{name}'. Variants: {}",
                        ch.name,
                        variant_names(&ch)
                    ));
                }
                self.save(&a.project, &mut p)?;
                Ok(Reply::text(format!(
                    "Removed the variant '{name}' of '{}'.",
                    ch.name
                )))
            }
            "preview" => {
                if ch.variants.is_empty() {
                    return Err(format!(
                        "'{}' has no color variants yet; create one with action \"set\"",
                        ch.name
                    ));
                }
                if let Some(n) = a.name.as_deref() {
                    if ch.variant(n).is_none() {
                        return Err(format!(
                            "'{}' has no variant '{n}'. Variants: {}",
                            ch.name,
                            variant_names(&ch)
                        ));
                    }
                }
                let png = variants_preview(&p, &ch, a.name.as_deref())?;
                Ok(Reply {
                    text: format!(
                        "Rows: original, then {}. Columns: base pose, then the first frame of each animation.",
                        match a.name.as_deref() {
                            Some(n) => format!("'{n}'"),
                            None => variant_names(&ch),
                        }
                    ),
                    image_png: Some(png),
                })
            }
            other => Err(format!(
                "unknown action '{other}', use list, set, remove or preview"
            )),
        }
    }

    pub fn effects(&self, a: &EffectsParams) -> Res {
        let action = a
            .action
            .as_deref()
            .unwrap_or("add")
            .trim()
            .to_ascii_lowercase();
        if !matches!(action.as_str(), "add" | "list" | "remove" | "freeze") {
            return Err(format!(
                "unknown action '{action}', use \"add\", \"list\", \"remove\" or \"freeze\""
            ));
        }
        let (_lock, mut p) = self.load_locked(&a.project)?;
        let r = Self::resolve(&p, &a.asset)?;
        let name = Self::asset_name(&p, r);
        let before = Self::sprite(&p, r)?.clone();
        if action == "list" {
            return Ok(Reply::json(json!({
                "asset": name,
                "frames": before.frames().len(),
                "effects": before.effects(),
                "layers": before.layer_names(),
                "types": EffectKind::IDS,
            })));
        }
        let wanted_type = a.kind.as_deref().map(|t| t.trim().to_ascii_lowercase());
        // Die Ebene des gemeinten Effekts: per Name oder, wenn eindeutig, per Art.
        let existing = match (&a.layer, &wanted_type) {
            (Some(l), _) => before.effect(l).map(|e| e.layer.clone()),
            (None, Some(t)) if action != "add" => {
                let same: Vec<&AutoEffect> = before
                    .effects()
                    .iter()
                    .filter(|e| e.kind.id() == t)
                    .collect();
                match same.as_slice() {
                    [one] => Some(one.layer.clone()),
                    [] => None,
                    _ => {
                        return Err(format!(
                            "'{name}' has several {t} effects; name one with `layer`: {}",
                            same.iter()
                                .map(|e| e.layer.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    }
                }
            }
            _ => None,
        };
        if action != "add" {
            let Some(layer) = existing else {
                return Err(format!(
                    "no such effect on '{name}'. Effects: {}",
                    list_or_none(before.effects().iter().map(|e| e.layer.as_str()))
                ));
            };
            let keep = action == "freeze";
            let s = Self::sprite_mut(&mut p, r)?;
            s.remove_effect(&layer, keep).map_err(e)?;
            s.refresh_outline().map_err(e)?;
            let after = s.clone();
            let frames: Vec<usize> = (0..after.frames().len().min(16)).collect();
            let png = render_plain(&after, &frames, None)?;
            self.save(&a.project, &mut p)?;
            self.remember(&a.project, r, before, after)?;
            return Ok(Reply {
                text: if keep {
                    format!("The effect on layer '{layer}' of '{name}' is now a normal layer: it stays as it is and can be edited, but no longer follows changes.")
                } else {
                    format!("Removed the effect '{layer}' and its layer from '{name}'.")
                },
                image_png: Some(png),
            });
        }

        // Hinzufügen oder ändern.
        let character = matches!(r, AssetRef::Character(_));
        let mut s = before.clone();
        let mut notes: Vec<String> = Vec::new();
        if let Some(n) = a.frames {
            if character && n > 1 {
                return Err("a character's base pose has one frame; add moving effects to one of its animations (or to an image)".into());
            }
            if !(1..=fx::MAX_FRAMES as u32).contains(&n) {
                return Err(format!("`frames` must be 1 to {}", fx::MAX_FRAMES));
            }
            let added = s.ensure_frames(n as usize).map_err(e)?;
            if added > 0 {
                notes.push(format!(
                    "Added {added} frames (copies of the existing ones) so the effect can move."
                ));
            }
        }
        let (w, h) = (s.width(), s.height());
        let mut spec = match &existing {
            Some(l) => {
                let old = s.effect(l).cloned().ok_or("effect vanished")?;
                match &wanted_type {
                    Some(t) if t != old.kind.id() => AutoEffect {
                        kind: EffectKind::default_for(t, w, h).ok_or_else(|| unknown_effect(t))?,
                        ..old
                    },
                    _ => old,
                }
            }
            None => {
                let t = wanted_type
                    .as_deref()
                    .ok_or_else(|| format!("`add` needs `type`: {}", EffectKind::IDS.join(", ")))?;
                let kind = EffectKind::default_for(t, w, h).ok_or_else(|| unknown_effect(t))?;
                let layer = match &a.layer {
                    Some(l) if !l.trim().is_empty() => l.trim().to_string(),
                    _ => s.free_layer_name(&kind.id().replace('_', " ")),
                };
                AutoEffect {
                    layer,
                    kind,
                    seed: 0,
                }
            }
        };
        if let Some(seed) = a.seed {
            spec.seed = seed;
        }
        let colors = |v: &Option<Vec<String>>| -> Res<Option<Vec<Rgba>>> {
            v.as_ref()
                .map(|v| v.iter().map(|c| parse_color(c)).collect::<Res<Vec<_>>>())
                .transpose()
        };
        let pt = |v: Option<[f32; 2]>| v.map(|[x, y]| (x, y));
        let mut unused: Vec<&str> = Vec::new();
        let mut check_unused = |used: &[&str]| {
            let given = [
                ("color", a.color.is_some()),
                ("colors", a.colors.is_some()),
                ("preset", a.preset.is_some()),
                ("radius", a.radius.is_some()),
                ("pulse", a.pulse.is_some()),
                ("dither", a.dither.is_some()),
                ("from", a.from.is_some()),
                ("to", a.to.is_some()),
                ("at", a.at.is_some()),
                ("glow_color", a.glow_color.is_some()),
                ("branches", a.branches.is_some()),
                ("flicker", a.flicker.is_some()),
                ("start", a.start.is_some()),
                ("length", a.length.is_some()),
                ("line", a.line.is_some()),
                ("mix", a.mix.is_some()),
                ("opacity", a.opacity.is_some()),
                ("wave", a.wave.is_some()),
                ("grow", a.grow.is_some()),
                ("spread", a.spread.is_some()),
                ("count", a.count.is_some()),
                ("speed", a.speed.is_some()),
                ("reverse", a.reverse.is_some()),
            ];
            for (k, set) in given {
                if set && !used.contains(&k) {
                    unused.push(k);
                }
            }
        };
        let mut grow_to: Option<u32> = None;
        match &mut spec.kind {
            EffectKind::Glow {
                color,
                radius,
                pulse,
                colors: only,
                dither,
            } => {
                check_unused(&["color", "colors", "radius", "pulse", "dither"]);
                if let Some(c) = &a.color {
                    *color = parse_color(c)?;
                }
                *radius = a.radius.unwrap_or(*radius);
                *pulse = a.pulse.unwrap_or((*pulse).min(radius.saturating_sub(1)));
                if let Some(c) = colors(&a.colors)? {
                    *only = c;
                }
                *dither = a.dither.unwrap_or(*dither);
            }
            EffectKind::Lightning {
                from,
                to,
                color,
                glow,
                branches,
                flicker,
            } => {
                check_unused(&["color", "from", "to", "glow_color", "branches", "flicker"]);
                *from = pt(a.from).unwrap_or(*from);
                *to = pt(a.to).unwrap_or(*to);
                if let Some(c) = &a.color {
                    *color = parse_color(c)?;
                }
                if let Some(g) = &a.glow_color {
                    *glow = match g.trim().to_ascii_lowercase().as_str() {
                        "none" | "" => None,
                        _ => Some(parse_color(g)?),
                    };
                }
                *branches = a.branches.unwrap_or(*branches);
                *flicker = a.flicker.unwrap_or(*flicker);
            }
            EffectKind::Explosion {
                at,
                radius,
                start,
                length,
                colors: ramp,
            } => {
                check_unused(&["at", "radius", "start", "length", "colors"]);
                *at = pt(a.at).unwrap_or(*at);
                *radius = a.radius.unwrap_or(*radius);
                *start = a.start.unwrap_or(*start);
                *length = a.length.unwrap_or(*length);
                if let Some(c) = colors(&a.colors)? {
                    *ramp = c;
                }
                if s.frames().len() == 1 {
                    notes.push("With one frame you see a single moment of the explosion; give `frames` (e.g. 8) for the whole blast.".into());
                }
            }
            EffectKind::Reflection {
                line,
                color,
                mix,
                opacity,
                wave,
            } => {
                check_unused(&["line", "color", "mix", "opacity", "wave", "grow"]);
                if a.line.is_some() {
                    *line = a.line;
                }
                if let Some(c) = &a.color {
                    *color = parse_color(c)?;
                }
                *mix = a.mix.unwrap_or(*mix);
                *opacity = a.opacity.unwrap_or(*opacity);
                *wave = a.wave.unwrap_or(*wave);
                // Passt das Spiegelbild nicht, wird ein Bild nach unten größer.
                if let Some(need) = s.reflection_height(*line) {
                    if need > h {
                        let image = matches!(r, AssetRef::Image(_));
                        if image && a.grow.unwrap_or(true) {
                            grow_to = Some(need.min(pixel_core::MAX_SIZE));
                        } else {
                            notes.push(format!(
                                "The canvas is {h} pixels high but the whole reflection needs {need}; it is cut off.{}",
                                if image { "" } else { " Make the canvas taller (resize_canvas) for the whole reflection." }
                            ));
                        }
                    }
                }
            }
            EffectKind::Particles {
                at,
                spread,
                count,
                speed,
                colors: ramp,
                ..
            } => {
                check_unused(&["at", "spread", "count", "speed", "colors"]);
                *at = pt(a.at).unwrap_or(*at);
                *spread = a.spread.unwrap_or(*spread);
                *count = a.count.unwrap_or(*count);
                *speed = a.speed.unwrap_or(*speed);
                if let Some(c) = colors(&a.colors)? {
                    *ramp = c;
                }
            }
            EffectKind::ColorCycle {
                colors: ramp,
                speed,
                reverse,
            } => {
                check_unused(&["colors", "preset", "speed", "reverse"]);
                match (colors(&a.colors)?, &a.preset) {
                    (Some(_), Some(_)) => {
                        return Err("give either `colors` or `preset`, not both".into())
                    }
                    (Some(c), None) => *ramp = c,
                    (None, Some(name)) => {
                        *ramp = fx::cycle_preset(name).ok_or_else(|| {
                            format!(
                                "unknown preset '{name}', use {}",
                                fx::CYCLE_PRESETS.join(", ")
                            )
                        })?
                    }
                    (None, None) if existing.is_none() => {
                        return Err("color_cycle needs `colors` (the ramp, in order, colors that are in the picture) or `preset` (water, fire, lava)".into())
                    }
                    (None, None) => {}
                }
                if let Some(v) = a.speed {
                    if v.fract() != 0.0 || !(1.0..=16.0).contains(&v) {
                        return Err(
                            "for color_cycle `speed` is frames per step, a whole number 1 to 16"
                                .into(),
                        );
                    }
                    *speed = v as u32;
                }
                *reverse = a.reverse.unwrap_or(*reverse);
                let used = s.used_colors();
                let ramp_now: Vec<Rgba> = ramp.iter().map(|&c| s.conform(c)).collect();
                if !ramp_now.iter().any(|c| used.contains(c)) {
                    notes.push(format!(
                        "None of the cycle colors are in the picture yet. Paint the area that should flow with them: {}.",
                        ramp_now.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")
                    ));
                }
            }
        }
        if !unused.is_empty() {
            notes.push(format!(
                "Ignored for {}: {}.",
                spec.kind.id(),
                unused.join(", ")
            ));
        }
        if let Some(new_h) = grow_to {
            s.resize_canvas(w, new_h, 0, 0).map_err(e)?;
            notes.push(format!(
                "The canvas grew from {w}×{h} to {w}×{new_h} so the reflection fits."
            ));
        }
        s.set_effect(spec.clone()).map_err(e)?;
        s.refresh_outline().map_err(e)?;
        if s.frames().len() == 1 && spec.kind.animates() {
            notes.push("There is only one frame, so nothing moves yet: call again with `frames` (e.g. 8) or use an animation.".into());
        }
        if s.palette.locked {
            notes.push(
                "The palette is locked, so the effect uses the nearest palette colors.".into(),
            );
        }
        let after = s.clone();
        *Self::sprite_mut(&mut p, r)? = s;
        let frames: Vec<usize> = (0..after.frames().len().min(16)).collect();
        let png = render_plain(&after, &frames, None)?;
        self.save(&a.project, &mut p)?;
        self.remember(&a.project, r, before, after.clone())?;
        let verb = if existing.is_some() {
            "Changed"
        } else {
            "Added"
        };
        let mut text = format!(
            "{verb} the {} effect on layer '{}' of '{name}' ({} frames). It is redrawn after every change in every frame; keep drawing on the other layers. Layers bottom to top: {}. Settings: {}",
            spec.kind.id(),
            spec.layer,
            after.frames().len(),
            after.layer_names().join(", "),
            serde_json::to_string(&spec).unwrap_or_default(),
        );
        for n in notes {
            text.push(' ');
            text.push_str(&n);
        }
        text.push_str(" `undo` takes this back.");
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    // ---------------------------------------------------------------- Export

    #[allow(clippy::too_many_arguments)]
    pub fn export(
        &self,
        project: &str,
        asset: &str,
        format: &str,
        frame: Option<usize>,
        scale: Option<u32>,
        columns: Option<u32>,
        background: Option<&str>,
        quality: Option<u8>,
        normal_map: Option<&NormalMapOpts>,
        variant: Option<&str>,
    ) -> Res {
        let p = self.load(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite(&p, r)?;
        let name = Self::asset_name(&p, r);
        let Some(wanted) = variant.map(str::trim).filter(|v| !v.is_empty()) else {
            return self.export_sprite(
                project, &p, r, &name, s, format, frame, scale, columns, background, quality,
                normal_map,
            );
        };
        let character = Self::character_of(&p, r).ok_or(
            "color variants belong to characters; `variant` works for a character or one of its animations",
        )?;
        let variants: Vec<&ColorVariant> = if wanted.eq_ignore_ascii_case("all") {
            character.variants.iter().collect()
        } else {
            vec![character.variant(wanted).ok_or_else(|| {
                format!(
                    "'{}' has no variant '{wanted}'. Variants: {}",
                    character.name,
                    variant_names(character)
                )
            })?]
        };
        if variants.is_empty() {
            return Err(format!(
                "'{}' has no color variants yet; create one with the tool variants",
                character.name
            ));
        }
        let mut files = Vec::new();
        let mut last = None;
        for v in variants {
            let turned = v.apply(s);
            let reply = self.export_sprite(
                project,
                &p,
                r,
                &format!("{name} {}", v.name),
                &turned,
                format,
                frame,
                scale,
                columns,
                background,
                quality,
                normal_map,
            )?;
            let mut value: Value = serde_json::from_str(&reply.text).map_err(e)?;
            for key in ["files", "normal_map_files"] {
                if let Some(Value::Array(list)) = value.get_mut(key).map(Value::take) {
                    files.extend(list);
                }
            }
            last = Some((value, reply.image_png));
        }
        let (mut value, image) = last.unwrap_or_default();
        value["files"] = Value::Array(files);
        if let Some(obj) = value.as_object_mut() {
            obj.remove("normal_map_files");
            obj.remove("sheet");
        }
        Ok(Reply {
            text: serde_json::to_string_pretty(&value).unwrap_or_default(),
            image_png: image,
        })
    }

    /// Die Figur zu einer Figur oder Animation.
    fn character_of(p: &Project, r: AssetRef) -> Option<&Character> {
        let id = match r {
            AssetRef::Character(id) => id,
            AssetRef::Animation(id) => p.animation(id).ok()?.character_id,
            AssetRef::Image(_) => return None,
        };
        p.character(id).ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn export_sprite(
        &self,
        project: &str,
        p: &Project,
        r: AssetRef,
        name: &str,
        s: &Sprite,
        format: &str,
        frame: Option<usize>,
        scale: Option<u32>,
        columns: Option<u32>,
        background: Option<&str>,
        quality: Option<u8>,
        normal_map: Option<&NormalMapOpts>,
    ) -> Res {
        let scale = scale.unwrap_or(1);
        if !(1..=export::MAX_SCALE).contains(&scale) {
            return Err(format!("scale must be 1 to {}", export::MAX_SCALE));
        }
        let frame = match frame {
            Some(_) => Some(Self::frame_index(s, frame)?),
            None => None,
        };
        let engine = match format.to_ascii_lowercase().as_str() {
            "godot" => Some(TileEngine::Godot),
            "tiled" => Some(TileEngine::Tiled),
            _ => None,
        };
        if let Some(engine) = engine {
            if normal_map.is_some() {
                return Err("normal maps are exported as png or sheet only".into());
            }
            let files = self
                .ws
                .export_tileset(project, name, s, engine, frame, scale)
                .map_err(e)?;
            let animated = frame.is_none() && s.frames().len() > 1;
            let note = match engine {
                TileEngine::Godot => "Copy both files into your Godot 4 project (same folder). Load the .tres as the tile_set of a TileMapLayer; the terrain is ready in terrain set 0, so paint with the Terrains tab and the right tiles join up by themselves.",
                TileEngine::Tiled => "Keep both files in the same folder. In Tiled, add the .tsx to the map and paint with the Terrain Brush of its Wang set.",
            };
            return Ok(Reply::json(json!({
                "files": files,
                "animated": animated,
                "note": if animated {
                    format!("{note} The tiles are animated: each tile's frames lie side by side.")
                } else {
                    note.to_string()
                },
            })));
        }
        let format = match format.to_ascii_lowercase().as_str() {
            "png" => ExportFormat::Png,
            "jpg" | "jpeg" => ExportFormat::Jpg {
                background: match background {
                    Some(c) => parse_color(c)?,
                    None => Rgba::WHITE,
                },
                quality: quality.unwrap_or(95),
            },
            "sheet" | "spritesheet" => ExportFormat::Sheet {
                columns: columns.unwrap_or(0),
            },
            "png8" | "indexed" | "indexed_png" => ExportFormat::PngIndexed,
            "gif" => ExportFormat::Gif {
                // Eine Animation spielt so ab, wie sie im Projekt eingestellt ist.
                looping: match r {
                    AssetRef::Animation(id) => p.animation(id).map_err(e)?.looping,
                    _ => true,
                },
            },
            "webm" => ExportFormat::WebM,
            other => {
                return Err(format!(
                    "unknown format '{other}', use png, png8, jpg, sheet, gif or webm (godot or tiled for an autotile set)"
                ))
            }
        };
        if format.is_whole_animation() && frame.is_some() {
            return Err("this format always contains all frames, omit `frame`".into());
        }
        if normal_map.is_some() && !matches!(format, ExportFormat::Png | ExportFormat::Sheet { .. })
        {
            return Err("normal maps are exported as png or sheet only".into());
        }
        // Erst alles prüfen, dann schreiben: Falsche Angaben sollen keine halben Exporte hinterlassen.
        let normal_options = normal_map.map(|nm| NormalOptions {
            bevel: nm.bevel.unwrap_or(3),
            strength: nm.strength.unwrap_or(1.0),
            detail: nm.detail.unwrap_or(0.3),
            flip_green: nm.directx.unwrap_or(false),
        });
        if let Some(o) = &normal_options {
            o.check().map_err(e)?;
        }
        let out = self
            .ws
            .export(project, name, s, format, frame, scale)
            .map_err(|err| match err {
                pixel_core::Error::TooManyColors { found, max } => format!(
                    "png8 holds at most {max} colors, this frame has {found}. Use png instead."
                ),
                pixel_core::Error::OddVideoScale => format!(
                    "webm needs an even scale (2, 4, 6 ... {}) so that pixel edges stay sharp",
                    export::MAX_SCALE
                ),
                pixel_core::Error::FfmpegMissing => "webm needs the free program ffmpeg, which was not found. Ask the user to install it: in PixelArt Studio open the export dialog (Ctrl+E), choose WebM and click \"ffmpeg jetzt installieren\"; or run `winget install Gyan.FFmpeg` in a terminal; or put ffmpeg.exe next to the program. No restart is needed afterwards. gif and sheet work without it.".to_string(),
                other => e(other),
            })?;
        let mut reply = json!({ "files": out.files });
        if let Some(meta) = out.sheet {
            reply["sheet"] = json!(meta);
        }
        if let Some(info) = out.gif {
            reply["gif"] = json!(info);
            if !info.exact {
                reply["note"] = json!("The frames have more colors than a GIF can hold (255 plus transparency), so they were reduced to similar colors. Use sheet or png for exact pixels.");
            }
        }
        if matches!(format, ExportFormat::WebM) {
            reply["note"] = json!("Video compression can shift colors by one or two steps. Use sheet, png or gif for exact pixels.");
        }
        let Some(o) = normal_options else {
            return Ok(Reply::json(reply));
        };
        let files = self
            .ws
            .export_normals(project, name, s, &o, format, frame, scale)
            .map_err(e)?;
        reply["normal_map_files"] = json!(files);
        reply["normal_map_note"] = json!(format!(
            "Normal maps ({} convention) next to the color files, with the suffix _n. Godot: put them into the normal_map of a CanvasTexture; Unity: add them as secondary texture _NormalMap of the sprite. Preview: left lit from the top left, right the normal map.",
            if o.flip_green { "DirectX" } else { "OpenGL" }
        ));
        let png = normal_preview(s, frame.unwrap_or(0), &o).map_err(e)?;
        Ok(Reply {
            text: serde_json::to_string_pretty(&reply).unwrap_or_default(),
            image_png: Some(png),
        })
    }
}

fn variant_names(c: &Character) -> String {
    if c.variants.is_empty() {
        return "none".into();
    }
    c.variants
        .iter()
        .map(|v| format!("'{}'", v.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Vorschau der Farbvarianten: je Zeile Original oder Variante, je Spalte die
/// Grundpose und der erste Frame jeder Animation (höchstens sieben).
fn variants_preview(p: &Project, c: &Character, only: Option<&str>) -> Res<Vec<u8>> {
    let mut sprites = vec![&c.base];
    sprites.extend(p.animations_of(c.id).map(|a| &a.sprite).take(7));
    let rows: Vec<Option<&ColorVariant>> = std::iter::once(None)
        .chain(
            c.variants
                .iter()
                .filter(|v| {
                    only.is_none_or(|n| workspace::name_key(n) == workspace::name_key(&v.name))
                })
                .map(Some),
        )
        .collect();
    let (w, h) = (c.base.width(), c.base.height());
    let gap = 2;
    let (cw, ch) = (w + gap, h + gap);
    let iw = cw * sprites.len() as u32 + gap;
    let ih = ch * rows.len() as u32 + gap;
    let scale = (640 / iw.max(ih)).clamp(1, 8);
    let mut img =
        image::RgbaImage::from_pixel(iw * scale, ih * scale, image::Rgba([44, 48, 60, 255]));
    for (ri, v) in rows.iter().enumerate() {
        for (ci, s) in sprites.iter().enumerate() {
            let turned;
            let s = match v {
                Some(v) => {
                    turned = v.apply(s);
                    &turned
                }
                None => *s,
            };
            let px = s.composite(0).map_err(e)?;
            let (sw, sh) = (s.width().min(w), s.height().min(h));
            let (ox, oy) = (gap + ci as u32 * cw, gap + ri as u32 * ch);
            for y in 0..sh {
                for x in 0..sw {
                    let c = px[(y * s.width() + x) as usize];
                    if c.a == 0 {
                        continue;
                    }
                    let bg = image::Rgba([44, 48, 60, 255]);
                    let a = c.a as u32;
                    let mix = |f: u8, b: u8| ((f as u32 * a + b as u32 * (255 - a)) / 255) as u8;
                    let out = image::Rgba([mix(c.r, bg[0]), mix(c.g, bg[1]), mix(c.b, bg[2]), 255]);
                    for dy in 0..scale {
                        for dx in 0..scale {
                            img.put_pixel((ox + x) * scale + dx, (oy + y) * scale + dy, out);
                        }
                    }
                }
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(e)?;
    Ok(out.into_inner())
}

/// Vorschau eines Autotile-Satzes: links der Satz, rechts eine Beispielkarte.
fn tileset_preview(
    set: &Sprite,
    info: &autotile::TileSet,
    frame: usize,
    seed: u64,
) -> pixel_core::Result<Vec<u8>> {
    let (map, mw, mh) = autotile::demo_map(set, info, frame, 12, 9, seed)?;
    let (sw, sh) = (set.width(), set.height());
    let gap = info.tile / 2;
    let (w, h) = (sw + gap + mw, sh.max(mh));
    let (w, h) = (w.min(pixel_core::MAX_SIZE), h.min(pixel_core::MAX_SIZE));
    let mut out = Sprite::new(w, h, ColorDepth::Rgba32, Palette::default())?;
    let px = set.composite(frame)?;
    let water = Rgba::rgb(52, 92, 160);
    let mut all = vec![Rgba::TRANSPARENT; (w * h) as usize];
    for y in 0..sh.min(h) {
        for x in 0..sw.min(w) {
            all[(y * w + x) as usize] = px[(y * sw + x) as usize];
        }
    }
    for y in 0..mh.min(h) {
        for x in 0..mw {
            let tx = sw + gap + x;
            if tx < w {
                all[(y * w + tx) as usize] = map[(y * mw + x) as usize].over(water);
            }
        }
    }
    out.layer_mut(0, 0)?.replace_pixels(all)?;
    let scale = (512 / w.max(h)).clamp(1, 4);
    export::frame_png(&out, 0, scale)
}

/// Vorschau einer Normal Map: links das Bild mit Licht von oben links, rechts die Normal Map.
/// Ist das Bild für beide nebeneinander zu breit, zeigt sie nur das beleuchtete Bild.
fn normal_preview(s: &Sprite, frame: usize, o: &NormalOptions) -> pixel_core::Result<Vec<u8>> {
    let (w, h) = (s.width() as usize, s.height() as usize);
    let color = s.composite(frame)?;
    let map = normalmap::normal_map(&color, w, h, o);
    let lamp = (-(w as f32) * 0.3, -(h as f32) * 0.3, w.max(h) as f32 * 0.8);
    let shaded = normalmap::lit(&color, &map, w, lamp, 0.35, o.flip_green);
    let gap = 2;
    if w * 2 + gap > pixel_core::MAX_SIZE as usize {
        let mut out = Sprite::new(w as u32, h as u32, ColorDepth::Rgba32, Palette::default())?;
        out.layer_mut(0, 0)?.replace_pixels(shaded)?;
        return export::frame_png(&out, 0, 1);
    }
    let width = w * 2 + gap;
    let mut both = vec![Rgba::TRANSPARENT; width * h];
    for y in 0..h {
        both[y * width..y * width + w].copy_from_slice(&shaded[y * w..(y + 1) * w]);
        both[y * width + w + gap..(y + 1) * width].copy_from_slice(&map[y * w..(y + 1) * w]);
    }
    let mut out = Sprite::new(
        width as u32,
        h as u32,
        ColorDepth::Rgba32,
        Palette::default(),
    )?;
    out.layer_mut(0, 0)?.replace_pixels(both)?;
    let scale = (512 / width.max(h)).clamp(1, 16) as u32;
    export::frame_png(&out, 0, scale)
}

// -------------------------------------------------------------------- Helfer

/// Liest `#RRGGBB`, `#RRGGBBAA` oder `transparent`.
pub fn parse_color(s: &str) -> Res<Rgba> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("transparent") || t.eq_ignore_ascii_case("none") {
        return Ok(Rgba::TRANSPARENT);
    }
    t.parse::<Rgba>()
        .map_err(|_| format!("invalid color '{s}': use #RRGGBB, #RRGGBBAA or \"transparent\""))
}

/// Der Stil eines Projekts, wie ihn das Modell zu sehen bekommt.
fn style_json(style: &Style) -> Value {
    let mut v = json!({});
    if !style.palette.is_empty() {
        v["palette"] = json!(style.palette);
    }
    if let Some(dir) = &style.light_dir {
        v["light_dir"] = json!(dir);
    }
    if let Some(c) = style.outline {
        v["outline_color"] = json!(c);
    }
    if !style.notes.is_empty() {
        v["notes"] = json!(style.notes);
    }
    if let Some(name) = &style.preset {
        v["style"] = json!(name);
    }
    v
}

/// Alles über einen Stil der Bibliothek, so wie Claude ihn zum Zeichnen braucht.
fn preset_json(st: &StylePreset) -> Value {
    let mut v = json!({
        "name": st.name,
        "built_in": st.builtin,
        "description": st.description,
        "palette": st.palette,
        "color_depth_bits": st.color_depth,
        "sizes": st.sizes,
        "outline": st.outline.id(),
        "outline_width": st.outline_width,
        "shades": st.shades,
        "dithering": st.dithering,
        "hue_shift": st.hue_shift,
        "anti_alias": st.anti_alias,
        "rules": st.rules,
    });
    if let Some(n) = st.colors_per_sprite {
        v["colors_per_sprite"] = json!(n);
    }
    if let Some(c) = st.outline_color() {
        v["outline_color"] = json!(c);
    }
    if let Some(dir) = &st.light_dir {
        v["light_dir"] = json!(dir);
    }
    v
}

/// Der Name einer Richtung, wie ihn der Stil speichert.
fn unknown_effect(t: &str) -> String {
    format!(
        "unknown effect type '{t}', use {}",
        EffectKind::IDS.join(", ")
    )
}

fn list_or_none<'a>(it: impl Iterator<Item = &'a str>) -> String {
    let v: Vec<&str> = it.collect();
    if v.is_empty() {
        "none".into()
    } else {
        v.join(", ")
    }
}

fn after_frames(p: &Project, r: AssetRef) -> usize {
    Studio::sprite(p, r).map_or(1, |s| s.frames().len())
}

fn describe_light(l: &light::AutoLight, frames: usize) -> String {
    let at = |(x, y): (f32, f32)| format!("[{x:.0}, {y:.0}]");
    let mut parts = vec![if l.path.len() == 1 {
        format!("light source at {}", at(l.path[0]))
    } else {
        format!(
            "light source moving {} over {frames} frame(s)",
            l.path
                .iter()
                .map(|p| at(*p))
                .collect::<Vec<_>>()
                .join(" → ")
        )
    }];
    parts.push(format!(
        "edges {} px wide, strength {}",
        l.width, l.strength
    ));
    parts.push(match l.color {
        Some(c) => format!("light color {c}"),
        None => "lighter along the palette".into(),
    });
    if !l.shadows {
        parts.push("no shadow side".into());
    }
    if let Some(c) = &l.cast {
        parts.push(format!("ground shadow {}% long", c.length));
    }
    parts.join(", ")
}

fn light_json(l: &light::AutoLight) -> Value {
    json!({
        "light_layer": l.light_layer,
        "shadow_layer": l.shadow_layer,
        "path": l.path.iter().map(|(x, y)| [x, y]).collect::<Vec<_>>(),
        "width": l.width,
        "strength": l.strength,
        "color": l.color.map_or("palette".to_string(), |c| c.to_string()),
        "shadows": l.shadows,
        "cast_shadow": l.cast.as_ref().map(|c| json!({"layer": c.layer, "length": c.length, "opacity": c.opacity})),
    })
}

fn describe_outline(o: &AutoOutline) -> String {
    let mut parts = vec![
        match o.color {
            Some(c) => format!("color {c}"),
            None => "selective (a darker shade of the color it touches)".into(),
        },
        if o.inside {
            "inside the shape"
        } else {
            "outside the shape"
        }
        .into(),
        format!("{} px", o.thickness),
    ];
    if o.corners {
        parts.push("with corners".into());
    }
    if o.inner {
        parts.push("with inner lines between layers".into());
    }
    if let (Some((x, y)), None) = (o.light, o.color) {
        parts.push(format!(
            "lighter towards {}",
            direction_name((i64::from(x), i64::from(y)))
        ));
    }
    parts.join(", ")
}

fn outline_json(o: &AutoOutline) -> Value {
    json!({
        "layer": o.layer,
        "color": o.color.map_or("selective".to_string(), |c| c.to_string()),
        "inside": o.inside,
        "corners": o.corners,
        "thickness": o.thickness,
        "inner": o.inner,
        "light_dir": o.light.map(|(x, y)| direction_name((i64::from(x), i64::from(y)))),
    })
}

fn direction_name((dx, dy): (i64, i64)) -> &'static str {
    match (dx.signum(), dy.signum()) {
        (-1, -1) => "top-left",
        (0, -1) => "top",
        (1, -1) => "top-right",
        (-1, 0) => "left",
        (1, 0) => "right",
        (-1, 1) => "bottom-left",
        (0, 1) => "bottom",
        _ => "bottom-right",
    }
}

/// Prüf-Hinweise als Sätze. Bei mehreren Frames steht dabei, welche gemeint sind.
fn hint_texts(s: &Sprite, hints: &[lint::Hint]) -> Vec<String> {
    let places = |at: &[(u32, u32)], count: usize| {
        let mut list: Vec<String> = at.iter().map(|(x, y)| format!("({x},{y})")).collect();
        if count > at.len() {
            list.push("…".into());
        }
        list.join(" ")
    };
    hints
        .iter()
        .map(|h| {
            let what = match &h.finding {
                Finding::StrayPixels { count, at } => format!(
                    "{count} stray pixel(s) without any neighbor at {}",
                    places(at, *count)
                ),
                Finding::OutlineGaps { color, count, at } => format!(
                    "the outline ({color}) is interrupted by {count} pixel(s) of another color at {}",
                    places(at, *count)
                ),
                Finding::SingleUseColors { colors } => {
                    let list: Vec<String> = colors
                        .iter()
                        .map(|(c, (x, y))| format!("{c} at ({x},{y})"))
                        .collect();
                    format!(
                        "color(s) used by one single pixel, maybe a slip: {}",
                        list.join(", ")
                    )
                }
                Finding::Seam { horizontal: true } => {
                    "left and right edge do not match, tiling shows a vertical seam (check with view tile=true)".to_string()
                }
                Finding::Seam { horizontal: false } => {
                    "top and bottom edge do not match, tiling shows a horizontal seam (check with view tile=true)".to_string()
                }
            };
            if s.frames().len() > 1 {
                format!("frame {}: {what}", list_frames(&h.frames))
            } else {
                what
            }
        })
        .collect()
}

/// Ein Bild der Farben einer Palette: ein Feld je Farbe, höchstens 16 je Zeile.
fn swatches(colors: &[Rgba]) -> Res<Vec<u8>> {
    const CELL: u32 = 32;
    const PER_ROW: usize = 16;
    let columns = colors.len().clamp(1, PER_ROW) as u32;
    let rows = colors.len().div_ceil(PER_ROW).max(1) as u32;
    let mut img =
        image::RgbaImage::from_pixel(columns * CELL, rows * CELL, image::Rgba([32, 32, 40, 255]));
    for (i, c) in colors.iter().enumerate() {
        let (cx, cy) = ((i % PER_ROW) as u32 * CELL, (i / PER_ROW) as u32 * CELL);
        for y in 1..CELL - 1 {
            for x in 1..CELL - 1 {
                img.put_pixel(cx + x, cy + y, image::Rgba([c.r, c.g, c.b, 255]));
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(e)?;
    Ok(out.into_inner())
}

fn note_snap(s: &mut Sprite, wanted: Rgba, snapped: &mut BTreeMap<String, String>) -> Rgba {
    let got = s.conform(wanted);
    if got != wanted && !wanted.is_transparent() {
        snapped.insert(
            wanted.to_string(),
            if got.is_transparent() {
                "transparent".into()
            } else {
                got.to_string()
            },
        );
    }
    got
}

/// Was ein `draw`-Aufruf über alle Ops hinweg mitführt.
struct DrawCtx<'a> {
    /// Farben, die an Palette oder Farbtiefe angepasst wurden.
    snapped: BTreeMap<String, String>,
    warnings: Vec<String>,
    /// Farbtausch des aktuellen Frames (`per_frame.colors`).
    swap: HashMap<Rgba, Rgba>,
    /// Sprites anderer Assets, aus denen gestempelt wird, nach dem Namen in der Op.
    sources: &'a HashMap<String, Sprite>,
    /// Der Stil des Projekts: Vorgaben für Ops, die Licht oder Umrissfarbe nicht nennen.
    style: Style,
    /// Die eigenen Formen aus dem Arbeitsordner, für die Op `shape`.
    shapes: &'a [CustomShape],
}

impl DrawCtx<'_> {
    fn color(&mut self, s: &mut Sprite, c: &str) -> Res<Rgba> {
        let wanted = parse_color(c)?;
        let wanted = self.swap.get(&wanted).copied().unwrap_or(wanted);
        Ok(note_snap(s, wanted, &mut self.snapped))
    }

    fn colors(&mut self, s: &mut Sprite, list: &[String]) -> Res<Vec<Rgba>> {
        if list.len() > Palette::MAX_COLORS {
            return Err(format!("at most {} colors", Palette::MAX_COLORS));
        }
        list.iter().map(|c| self.color(s, c)).collect()
    }
}

/// "top-left", "down", "bottom_right" … als Schritt in x und y.
fn parse_direction(text: &str, what: &str) -> Res<(i64, i64)> {
    let (mut dx, mut dy) = (0, 0);
    for part in text.split(|c: char| c == '-' || c == '_' || c.is_whitespace()) {
        match part.to_ascii_lowercase().as_str() {
            "" => {}
            "left" => dx = -1,
            "right" => dx = 1,
            "up" | "top" => dy = -1,
            "down" | "bottom" => dy = 1,
            _ => {
                return Err(format!(
                    "unknown {what} '{text}', use left, right, up/top, down/bottom or a diagonal like top-left"
                ))
            }
        }
    }
    if (dx, dy) == (0, 0) {
        return Err(format!("{what} is empty"));
    }
    Ok((dx, dy))
}

fn parse_pattern(text: Option<&str>) -> Res<Option<Pattern>> {
    Ok(Some(
        match text.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
            None => return Ok(None),
            Some("bayer2") => Pattern::Bayer2,
            Some("bayer4") => Pattern::Bayer4,
            Some("bayer8") => Pattern::Bayer8,
            Some("checker") => Pattern::Checker,
            Some(other) => {
                return Err(format!(
                    "unknown pattern '{other}', use bayer2, bayer4, bayer8 or checker"
                ))
            }
        },
    ))
}

fn parse_flow(direction: Option<&str>, center: Option<[f64; 2]>, radius: Option<f64>) -> Res<Flow> {
    match center {
        Some([cx, cy]) => Ok(Flow::Radial { cx, cy, radius }),
        None => {
            if radius.is_some() {
                return Err("`radius` needs `center`".into());
            }
            let (dx, dy) = parse_direction(direction.unwrap_or("down"), "direction")?;
            Ok(Flow::Linear {
                dx: dx as f64,
                dy: dy as f64,
            })
        }
    }
}

/// Wählt die Pixel aus, auf die eine Flächen-Op wirkt.
fn select(s: &Sprite, t: Target, area: &Where, default: Mask) -> Res<Selection> {
    let mask = match area.mask.as_deref().map(str::trim) {
        None => default,
        Some(m) if m.eq_ignore_ascii_case("all") => Mask::All,
        Some(m) if m.eq_ignore_ascii_case("opaque") => Mask::Opaque,
        Some(m) if m.eq_ignore_ascii_case("transparent") => Mask::Transparent,
        Some(m) => Mask::Color(parse_color(m).map_err(|_| {
            format!("invalid mask '{m}': use all, opaque, transparent or a color like #566c86")
        })?),
    };
    let rect = if area.x.is_some() || area.y.is_some() || area.w.is_some() || area.h.is_some() {
        Some(Area {
            x: area.x.unwrap_or(0),
            y: area.y.unwrap_or(0),
            w: area.w.unwrap_or(u32::MAX),
            h: area.h.unwrap_or(u32::MAX),
        })
    } else {
        None
    };
    Selection::new(s, t, rect, mask).map_err(e)
}

/// Meldet, wenn eine Flächen-Op nichts getroffen hat: das ist fast immer ein Versehen.
fn warn_if_empty(sel: &Selection, op: &str, area: &Where, warnings: &mut Vec<String>) {
    if sel.is_empty() {
        warnings.push(match &area.mask {
            Some(mask) => {
                format!("{op} changed nothing: no pixel in the area matches mask '{mask}'")
            }
            None => format!("{op} changed nothing: the area holds no matching pixels"),
        });
    }
}

/// `[x, y, w, h]` als Ausschnitt.
fn to_area(a: &[i64; 4]) -> Res<Area> {
    let [x, y, w, h] = *a;
    let max = pixel_core::MAX_SIZE as i64;
    if !(1..=max).contains(&w) || !(1..=max).contains(&h) {
        return Err("`area` is [x, y, w, h] with w and h of at least 1".into());
    }
    check_box(x, y, w as u32, h as u32)?;
    Ok(Area {
        x,
        y,
        w: w as u32,
        h: h as u32,
    })
}

fn apply_op(s: &mut Sprite, t: Target, op: &DrawOp, ctx: &mut DrawCtx) -> Res<()> {
    match op {
        DrawOp::Pixels { color: c, points } => {
            let c = ctx.color(s, c)?;
            let pts: Vec<(i64, i64)> = points.iter().map(|p| (p[0], p[1])).collect();
            draw::pixels(s, t, &pts, c).map_err(e)
        }
        DrawOp::Line { color: c, from, to } => {
            let c = ctx.color(s, c)?;
            let lim = 4 * pixel_core::MAX_SIZE as i64;
            if [from[0], from[1], to[0], to[1]]
                .iter()
                .any(|v| !(-lim..=lim).contains(v))
            {
                return Err("line coordinates are far outside the canvas".into());
            }
            draw::line(s, t, (from[0], from[1]), (to[0], to[1]), c).map_err(e)
        }
        DrawOp::Rect {
            color: c,
            x,
            y,
            w,
            h,
            filled,
        } => {
            let c = ctx.color(s, c)?;
            check_box(*x, *y, *w, *h)?;
            draw::rect(s, t, *x, *y, *w, *h, c, *filled).map_err(e)
        }
        DrawOp::Ellipse {
            color: c,
            x,
            y,
            w,
            h,
            filled,
        } => {
            let c = ctx.color(s, c)?;
            check_box(*x, *y, *w, *h)?;
            draw::ellipse(s, t, *x, *y, *w, *h, c, *filled).map_err(e)
        }
        DrawOp::Fill { color: c, x, y } => {
            let c = ctx.color(s, c)?;
            draw::flood_fill(s, t, *x, *y, c).map(|_| ()).map_err(e)
        }
        DrawOp::Grid { x, y, rows, legend } => {
            let mut map: HashMap<char, Rgba> = HashMap::new();
            for (k, v) in legend {
                let mut chars = k.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else {
                    return Err(format!("legend key '{k}' must be exactly one character"));
                };
                map.insert(ch, ctx.color(s, v)?);
            }
            if rows.len() > pixel_core::MAX_SIZE as usize {
                return Err("grid has too many rows".into());
            }
            // Erst alles prüfen, dann zeichnen, damit ein Tippfehler kein halbes Bild hinterlässt.
            let mut by_color: HashMap<Rgba, Vec<(i64, i64)>> = HashMap::new();
            for (ry, row) in rows.iter().enumerate() {
                for (rx, ch) in row.chars().enumerate() {
                    match map.get(&ch) {
                        Some(&c) => by_color
                            .entry(c)
                            .or_default()
                            .push((x + rx as i64, y + ry as i64)),
                        None if ch == '.' || ch == ' ' => {}
                        None => {
                            let mut known: Vec<String> =
                                map.keys().map(|k| k.to_string()).collect();
                            known.sort();
                            return Err(format!(
                                "row {ry}: character '{ch}' is not in the legend. Legend keys: {}",
                                known.join(" ")
                            ));
                        }
                    }
                }
            }
            for (c, pts) in by_color {
                draw::pixels(s, t, &pts, c).map_err(e)?;
            }
            Ok(())
        }
        DrawOp::ReplaceColor { from, to } => {
            let from = parse_color(from)?;
            let to = ctx.color(s, to)?;
            let changed = draw::replace_color(s, t, from, to).map_err(e)?;
            if changed == 0 {
                ctx.warnings.push(format!(
                    "replace_color found no pixel of color {from} on this layer"
                ));
            }
            Ok(())
        }
        DrawOp::Shift { dx, dy } => {
            let lost = draw::shift(s, t, *dx, *dy).map_err(e)?;
            if lost > 0 {
                ctx.warnings.push(format!(
                    "shift by ({dx}, {dy}) pushed {lost} pixel(s) off the canvas, they are lost"
                ));
            }
            Ok(())
        }
        DrawOp::FlipH => draw::flip_horizontal(s, t).map_err(e),
        DrawOp::FlipV => {
            let patch = Patch::cut(s, t.frame, Some(t.layer), None)
                .map_err(e)?
                .flipped(false, true);
            s.layer_mut(t.frame, t.layer)
                .map_err(e)?
                .replace_pixels(patch.pixels)
                .map_err(e)
        }
        DrawOp::Clear => {
            s.layer_mut(t.frame, t.layer).map_err(e)?.clear();
            Ok(())
        }
        DrawOp::Outline {
            color,
            thickness,
            diagonals,
        } => {
            let c = match (color, ctx.style.outline) {
                (Some(color), _) => ctx.color(s, color)?,
                (None, Some(color)) => note_snap(s, color, &mut ctx.snapped),
                (None, None) => {
                    return Err(
                        "`color` is missing and the project style sets no outline color (see set_style)"
                            .into(),
                    )
                }
            };
            let thickness = thickness.unwrap_or(1);
            if !(1..=16).contains(&thickness) {
                return Err("`thickness` must be 1 to 16".into());
            }
            let added = effects::outline(s, t, c, thickness, *diagonals).map_err(e)?;
            if added == 0 {
                ctx.warnings.push(
                    "outline changed nothing: the layer is empty or completely filled".into(),
                );
            }
            Ok(())
        }
        DrawOp::Shade {
            light_dir,
            ramp,
            area,
        } => {
            let light_dir = light_dir
                .as_deref()
                .or(ctx.style.light_dir.as_deref())
                .unwrap_or("top-left");
            let light = parse_direction(light_dir, "light_dir")?;
            let ramp = ctx.colors(s, ramp)?;
            let sel = select(s, t, area, Mask::Opaque)?;
            warn_if_empty(&sel, "shade", area, &mut ctx.warnings);
            effects::shade(s, t, &sel, light, &ramp)
                .map(|_| ())
                .map_err(e)
        }
        DrawOp::Dither {
            from_color,
            to_color,
            direction,
            center,
            radius,
            pattern,
            area,
        } => {
            let colors = [ctx.color(s, from_color)?, ctx.color(s, to_color)?];
            let flow = parse_flow(direction.as_deref(), *center, *radius)?;
            let pattern = parse_pattern(pattern.as_deref())?.unwrap_or(Pattern::Bayer4);
            let sel = select(s, t, area, Mask::All)?;
            warn_if_empty(&sel, "dither", area, &mut ctx.warnings);
            effects::gradient(s, t, &sel, &colors, flow, None, Some(pattern))
                .map(|_| ())
                .map_err(e)
        }
        DrawOp::Gradient {
            colors,
            direction,
            center,
            radius,
            steps,
            pattern,
            area,
        } => {
            let colors = ctx.colors(s, colors)?;
            if steps.is_some_and(|n| !(1..=256).contains(&n)) {
                return Err("`steps` must be 1 to 256".into());
            }
            let flow = parse_flow(direction.as_deref(), *center, *radius)?;
            let pattern = parse_pattern(pattern.as_deref())?;
            let sel = select(s, t, area, Mask::All)?;
            warn_if_empty(&sel, "gradient", area, &mut ctx.warnings);
            effects::gradient(s, t, &sel, &colors, flow, *steps, pattern)
                .map(|_| ())
                .map_err(e)
        }
        DrawOp::Noise {
            colors,
            density,
            seed,
            area,
        } => {
            let colors = ctx.colors(s, colors)?;
            let sel = select(s, t, area, Mask::All)?;
            warn_if_empty(&sel, "noise", area, &mut ctx.warnings);
            effects::noise(s, t, &sel, &colors, *density, *seed)
                .map(|_| ())
                .map_err(e)
        }
        DrawOp::Shader {
            formula,
            colors,
            seed,
            area,
        } => {
            let formula = Expr::parse(formula).map_err(e)?;
            let colors = ctx.colors(s, colors)?;
            let sel = select(s, t, area, Mask::All)?;
            warn_if_empty(&sel, "shader", area, &mut ctx.warnings);
            let painted = effects::shader(s, t, &sel, &formula, &colors, *seed).map_err(e)?;
            if painted == 0 && !sel.is_empty() {
                ctx.warnings.push(
                    "shader changed nothing: the formula was negative for every pixel".into(),
                );
            }
            Ok(())
        }
        DrawOp::Stamp {
            x,
            y,
            src,
            from_asset,
            from_frame,
            from_layer,
            flip_h,
            flip_v,
            rotate,
            color_map,
        } => {
            let area = match src {
                None => None,
                Some([sx, sy, sw, sh]) => {
                    let max = pixel_core::MAX_SIZE as i64;
                    if !(1..=max).contains(sw) || !(1..=max).contains(sh) {
                        return Err("`src` is [x, y, w, h] with w and h of at least 1".into());
                    }
                    Some(Area {
                        x: *sx,
                        y: *sy,
                        w: *sw as u32,
                        h: *sh as u32,
                    })
                }
            };
            let merged = matches!(from_layer, Some(Value::String(name)) if name.trim().eq_ignore_ascii_case("all"));
            let other = from_asset.as_deref().and_then(|name| ctx.sources.get(name));
            let patch = match other {
                Some(other) => {
                    let f = Studio::frame_index(other, Some(from_frame.unwrap_or(0)))?;
                    let l = match from_layer {
                        None | Some(Value::Null) => None,
                        _ if merged => None,
                        some => Some(Studio::layer_index(other, some.as_ref())?),
                    };
                    Patch::cut(other, f, l, area)
                }
                None => {
                    let f = Studio::frame_index(s, Some(from_frame.unwrap_or(t.frame)))?;
                    let l = match from_layer {
                        None | Some(Value::Null) => Some(t.layer),
                        _ if merged => None,
                        some => Some(Studio::layer_index(s, some.as_ref())?),
                    };
                    Patch::cut(s, f, l, area)
                }
            }
            .map_err(e)?;
            let mut map = Vec::new();
            for (from, to) in color_map.iter().flatten() {
                map.push((parse_color(from)?, ctx.color(s, to)?));
            }
            let patch = patch
                .flipped(*flip_h, *flip_v)
                .rotated(rotate.unwrap_or(0))
                .map_err(e)?
                .recolored(&map);
            let lim = 4 * pixel_core::MAX_SIZE as i64;
            if !(-lim..=lim).contains(x) || !(-lim..=lim).contains(y) {
                return Err("stamp position is far outside the canvas".into());
            }
            let painted = effects::stamp(s, t, &patch, *x, *y).map_err(e)?;
            if painted == 0 {
                ctx.warnings.push(
                    "stamp changed nothing: the source piece is empty or lands outside the canvas"
                        .into(),
                );
            }
            Ok(())
        }
        DrawOp::Shape {
            shape,
            color,
            x,
            y,
            w,
            h,
            filled,
            keep_colors,
            flip_h,
            flip_v,
        } => {
            check_box(*x, *y, *w, *h)?;
            let max = pixel_core::MAX_SIZE;
            if *w == 0 || *h == 0 || *w > max || *h > max {
                return Err(format!("a shape needs w and h from 1 to {max}"));
            }
            let custom = ctx
                .shapes
                .iter()
                .find(|c| workspace::name_key(&c.name) == workspace::name_key(shape));
            let (mask, colors) = match (Shape::parse(shape), custom) {
                (Some(b), _) => (b.mask(*w, *h), None),
                (None, Some(c)) => (c.mask(*w, *h), keep_colors.then(|| c.colors(*w, *h))),
                (None, None) => {
                    let mut names: Vec<String> =
                        Shape::ALL.iter().map(|b| b.id().to_string()).collect();
                    names.extend(ctx.shapes.iter().map(|c| c.name.clone()));
                    return Err(format!(
                        "unknown shape '{shape}', use one of: {}",
                        names.join(", ")
                    ));
                }
            };
            if *keep_colors && colors.is_none() {
                return Err("`keep_colors` only works with a saved custom shape".into());
            }
            let c = match color {
                Some(c) => ctx.color(s, c)?,
                None if colors.is_some() => Rgba::TRANSPARENT,
                None => return Err("this shape needs `color`".into()),
            };
            let mask = if *filled {
                mask
            } else {
                shapes::edge(&mask, *w, *h)
            };
            let mask = shapes::flip(&mask, *w, *h, *flip_h, *flip_v);
            let colors = colors.map(|cs| shapes::flip(&cs, *w, *h, *flip_h, *flip_v));
            let n =
                shapes::draw_mask(s, t, *x, *y, *w, *h, &mask, c, colors.as_deref()).map_err(e)?;
            if n == 0 {
                ctx.warnings
                    .push("shape changed nothing: it lies outside the canvas".into());
            }
            Ok(())
        }
        DrawOp::Spray {
            color,
            x,
            y,
            radius,
            density,
            seed,
        } => {
            let c = ctx.color(s, color)?;
            check_box(*x, *y, 1, 1)?;
            let n = brush::spray(
                s,
                t,
                *x,
                *y,
                radius.unwrap_or(3),
                density.unwrap_or(0.25),
                c,
                *seed,
            )
            .map_err(e)?;
            if n == 0 {
                ctx.warnings.push(
                    "spray changed nothing: raise density or radius, or check the position".into(),
                );
            }
            Ok(())
        }
        DrawOp::Lighten { steps, area } | DrawOp::Darken { steps, area } => {
            let lighten = matches!(op, DrawOp::Lighten { .. });
            let name = if lighten { "lighten" } else { "darken" };
            let steps = steps.unwrap_or(1);
            if !(1..=8).contains(&steps) {
                return Err("`steps` must be 1 to 8".into());
            }
            let sel = select(s, t, area, Mask::Opaque)?;
            let w = s.width() as usize;
            let points: Vec<(i64, i64)> = s
                .layer(t.frame, t.layer)
                .map_err(e)?
                .pixels()
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.is_transparent())
                .map(|(i, _)| ((i % w) as i64, (i / w) as i64))
                .filter(|&(x, y)| sel.contains_point(x, y))
                .collect();
            let mut changed = 0;
            for _ in 0..steps {
                changed += brush::tone(s, t, &points, lighten).map_err(e)?;
            }
            if changed == 0 {
                ctx.warnings.push(format!(
                    "{name} changed nothing: no drawn pixel in the area, or no {} color in the palette",
                    if lighten { "lighter" } else { "darker" }
                ));
            }
            Ok(())
        }
        DrawOp::MoveArea { area, dx, dy } => {
            let a = to_area(area)?;
            check_box(*dx, *dy, 1, 1)?;
            brush::move_area(s, t, a, *dx, *dy).map(|_| ()).map_err(e)
        }
        DrawOp::FlipArea {
            area,
            horizontal,
            vertical,
        } => {
            if !horizontal && !vertical {
                return Err("flip_area needs `horizontal` and/or `vertical`".into());
            }
            brush::flip_area(s, t, to_area(area)?, *horizontal, *vertical).map_err(e)
        }
        DrawOp::ClearArea { area } => {
            if brush::clear_area(s, t, to_area(area)?).map_err(e)? == 0 {
                ctx.warnings
                    .push("clear_area changed nothing: the area was already empty".into());
            }
            Ok(())
        }
    }
}

fn check_box(x: i64, y: i64, w: u32, h: u32) -> Res<()> {
    let lim = 4 * pixel_core::MAX_SIZE as i64;
    // Bereichsprüfung statt abs(): i64::MIN hat keinen positiven Betrag.
    let ok = |v: i64| (-lim..=lim).contains(&v);
    if !ok(x) || !ok(y) || !ok(w as i64) || !ok(h as i64) {
        return Err("shape is far outside the canvas".into());
    }
    Ok(())
}

fn sprite_bytes(s: &Sprite) -> usize {
    s.frames().len() * s.layer_count().max(1) * s.width() as usize * s.height() as usize * 4
}

fn list_frames(frames: &[usize]) -> String {
    frames
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Ziffern 0 bis 9 als 3×5-Raster für die Koordinaten am Rand der Vorschau.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];
/// Rand für die Koordinaten: links Platz für vier Ziffern, oben für eine Zeile.
const MARGIN_LEFT: u32 = 18;
const MARGIN_TOP: u32 = 8;
/// Größte Kachel, die `tile` dreimal nebeneinander zeigt.
const MAX_TILE: u32 = 256;

/// Schreibt eine Zahl; `right` ist die rechte Kante der letzten Ziffer.
fn draw_number(img: &mut image::RgbaImage, right: u32, top: u32, value: u32) {
    let text = value.to_string();
    let mut x = right as i64 - (text.len() as i64 * 4 - 1);
    for ch in text.bytes() {
        let glyph = DIGITS[(ch - b'0') as usize];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3 {
                let (px, py) = (x + col, top as i64 + row as i64);
                let on = bits & (0b100 >> col) != 0;
                if on && px >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    img.put_pixel(px as u32, py as u32, image::Rgba([225, 225, 230, 255]));
                }
            }
        }
        x += 4;
    }
}

/// Vorschaubild: die gewählten Frames nebeneinander, ganzzahlig vergrößert,
/// auf grauem Schachbrett, damit Transparenz erkennbar ist. Auf Wunsch mit
/// Pixelraster und Koordinaten, als Ausschnitt oder 3×3 gekachelt.
/// Gibt das PNG und Hinweise zum Bild zurück.
fn render(
    s: &Sprite,
    frames: &[usize],
    layer: Option<usize>,
    o: &ViewOptions,
) -> Res<(Vec<u8>, Vec<String>)> {
    let (w, h) = (s.width(), s.height());
    let mut notes = Vec::new();
    let backdrop = match o.background.as_deref().map(str::trim) {
        None => None,
        Some(b) if b.eq_ignore_ascii_case("checker") || b.eq_ignore_ascii_case("checkerboard") => {
            None
        }
        Some(b) => {
            let c = parse_color(b).map_err(|_| {
                format!("invalid background '{b}': use \"checker\" or a color like #000000")
            })?;
            (!c.is_transparent()).then_some(Rgba { a: 255, ..c })
        }
    };
    notes.push(match backdrop {
        None => "Background: gray checkerboard (= transparent).".to_string(),
        Some(c) => format!("Background: {c} (= transparent)."),
    });
    if o.tile && (w > MAX_TILE || h > MAX_TILE) {
        return Err(format!(
            "`tile` works up to {MAX_TILE}×{MAX_TILE} px, this asset is {w}×{h}"
        ));
    }
    // Größe des Bildes, aus dem der Ausschnitt kommt.
    let (sw, sh) = if o.tile { (3 * w, 3 * h) } else { (w, h) };
    let (rx, ry, rw, rh) = match o.zoom_region {
        None => (0, 0, sw, sh),
        Some([x, y, rw, rh]) => {
            let x0 = x.clamp(0, sw as i64);
            let y0 = y.clamp(0, sh as i64);
            let x1 = x.saturating_add(rw).clamp(0, sw as i64);
            let y1 = y.saturating_add(rh).clamp(0, sh as i64);
            if x1 <= x0 || y1 <= y0 {
                return Err(format!(
                    "zoom_region [{x}, {y}, {rw}, {rh}] lies outside the picture ({sw}×{sh} px)"
                ));
            }
            (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
        }
    };
    if o.tile {
        notes.push(format!(
            "Tiled 3×3 to check the seams; the tile repeats every {w}×{h} px."
        ));
    }
    if o.zoom_region.is_some() {
        notes.push(format!(
            "Region x {rx} to {}, y {ry} to {}.",
            rx + rw - 1,
            ry + rh - 1
        ));
    }
    let n = frames.len() as u32;
    // Die Frames so in Zeilen und Spalten legen, dass sie möglichst groß bleiben.
    // Zwischen den Frames liegt 1 Quell-Pixel Abstand.
    let mut layout = (0.0f64, n, 1u32);
    for cols in (1..=n).rev() {
        let rows = n.div_ceil(cols);
        let fit = (PREVIEW_WIDTH as f64 / (cols * rw + cols - 1) as f64)
            .min(PREVIEW_HEIGHT as f64 / (rows * rh + rows - 1) as f64);
        // Ab 1× zählen nur ganze Stufen, und bei Gleichstand gewinnt die breitere Reihe.
        let fit = if fit >= 1.0 {
            fit.floor().min(32.0)
        } else {
            fit
        };
        if fit > layout.0 {
            layout = (fit, cols, rows);
        }
    }
    let (fit, cols, rows) = layout;
    let scale = (fit as u32).clamp(1, 32);
    let grid = o.grid && scale >= 4;
    if o.grid && !grid {
        notes.push(format!(
            "The grid was left out: at {scale}× the pixels are too small. Use zoom_region or a single frame to get closer."
        ));
    }
    let (ml, mt) = if grid {
        (MARGIN_LEFT, MARGIN_TOP)
    } else {
        (0, 0)
    };
    let (cell_w, cell_h) = (rw * scale + ml, rh * scale + mt);
    let gap = scale;
    let out_w = cols * cell_w + (cols - 1) * gap;
    let out_h = rows * cell_h + (rows - 1) * gap;
    let checker = if w.max(h) >= 16 {
        (w.max(h) / 16).max(2)
    } else {
        1
    };
    // Beschriftet wird in Schritten, bei denen die Zahlen nebeneinander passen.
    let label_step = [1u32, 2, 4, 8, 16, 32, 64]
        .into_iter()
        .find(|step| step * scale >= 18)
        .unwrap_or(64);
    let mut img = image::RgbaImage::from_pixel(out_w, out_h, image::Rgba([40, 40, 48, 255]));
    for (i, &f) in frames.iter().enumerate() {
        let px: Vec<Rgba> = match layer {
            Some(l) => s.layer(f, l).map_err(e)?.pixels().to_vec(),
            None => s.composite(f).map_err(e)?,
        };
        let ox = (i as u32 % cols) * (cell_w + gap) + ml;
        let oy = (i as u32 / cols) * (cell_h + gap) + mt;
        for y in 0..rh {
            for x in 0..rw {
                // Koordinaten im Bild; beim Kacheln wiederholt sich die Quelle.
                let (ax, ay) = (rx + x, ry + y);
                let (qx, qy) = (ax % w, ay % h);
                let bg = backdrop.unwrap_or_else(|| {
                    if ((qx / checker) + (qy / checker)) % 2 == 0 {
                        Rgba::rgb(200, 200, 204)
                    } else {
                        Rgba::rgb(168, 168, 174)
                    }
                });
                let c = px[(qy * w + qx) as usize].over(bg);
                for sy in 0..scale {
                    for sx in 0..scale {
                        // Rasterlinien liegen auf der linken und oberen Kante jedes Pixels.
                        let line = |a: u32, edge: bool, period: u32| -> Option<[u8; 3]> {
                            if !edge {
                                None
                            } else if o.tile && a.is_multiple_of(period) {
                                Some([255, 60, 200])
                            } else if a.is_multiple_of(8) {
                                Some([20, 20, 28])
                            } else {
                                Some([96, 96, 104])
                            }
                        };
                        let rule = if grid {
                            line(ax, sx == 0, w).or(line(ay, sy == 0, h))
                        } else {
                            None
                        };
                        let out = match rule {
                            Some([r, g, b]) => image::Rgba([r, g, b, 255]),
                            None => image::Rgba(c.to_array()),
                        };
                        img.put_pixel(ox + x * scale + sx, oy + y * scale + sy, out);
                    }
                }
            }
        }
        if grid {
            for x in (0..rw).filter(|x| (rx + x) % label_step == 0) {
                // Die Zahl steht linksbündig über ihrer Pixelspalte.
                let value = rx + x;
                let width = value.to_string().len() as u32 * 4 - 1;
                draw_number(&mut img, ox + x * scale + width, oy - mt + 1, value);
            }
            for y in (0..rh).filter(|y| (ry + y) % label_step == 0) {
                draw_number(&mut img, ox - 3, oy + y * scale + 1, ry + y);
            }
        }
    }
    if grid {
        notes.push(format!(
            "Grid: one cell per pixel, darker lines every 8 px, coordinates on the top and left edge every {label_step} px."
        ));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(e)?;
    Ok((out.into_inner(), notes))
}

/// Vorschau ohne Extras.
fn render_plain(s: &Sprite, frames: &[usize], layer: Option<usize>) -> Res<Vec<u8>> {
    Ok(render(s, frames, layer, &ViewOptions::default())?.0)
}
