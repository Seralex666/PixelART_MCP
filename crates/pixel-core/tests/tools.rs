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

#[test]
fn photos_become_pixel_art_and_references_stay_out_of_the_picture() {
    use pixel_core::import::{prepare, Options, Picture, PictureFrame};
    use pixel_core::pixelate::{Background, Outline, Pixelate};
    // Ein „Foto“: roter Kreis mit Verlauf auf hellem Grund.
    let (w, h) = (200u32, 160u32);
    let pixels = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x as i64, y as i64)))
        .map(|(x, y)| {
            let (dx, dy) = (x - 100, y - 80);
            if dx * dx + dy * dy < 60 * 60 {
                Rgba::rgb((200 + dx / 2) as u8, (30 + y / 4) as u8, 40)
            } else {
                Rgba::rgb(235, 238, 240)
            }
        })
        .collect();
    let photo = Picture {
        width: w,
        height: h,
        frames: vec![PictureFrame {
            pixels,
            duration_ms: 100,
        }],
        timed: false,
    };
    let options = Options {
        pixelate: Some(Pixelate {
            width: 40,
            height: 32,
            colors: 5,
            background: Background::Auto,
            outline: Outline::Auto,
            ..Pixelate::for_source(w, h)
        }),
        ..Options::default()
    };
    let ready = prepare(&photo, options).unwrap();
    assert_eq!((ready.picture.width, ready.picture.height), (40, 32));
    assert!(ready.background_removed > 10_000);
    let px = &ready.picture.frames[0].pixels;
    assert!(px[0].is_transparent(), "Hintergrund ist weg");
    assert!(!px[16 * 40 + 20].is_transparent(), "Kreis bleibt");
    let colors: std::collections::HashSet<_> = px.iter().filter(|c| c.a > 0).collect();
    assert!(colors.len() <= 5, "{}", colors.len());

    // Vorlage-Ebene: sichtbar mit Vorlagen, aber nicht im Bild, nicht in der Palette.
    let mut s = Sprite::new(
        8,
        8,
        ColorDepth::Indexed8,
        Palette::new("p", vec![Rgba::rgb(0, 0, 0)]),
    )
    .unwrap();
    s.set_pixel(0, 0, 1, 1, Rgba::rgb(0, 0, 0)).unwrap();
    let r = pixel_core::pixelate::reference(&photo.frames[0].pixels, w, h, 8, 8);
    assert_eq!(s.add_reference_layer("Vorlage", &[r], 128).unwrap(), 0);
    assert!(s.is_reference(0) && !s.is_reference(1));
    assert!(s.composite(0).unwrap()[4 * 8 + 4].is_transparent());
    assert!(!s.composite_with_references(0).unwrap()[4 * 8 + 4].is_transparent());
    assert_eq!(s.used_colors(), vec![Rgba::rgb(0, 0, 0)]);
    s.add_frame();
    assert!(s.frame(1).unwrap().layers[0].reference);
    // Die Markierung übersteht Speichern und Laden.
    let json = serde_json::to_string(&s).unwrap();
    let back: Sprite = serde_json::from_str(&json).unwrap();
    assert!(back.is_reference(0));
}

#[test]
fn light_and_outline_work_together_and_the_ground_shadow_gets_no_outline() {
    use pixel_core::contour::AutoOutline;
    use pixel_core::light::{AutoLight, CastShadow};
    use pixel_core::quant::luminance;
    let body = Rgba::rgb(60, 110, 200);
    let mut s = sprite(20, 16);
    for y in 4..12 {
        for x in 8..12 {
            s.set_pixel(0, 0, x, y, body).unwrap();
        }
    }
    s.duplicate_frame(0).unwrap();
    s.set_auto_outline(AutoOutline::new("outline")).unwrap();
    let outline = |s: &Sprite| {
        let i = s.layer_index("outline").unwrap();
        s.layer(0, i)
            .unwrap()
            .pixels()
            .iter()
            .filter(|p| !p.is_transparent())
            .count()
    };
    let before = outline(&s);

    let mut spec = AutoLight::new((-20.0, -10.0));
    spec.light_layer = "light".into();
    spec.shadow_layer = "shade".into();
    spec.path.push((40.0, -10.0));
    spec.cast = Some(CastShadow::new("ground"));
    s.set_auto_light(spec).unwrap();
    s.refresh_outline().unwrap();

    // Order from the bottom: ground shadow, the figure, shade, light, outline on top.
    let names = s.layer_names();
    assert_eq!(names.len(), 5);
    assert_eq!(
        [&names[0], &names[2], &names[3], &names[4]],
        ["ground", "shade", "light", "outline"]
    );
    // The outline goes around the figure only, not around light or ground shadow.
    assert_eq!(outline(&s), before);
    // Frame 0: light from the left, the ground shadow falls to the right.
    let picture = s.composite(0).unwrap();
    let px = |x: usize, y: usize| picture[y * 20 + x];
    assert!(luminance(px(8, 8)) > luminance(body));
    assert!(luminance(px(11, 8)) < luminance(body));
    let ground = s.cast_shadow_layer().unwrap();
    let row = |f: usize, x: i64| {
        !s.layer(f, ground)
            .unwrap()
            .get(x, 11)
            .unwrap()
            .is_transparent()
    };
    assert!(row(0, 13) && !row(0, 6));
    // Frame 1: the light is on the right, so everything flips.
    assert!(row(1, 6) && !row(1, 13));
    // Saved and loaded, the light stays.
    let json = serde_json::to_string(&s).unwrap();
    let back: Sprite = serde_json::from_str(&json).unwrap();
    assert_eq!(back.auto_light(), s.auto_light());
}

#[test]
fn own_styles_are_kept_in_the_save_folder_and_built_in_ones_stay_fixed() {
    use pixel_core::stylebook::StylePreset;
    use pixel_core::workspace::Workspace;
    let dir = std::env::temp_dir().join(format!("pixel-styles-{}", uuid::Uuid::new_v4()));
    let ws = Workspace::open(&dir).unwrap();
    assert_eq!(ws.styles().len(), 6);
    assert!(ws.style("8bit").unwrap().builtin);

    let mut own = StylePreset::new("Sumpf");
    own.rules = vec!["grün und braun".into()];
    ws.save_style(&own).unwrap();
    let back = ws.style("sumpf").unwrap();
    assert!(!back.builtin);
    assert_eq!(back.rules, own.rules);
    assert_eq!(ws.styles().len(), 7);
    // Gleicher Name in anderer Schreibweise ersetzt den alten Stil.
    own.name = "SUMPF".into();
    ws.save_style(&own).unwrap();
    assert_eq!(ws.styles().len(), 7);
    // Feste Stile lassen sich nicht überschreiben.
    assert!(ws.save_style(&StylePreset::new("Manga")).is_err());
    // Ein Stil fürs Projekt: Palette, Licht und Umriss kommen mit, der Name wird gemerkt.
    let mut style = Style::default();
    ws.style("comic").unwrap().apply_to(&mut style);
    assert_eq!(style.outline, Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(style.preset.as_deref(), Some("Comic"));
    assert!(ws.delete_style("sumpf").unwrap());
    assert!(ws.style("Sumpf").is_none());
    std::fs::remove_dir_all(dir).ok();
}
