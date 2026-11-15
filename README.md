# PixelART MCP

Ein eigenständiges Pixel-Art-Studio in Rust, das du selbst bedienen kannst und das
Claude (oder eine andere KI) über MCP steuern kann.

## Ziele

- **Echte Pixel:** Jedes Bild ist ein Raster aus einzelnen Pixeln, bis 1024×1024. Keine Glättung, keine Skalierungstricks.
- **Farbtiefe:** 8 Bit (Palette mit bis zu 256 Farben), 16 Bit (RGB565) oder 32 Bit (RGBA).
- **Konstanz:** Figuren sind feste Assets mit gesperrter Palette. Animationen (Idle, Walk, Run, Jump, Attack, Hurt, Death) werden aus der Figur abgeleitet und gegen sie geprüft.
- **Export:** PNG, JPG, Spritesheets mit JSON-Metadaten, später GIF und WebM.
- **MCP:** Claude zeichnet in denselben Projektdateien, die du im Editor öffnest.

## Stand

| Schritt | Inhalt | Status |
|---|---|---|
| 1 | `pixel-core`: Datenmodell, Zeichenwerkzeuge, Projektformat, PNG/JPG/Sheet-Export | fertig |
| 2 | `pixel-mcp`: MCP-Server mit 16 Werkzeugen für Claude und andere MCP-Clients | fertig |
| 3 | Editor (egui) | offen |
| 4 | GIF- und WebM-Export, indizierte PNGs | offen |

## Aufbau

```
crates/
  pixel-core/   Gemeinsamer Kern für Editor und MCP-Server
  pixel-mcp/    MCP-Server (Programm `pixelart-mcp`)
docs/
  MCP-EINRICHTUNG.md   So verbindest du Claude mit dem Studio
```

Wichtige Typen in `pixel-core`:

- `Sprite`: Pixelbild mit Ebenen und Frames, erzwingt die Farbtiefe beim Schreiben.
- `Character`: Figur mit Grundpose, gesperrter Palette und Ankerpunkt.
- `Animation`: gehört zu einer Figur, jeder Frame startet als Kopie der Grundpose.
- `Project`: Figuren, Animationen, freie Bilder und Paletten in einer `.pxproj`-Datei. `validate()` meldet jede Abweichung einer Animation von ihrer Figur.
- `CharacterBundle`: einzelne Figur als `.pxchar`-Datei, um sie unverändert in andere Projekte zu übernehmen.

## Mit Claude verbinden

Siehe [docs/MCP-EINRICHTUNG.md](docs/MCP-EINRICHTUNG.md). Kurz: `pixelart-mcp.exe` in Claude Desktop
oder Claude Code als MCP-Server eintragen, danach kann Claude Projekte anlegen, Figuren zeichnen,
animieren und exportieren.

Die fertige Windows-exe liegt unter [Releases](https://github.com/Seralex666/PixelART_MCP/releases).
Ein Tag wie `v0.1.0` baut sie automatisch auf GitHub. Selbst bauen: `cargo build --release -p pixel-mcp`.

## Ausprobieren

```sh
cargo test
cargo run -p pixel-core --example demo -- demo-out
```

Das Beispiel legt eine Figur an, leitet eine Idle-Animation ab und schreibt
`demo.pxproj`, `held.png` sowie `held-idle.png` mit `held-idle.json`.
