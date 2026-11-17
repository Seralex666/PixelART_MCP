//! Zeichenwerkzeuge: was ein Mausstrich auf der Leinwand bewirkt.
//! Ohne Oberfläche, damit es sich testen lässt.

use crate::doc::Res;
use pixel_core::draw::{self, Target};
use pixel_core::lang::tr;
use pixel_core::{Rgba, Sprite};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pencil,
    Eraser,
    Fill,
    Picker,
    Line,
    Rect,
    Ellipse,
    Move,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Pencil,
        Tool::Eraser,
        Tool::Fill,
        Tool::Picker,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Move,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Pencil => tr("Stift", "Pencil"),
            Tool::Eraser => tr("Radierer", "Eraser"),
            Tool::Fill => tr("Füllen", "Fill"),
            Tool::Picker => tr("Pipette", "Picker"),
            Tool::Line => tr("Linie", "Line"),
            Tool::Rect => tr("Rechteck", "Rectangle"),
            Tool::Ellipse => "Ellipse",
            Tool::Move => tr("Verschieben", "Move"),
        }
    }

    /// Tastenkürzel.
    pub fn key(self) -> char {
        match self {
            Tool::Pencil => 'B',
            Tool::Eraser => 'E',
            Tool::Fill => 'G',
            Tool::Picker => 'I',
            Tool::Line => 'L',
            Tool::Rect => 'R',
            Tool::Ellipse => 'O',
            Tool::Move => 'M',
        }
    }

    /// Ob die Pinselgröße bei diesem Werkzeug etwas bewirkt.
    pub fn uses_size(self) -> bool {
        matches!(self, Tool::Pencil | Tool::Eraser | Tool::Line)
    }

    /// Ob „gefüllt“ bei diesem Werkzeug etwas bewirkt.
    pub fn uses_fill(self) -> bool {
        matches!(self, Tool::Rect | Tool::Ellipse)
    }
}

pub const MAX_BRUSH: u32 = 16;

/// Einstellungen eines Strichs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub tool: Tool,
    pub color: Rgba,
    /// Kantenlänge des Pinsels in Pixeln (Stift, Radierer, Linie).
    pub size: u32,
    /// Rechteck und Ellipse gefüllt statt nur als Umriss.
    pub filled: bool,
}

type Cell = (i64, i64);

/// Ein laufender Mausstrich: gedrückt, gezogen, losgelassen.
pub struct Stroke {
    brush: Brush,
    target: Target,
    start: Cell,
    last: Cell,
    /// Stand der Ebene beim Beginn, für Werkzeuge mit Vorschau (Linie, Formen, Verschieben).
    base: Option<Vec<Rgba>>,
}

impl Stroke {
    /// Beginnt einen Strich an der Zelle `at` und setzt den ersten Punkt.
    pub fn begin(s: &mut Sprite, target: Target, brush: Brush, at: Cell) -> Res<Self> {
        let layer = s
            .layer(target.frame, target.layer)
            .map_err(|e| e.to_string())?;
        let base = matches!(
            brush.tool,
            Tool::Line | Tool::Rect | Tool::Ellipse | Tool::Move
        )
        .then(|| layer.pixels().to_vec());
        let mut stroke = Self {
            brush,
            target,
            start: at,
            last: at,
            base,
        };
        match brush.tool {
            Tool::Fill => {
                // Außerhalb des Bildes gibt es nichts zu füllen.
                if at.0 >= 0 && at.1 >= 0 && at.0 < s.width() as i64 && at.1 < s.height() as i64 {
                    draw::flood_fill(s, target, at.0, at.1, brush.color)
                        .map_err(|e| e.to_string())?;
                }
            }
            Tool::Picker => {}
            _ => stroke.drag(s, at)?,
        }
        Ok(stroke)
    }

    /// Die Zelle, bis zu der der Strich schon geführt ist.
    pub fn last(&self) -> Cell {
        self.last
    }

    /// Führt den Strich bis zur Zelle `at` weiter.
    pub fn drag(&mut self, s: &mut Sprite, at: Cell) -> Res {
        let t = self.target;
        let b = self.brush;
        let err = |e: pixel_core::Error| e.to_string();
        match b.tool {
            Tool::Pencil => dabs(s, t, self.last, at, b.size, b.color)?,
            Tool::Eraser => dabs(s, t, self.last, at, b.size, Rgba::TRANSPARENT)?,
            Tool::Line => {
                self.restore(s)?;
                dabs(s, t, self.start, at, b.size, b.color)?;
            }
            Tool::Rect | Tool::Ellipse => {
                self.restore(s)?;
                let x = self.start.0.min(at.0);
                let y = self.start.1.min(at.1);
                let w = (self.start.0 - at.0).unsigned_abs() as u32 + 1;
                let h = (self.start.1 - at.1).unsigned_abs() as u32 + 1;
                if b.tool == Tool::Rect {
                    draw::rect(s, t, x, y, w, h, b.color, b.filled).map_err(err)?;
                } else {
                    draw::ellipse(s, t, x, y, w, h, b.color, b.filled).map_err(err)?;
                }
            }
            Tool::Move => {
                self.restore(s)?;
                draw::shift(s, t, at.0 - self.start.0, at.1 - self.start.1).map_err(err)?;
            }
            Tool::Fill | Tool::Picker => {}
        }
        self.last = at;
        Ok(())
    }

    fn restore(&self, s: &mut Sprite) -> Res {
        if let Some(base) = &self.base {
            s.layer_mut(self.target.frame, self.target.layer)
                .and_then(|l| l.replace_pixels(base.clone()))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Zeichnet eine Linie aus quadratischen Pinselabdrücken.
fn dabs(s: &mut Sprite, t: Target, from: Cell, to: Cell, size: u32, c: Rgba) -> Res {
    let err = |e: pixel_core::Error| e.to_string();
    let size = size.clamp(1, MAX_BRUSH);
    if size == 1 {
        return draw::line(s, t, from, to, c).map_err(err);
    }
    let offset = (size as i64 - 1) / 2;
    for (x, y) in line_cells(from, to) {
        draw::rect(s, t, x - offset, y - offset, size, size, c, true).map_err(err)?;
    }
    Ok(())
}

/// Alle Zellen auf der Strecke von `from` nach `to` (Bresenham).
fn line_cells(from: Cell, to: Cell) -> Vec<Cell> {
    let (mut x, mut y) = from;
    let dx = (to.0 - x).abs();
    let dy = -(to.1 - y).abs();
    let sx = if x < to.0 { 1 } else { -1 };
    let sy = if y < to.1 { 1 } else { -1 };
    let mut error = dx + dy;
    let mut cells = Vec::with_capacity((dx.max(-dy) + 1) as usize);
    loop {
        cells.push((x, y));
        if (x, y) == to {
            break;
        }
        let e2 = 2 * error;
        if e2 >= dy {
            error += dy;
            x += sx;
        }
        if e2 <= dx {
            error += dx;
            y += sy;
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixel_core::{ColorDepth, Palette};

    const RED: Rgba = Rgba::rgb(255, 0, 0);

    fn sprite() -> Sprite {
        Sprite::new(8, 8, ColorDepth::Rgba32, Palette::default_16()).unwrap()
    }

    fn brush(tool: Tool) -> Brush {
        Brush {
            tool,
            color: RED,
            size: 1,
            filled: false,
        }
    }

    fn count(s: &Sprite, c: Rgba) -> usize {
        s.layer(0, 0)
            .unwrap()
            .pixels()
            .iter()
            .filter(|&&p| p == c)
            .count()
    }

    const T: Target = Target::new(0, 0);

    #[test]
    fn pencil_connects_fast_mouse_moves() {
        let mut s = sprite();
        let mut st = Stroke::begin(&mut s, T, brush(Tool::Pencil), (0, 0)).unwrap();
        st.drag(&mut s, (7, 0)).unwrap();
        assert_eq!(count(&s, RED), 8);
    }

    #[test]
    fn big_brush_and_eraser() {
        let mut s = sprite();
        let mut b = brush(Tool::Pencil);
        b.size = 3;
        Stroke::begin(&mut s, T, b, (4, 4)).unwrap();
        assert_eq!(count(&s, RED), 9);
        b.tool = Tool::Eraser;
        b.size = 1;
        Stroke::begin(&mut s, T, b, (4, 4)).unwrap();
        assert_eq!(count(&s, RED), 8);
    }

    #[test]
    fn shape_preview_does_not_leave_traces() {
        let mut s = sprite();
        let mut st = Stroke::begin(&mut s, T, brush(Tool::Rect), (1, 1)).unwrap();
        st.drag(&mut s, (6, 6)).unwrap();
        st.drag(&mut s, (3, 3)).unwrap();
        // Nur der Umriss des letzten Rechtecks 3×3 bleibt: 8 Pixel.
        assert_eq!(count(&s, RED), 8);
        // Rückwärts aufgezogen ergibt dasselbe.
        let mut s2 = sprite();
        let mut st = Stroke::begin(&mut s2, T, brush(Tool::Rect), (3, 3)).unwrap();
        st.drag(&mut s2, (1, 1)).unwrap();
        assert_eq!(s, s2);
    }

    #[test]
    fn move_shifts_from_the_original() {
        let mut s = sprite();
        Stroke::begin(&mut s, T, brush(Tool::Pencil), (2, 2)).unwrap();
        let mut st = Stroke::begin(&mut s, T, brush(Tool::Move), (0, 0)).unwrap();
        st.drag(&mut s, (1, 0)).unwrap();
        st.drag(&mut s, (3, 1)).unwrap();
        assert_eq!(s.layer(0, 0).unwrap().get(5, 3).unwrap(), RED);
        assert_eq!(count(&s, RED), 1);
    }

    #[test]
    fn fill_outside_the_image_is_ignored() {
        let mut s = sprite();
        Stroke::begin(&mut s, T, brush(Tool::Fill), (-3, 2)).unwrap();
        assert_eq!(count(&s, RED), 0);
        Stroke::begin(&mut s, T, brush(Tool::Fill), (3, 2)).unwrap();
        assert_eq!(count(&s, RED), 64);
    }

    #[test]
    fn strokes_far_outside_do_not_fail() {
        let mut s = sprite();
        let mut st = Stroke::begin(&mut s, T, brush(Tool::Pencil), (-50, -50)).unwrap();
        st.drag(&mut s, (60, 60)).unwrap();
        assert_eq!(count(&s, RED), 8);
    }
}
