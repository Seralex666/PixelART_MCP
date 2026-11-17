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
| 2 | `pixel-mcp`: MCP-Server mit 23 Werkzeugen für Claude und andere MCP-Clients | fertig |
| 3 | `pixel-editor`: Editor mit Leinwand, Werkzeugen, Ebenen, Timeline, Export und Live-Abgleich mit Claude | fertig |
| 4 | GIF- und WebM-Export, indizierte PNGs | fertig |
| 5 | Import von PNG, JPG, GIF und Spritesheets im Editor und über MCP | fertig |
| 6 | Pixel-Art-Techniken für Claude: Kontur, Licht, Raster, Verlauf, Körnung, Formel, Stempel, mehrere Frames je Aufruf | fertig |
| 7 | Prüf-Hinweise, Paletten aus Bildern, Farbreduktion, Projekt-Stil | fertig |
| 8 | Einstellungen (Speicherordner, Sprache Deutsch/Englisch), Werkzeug-Symbole | fertig |
| 9 | Automatische Konturen, Projekte löschen, aufgeräumte Seitenleisten, Programmsymbol | fertig |

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
- Stift, Radierer, Füllen, Pipette, Linie, Rechteck, Ellipse, Verschieben, jeweils mit eigenem Pixel-Symbol
- Oberfläche auf Deutsch oder Englisch (Datei › Einstellungen)
- Palette, Ebenen mit Vorschaubild, Timeline mit Vorschaubildern und Animationsvorschau
- Automatische Kontur (Kontur…), siehe unten
- Projekte löschen: Datei › Projekt löschen… oder Rechtsklick in der Projektliste
- Neue Figuren, Animationen und Bilder aus Vorlagen; Export als PNG, JPG, Spritesheet, GIF oder WebM
- Bilder importieren (Strg+I oder Datei ins Fenster ziehen), siehe unten
- Gespeichert wird nach jeder Änderung von selbst, Rückgängig mit Strg+Z

Editor und MCP-Server arbeiten im selben Arbeitsordner. Zeichnet Claude, lädt der Editor das Projekt
nach kurzer Zeit neu; „Rückgängig“ nimmt auch Claudes letzte Änderung am geöffneten Bild zurück.

Der Arbeitsordner ist, in dieser Reihenfolge: `--home <Ordner>`, die Umgebungsvariable `PIXELART_HOME`,
der Speicherordner aus **Datei › Einstellungen**, ein Ordner `Projekte` neben dem Programm, sonst
`Dokumente/PixelArt Studio`.

In den Einstellungen wählst du auch die Sprache. Sie gilt sofort für das ganze Programm; Claude bekommt
seine Werkzeuge immer auf Englisch. Gespeichert werden die Einstellungen in
`%APPDATA%\PixelArt Studio\settings.json` (unter Linux `~/.config/PixelArt Studio/settings.json`,
abweichend über die Umgebungsvariable `PIXELART_SETTINGS`).

## Zeichentechniken für Claude

Das Werkzeug `draw` kennt neben Pixeln, Linien, Flächen und dem Textraster auch Techniken, die sonst viele
einzelne Aufrufe oder ein Skript bräuchten. Alle arbeiten pixelgenau, ohne Glättung, und halten sich an
gesperrte Paletten.

| Op | Wirkung |
|---|---|
| `outline` | Kontur um alles, was auf der Ebene gezeichnet ist |
| `shade` | Licht und Schatten aus einer Richtung, mit einer Farbreihe von dunkel nach hell |
| `gradient` | Verlauf in harten Stufen, gerade oder rund, auf Wunsch gerastert |
| `dither` | Rasterübergang zwischen zwei Farben (Bayer-Muster oder Schachbrett) |
| `noise` | Körnung mit festem Startwert, also jedes Mal dasselbe Bild |
| `shader` | Formel je Pixel mit `x`, `y`, `frame`, `dist()`, `noise()`, `rand()`: für Glut, Licht und Muster |
| `stamp` | Ein Stück kopieren und einfügen, auch gespiegelt, gedreht, umgefärbt oder aus einem anderen Asset |

Die Flächen-Ops nehmen einen Ausschnitt (`x`, `y`, `w`, `h`) und eine Maske (`mask`: alles, nur Gezeichnetes,
nur Leeres oder nur eine bestimmte Farbe). Die Formel von `shader` kann nur rechnen: Es gibt keine Schleifen,
keinen Zugriff auf Dateien und keinen fremden Code, und ihre Länge ist begrenzt.

Ein Aufruf kann mehrere Frames auf einmal zeichnen (`frame`: Zahl, Liste oder `"all"`). `per_frame` legt fest,
was sich von Frame zu Frame ändert: andere Farben oder zusätzliche Ops wie eine Verschiebung. `undo` nimmt die
letzten Zeichen-Aufrufe zurück, solange das Bild inzwischen nicht anders geändert wurde. `view` zeigt auf Wunsch
ein Pixelraster mit Koordinaten, einen Ausschnitt oder die Kachel 3×3 nebeneinander.

## Automatische Kontur

Über **Kontur…** (unter den Ebenen, im Menü Bearbeiten oder per Rechtsklick auf ein Asset) bekommt ein Bild,
eine Animation oder eine Figur samt allen Animationen eine Kontur auf einer eigenen Ebene ganz oben. Sie entsteht
aus allen anderen sichtbaren Ebenen und wird nach jeder Änderung in allen Frames neu gezeichnet, egal ob du
zeichnest oder Claude. Claude benutzt dafür `set_outline`. Die Kontur ist freiwillig; ohne sie ändert sich nichts.

- **Selektiv** (jeder Konturpixel ist eine dunklere Stufe der Farbe daneben, auf der Lichtseite heller) oder
  **eine Farbe**. Bei Figuren kommen die Farben immer aus ihrer Palette.
- **Außen** um die Figur oder **innen** auf ihrem Rand, dann wird sie nicht größer; Dicke 1 bis 4 Pixel.
- **Über Eck** schließt diagonale Lücken, **Innenlinien** trennen Ebenen, etwa einen Arm vor dem Körper.
- **Festschreiben** macht aus der Kontur eine normale Ebene zum Nachbessern, **Entfernen** löscht sie.
  Neue Animationen einer Figur übernehmen ihre Kontur.

## Stil, Paletten und Prüf-Hinweise

- **Projekt-Stil** (`set_style`): Palette, Lichtrichtung, Umrissfarbe und Notizen für ein ganzes Projekt.
  Neue Bilder und Figuren starten mit der Palette, `shade` und `outline` nehmen Licht und Umrissfarbe von
  dort, wenn ein Aufruf sie nicht nennt. Alte Projekte ohne Stil laden wie bisher.
- **Paletten aus Bildern** (`palette_from_image`): liest die Hauptfarben einer Bilddatei oder eines Assets,
  von dunkel nach hell. Ähnliche Farben fallen zusammen, es bleibt jeweils die häufigste, also keine Mischfarbe.
- **Farben reduzieren** (`reduce_colors`): bringt ein Bild oder eine Figur samt Animationen auf weniger Farben,
  nach derselben Regel. Seltene, deutlich andere Farben wie Glanzlichter bleiben erhalten.
- **Prüf-Hinweise:** `get_project` und `view` nennen typische Versehen mit Koordinaten: einzelne verirrte
  Pixel, Lücken im Umriss, Farben mit nur einem Pixel und Kacheln, deren Ränder nicht zusammenpassen. Es sind
  Hinweise, keine Fehler; ein Sternenhimmel aus vielen Einzelpixeln zählt zum Beispiel nicht.

## Bilder importieren

Mit Strg+I, über Datei › Bild importieren oder indem du eine Datei ins Fenster ziehst. Claude benutzt
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

Im Editor unter **Hilfe › Mit Claude verbinden…** stehen alle Angaben mit den Pfaden deines Rechners,
dazu ein fertiger Prompt, mit dem Claude die Verbindung selbst einrichtet. Dieselbe Anleitung zum
Nachlesen: [docs/MCP-EINRICHTUNG.md](docs/MCP-EINRICHTUNG.md).

Das fertige Windows-Programm liegt unter [Releases](https://github.com/Seralex666/PixelART_MCP/releases).
Es ist eine einzige Datei, `pixelart-studio.exe`: per Doppelklick der Editor, mit `--mcp` der MCP-Server.
Gebaut wird sie auf GitHub über den Workflow „Release“ (von Hand starten oder einen Tag wie `v0.6.0` pushen).
Selbst bauen: `cargo build --release`.

## Ausprobieren

```sh
cargo test
cargo run -p pixel-editor
cargo run -p pixel-core --example demo -- demo-out
```

Das Beispiel legt eine Figur an, leitet eine Idle-Animation ab und schreibt
`demo.pxproj`, `held.png` sowie `held-idle.png` mit `held-idle.json`.
