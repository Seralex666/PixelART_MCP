//! Kleine Pixel-Symbole für die Werkzeuge, als 16×16-Raster im Programm selbst.
//!
//! `#` ist die Umrissfarbe und folgt dem hellen oder dunklen Erscheinungsbild,
//! die Buchstaben sind feste Farben, `.` ist durchsichtig.

use crate::tools::Tool;
use eframe::egui::{self, Color32};

const SIZE: usize = 16;

fn art(tool: Tool) -> [&'static str; SIZE] {
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
        _ => Color32::TRANSPARENT,
    }
}

/// Das Symbol eines Werkzeugs als Bild.
pub fn image(tool: Tool, outline: Color32) -> egui::ColorImage {
    let rows = art(tool);
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
    textures: Vec<(Tool, egui::TextureHandle)>,
}

impl Icons {
    /// Das Symbol für `tool`. Wechselt das Erscheinungsbild, entstehen die Texturen neu.
    pub fn get(&mut self, ctx: &egui::Context, tool: Tool) -> egui::TextureId {
        let outline = ctx.global_style().visuals.strong_text_color();
        if self.outline != Some(outline) {
            self.outline = Some(outline);
            self.textures = Tool::ALL
                .iter()
                .map(|&t| {
                    let tex = ctx.load_texture(
                        format!("tool-{t:?}"),
                        image(t, outline),
                        egui::TextureOptions::NEAREST,
                    );
                    (t, tex)
                })
                .collect();
        }
        self.textures
            .iter()
            .find(|(t, _)| *t == tool)
            .map(|(_, tex)| tex.id())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_complete_icon_in_known_colors() {
        for tool in Tool::ALL {
            let rows = art(tool);
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
