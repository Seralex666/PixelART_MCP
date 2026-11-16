# PixelART MCP

Ein eigenständiges Pixel-Art-Studio in Rust, das du selbst bedienen kannst und das
Claude (oder eine andere KI) über MCP steuern kann.

## Ziele

- **Echte Pixel:** Jedes Bild ist ein Raster aus einzelnen Pixeln, bis 1024×1024. Keine Glättung, keine Skalierungstricks.
- **Farbtiefe:** 8 Bit (Palette mit bis zu 256 Farben), 16 Bit (RGB565) oder 32 Bit (RGBA).
- **Konstanz:** Figuren sind feste Assets mit gesperrter Palette. Animationen (Idle, Walk, Run, Jump, Attack, Hurt, Death) werden aus der Figur abgeleitet und gegen sie geprüft.
- **Export:** PNG (auch mit Palette), JPG, Spritesheets mit JSON-Metadaten, animierte GIFs und WebM-Videos.
- **Import:** Vorhandene PNG-, JPG- und GIF-Dateien werden zu Bildern, Figuren oder Animationen, auch aus Spritesheets.
- **MCP:** Claude zeichnet in denselben Projektdateien, die du im Editor öffnest.

## Stand

| Schritt | Inhalt | Status |
|---|---|---|
| 1 | `pixel-core`: Datenmodell, Zeichenwerkzeuge, Projektformat, PNG/JPG/Sheet-Export | fertig |
| 2 | `pixel-mcp`: MCP-Server mit 17 Werkzeugen für Claude und andere MCP-Clients | fertig |
| 3 | `pixel-editor`: Editor mit Leinwand, Werkzeugen, Ebenen, Timeline, Export und Live-Abgleich mit Claude | fertig |
| 4 | GIF- und WebM-Export, indizierte PNGs | fertig |
| 5 | Import von PNG, JPG, GIF und Spritesheets im Editor und über MCP | fertig |

## Aufbau

```
crates/
  pixel-core/   Gemeinsamer Kern für Editor und MCP-Server
  pixel-mcp/    MCP-Server, als Bibliothek im Editor eingebaut und als eigenes Programm `pixelart-mcp`
  pixel-editor/ Editor (Programm `pixelart-studio`), mit `--mcp` zugleich der MCP-Server
docs/
  MCP-EINRICHTUNG.md   So verbindest du Claude mit dem Studio
```

Wichtige Typen in `pixel-core`:

- `Sprite`: Pixelbild mit Ebenen und Frames, erzwingt die Farbtiefe beim Schreiben.
- `Character`: Figur mit Grundpose, gesperrter Palette und Ankerpunkt.
- `Animation`: gehört zu einer Figur, jeder Frame startet als Kopie der Grundpose.
- `Project`: Figuren, Animationen, freie Bilder und Paletten in einer `.pxproj`-Datei. `validate()` meldet jede Abweichung einer Animation von ihrer Figur.
- `CharacterBundle`: einzelne Figur als `.pxchar`-Datei, um sie unverändert in andere Projekte zu übernehmen.

## Der Editor

`pixelart-studio` ist das Programm zum Selberzeichnen:

- Leinwand mit pixelgenauer Vergrößerung, Raster und Zwiebelhaut (vorheriger Frame scheint durch)
- Stift, Radierer, Füllen, Pipette, Linie, Rechteck, Ellipse, Verschieben
- Palette, Ebenen, Timeline mit Vorschaubildern und Animationsvorschau
- Neue Figuren, Animationen und Bilder aus Vorlagen; Export als PNG, JPG, Spritesheet, GIF oder WebM
- Bilder importieren (Strg+I oder Datei ins Fenster ziehen), siehe unten
- Gespeichert wird nach jeder Änderung von selbst, Rückgängig mit Strg+Z

Editor und MCP-Server arbeiten im selben Arbeitsordner. Zeichnet Claude, lädt der Editor das Projekt
nach kurzer Zeit neu; „Rückgängig“ nimmt auch Claudes letzte Änderung am geöffneten Bild zurück.

Der Arbeitsordner ist, in dieser Reihenfolge: `--home <Ordner>`, die Umgebungsvariable `PIXELART_HOME`,
ein Ordner `Projekte` neben dem Programm, sonst `Dokumente/PixelArt Studio`.

## Bilder importieren

Mit Strg+I, über Datei → Bild importieren oder indem du eine Datei ins Fenster ziehst. Claude benutzt
dafür das Werkzeug `import_image`. Gelesen werden PNG, JPG und GIF.

- **Als Bild:** ein freies Bild, zum Beispiel ein Tile.
- **Als neue Figur:** Die Datei legt Größe und Palette fest. Hat sie mehrere Frames, wird der erste die
  Grundpose, und auf Wunsch werden alle Frames zusätzlich eine Animation.
- **Als Animation einer Figur:** Die Frames müssen so groß sein wie die Figur. Farben, die die Figur noch
  nicht hat, kommen in ihre Palette oder werden an die vorhandenen angepasst.

Vergrößerte Pixel-Art (jeder Pixel ein Block aus 4×4 oder 8×8 Bildpunkten) wird erkannt und auf ihre
echte Größe gebracht. Spritesheets teilst du nach Spalten und Zeilen oder nach Framegröße; bei einer
Animation reicht die Größe der Figur. Leere Frames am Ende entfallen. Ein animiertes GIF bringt seine
Frames und deren Dauer mit. Hat eine Datei mehr Farben, als Farbtiefe oder Palette fassen, werden
ähnliche Farben zusammengelegt; das Ergebnis nennt, wie viele sich geändert haben.

## Exportformate

| Format | Was herauskommt | Pixelgenau |
|---|---|---|
| PNG | Je Frame eine Datei, Vollfarbe mit Transparenz | ja |
| PNG mit Palette | Dieselben Pixel mit Farbtabelle (PNG-8), kleinere Datei, höchstens 256 Farben | ja |
| JPG | Je Frame eine Datei, ohne Transparenz | nein |
| Spritesheet | Alle Frames in einem PNG, dazu JSON mit Positionen und Frame-Dauern | ja |
| GIF | Animation mit den Frame-Dauern der Timeline, Schleife wie eingestellt | ja, bis 255 Farben |
| WebM | Video (VP9) mit Transparenz | fast, Farben können um ein bis zwei Stufen abweichen |

GIF kennt Transparenz nur als an oder aus: Pixel unter halber Deckkraft werden durchsichtig, alle anderen
deckend. Hat eine Animation mehr als 255 Farben, werden sie auf ähnliche Farben reduziert, und der Export
sagt das dazu.

WebM schreibt das kostenlose Programm [ffmpeg](https://ffmpeg.org). Ist es nicht installiert, funktioniert
alles andere weiter. Unter Windows installiert es der Editor auf Knopfdruck: im Export-Dialog „WebM“ wählen
und „ffmpeg jetzt installieren“ anklicken. Dahinter steckt `winget install Gyan.FFmpeg`; das geht auch von
Hand, oder du legst `ffmpeg.exe` neben das Programm. Gesucht wird in dieser Reihenfolge: Umgebungsvariable
`PIXELART_FFMPEG`, Ordner des Programms, Suchpfad des Systems, Ablageort von winget. WebM braucht eine gerade Vergrößerung
(2×, 4×, …), damit die Pixelkanten scharf bleiben.

## Mit Claude verbinden

Im Editor unter **Hilfe → Mit Claude verbinden…** stehen alle Angaben mit den Pfaden deines Rechners,
dazu ein fertiger Prompt, mit dem Claude die Verbindung selbst einrichtet. Dieselbe Anleitung zum
Nachlesen: [docs/MCP-EINRICHTUNG.md](docs/MCP-EINRICHTUNG.md).

Das fertige Windows-Programm liegt unter [Releases](https://github.com/Seralex666/PixelART_MCP/releases).
Es ist eine einzige Datei, `pixelart-studio.exe`: per Doppelklick der Editor, mit `--mcp` der MCP-Server.
Gebaut wird sie auf GitHub über den Workflow „Release“ (von Hand starten oder einen Tag wie `v0.5.0` pushen).
Selbst bauen: `cargo build --release`.

## Ausprobieren

```sh
cargo test
cargo run -p pixel-editor
cargo run -p pixel-core --example demo -- demo-out
```

Das Beispiel legt eine Figur an, leitet eine Idle-Animation ab und schreibt
`demo.pxproj`, `held.png` sowie `held-idle.png` mit `held-idle.json`.
