//! MCP-Anbindung: Parameter-Typen und Werkzeug-Definitionen. Die Logik liegt in `studio`.

use crate::studio::{DrawOp, ImportArgs, PerFrame, Reply, Res, Studio, ViewOptions};
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
2. create_project, then create_character (for anything that will be animated or reused) or create_image (tiles, items, scenes).
3. draw on it. For exact shapes prefer the `grid` op: text rows plus a legend give control over every pixel. Use layers (e.g. legs, body, head, weapon) so parts can move independently in animations; create_image and create_character take `layers`, create_image also `frames`.
4. Add depth with the technique ops instead of placing every pixel by hand: `outline` around a shape, `shade` for light and shadow from one direction, `gradient` (hard bands) and `dither` for transitions, `noise` for grain, `stamp` to repeat a piece, and `shader` (a per-pixel math formula with x, y, frame, dist(), noise(), rand()) for glow, light and anything procedural. Most take an area (x, y, w, h) and a `mask` ("opaque", "transparent" or a color) so they only touch the part you mean.
5. Every draw call returns a preview image. Look at it and correct mistakes before continuing; `undo` takes back the last draw calls. view has `grid` (coordinates), `zoom_region` and `tile` (seam check); get_pixels reads exact pixels back as text.
6. Animation: create_animation (idle, walk, run, jump, attack, hurt, death or a custom name). All frames start as exact copies of the character's base pose; change only what moves. One draw call can cover several frames: `frame: "all"` applies the ops to every frame, and `per_frame` gives each frame its own color substitutions or extra ops (e.g. a shift, or a brighter glow color). In `shader` use `frame` or `t` to animate.
7. export as png, jpg, sheet (sprite sheet with JSON metadata), gif or webm (animated).
Existing art: import_image brings a PNG, JPG or GIF file (also sprite sheets) into the project as an image, a character or an animation.

Consistency rules:
- A character's palette is locked when it is created. Colors drawn on the character or its animations snap to the nearest palette color, so choose the full palette up front (skin, hair, clothes, outline, shadows, highlights). add_character_colors appends colors later without changing existing ones.
- Animations always have the size, color depth and palette of their character. get_project lists any consistency issues.
- To reuse a character in another project use copy_character; it stays pixel-identical.

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
    /// "add" (needs name), "remove" (needs layer), "rename" (needs layer and name) or "set" (needs layer plus visible and/or opacity).
    action: String,
    /// Layer index or name.
    layer: Option<Value>,
    /// New layer name.
    name: Option<String>,
    /// Show or hide the layer.
    visible: Option<bool>,
    /// Layer opacity, 0 to 255.
    opacity: Option<u8>,
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
pub struct ImportImage {
    /// Project name.
    project: String,
    /// Full path of a PNG, JPG or GIF file on the user's computer.
    path: String,
    /// What to create: "image" (default), "character" (frame 0 becomes the base pose and the file's colors become the locked palette) or "animation" (of an existing character, see `character`).
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
}

#[derive(Deserialize, JsonSchema)]
pub struct Export {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// "png", "png8" (indexed PNG with a color table, at most 256 colors, same pixels, smaller file), "jpg", "sheet" (sprite sheet PNG plus JSON metadata with frame positions and durations), "gif" (animated, exact colors up to 255 plus transparency, uses the frame durations and the animation's loop setting) or "webm" (video with transparency, needs ffmpeg on the computer and an even scale).
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
}

// ------------------------------------------------------------------ Werkzeuge

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
        description = "Overview of a project: characters with palettes and animations, images, sizes, layers, frame counts, and any consistency issues.",
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
        description = "Draw on one layer with a list of operations, on one frame or on several at once (`frame`: index, list or \"all\"; `per_frame` for what differs). Basic ops: pixels, line, rect, ellipse, fill, grid, replace_color, shift, flip_h, flip_v, clear. Pixel-art techniques: outline, shade (light and shadow), dither, gradient (hard steps), noise, shader (per-pixel formula), stamp (copy and paste a piece). Everything is pixel-exact, without anti-aliasing, and respects locked palettes. Returns a preview of the frames drawn on. Drawing with color \"transparent\" erases.",
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
        description = "Import an existing PNG, JPG or GIF file from the user's computer as an image, a character or an animation of a character. Splits sprite sheets into frames, reads the frames and timing of animated GIFs, and brings enlarged pixel art back to its true pixel size. Returns a preview.",
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
