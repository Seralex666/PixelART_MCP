//! Der MCP-Server von PixelART MCP als Bibliothek.
//!
//! Er spricht MCP über stdin/stdout und wird von Claude Desktop, Claude Code
//! oder einem anderen MCP-Client gestartet. Eingebaut ist er an zwei Stellen:
//! im Editor (`pixelart-studio --mcp`), damit eine einzige Datei genügt, und im
//! eigenständigen Programm `pixelart-mcp` für Rechner ohne Bildschirm.

mod server;
mod studio;

use rmcp::ServiceExt;
use std::path::PathBuf;

/// Name, mit dem sich der Server beim MCP-Client meldet.
pub const SERVER_NAME: &str = "pixelart-studio";

/// Startet den Server im Arbeitsordner `home` und bedient den MCP-Client,
/// bis der die Verbindung schließt.
pub fn serve_stdio(home: PathBuf) -> Result<(), String> {
    let studio = studio::Studio::new(home)?;
    // stdout gehört dem Protokoll; Meldungen für Menschen gehen nach stderr.
    eprintln!(
        "pixelart-mcp {}: workspace {}",
        env!("CARGO_PKG_VERSION"),
        studio.home().display()
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("cannot start the async runtime: {err}"))?;
    runtime.block_on(async {
        let service = server::PixelServer::new(studio)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|err| format!("cannot start the MCP server: {err}"))?;
        service
            .waiting()
            .await
            .map_err(|err| format!("the MCP server stopped with an error: {err}"))?;
        Ok(())
    })
}
