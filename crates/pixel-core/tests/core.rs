use pixel_core::draw::{self, Target};
use pixel_core::project::CharacterBundle;
use pixel_core::*;

const T: Target = Target::new(0, 0);

fn c(hex: &str) -> Rgba {
    hex.parse().unwrap()
}

#[test]
fn color_parsing_and_display() {
    assert_eq!(c("#ff8000"), Rgba::rgb(255, 128, 0));
    assert_eq!(c("ff800080"), Rgba::new(255, 128, 0, 128));
    assert_eq!(Rgba::rgb(255, 128, 0).to_string(), "#ff8000");
    assert!("#12345".parse::<Rgba>().is_err());
    assert!("#äöüäöü".parse::<Rgba>().is_err());
}

#[test]
fn size_limits() {
    assert!(Sprite::new(1024, 1024, ColorDepth::Rgba32, Palette::default()).is_ok());
    assert!(Sprite::new(1025, 16, ColorDepth::Rgba32, Palette::default()).is_err());
    assert!(Sprite::new(0, 16, ColorDepth::Rgba32, Palette::default()).is_err());
}

#[test]
fn rgba32_keeps_color_exactly() {
    let mut s = Sprite::new(4, 4, ColorDepth::Rgba32, Palette::default()).unwrap();
    let col = Rgba::new(13, 77, 201, 99);
    assert_eq!(s.set_pixel(0, 0, 1, 2, col).unwrap(), col);
    assert_eq!(s.layer(0, 0).unwrap().get(1, 2).unwrap(), col);
}

#[test]
fn rgb565_quantizes_and_is_idempotent() {
    let mut s = Sprite::new(4, 4, ColorDepth::Rgb565, Palette::default()).unwrap();
    let got = s
        .set_pixel(0, 0, 0, 0, Rgba::new(13, 77, 201, 200))
        .unwrap();
    assert_eq!(got.a, 255);
    assert_eq!(got.to_rgb565(), got);
    assert_eq!(Rgba::WHITE.to_rgb565(), Rgba::WHITE);
    assert_eq!(Rgba::BLACK.to_rgb565(), Rgba::BLACK);
    assert_eq!(
        s.set_pixel(0, 0, 1, 0, Rgba::new(255, 0, 0, 40)).unwrap(),
        Rgba::TRANSPARENT
    );
}

#[test]
fn indexed8_grows_palette_then_snaps() {
    let mut s = Sprite::new(4, 4, ColorDepth::Indexed8, Palette::new("p", vec![])).unwrap();
    for i in 0..=255u8 {
        assert_eq!(s.conform(Rgba::rgb(i, 0, 0)), Rgba::rgb(i, 0, 0));
    }
    assert_eq!(s.palette.len(), 256);
    // Palette voll: neue Farbe rastet auf die nächste vorhandene ein.
    let got = s.conform(Rgba::rgb(10, 3, 0));
    assert!(s.palette.contains(got));
    assert_eq!(s.palette.len(), 256);
}

#[test]
fn locked_palette_never_gains_colors() {
    let mut ch = Character::new("Held", 16, 24, ColorDepth::Rgba32, Palette::default_16()).unwrap();
    let before = ch.palette().colors.clone();
    let got = ch
        .base
        .set_pixel(0, 0, 3, 3, Rgba::rgb(250, 250, 250))
        .unwrap();
    assert_eq!(got, c("#f4f4f4"));
    assert_eq!(ch.palette().colors, before);
}

#[test]
fn drawing_tools_are_pixel_exact() {
    let mut s = Sprite::new(8, 8, ColorDepth::Rgba32, Palette::default()).unwrap();
    let red = Rgba::rgb(255, 0, 0);
    draw::line(&mut s, T, (0, 0), (7, 7), red).unwrap();
    let l = s.layer(0, 0).unwrap();
    assert_eq!(l.pixels().iter().filter(|p| **p == red).count(), 8);
    assert!((0..8).all(|i| l.get(i, i).unwrap() == red));

    let mut s = Sprite::new(8, 8, ColorDepth::Rgba32, Palette::default()).unwrap();
    draw::rect(&mut s, T, 1, 1, 6, 6, red, false).unwrap();
    assert_eq!(
        s.layer(0, 0)
            .unwrap()
            .pixels()
            .iter()
            .filter(|p| **p == red)
            .count(),
        20
    );
    // Innenfläche füllen, der Rand hält die Füllung auf.
    let blue = Rgba::rgb(0, 0, 255);
    assert_eq!(draw::flood_fill(&mut s, T, 3, 3, blue).unwrap(), 16);
    assert_eq!(s.layer(0, 0).unwrap().get(0, 0).unwrap(), Rgba::TRANSPARENT);

    // Zeichnen über den Rand hinaus wird abgeschnitten statt zu scheitern.
    draw::rect(&mut s, T, -5, -5, 100, 100, red, true).unwrap();
    assert!(s.layer(0, 0).unwrap().pixels().iter().all(|p| *p == red));
    draw::line(&mut s, T, (-10, 3), (20, 3), blue).unwrap();
}

#[test]
fn ellipse_is_symmetric_and_closed() {
    for (w, h) in [(7u32, 7u32), (8, 8), (12, 5), (1, 1), (2, 9)] {
        let mut s = Sprite::new(16, 16, ColorDepth::Rgba32, Palette::default()).unwrap();
        let col = Rgba::WHITE;
        draw::ellipse(&mut s, T, 2, 2, w, h, col, false).unwrap();
        let l = s.layer(0, 0).unwrap();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let a = l.get(2 + x, 2 + y).unwrap();
                assert_eq!(
                    a,
                    l.get(2 + w as i64 - 1 - x, 2 + y).unwrap(),
                    "h-sym {w}x{h}"
                );
                assert_eq!(
                    a,
                    l.get(2 + x, 2 + h as i64 - 1 - y).unwrap(),
                    "v-sym {w}x{h}"
                );
            }
        }
        // Geschlossen: Füllen von außen erreicht die Mitte nicht.
        if w >= 5 && h >= 5 {
            draw::flood_fill(&mut s, T, 0, 0, Rgba::rgb(1, 2, 3)).unwrap();
            let mid = s
                .layer(0, 0)
                .unwrap()
                .get(2 + w as i64 / 2, 2 + h as i64 / 2)
                .unwrap();
            assert_eq!(mid, Rgba::TRANSPARENT, "Umriss {w}x{h} ist offen");
        }
    }
}

#[test]
fn layers_and_frames_stay_in_sync() {
    let mut s = Sprite::new(4, 4, ColorDepth::Rgba32, Palette::default()).unwrap();
    s.add_frame();
    let top = s.add_layer("Oben");
    assert!(s.frames().iter().all(|f| f.layers.len() == 2));
    s.set_pixel(0, 0, 0, 0, Rgba::rgb(255, 0, 0)).unwrap();
    s.set_pixel(0, top, 0, 0, Rgba::rgb(0, 255, 0)).unwrap();
    assert_eq!(s.composite(0).unwrap()[0], Rgba::rgb(0, 255, 0));
    s.layer_mut(0, top).unwrap().visible = false;
    assert_eq!(s.composite(0).unwrap()[0], Rgba::rgb(255, 0, 0));
    assert_eq!(s.composite(1).unwrap()[0], Rgba::TRANSPARENT);
    let dup = s.duplicate_frame(0).unwrap();
    assert_eq!(s.frame(dup).unwrap(), s.frame(0).unwrap());
    assert!(s.remove_layer(5).is_err());
    s.check().unwrap();
}

fn hero() -> Character {
    let mut ch =
        Character::new("Held", 16, 24, ColorDepth::Indexed8, Palette::default_16()).unwrap();
    draw::rect(&mut ch.base, T, 5, 2, 6, 6, c("#ffcd75"), true).unwrap();
    draw::rect(&mut ch.base, T, 4, 8, 8, 10, c("#3b5dc9"), true).unwrap();
    draw::rect(&mut ch.base, T, 5, 18, 2, 6, c("#333c57"), true).unwrap();
    draw::rect(&mut ch.base, T, 9, 18, 2, 6, c("#333c57"), true).unwrap();
    ch
}

#[test]
fn animations_start_from_base_pose_and_validate() {
    let mut p = Project::new("Test");
    let id = p.add_character(hero());
    for kind in AnimationKind::PRESETS {
        let n = kind.default_frames();
        let a = p.add_animation(id, kind, None).unwrap();
        let anim = p.animation(a).unwrap();
        assert_eq!(anim.sprite.frames().len(), n);
        let base = p.character(id).unwrap().base.composite(0).unwrap();
        for i in 0..n {
            assert_eq!(anim.sprite.composite(i).unwrap(), base);
        }
    }
    assert!(p.validate().is_empty());

    // Zeichnen in der Animation kann die Palette nicht verlassen.
    let a = p.animations[0].id;
    let got = p
        .animation_mut(a)
        .unwrap()
        .sprite
        .set_pixel(1, 0, 0, 0, Rgba::rgb(1, 200, 3))
        .unwrap();
    assert!(p.character(id).unwrap().palette().contains(got));
    assert!(p.validate().is_empty());

    // Wer die Sperre umgeht, wird von validate() erwischt.
    let anim = p.animation_mut(a).unwrap();
    anim.sprite
        .layer_mut(0, 0)
        .unwrap()
        .set(0, 0, Rgba::rgb(1, 200, 3))
        .unwrap();
    let issues = p.validate();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("#01c803"));
}

#[test]
fn project_roundtrip_and_character_transfer() {
    let dir = std::env::temp_dir().join(format!("pixel-core-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut p = Project::new("Spiel A");
    let id = p.add_character(hero());
    p.add_animation(id, AnimationKind::Walk, None).unwrap();
    let mut tile = Sprite::new(16, 16, ColorDepth::Rgb565, Palette::default()).unwrap();
    draw::ellipse(&mut tile, T, 0, 0, 16, 16, c("#38b764"), true).unwrap();
    p.add_image("Busch", tile);

    let path = dir.join("a.pxproj");
    p.save(&path).unwrap();
    let loaded = Project::load(&path).unwrap();
    assert_eq!(loaded, p);
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "keine Temp-Dateien übrig"
    );

    // Figur in ein zweites Projekt übernehmen: Pixel für Pixel identisch.
    let bundle_path = dir.join("held.pxchar");
    p.export_character(id).unwrap().save(&bundle_path).unwrap();
    let mut q = Project::new("Spiel B");
    let imported = q
        .import_character(CharacterBundle::load(&bundle_path).unwrap())
        .unwrap();
    assert_eq!(imported, id);
    assert_eq!(q.character(id).unwrap(), p.character(id).unwrap());
    assert_eq!(q.animations_of(id).count(), 1);
    // Erneuter Import ersetzt statt zu verdoppeln.
    q.import_character(CharacterBundle::load(&bundle_path).unwrap())
        .unwrap();
    assert_eq!(q.characters.len(), 1);
    assert_eq!(q.animations.len(), 1);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rejects_corrupt_and_future_files() {
    let mut p = Project::new("X");
    p.add_image(
        "b",
        Sprite::new(4, 4, ColorDepth::Rgba32, Palette::default()).unwrap(),
    );
    let json = p.to_json().unwrap();
    assert!(
        Project::from_json(&json.replace("\"format_version\":1", "\"format_version\":99")).is_err()
    );
    // Breite manipuliert: Pixelzahl passt nicht mehr.
    assert!(Project::from_json(&json.replace("\"width\":4", "\"width\":5")).is_err());
    assert!(Project::from_json("{").is_err());
}

#[test]
fn export_scales_without_smoothing() {
    let mut s = Sprite::new(2, 2, ColorDepth::Rgba32, Palette::default()).unwrap();
    s.set_pixel(0, 0, 0, 0, Rgba::rgb(255, 0, 0)).unwrap();
    s.set_pixel(0, 0, 1, 1, Rgba::rgb(0, 0, 255)).unwrap();
    let png = export::frame_png(&s, 0, 4).unwrap();
    let (w, h, px) = export::decode(&png).unwrap();
    assert_eq!((w, h), (8, 8));
    let unique: std::collections::HashSet<_> = px.iter().collect();
    assert_eq!(unique.len(), 3, "nur die Originalfarben, keine Mischfarben");
    assert_eq!(px[3], Rgba::rgb(255, 0, 0));
    assert_eq!(px[4], Rgba::TRANSPARENT);
    assert_eq!(px[7 * 8 + 7], Rgba::rgb(0, 0, 255));

    let jpg = export::frame_jpeg(&s, 0, 4, Rgba::WHITE, 95).unwrap();
    let (w, h, px) = export::decode(&jpg).unwrap();
    assert_eq!((w, h), (8, 8));
    assert!(
        px[4 * 8].r > 240 && px[4 * 8].a == 255,
        "Transparenz liegt auf Weiß"
    );
}

#[test]
fn spritesheet_layout_and_metadata() {
    let mut s = Sprite::new(4, 6, ColorDepth::Rgba32, Palette::default()).unwrap();
    for i in 1..5 {
        s.add_frame();
        s.set_pixel(i, 0, 0, 0, Rgba::rgb(i as u8 * 50, 0, 0))
            .unwrap();
    }
    let (png, meta) = export::sheet_png(&s, 3, 2).unwrap();
    assert_eq!((meta.columns, meta.rows), (3, 2));
    assert_eq!((meta.image_width, meta.image_height), (24, 24));
    assert_eq!(meta.frames.len(), 5);
    assert_eq!((meta.frames[4].x, meta.frames[4].y), (8, 12));
    let (w, _, px) = export::decode(&png).unwrap();
    assert_eq!(px[(12 * w + 8) as usize], Rgba::rgb(200, 0, 0));
    let (_, one_row) = export::sheet_png(&s, 0, 1).unwrap();
    assert_eq!((one_row.columns, one_row.rows), (5, 1));
}

#[test]
fn templates_are_valid() {
    let mut ids = std::collections::HashSet::new();
    for t in TEMPLATES {
        assert!(ids.insert(t.id), "doppelte Vorlage {}", t.id);
        assert!(Sprite::new(t.width, t.height, ColorDepth::Rgba32, Palette::default()).is_ok());
    }
    assert_eq!(template::find("art-1024").unwrap().width, 1024);
}

#[test]
fn from_sprite_locks_a_palette_that_covers_every_pixel() {
    // 32-Bit-Sprites tragen ihre Farben nicht in die Palette ein. Beim Umwandeln
    // in eine Figur müssen sie trotzdem alle in der gesperrten Palette landen.
    let mut s = Sprite::new(4, 4, ColorDepth::Rgba32, Palette::default_16()).unwrap();
    let odd = Rgba::rgb(12, 34, 56);
    s.set_pixel(0, 0, 0, 0, odd).unwrap();
    let ch = Character::from_sprite("X", s).unwrap();
    assert!(ch.palette().locked && ch.palette().contains(odd));

    let mut p = Project::new("T");
    let id = p.add_character(ch);
    let a = p.add_animation(id, AnimationKind::Idle, None).unwrap();
    assert!(p.validate().is_empty());
    // Dieselbe Farbe erneut malen ändert sie nicht.
    let got = p
        .animation_mut(a)
        .unwrap()
        .sprite
        .set_pixel(0, 0, 1, 1, odd)
        .unwrap();
    assert_eq!(got, odd);

    // Zu viele Farben für eine Palette: klare Absage statt stiller Verfälschung.
    let mut big = Sprite::new(32, 32, ColorDepth::Rgba32, Palette::new("leer", vec![])).unwrap();
    for i in 0..300u32 {
        big.set_pixel(
            0,
            0,
            (i % 32) as i64,
            (i / 32) as i64,
            Rgba::rgb(i as u8, (i >> 8) as u8, 7),
        )
        .unwrap();
    }
    assert!(Character::from_sprite("Bunt", big).is_err());
}

#[test]
fn locked_palette_respects_color_depth() {
    let mut ch = Character::new("R", 4, 4, ColorDepth::Rgb565, Palette::default_16()).unwrap();
    for &c in &ch.palette().colors {
        assert_eq!(c.to_rgb565(), c, "Palettenfarbe {c} liegt nicht in RGB565");
    }
    let got = ch.base.set_pixel(0, 0, 0, 0, c("#1a1c2c")).unwrap();
    assert_eq!(got.to_rgb565(), got);
    assert!(ch.palette().contains(got));

    // Wechsel der Farbtiefe normalisiert Palette und Pixel gemeinsam.
    let mut s = Sprite::new(2, 2, ColorDepth::Rgba32, Palette::default_16()).unwrap();
    s.set_pixel(0, 0, 0, 0, c("#1a1c2c")).unwrap();
    s.palette.locked = true;
    s.set_depth(ColorDepth::Rgb565);
    let px = s.layer(0, 0).unwrap().get(0, 0).unwrap();
    assert_eq!(px.to_rgb565(), px);
    assert!(s.palette.contains(px));
}

#[test]
fn empty_palette_cannot_hold_colors() {
    assert!(Character::new(
        "Leer",
        4,
        4,
        ColorDepth::Indexed8,
        Palette::new("leer", vec![])
    )
    .is_err());
    assert_eq!(
        Palette::new("leer", vec![]).nearest(Rgba::WHITE),
        Rgba::TRANSPARENT
    );
    let mut s = Sprite::new(2, 2, ColorDepth::Indexed8, Palette::new("leer", vec![])).unwrap();
    s.palette.locked = true;
    assert_eq!(
        s.set_pixel(0, 0, 0, 0, Rgba::WHITE).unwrap(),
        Rgba::TRANSPARENT
    );
    assert!(s.used_colors().is_empty());
}

#[test]
fn huge_shapes_are_bounded() {
    let mut s = Sprite::new(8, 8, ColorDepth::Rgba32, Palette::default()).unwrap();
    let red = Rgba::rgb(255, 0, 0);
    let start = std::time::Instant::now();
    // Weit jenseits der Grenze: sofortige Absage, keine Milliarden Schleifendurchläufe.
    assert!(draw::line(&mut s, T, (0, 0), (1_000_000_000, 0), red).is_err());
    assert!(draw::ellipse(&mut s, T, 0, 0, 8, u32::MAX, red, true).is_err());
    assert!(draw::rect(&mut s, T, i64::MAX, 0, u32::MAX, 1, red, true).is_err());
    assert!(s.used_colors().is_empty());
    // Innerhalb der Grenze, aber viel größer als das Bild: schnell und korrekt.
    let big = draw::COORD_LIMIT as u32;
    draw::ellipse(
        &mut s,
        T,
        -(big as i64) / 2,
        -(big as i64) / 2,
        big,
        big,
        red,
        true,
    )
    .unwrap();
    assert!(s.layer(0, 0).unwrap().pixels().iter().all(|p| *p == red));
    draw::ellipse(&mut s, T, 100, 100, big, big, Rgba::WHITE, false).unwrap();
    draw::line(
        &mut s,
        T,
        (-draw::COORD_LIMIT, 3),
        (draw::COORD_LIMIT, 3),
        Rgba::WHITE,
    )
    .unwrap();
    assert!((0..8).all(|x| s.layer(0, 0).unwrap().get(x, 3).unwrap() == Rgba::WHITE));
    draw::shift(&mut s, T, i64::MAX, i64::MIN).unwrap();
    assert!(
        start.elapsed().as_secs() < 2,
        "zu langsam: {:?}",
        start.elapsed()
    );
}

#[test]
fn oversized_layer_dimensions_in_a_file_are_an_error_not_a_panic() {
    let mut p = Project::new("X");
    p.add_image(
        "b",
        Sprite::new(4, 4, ColorDepth::Rgba32, Palette::default()).unwrap(),
    );
    let mut v: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
    let layer = &mut v["images"][0]["sprite"]["frames"][0]["layers"][0];
    layer["width"] = 65536.into();
    layer["height"] = 65536.into();
    assert!(Project::from_json(&v.to_string()).is_err());
}
