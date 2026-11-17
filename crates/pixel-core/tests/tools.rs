//! Formen, Sprühdose, Aufhellen und Abdunkeln, Spiegeln, Ausschnitte und Leinwandgröße.

use pixel_core::brush;
use pixel_core::draw::{self, Target};
use pixel_core::effects::Area;
use pixel_core::shapes::{self, CustomShape, Shape};
use pixel_core::*;

const T: Target = Target::new(0, 0);

fn c(hex: &str) -> Rgba {
    hex.parse().unwrap()
}

fn sprite(w: u32, h: u32) -> Sprite {
    Sprite::new(w, h, ColorDepth::Rgba32, Palette::default()).unwrap()
}

fn count(s: &Sprite) -> usize {
    s.layer(0, 0)
        .unwrap()
        .pixels()
        .iter()
        .filter(|p| !p.is_transparent())
        .count()
}

fn at(s: &Sprite, x: i64, y: i64) -> Rgba {
    s.layer(0, 0).unwrap().get(x, y).unwrap()
}

#[test]
fn every_shape_fits_its_box_and_the_outline_is_its_edge() {
    for shape in Shape::ALL {
        for (w, h) in [(1, 1), (3, 3), (9, 9), (16, 11)] {
            let mask = shape.mask(w, h);
            assert_eq!(mask.len(), (w * h) as usize);
            assert!(mask.contains(&true), "{shape:?} {w}x{h} is empty");
        }
        let mut s = sprite(20, 20);
        let filled =
            shapes::draw_shape(&mut s, T, shape, 2, 2, 15, 15, c("#ff0000"), true).unwrap();
        let mut o = sprite(20, 20);
        let edge = shapes::draw_shape(&mut o, T, shape, 2, 2, 15, 15, c("#ff0000"), false).unwrap();
        assert!(edge < filled, "{shape:?}: edge {edge} vs filled {filled}");
        assert!(edge > 8);
        assert_eq!(Shape::parse(shape.id()), Some(shape));
    }
}

#[test]
fn heart_and_diamond_are_symmetric_left_to_right() {
    for shape in [Shape::Heart, Shape::Diamond, Shape::Star, Shape::Triangle] {
        let (w, h) = (13, 12);
        let m = shape.mask(w, h);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    m[(y * w + x) as usize],
                    m[(y * w + (w - 1 - x)) as usize],
                    "{shape:?} at {x},{y}"
                );
            }
        }
    }
    // Das Herz hat oben in der Mitte eine Kerbe und unten eine Spitze.
    let m = Shape::Heart.mask(11, 10);
    assert!(!m[5], "notch at the top middle");
    assert!(m[9 * 11 + 5], "tip at the bottom middle");
    assert!(!m[9 * 11]);
}

#[test]
fn custom_shapes_come_from_drawings_and_scale() {
    let mut s = sprite(10, 10);
    draw::rect(&mut s, T, 2, 3, 3, 2, c("#00ff00"), true).unwrap();
    draw::pixels(&mut s, T, &[(4, 5)], c("#0000ff")).unwrap();
    let shape = CustomShape::from_layer("Klotz", &s, T, None).unwrap();
    assert_eq!((shape.width, shape.height), (3, 3));
    let mut out = sprite(10, 10);
    // Doppelt so groß, in einer Farbe: 6 Pixel gefüllt werden zu 24.
    let n =
        shapes::draw_custom(&mut out, T, &shape, 0, 0, 6, 6, c("#ff0000"), true, false).unwrap();
    assert_eq!(n, 4 * 7);
    let mut keep = sprite(10, 10);
    shapes::draw_custom(&mut keep, T, &shape, 0, 0, 3, 3, c("#ff0000"), true, true).unwrap();
    assert_eq!(at(&keep, 2, 2), c("#0000ff"));
    assert_eq!(at(&keep, 0, 0), c("#00ff00"));
    assert!(CustomShape::from_layer("leer", &s, T, Some((7, 7, 3, 3))).is_err());
}

#[test]
fn shapes_are_kept_in_the_save_folder() {
    let dir = std::env::temp_dir().join(format!("pixelart-shapes-{}", uuid::Uuid::new_v4()));
    let ws = Workspace::open(&dir).unwrap();
    assert!(ws.custom_shapes().is_empty());
    let mut s = sprite(4, 4);
    draw::rect(&mut s, T, 0, 0, 2, 2, c("#ff0000"), true).unwrap();
    let shape = CustomShape::from_layer("Mein Block", &s, T, None).unwrap();
    ws.save_shape(&shape).unwrap();
    let all = ws.custom_shapes();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0], shape);
    assert!(ws.custom_shape("mein block").is_some());
    assert!(ws.delete_shape("Mein Block").unwrap());
    assert!(ws.custom_shapes().is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn spray_scatters_inside_its_radius_and_repeats_with_the_same_seed() {
    let mut a = sprite(30, 30);
    let n = brush::spray(&mut a, T, 15, 15, 5, 0.3, c("#ff0000"), 7).unwrap();
    assert!(n > 5 && n < 60, "{n}");
    let mut b = sprite(30, 30);
    brush::spray(&mut b, T, 15, 15, 5, 0.3, c("#ff0000"), 7).unwrap();
    assert_eq!(a, b);
    for y in 0..30 {
        for x in 0..30 {
            if !at(&a, x, y).is_transparent() {
                assert!((x - 15).pow(2) + (y - 15).pow(2) <= 30);
            }
        }
    }
    let mut full = sprite(30, 30);
    assert_eq!(
        brush::spray(&mut full, T, 0, 0, 2, 1.0, c("#ff0000"), 1).unwrap(),
        8
    );
    assert!(brush::spray(&mut full, T, 0, 0, 2, 1.5, c("#ff0000"), 1).is_err());
}

#[test]
fn tone_walks_along_the_palette_ramp() {
    let ramp = ["#1a1c2c", "#29366f", "#3b5dc9", "#41a6f6", "#73eff7"];
    let mut pal = Palette::new("p", ramp.iter().map(|h| c(h)).collect());
    pal.colors.push(c("#b13e53"));
    let ch = Character::new("Held", 4, 4, ColorDepth::Indexed8, pal).unwrap();
    let mut s = ch.base;
    draw::pixels(&mut s, T, &[(0, 0), (1, 0)], c("#3b5dc9")).unwrap();
    // Doppelte Punkte zählen nur einmal.
    let n = brush::tone(&mut s, T, &[(0, 0), (0, 0), (2, 2)], true).unwrap();
    assert_eq!(n, 1);
    assert_eq!(at(&s, 0, 0), c("#41a6f6"));
    brush::tone(&mut s, T, &[(1, 0)], false).unwrap();
    assert_eq!(at(&s, 1, 0), c("#29366f"));
    // Am Ende der Treppe bleibt die Farbe, das Rot wird nicht genommen.
    brush::tone(&mut s, T, &[(0, 0)], true).unwrap();
    brush::tone(&mut s, T, &[(0, 0)], true).unwrap();
    assert_eq!(at(&s, 0, 0), c("#73eff7"));
    // Ohne passende Palettenfarbe wird die Farbe selbst heller.
    let mut free = sprite(2, 2);
    draw::pixels(&mut free, T, &[(0, 0)], c("#804020")).unwrap();
    brush::tone(&mut free, T, &[(0, 0)], true).unwrap();
    let lum = pixel_core::quant::luminance;
    assert!(lum(at(&free, 0, 0)) > lum(c("#804020")));
}

#[test]
fn mirror_copies_changes_to_the_other_side() {
    let mut s = sprite(8, 6);
    let before = s.layer(0, 0).unwrap().pixels().to_vec();
    draw::pixels(&mut s, T, &[(1, 1)], c("#ff0000")).unwrap();
    brush::mirror_changes(&mut s, T, &before, true, true).unwrap();
    for (x, y) in [(1, 1), (6, 1), (1, 4), (6, 4)] {
        assert_eq!(at(&s, x, y), c("#ff0000"), "{x},{y}");
    }
    assert_eq!(count(&s), 4);
}

#[test]
fn areas_move_flip_and_clear() {
    let mut s = sprite(10, 10);
    draw::pixels(&mut s, T, &[(1, 1)], c("#ff0000")).unwrap();
    draw::pixels(&mut s, T, &[(2, 1)], c("#00ff00")).unwrap();
    let a = Area {
        x: 1,
        y: 1,
        w: 2,
        h: 1,
    };
    brush::flip_area(&mut s, T, a, true, false).unwrap();
    assert_eq!(at(&s, 1, 1), c("#00ff00"));
    let moved = brush::move_area(&mut s, T, a, 3, 2).unwrap();
    assert_eq!((moved.x, moved.y), (4, 3));
    assert_eq!(at(&s, 4, 3), c("#00ff00"));
    assert!(at(&s, 1, 1).is_transparent());
    assert_eq!(brush::clear_area(&mut s, T, moved).unwrap(), 2);
    assert_eq!(count(&s), 0);
}

#[test]
fn canvas_resize_keeps_content_and_characters_stay_consistent() {
    let mut s = sprite(4, 4);
    draw::pixels(&mut s, T, &[(0, 0), (3, 3)], c("#ff0000")).unwrap();
    assert_eq!(s.content_bounds(), Some((0, 0, 4, 4)));
    let (dx, dy) = sprite::canvas_offset((4, 4), (8, 6), (0, 1));
    assert_eq!((dx, dy), (2, 2));
    s.resize_canvas(8, 6, dx, dy).unwrap();
    assert_eq!((s.width(), s.height()), (8, 6));
    assert_eq!(at(&s, 2, 2), c("#ff0000"));
    assert_eq!(at(&s, 5, 5), c("#ff0000"));
    // Zuschneiden auf den Inhalt.
    let (x, y, w, h) = s.content_bounds().unwrap();
    s.resize_canvas(w, h, -x, -y).unwrap();
    assert_eq!((s.width(), s.height()), (4, 4));
    assert!(s.resize_canvas(0, 4, 0, 0).is_err());

    let pal = Palette::new("p", vec![c("#ff0000")]);
    let mut p = Project::new("T");
    let id = p.add_character(Character::new("Held", 8, 8, ColorDepth::Indexed8, pal).unwrap());
    p.add_animation(id, AnimationKind::Walk, Some(2)).unwrap();
    p.resize_character(id, 12, 10, 2, 2).unwrap();
    assert_eq!(p.character(id).unwrap().size(), (12, 10));
    assert_eq!(p.character(id).unwrap().anchor, (6, 9));
    assert!(p.validate().is_empty(), "{:?}", p.validate());
}

#[test]
fn ramp_palettes_have_32_and_64_distinct_colors_from_dark_to_light() {
    for (p, n) in [(Palette::ramps_32(), 32), (Palette::ramps_64(), 64)] {
        assert_eq!(p.len(), n);
        let distinct: std::collections::HashSet<_> = p.colors.iter().collect();
        assert_eq!(distinct.len(), n, "{}", p.name);
    }
    let p = Palette::ramps_32();
    let lum = pixel_core::quant::luminance;
    // Jede Treppe wird heller.
    for ramp in p.colors[4..].chunks(4) {
        assert!(ramp.windows(2).all(|w| lum(w[0]) < lum(w[1])), "{ramp:?}");
    }
}
