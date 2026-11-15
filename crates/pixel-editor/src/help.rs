//! Menü „Hilfe“: wie Claude (oder ein anderer MCP-Client) mit dem Studio
//! verbunden wird, samt fertigen Texten zum Kopieren.

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
        format!(
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
            name = SERVER_NAME,
            flag = MCP_FLAG,
            server = self.server_str(),
            home = self.home_str(),
            command = self.claude_code_command(),
            json = self.desktop_json(),
        )
    }

    /// Prompt für ein bereits verbundenes Claude: so arbeitet man mit dem Studio.
    pub fn usage_prompt(&self) -> String {
        let project = match &self.project {
            Some(p) => format!(
                "Arbeite im Projekt \"{p}\" (get_project zeigt, was es dort schon gibt)."
            ),
            None => "Frag mich, in welchem Projekt du arbeiten sollst, oder lege mit create_project ein neues an.".to_string(),
        };
        format!(
            r#"Du bist über den MCP-Server "{name}" mit meinem PixelArt Studio verbunden. Ich sehe im Editor live, was du zeichnest.

So arbeitest du damit:
1. Rufe zuerst get_studio_info auf. Es nennt Arbeitsordner, Projekte, Bildvorlagen, Paletten und Animationsarten.
2. {project}
3. Eine Figur legst du mit create_character an. Größe und Palette sind danach fest. Animationen (idle, walk, run, jump, attack, hurt, death) leitest du mit create_animation aus der Figur ab; jeder Frame startet als Kopie der Grundpose. So bleibt die Figur überall dieselbe.
4. Freie Bilder wie Tiles, Items oder Szenen legst du mit create_image an (höchstens 1024×1024, Farbtiefe 8, 16 oder 32 Bit).
5. Gezeichnet wird mit draw. Sieh dir das Ergebnis danach mit view an und lies bei Bedarf exakte Pixel mit get_pixels.
6. Braucht eine Figur eine weitere Farbe, nimm sie mit add_character_colors in ihre Palette auf.
7. Mit export schreibst du PNG, JPG, ein Spritesheet, ein animiertes GIF oder ein WebM-Video in den Ordner "exports" im Arbeitsordner.

Arbeite in kleinen Schritten und kontrolliere jedes Zwischenergebnis mit view, bevor du weiterzeichnest."#,
            name = SERVER_NAME,
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
