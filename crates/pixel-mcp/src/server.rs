//! MCP-Anbindung: Parameter-Typen und Werkzeug-Definitionen. Die Logik liegt in `studio`.

use crate::studio::{DrawOp, Reply, Res, Studio};
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
3. draw on it. Prefer the `grid` op: text rows plus a legend give exact control over every pixel. Use layers (e.g. legs, body, head, weapon) so parts can move independently in animations.
4. Every draw call returns a preview image. Look at it and correct mistakes before continuing. Use get_pixels to read exact pixels back as text.
5. create_animation (idle, walk, run, jump, attack, hurt, death or a custom name). All frames start as exact copies of the character's base pose; change only what moves in each frame (shift op on one layer, small grid patches).
6. export as png, jpg or sheet (sprite sheet with JSON metadata).

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
    /// Frame index, 0-based. Default 0.
    frame: Option<usize>,
    /// Layer index (0 = bottom) or layer name. Default 0.
    layer: Option<Value>,
    /// Operations, applied in order. If one fails, nothing is saved.
    ops: Vec<DrawOp>,
}

#[derive(Deserialize, JsonSchema)]
pub struct View {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// Frame index. Omit to see all frames side by side.
    frame: Option<usize>,
    /// Layer index or name to see only that layer. Omit for the full picture.
    layer: Option<Value>,
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
    /// "add" (empty frame at the end), "duplicate" (needs frame), "remove" (needs frame), "copy" (content of from_frame into frame), "set_duration" (needs duration_ms; all frames if frame is omitted) or "set_looping" (animations only, needs looping).
    action: String,
    /// Frame index, 0-based.
    frame: Option<usize>,
    /// Source frame for "copy".
    from_frame: Option<usize>,
    /// Frame duration in milliseconds.
    duration_ms: Option<u32>,
    /// Whether the animation loops.
    looping: Option<bool>,
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
pub struct Export {
    /// Project name.
    project: String,
    /// Name or id of a character, animation or image.
    asset: String,
    /// "png", "jpg" or "sheet" (sprite sheet PNG plus JSON metadata with frame positions and durations).
    format: String,
    /// Frame index for png/jpg. Omit to export every frame as its own file.
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
        description = "Create a free image (tile, item, icon, scene) from a template or with a custom size up to 1024×1024.",
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
        description = "Draw on one layer of one frame with a list of operations: pixels, line, rect, ellipse, fill, grid, replace_color, shift, flip_h, clear. Returns a preview image of the frame. Drawing with color \"transparent\" erases.",
        annotations(title = "Draw", destructive_hint = false)
    )]
    fn draw(&self, Parameters(a): Parameters<Draw>) -> CallToolResult {
        self.run(|s| s.draw(&a.project, &a.asset, a.frame, a.layer.as_ref(), a.ops))
    }

    #[tool(
        description = "Look at an asset: returns an enlarged preview image of one frame, or of all frames side by side.",
        annotations(title = "View", read_only_hint = true)
    )]
    fn view(&self, Parameters(a): Parameters<View>) -> CallToolResult {
        self.run(|s| s.view(&a.project, &a.asset, a.frame, a.layer.as_ref()))
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
        description = "Add, duplicate, remove or copy frames, set frame durations, or set whether an animation loops.",
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
        description = "Export an asset to files in the workspace's exports folder: single frames as PNG or JPG, or a sprite sheet with JSON metadata. Returns the file paths.",
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
                "pixelart-studio",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }
}
