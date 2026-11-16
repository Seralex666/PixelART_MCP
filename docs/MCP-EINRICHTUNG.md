# MCP-Server einrichten

Editor und MCP-Server sind dieselbe Datei: `pixelart-studio.exe`. Per Doppelklick öffnet sie den Editor.
Mit der Angabe `--mcp` gestartet, öffnet sie kein Fenster, sondern arbeitet als MCP-Server für Claude.
Sie braucht keine Installation. Claude startet sie so von selbst, sobald sie eingetragen ist.

**Am einfachsten:** Im Editor unter **Hilfe → Mit Claude verbinden…** stehen
alle Angaben schon mit den Pfaden deines Rechners, jeweils mit einem Knopf zum Kopieren. Dort gibt es
auch einen fertigen Prompt, mit dem Claude die Verbindung selbst einrichtet. Diese Seite beschreibt
dasselbe zum Nachlesen.

In den Beispielen liegt die exe in `C:\Users\DEINNAME\Desktop\PixelArt Studio`. Ersetze `DEINNAME`
durch deinen Windows-Benutzernamen.

## Wo liegen meine Projekte?

Im Arbeitsordner. Dort legt der Server pro Projekt eine Datei `<Name>.pxproj` an und schreibt
Exporte nach `exports\<Projekt>\`. Den Ordner bestimmst du mit `--home`. Ohne Angabe gilt: Liegt neben
dem Programm ein Ordner `Projekte`, wird er genommen, sonst `Dokumente\PixelArt Studio`.

Editor und MCP-Server müssen denselben Arbeitsordner benutzen, damit du im Editor siehst, was Claude
zeichnet. Weil beide dieselbe Datei sind, passt das von selbst.

## Claude Desktop

1. In Claude Desktop: **Einstellungen → Entwickler → Konfiguration bearbeiten**. Es öffnet sich die
   Datei `claude_desktop_config.json` (sie liegt unter `%APPDATA%\Claude\`).
2. Diesen Inhalt eintragen. Falls schon andere Server eingetragen sind, nur den Block `"pixelart"`
   in `"mcpServers"` ergänzen. In JSON müssen Backslashes doppelt geschrieben werden.

```json
{
  "mcpServers": {
    "pixelart": {
      "command": "C:\\Users\\DEINNAME\\Desktop\\PixelArt Studio\\pixelart-studio.exe",
      "args": ["--mcp", "--home", "C:\\Users\\DEINNAME\\Desktop\\PixelArt Studio\\Projekte"]
    }
  }
}
```

3. Claude Desktop komplett beenden (auch im Infobereich neben der Uhr) und neu starten.
4. Im Chat sollte unter den Werkzeugen jetzt „pixelart“ erscheinen. Probiere:
   *„Lege im PixelArt Studio ein Projekt ‚Test‘ an und zeichne einen 16×16-Apfel.“*

## Claude Code

```sh
claude mcp add --scope user pixelart -- "C:\Users\DEINNAME\Desktop\PixelArt Studio\pixelart-studio.exe" --mcp --home "C:\Users\DEINNAME\Desktop\PixelArt Studio\Projekte"
```

`--scope user` trägt den Server für alle Ordner ein, nicht nur für den, in dem du den Befehl ausführst.

## Claude richtet es selbst ein

Wo Claude auf deinem Rechner Befehle ausführen oder Dateien bearbeiten darf (zum Beispiel in Claude
Code), kannst du die Einrichtung abgeben: Im Editor unter **Hilfe → Mit Claude verbinden…** den Prompt
aus Abschnitt 1 kopieren und an Claude schicken. Er nennt Programm, Argumente, den Befehl für Claude
Code, den Eintrag für Claude Desktop und wie Claude das Ergebnis prüft.

Abschnitt 5 im selben Dialog enthält einen zweiten Prompt für den Anfang eines Gesprächs. Er erklärt
einem verbundenen Claude, wie es mit dem Studio arbeitet.

## Andere MCP-Clients

Jeder Client, der lokale MCP-Server über stdio starten kann, funktioniert. Einzutragen sind immer
dieselben zwei Angaben: das Programm (`pixelart-studio.exe`) und die Argumente (`--mcp --home <Ordner>`).

Für Rechner ohne Bildschirm lässt sich der Server auch als eigenes kleines Programm bauen:
`cargo build --release -p pixel-mcp` ergibt `pixelart-mcp`, das ohne `--mcp` auskommt.

## Werkzeuge

| Werkzeug | Zweck |
|---|---|
| `get_studio_info` | Arbeitsordner, Projekte, Vorlagen, Paletten, Animationsarten |
| `create_project`, `get_project` | Projekt anlegen, Überblick samt Konstanz-Prüfung |
| `create_character` | Figur mit fester Größe und gesperrter Palette |
| `create_animation` | Animation einer Figur; jeder Frame startet als Kopie der Grundpose |
| `create_image` | Freies Bild (Tile, Item, Szene) aus Vorlage oder mit eigener Größe |
| `draw` | Zeichnen: Pixel, Linie, Rechteck, Ellipse, Füllen, Textraster, Verschieben, Spiegeln |
| `view` | Vergrößerte Vorschau eines Frames oder aller Frames |
| `get_pixels` | Pixel exakt als Text lesen |
| `edit_layers`, `edit_frames` | Ebenen und Frames verwalten, Frame-Dauer setzen |
| `set_palette`, `add_character_colors` | Palette eines Bildes tauschen, Palette einer Figur erweitern |
| `copy_character` | Figur samt Animationen unverändert in ein anderes Projekt übernehmen |
| `import_image` | PNG, JPG oder GIF (auch Spritesheets) als Bild, Figur oder Animation ins Projekt holen |
| `export` | PNG (auch mit Palette), JPG, Spritesheet mit JSON-Metadaten, animiertes GIF oder WebM-Video |
| `delete_asset` | Figur, Animation oder Bild löschen |

## Wenn etwas nicht klappt

- **Server erscheint nicht:** Pfad zur exe prüfen und ob die Backslashes in der JSON-Datei doppelt sind.
  Claude Desktop schreibt Fehler nach `%APPDATA%\Claude\logs\`.
- **Windows warnt vor der exe:** Die Datei ist nicht signiert. Über „Weitere Informationen → Trotzdem
  ausführen“ lässt sie sich starten.
- **WebM-Export meldet, ffmpeg fehle:** Im Editor den Export-Dialog öffnen (Strg+E), „WebM“ wählen und
  auf „ffmpeg jetzt installieren“ klicken. Von Hand geht es mit `winget install Gyan.FFmpeg` in der
  Eingabeaufforderung, oder du legst `ffmpeg.exe` neben das Programm. Ein Neustart ist nicht nötig.
  Alle anderen Formate brauchen kein ffmpeg.
- **Das Programm lässt sich nicht ersetzen:** Solange der Editor offen ist oder Claude den Server
  benutzt, sperrt Windows die Datei. Zum Aktualisieren den Editor schließen und Claude beenden.
- **Selbst testen:** `pixelart-studio.exe --version` in der Eingabeaufforderung zeigt die Version an.
