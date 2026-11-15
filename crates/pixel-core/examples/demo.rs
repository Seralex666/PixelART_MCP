//! Kleines Beispiel: legt eine Figur an, leitet eine Idle-Animation ab und
//! exportiert Projektdatei, Einzelbild und Spritesheet.
//!
//! Aufruf: `cargo run -p pixel-core --example demo -- <zielordner>`

use pixel_core::draw::{self, Target};
use pixel_core::*;
use std::path::PathBuf;

fn main() -> Result<()> {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "demo-out".into()));
    std::fs::create_dir_all(&out)?;
    let c = |hex: &str| hex.parse::<Rgba>().unwrap();

    // Figur mit drei Ebenen, damit sich Körperteile getrennt bewegen lassen.
    let mut hero = Character::new("Held", 16, 24, ColorDepth::Indexed8, Palette::default_16())?;
    hero.description = "Kleiner Abenteurer mit blauem Wams".into();
    let s = &mut hero.base;
    s.rename_layer(0, "Beine")?;
    let body = s.add_layer("Körper");
    let head = s.add_layer("Kopf");
    let (legs, body, head) = (
        Target::new(0, 0),
        Target::new(0, body),
        Target::new(0, head),
    );

    draw::rect(s, legs, 5, 17, 2, 6, c("#333c57"), true)?;
    draw::rect(s, legs, 9, 17, 2, 6, c("#333c57"), true)?;
    draw::rect(s, legs, 4, 22, 3, 2, c("#1a1c2c"), true)?;
    draw::rect(s, legs, 9, 22, 3, 2, c("#1a1c2c"), true)?;

    draw::rect(s, body, 4, 9, 8, 8, c("#3b5dc9"), true)?;
    draw::rect(s, body, 4, 15, 8, 1, c("#ef7d57"), true)?;
    draw::rect(s, body, 3, 10, 1, 5, c("#29366f"), true)?;
    draw::rect(s, body, 12, 10, 1, 5, c("#29366f"), true)?;
    draw::pixels(s, body, &[(3, 15), (12, 15)], c("#ffcd75"))?;

    draw::ellipse(s, head, 4, 1, 8, 8, c("#ffcd75"), true)?;
    draw::rect(s, head, 4, 1, 8, 3, c("#5d275d"), true)?;
    draw::pixels(
        s,
        head,
        &[(4, 4), (11, 4), (5, 0), (6, 0), (9, 0), (10, 0)],
        c("#5d275d"),
    )?;
    draw::pixels(s, head, &[(6, 5), (9, 5)], c("#1a1c2c"))?;
    draw::pixels(s, head, &[(7, 7), (8, 7)], c("#b13e53"))?;

    let mut project = Project::new("Demo");
    let hero_id = project.add_character(hero);

    // Idle: Der Kopf senkt sich in den mittleren Frames um 1 Pixel (Atmen).
    let idle = project.add_animation(hero_id, AnimationKind::Idle, None)?;
    let sprite = &mut project.animation_mut(idle)?.sprite;
    for frame in [1, 2] {
        draw::shift(sprite, Target::new(frame, head.layer), 0, 1)?;
    }

    assert!(
        project.validate().is_empty(),
        "Animation weicht von der Figur ab"
    );

    project.save(&out.join("demo.pxproj"))?;
    std::fs::write(
        out.join("held.png"),
        export::frame_png(&project.character(hero_id)?.base, 0, 8)?,
    )?;
    let meta = export::save_sheet(
        &project.animation(idle)?.sprite,
        &out.join("held-idle"),
        0,
        8,
    )?;
    println!(
        "Gespeichert in {}: {} Frames, Sheet {}×{}",
        out.display(),
        meta.frames.len(),
        meta.image_width,
        meta.image_height
    );
    Ok(())
}
