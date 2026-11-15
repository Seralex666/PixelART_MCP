//! `pixelart-studio`: der Editor von PixelART MCP und zugleich der MCP-Server.
//!
//! Ohne weitere Angabe öffnet sich der Editor. Mit `--mcp` läuft dasselbe
//! Programm als MCP-Server über stdin/stdout, so wie Claude es startet. Beide
//! arbeiten im selben Arbeitsordner, deshalb zeigt der Editor, was Claude
//! zeichnet. Arbeitsordner, in dieser Reihenfolge:
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
use std::process::ExitCode;

const USAGE: &str = "PixelArt Studio\n\nOhne Angabe startet der Editor.\n\n  --mcp              als MCP-Server über stdin/stdout laufen (so startet ihn Claude)\n  --home <Ordner>    Arbeitsordner für Projekte und Exporte\n  --project <Name>   dieses Projekt im Editor öffnen\n  --version          Version ausgeben";

/// Was das Programm tun soll.
#[derive(Debug, PartialEq)]
enum Mode {
    Editor {
        project: Option<String>,
    },
    /// MCP-Server statt Fenster.
    Mcp,
    /// Text ausgeben und beenden.
    Print(String),
}

#[derive(Debug, PartialEq)]
struct Args {
    mode: Mode,
    /// `None`: der Standard-Arbeitsordner.
    home: Option<PathBuf>,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut home = None;
    let mut project = None;
    let mut mcp = false;
    let mut args = args.into_iter();
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
            "--mcp" => mcp = true,
            "--version" | "-V" => {
                let text = format!("pixelart-studio {}", env!("CARGO_PKG_VERSION"));
                return Ok(Args {
                    mode: Mode::Print(text),
                    home,
                });
            }
            "--help" | "-h" => {
                return Ok(Args {
                    mode: Mode::Print(USAGE.to_string()),
                    home,
                })
            }
            other => {
                return Err(format!(
                    "Unbekanntes Argument „{other}“. Erlaubt sind --mcp, --home <Ordner>, --project <Name> und --version."
                ))
            }
        }
    }
    let mode = if mcp {
        Mode::Mcp
    } else {
        Mode::Editor { project }
    };
    Ok(Args { mode, home })
}

fn home_or_default(home: Option<PathBuf>) -> Result<PathBuf, String> {
    home.or_else(Workspace::default_home).ok_or_else(|| {
        "Es wurde kein Dokumente-Ordner gefunden. Starte mit --home <Ordner>.".into()
    })
}

/// Ein Windows-Programm ohne Konsolenfenster hat keine Standardausgabe, wenn
/// man es in der Eingabeaufforderung startet. Für `--version` und Fehler im
/// MCP-Modus hängt es sich deshalb an die Konsole des Aufrufers. Startet ein
/// MCP-Client das Programm, hat es schon Leitungen und braucht das nicht.
#[cfg(windows)]
fn attach_parent_console() {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    if std::io::stdout().as_raw_handle().is_null() {
        // SAFETY: Der Aufruf hat keine Voraussetzungen; scheitert er, weil es
        // keine Konsole gibt, bleibt alles wie zuvor.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

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

fn main() -> ExitCode {
    let args = parse_args(std::env::args().skip(1));
    let (home, project) = match args {
        Ok(Args {
            mode: Mode::Print(text),
            ..
        }) => {
            attach_parent_console();
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Ok(Args {
            mode: Mode::Mcp,
            home,
        }) => {
            attach_parent_console();
            return match home_or_default(home).and_then(pixel_mcp::serve_stdio) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("pixelart-studio --mcp: {err}");
                    ExitCode::FAILURE
                }
            };
        }
        Ok(Args {
            mode: Mode::Editor { project },
            home,
        }) => (home_or_default(home), project),
        Err(err) => (Err(err), None),
    };
    let start = home.and_then(|home| {
        let ws = Workspace::open(&home).map_err(|err| {
            format!(
                "Der Arbeitsordner {} lässt sich nicht anlegen: {err}",
                home.display()
            )
        })?;
        Ok((ws, project))
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PixelArt Studio")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([760.0, 480.0])
            .with_icon(icon()),
        ..Default::default()
    };
    let run = eframe::run_native(
        "PixelArt Studio",
        options,
        Box::new(move |_cc| {
            Ok(match start {
                Ok((ws, project)) => Box::new(app::App::new(ws, project)) as Box<dyn eframe::App>,
                Err(err) => Box::new(Failed(err)),
            })
        }),
    );
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("PixelArt Studio: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn no_arguments_open_the_editor() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.mode, Mode::Editor { project: None });
        assert_eq!(args.home, None);
    }

    #[test]
    fn mcp_mode_takes_the_same_home_as_the_editor() {
        for order in [["--mcp", "--home", "x"], ["--home", "x", "--mcp"]] {
            let args = parse(&order).unwrap();
            assert_eq!(args.mode, Mode::Mcp);
            assert_eq!(args.home, Some(PathBuf::from("x")));
        }
        let editor = parse(&["--home", "x", "--project", "Spiel"]).unwrap();
        assert_eq!(
            editor.mode,
            Mode::Editor {
                project: Some("Spiel".into())
            }
        );
    }

    #[test]
    fn version_and_mistakes_are_reported() {
        let Mode::Print(text) = parse(&["--version"]).unwrap().mode else {
            panic!("--version prints");
        };
        assert!(text.starts_with("pixelart-studio "));
        assert!(parse(&["--home"]).is_err());
        assert!(parse(&["--frobnicate"]).unwrap_err().contains("--mcp"));
    }
}
