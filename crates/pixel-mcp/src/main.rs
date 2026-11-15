//! `pixelart-mcp`: der MCP-Server als eigenes Programm ohne Editor.
//!
//! Dasselbe leistet `pixelart-studio --mcp`. Dieses Programm ist für Rechner
//! gedacht, auf denen der Editor nicht laufen kann oder soll.
//!
//! Arbeitsordner (dort liegen Projekte und Exporte), in dieser Reihenfolge:
//! 1. `--home <ordner>`
//! 2. Umgebungsvariable `PIXELART_HOME`
//! 3. ein Ordner `Projekte` neben dem Programm
//! 4. `Dokumente/PixelArt Studio`

use std::path::PathBuf;
use std::process::ExitCode;

fn home_dir() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "--home" => {
                return args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--home needs a folder".into())
            }
            "--version" | "-V" => {
                println!("pixelart-mcp {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "pixelart-mcp {}\n\nMCP server for PixelART MCP (speaks MCP over stdio).\n\nOptions:\n  --home <folder>  Workspace for projects and exports\n                   (default: $PIXELART_HOME, a folder 'Projekte' next to the\n                   program, or Documents/PixelArt Studio)\n  --version        Print version",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument '{other}', try --help")),
        }
    }
    pixel_core::Workspace::default_home()
        .ok_or("cannot find a documents folder, pass --home <folder>".into())
}

fn main() -> ExitCode {
    match home_dir().and_then(pixel_mcp::serve_stdio) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pixelart-mcp: {err}");
            ExitCode::FAILURE
        }
    }
}
