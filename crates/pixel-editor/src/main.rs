//! `pixelart-studio`: der Editor von PixelART MCP.
//!
//! Er arbeitet im selben Arbeitsordner wie der MCP-Server und zeigt deshalb,
//! was Claude zeichnet. Arbeitsordner, in dieser Reihenfolge:
//! 1. `--home <ordner>`
//! 2. Umgebungsvariable `PIXELART_HOME`
//! 3. ein Ordner `Projekte` neben dem Programm
//! 4. `Dokumente/PixelArt Studio`
//!
//! `--project <name>` öffnet ein bestimmtes Projekt, sonst das zuletzt bearbeitete.

// Unter Windows kein zusätzliches Konsolenfenster öffnen.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas;
mod dialogs;
mod doc;
mod help;
mod tools;

use eframe::egui;
use pixel_core::Workspace;
use std::path::PathBuf;

/// Startoptionen: `--home <Ordner>` und `--project <Name>`.
struct Args {
    home: PathBuf,
    project: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut home = None;
    let mut project = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--home" => {
                home = Some(PathBuf::from(
                    args.next().ok_or("--home braucht einen Ordner")?,
                ));
            }
            "--project" => {
                project = Some(args.next().ok_or("--project braucht einen Projektnamen")?);
            }
            other => {
                return Err(format!(
                    "Unbekanntes Argument „{other}“. Erlaubt sind --home <Ordner> und --project <Name>."
                ))
            }
        }
    }
    let home = match home {
        Some(home) => home,
        None => Workspace::default_home()
            .ok_or("Es wurde kein Dokumente-Ordner gefunden. Starte mit --home <Ordner>.")?,
    };
    Ok(Args { home, project })
}

/// Fenstersymbol: ein kleines Pixel-Herz.
fn icon() -> egui::IconData {
    const ART: [&str; 8] = [
        ".##..##.", "########", "########", "########", ".######.", "..####..", "...##...",
        "........",
    ];
    const SCALE: usize = 4;
    let size = ART.len() * SCALE;
    let mut rgba = vec![0u8; size * size * 4];
    for (y, row) in ART.iter().enumerate() {
        for (x, ch) in row.bytes().enumerate() {
            if ch != b'#' {
                continue;
            }
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    let i = ((y * SCALE + dy) * size + x * SCALE + dx) * 4;
                    rgba[i..i + 4].copy_from_slice(&[230, 60, 90, 255]);
                }
            }
        }
    }
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

/// Zeigt einen Startfehler in einem Fenster, weil es unter Windows keine Konsole gibt.
struct Failed(String);

impl eframe::App for Failed {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("PixelArt Studio kann nicht starten");
            ui.add_space(8.0);
            ui.label(&self.0);
        });
    }
}

fn main() -> eframe::Result {
    let start = parse_args().and_then(|args| {
        let ws = Workspace::open(&args.home).map_err(|err| {
            format!(
                "Der Arbeitsordner {} lässt sich nicht anlegen: {err}",
                args.home.display()
            )
        })?;
        Ok((ws, args.project))
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PixelArt Studio")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([760.0, 480.0])
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "PixelArt Studio",
        options,
        Box::new(move |_cc| {
            Ok(match start {
                Ok((ws, project)) => Box::new(app::App::new(ws, project)) as Box<dyn eframe::App>,
                Err(err) => Box::new(Failed(err)),
            })
        }),
    )
}
