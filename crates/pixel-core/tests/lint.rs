//! Prüf-Hinweise, Farbreduktion und Projekt-Stil.

use pixel_core::draw::{self, Target};
use pixel_core::lint::{self, Checks, Finding};
use pixel_core::*;

const T: Target = Target::new(0, 0);

fn c(hex: &str) -> Rgba {
    hex.parse().unwrap()
}

fn sprite(w: u32, h: u32) -> Sprite {
    Sprite::new(w, h, ColorDepth::Rgba32, Palette::default()).unwrap()
}

/// Ein gefülltes Rechteck mit Umriss: die Figur, an der die Prüfungen ansetzen.
fn outlined_box() -> Sprite {
    let mut s = sprite(16, 16);
    draw::rect(&mut s, T, 3, 3, 10, 10, c("#1a1c2c"), true).unwrap();
    draw::rect(&mut s, T, 4, 4, 8, 8, c("#41a6f6"), true).unwrap();
    s
}

#[test]
fn a_clean_sprite_gets_no_hints() {
    assert_eq!(lint::check(&outlined_box(), Checks::default()), vec![]);
    assert_eq!(lint::check(&sprite(16, 16), Checks::default()), vec![]);
}

#[test]
fn stray_pixels_are_found_with_their_place() {
    let mut s = outlined_box();
    draw::pixels(&mut s, T, &[(0, 15)], c("#1a1c2c")).unwrap();
    let hints = lint::check(&s, Checks::default());
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].frames, vec![0]);
    assert_eq!(
        hints[0].finding,
        Finding::StrayPixels {
            count: 1,
            at: vec![(0, 15)]
        }
    );
}

#[test]
fn many_single_pixels_count_as_intended() {
    // Ein Sternenhimmel: lauter einzelne Pixel, das ist kein Versehen.
    let mut s = sprite(32, 32);
    let stars: Vec<(i64, i64)> = (0..20).map(|i| (i % 5 * 6 + 1, i / 5 * 6 + 1)).collect();
    draw::pixels(&mut s, T, &stars, c("#f4f4f4")).unwrap();
    assert_eq!(lint::check(&s, Checks::default()), vec![]);
}

#[test]
fn a_gap_in_the_outline_is_found() {
    let mut s = outlined_box();
    draw::pixels(&mut s, T, &[(3, 7), (12, 12)], c("#41a6f6")).unwrap();
    let hints = lint::check(&s, Checks::default());
    assert_eq!(
        hints[0].finding,
        Finding::OutlineGaps {
            color: c("#1a1c2c"),
            count: 2,
            at: vec![(3, 7), (12, 12)]
        }
    );
    // Eine Form ohne einheitlichen Rand hat keinen Umriss, also auch keine Lücke.
    let mut plain = sprite(16, 16);
    draw::rect(&mut plain, T, 3, 3, 10, 5, c("#41a6f6"), true).unwrap();
    draw::rect(&mut plain, T, 3, 8, 10, 5, c("#b13e53"), true).unwrap();
    assert_eq!(lint::check(&plain, Checks::default()), vec![]);
}

#[test]
fn the_project_outline_color_decides_what_a_gap_is() {
    // Zwei Drittel des Rands haben die Umrissfarbe des Projekts: ohne Vorgabe
    // wäre das zu wenig für einen Umriss, mit Vorgabe ist der Rest eine Lücke.
    let mut s = outlined_box();
    draw::rect(&mut s, T, 3, 12, 10, 1, c("#b13e53"), true).unwrap();
    draw::rect(&mut s, T, 12, 8, 1, 4, c("#b13e53"), true).unwrap();
    assert_eq!(lint::check(&s, Checks::default()), vec![]);
    let hints = lint::check(
        &s,
        Checks {
            outline: Some(c("#1a1c2c")),
            seams: false,
        },
    );
    assert!(matches!(
        hints[0].finding,
        Finding::OutlineGaps { count: 14, .. }
    ));
}

#[test]
fn a_color_used_by_one_pixel_is_found() {
    let mut s = outlined_box();
    draw::pixels(&mut s, T, &[(6, 6)], c("#41a6f5")).unwrap();
    let hints = lint::check(&s, Checks::default());
    assert_eq!(
        hints[0].finding,
        Finding::SingleUseColors {
            colors: vec![(c("#41a6f5"), (6, 6))]
        }
    );
}

#[test]
fn tiles_are_checked_for_seams_only_when_asked() {
    // Links dunkel, rechts hell: nebeneinander gelegt entsteht eine harte Kante.
    let mut s = sprite(16, 16);
    for x in 0..16 {
        let v = (x * 16) as u8;
        draw::rect(&mut s, T, x, 0, 1, 16, Rgba::rgb(v, v, v), true).unwrap();
    }
    assert_eq!(lint::check(&s, Checks::default()), vec![]);
    let seams = Checks {
        outline: None,
        seams: true,
    };
    let hints = lint::check(&s, seams);
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].finding, Finding::Seam { horizontal: true });

    // Ein Schachbrett setzt sich über den Rand genauso fort wie im Bild.
    let mut even = sprite(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            let col = if (x / 4 + y / 4) % 2 == 0 {
                c("#566c86")
            } else {
                c("#333c57")
            };
            draw::pixels(&mut even, T, &[(x, y)], col).unwrap();
        }
    }
    assert_eq!(lint::check(&even, seams), vec![]);
}

#[test]
fn equal_findings_in_several_frames_become_one_hint() {
    let mut s = outlined_box();
    draw::pixels(&mut s, T, &[(0, 15)], c("#1a1c2c")).unwrap();
    s.duplicate_frame(0).unwrap();
    s.duplicate_frame(0).unwrap();
    draw::pixels(&mut s, Target::new(2, 0), &[(0, 15)], Rgba::TRANSPARENT).unwrap();
    let hints = lint::check(&s, Checks::default());
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].frames, vec![0, 1]);
    assert_eq!(lint::check_frames(&s, &[2], Checks::default()), vec![]);
}

#[test]
fn reduce_merges_similar_colors_and_keeps_the_rest() {
    let mut s = sprite(8, 1);
    let colors = [
        "#100000", "#120000", "#140000", "#00ff00", "#00fd00", "#0000ff", "#0000fd", "#0000fb",
    ];
    for (x, hex) in colors.iter().enumerate() {
        draw::pixels(&mut s, T, &[(x as i64, 0)], c(hex)).unwrap();
    }
    let report = quant::reduce(&mut [&mut s], 3).unwrap();
    assert_eq!((report.before, report.after), (8, 3));
    assert_eq!(s.used_colors().len(), 3);
    assert_eq!(s.palette.colors, report.colors);
    // Jede Gruppe ist eine der drei Grundfarben geblieben.
    let px = |x: i64| s.layer(0, 0).unwrap().get(x, 0).unwrap();
    assert_eq!(px(0), px(2));
    assert_eq!(px(3), px(4));
    assert_eq!(px(5), px(7));
    assert!(px(0).r > 0 && px(3).g > 250 && px(5).b > 250);

    // Reicht die Farbzahl schon, bleibt alles, wie es ist.
    let before = s.clone();
    let report = quant::reduce(&mut [&mut s], 16).unwrap();
    assert_eq!((report.before, report.after), (3, 3));
    assert_eq!(s, before);
}

#[test]
fn reduce_keeps_a_character_and_its_animations_on_one_palette() {
    let pal = Palette::new(
        "p",
        vec![c("#200000"), c("#240000"), c("#00ff00"), c("#0000ff")],
    );
    let mut p = Project::new("t");
    let mut ch = Character::new("Held", 4, 4, ColorDepth::Indexed8, pal).unwrap();
    draw::pixels(&mut ch.base, T, &[(0, 0)], c("#200000")).unwrap();
    draw::pixels(&mut ch.base, T, &[(1, 0)], c("#00ff00")).unwrap();
    let id = p.add_character(ch);
    let anim = p.add_animation(id, AnimationKind::Walk, Some(2)).unwrap();
    {
        let a = p.animation_mut(anim).unwrap();
        draw::pixels(&mut a.sprite, Target::new(1, 0), &[(2, 0)], c("#240000")).unwrap();
        draw::pixels(&mut a.sprite, Target::new(1, 0), &[(3, 0)], c("#0000ff")).unwrap();
    }
    let report = p.reduce_character_colors(id, 3).unwrap();
    assert_eq!((report.before, report.after), (4, 3));
    let ch = p.character(id).unwrap();
    assert!(ch.palette().locked);
    assert_eq!(ch.palette().len(), 3);
    assert_eq!(p.animation(anim).unwrap().sprite.palette, *ch.palette());
    assert!(p.validate().is_empty());
}

#[test]
fn palette_of_sorts_dark_to_light_and_ignores_transparency() {
    let pixels = vec![
        c("#ffffff"),
        c("#000000"),
        Rgba::TRANSPARENT,
        c("#808080"),
        c("#ffffff"),
        Rgba::new(255, 0, 0, 10),
    ];
    let (colors, found) = quant::palette_of(pixels.clone(), 16);
    assert_eq!(found, 3);
    assert_eq!(colors, vec![c("#000000"), c("#808080"), c("#ffffff")]);
    let (colors, _) = quant::palette_of(pixels, 2);
    assert_eq!(colors.len(), 2);
}

#[test]
fn the_style_survives_saving_and_old_files_load_without_one() {
    let mut p = Project::new("t");
    assert!(!p.to_json().unwrap().contains("style"));
    p.style = Style {
        palette: vec![c("#1a1c2c"), c("#f4f4f4")],
        light_dir: Some("top-left".into()),
        outline: Some(c("#1a1c2c")),
        notes: "2 px Umriss".into(),
        preset: Some("Manga".into()),
    };
    let back = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert_eq!(back.style, p.style);
    // Eine Datei aus Version 0.5 kennt das Feld nicht.
    let old = r#"{"format_version":1,"id":"6a0b6a0e-5f0a-4c62-9a55-0d2f0e3b1c11","name":"alt"}"#;
    assert!(Project::from_json(old).unwrap().style.is_empty());
}
