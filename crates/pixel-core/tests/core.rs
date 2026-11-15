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

fn png_header(bytes: &[u8]) -> (png::ColorType, png::BitDepth) {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    (reader.info().color_type, reader.info().bit_depth)
}

#[test]
fn indexed_png_has_the_same_pixels_in_the_smallest_table() {
    // Ungerade Breite: Zeilen enden mitten im Byte.
    let mut s = Sprite::new(3, 2, ColorDepth::Rgba32, Palette::default()).unwrap();
    s.set_pixel(0, 0, 0, 0, Rgba::rgb(255, 0, 0)).unwrap();
    for scale in [1, 3] {
        let indexed = export::frame_png_indexed(&s, 0, scale).unwrap();
        assert_eq!(
            png_header(&indexed),
            (png::ColorType::Indexed, png::BitDepth::One),
            "zwei Farben brauchen ein Bit"
        );
        assert_eq!(
            export::decode(&indexed).unwrap(),
            export::decode(&export::frame_png(&s, 0, scale).unwrap()).unwrap()
        );
    }

    s.set_pixel(0, 0, 1, 0, Rgba::rgb(0, 255, 0)).unwrap();
    s.set_pixel(0, 0, 2, 0, Rgba::rgb(0, 0, 255)).unwrap();
    s.set_pixel(0, 0, 0, 1, Rgba::new(10, 20, 30, 128)).unwrap();
    s.set_pixel(0, 0, 2, 1, Rgba::rgb(1, 2, 3)).unwrap();
    let indexed = export::frame_png_indexed(&s, 0, 5).unwrap();
    assert_eq!(
        png_header(&indexed),
        (png::ColorType::Indexed, png::BitDepth::Four),
        "sechs Farben brauchen vier Bit"
    );
    let (w, h, px) = export::decode(&indexed).unwrap();
    assert_eq!((w, h), (15, 10));
    assert_eq!(
        px,
        export::decode(&export::frame_png(&s, 0, 5).unwrap())
            .unwrap()
            .2
    );
    assert_eq!(px[5 * 15], Rgba::new(10, 20, 30, 128), "halbtransparent");
    assert_eq!(px[5 * 15 + 5], Rgba::TRANSPARENT);
    assert!(indexed.len() < export::frame_png(&s, 0, 5).unwrap().len());

    let mut many = Sprite::new(20, 20, ColorDepth::Rgba32, Palette::default()).unwrap();
    for i in 0..400i64 {
        let c = Rgba::rgb((i % 256) as u8, (i / 256) as u8, 7);
        many.set_pixel(0, 0, i % 20, i / 20, c).unwrap();
    }
    assert!(matches!(
        export::frame_png_indexed(&many, 0, 1),
        Err(Error::TooManyColors {
            found: 400,
            max: 256
        })
    ));
}

/// Liest ein GIF zurück: Größe, Wiederholung und je Frame Dauer und Pixel.
fn read_gif(bytes: &[u8]) -> (u16, u16, gif::Repeat, Vec<(u16, Vec<Rgba>)>) {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(std::io::Cursor::new(bytes)).unwrap();
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        let px = frame
            .buffer
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| Rgba::new(c[0], c[1], c[2], c[3]))
            .collect();
        frames.push((frame.delay, px));
    }
    (decoder.width(), decoder.height(), decoder.repeat(), frames)
}

#[test]
fn gif_keeps_colors_transparency_and_timing() {
    let mut s = Sprite::new(3, 2, ColorDepth::Rgba32, Palette::default()).unwrap();
    s.add_frame();
    s.set_pixel(0, 0, 0, 0, Rgba::rgb(255, 0, 0)).unwrap();
    s.set_pixel(0, 0, 1, 0, Rgba::new(0, 0, 255, 200)).unwrap();
    s.set_pixel(0, 0, 2, 0, Rgba::new(0, 255, 0, 60)).unwrap();
    s.set_pixel(1, 0, 2, 1, Rgba::rgb(0, 0, 1)).unwrap();
    s.frame_mut(0).unwrap().duration_ms = 100;
    s.frame_mut(1).unwrap().duration_ms = 255;

    let (bytes, info) = export::animation_gif(&s, 2, true).unwrap();
    assert_eq!(
        info,
        export::GifInfo {
            colors: 3,
            exact: true
        }
    );
    let (w, h, repeat, frames) = read_gif(&bytes);
    assert_eq!((w, h), (6, 4));
    assert!(matches!(repeat, gif::Repeat::Infinite));
    assert_eq!(frames.len(), 2);
    assert_eq!((frames[0].0, frames[1].0), (10, 26), "Hundertstelsekunden");
    let first = &frames[0].1;
    assert_eq!(first[0], Rgba::rgb(255, 0, 0));
    assert_eq!(first[7], Rgba::rgb(255, 0, 0), "vergrößert ohne Glättung");
    assert_eq!(first[2], Rgba::rgb(0, 0, 255), "über halb deckend: deckend");
    assert_eq!(first[4].a, 0, "unter halb deckend: durchsichtig");
    assert_eq!(first[12].a, 0);
    let second = &frames[1].1;
    assert_eq!(second[0].a, 0, "das vorige Bild scheint nicht durch");
    assert_eq!(second[3 * 6 + 5], Rgba::rgb(0, 0, 1), "fast schwarz bleibt");

    let (once, _) = export::animation_gif(&s, 1, false).unwrap();
    assert!(matches!(read_gif(&once).2, gif::Repeat::Finite(0)));

    assert_eq!(export::gif_delay(5), 2, "zu kurze Frames spielt niemand ab");
    assert_eq!(export::gif_delay(16), 2);
    assert_eq!(export::gif_delay(1000), 100);
}

#[test]
fn gif_reduces_colors_only_when_there_are_too_many() {
    // Genau 255 Farben und Transparenz passen noch exakt.
    let mut s = Sprite::new(16, 16, ColorDepth::Rgba32, Palette::default()).unwrap();
    for i in 0..255i64 {
        let c = Rgba::rgb(i as u8, 255 - i as u8, 9);
        s.set_pixel(0, 0, i % 16, i / 16, c).unwrap();
    }
    let (bytes, info) = export::animation_gif(&s, 1, true).unwrap();
    assert_eq!((info.colors, info.exact), (255, true));
    let px = &read_gif(&bytes).3[0].1;
    assert_eq!(px[254], Rgba::rgb(254, 1, 9));
    assert_eq!(px[255].a, 0);

    // Ein Farbverlauf mit 1024 Farben muss reduziert werden.
    let mut s = Sprite::new(32, 32, ColorDepth::Rgba32, Palette::default()).unwrap();
    for y in 0..32i64 {
        for x in 0..32i64 {
            if (x, y) != (0, 0) {
                let c = Rgba::rgb((x * 8) as u8, (y * 8) as u8, ((x + y) * 4) as u8);
                s.set_pixel(0, 0, x, y, c).unwrap();
            }
        }
    }
    let (bytes, info) = export::animation_gif(&s, 1, true).unwrap();
    assert!(!info.exact);
    assert!(info.colors <= 255);
    let px = &read_gif(&bytes).3[0].1;
    assert_eq!(px[0].a, 0, "Transparenz bleibt Transparenz");
    let original = s.composite(0).unwrap();
    for (got, want) in px.iter().zip(&original).skip(1) {
        assert_eq!(got.a, 255);
        assert!(
            got.distance_sq(*want) < 9 * 12 * 12,
            "{got} ist zu weit weg von {want}"
        );
    }
}

#[test]
fn video_frames_follow_the_frame_durations() {
    use pixel_core::video::{ffmpeg_args, VideoPlan};
    let plan = VideoPlan::new(&[100, 100, 100]);
    assert_eq!(
        (plan.tick_ms, plan.repeats.as_slice()),
        (100, &[1, 1, 1][..])
    );
    let plan = VideoPlan::new(&[100, 250, 400]);
    assert_eq!(
        (plan.tick_ms, plan.repeats.as_slice()),
        (50, &[2, 5, 8][..])
    );
    assert_eq!(plan.total_frames(), 15);
    let plan = VideoPlan::new(&[33, 4]);
    assert_eq!((plan.tick_ms, plan.repeats.as_slice()), (10, &[3, 1][..]));

    let args = ffmpeg_args(64, 32, 50, true);
    let has = |pair: [&str; 2]| args.windows(2).any(|w| w == pair);
    assert!(has(["-s", "64x32"]) && has(["-r", "1000/50"]) && has(["-i", "-"]));
    assert!(has(["-lossless", "1"]) && has(["-auto-alt-ref", "0"]));
    assert!(args.iter().any(|a| a.ends_with("format=yuva420p")));
    let opaque = ffmpeg_args(64, 32, 50, false);
    assert!(opaque.iter().any(|a| a.ends_with("format=yuv420p")));
    assert!(!opaque.contains(&"-auto-alt-ref".to_string()));
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

#[test]
fn project_files_are_compact() {
    let mut p = Project::new("Groß");
    let mut s = Sprite::new(1024, 1024, ColorDepth::Rgba32, Palette::default()).unwrap();
    draw::rect(&mut s, T, 100, 100, 800, 800, c("#3b5dc9"), true).unwrap();
    p.add_image("Fläche", s);
    let json = p.to_json().unwrap();
    assert!(
        json.len() < 100_000,
        "1024×1024 sollte klein bleiben, ist {} Bytes",
        json.len()
    );
    assert_eq!(Project::from_json(&json).unwrap(), p);
}

#[test]
fn character_palette_is_append_only() {
    let mut p = Project::new("Test");
    let id = p.add_character(hero());
    let anim = p.add_animation(id, AnimationKind::Run, None).unwrap();
    let before = p.character(id).unwrap().palette().colors.clone();
    let n = p
        .add_character_colors(id, &[c("#ff00ff"), before[0]])
        .unwrap();
    assert_eq!(n, before.len() + 1);
    let after = &p.character(id).unwrap().palette().colors;
    assert_eq!(&after[..before.len()], &before[..]);
    assert!(p.character(id).unwrap().palette().locked);
    assert_eq!(
        p.animation(anim).unwrap().sprite.palette,
        *p.character(id).unwrap().palette()
    );
    // Die neue Farbe ist jetzt in der Animation zeichenbar.
    let got = p
        .animation_mut(anim)
        .unwrap()
        .sprite
        .set_pixel(0, 0, 0, 0, c("#ff00ff"))
        .unwrap();
    assert_eq!(got, c("#ff00ff"));
    assert!(p.validate().is_empty());
}

#[test]
fn image_palette_swap_remaps_indexed_pixels() {
    let mut s = Sprite::new(2, 1, ColorDepth::Indexed8, Palette::new("p", vec![])).unwrap();
    s.set_pixel(0, 0, 0, 0, Rgba::rgb(250, 10, 10)).unwrap();
    s.set_palette(Palette::new(
        "rb",
        vec![Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255)],
    ))
    .unwrap();
    assert_eq!(
        s.layer(0, 0).unwrap().get(0, 0).unwrap(),
        Rgba::rgb(255, 0, 0)
    );
    assert_eq!(s.layer(0, 0).unwrap().get(1, 0).unwrap(), Rgba::TRANSPARENT);
    let mut ch = hero();
    assert!(ch.base.set_palette(Palette::grayscale(4)).is_err());
}

#[test]
fn reads_uncompressed_pixel_data_from_the_first_format_version() {
    use base64::Engine;
    let mut p = Project::new("Alt");
    let mut s = Sprite::new(2, 1, ColorDepth::Rgba32, Palette::default()).unwrap();
    s.set_pixel(0, 0, 1, 0, Rgba::rgb(10, 20, 30)).unwrap();
    p.add_image("b", s);
    let mut v: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
    let pixels = &mut v["images"][0]["sprite"]["frames"][0]["layers"][0]["pixels"];
    assert!(pixels.as_str().unwrap().starts_with("z:"));
    // So sah das Feld vor der Komprimierung aus: rohe RGBA-Bytes in Base64.
    let raw = [0u8, 0, 0, 0, 10, 20, 30, 255];
    *pixels = base64::engine::general_purpose::STANDARD.encode(raw).into();
    assert_eq!(Project::from_json(&v.to_string()).unwrap(), p);
}

#[test]
fn new_frames_inherit_layer_visibility_and_opacity() {
    let mut s = Sprite::new(2, 2, ColorDepth::Rgba32, Palette::default()).unwrap();
    let top = s.add_layer("Oben");
    s.layer_mut(0, top).unwrap().visible = false;
    s.layer_mut(0, 0).unwrap().opacity = 128;
    let f = s.add_frame();
    assert!(!s.layer(f, top).unwrap().visible);
    assert_eq!(s.layer(f, 0).unwrap().opacity, 128);
    assert!(s
        .layer(f, 0)
        .unwrap()
        .pixels()
        .iter()
        .all(|p| p.is_transparent()));
}

#[test]
fn extreme_coordinates_are_rejected_without_overflow() {
    let mut s = Sprite::new(8, 8, ColorDepth::Rgba32, Palette::default()).unwrap();
    let red = Rgba::rgb(255, 0, 0);
    for v in [i64::MIN, i64::MAX, i64::MIN + 1] {
        assert!(draw::line(&mut s, T, (v, 0), (0, 0), red).is_err());
        assert!(draw::line(&mut s, T, (0, 0), (0, v), red).is_err());
        assert!(draw::rect(&mut s, T, v, 0, 4, 4, red, true).is_err());
        assert!(draw::ellipse(&mut s, T, 0, v, 4, 4, red, true).is_err());
    }
    assert!(s.used_colors().is_empty());
}

mod workspace {
    use pixel_core::workspace::{
        check_asset_name, check_project_name, file_safe, ExportFormat, NameClash, ProjectNameError,
        Workspace,
    };
    use pixel_core::{ColorDepth, Palette, Rgba, Sprite};

    fn temp() -> Workspace {
        let dir = std::env::temp_dir().join(format!("pixelart-core-test-{}", uuid::Uuid::new_v4()));
        Workspace::open(dir).unwrap()
    }

    #[test]
    fn project_names_cannot_leave_the_workspace() {
        let ws = temp();
        for bad in [
            "",
            "  ",
            "../evil",
            "a/b",
            "a\\b",
            "x.pxproj",
            &"a".repeat(65),
        ] {
            assert_eq!(
                check_project_name(bad),
                Err(ProjectNameError::Invalid),
                "{bad}"
            );
            assert!(ws.project_path(bad).is_err());
        }
        assert_eq!(check_project_name("NUL"), Err(ProjectNameError::Reserved));
        let path = ws.project_path("  Mein Spiel-2 ").unwrap();
        assert_eq!(path, ws.home().join("Mein Spiel-2.pxproj"));
    }

    #[test]
    fn create_list_load_roundtrip() {
        let ws = temp();
        assert!(ws.project_names().is_empty());
        ws.create("beta").unwrap();
        ws.create("Alpha").unwrap();
        assert!(ws.create("beta").is_err());
        assert_eq!(ws.project_names(), ["Alpha", "beta"]);
        assert!(ws.exists("Alpha") && !ws.exists("Gamma"));
        assert_eq!(ws.load("Alpha").unwrap().name, "Alpha");
        assert!(ws.modified("Alpha").is_some());
        assert!(ws.modified("Gamma").is_none());
    }

    #[test]
    fn asset_names_clash_when_they_export_to_the_same_file() {
        let taken = vec!["Tile A".to_string()];
        assert_eq!(
            check_asset_name(&taken, "tile a"),
            Err(NameClash::Same("Tile A".into()))
        );
        assert_eq!(
            check_asset_name(&taken, "Tile_A"),
            Err(NameClash::Similar("Tile A".into()))
        );
        assert_eq!(check_asset_name(&taken, "Tile B"), Ok(()));
        assert_eq!(file_safe("con"), "con_");
        assert_eq!(file_safe("a/b:c"), "a_b_c");
    }

    #[test]
    fn export_names_files_by_asset_frame_and_scale() {
        let ws = temp();
        ws.create("Spiel").unwrap();
        let mut s = Sprite::new(4, 4, ColorDepth::Rgba32, Palette::default_16()).unwrap();
        s.add_frame();
        let dir = ws.exports_dir("Spiel");

        let png = ws
            .export("Spiel", "Held 1", &s, ExportFormat::Png, None, 1)
            .unwrap();
        assert_eq!(
            png.files,
            [dir.join("Held_1-00.png"), dir.join("Held_1-01.png")]
        );
        let one = ws
            .export("Spiel", "Held 1", &s, ExportFormat::Png, Some(1), 2)
            .unwrap();
        assert_eq!(one.files, [dir.join("Held_1-01@2x.png")]);
        let jpg = ExportFormat::Jpg {
            background: Rgba::WHITE,
            quality: 90,
        };
        let jpg = ws.export("Spiel", "Held 1", &s, jpg, Some(0), 1).unwrap();
        assert_eq!(jpg.files, [dir.join("Held_1-00.jpg")]);
        let sheet = ExportFormat::Sheet { columns: 0 };
        let sheet = ws.export("Spiel", "Held 1", &s, sheet, None, 3).unwrap();
        assert_eq!(
            sheet.files,
            [
                dir.join("Held_1-sheet@3x.png"),
                dir.join("Held_1-sheet@3x.json")
            ]
        );
        assert_eq!(sheet.sheet.unwrap().frames.len(), 2);
        for f in png
            .files
            .iter()
            .chain(&one.files)
            .chain(&jpg.files)
            .chain(&sheet.files)
        {
            assert!(f.is_file(), "{}", f.display());
        }

        assert!(ws
            .export("Spiel", "Held 1", &s, ExportFormat::Png, Some(2), 1)
            .is_err());
        assert!(ws
            .export("Spiel", "Held 1", &s, ExportFormat::Png, None, 0)
            .is_err());
        assert!(ws
            .export("../x", "Held 1", &s, ExportFormat::Png, None, 1)
            .is_err());
    }

    #[test]
    fn export_writes_gif_indexed_png_and_webm() {
        let ws = temp();
        ws.create("Spiel").unwrap();
        let mut s = Sprite::new(4, 4, ColorDepth::Rgba32, Palette::default_16()).unwrap();
        s.add_frame();
        s.set_pixel(0, 0, 0, 0, Rgba::rgb(200, 30, 30)).unwrap();
        s.set_pixel(1, 0, 3, 3, Rgba::rgb(30, 30, 200)).unwrap();
        let dir = ws.exports_dir("Spiel");

        let gif = ExportFormat::Gif { looping: true };
        let gif = ws.export("Spiel", "Held 1", &s, gif, None, 4).unwrap();
        assert_eq!(gif.files, [dir.join("Held_1@4x.gif")]);
        assert!(gif.gif.unwrap().exact);
        assert!(std::fs::read(&gif.files[0]).unwrap().starts_with(b"GIF89a"));

        let indexed = ExportFormat::PngIndexed;
        let indexed = ws.export("Spiel", "Held 1", &s, indexed, None, 1).unwrap();
        assert_eq!(
            indexed.files,
            [dir.join("Held_1-00.png"), dir.join("Held_1-01.png")]
        );
        assert!(indexed.gif.is_none());

        // Ungerade Vergrößerung macht im Video unscharfe Kanten.
        let odd = ws.export("Spiel", "Held 1", &s, ExportFormat::WebM, None, 3);
        assert!(matches!(odd, Err(pixel_core::Error::OddVideoScale)));
        // Der eigentliche Export braucht ffmpeg und läuft nur, wo es das gibt.
        match ws.export("Spiel", "Held 1", &s, ExportFormat::WebM, None, 4) {
            Ok(webm) => {
                assert_eq!(webm.files, [dir.join("Held_1@4x.webm")]);
                let bytes = std::fs::read(&webm.files[0]).unwrap();
                assert!(bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]), "WebM-Kopf");
            }
            Err(pixel_core::Error::FfmpegMissing) => {
                assert!(pixel_core::video::find_ffmpeg().is_none());
                assert!(!dir.join("Held_1@4x.webm").exists());
            }
            Err(other) => panic!("{other}"),
        }
    }

    #[test]
    fn a_relative_home_becomes_absolute() {
        let name = format!("pixelart-rel-{}", uuid::Uuid::new_v4());
        let ws = Workspace::open(std::path::Path::new("target").join(&name)).unwrap();
        assert!(ws.home().is_absolute());
        assert!(ws
            .home()
            .ends_with(std::path::Path::new("target").join(&name)));
        std::fs::remove_dir(ws.home()).unwrap();
    }

    #[test]
    fn a_project_lock_keeps_others_out_until_it_is_dropped() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let ws = temp();
        ws.create("Spiel").unwrap();
        let lock = ws.lock("Spiel").unwrap();
        // Ein anderes Projekt ist davon nicht betroffen.
        ws.create("Anderes").unwrap();
        drop(ws.lock("Anderes").unwrap());

        let got_it = Arc::new(AtomicBool::new(false));
        let waiter = {
            let (ws, got_it) = (ws.clone(), got_it.clone());
            std::thread::spawn(move || {
                // Gleiche Datei unter Windows, also dieselbe Sperre.
                let _lock = ws.lock(" SPIEL ").unwrap();
                got_it.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(!got_it.load(Ordering::SeqCst), "die Sperre hält nicht");
        drop(lock);
        waiter.join().unwrap();
        assert!(got_it.load(Ordering::SeqCst));
        assert!(ws.lock("../x").is_err());
    }
}
