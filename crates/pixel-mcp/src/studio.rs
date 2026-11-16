//! Die eigentliche Logik hinter den MCP-Werkzeugen, ohne Protokoll-Details.
//!
//! Jede Operation lädt das Projekt aus dem Arbeitsordner, ändert es und
//! speichert es sofort wieder. So sieht ein geöffneter Editor jede Änderung,
//! und nach einem Absturz geht nichts verloren.

use pixel_core::draw::{self, Target};
use pixel_core::import::{self, Grid, NewColors, Shrink};
use pixel_core::project::CharacterBundle;
use pixel_core::workspace::{
    self, ExportFormat, NameClash, ProjectLock, ProjectNameError, Workspace,
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

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// Größte Seitenlänge, bis zu der `get_pixels` eine Region als Text ausgibt.
const MAX_TEXT_REGION: u32 = 128;
/// Zeichen für die Textdarstellung von Pixeln. `.` steht immer für transparent.
const SYMBOLS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// Zielgröße der längeren Seite eines Vorschaubildes in Bildschirm-Pixeln.
const PREVIEW_TARGET: u32 = 512;
const MAX_OPS: usize = 5000;
const MAX_FRAMES: usize = 256;
const MAX_LAYERS: usize = 64;

pub struct Studio {
    ws: Workspace,
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
    /// Erase the whole layer.
    Clear,
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
}

impl Studio {
    pub fn new(home: PathBuf) -> Res<Self> {
        let ws = Workspace::open(&home)
            .map_err(|err| format!("cannot create workspace {}: {err}", home.display()))?;
        Ok(Self { ws })
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

    fn save(&self, name: &str, p: &Project) -> Res<()> {
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
            "templates": TEMPLATES,
            "palette_presets": ["default16", "gray4", "gray8", "gray16"],
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
        let p = Project::new(name.trim());
        self.save(name, &p)?;
        Ok(Reply::text(format!(
            "Created project '{}' at {}",
            name.trim(),
            path.display()
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
        let characters: Vec<Value> = p
            .characters
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "id": c.id,
                    "description": c.description,
                    "anchor": [c.anchor.0, c.anchor.1],
                    "base": sprite_info(&c.base),
                    "palette": c.palette().colors,
                    "animations": p.animations_of(c.id).map(|a| json!({
                        "name": a.name,
                        "id": a.id,
                        "kind": a.kind.name(),
                        "frames": a.sprite.frames().len(),
                        "frame_ms": a.sprite.frames().iter().map(|f| f.duration_ms).collect::<Vec<_>>(),
                        "looping": a.looping,
                        "layers": a.sprite.layer_names(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let images: Vec<Value> = p
            .images
            .iter()
            .map(|i| {
                let mut v = sprite_info(&i.sprite);
                v["name"] = json!(i.name);
                v["id"] = json!(i.id);
                v
            })
            .collect();
        Ok(Reply::json(json!({
            "project": p.name,
            "characters": characters,
            "images": images,
            "consistency_issues": p.validate(),
        })))
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
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let name = Self::check_new_name(&p, name)?;
        let (w, h) = Self::size(template_id, width, height)?;
        let depth = Self::depth(depth_bits)?;
        let pal = Self::palette(palette_preset, palette, depth)?;
        let sprite = Sprite::new(w, h, depth, pal).map_err(e)?;
        let id = p.add_image(name.clone(), sprite);
        self.save(project, &p)?;
        Ok(Reply::text(format!(
            "Created image '{name}' ({w}×{h}, {} bit), id {id}. It has one empty layer and one frame.",
            depth.bits()
        )))
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
        layers: Option<&[String]>,
    ) -> Res {
        let (_lock, mut p) = self.load_locked(project)?;
        let name = Self::check_new_name(&p, name)?;
        let (w, h) = Self::size(template_id, width, height)?;
        let depth = Self::depth(depth_bits)?;
        let pal = Self::palette(palette_preset, palette, depth)?;
        if pal.is_empty() {
            return Err("a character needs a palette with at least one color".into());
        }
        let mut ch = Character::new(name.clone(), w, h, depth, pal).map_err(e)?;
        ch.description = description.unwrap_or_default().to_string();
        if let Some(layers) = layers.filter(|l| !l.is_empty()) {
            if layers.len() > MAX_LAYERS {
                return Err(format!("at most {MAX_LAYERS} layers"));
            }
            ch.base.rename_layer(0, layers[0].clone()).map_err(e)?;
            for l in &layers[1..] {
                ch.base.add_layer(l.clone());
            }
        }
        let info = json!({
            "created": name,
            "id": ch.id,
            "size": [w, h],
            "color_depth_bits": depth.bits(),
            "layers": ch.base.layer_names(),
            "palette": ch.palette().colors,
            "note": "The palette is now locked: every color you draw on this character or its animations snaps to the nearest palette color. Use add_character_colors to extend it. Draw the base pose next, then create animations from it.",
        });
        p.add_character(ch);
        self.save(project, &p)?;
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
        self.save(project, &p)?;
        Ok(Reply::json(info))
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

        let (created, report) = match a.kind.unwrap_or("image").to_ascii_lowercase().as_str() {
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
                    "unknown `as` value '{other}', use image, character or animation"
                ))
            }
        };
        if report.changed > 0 {
            notes.push(format!(
                "{} of {} colors were changed to fit the color depth or palette.",
                report.changed, report.source_colors
            ));
        }
        self.save(a.project, &p)?;
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
            image_png: Some(preview_png(s, &shown, None)?),
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
        self.save(project, &p)?;
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
        self.save(to_project, &dst)?;
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

    pub fn draw(
        &self,
        project: &str,
        asset: &str,
        frame: Option<usize>,
        layer: Option<&Value>,
        ops: Vec<DrawOp>,
    ) -> Res {
        if ops.is_empty() {
            return Err("ops is empty".into());
        }
        if ops.len() > MAX_OPS {
            return Err(format!("at most {MAX_OPS} ops per call"));
        }
        let (_lock, mut p) = self.load_locked(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite_mut(&mut p, r)?;
        let f = Self::frame_index(s, frame)?;
        let l = Self::layer_index(s, layer)?;
        let t = Target::new(f, l);
        let mut snapped: BTreeMap<String, String> = BTreeMap::new();
        let mut warnings: Vec<String> = Vec::new();
        for (i, op) in ops.iter().enumerate() {
            apply_op(s, t, op, &mut snapped, &mut warnings)
                .map_err(|err| format!("op {i}: {err} (nothing was saved)"))?;
        }
        let png = preview_png(s, &[f], None)?;
        let mut text = format!(
            "Applied {} op(s) to '{}', frame {f}, layer {l} ('{}').",
            ops.len(),
            Self::asset_name(&p, r),
            Self::sprite(&p, r)?.layer_names()[l],
        );
        if !snapped.is_empty() {
            let list: Vec<String> = snapped.iter().map(|(a, b)| format!("{a}→{b}")).collect();
            text.push_str(&format!(
                " Colors adjusted to the palette or color depth: {}.",
                list.join(", ")
            ));
        }
        for w in &warnings {
            text.push_str(&format!(" Warning: {w}."));
        }
        if matches!(r, AssetRef::Character(id) if p.animations_of(id).next().is_some()) {
            text.push_str(
                " Note: existing animations of this character keep the earlier base pose.",
            );
        }
        self.save(project, &p)?;
        Ok(Reply {
            text,
            image_png: Some(png),
        })
    }

    pub fn view(
        &self,
        project: &str,
        asset: &str,
        frame: Option<usize>,
        layer: Option<&Value>,
    ) -> Res {
        let p = self.load(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite(&p, r)?;
        let frames: Vec<usize> = match frame {
            Some(_) => vec![Self::frame_index(s, frame)?],
            None => (0..s.frames().len()).collect(),
        };
        let layer = match layer {
            None | Some(Value::Null) => None,
            some => Some(Self::layer_index(s, some)?),
        };
        let png = preview_png(s, &frames, layer)?;
        let text = format!(
            "'{}' ({}), {}×{} px, {} bit, layers: {}. Showing {}{} on a gray checkerboard (= transparent).",
            Self::asset_name(&p, r),
            r.kind(),
            s.width(),
            s.height(),
            s.depth.bits(),
            s.layer_names().join(", "),
            if frames.len() == 1 {
                format!("frame {}", frames[0])
            } else {
                format!("all {} frames left to right", frames.len())
            },
            layer.map(|l| format!(", only layer {l}")).unwrap_or_default(),
        );
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
            other => {
                return Err(format!(
                    "unknown action '{other}', use add, remove, rename or set"
                ))
            }
        };
        let layers = s.layer_names();
        self.save(project, &p)?;
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
                if s.frames().len() >= MAX_FRAMES {
                    return Err(format!("at most {MAX_FRAMES} frames"));
                }
                let i = s.add_frame();
                format!("Added empty frame {i} at the end.")
            }
            "duplicate" => {
                if s.frames().len() >= MAX_FRAMES {
                    return Err(format!("at most {MAX_FRAMES} frames"));
                }
                let f = exists(s, need(frame, "frame")?)?;
                let i = s.duplicate_frame(f).map_err(e)?;
                format!("Duplicated frame {f}; the copy is frame {i}, later frames moved up by one.")
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
        self.save(project, &p)?;
        Ok(Reply::text(format!(
            "{msg} The asset now has {n} frame(s)."
        )))
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
        self.save(project, &p)?;
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
        self.save(project, &p)?;
        Ok(Reply::json(json!({
            "palette": pal,
            "note": "New colors were appended; existing colors and pixels are unchanged. The extended palette also applies to all animations of this character.",
        })))
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
    ) -> Res {
        let p = self.load(project)?;
        let r = Self::resolve(&p, asset)?;
        let s = Self::sprite(&p, r)?;
        let scale = scale.unwrap_or(1);
        if !(1..=export::MAX_SCALE).contains(&scale) {
            return Err(format!("scale must be 1 to {}", export::MAX_SCALE));
        }
        let frame = match frame {
            Some(_) => Some(Self::frame_index(s, frame)?),
            None => None,
        };
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
                    "unknown format '{other}', use png, png8, jpg, sheet, gif or webm"
                ))
            }
        };
        if format.is_whole_animation() && frame.is_some() {
            return Err("this format always contains all frames, omit `frame`".into());
        }
        let name = Self::asset_name(&p, r);
        let out = self
            .ws
            .export(project, &name, s, format, frame, scale)
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
        Ok(Reply::json(reply))
    }
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

fn apply_op(
    s: &mut Sprite,
    t: Target,
    op: &DrawOp,
    snapped: &mut BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> Res<()> {
    let mut color =
        |s: &mut Sprite, c: &str| -> Res<Rgba> { Ok(note_snap(s, parse_color(c)?, snapped)) };
    match op {
        DrawOp::Pixels { color: c, points } => {
            let c = color(s, c)?;
            let pts: Vec<(i64, i64)> = points.iter().map(|p| (p[0], p[1])).collect();
            draw::pixels(s, t, &pts, c).map_err(e)
        }
        DrawOp::Line { color: c, from, to } => {
            let c = color(s, c)?;
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
            let c = color(s, c)?;
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
            let c = color(s, c)?;
            check_box(*x, *y, *w, *h)?;
            draw::ellipse(s, t, *x, *y, *w, *h, c, *filled).map_err(e)
        }
        DrawOp::Fill { color: c, x, y } => {
            let c = color(s, c)?;
            draw::flood_fill(s, t, *x, *y, c).map(|_| ()).map_err(e)
        }
        DrawOp::Grid { x, y, rows, legend } => {
            let mut map: HashMap<char, Rgba> = HashMap::new();
            for (k, v) in legend {
                let mut chars = k.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else {
                    return Err(format!("legend key '{k}' must be exactly one character"));
                };
                map.insert(ch, color(s, v)?);
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
                            return Err(format!("row {ry}: character '{ch}' is not in the legend"))
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
            let to = color(s, to)?;
            draw::replace_color(s, t, from, to).map(|_| ()).map_err(e)
        }
        DrawOp::Shift { dx, dy } => {
            let lost = draw::shift(s, t, *dx, *dy).map_err(e)?;
            if lost > 0 {
                warnings.push(format!(
                    "shift by ({dx}, {dy}) pushed {lost} pixel(s) off the canvas, they are lost"
                ));
            }
            Ok(())
        }
        DrawOp::FlipH => draw::flip_horizontal(s, t).map_err(e),
        DrawOp::Clear => {
            s.layer_mut(t.frame, t.layer).map_err(e)?.clear();
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

/// Vorschaubild: die gewählten Frames nebeneinander, ganzzahlig vergrößert,
/// auf grauem Schachbrett, damit Transparenz erkennbar ist.
fn preview_png(s: &Sprite, frames: &[usize], layer: Option<usize>) -> Res<Vec<u8>> {
    let (w, h) = (s.width(), s.height());
    let n = frames.len() as u32;
    // Zwischen den Frames 1 Quell-Pixel Abstand.
    let total_w = w * n + (n - 1);
    let scale = (PREVIEW_TARGET / total_w.max(h)).clamp(1, 32);
    // Sehr breite Streifen umbrechen, statt sie winzig zu zeigen.
    let (cols, rows) = if scale == 1 && n > 1 && total_w > 2 * PREVIEW_TARGET {
        let cols = (2 * PREVIEW_TARGET / (w + 1)).max(1).min(n);
        (cols, n.div_ceil(cols))
    } else {
        (n, 1)
    };
    let (out_w, out_h) = (cols * w + (cols - 1), rows * h + (rows - 1));
    let cell = if w.max(h) >= 16 {
        (w.max(h) / 16).max(2)
    } else {
        1
    };
    let mut img =
        image::RgbaImage::from_pixel(out_w * scale, out_h * scale, image::Rgba([40, 40, 48, 255]));
    for (i, &f) in frames.iter().enumerate() {
        let px: Vec<Rgba> = match layer {
            Some(l) => s.layer(f, l).map_err(e)?.pixels().to_vec(),
            None => s.composite(f).map_err(e)?,
        };
        let (ox, oy) = ((i as u32 % cols) * (w + 1), (i as u32 / cols) * (h + 1));
        for y in 0..h {
            for x in 0..w {
                let light = ((x / cell) + (y / cell)) % 2 == 0;
                let bg = if light {
                    Rgba::rgb(200, 200, 204)
                } else {
                    Rgba::rgb(168, 168, 174)
                };
                let c = px[(y * w + x) as usize].over(bg);
                for sy in 0..scale {
                    for sx in 0..scale {
                        img.put_pixel(
                            (ox + x) * scale + sx,
                            (oy + y) * scale + sy,
                            image::Rgba(c.to_array()),
                        );
                    }
                }
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(e)?;
    Ok(out.into_inner())
}
