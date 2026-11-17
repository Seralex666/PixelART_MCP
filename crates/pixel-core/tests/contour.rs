//! Automatische Kontur.

use pixel_core::contour::AutoOutline;
use pixel_core::draw::{self, Target};
use pixel_core::*;

fn c(hex: &str) -> Rgba {
    hex.parse().unwrap()
}

const BLUE: &str = "#41a6f6";

/// Ein 4×4-Quadrat in der Mitte eines 10×10-Bildes, auf Ebene 0.
fn square() -> Sprite {
    let mut s = Sprite::new(10, 10, ColorDepth::Rgba32, Palette::default()).unwrap();
    draw::rect(&mut s, Target::new(0, 0), 3, 3, 4, 4, c(BLUE), true).unwrap();
    s
}

fn layer_pixels(s: &Sprite, frame: usize, layer: usize) -> Vec<Rgba> {
    s.layer(frame, layer).unwrap().pixels().to_vec()
}

fn count(px: &[Rgba]) -> usize {
    px.iter().filter(|c| !c.is_transparent()).count()
}

#[test]
fn outside_outline_goes_on_its_own_layer_around_the_shape() {
    let mut s = square();
    let mut spec = AutoOutline::new("Kontur");
    spec.color = Some(c("#000000"));
    let n = s.set_auto_outline(spec).unwrap();
    // 4 Seiten × 4 Pixel, ohne Ecken.
    assert_eq!(n, 16);
    assert_eq!(s.layer_names(), vec!["Ebene 1", "Kontur"]);
    let px = layer_pixels(&s, 0, 1);
    assert_eq!(px[3 * 10 + 2], c("#000000"));
    assert!(
        px[2 * 10 + 2].is_transparent(),
        "no corner without `corners`"
    );
    // Das Original bleibt unberührt.
    assert_eq!(count(&layer_pixels(&s, 0, 0)), 16);
}

#[test]
fn corners_thickness_and_inside() {
    let mut s = square();
    let mut spec = AutoOutline::new("Kontur");
    spec.color = Some(c("#000000"));
    spec.corners = true;
    assert_eq!(s.set_auto_outline(spec.clone()).unwrap(), 20);
    spec.thickness = 2;
    // 8×8 Rahmen um das 4×4-Quadrat, also 64 − 16.
    assert_eq!(s.set_auto_outline(spec.clone()).unwrap(), 48);
    spec.thickness = 1;
    spec.corners = false;
    spec.inside = true;
    // Innen: der Rand des Quadrats selbst, 16 − 4 Pixel.
    assert_eq!(s.set_auto_outline(spec).unwrap(), 12);
    let px = layer_pixels(&s, 0, 1);
    assert!(!px[3 * 10 + 3].is_transparent());
    assert!(px[4 * 10 + 4].is_transparent());
    // Setzen derselben Ebene legt keine zweite an.
    assert_eq!(s.layer_count(), 2);
}

#[test]
fn selective_outline_is_a_darker_shade_and_lighter_towards_the_light() {
    let mut s = square();
    let mut spec = AutoOutline::new("Kontur");
    spec.light = Some((-1, -1));
    s.set_auto_outline(spec).unwrap();
    let px = layer_pixels(&s, 0, 1);
    let lum = |c: Rgba| pixel_core::quant::luminance(c);
    let lit = px[3 * 10 + 2]; // links, zum Licht
    let dark = px[3 * 10 + 7]; // rechts, vom Licht weg
    assert!(lum(dark) < lum(lit), "{dark} should be darker than {lit}");
    assert!(lum(lit) < lum(c(BLUE)));
}

#[test]
fn locked_palettes_only_get_palette_colors() {
    let pal = Palette::new("p", vec![c("#000000"), c("#29366f"), c(BLUE), c("#ffffff")]);
    let mut ch = Character::new("Held", 10, 10, ColorDepth::Indexed8, pal).unwrap();
    draw::rect(&mut ch.base, Target::new(0, 0), 3, 3, 4, 4, c(BLUE), true).unwrap();
    ch.base
        .set_auto_outline(AutoOutline::new("Kontur"))
        .unwrap();
    let px = layer_pixels(&ch.base, 0, 1);
    assert!(px.iter().all(|&p| ch.palette().contains(p)));
    assert_eq!(
        px[3 * 10 + 2],
        c("#29366f"),
        "the dark blue is the closest darker color"
    );
}

#[test]
fn the_outline_follows_every_change_in_every_frame() {
    let mut s = square();
    s.duplicate_frame(0).unwrap();
    s.set_auto_outline(AutoOutline::new("Kontur")).unwrap();
    draw::pixels(&mut s, Target::new(1, 0), &[(0, 0)], c(BLUE)).unwrap();
    assert!(s.refresh_outline().unwrap());
    assert_eq!(count(&layer_pixels(&s, 0, 1)), 16);
    assert_eq!(count(&layer_pixels(&s, 1, 1)), 16 + 2);
    assert!(
        !s.refresh_outline().unwrap(),
        "nothing changed the second time"
    );
}

#[test]
fn inner_lines_separate_a_front_layer_from_the_one_behind() {
    let mut s = square();
    let arm = s.add_layer("Arm");
    draw::rect(&mut s, Target::new(0, arm), 5, 3, 1, 4, c("#ffcd75"), true).unwrap();
    let mut spec = AutoOutline::new("Kontur");
    spec.color = Some(c("#000000"));
    let without = s.set_auto_outline(spec.clone()).unwrap();
    spec.inner = true;
    let with = s.set_auto_outline(spec).unwrap();
    // Links und rechts vom Arm je eine Linie aus 4 Pixeln auf dem Körper.
    assert_eq!(with, without + 8);
}

#[test]
fn stopping_keeps_or_removes_the_layer_and_renaming_keeps_the_automatic() {
    let mut s = square();
    s.set_auto_outline(AutoOutline::new("Kontur")).unwrap();
    s.rename_layer(1, "Linie").unwrap();
    assert_eq!(s.auto_outline().unwrap().layer, "Linie");
    s.stop_outline(true).unwrap();
    assert!(s.auto_outline().is_none());
    assert_eq!(s.layer_count(), 2);
    s.set_auto_outline(AutoOutline::new("Linie")).unwrap();
    s.stop_outline(false).unwrap();
    assert_eq!(s.layer_count(), 1);
    // Wird die Ebene von Hand gelöscht, endet die Automatik beim nächsten Abgleich.
    s.set_auto_outline(AutoOutline::new("Kontur")).unwrap();
    s.remove_layer(1).unwrap();
    s.refresh_outline().unwrap();
    assert!(s.auto_outline().is_none());
}

#[test]
fn outline_survives_saving_and_animations_inherit_it() {
    let pal = Palette::new("p", vec![c("#000000"), c("#29366f"), c(BLUE)]);
    let mut p = Project::new("T");
    let ch = Character::new("Held", 10, 10, ColorDepth::Indexed8, pal).unwrap();
    let id = p.add_character(ch);
    draw::rect(
        &mut p.character_mut(id).unwrap().base,
        Target::new(0, 0),
        3,
        3,
        4,
        4,
        c(BLUE),
        true,
    )
    .unwrap();
    let walk = p.add_animation(id, AnimationKind::Walk, Some(2)).unwrap();
    let n = p
        .set_character_outline(id, Some(AutoOutline::new("Kontur")), false, true)
        .unwrap();
    assert_eq!(n, 1);
    let json = p.to_json().unwrap();
    let mut back = Project::from_json(&json).unwrap();
    assert_eq!(back, p);
    draw::pixels(
        &mut back.animation_mut(walk).unwrap().sprite,
        Target::new(1, 0),
        &[(0, 0)],
        c(BLUE),
    )
    .unwrap();
    assert!(back.refresh_outlines().unwrap());
    assert!(back.validate().is_empty(), "{:?}", back.validate());
    // Eine neue Animation übernimmt die Kontur von der Grundpose.
    let jump = back
        .add_animation(id, AnimationKind::Jump, Some(1))
        .unwrap();
    assert!(back
        .animation(jump)
        .unwrap()
        .sprite
        .auto_outline()
        .is_some());
}
