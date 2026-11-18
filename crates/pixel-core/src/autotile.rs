//! Kacheln, die sich selbst zusammensetzen (Autotile).
//!
//! Gezeichnet werden nur wenige Kacheln in einer Vorlage, 2 Kacheln breit und
//! 3 hoch (wie bei RPG Maker): oben links eine freie Vorschau, oben rechts die
//! Innenecken, darunter ein Block aus 2 × 2 Kacheln mit Außenecken, Rändern und
//! Mitte. Jede fertige Kachel setzt sich aus vier Vierteln dieser Vorlage
//! zusammen, je nachdem, welche Nachbarn dasselbe Gelände haben. So entstehen
//! alle 47 Kacheln für „Ecken und Seiten“ (Blob) oder die 16 für „nur Seiten“.
//!
//! Dazu kommen der Export für Godot 4 (TileSet mit Terrain) und Tiled (Wang-Set),
//! auch mit animierten Kacheln.

use crate::lang::tr;
use crate::{ColorDepth, Error, Palette, Result, Rgba, Sprite};
use serde::{Deserialize, Serialize};

/// Nachbar oben, oben rechts, rechts … als Bits einer Maske.
pub const N: u8 = 1;
pub const NE: u8 = 2;
pub const E: u8 = 4;
pub const SE: u8 = 8;
pub const S: u8 = 16;
pub const SW: u8 = 32;
pub const W: u8 = 64;
pub const NW: u8 = 128;
const SIDES: u8 = N | E | S | W;
const CORNERS: u8 = NE | SE | SW | NW;

/// Kleinste und größte Kachel.
pub const MIN_TILE: u32 = 4;
pub const MAX_TILE: u32 = 256;

fn fail<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::Effect(m.into()))
}

/// Welcher Satz entsteht.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TileSetKind {
    /// 47 Kacheln, Ecken und Seiten (Godot „Match Corners and Sides“, Tiled „mixed“).
    #[default]
    Blob47,
    /// 16 Kacheln, nur Seiten (Godot „Match Sides“, Tiled „edge“); keine Innenecken.
    Sides16,
}

impl TileSetKind {
    pub const ALL: [TileSetKind; 2] = [Self::Blob47, Self::Sides16];

    pub fn id(self) -> &'static str {
        match self {
            Self::Blob47 => "blob47",
            Self::Sides16 => "sides16",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Blob47 => tr(
                "47 Kacheln: Ecken und Seiten",
                "47 tiles: corners and sides",
            ),
            Self::Sides16 => tr("16 Kacheln: nur Seiten", "16 tiles: sides only"),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace(['-', ' ', '_'], "")
            .as_str()
        {
            "blob47" | "blob" | "47" | "cornersandsides" => Some(Self::Blob47),
            "sides16" | "sides" | "16" => Some(Self::Sides16),
            _ => None,
        }
    }

    pub fn columns(self) -> u32 {
        match self {
            Self::Blob47 => 8,
            Self::Sides16 => 4,
        }
    }

    /// Die Masken der Kacheln in der Reihenfolge des Satzes.
    pub fn masks(self) -> Vec<u8> {
        let mut v: Vec<u8> = match self {
            Self::Blob47 => (0..=255u8).map(normalize).collect(),
            Self::Sides16 => (0..16u8)
                .map(|b| {
                    let sides = [N, E, S, W]
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| b & (1 << i) != 0)
                        .fold(0, |m, (_, &s)| m | s);
                    normalize(sides | CORNERS)
                })
                .collect(),
        };
        v.sort_by_key(|&m| (std::cmp::Reverse(m.count_ones()), m));
        v.dedup();
        v
    }

    /// Die Kachel für Nachbarn `n` (alle 8 Bits, so wie sie sind).
    fn wanted(self, n: u8) -> u8 {
        match self {
            Self::Blob47 => normalize(n),
            Self::Sides16 => normalize((n & SIDES) | CORNERS),
        }
    }
}

/// Eine Ecke zählt nur, wenn beide Seiten daneben dazugehören.
pub fn normalize(m: u8) -> u8 {
    let mut out = m & SIDES;
    for (corner, a, b) in [(NE, N, E), (SE, S, E), (SW, S, W), (NW, N, W)] {
        if m & corner != 0 && m & a != 0 && m & b != 0 {
            out |= corner;
        }
    }
    out
}

/// Ein fertiger Satz: welche Kachel wo liegt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileSet {
    pub kind: TileSetKind,
    /// Kantenlänge einer Kachel.
    pub tile: u32,
    pub columns: u32,
    /// Maske jeder Kachel, von links oben zeilenweise.
    pub masks: Vec<u8>,
    /// Aus welcher Vorlage der Satz entstand (Name des Bildes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl TileSet {
    pub fn rows(&self) -> u32 {
        (self.masks.len() as u32).div_ceil(self.columns)
    }

    /// Welche Kachel zu den Nachbarn `n` passt.
    pub fn index_for(&self, n: u8) -> Option<usize> {
        let want = self.kind.wanted(n);
        self.masks.iter().position(|&m| m == want)
    }
}

/// Prüft die Größe einer Vorlage und gibt die Kantenlänge einer Kachel zurück.
pub fn tile_of_template(w: u32, h: u32) -> Result<u32> {
    let t = w / 2;
    if !w.is_multiple_of(2)
        || h != t * 3
        || !t.is_multiple_of(2)
        || !(MIN_TILE..=MAX_TILE).contains(&t)
    {
        return fail(crate::trf!(
            "Eine Autotile-Vorlage ist 2 Kacheln breit und 3 hoch, mit gerader Kachelgröße von {MIN_TILE} bis {MAX_TILE} (etwa 32×48 für 16er-Kacheln); dieses Bild ist {w}×{h}",
            "an autotile template is 2 tiles wide and 3 tall with an even tile size of {MIN_TILE} to {MAX_TILE} (e.g. 32×48 for 16 px tiles); this image is {w}×{h}"
        ));
    }
    Ok(t)
}

/// Eine leere Vorlage für Kacheln der Größe `tile`, mit einer Vorlage-Ebene, die
/// zeigt, was wohin gehört.
pub fn template(tile: u32, depth: ColorDepth, palette: Palette) -> Result<Sprite> {
    tile_of_template(tile * 2, tile * 3)?;
    let (w, h) = (tile * 2, tile * 3);
    let mut s = Sprite::new(w, h, depth, palette)?;
    let l = s.layer_index(&s.layer_names()[0]).unwrap_or(0);
    s.rename_layer(l, tr("Gelände", "Terrain"))?;
    let (t, q) = (tile as usize, tile as usize / 2);
    let mut guide = vec![Rgba::TRANSPARENT; (w * h) as usize];
    let put = |g: &mut Vec<Rgba>, x: usize, y: usize, c: Rgba| g[y * w as usize + x] = c;
    let fill = Rgba::new(120, 200, 120, 255);
    let cut = Rgba::new(230, 230, 230, 255);
    let line = Rgba::new(60, 60, 60, 255);
    let soft = Rgba::new(150, 150, 150, 255);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let (tx, ty) = (x / t, y / t);
            // Block unten und Innenecken oben rechts: Gelände.
            let terrain = ty >= 1 || tx == 1;
            if terrain {
                put(&mut guide, x, y, fill);
            }
            // Innenecken: in jeder Ecke ein Stück ohne Gelände.
            if tx == 1 && ty == 0 {
                let (lx, ly) = (x - t, y);
                let edge = q / 2;
                let near_x = lx < edge || lx >= t - edge;
                let near_y = ly < edge || ly >= t - edge;
                if near_x && near_y {
                    put(&mut guide, x, y, cut);
                }
            }
            if x % q == 0 || y % q == 0 {
                put(&mut guide, x, y, soft);
            }
            if x % t == 0 || y % t == 0 || x == w as usize - 1 || y == h as usize - 1 {
                put(&mut guide, x, y, line);
            }
        }
    }
    s.add_reference_layer(tr("Vorlage Autotile", "Autotile guide"), &[guide], 90)?;
    Ok(s)
}

/// Baut aus einer Vorlage den ganzen Satz, in allen Frames der Vorlage.
pub fn build(template: &Sprite, kind: TileSetKind) -> Result<(Sprite, TileSet)> {
    let t = tile_of_template(template.width(), template.height())?;
    let masks = kind.masks();
    let columns = kind.columns();
    let rows = (masks.len() as u32).div_ceil(columns);
    let (ow, oh) = (columns * t, rows * t);
    let mut out = Sprite::new(ow, oh, template.depth, template.palette.clone())?;
    let name = tr("Kacheln", "Tiles");
    let l0 = out.layer_names()[0].clone();
    if let Some(l) = out.layer_index(&l0) {
        out.rename_layer(l, name)?;
    }
    out.ensure_frames(template.frames().len())?;
    let tw = template.width() as usize;
    let (t, q) = (t as usize, t as usize / 2);
    for f in 0..template.frames().len() {
        let src = template.composite(f)?;
        let mut px = vec![Rgba::TRANSPARENT; (ow * oh) as usize];
        for (i, &m) in masks.iter().enumerate() {
            let (cx, cy) = (
                (i as u32 % columns) as usize * t,
                (i as u32 / columns) as usize * t,
            );
            for (corner, (ox, oy)) in [(0, (0, 0)), (1, (q, 0)), (2, (0, q)), (3, (q, q))] {
                let (sx, sy) = quarter_source(m, corner, t, q);
                for y in 0..q {
                    for x in 0..q {
                        px[(cy + oy + y) * ow as usize + cx + ox + x] = src[(sy + y) * tw + sx + x];
                    }
                }
            }
        }
        out.layer_mut(f, 0)?.replace_pixels(px)?;
        out.frame_mut(f)?.duration_ms = template.frame(f)?.duration_ms;
    }
    Ok((
        out,
        TileSet {
            kind,
            tile: t as u32,
            columns,
            masks,
            source: None,
        },
    ))
}

/// Wo in der Vorlage das Viertel `corner` (0 oben links, 1 oben rechts, 2 unten
/// links, 3 unten rechts) einer Kachel mit der Maske `m` liegt.
fn quarter_source(m: u8, corner: usize, t: usize, q: usize) -> (usize, usize) {
    let (v, h, d) = match corner {
        0 => (N, W, NW),
        1 => (N, E, NE),
        2 => (S, W, SW),
        _ => (S, E, SE),
    };
    let (has_v, has_h, has_d) = (m & v != 0, m & h != 0, m & d != 0);
    // Viertel im Block (4 × 4 Viertel ab der zweiten Kachelzeile).
    let block = |qx: usize, qy: usize| (qx * q, t + qy * q);
    let (right, bottom) = (corner % 2 == 1, corner >= 2);
    match (has_v, has_h) {
        (true, true) if has_d => block(if right { 1 } else { 2 }, if bottom { 1 } else { 2 }),
        (true, true) => (t + if right { q } else { 0 }, if bottom { q } else { 0 }),
        (false, false) => block(if right { 3 } else { 0 }, if bottom { 3 } else { 0 }),
        // Nur oben oder unten Gelände: Rand links oder rechts.
        (true, false) => block(if right { 3 } else { 0 }, if bottom { 1 } else { 2 }),
        // Nur seitlich Gelände: Rand oben oder unten.
        (false, true) => block(if right { 1 } else { 2 }, if bottom { 3 } else { 0 }),
    }
}

/// Eine kleine Beispielkarte mit dem Satz: eine Insel mit Buchten und Löchern,
/// `cols × rows` Kacheln. Gibt die Pixel und die Größe zurück.
pub fn demo_map(
    set: &Sprite,
    info: &TileSet,
    frame: usize,
    cols: u32,
    rows: u32,
    seed: u64,
) -> Result<(Vec<Rgba>, u32, u32)> {
    let cells = demo_cells(cols, rows, seed);
    let px = set.composite(frame)?;
    let t = info.tile as usize;
    let (w, h) = (cols * info.tile, rows * info.tile);
    let mut out = vec![Rgba::TRANSPARENT; (w * h) as usize];
    let sw = set.width() as usize;
    let at = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < i64::from(cols)
            && y < i64::from(rows)
            && cells[(y as u32 * cols + x as u32) as usize]
    };
    for cy in 0..rows as i64 {
        for cx in 0..cols as i64 {
            if !at(cx, cy) {
                continue;
            }
            let mut n = 0u8;
            for (bit, dx, dy) in [
                (N, 0, -1),
                (NE, 1, -1),
                (E, 1, 0),
                (SE, 1, 1),
                (S, 0, 1),
                (SW, -1, 1),
                (W, -1, 0),
                (NW, -1, -1),
            ] {
                if at(cx + dx, cy + dy) {
                    n |= bit;
                }
            }
            let Some(i) = info.index_for(n) else { continue };
            let (tx, ty) = (
                (i as u32 % info.columns) as usize * t,
                (i as u32 / info.columns) as usize * t,
            );
            for y in 0..t {
                for x in 0..t {
                    let (sx, sy) = (tx + x, ty + y);
                    if sx < sw && sy < set.height() as usize {
                        out[(cy as usize * t + y) * w as usize + cx as usize * t + x] =
                            px[sy * sw + sx];
                    }
                }
            }
        }
    }
    Ok((out, w, h))
}

/// Welche Felder der Beispielkarte Gelände sind: eine Insel aus Rauschen.
fn demo_cells(cols: u32, rows: u32, seed: u64) -> Vec<bool> {
    let (cx, cy) = (f64::from(cols) / 2.0 - 0.5, f64::from(rows) / 2.0 - 0.5);
    let mut v = Vec::with_capacity((cols * rows) as usize);
    for y in 0..rows {
        for x in 0..cols {
            let dx = (f64::from(x) - cx) / (f64::from(cols) / 2.0);
            let dy = (f64::from(y) - cy) / (f64::from(rows) / 2.0);
            let fall = 1.0 - (dx * dx + dy * dy).sqrt();
            let n = crate::expr::value_noise(seed, f64::from(x) * 0.6, f64::from(y) * 0.6, 3);
            v.push(fall + (n - 0.5) * 0.9 > 0.25);
        }
    }
    v
}

/// Wie ein Satz für den Export im Bild liegt: bei mehreren Frames stehen die
/// Frames jeder Kachel nebeneinander, so wie Godot Kachel-Animationen erwartet.
pub struct Atlas {
    pub pixels: Vec<Rgba>,
    pub width: u32,
    pub height: u32,
    /// Frames je Kachel (1 ohne Animation).
    pub frames: u32,
    pub durations: Vec<u32>,
}

/// Legt den Satz für den Export aus. Mit `frame` nur dieser Frame.
pub fn atlas(set: &Sprite, info: &TileSet, frame: Option<usize>) -> Result<Atlas> {
    let frames: Vec<usize> = match frame {
        Some(f) => {
            set.frame(f)?;
            vec![f]
        }
        None => (0..set.frames().len()).collect(),
    };
    let n = frames.len() as u32;
    let t = info.tile;
    let (cols, rows) = (info.columns, info.rows());
    let (w, h) = (cols * n * t, rows * t);
    let mut px = vec![Rgba::TRANSPARENT; (w * h) as usize];
    let sw = set.width() as usize;
    for (k, &f) in frames.iter().enumerate() {
        let src = set.composite(f)?;
        for i in 0..info.masks.len() as u32 {
            let (c, r) = (i % cols, i / cols);
            let (sx0, sy0) = ((c * t) as usize, (r * t) as usize);
            let (dx0, dy0) = (((c * n + k as u32) * t) as usize, (r * t) as usize);
            for y in 0..t as usize {
                for x in 0..t as usize {
                    if sx0 + x < sw && sy0 + y < set.height() as usize {
                        px[(dy0 + y) * w as usize + dx0 + x] = src[(sy0 + y) * sw + sx0 + x];
                    }
                }
            }
        }
    }
    Ok(Atlas {
        pixels: px,
        width: w,
        height: h,
        frames: n,
        durations: frames
            .iter()
            .map(|&f| set.frame(f).map_or(100, |fr| fr.duration_ms))
            .collect(),
    })
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_godot(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Godot-4-TileSet (.tres) zum Bild `image` (Dateiname neben der .tres-Datei).
/// `scale` vergrößert die Kacheln wie das Bild.
pub fn godot_tres(info: &TileSet, atlas: &Atlas, image: &str, name: &str, scale: u32) -> String {
    let t = info.tile * scale;
    let mut s = String::new();
    s.push_str("[gd_resource type=\"TileSet\" load_steps=3 format=3]\n\n");
    s.push_str(&format!(
        "[ext_resource type=\"Texture2D\" path=\"{}\" id=\"1_tiles\"]\n\n",
        escape_godot(image)
    ));
    s.push_str("[sub_resource type=\"TileSetAtlasSource\" id=\"TileSetAtlasSource_tiles\"]\n");
    s.push_str("texture = ExtResource(\"1_tiles\")\n");
    s.push_str(&format!("texture_region_size = Vector2i({t}, {t})\n"));
    let bits: [(u8, &str); 8] = [
        (E, "right_side"),
        (SE, "bottom_right_corner"),
        (S, "bottom_side"),
        (SW, "bottom_left_corner"),
        (W, "left_side"),
        (NW, "top_left_corner"),
        (N, "top_side"),
        (NE, "top_right_corner"),
    ];
    for (i, &m) in info.masks.iter().enumerate() {
        let (c, r) = (i as u32 % info.columns, i as u32 / info.columns);
        let key = format!("{}:{r}", c * atlas.frames);
        if atlas.frames > 1 {
            s.push_str(&format!(
                "{key}/animation_frames_count = {}\n",
                atlas.frames
            ));
            for (k, d) in atlas.durations.iter().enumerate() {
                s.push_str(&format!(
                    "{key}/animation_frame_{k}/duration = {}\n",
                    f64::from(*d) / 1000.0
                ));
            }
        }
        s.push_str(&format!("{key}/0 = 0\n"));
        s.push_str(&format!("{key}/0/terrain_set = 0\n"));
        s.push_str(&format!("{key}/0/terrain = 0\n"));
        for (bit, label) in bits {
            let side = bit & SIDES != 0;
            if m & bit != 0 && (side || info.kind == TileSetKind::Blob47) {
                s.push_str(&format!("{key}/0/terrains_peering_bit/{label} = 0\n"));
            }
        }
    }
    s.push_str("\n[resource]\n");
    s.push_str(&format!("tile_size = Vector2i({t}, {t})\n"));
    let mode = match info.kind {
        TileSetKind::Blob47 => 0,
        TileSetKind::Sides16 => 2,
    };
    s.push_str(&format!("terrain_set_0/mode = {mode}\n"));
    s.push_str(&format!(
        "terrain_set_0/terrain_0/name = \"{}\"\n",
        escape_godot(name)
    ));
    s.push_str("terrain_set_0/terrain_0/color = Color(0.3, 0.7, 0.4, 1)\n");
    s.push_str("sources/0 = SubResource(\"TileSetAtlasSource_tiles\")\n");
    s
}

/// Tiled-Tileset (.tsx) mit Wang-Set zum Bild `image`.
pub fn tiled_tsx(info: &TileSet, atlas: &Atlas, image: &str, name: &str, scale: u32) -> String {
    let t = info.tile * scale;
    let columns = info.columns * atlas.frames;
    let count = columns * info.rows();
    let name = escape_xml(name);
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(&format!(
        "<tileset version=\"1.10\" tiledversion=\"1.10.2\" name=\"{name}\" tilewidth=\"{t}\" tileheight=\"{t}\" tilecount=\"{count}\" columns=\"{columns}\">\n"
    ));
    s.push_str(&format!(
        " <image source=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        escape_xml(image),
        atlas.width * scale,
        atlas.height * scale
    ));
    let id = |i: usize| {
        let (c, r) = (i as u32 % info.columns, i as u32 / info.columns);
        r * columns + c * atlas.frames
    };
    if atlas.frames > 1 {
        for i in 0..info.masks.len() {
            let base = id(i);
            s.push_str(&format!(" <tile id=\"{base}\">\n  <animation>\n"));
            for (k, d) in atlas.durations.iter().enumerate() {
                s.push_str(&format!(
                    "   <frame tileid=\"{}\" duration=\"{d}\"/>\n",
                    base + k as u32
                ));
            }
            s.push_str("  </animation>\n </tile>\n");
        }
    }
    let kind = match info.kind {
        TileSetKind::Blob47 => "mixed",
        TileSetKind::Sides16 => "edge",
    };
    s.push_str(" <wangsets>\n");
    s.push_str(&format!(
        "  <wangset name=\"{name}\" type=\"{kind}\" tile=\"-1\">\n"
    ));
    s.push_str(&format!(
        "   <wangcolor name=\"{name}\" color=\"#4cb364\" tile=\"-1\" probability=\"1\"/>\n"
    ));
    for (i, &m) in info.masks.iter().enumerate() {
        // Reihenfolge bei Tiled: oben, oben rechts, rechts, unten rechts, unten,
        // unten links, links, oben links.
        let ids: Vec<&str> = [N, NE, E, SE, S, SW, W, NW]
            .iter()
            .map(|&b| {
                let corner = b & CORNERS != 0;
                if m & b != 0 && !(corner && info.kind == TileSetKind::Sides16) {
                    "1"
                } else {
                    "0"
                }
            })
            .collect();
        s.push_str(&format!(
            "   <wangtile tileid=\"{}\" wangid=\"{}\"/>\n",
            id(i),
            ids.join(",")
        ));
    }
    s.push_str("  </wangset>\n </wangsets>\n</tileset>\n");
    s
}

impl Sprite {
    /// Der Autotile-Satz, falls dieses Bild einer ist.
    pub fn tileset(&self) -> Option<&TileSet> {
        self.tileset.as_deref()
    }

    pub fn set_tileset(&mut self, info: Option<TileSet>) {
        self.tileset = info.map(Box::new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_47_blob_tiles_and_16_side_tiles() {
        assert_eq!(TileSetKind::Blob47.masks().len(), 47);
        assert_eq!(TileSetKind::Sides16.masks().len(), 16);
        assert_eq!(normalize(NE), 0, "a lone corner does not count");
        assert_eq!(normalize(N | E | NE), N | E | NE);
    }

    #[test]
    fn every_neighbourhood_finds_a_tile() {
        for kind in TileSetKind::ALL {
            let info = TileSet {
                kind,
                tile: 8,
                columns: kind.columns(),
                masks: kind.masks(),
                source: None,
            };
            for n in 0..=255u8 {
                assert!(info.index_for(n).is_some(), "{kind:?} {n:08b}");
            }
        }
    }

    #[test]
    fn quarters_come_from_the_right_place() {
        // Jedes Viertel der Vorlage bekommt eine eigene Farbe.
        let mut s = Sprite::new(8, 12, ColorDepth::Rgba32, Palette::default()).unwrap();
        for y in 0..12 {
            for x in 0..8 {
                let c = Rgba::rgb((x / 2 * 40) as u8, (y / 2 * 40) as u8, 7);
                s.set_pixel(0, 0, x, y, c).unwrap();
            }
        }
        let (set, info) = build(&s, TileSetKind::Blob47).unwrap();
        let tile = |m: u8| info.index_for(m).unwrap();
        let px = set.composite(0).unwrap();
        let at = |i: usize, x: usize, y: usize| {
            let (c, r) = (i % 8, i / 8);
            px[(r * 4 + y) * 32 + c * 4 + x]
        };
        let quarter = |qx: i64, qy: i64| Rgba::rgb((qx * 40) as u8, (qy * 40) as u8, 7);
        // Ganz allein: die vier Außenecken des Blocks.
        let alone = tile(0);
        assert_eq!(at(alone, 0, 0), quarter(0, 2));
        assert_eq!(at(alone, 3, 3), quarter(3, 5));
        // Rundum Gelände: die Mitte des Blocks.
        let full = tile(255);
        assert_eq!(at(full, 0, 0), quarter(2, 4));
        assert_eq!(at(full, 2, 2), quarter(1, 3));
        // Alle Seiten, keine Ecken: die Innenecken oben rechts.
        let inner = tile(SIDES);
        assert_eq!(at(inner, 0, 0), quarter(2, 0));
        assert_eq!(at(inner, 3, 3), quarter(3, 1));
    }

    #[test]
    fn exports_name_every_tile() {
        let s = template(8, ColorDepth::Rgba32, Palette::default()).unwrap();
        let (mut set, info) = build(&s, TileSetKind::Blob47).unwrap();
        set.ensure_frames(2).unwrap();
        let a = atlas(&set, &info, None).unwrap();
        assert_eq!((a.width, a.height, a.frames), (8 * 2 * 8, 6 * 8, 2));
        let tres = godot_tres(&info, &a, "Gras.png", "Gras", 1);
        assert_eq!(tres.matches("/0/terrain = 0").count(), 47);
        assert!(tres.contains("2:0/animation_frames_count = 2"));
        let tsx = tiled_tsx(&info, &a, "Gras.png", "Gras", 2);
        assert_eq!(tsx.matches("<wangtile").count(), 47);
        assert!(tsx.contains("tilewidth=\"16\""));
        assert!(tsx.contains("wangid=\"1,1,1,1,1,1,1,1\""));
    }
}
