//! MCP-Anbindung: Parameter-Typen und Werkzeug-Definitionen. Die Logik liegt in `studio`.

use crate::studio::{
    AutotileParams, DrawOp, EffectsParams, ImportArgs, LightArgs, NormalMapOpts, OutlineArgs,
    PerFrame, PixelateArgs, Reply, Res, Studio, StyleArgs, StylesParams, ViewOptions,
};
use base64::Engine;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

const INSTRUCTIONS: &str = r##"PixelArt Studio creates REAL pixel art: every image is an exact grid of pixels (max 1024×1024), never a blurry or upscaled picture. Coordinates are 0-based, x grows right, y grows down. Colors are "#RRGGBB", "#RRGGBBAA" or "transparent".

Typical workflow:
1. get_studio_info to see projects, canvas templates, palette presets and animation presets.
2. create_project, optionally set_style (shared palette, light direction, outline color; `style` takes a whole style from the style library such as "Manga", "Comic", "8 Bit", "16 Bit", "HQ Pixel Art" or "Castles"), then create_character (for anything that will be animated or reused) or create_image (tiles, items, scenes).
3. draw on it. For exact shapes prefer the `grid` op: text rows plus a legend give control over every pixel. The `shape` op draws hearts, stars, diamonds and other shapes at any size, also custom shapes saved with custom_shapes; `mirror` makes symmetric drawing easy. Use layers (e.g. legs, body, head, weapon) so parts can move independently in animations; create_image and create_character take `layers`, create_image also `frames`.
4. Add depth with the technique ops instead of placing every pixel by hand. For characters and animations prefer set_outline: an automatic outline on its own layer that follows every change in every frame (selective or one color, inside or outside, inner lines between layers). For light that changes over an animation (a torch passing, the sun moving, a flash) use set_light: lit and shaded edges plus an optional ground shadow, following a light path across the frames. Ops: `outline` around a shape on one layer, `shade` for light and shadow from one direction, `gradient` (hard bands) and `dither` for transitions, `noise` for grain, `lighten`/`darken` to push pixels one step along the palette's color ramps, `spray` for soft scattered texture, `stamp` to repeat a piece, and `shader` (a per-pixel math formula with x, y, frame, dist(), noise(), rand()) for glow, light and anything procedural. Most take an area (x, y, w, h) and a `mask` ("opaque", "transparent" or a color) so they only touch the part you mean.
5. Every draw call returns a preview image. Look at it and correct mistakes before continuing; `undo` takes back the last draw calls. view has `grid` (coordinates), `zoom_region` and `tile` (seam check); get_pixels reads exact pixels back as text.
6. Animation: create_animation (idle, walk, run, jump, attack, hurt, death or a custom name). All frames start as exact copies of the character's base pose; change only what moves. One draw call can cover several frames: `frame: "all"` applies the ops to every frame, and `per_frame` gives each frame its own color substitutions or extra ops (e.g. a shift, or a brighter glow color). In `shader` use `frame` or `t` to animate.
7. Before finishing, get_project lists `hints` on likely slips (stray pixels, outline gaps, single-use colors, tile seams); view shows them too. palette_from_image takes the colors from reference art, reduce_colors brings an asset down to fewer colors.
8. export as png, jpg, sheet (sprite sheet with JSON metadata), gif or webm (animated). With `normal_map` export also writes normal maps (suffix _n) for dynamic lighting in Godot or Unity.
Canvas too small or too big: resize_canvas (keeps a character and its animations the same size). Existing art: import_image brings a PNG, JPG or GIF file (also sprite sheets) into the project as an image, a character or an animation; with `pixelate` it converts photos and drawings into pixel art, and as="reference" adds a tracing layer (not exported) to draw over.

Consistency rules:
- A character's palette is locked when it is created. Colors drawn on the character or its animations snap to the nearest palette color, so choose the full palette up front (skin, hair, clothes, outline, shadows, highlights). add_character_colors appends colors later without changing existing ones.
- Animations always have the size, color depth and palette of their character. get_project lists any consistency issues.
- To reuse a character in another project use copy_character; it stays pixel-identical.

Styles: when the user names a style ("in manga style", "8 bit"), look it up with styles(action: "get") or pass it as `style` to set_style, create_image or create_character, and follow its rules, palette, shades and outline exactly. Own styles can be saved with styles(action: "save").
Tiles: for terrain that joins up by itself (grass, water, walls) use autotile: create a template, draw the few source tiles on it, build the set and export it for Godot or Tiled. view with tile=true checks a single seamless tile.
Effects: for glow, lightning, an explosion, a reflection in water, sparks, smoke, magic sparkles or color cycling (flowing water, flickering fire, lava) use the effects tool rather than drawing them frame by frame. Each effect sits on its own layer, loops over the frames and follows every change; `frames` turns a still image into a loop first. For color_cycle paint the flowing area with the ramp colors (a `preset` like "water" names them) and the colors move.

Pixel art craft: keep outlines 1 pixel wide, use few colors (3 to 5 shades per material), avoid single stray pixels, light from one direction, and keep the silhouette readable at 1× size."##;

#[derive(Clone)]
pub struct PixelServer {
    studio: Arc<Mutex<Studio>>,
    tool_router: ToolRouter<Self>,
}

impl PixelServer {
    pub fn new(studio: Studio) -> Self {
        Self {
            studio: Arc::new(Mutex::new(studio)),
            tool_router: Self::tool_router(),
        }
    }

    /// Führt eine Operation aus und verpackt Ergebnis oder Fehler so, dass das Modell es lesen kann.
    fn run(&self, f: impl FnOnce(&Studio) -> Res) -> CallToolResult {
        // Der Mutex sorgt dafür, dass Laden, Ändern und Speichern nie ineinanderlaufen.
        let studio = self
            .studio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match f(&studio) {
            Ok(Reply { text, image_png }) => {
                let mut content = vec![ContentBlock::text(text)];
                if let Some(png) = image_png {
                    let data = base64::engine::general_purpose::STANDARD.encode(png);
                    content.push(ContentBlock::image(data, "image/png"));
                }
                CallToolResult::success(content)
            }
            Err(msg) => CallToolResult::error(vec![ContentBlock::text(msg)]),
        }
    }
}

// ------------------------------------------------------------------ Parameter

#[derive(Deserialize, JsonSchema)]
pub struct ProjectOnly {
    /// Project name.
    project: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateImage {
    /// Project name.
    project: String,
    /// Name of the new image, unique within the project.
    name: String,
    /// Canvas template id from get_studio_info, e.g. "tile-32". Alternative to width/height.
    template: Option<String>,
    /// Width in pixels (1 to 1024). Use together with height instead of template.
    width: Option<u32>,
    /// Height in pixels (1 to 1024).
    height: Option<u32>,
    /// 8 (indexed palette), 16 (RGB565) or 32 (RGBA, default).
    color_depth_bits: Option<u8>,
    /// Palette preset name from get_studio_info. Default "default16".
    palette_preset: Option<String>,
    /// Custom palette as a list of colors. Alternative to palette_preset.
    palette: Option<Vec<String>>,
    /// Style from the style library, e.g. "Manga", "8 Bit", "16 Bit", "HQ Pixel Art", "Comic", "Castles" or an own style (see styles). Brings its palette, color depth and usual size unless you give them, and the reply lists the style's rules to follow. Without it the project's style applies.
    style: Option<String>,
    /// Layer names from bottom to top, e.g. ["stone", "glow"]. Default: one layer.
    layers: Option<Vec<String>>,
    /// Number of frames, 1 to 256. Default 1. All start empty.
    frames: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateCharacter {
    /// Project name.
    project: String,
    /// Character name, unique within the project.
    name: String,
    /// Short description of the character (look, role). Helps keep it consistent later.
    description: Option<String>,
    /// Canvas template id, e.g. "char-32x48". Alternative to width/height.
    template: Option<String>,
    /// Width in pixels (1 to 1024).
    width: Option<u32>,
    /// Height in pixels (1 to 1024).
    height: Option<u32>,
    /// 8 (indexed palette), 16 (RGB565) or 32 (RGBA, default).
    color_depth_bits: Option<u8>,
    /// Palette preset name. Default "default16".
    palette_preset: Option<String>,
    /// The character's full palette as a list of colors. It is locked after creation.
    palette: Option<Vec<String>>,
    /// Style from the style library, e.g. "Manga", "8 Bit", "16 Bit", "HQ Pixel Art", "Comic", "Castles" or an own style (see styles). Brings its palette, color depth and usual size unless you give them, and the reply lists the style's rules to follow. Without it the project's style applies.
    style: Option<String>,
    /// Layer names from bottom to top, e.g. ["legs", "body", "head"]. Default: one layer.
    layers: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateAnimation {
    /// Project name.
    project: String,
    /// Name or id of the character.
    character: String,
    /// idle, walk, run, jump, attack, hurt, death, or any custom name.
    kind: String,
    /// Number of frames. Default depends on the kind (see get_studio_info).
    frames: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AssetOnly {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CopyCharacter {
    /// Project that contains the character.
    from_project: String,
    /// Name or id of the character.
    character: String,
    /// Project that should receive the character and its animations.
    to_project: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct Draw {
    /// Project name.
    project: String,
    /// Name or id of a character (its base pose), animation or image.
    asset: String,
    /// Frame to draw on: an index (0-based), a list of indexes like [0, 2], or "all". The
    /// ops are applied to every listed frame. Default 0.
    frame: Option<Value>,
    /// Layer index (0 = bottom) or layer name. Default 0.
    layer: Option<Value>,
    /// Operations, applied in order to each frame. If one fails, nothing is saved.
    #[serde(default)]
    ops: Vec<DrawOp>,
    /// What differs between frames: per frame a color substitution for the shared ops
    /// and/or extra ops. Entry i belongs to the i-th frame of `frame` unless it names one.
    /// Example: [{}, {"colors": {"#ef7d57": "#ffcd75"}}, {"ops": [{"op": "shift", "dx": 0, "dy": -1}]}]
    #[serde(default)]
    per_frame: Vec<PerFrame>,
    /// Symmetric drawing: everything the ops change is copied to the mirrored side of the
    /// canvas. "left_right" (mirror at the vertical center line), "top_bottom" or "both".
    mirror: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct Undo {
    /// Project name.
    project: String,
    /// How many draw calls to take back, newest first. Default 1.
    steps: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct View {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// Frame index, a list of indexes, or "all". Omit to see all frames side by side.
    frame: Option<Value>,
    /// Layer index or name to see only that layer. Omit for the full picture.
    layer: Option<Value>,
    /// Draw the pixel grid with coordinates on the top and left edge. Needs enough zoom:
    /// combine with zoom_region or a single frame on larger pictures.
    #[serde(default)]
    grid: bool,
    /// Show only the area [x, y, w, h], enlarged.
    zoom_region: Option<[i64; 4]>,
    /// Show the picture tiled 3×3 to check that a tile repeats without seams.
    #[serde(default)]
    tile: bool,
    /// What shows through transparent pixels: "checker" (default) or a color like "#000000".
    background: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetPixels {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// Frame index. Default 0.
    frame: Option<usize>,
    /// Layer index or name. Omit to read the combined picture of all visible layers.
    layer: Option<Value>,
    /// Left edge of the region. Default 0.
    x: Option<u32>,
    /// Top edge of the region. Default 0.
    y: Option<u32>,
    /// Region width, at most 128. Default: to the right edge.
    w: Option<u32>,
    /// Region height, at most 128. Default: to the bottom edge.
    h: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditLayers {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// "add" (needs name), "remove" (needs layer), "rename" (needs layer and name), "set" (needs layer plus visible and/or opacity) or "move" (needs layer and to).
    action: String,
    /// Layer index or name.
    layer: Option<Value>,
    /// New layer name.
    name: Option<String>,
    /// Show or hide the layer.
    visible: Option<bool>,
    /// Layer opacity, 0 to 255.
    opacity: Option<u8>,
    /// For "move": the new index of the layer, 0 = bottom.
    to: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditFrames {
    /// Project name.
    project: String,
    /// Name or id of an animation or image.
    asset: String,
    /// "add" (empty frames at the end), "duplicate" (needs frame), "remove" (needs frame), "copy" (content of from_frame into frame), "set_duration" (needs duration_ms; all frames if frame is omitted) or "set_looping" (animations only, needs looping).
    action: String,
    /// Frame index, 0-based.
    frame: Option<usize>,
    /// Source frame for "copy".
    from_frame: Option<usize>,
    /// Frame duration in milliseconds.
    duration_ms: Option<u32>,
    /// Whether the animation loops.
    looping: Option<bool>,
    /// For "add" and "duplicate": how many frames to add. Default 1.
    count: Option<usize>,
    /// For "duplicate": index where the first copy goes. Default: right after `frame`.
    insert_at: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetPalette {
    /// Project name.
    project: String,
    /// Name or id of an image.
    asset: String,
    /// Palette preset name from get_studio_info.
    palette_preset: Option<String>,
    /// Custom palette as a list of colors.
    palette: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddCharacterColors {
    /// Project name.
    project: String,
    /// Name or id of the character.
    character: String,
    /// Colors to append to the character's palette.
    colors: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetStyle {
    /// Project name.
    project: String,
    /// A style from the style library (see styles), e.g. "Manga" or "8 Bit": sets its palette, light direction and outline color for the project and remembers the style, so get_project and new assets show its rules. Other fields given in the same call override it.
    style: Option<String>,
    /// The project's colors. New images and characters start with them unless they name a palette.
    palette: Option<Vec<String>>,
    /// Palette preset name from get_studio_info, instead of `palette`.
    palette_preset: Option<String>,
    /// Where the light comes from, e.g. "top-left". `shade` uses it when it gives no `light_dir`. "none" removes it.
    light_dir: Option<String>,
    /// Outline color. `outline` uses it when it gives no `color`, and the outline hints check against it. "none" removes it.
    outline_color: Option<String>,
    /// Free style notes for everyone drawing in this project, e.g. "1 px outline, no pure black". Up to 500 characters.
    notes: Option<String>,
    /// Remove the whole style first. Combine with other fields to start over.
    #[serde(default)]
    clear: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct PaletteFromImage {
    /// Path of a PNG, JPG or GIF file on the user's computer. Give this, or `project` and `asset`.
    path: Option<String>,
    /// Project name, together with `asset`.
    project: Option<String>,
    /// Name or id of an image, character or animation in the project.
    asset: Option<String>,
    /// Most colors to return, 1 to 256. Default 16.
    max_colors: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReduceColors {
    /// Project name.
    project: String,
    /// Name or id of an image or a character. A character is reduced together with all its animations.
    asset: String,
    /// Most colors to keep, 1 to 256.
    colors: usize,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteProject {
    /// Project name.
    project: String,
    /// Repeat the project name exactly, as a safeguard against deleting the wrong project.
    confirm: String,
    /// Also delete the project's exported files. Default false.
    #[serde(default)]
    delete_exports: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetOutline {
    /// Project name.
    project: String,
    /// Name or id of an image, a character or an animation. A character gets the outline together with all its animations unless `with_animations` is false.
    asset: String,
    /// "set" (default) creates or changes the outline, "remove" deletes it with its layer, "freeze" stops the automatic and keeps the outline as a normal layer you can edit by hand.
    action: Option<String>,
    /// "selective" (each outline pixel is a darker shade of the color it touches, lighter towards the light) or a fixed color like "#1a1c2c". Default for a new outline: the project style's outline color, else selective.
    color: Option<String>,
    /// true: color the edge pixels of the shape instead of adding pixels outside it, so the shape does not grow. Default false.
    inside: Option<bool>,
    /// true: also count diagonal neighbours, which closes corner gaps and makes slopes look thicker. Default false.
    corners: Option<bool>,
    /// Thickness in pixels, 1 to 4. Default 1.
    thickness: Option<u32>,
    /// true: also draw lines where an upper layer meets a lower one (an arm in front of the body). Default false.
    inner: Option<bool>,
    /// Where the light comes from, e.g. "top-left"; a selective outline is lighter on that side. "none" for even outlines. Default: the project style's light direction.
    light_dir: Option<String>,
    /// Name of the outline layer. Default "outline".
    layer: Option<String>,
    /// For a character: also give all its animations the same outline. Default true.
    with_animations: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetLight {
    /// Project name.
    project: String,
    /// Name or id of an image, a character (its base pose; new animations inherit the light) or an animation.
    asset: String,
    /// "set" (default) creates or changes the light, "remove" deletes it with its layers, "freeze" stops the automatic and keeps the layers as normal layers you can edit by hand.
    action: Option<String>,
    /// Where the light source is: a direction like "top-left" (far outside, nearly parallel light) or a point [x, y] in canvas pixels (may be outside, e.g. a torch at [-4, 10]). With `to` the light moves from here to there over the frames.
    from: Option<Value>,
    /// Where the light source ends up in the last frame: a direction or [x, y]. Without `from` the current start is kept.
    to: Option<Value>,
    /// Light path through several points [[x, y], ...] (1 to 32), spread evenly over the frames from first to last. Alternative to from/to.
    path: Option<Vec<[f32; 2]>>,
    /// Width of the lit and shaded edges in pixels, 1 to 4. Default 1.
    width: Option<u32>,
    /// 1 (one palette step lighter/darker) or 2 (the outermost edge pixels two steps). Default 1.
    strength: Option<u32>,
    /// "palette" (default: lighter along the palette's color ramps) or a light color like "#ffb347" for a torch; lit edges blend towards it.
    color: Option<String>,
    /// Darken the edges facing away from the light. Default true.
    shadows: Option<bool>,
    /// Add a flat ground shadow below the shape that falls away from the light, on its own bottom layer. Default false (or keep the current setting).
    cast_shadow: Option<bool>,
    /// Ground shadow length relative to the shape's height, 10 to 300 percent. Default 100.
    cast_length: Option<u32>,
    /// Ground shadow layer opacity, 0 to 255. Default 110.
    cast_opacity: Option<u8>,
    /// Ground shadow color, "auto" (darkest palette color, default) or a color.
    cast_color: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImportImage {
    /// Project name.
    project: String,
    /// Full path of a PNG, JPG or GIF file on the user's computer.
    path: String,
    /// What to create: "image" (default), "character" (frame 0 becomes the base pose and the file's colors become the locked palette), "animation" (of an existing character, see `character`) or "reference" (a semi-transparent tracing layer at the bottom of an existing `asset`, fitted to its canvas; not part of the picture, left out of export, palette and outline).
    #[serde(rename = "as")]
    kind: Option<String>,
    /// Name of the new asset. Default: the file name; for animations "<character> <kind>".
    name: Option<String>,
    /// For as="animation": name or id of the character the animation belongs to. Frames must have the character's size.
    character: Option<String>,
    /// Animation kind: idle, walk, run, jump, attack, hurt, death or a custom name. For as="animation" it sets the kind (default: the file name). For as="character" with a multi-frame file it also keeps all frames as an animation of the new character.
    animation: Option<String>,
    /// Sprite sheet: width of one frame in true pixels (after downscale). Use with frame_height.
    frame_width: Option<u32>,
    /// Sprite sheet: height of one frame in true pixels (after downscale).
    frame_height: Option<u32>,
    /// Sprite sheet: number of frames per row. Alternative to frame_width/frame_height.
    columns: Option<u32>,
    /// Sprite sheet: number of rows. Default 1.
    rows: Option<u32>,
    /// Integer factor to scale the file down by. Default: detect automatically, so pixel art that was saved enlarged (every pixel a block of N×N) comes in at its true size. 1 keeps the file as it is.
    downscale: Option<u32>,
    /// 8, 16 or 32 (default). Ignored for animations, which use their character's depth.
    color_depth_bits: Option<u8>,
    /// For as="animation": "add" (default) appends colors the character does not have yet to its palette, so the frames stay pixel exact; "snap" replaces them with the nearest palette color.
    new_colors: Option<String>,
    /// Keep fully transparent frames at the end of a sprite sheet. Default false.
    keep_empty_frames: Option<bool>,
    /// Turn a photo, painting or drawing into pixel art: scale it down to a target size, reduce the colors, optionally dither, remove the background and add an outline. Without it the file is taken as pixel art (exact pixels). For as="reference" only `background`/`tolerance` apply.
    pixelate: Option<PixelateOpts>,
    /// For as="reference": name or id of the character, animation or image that gets the tracing layer.
    asset: Option<String>,
    /// For as="reference": opacity of the tracing layer, 0 to 255. Default 128.
    opacity: Option<u8>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct PixelateOpts {
    /// Target width in pixels. Give width and/or height; the missing one keeps the aspect ratio.
    width: Option<u32>,
    /// Target height in pixels.
    height: Option<u32>,
    /// Without width/height: the longer side in pixels, aspect ratio kept. Default 64. For as="animation" the default is the character's size.
    size: Option<u32>,
    /// Number of colors, 2 to 256. Default 16. Ignored when `palette` gives fixed colors.
    colors: Option<usize>,
    /// Which colors to use: "image" (default: the best `colors` colors of the picture), "project" (the project style palette) or the name of an asset whose palette to use. For as="animation" the default is the character's palette.
    palette: Option<String>,
    /// "none" (default, flat areas), "ordered" (even checker pattern, calm gradients) or "diffusion" (Floyd-Steinberg, most accurate, grainier).
    dither: Option<String>,
    /// "keep" (default), "auto" (the most common edge color, flood-filled from the edges, becomes transparent) or a color like "#ffffff".
    background: Option<String>,
    /// How similar a color must be to the background to be removed, 0 to 100 percent. Default 12.
    tolerance: Option<u8>,
    /// "none" (default), "auto" (darkest palette color, or a very dark shade of it) or a color: a 1-pixel outline around everything opaque.
    outline: Option<String>,
    /// Boost saturation and contrast before reducing colors; helps washed-out photos. Default false.
    vivid: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct Export {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// "png", "png8" (indexed PNG with a color table, at most 256 colors, same pixels, smaller file), "jpg", "sheet" (sprite sheet PNG plus JSON metadata with frame positions and durations), "gif" (animated, exact colors up to 255 plus transparency, uses the frame durations and the animation's loop setting) or "webm" (video with transparency, needs ffmpeg on the computer and an even scale). For a tile set made by autotile also "godot" (PNG plus a Godot 4 TileSet .tres with terrain) or "tiled" (PNG plus a Tiled .tsx with a Wang set); without `frame` all frames become animated tiles.
    format: String,
    /// Frame index for png/png8/jpg. Omit to export every frame as its own file.
    frame: Option<usize>,
    /// Integer enlargement factor, 1 to 64, without smoothing. Default 1 (true size).
    scale: Option<u32>,
    /// Sheet only: frames per row. Default: all frames in one row.
    columns: Option<u32>,
    /// JPG only: background color replacing transparency. Default white.
    background: Option<String>,
    /// JPG only: quality 1 to 100. Default 95.
    quality: Option<u8>,
    /// Also export a normal map for dynamic 2D lighting in game engines (Godot, Unity), as files with the suffix _n in the same format (png or sheet). Give {} for the defaults.
    normal_map: Option<NormalMapOpts>,
}

// ------------------------------------------------------------------ Werkzeuge

#[derive(Deserialize, JsonSchema)]
pub struct CustomShapes {
    /// "list" (default), "save" (needs name, project and asset) or "delete" (needs name).
    action: Option<String>,
    /// Name of the custom shape.
    name: Option<String>,
    /// For "save": project of the asset to take the shape from.
    project: Option<String>,
    /// For "save": name or id of the character, animation or image.
    asset: Option<String>,
    /// For "save": frame index. Default 0.
    frame: Option<usize>,
    /// For "save": layer index or name. Default 0.
    layer: Option<Value>,
    /// For "save": only this area [x, y, w, h]. The shape is trimmed to what is drawn inside.
    area: Option<[i64; 4]>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ResizeCanvas {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// New width in pixels. Default: unchanged.
    width: Option<u32>,
    /// New height in pixels. Default: unchanged.
    height: Option<u32>,
    /// Where the existing content stays: "center" (default), "top", "bottom", "left",
    /// "right" or a corner like "bottom-left". For characters "bottom" keeps the feet in place.
    anchor: Option<String>,
    /// Instead of a size: cut the canvas down to the smallest box around everything drawn,
    /// over all frames and layers.
    #[serde(default)]
    crop_to_content: bool,
}

#[tool_router]
impl PixelServer {
    #[tool(
        description = "Start here. Lists the workspace folder, existing projects, canvas templates, palette presets, color depths and animation presets with tips.",
        annotations(title = "Studio info", read_only_hint = true)
    )]
    fn get_studio_info(&self) -> CallToolResult {
        self.run(|s| s.studio_info())
    }

    #[tool(
        description = "Create a new, empty project. A project holds characters, their animations and free images.",
        annotations(title = "Create project", destructive_hint = false)
    )]
    fn create_project(&self, Parameters(a): Parameters<ProjectOnly>) -> CallToolResult {
        self.run(|s| s.create_project(&a.project))
    }

    #[tool(
        description = "Overview of a project: its style, characters with palettes and animations, images, sizes, layers, frame counts, consistency issues, and `hints` on likely slips per asset (stray pixels, gaps in an outline, colors used by a single pixel, tiles that do not repeat seamlessly).",
        annotations(title = "Project overview", read_only_hint = true)
    )]
    fn get_project(&self, Parameters(a): Parameters<ProjectOnly>) -> CallToolResult {
        self.run(|s| s.get_project(&a.project))
    }

    #[tool(
        description = "Create a free image (tile, item, icon, scene) from a template or with a custom size up to 1024×1024. `layers` and `frames` set it up in one step.",
        annotations(title = "Create image", destructive_hint = false)
    )]
    fn create_image(&self, Parameters(a): Parameters<CreateImage>) -> CallToolResult {
        self.run(|s| {
            s.create_image(
                &a.project,
                &a.name,
                a.template.as_deref(),
                a.width,
                a.height,
                a.color_depth_bits,
                a.palette_preset.as_deref(),
                a.palette.as_deref(),
                a.style.as_deref(),
                a.layers.as_deref(),
                a.frames,
            )
        })
    }

    #[tool(
        description = "Create a character: a reusable asset with a fixed size and a locked palette. Draw its base pose afterwards, then derive animations from it. Use this for anything that must look the same across animations and projects.",
        annotations(title = "Create character", destructive_hint = false)
    )]
    fn create_character(&self, Parameters(a): Parameters<CreateCharacter>) -> CallToolResult {
        self.run(|s| {
            s.create_character(
                &a.project,
                &a.name,
                a.description.as_deref(),
                a.template.as_deref(),
                a.width,
                a.height,
                a.color_depth_bits,
                a.palette_preset.as_deref(),
                a.palette.as_deref(),
                a.style.as_deref(),
                a.layers.as_deref(),
            )
        })
    }

    #[tool(
        description = "Create an animation for a character. Every frame starts as an exact copy of the base pose, so the character stays identical. Draw the base pose first.",
        annotations(title = "Create animation", destructive_hint = false)
    )]
    fn create_animation(&self, Parameters(a): Parameters<CreateAnimation>) -> CallToolResult {
        self.run(|s| s.create_animation(&a.project, &a.character, &a.kind, a.frames))
    }

    #[tool(
        description = "Delete a whole project file for good, optionally with its exported files. Only do this when the user asked for it; `confirm` must repeat the project name. This cannot be undone.",
        annotations(title = "Delete project", destructive_hint = true)
    )]
    fn delete_project(&self, Parameters(a): Parameters<DeleteProject>) -> CallToolResult {
        self.run(|s| s.delete_project(&a.project, &a.confirm, a.delete_exports))
    }

    #[tool(
        description = "Delete a character (with all its animations), an animation or an image. This cannot be undone.",
        annotations(title = "Delete asset", destructive_hint = true)
    )]
    fn delete_asset(&self, Parameters(a): Parameters<AssetOnly>) -> CallToolResult {
        self.run(|s| s.delete_asset(&a.project, &a.asset))
    }

    #[tool(
        description = "Copy a character with all its animations into another project, pixel-identical. If the character already exists there, it is updated.",
        annotations(title = "Copy character to project", destructive_hint = false)
    )]
    fn copy_character(&self, Parameters(a): Parameters<CopyCharacter>) -> CallToolResult {
        self.run(|s| s.copy_character(&a.from_project, &a.character, &a.to_project))
    }

    #[tool(
        description = "Draw on one layer with a list of operations, on one frame or on several at once (`frame`: index, list or \"all\"; `per_frame` for what differs). Basic ops: pixels, line, rect, ellipse, shape (heart, diamond, star, triangle, cross, hexagon, arrow, circle or a saved custom shape), fill, grid, spray, replace_color, shift, flip_h, flip_v, clear, and for an area move_area, flip_area, clear_area. Pixel-art techniques: outline, shade (light and shadow), lighten and darken (one step along the palette's color ramps), dither, gradient (hard steps), noise, shader (per-pixel formula), stamp (copy and paste a piece). `mirror` draws symmetrically. Everything is pixel-exact, without anti-aliasing, and respects locked palettes. Returns a preview of the frames drawn on. Drawing with color \"transparent\" erases.",
        annotations(title = "Draw", destructive_hint = false)
    )]
    fn draw(&self, Parameters(a): Parameters<Draw>) -> CallToolResult {
        self.run(|s| {
            s.draw(
                &a.project,
                &a.asset,
                a.frame.as_ref(),
                a.layer.as_ref(),
                a.ops,
                a.per_frame,
                a.mirror.as_deref(),
            )
        })
    }

    #[tool(
        description = "Take back the last draw calls of this session in a project, newest first. An asset that was changed in the meantime (in the editor or by another tool) is left alone.",
        annotations(title = "Undo draw", destructive_hint = true)
    )]
    fn undo(&self, Parameters(a): Parameters<Undo>) -> CallToolResult {
        self.run(|s| s.undo(&a.project, a.steps))
    }

    #[tool(
        description = "Look at an asset: returns an enlarged preview image of one frame, or of all frames side by side. Options: `grid` (pixel grid with coordinates), `zoom_region` (a detail), `tile` (3×3 to check seams), `background`.",
        annotations(title = "View", read_only_hint = true)
    )]
    fn view(&self, Parameters(a): Parameters<View>) -> CallToolResult {
        let options = ViewOptions {
            grid: a.grid,
            zoom_region: a.zoom_region,
            tile: a.tile,
            background: a.background,
        };
        self.run(|s| {
            s.view(
                &a.project,
                &a.asset,
                a.frame.as_ref(),
                a.layer.as_ref(),
                &options,
            )
        })
    }

    #[tool(
        description = "Read exact pixels as text rows plus a color legend (region up to 128×128). The result can be edited and passed back to the grid op of draw.",
        annotations(title = "Read pixels", read_only_hint = true)
    )]
    fn get_pixels(&self, Parameters(a): Parameters<GetPixels>) -> CallToolResult {
        self.run(|s| {
            s.get_pixels(
                &a.project,
                &a.asset,
                a.frame,
                a.layer.as_ref(),
                a.x,
                a.y,
                a.w,
                a.h,
            )
        })
    }

    #[tool(
        description = "Add, remove, rename, show/hide layers or set their opacity. Layers are shared by all frames of an asset.",
        annotations(title = "Edit layers", destructive_hint = true)
    )]
    fn edit_layers(&self, Parameters(a): Parameters<EditLayers>) -> CallToolResult {
        self.run(|s| {
            s.edit_layers(
                &a.project,
                &a.asset,
                &a.action,
                a.layer.as_ref(),
                a.name.as_deref(),
                a.visible,
                a.opacity,
                a.to,
            )
        })
    }

    #[tool(
        description = "Add, duplicate, remove or copy frames (`count` adds several at once), set frame durations, or set whether an animation loops.",
        annotations(title = "Edit frames", destructive_hint = true)
    )]
    fn edit_frames(&self, Parameters(a): Parameters<EditFrames>) -> CallToolResult {
        self.run(|s| {
            s.edit_frames(
                &a.project,
                &a.asset,
                &a.action,
                a.frame,
                a.from_frame,
                a.duration_ms,
                a.looping,
                a.count,
                a.insert_at,
            )
        })
    }

    #[tool(
        description = "Replace the palette of an image. In 8-bit images existing pixels are remapped to the nearest new color. Not allowed for characters.",
        annotations(title = "Set image palette", destructive_hint = true)
    )]
    fn set_palette(&self, Parameters(a): Parameters<SetPalette>) -> CallToolResult {
        self.run(|s| {
            s.set_palette(
                &a.project,
                &a.asset,
                a.palette_preset.as_deref(),
                a.palette.as_deref(),
            )
        })
    }

    #[tool(
        description = "Append colors to a character's locked palette. Existing colors and pixels stay unchanged; the new colors are also available in all animations of the character.",
        annotations(title = "Add character colors", destructive_hint = false)
    )]
    fn add_character_colors(
        &self,
        Parameters(a): Parameters<AddCharacterColors>,
    ) -> CallToolResult {
        self.run(|s| s.add_character_colors(&a.project, &a.character, &a.colors))
    }

    #[tool(
        description = "Set the project style that all its assets share: palette, light direction, outline color and notes. New images and characters start with the palette, `shade` and `outline` use light direction and outline color when a call leaves them out, and get_project shows the style to everyone working on the project. Fields left out stay as they are.",
        annotations(title = "Set project style", destructive_hint = false)
    )]
    fn set_style(&self, Parameters(a): Parameters<SetStyle>) -> CallToolResult {
        self.run(|s| {
            s.set_style(
                &a.project,
                &StyleArgs {
                    style: a.style.as_deref(),
                    palette_preset: a.palette_preset.as_deref(),
                    palette: a.palette.as_deref(),
                    light_dir: a.light_dir.as_deref(),
                    outline_color: a.outline_color.as_deref(),
                    notes: a.notes.as_deref(),
                    clear: a.clear,
                },
            )
        })
    }

    #[tool(
        description = "Read the main colors of an image file or an asset as a palette, sorted dark to light, with a swatch preview. Similar colors are merged and the most used one of each group is kept, so the result only has colors that really occur. Use it to take over the colors of reference art or of an existing asset.",
        annotations(title = "Palette from image", read_only_hint = true)
    )]
    fn palette_from_image(&self, Parameters(a): Parameters<PaletteFromImage>) -> CallToolResult {
        self.run(|s| {
            s.palette_from_image(
                a.path.as_deref(),
                a.project.as_deref(),
                a.asset.as_deref(),
                a.max_colors,
            )
        })
    }

    #[tool(
        description = "Reduce an image, or a character together with all its animations, to at most `colors` colors. The most similar colors are merged into the most used color of their group, so no new mixed colors appear and rare but distinct colors (highlights) survive. The palette becomes exactly the remaining colors. Returns a preview.",
        annotations(title = "Reduce colors", destructive_hint = true)
    )]
    fn reduce_colors(&self, Parameters(a): Parameters<ReduceColors>) -> CallToolResult {
        self.run(|s| s.reduce_colors(&a.project, &a.asset, a.colors))
    }

    #[tool(
        description = "Give an image, a character (with all its animations) or an animation an automatic outline on its own top layer. It is computed from all other visible layers and redrawn after every change in every frame, by you or in the editor, so draw the shapes without outline and let this follow. Options: selective (darker shade of the touching color, lighter towards the light) or a fixed color, outside or inside the shape, corners, thickness, inner lines between layers. Calling it again changes only the options given. Returns a preview.",
        annotations(title = "Automatic outline", destructive_hint = false)
    )]
    fn set_outline(&self, Parameters(a): Parameters<SetOutline>) -> CallToolResult {
        self.run(|s| {
            s.set_outline(&OutlineArgs {
                project: &a.project,
                asset: &a.asset,
                action: a.action.as_deref(),
                color: a.color.as_deref(),
                inside: a.inside,
                corners: a.corners,
                thickness: a.thickness,
                inner: a.inner,
                light_dir: a.light_dir.as_deref(),
                layer: a.layer.as_deref(),
                with_animations: a.with_animations,
            })
        })
    }

    #[tool(
        description = "The style library. Built-in styles: 8 Bit, 16 Bit, HQ Pixel Art, Comic, Manga and Castles; the user can add own ones. action \"list\" (default) names all styles with a description; \"get\" returns one style completely: palette, color depth, usual sizes, colors per sprite, outline, shades, dithering, hue shifting, anti-aliasing, light direction and rules in words. Follow these exactly when drawing in that style. \"save\" creates or changes an own style (based_on copies another style first, from_project takes over a project's style), \"delete\" removes an own one. Use a style with set_style(style) for a whole project, or with create_image/create_character(style).",
        annotations(title = "Style library", destructive_hint = false)
    )]
    fn styles(&self, Parameters(a): Parameters<StylesParams>) -> CallToolResult {
        self.run(|s| s.styles(&a))
    }

    #[tool(
        description = "Give an image, a character's base pose or an animation automatic light and shadow from a light source that can move over the frames (a torch passing by, the sun crossing the sky, a flash). Edges facing the light become one palette step lighter (or blend towards a light color), edges facing away one step darker, each on its own layer, plus an optional ground shadow that falls away from the light. Everything is redrawn from the other visible layers after every change in every frame, like the automatic outline. Calling it again changes only the options given. Returns a preview of the frames.",
        annotations(title = "Automatic light and shadow", destructive_hint = false)
    )]
    fn set_light(&self, Parameters(a): Parameters<SetLight>) -> CallToolResult {
        self.run(|s| {
            s.set_light(&LightArgs {
                project: &a.project,
                asset: &a.asset,
                action: a.action.as_deref(),
                from: a.from.as_ref(),
                to: a.to.as_ref(),
                path: a.path.as_deref(),
                width: a.width,
                strength: a.strength,
                color: a.color.as_deref(),
                shadows: a.shadows,
                cast_shadow: a.cast_shadow,
                cast_length: a.cast_length,
                cast_opacity: a.cast_opacity,
                cast_color: a.cast_color.as_deref(),
            })
        })
    }

    #[tool(
        description = "Effects that run over the frames of an image, an animation or a character's base pose, each on its own layer and redrawn after every change, like the automatic outline: \"glow\" (a pulsing halo behind the figure, or with `colors` only around glowing parts like eyes or a blade), \"lightning\" (a jagged flickering bolt from `from` to `to` with branches), \"explosion\" (flash, fireball, debris and smoke around `at`, played once), \"reflection\" (a tinted mirror image in water below the figure with moving waves), \"sparks\", \"smoke\" and \"magic\" (looping particles from `at`) and \"color_cycle\" (palette animation: pixels of the given ramp colors shift one color per step, for flowing water, flickering fire, glowing lava; `preset` gives ready ramps to paint with). Use `frames` to turn a still image into a loop first. Calling again with the effect's `layer` changes only the options given; action \"list\", \"remove\" or \"freeze\". Returns a preview of the frames.",
        annotations(title = "Effects and color cycling", destructive_hint = false)
    )]
    fn effects(&self, Parameters(a): Parameters<EffectsParams>) -> CallToolResult {
        self.run(|s| s.effects(&a))
    }

    #[tool(
        description = "Autotile: draw only a few tiles and get the whole terrain tile set. action \"template\" creates an empty template image (2 tiles wide, 3 tall, with a guide layer): a 2×2 block of terrain with outer corners, edges and fill, plus one tile with the four inner corners. action \"build\" turns the drawn template into all 47 tiles (blob47: corners and sides, with inner corners) or 16 tiles (sides16) as a new image, every tile assembled from quarters of the template, in every frame (animated tiles work). action \"preview\" shows a set on a sample map. Export the set with export format \"godot\" or \"tiled\" for terrain painting in the engine. Returns a preview with the set and a sample map.",
        annotations(title = "Autotile tile sets", destructive_hint = false)
    )]
    fn autotile(&self, Parameters(a): Parameters<AutotileParams>) -> CallToolResult {
        self.run(|s| s.autotile(&a))
    }

    #[tool(
        description = "Custom shapes the user or you drew and saved: list them, save a new one from a drawing (one layer of a frame, optionally an area; trimmed to what is drawn) or delete one. Saved shapes live in the workspace, work in every project and can be drawn at any size with the draw op `shape`, in one color or in their own colors (`keep_colors`).",
        annotations(title = "Custom shapes", destructive_hint = true)
    )]
    fn custom_shapes(&self, Parameters(a): Parameters<CustomShapes>) -> CallToolResult {
        self.run(|s| {
            s.custom_shapes(
                a.action.as_deref().unwrap_or("list"),
                a.name.as_deref(),
                a.project.as_deref(),
                a.asset.as_deref(),
                a.frame,
                a.layer.as_ref(),
                a.area,
            )
        })
    }

    #[tool(
        description = "Change the canvas size of an asset without scaling: add room around the content or cut it away, keeping the content at `anchor`, or crop to the drawn content. A character and all its animations always change together so they stay the same size; the anchor point moves along. Returns a preview.",
        annotations(title = "Resize canvas", destructive_hint = true)
    )]
    fn resize_canvas(&self, Parameters(a): Parameters<ResizeCanvas>) -> CallToolResult {
        self.run(|s| {
            s.resize_canvas(
                &a.project,
                &a.asset,
                a.width,
                a.height,
                a.anchor.as_deref(),
                a.crop_to_content,
            )
        })
    }

    #[tool(
        description = "Import an existing PNG, JPG or GIF file from the user's computer as an image, a character or an animation of a character. Splits sprite sheets into frames, reads the frames and timing of animated GIFs, and brings enlarged pixel art back to its true pixel size. With `pixelate` it turns a photo or drawing into pixel art (target size, colors or a palette, dithering, background removal, outline). With as=\"reference\" it adds the file as a semi-transparent tracing layer to an existing asset. Returns a preview.",
        annotations(title = "Import image", destructive_hint = false)
    )]
    fn import_image(&self, Parameters(a): Parameters<ImportImage>) -> CallToolResult {
        self.run(|s| {
            s.import_image(&ImportArgs {
                project: &a.project,
                path: &a.path,
                name: a.name.as_deref(),
                kind: a.kind.as_deref(),
                character: a.character.as_deref(),
                animation: a.animation.as_deref(),
                frame_width: a.frame_width,
                frame_height: a.frame_height,
                columns: a.columns,
                rows: a.rows,
                downscale: a.downscale,
                depth_bits: a.color_depth_bits,
                new_colors: a.new_colors.as_deref(),
                keep_empty_frames: a.keep_empty_frames.unwrap_or(false),
                pixelate: a.pixelate.as_ref().map(|x| PixelateArgs {
                    width: x.width,
                    height: x.height,
                    size: x.size,
                    colors: x.colors,
                    palette: x.palette.as_deref(),
                    dither: x.dither.as_deref(),
                    background: x.background.as_deref(),
                    tolerance: x.tolerance,
                    outline: x.outline.as_deref(),
                    vivid: x.vivid,
                }),
                asset: a.asset.as_deref(),
                opacity: a.opacity,
            })
        })
    }

    #[tool(
        description = "Export an asset to files in the workspace's exports folder: single frames as PNG or JPG, a sprite sheet with JSON metadata, or the whole animation as GIF or WebM. Returns the file paths.",
        annotations(title = "Export", destructive_hint = false)
    )]
    fn export(&self, Parameters(a): Parameters<Export>) -> CallToolResult {
        self.run(|s| {
            s.export(
                &a.project,
                &a.asset,
                &a.format,
                a.frame,
                a.scale,
                a.columns,
                a.background.as_deref(),
                a.quality,
                a.normal_map.as_ref(),
            )
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for PixelServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                crate::SERVER_NAME,
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }
}
