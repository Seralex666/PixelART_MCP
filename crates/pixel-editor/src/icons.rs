//! Kleine Pixel-Symbole für Werkzeuge und Schalter, als 16×16-Raster im Programm selbst.
//!
//! `#` ist die Umrissfarbe und folgt dem hellen oder dunklen Erscheinungsbild,
//! die Buchstaben sind feste Farben, `.` ist durchsichtig.

use crate::tools::Tool;
use eframe::egui::{self, Color32};

const SIZE: usize = 16;

/// Ein Symbol: ein Werkzeug oder ein Schalter der Werkzeugleiste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Tool(Tool),
    /// Pinsel vor der Pinselgröße.
    Brush,
    /// Knopf „Kontur…“.
    Outline,
    /// Knopf „Licht…“.
    Light,
    /// Spiegeln links und rechts.
    MirrorX,
    /// Spiegeln oben und unten.
    MirrorY,
}

impl Icon {
    pub fn all() -> Vec<Icon> {
        let mut all: Vec<Icon> = Tool::ALL.iter().map(|&t| Icon::Tool(t)).collect();
        all.extend([
            Icon::Brush,
            Icon::Outline,
            Icon::Light,
            Icon::MirrorX,
            Icon::MirrorY,
        ]);
        all
    }
}

fn rows(icon: Icon) -> Vec<String> {
    match icon {
        Icon::Tool(t) => tool_art(t).iter().map(|r| r.to_string()).collect(),
        Icon::Brush => lines(&[
            "................",
            "...........##...",
            "..........#gg#..",
            ".........#gg#...",
            "........#gg#....",
            ".......#gg#.....",
            "......#ww#......",
            ".....#ww#.......",
            "....#bb#........",
            "...#bbb#........",
            "..#bbb#.........",
            "..#bb#..........",
            "..###...........",
            "................",
            "................",
            "................",
        ]),
        Icon::Light => lines(&[
            "................",
            ".......yy.......",
            "..y....yy....y..",
            "...y........y...",
            "......####......",
            ".....#yyyy#.....",
            "....#yywwyy#....",
            "yy..#ywwwwy#..yy",
            "yy..#ywwwwy#..yy",
            "....#yywwyy#....",
            ".....#yyyy#.....",
            "......####......",
            "...y........y...",
            "..y....yy....y..",
            ".......yy.......",
            "................",
        ]),
        Icon::Outline => lines(&[
            "................",
            "......####......",
            ".....#yyyy#.....",
            "....#yyyyyy#....",
            "....#yyyyyy#....",
            ".....#yyyy#.....",
            "......####......",
            "....########....",
            "...#bbbbbbbb#...",
            "..#bbbbbbbbbb#..",
            "..#bbbbbbbbbb#..",
            "..#bbbbbbbbbb#..",
            "...##########...",
            "................",
            "................",
            "................",
        ]),
        Icon::MirrorX => lines(&MIRROR),
        Icon::MirrorY => {
            // Dasselbe Bild, an der Diagonale gekippt.
            let m: Vec<&[u8]> = MIRROR.iter().map(|r| r.as_bytes()).collect();
            (0..SIZE)
                .map(|y| (0..SIZE).map(|x| m[x][y] as char).collect())
                .collect()
        }
    }
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|r| r.to_string()).collect()
}

const MIRROR: [&str; SIZE] = [
    "................",
    "................",
    ".......##.......",
    ".......##.......",
    ".....#....#.....",
    "....##....##....",
    "...#b#.##.#c#...",
    "..#bb#.##.#cc#..",
    ".#bbb#....#ccc#.",
    "..#bb#....#cc#..",
    "...#b#.##.#c#...",
    "....##.##.##....",
    ".....#....#.....",
    "................",
    ".......##.......",
    "................",
];

fn tool_art(tool: Tool) -> [&'static str; SIZE] {
    match tool {
        Tool::Pencil => [
            "................",
            "............###.",
            "...........#ppp#",
            "..........#ppp#.",
            ".........#ggg#..",
            "........#yyy#...",
            ".......#yyy#....",
            "......#yyy#.....",
            ".....#yyy#......",
            "....#www#.......",
            "...#ww#.........",
            "..###...........",
            "..#.............",
            "................",
            "................",
            "................",
        ],
        Tool::Eraser => [
            "................",
            "................",
            "...........####.",
            "..........#pppp#",
            ".........#pppp#.",
            "........#pppp#..",
            ".......#pppp#...",
            "......#e#pp#....",
            ".....#eee#e#....",
            "....#eeee#......",
            "...#eeee#.......",
            "...#eee#........",
            "....###.........",
            "................",
            "................",
            "................",
        ],
        Tool::Fill => [
            "................",
            "......####......",
            ".....#....#.....",
            ".....#....#.....",
            "...##########...",
            "...#bbbbbbbb#...",
            "...#gggggggg#b..",
            "...#gggggggg#bb.",
            "....#gggggg#.bb.",
            "....#gggggg#.bb.",
            ".....#gggg#...b.",
            ".....######.....",
            "................",
            "................",
            "................",
            "................",
        ],
        Tool::Picker => [
            "................",
            "............##..",
            "...........#rr#.",
            "..........#rrrr#",
            "...........#rr#.",
            "..........###...",
            ".........#c#....",
            "........#c#.....",
            ".......#c#......",
            "......#c#.......",
            ".....#c#........",
            "....#c#.........",
            "...##...........",
            "..b.............",
            "................",
            "................",
        ],
        Tool::Spray => [
            "................",
            ".........r..r...",
            "......##...r..r.",
            ".....#cc#.r..r..",
            ".....####...r...",
            "....#gggg#......",
            "....#gggg#......",
            "....#bbbb#......",
            "....#bbbb#......",
            "....#bbbb#......",
            "....#bbbb#......",
            "....#gggg#......",
            "....#gggg#......",
            "....######......",
            "................",
            "................",
        ],
        Tool::Shade => [
            "................",
            "................",
            "......####......",
            "....##eekk##....",
            "...#eeeekkkk#...",
            "...#eeeekkkk#...",
            "..#eeeeekkkkk#..",
            "..#eeeeekkkkk#..",
            "..#eeeeekkkkk#..",
            "..#eeeeekkkkk#..",
            "...#eeeekkkk#...",
            "...#eeeekkkk#...",
            "....##eekk##....",
            "......####......",
            "................",
            "................",
        ],
        Tool::Shape => [
            "................",
            "................",
            "...###....###...",
            "..#rrr#..#rrr#..",
            ".#rrrrr##rrrrr#.",
            ".#rrrrrrrrrrrr#.",
            ".#rrrrrrrrrrrr#.",
            ".#rrrrrrrrrrrr#.",
            "..#rrrrrrrrrr#..",
            "...#rrrrrrrr#...",
            "....#rrrrrr#....",
            ".....#rrrr#.....",
            "......#rr#......",
            ".......##.......",
            "................",
            "................",
        ],
        Tool::Select => [
            "................",
            ".##..##..##..#..",
            ".#...........#..",
            "................",
            "................",
            ".#...........#..",
            ".#...........#..",
            "................",
            "................",
            ".#...........#..",
            ".#...........#..",
            "................",
            "................",
            ".##..##..##..#..",
            "................",
            "................",
        ],
        Tool::Line => [
            "................",
            "................",
            ".............##.",
            "............###.",
            "...........##...",
            "..........##....",
            ".........##.....",
            "........##......",
            ".......##.......",
            "......##........",
            ".....##.........",
            "....##..........",
            "...##...........",
            ".###............",
            ".##.............",
            "................",
        ],
        Tool::Rect => [
            "................",
            "................",
            "................",
            "..############..",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "..############..",
            "................",
            "................",
            "................",
        ],
        Tool::Ellipse => [
            "................",
            "................",
            "......####......",
            "....##....##....",
            "...#........#...",
            "...#........#...",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "..#..........#..",
            "...#........#...",
            "...#........#...",
            "....##....##....",
            "......####......",
            "................",
            "................",
        ],
        Tool::Move => [
            "................",
            ".......##.......",
            "......####......",
            ".....######.....",
            ".......##.......",
            "...#...##...#...",
            "..##...##...##..",
            ".##############.",
            ".##############.",
            "..##...##...##..",
            "...#...##...#...",
            ".......##.......",
            ".....######.....",
            "......####......",
            ".......##.......",
            "................",
        ],
    }
}

fn color(ch: u8, outline: Color32) -> Color32 {
    match ch {
        b'#' => outline,
        b'p' => Color32::from_rgb(232, 106, 138),
        b'g' => Color32::from_rgb(160, 168, 184),
        b'y' => Color32::from_rgb(242, 193, 78),
        b'w' => Color32::from_rgb(241, 211, 161),
        b'e' => Color32::from_rgb(223, 231, 239),
        b'b' => Color32::from_rgb(59, 142, 232),
        b'r' => Color32::from_rgb(217, 83, 79),
        b'c' => Color32::from_rgb(159, 216, 240),
        b'k' => Color32::from_rgb(58, 62, 82),
        _ => Color32::TRANSPARENT,
    }
}

/// Ein Symbol als Bild.
pub fn image(icon: Icon, outline: Color32) -> egui::ColorImage {
    let rows = rows(icon);
    let pixels = rows
        .iter()
        .flat_map(|row| row.bytes().map(|ch| color(ch, outline)))
        .collect();
    egui::ColorImage::new([SIZE, SIZE], pixels)
}

/// Die Symbole als Texturen, passend zum aktuellen Erscheinungsbild.
#[derive(Default)]
pub struct Icons {
    outline: Option<Color32>,
    textures: Vec<(Icon, egui::TextureHandle)>,
}

impl Icons {
    /// Das Symbol eines Werkzeugs.
    pub fn get(&mut self, ctx: &egui::Context, tool: Tool) -> egui::TextureId {
        self.icon(ctx, Icon::Tool(tool))
    }

    /// Ein Symbol. Wechselt das Erscheinungsbild, entstehen die Texturen neu.
    pub fn icon(&mut self, ctx: &egui::Context, icon: Icon) -> egui::TextureId {
        let outline = ctx.global_style().visuals.strong_text_color();
        if self.outline != Some(outline) {
            self.outline = Some(outline);
            self.textures = Icon::all()
                .into_iter()
                .map(|i| {
                    let tex = ctx.load_texture(
                        format!("icon-{i:?}"),
                        image(i, outline),
                        egui::TextureOptions::NEAREST,
                    );
                    (i, tex)
                })
                .collect();
        }
        self.textures
            .iter()
            .find(|(i, _)| *i == icon)
            .map(|(_, tex)| tex.id())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_complete_and_in_known_colors() {
        for tool in Icon::all() {
            let rows = rows(tool);
            assert_eq!(rows.len(), SIZE, "{tool:?}");
            assert!(rows.iter().all(|r| r.len() == SIZE), "{tool:?}");
            assert!(
                rows.iter()
                    .flat_map(|r| r.bytes())
                    .all(|ch| ch == b'.' || color(ch, Color32::WHITE) != Color32::TRANSPARENT),
                "{tool:?} uses an unknown color"
            );
            let img = image(tool, Color32::WHITE);
            assert!(img.pixels.contains(&Color32::WHITE), "{tool:?}");
        }
    }
}
