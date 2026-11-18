//! Menü „Hilfe“: wie Claude (oder ein anderer MCP-Client) mit dem Studio
//! verbunden wird, samt fertigen Texten zum Kopieren.

use pixel_core::lang::tr;
use pixel_core::trf;
use std::path::{Path, PathBuf};

/// Name, unter dem der Server beim MCP-Client eingetragen wird.
pub const SERVER_NAME: &str = "pixelart";

#[cfg(windows)]
const PROGRAM_FILE: &str = "pixelart-studio.exe";
#[cfg(not(windows))]
const PROGRAM_FILE: &str = "pixelart-studio";

/// Mit dieser Angabe läuft das Programm als MCP-Server statt als Editor.
pub const MCP_FLAG: &str = "--mcp";

/// Alles, was ein MCP-Client für die Verbindung wissen muss.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectInfo {
    /// Der MCP-Server: dieses Programm selbst, gestartet mit `--mcp`.
    pub server: PathBuf,
    /// Arbeitsordner des Editors. Der Server bekommt denselben, damit beide
    /// dieselben Projekte sehen.
    pub home: PathBuf,
    /// Das gerade geöffnete Projekt, für den Arbeits-Prompt.
    pub project: Option<String>,
}

impl ConnectInfo {
    pub fn detect(home: &Path, project: Option<&str>) -> Self {
        let server = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(PROGRAM_FILE));
        Self {
            server,
            home: home.to_path_buf(),
            project: project.map(String::from),
        }
    }

    fn server_str(&self) -> String {
        self.server.display().to_string()
    }

    fn home_str(&self) -> String {
        self.home.display().to_string()
    }

    /// Eintrag für `claude_desktop_config.json`. JSON verdoppelt die Backslashes
    /// von Windows-Pfaden von selbst.
    pub fn desktop_json(&self) -> String {
        let config = serde_json::json!({
            "mcpServers": {
                SERVER_NAME: {
                    "command": self.server_str(),
                    "args": [MCP_FLAG, "--home", self.home_str()],
                }
            }
        });
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }

    /// Befehl für Claude Code. `--scope user` trägt den Server für alle Ordner ein.
    pub fn claude_code_command(&self) -> String {
        format!(
            "claude mcp add --scope user {SERVER_NAME} -- \"{}\" {MCP_FLAG} --home \"{}\"",
            self.server_str(),
            self.home_str()
        )
    }

    /// Prompt, mit dem Claude die Verbindung selbst einrichtet. Dafür muss Claude
    /// auf dem Rechner Befehle ausführen oder Dateien bearbeiten dürfen.
    pub fn setup_prompt(&self) -> String {
        let name = SERVER_NAME;
        let flag = MCP_FLAG;
        let server = self.server_str();
        let home = self.home_str();
        let command = self.claude_code_command();
        let json = self.desktop_json();
        trf!(
            r#"Bitte richte für mich den MCP-Server "{name}" ein. Er gehört zum Programm "PixelArt Studio" und lässt dich echte Pixel-Art zeichnen, die ich live im Editor sehe.

Angaben zum Server:
- Name: {name}
- Programm: {server}
- Argumente: {flag} --home "{home}"
- Übertragung: stdio (lokales Programm, kein Netzwerk, keine Zugangsdaten)
- Hinweis: Es ist dieselbe Datei wie der Editor. Mit {flag} öffnet sie kein Fenster, sondern spricht MCP über stdin/stdout.

So richtest du ihn ein, je nachdem wo du läufst:

1. Claude Code: Führe diesen Befehl aus und prüfe das Ergebnis mit "claude mcp list".
   {command}

2. Claude Desktop: Öffne die Datei %APPDATA%\Claude\claude_desktop_config.json (lege sie an, falls sie fehlt) und ergänze unter "mcpServers" den Eintrag "{name}". Lass alle vorhandenen Einträge unverändert. So sieht der Eintrag aus:
{json}
   Danach muss Claude Desktop komplett beendet (auch im Infobereich neben der Uhr) und neu gestartet werden. Sag mir Bescheid, wenn ich das tun soll.

3. Anderer MCP-Client: Trage einen lokalen stdio-Server mit dem Programm und den Argumenten von oben ein.

Prüfen:
- "{server}" --version muss "pixelart-studio" und eine Versionsnummer ausgeben.
- Sobald der Server verbunden ist, rufe das Werkzeug get_studio_info auf und nenne mir den Arbeitsordner. Er muss {home} sein.

Wenn etwas davon nicht geht, sag mir genau, welcher Schritt und welche Fehlermeldung."#,
            r#"Please set up the MCP server "{name}" for me. It belongs to the program "PixelArt Studio" and lets you draw real pixel art that I see live in the editor.

Server details:
- Name: {name}
- Program: {server}
- Arguments: {flag} --home "{home}"
- Transport: stdio (local program, no network, no credentials)
- Note: It is the same file as the editor. With {flag} it opens no window and speaks MCP over stdin/stdout.

How to set it up, depending on where you run:

1. Claude Code: Run this command and check the result with "claude mcp list".
   {command}

2. Claude Desktop: Open the file %APPDATA%\Claude\claude_desktop_config.json (create it if it is missing) and add the entry "{name}" under "mcpServers". Leave all existing entries unchanged. This is the entry:
{json}
   Afterwards Claude Desktop must be quit completely (also in the system tray next to the clock) and started again. Tell me when I should do that.

3. Other MCP client: Add a local stdio server with the program and the arguments from above.

Check:
- "{server}" --version must print "pixelart-studio" and a version number.
- As soon as the server is connected, call the tool get_studio_info and tell me the workspace folder. It must be {home}.

If any of this does not work, tell me exactly which step and which error message."#
        )
    }

    /// Prompt für ein bereits verbundenes Claude: so arbeitet man mit dem Studio.
    pub fn usage_prompt(&self) -> String {
        let project = match &self.project {
            Some(p) => trf!(
                "Arbeite im Projekt \"{p}\" (get_project zeigt, was es dort schon gibt).",
                "Work in the project \"{p}\" (get_project shows what is already there)."
            ),
            None => tr(
                "Frag mich, in welchem Projekt du arbeiten sollst, oder lege mit create_project ein neues an.",
                "Ask me which project to work in, or create a new one with create_project.",
            )
            .to_string(),
        };
        let name = SERVER_NAME;
        trf!(
            r#"Du bist über den MCP-Server "{name}" mit meinem PixelArt Studio verbunden. Ich sehe im Editor live, was du zeichnest.

So arbeitest du damit:
1. Rufe zuerst get_studio_info auf. Es nennt Arbeitsordner, Projekte, Bildvorlagen, Paletten und Animationsarten.
2. {project}
3. Eine Figur legst du mit create_character an. Größe und Palette sind danach fest. Animationen (idle, walk, run, jump, attack, hurt, death) leitest du mit create_animation aus der Figur ab; jeder Frame startet als Kopie der Grundpose. So bleibt die Figur überall dieselbe.
4. Freie Bilder wie Tiles, Items oder Szenen legst du mit create_image an (höchstens 1024×1024, Farbtiefe 8, 16 oder 32 Bit).
5. Gezeichnet wird mit draw. Neben Pixeln, Formen und dem Textraster (grid) gibt es Techniken für mehr Tiefe: outline, shade, gradient, dither, noise, stamp und shader (Formel je Pixel). Ein Aufruf kann mehrere Frames zeichnen (frame: "all", Unterschiede über per_frame). Sieh dir das Ergebnis mit view an (grid und zoom_region helfen beim genauen Hinsehen), lies exakte Pixel mit get_pixels und nimm Fehlgriffe mit undo zurück.
6. Braucht eine Figur eine weitere Farbe, nimm sie mit add_character_colors in ihre Palette auf. Mit set_style legst du Palette, Lichtrichtung und Umrissfarbe für das ganze Projekt fest, palette_from_image liest die Farben aus einer Vorlage, reduce_colors bringt ein Asset auf weniger Farben. get_project nennt Hinweise auf typische Versehen wie verirrte Pixel oder Lücken im Umriss.
7. Nenne ich einen Stil wie Manga, Comic, 8 Bit, 16 Bit, HQ Pixel-Art oder Burgen (oder einen eigenen), hol ihn mit styles und halte dich genau an Palette, Umriss, Schattierung und Regeln; create_image, create_character und set_style nehmen ihn mit `style`.
8. Mit export schreibst du PNG, JPG, ein Spritesheet, ein animiertes GIF oder ein WebM-Video in den Ordner "exports" im Arbeitsordner; mit normal_map kommen Normal Maps für Licht im Spiel dazu.
9. Vorhandene Dateien (PNG, JPG, GIF, auch Spritesheets) holst du mit import_image als Bild, Figur oder Animation ins Projekt.
10. Für Leuchten, Blitze, Explosionen, Spiegelungen im Wasser, Funken, Rauch, Magie und Farbzyklen (fließendes Wasser, Feuer, Lava) nimmst du effects, statt sie Frame für Frame zu malen. Jeder Effekt liegt auf einer eigenen Ebene und läuft über alle Frames; mit frames wird aus einem Standbild eine Schleife.
11. Für Böden, Wasser und Wände in Spielen legst du mit autotile eine Vorlage an, malst den Block aus 2 × 2 Kacheln und die Kachel mit den Innenecken auf die Ebene "Terrain" und baust daraus den ganzen Satz. export mit format godot oder tiled schreibt ihn für die Engine.

Arbeite in kleinen Schritten und kontrolliere jedes Zwischenergebnis mit view, bevor du weiterzeichnest."#,
            r#"You are connected to my PixelArt Studio through the MCP server "{name}". I see live in the editor what you draw.

How to work with it:
1. First call get_studio_info. It lists the workspace folder, projects, canvas templates, palettes and animation presets.
2. {project}
3. Create a character with create_character. Its size and palette are fixed afterwards. Derive animations (idle, walk, run, jump, attack, hurt, death) from the character with create_animation; every frame starts as a copy of the base pose. That keeps the character the same everywhere.
4. Create free images such as tiles, items or scenes with create_image (at most 1024×1024, color depth 8, 16 or 32 bit).
5. Draw with draw. Besides pixels, shapes and the text grid (grid) there are techniques for more depth: outline, shade, gradient, dither, noise, stamp and shader (a formula per pixel). One call can draw several frames (frame: "all", differences via per_frame). Look at the result with view (grid and zoom_region help to look closely), read exact pixels with get_pixels and take back mistakes with undo.
6. If a character needs another color, add it to its palette with add_character_colors. set_style sets palette, light direction and outline color for the whole project, palette_from_image reads the colors from a reference, reduce_colors brings an asset down to fewer colors. get_project gives hints on typical slips such as stray pixels or gaps in an outline.
7. When I name a style such as manga, comic, 8 bit, 16 bit, HQ pixel art or castles (or an own one), get it with styles and follow its palette, outline, shading and rules exactly; create_image, create_character and set_style take it as `style`.
8. export writes PNG, JPG, a sprite sheet, an animated GIF or a WebM video into the folder "exports" in the workspace folder; with normal_map it adds normal maps for lighting in games.
9. import_image brings existing files (PNG, JPG, GIF, also sprite sheets) into the project as an image, a character or an animation.
10. For glow, lightning, explosions, reflections in water, sparks, smoke, magic and color cycling (flowing water, fire, lava) use effects instead of painting them frame by frame. Each effect sits on its own layer and runs over all frames; frames turns a still image into a loop.
11. For floors, water and walls in games, create a template with autotile, paint the 2 × 2 block and the inner corner tile on the layer "Terrain" and build the whole set from it. export with format godot or tiled writes it for the engine.

Work in small steps and check every intermediate result with view before you continue drawing."#
        )
    }
}

/// Öffnet einen Ordner im Dateimanager.
pub fn open_folder(path: &Path) {
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let program = "xdg-open";
    // Klappt das nicht, steht der Pfad trotzdem lesbar in der Oberfläche.
    let _ = std::process::Command::new(program).arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ConnectInfo {
        ConnectInfo {
            server: PathBuf::from(
                r"C:\Users\Max Muster\Desktop\PixelArt Studio\pixelart-studio.exe",
            ),
            home: PathBuf::from(r"C:\Users\Max Muster\Desktop\PixelArt Studio\Projekte"),
            project: Some("Mein Spiel".into()),
        }
    }

    #[test]
    fn desktop_json_is_valid_and_keeps_paths() {
        let json = info().desktop_json();
        // Backslashes stehen in der Datei doppelt …
        assert!(json.contains(r"C:\\Users\\Max Muster\\Desktop"));
        // … und ergeben beim Lesen wieder den echten Pfad.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let entry = &v["mcpServers"]["pixelart"];
        assert_eq!(
            entry["command"],
            r"C:\Users\Max Muster\Desktop\PixelArt Studio\pixelart-studio.exe"
        );
        assert_eq!(entry["args"][0], "--mcp");
        assert_eq!(entry["args"][1], "--home");
        assert_eq!(
            entry["args"][2],
            r"C:\Users\Max Muster\Desktop\PixelArt Studio\Projekte"
        );
    }

    #[test]
    fn command_quotes_paths_with_spaces() {
        assert_eq!(
            info().claude_code_command(),
            r#"claude mcp add --scope user pixelart -- "C:\Users\Max Muster\Desktop\PixelArt Studio\pixelart-studio.exe" --mcp --home "C:\Users\Max Muster\Desktop\PixelArt Studio\Projekte""#
        );
    }

    #[test]
    fn prompts_carry_everything_needed() {
        let i = info();
        let setup = i.setup_prompt();
        assert!(setup.contains(&i.claude_code_command()));
        assert!(setup.contains(&i.desktop_json()));
        assert!(setup.contains("claude_desktop_config.json"));
        assert!(setup.contains("get_studio_info"));
        let usage = i.usage_prompt();
        assert!(usage.contains("\"Mein Spiel\""));
        let mut none = i.clone();
        none.project = None;
        assert!(none.usage_prompt().contains("create_project"));
    }
}
