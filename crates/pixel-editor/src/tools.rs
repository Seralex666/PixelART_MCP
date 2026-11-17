//! Zeichenwerkzeuge: was ein Mausstrich auf der Leinwand bewirkt.
//! Ohne Oberfläche, damit es sich testen lässt.

use crate::doc::Res;
use pixel_core::brush;
use pixel_core::draw::{self, Target};
use pixel_core::effects::{self, Area, Patch};
use pixel_core::lang::tr;
use pixel_core::shapes::{self, CustomShape, Shape};
use pixel_core::{Rgba, Sprite};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pencil,
    Eraser,
    Fill,
    Picker,
    Spray,
    Shade,
    Line,
    Rect,
    Ellipse,
    Shape,
    Select,
    Move,
}

impl Tool {
    pub const ALL: [Tool; 12] = [
        Tool::Pencil,
        Tool::Eraser,
        Tool::Fill,
        Tool::Picker,
        Tool::Spray,
        Tool::Shade,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Shape,
        Tool::Select,
        Tool::Move,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Pencil => tr("Stift", "Pencil"),
            Tool::Eraser => tr("Radierer", "Eraser"),
            Tool::Fill => tr("Füllen", "Fill"),
            Tool::Picker => tr("Pipette", "Picker"),
            Tool::Spray => tr("Sprühdose", "Spray"),
            Tool::Shade => tr("Aufhellen / Abdunkeln", "Lighten / darken"),
            Tool::Line => tr("Linie", "Line"),
            Tool::Rect => tr("Rechteck", "Rectangle"),
            Tool::Ellipse => "Ellipse",
            Tool::Shape => tr("Formen", "Shapes"),
            Tool::Select => tr("Auswahl", "Select"),
            Tool::Move => tr("Ebene verschieben", "Move layer"),
        }
    }

    /// Tastenkürzel.
    pub fn key(self) -> char {
        match self {
            Tool::Pencil => 'B',
            Tool::Eraser => 'E',
            Tool::Fill => 'G',
            Tool::Picker => 'I',
            Tool::Spray => 'A',
            Tool::Shade => 'D',
            Tool::Line => 'L',
            Tool::Rect => 'R',
            Tool::Ellipse => 'O',
            Tool::Shape => 'U',
            Tool::Select => 'S',
            Tool::Move => 'M',
        }
    }

    /// Ob die Pinselgröße bei diesem Werkzeug etwas bewirkt.
    pub fn uses_size(self) -> bool {
        matches!(
            self,
            Tool::Pencil | Tool::Eraser | Tool::Line | Tool::Spray | Tool::Shade
        )
    }

    /// Ob „gefüllt“ bei diesem Werkzeug etwas bewirkt.
    pub fn uses_fill(self) -> bool {
        matches!(self, Tool::Rect | Tool::Ellipse | Tool::Shape)
    }

    /// Ob das Spiegeln beim Zeichnen bei diesem Werkzeug wirkt.
    pub fn mirrors(self) -> bool {
        !matches!(self, Tool::Picker | Tool::Select | Tool::Move)
    }
}

pub const MAX_BRUSH: u32 = 16;

/// Welche Form das Formen-Werkzeug zeichnet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeChoice {
    Builtin(Shape),
    /// Eine eigene Form, nach ihrer Stelle in der Liste der eigenen Formen.
    Custom(usize),
}

/// Einstellungen eines Strichs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub tool: Tool,
    pub color: Rgba,
    /// Kantenlänge des Pinsels in Pixeln (Stift, Radierer, Linie, Aufhellen);
    /// bei der Sprühdose der Radius.
    pub size: u32,
    /// Rechteck, Ellipse und Formen gefüllt statt nur als Umriss.
    pub filled: bool,
    /// Dichte der Sprühdose in Prozent.
    pub density: u32,
    /// Aufhellen statt Abdunkeln.
    pub lighten: bool,
    /// Stift ohne doppelte Pixel in Ecken („pixel-perfekt“), nur bei Größe 1.
    pub pixel_perfect: bool,
    /// Beim Zeichnen links und rechts spiegeln (an der senkrechten Mitte).
    pub mirror_x: bool,
    /// Beim Zeichnen oben und unten spiegeln (an der waagerechten Mitte).
    pub mirror_y: bool,
    pub shape: ShapeChoice,
    /// Eigene Formen in ihren eigenen Farben zeichnen.
    pub keep_colors: bool,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            tool: Tool::Pencil,
            color: Rgba::BLACK,
            size: 1,
            filled: false,
            density: 30,
            lighten: true,
            pixel_perfect: false,
            mirror_x: false,
            mirror_y: false,
            shape: ShapeChoice::Builtin(Shape::Heart),
            keep_colors: false,
        }
    }
}

type Cell = (i64, i64);

/// Ein angehobener Ausschnitt: die Pixel der Auswahl und die Ebene ohne sie.
/// Solange er schwebt, lässt er sich verschieben, ohne zu übermalen, was darunter liegt.
#[derive(Clone, Debug, PartialEq)]
pub struct Lifted {
    pub patch: Patch,
    pub area: Area,
    /// Die Ebene ohne den Ausschnitt.
    pub base: Vec<Rgba>,
}

impl Lifted {
    /// Hebt `area` aus der Ebene, ohne sie zu ändern.
    pub fn lift(s: &Sprite, t: Target, area: Area) -> Res<Self> {
        let area = brush::clip_area(s, area).ok_or_else(|| {
            tr(
                "Die Auswahl liegt außerhalb des Bildes",
                "The selection lies outside the image",
            )
            .to_string()
        })?;
        let patch = Patch::cut(s, t.frame, Some(t.layer), Some(area)).map_err(|e| e.to_string())?;
        let mut copy = s.clone();
        brush::clear_area(&mut copy, t, area).map_err(|e| e.to_string())?;
        let base = copy
            .layer(t.frame, t.layer)
            .map_err(|e| e.to_string())?
            .pixels()
            .to_vec();
        Ok(Self { patch, area, base })
    }
}

/// Ein laufender Mausstrich: gedrückt, gezogen, losgelassen.
pub struct Stroke {
    brush: Brush,
    target: Target,
    start: Cell,
    last: Cell,
    /// Stand der Ebene beim Beginn: für Vorschau, Aufhellen und pixel-perfekte Linien.
    base: Vec<Rgba>,
    size: (i64, i64),
    /// Bisheriger Weg des Stifts, für pixel-perfekte Linien.
    path: Vec<Cell>,
    /// Schon aufgehellte oder abgedunkelte Pixel: jeder zählt einmal je Strich.
    toned: HashSet<Cell>,
    custom: Option<CustomShape>,
    /// Beim Verschieben einer Auswahl: der angehobene Ausschnitt.
    lifted: Option<Lifted>,
    /// Die Auswahl, die der Strich aufzieht oder verschiebt.
    selection: Option<Area>,
    puffs: u64,
}

impl Stroke {
    /// Beginnt einen Strich an der Zelle `at` und setzt den ersten Punkt.
    /// `custom` ist die gewählte eigene Form, `lifted` eine Auswahl, die verschoben wird.
    pub fn begin(
        s: &mut Sprite,
        target: Target,
        brush: Brush,
        at: Cell,
        custom: Option<CustomShape>,
        lifted: Option<Lifted>,
    ) -> Res<Self> {
        let layer = s
            .layer(target.frame, target.layer)
            .map_err(|e| e.to_string())?;
        let base = layer.pixels().to_vec();
        let mut stroke = Self {
            brush,
            target,
            start: at,
            last: at,
            base,
            size: (i64::from(s.width()), i64::from(s.height())),
            path: Vec::new(),
            toned: HashSet::new(),
            custom,
            lifted,
            selection: None,
            puffs: 0,
        };
        match brush.tool {
            Tool::Fill => {
                for (x, y) in stroke.mirrored(at) {
                    // Außerhalb des Bildes gibt es nichts zu füllen.
                    if stroke.inside((x, y)) {
                        draw::flood_fill(s, target, x, y, brush.color)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            Tool::Picker => {}
            Tool::Pencil | Tool::Eraser | Tool::Spray | Tool::Shade => {
                stroke.paint(s, at, at)?;
            }
            _ => stroke.drag(s, at)?,
        }
        Ok(stroke)
    }

    /// Die Zelle, bis zu der der Strich schon geführt ist.
    pub fn last(&self) -> Cell {
        self.last
    }

    pub fn tool(&self) -> Tool {
        self.brush.tool
    }

    /// Ob die Maus seit dem Drücken bewegt wurde.
    pub fn dragged(&self) -> bool {
        self.start != self.last
    }

    /// Ob dieser Strich eine Auswahl verschiebt (statt eine aufzuziehen).
    pub fn is_moving(&self) -> bool {
        self.lifted.is_some()
    }

    /// Die Auswahl, die dieser Strich aufzieht oder verschoben hat.
    pub fn selection(&self) -> Option<Area> {
        self.selection
    }

    /// Beendet den Strich und gibt eine verschobene Auswahl zurück, die weiter schwebt.
    pub fn finish(self) -> Option<Lifted> {
        let area = self.selection?;
        self.lifted.map(|l| Lifted { area, ..l })
    }

    fn inside(&self, (x, y): Cell) -> bool {
        x >= 0 && y >= 0 && x < self.size.0 && y < self.size.1
    }

    /// Die Spiegelungen, die beim Zeichnen gelten: immer das Original, dazu je nach Einstellung.
    fn flips(&self) -> Vec<(bool, bool)> {
        let b = self.brush;
        let mut out = vec![(false, false)];
        if b.tool.mirrors() {
            if b.mirror_x {
                out.push((true, false));
            }
            if b.mirror_y {
                out.push((false, true));
            }
            if b.mirror_x && b.mirror_y {
                out.push((true, true));
            }
        }
        out
    }

    fn flip_cell(&self, (x, y): Cell, (fx, fy): (bool, bool)) -> Cell {
        (
            if fx { self.size.0 - 1 - x } else { x },
            if fy { self.size.1 - 1 - y } else { y },
        )
    }

    /// Ein Rechteck `(x, y, w, h)` gespiegelt.
    fn flip_box(&self, (x, y, w, h): (i64, i64, u32, u32), (fx, fy): (bool, bool)) -> (i64, i64) {
        (
            if fx {
                self.size.0 - x - i64::from(w)
            } else {
                x
            },
            if fy {
                self.size.1 - y - i64::from(h)
            } else {
                y
            },
        )
    }

    fn mirrored(&self, c: Cell) -> Vec<Cell> {
        let mut out: Vec<Cell> = self
            .flips()
            .into_iter()
            .map(|f| self.flip_cell(c, f))
            .collect();
        out.dedup();
        out
    }

    /// Führt den Strich bis zur Zelle `at` weiter.
    pub fn drag(&mut self, s: &mut Sprite, at: Cell) -> Res {
        let t = self.target;
        let b = self.brush;
        let err = |e: pixel_core::Error| e.to_string();
        match b.tool {
            Tool::Pencil | Tool::Eraser | Tool::Spray | Tool::Shade => {
                self.paint(s, self.last, at)?;
            }
            Tool::Line => {
                self.restore(s)?;
                for f in self.flips() {
                    let (a, z) = (self.flip_cell(self.start, f), self.flip_cell(at, f));
                    dabs(s, t, a, z, b.size, b.color)?;
                }
            }
            Tool::Rect | Tool::Ellipse | Tool::Shape => {
                self.restore(s)?;
                let x = self.start.0.min(at.0);
                let y = self.start.1.min(at.1);
                let w = (self.start.0 - at.0).unsigned_abs() as u32 + 1;
                let h = (self.start.1 - at.1).unsigned_abs() as u32 + 1;
                for f in self.flips() {
                    let (fx, fy) = self.flip_box((x, y, w, h), f);
                    match b.tool {
                        Tool::Rect => {
                            draw::rect(s, t, fx, fy, w, h, b.color, b.filled).map_err(err)?
                        }
                        Tool::Ellipse => {
                            draw::ellipse(s, t, fx, fy, w, h, b.color, b.filled).map_err(err)?
                        }
                        _ => self.shape(s, (fx, fy, w, h), f)?,
                    }
                }
            }
            Tool::Select => {
                let (dx, dy) = (at.0 - self.start.0, at.1 - self.start.1);
                if let Some(l) = &self.lifted {
                    s.layer_mut(t.frame, t.layer)
                        .and_then(|layer| layer.replace_pixels(l.base.clone()))
                        .map_err(err)?;
                    effects::stamp(s, t, &l.patch, l.area.x + dx, l.area.y + dy).map_err(err)?;
                    self.selection = Some(Area {
                        x: l.area.x + dx,
                        y: l.area.y + dy,
                        ..l.area
                    });
                } else {
                    self.selection = Some(Area {
                        x: self.start.0.min(at.0),
                        y: self.start.1.min(at.1),
                        w: (dx.unsigned_abs() + 1) as u32,
                        h: (dy.unsigned_abs() + 1) as u32,
                    });
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

    /// Ein weiterer Sprühstoß an der aktuellen Stelle, solange die Maus gedrückt bleibt.
    pub fn tick(&mut self, s: &mut Sprite) -> Res {
        if self.brush.tool == Tool::Spray {
            self.spray(s, self.last)?;
        }
        Ok(())
    }

    /// Stift, Radierer, Sprühdose und Aufhellen von `from` bis `to`.
    fn paint(&mut self, s: &mut Sprite, from: Cell, to: Cell) -> Res {
        let t = self.target;
        let b = self.brush;
        let color = if b.tool == Tool::Eraser {
            Rgba::TRANSPARENT
        } else {
            b.color
        };
        match b.tool {
            Tool::Pencil if b.pixel_perfect && b.size <= 1 => {
                let cells = line_cells(from, to);
                // Die erste Zelle ist beim Weiterziehen schon gesetzt.
                let fresh = if from == to { &cells[..] } else { &cells[1..] };
                for &c in fresh {
                    self.path.push(c);
                    self.set(s, c, color)?;
                    self.drop_corner(s)?;
                }
            }
            Tool::Pencil | Tool::Eraser => {
                for f in self.flips() {
                    let (a, z) = (self.flip_cell(from, f), self.flip_cell(to, f));
                    dabs(s, t, a, z, b.size, color)?;
                }
            }
            Tool::Spray => {
                for c in line_cells(from, to) {
                    self.spray(s, c)?;
                }
            }
            Tool::Shade => {
                let n = b.size.clamp(1, MAX_BRUSH) as i64;
                let offset = (n - 1) / 2;
                let mut cells = Vec::new();
                for (x, y) in line_cells(from, to) {
                    for dy in 0..n {
                        for dx in 0..n {
                            cells.extend(self.mirrored((x - offset + dx, y - offset + dy)));
                        }
                    }
                }
                self.tone(s, &cells)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Setzt einen Pixel samt seinen Spiegelungen.
    fn set(&self, s: &mut Sprite, c: Cell, color: Rgba) -> Res {
        draw::pixels(s, self.target, &self.mirrored(c), color).map_err(|e| e.to_string())
    }

    /// Pixel-perfekt: Bilden die letzten drei Zellen eine Ecke, fällt die mittlere weg.
    fn drop_corner(&mut self, s: &mut Sprite) -> Res {
        let n = self.path.len();
        if n < 3 {
            return Ok(());
        }
        let (a, m, c) = (self.path[n - 3], self.path[n - 2], self.path[n - 1]);
        let diagonal = (a.0 - c.0).abs() == 1 && (a.1 - c.1).abs() == 1;
        let corner = (m.0 == a.0 || m.1 == a.1) && (m.0 == c.0 || m.1 == c.1);
        if diagonal && corner {
            for (x, y) in self.mirrored(m) {
                if self.inside((x, y)) {
                    let old = self.base[(y * self.size.0 + x) as usize];
                    s.layer_mut(self.target.frame, self.target.layer)
                        .map_err(|e| e.to_string())?
                        .set_clipped(x, y, old);
                }
            }
            self.path.remove(n - 2);
        }
        Ok(())
    }

    fn spray(&mut self, s: &mut Sprite, (x, y): Cell) -> Res {
        self.puffs += 1;
        let b = self.brush;
        // Je Stoß nur ein Teil der Dichte; sie baut sich beim Halten auf.
        let density = f64::from(b.density.clamp(1, 100)) / 100.0 * 0.3;
        let seed = self.puffs.wrapping_mul(0x9e37_79b9) ^ ((x as u64) << 20) ^ (y as u64);
        let points = brush::spray_points(x, y, b.size.clamp(1, MAX_BRUSH), density, seed);
        let mut all = Vec::with_capacity(points.len());
        for p in points {
            all.extend(self.mirrored(p));
        }
        draw::pixels(s, self.target, &all, b.color).map_err(|e| e.to_string())
    }

    /// Hellt oder dunkelt Zellen um eine Stufe ab, ausgehend vom Stand beim Beginn.
    fn tone(&mut self, s: &mut Sprite, cells: &[Cell]) -> Res {
        let lighten = self.brush.lighten;
        for &c in cells {
            if !self.inside(c) || !self.toned.insert(c) {
                continue;
            }
            let old = self.base[(c.1 * self.size.0 + c.0) as usize];
            let new = brush::tone_color(s, old, lighten);
            if new != old {
                s.set_pixel(self.target.frame, self.target.layer, c.0, c.1, new)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Zeichnet die gewählte Form in ein Rechteck, für eine Spiegelung passend gedreht.
    fn shape(
        &self,
        s: &mut Sprite,
        (x, y, w, h): (i64, i64, u32, u32),
        (fx, fy): (bool, bool),
    ) -> Res {
        let b = self.brush;
        let (mask, colors) = match (b.shape, &self.custom) {
            (ShapeChoice::Custom(_), Some(c)) => {
                (c.mask(w, h), b.keep_colors.then(|| c.colors(w, h)))
            }
            (ShapeChoice::Builtin(shape), _) => (shape.mask(w, h), None),
            (ShapeChoice::Custom(_), None) => (Shape::Heart.mask(w, h), None),
        };
        let mask = if b.filled {
            mask
        } else {
            shapes::edge(&mask, w, h)
        };
        let mask = shapes::flip(&mask, w, h, fx, fy);
        let colors = colors.map(|cs| shapes::flip(&cs, w, h, fx, fy));
        shapes::draw_mask(
            s,
            self.target,
            x,
            y,
            w,
            h,
            &mask,
            b.color,
            colors.as_deref(),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn restore(&self, s: &mut Sprite) -> Res {
        s.layer_mut(self.target.frame, self.target.layer)
            .and_then(|l| l.replace_pixels(self.base.clone()))
            .map_err(|e| e.to_string())
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
            ..Brush::default()
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

    fn at(s: &Sprite, x: i64, y: i64) -> Rgba {
        s.layer(0, 0).unwrap().get(x, y).unwrap()
    }

    fn begin(s: &mut Sprite, b: Brush, c: Cell) -> Stroke {
        Stroke::begin(s, T, b, c, None, None).unwrap()
    }

    const T: Target = Target::new(0, 0);

    #[test]
    fn pencil_connects_fast_mouse_moves() {
        let mut s = sprite();
        let mut st = begin(&mut s, brush(Tool::Pencil), (0, 0));
        st.drag(&mut s, (7, 0)).unwrap();
        assert_eq!(count(&s, RED), 8);
    }

    #[test]
    fn big_brush_and_eraser() {
        let mut s = sprite();
        let mut b = brush(Tool::Pencil);
        b.size = 3;
        begin(&mut s, b, (4, 4));
        assert_eq!(count(&s, RED), 9);
        b.tool = Tool::Eraser;
        b.size = 1;
        begin(&mut s, b, (4, 4));
        assert_eq!(count(&s, RED), 8);
    }

    #[test]
    fn shape_preview_does_not_leave_traces() {
        let mut s = sprite();
        let mut st = begin(&mut s, brush(Tool::Rect), (1, 1));
        st.drag(&mut s, (6, 6)).unwrap();
        st.drag(&mut s, (3, 3)).unwrap();
        // Nur der Umriss des letzten Rechtecks 3×3 bleibt: 8 Pixel.
        assert_eq!(count(&s, RED), 8);
        // Rückwärts aufgezogen ergibt dasselbe.
        let mut s2 = sprite();
        let mut st = begin(&mut s2, brush(Tool::Rect), (3, 3));
        st.drag(&mut s2, (1, 1)).unwrap();
        assert_eq!(s, s2);
    }

    #[test]
    fn move_shifts_from_the_original() {
        let mut s = sprite();
        begin(&mut s, brush(Tool::Pencil), (2, 2));
        let mut st = begin(&mut s, brush(Tool::Move), (0, 0));
        st.drag(&mut s, (1, 0)).unwrap();
        st.drag(&mut s, (3, 1)).unwrap();
        assert_eq!(at(&s, 5, 3), RED);
        assert_eq!(count(&s, RED), 1);
    }

    #[test]
    fn fill_outside_the_image_is_ignored() {
        let mut s = sprite();
        begin(&mut s, brush(Tool::Fill), (-3, 2));
        assert_eq!(count(&s, RED), 0);
        begin(&mut s, brush(Tool::Fill), (3, 2));
        assert_eq!(count(&s, RED), 64);
    }

    #[test]
    fn strokes_far_outside_do_not_fail() {
        let mut s = sprite();
        let mut st = begin(&mut s, brush(Tool::Pencil), (-50, -50));
        st.drag(&mut s, (60, 60)).unwrap();
        assert_eq!(count(&s, RED), 8);
    }

    #[test]
    fn mirrored_drawing_lands_on_both_sides() {
        let mut s = sprite();
        let mut b = brush(Tool::Pencil);
        b.mirror_x = true;
        b.mirror_y = true;
        begin(&mut s, b, (1, 2));
        for (x, y) in [(1, 2), (6, 2), (1, 5), (6, 5)] {
            assert_eq!(at(&s, x, y), RED, "{x},{y}");
        }
        // Ein Rechteck links oben erscheint gespiegelt rechts unten.
        let mut s = sprite();
        let mut b = brush(Tool::Rect);
        b.mirror_x = true;
        b.mirror_y = true;
        b.filled = true;
        let mut st = begin(&mut s, b, (0, 0));
        st.drag(&mut s, (1, 2)).unwrap();
        assert_eq!(count(&s, RED), 4 * 6);
        assert_eq!(at(&s, 7, 7), RED);
        assert_eq!(at(&s, 6, 5), RED);
    }

    #[test]
    fn pixel_perfect_pencil_drops_corner_pixels() {
        let mut s = sprite();
        let mut b = brush(Tool::Pencil);
        b.pixel_perfect = true;
        // Treppe: rechts, runter, rechts, runter … ohne Eckpixel.
        let mut st = begin(&mut s, b, (0, 0));
        for c in [(1, 0), (1, 1), (2, 1), (2, 2), (3, 2)] {
            st.drag(&mut s, c).unwrap();
        }
        assert_eq!(at(&s, 1, 0), Rgba::TRANSPARENT);
        assert_eq!(at(&s, 2, 1), Rgba::TRANSPARENT);
        assert_eq!(count(&s, RED), 4);
        // Ohne die Einstellung bleiben die Ecken.
        let mut s = sprite();
        let mut st = begin(&mut s, brush(Tool::Pencil), (0, 0));
        for c in [(1, 0), (1, 1), (2, 1), (2, 2), (3, 2)] {
            st.drag(&mut s, c).unwrap();
        }
        assert_eq!(count(&s, RED), 6);
    }

    #[test]
    fn spray_builds_up_and_shade_changes_each_pixel_once() {
        let mut s = Sprite::new(20, 20, ColorDepth::Rgba32, Palette::default_16()).unwrap();
        let mut b = brush(Tool::Spray);
        b.size = 4;
        b.density = 100;
        let mut st = Stroke::begin(&mut s, T, b, (10, 10), None, None).unwrap();
        let first = count(&s, RED);
        for _ in 0..20 {
            st.tick(&mut s).unwrap();
        }
        assert!(count(&s, RED) > first);

        let mut s = sprite();
        draw::rect(&mut s, T, 0, 0, 8, 8, Rgba::rgb(0x3b, 0x5d, 0xc9), true).unwrap();
        let mut b = brush(Tool::Shade);
        b.size = 2;
        let mut st = begin(&mut s, b, (2, 2));
        let once = at(&s, 2, 2);
        assert_ne!(once, Rgba::rgb(0x3b, 0x5d, 0xc9));
        // Zurück und wieder hin: derselbe Pixel wird nicht noch heller.
        st.drag(&mut s, (4, 2)).unwrap();
        st.drag(&mut s, (2, 2)).unwrap();
        assert_eq!(at(&s, 2, 2), once);
    }

    #[test]
    fn shapes_and_selection_moves() {
        let mut s = sprite();
        let mut b = brush(Tool::Shape);
        b.filled = true;
        let mut st = begin(&mut s, b, (0, 0));
        st.drag(&mut s, (6, 6)).unwrap();
        assert_eq!(at(&s, 0, 0), Rgba::TRANSPARENT, "corner next to the heart");
        assert_eq!(at(&s, 3, 0), Rgba::TRANSPARENT, "notch of the heart");
        assert_eq!(at(&s, 1, 0), RED);

        // Auswahl aufziehen ändert nichts, Verschieben hebt den Ausschnitt an.
        let mut s = sprite();
        draw::pixels(&mut s, T, &[(1, 1)], RED).unwrap();
        let before = s.clone();
        let mut st = begin(&mut s, brush(Tool::Select), (0, 0));
        st.drag(&mut s, (2, 2)).unwrap();
        assert_eq!(s, before);
        let area = st.selection().unwrap();
        assert_eq!((area.x, area.y, area.w, area.h), (0, 0, 3, 3));
        let lifted = Lifted::lift(&s, T, area).unwrap();
        let mut st =
            Stroke::begin(&mut s, T, brush(Tool::Select), (1, 1), None, Some(lifted)).unwrap();
        st.drag(&mut s, (4, 3)).unwrap();
        st.drag(&mut s, (5, 4)).unwrap();
        assert_eq!(at(&s, 5, 4), RED);
        assert_eq!(count(&s, RED), 1);
        let floating = st.finish().unwrap();
        assert_eq!((floating.area.x, floating.area.y), (4, 3));
    }
}
