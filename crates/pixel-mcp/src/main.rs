//! `pixelart-mcp`: MCP-Server für PixelART MCP.
//!
//! Der Server spricht MCP über stdin/stdout und wird von Claude Desktop,
//! Claude Code oder einem anderen MCP-Client gestartet.
//!
//! Arbeitsordner (dort liegen Projekte und Exporte), in dieser Reihenfolge:
//! 1. `--home <ordner>`
//! 2. Umgebungsvariable `PIXELART_HOME`
//! 3. `Dokumente/PixelArt Studio`

mod server;
mod studio;

use rmcp::ServiceExt;
use std::path::PathBuf;

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
                    "pixelart-mcp {}\n\nMCP server for PixelART MCP (speaks MCP over stdio).\n\nOptions:\n  --home <folder>  Workspace for projects and exports\n                   (default: $PIXELART_HOME or Documents/PixelArt Studio)\n  --version        Print version",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument '{other}', try --help")),
        }
    }
    if let Some(dir) = std::env::var_os("PIXELART_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .map(|d| d.join("PixelArt Studio"))
        .ok_or("cannot find a documents folder, pass --home <folder>".into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stdout gehört dem Protokoll; Meldungen für Menschen gehen nach stderr.
    let studio = studio::Studio::new(home_dir()?)?;
    eprintln!(
        "pixelart-mcp {}: workspace {}",
        env!("CARGO_PKG_VERSION"),
        studio.home().display()
    );
    let service = server::PixelServer::new(studio)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
