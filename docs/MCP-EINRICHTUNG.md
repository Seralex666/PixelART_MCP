# MCP-Server einrichten

Der MCP-Server ist eine einzelne Datei: `pixelart-mcp.exe` (Windows) bzw. `pixelart-mcp` (Linux, macOS).
Er braucht keine Installation. Claude startet ihn selbst, sobald er eingetragen ist.

In den Beispielen liegt die exe in `C:\Users\DEINNAME\Desktop\PixelArt Studio`. Ersetze `DEINNAME`
durch deinen Windows-Benutzernamen.

## Wo liegen meine Projekte?

Im Arbeitsordner. Dort legt der Server pro Projekt eine Datei `<Name>.pxproj` an und schreibt
Exporte nach `exports\<Projekt>\`. Den Ordner bestimmst du mit `--home`. Ohne Angabe ist es
`Dokumente\PixelArt Studio`.

## Claude Desktop

1. In Claude Desktop: **Einstellungen → Entwickler → Konfiguration bearbeiten**. Es öffnet sich die
   Datei `claude_desktop_config.json` (sie liegt unter `%APPDATA%\Claude\`).
2. Diesen Inhalt eintragen. Falls schon andere Server eingetragen sind, nur den Block `"pixelart"`
   in `"mcpServers"` ergänzen. In JSON müssen Backslashes doppelt geschrieben werden.

```json
{
  "mcpServers": {
    "pixelart": {
      "command": "C:\\Users\\DEINNAME\\Desktop\\PixelArt Studio\\pixelart-mcp.exe",
      "args": ["--home", "C:\\Users\\DEINNAME\\Desktop\\PixelArt Studio\\Projekte"]
    }
  }
}
```

3. Claude Desktop komplett beenden (auch im Infobereich neben der Uhr) und neu starten.
4. Im Chat sollte unter den Werkzeugen jetzt „pixelart“ erscheinen. Probiere:
   *„Lege im PixelArt Studio ein Projekt ‚Test‘ an und zeichne einen 16×16-Apfel.“*

## Claude Code

```sh
claude mcp add pixelart -- "C:\Users\DEINNAME\Desktop\PixelArt Studio\pixelart-mcp.exe" --home "C:\Users\DEINNAME\Desktop\PixelArt Studio\Projekte"
```

## Andere MCP-Clients

Jeder Client, der lokale MCP-Server über stdio starten kann, funktioniert. Einzutragen sind immer
dieselben zwei Angaben: das Programm (`pixelart-mcp.exe`) und die Argumente (`--home <Ordner>`).

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
| `export` | PNG, JPG oder Spritesheet mit JSON-Metadaten |
| `delete_asset` | Figur, Animation oder Bild löschen |

## Wenn etwas nicht klappt

- **Server erscheint nicht:** Pfad zur exe prüfen und ob die Backslashes in der JSON-Datei doppelt sind.
  Claude Desktop schreibt Fehler nach `%APPDATA%\Claude\logs\`.
- **Windows warnt vor der exe:** Die Datei ist nicht signiert. Über „Weitere Informationen → Trotzdem
  ausführen“ lässt sie sich starten.
- **Selbst testen:** `pixelart-mcp.exe --version` in der Eingabeaufforderung zeigt die Version an.
