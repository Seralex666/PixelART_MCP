//! Einstellungen, die über einen Neustart hinaus gelten: Arbeitsordner und Sprache.
//!
//! Sie liegen in `%APPDATA%\PixelArt Studio\settings.json` (unter Linux in
//! `~/.config/PixelArt Studio`). `PIXELART_SETTINGS` legt eine andere Datei fest,
//! etwa für Tests. Fehlt die Datei oder ist sie kaputt, gelten die Vorgaben.

use pixel_core::lang::Lang;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const FILE: &str = "settings.json";
const FOLDER: &str = "PixelArt Studio";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Der Arbeitsordner für Projekte und Exporte. `None` heißt Vorgabe.
    pub home: Option<PathBuf>,
    pub lang: Lang,
}

impl Settings {
    /// Wo die Einstellungen liegen.
    pub fn path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("PIXELART_SETTINGS").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(p));
        }
        Some(dirs::config_dir()?.join(FOLDER).join(FILE))
    }

    pub fn load() -> Self {
        Self::path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return Self::default();
        };
        Self {
            home: v["home"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            lang: v["language"]
                .as_str()
                .and_then(Lang::from_code)
                .unwrap_or_default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or_else(|| {
            pixel_core::lang::tr(
                "Es gibt keinen Ordner für Einstellungen",
                "There is no folder for settings",
            )
            .to_string()
        })?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let v = json!({
            "home": self.home.as_ref().map(|p| p.display().to_string()),
            "language": self.lang.code(),
        });
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default())
        };
        write().map_err(|err| {
            pixel_core::trf!(
                "Die Einstellungen lassen sich nicht speichern ({}): {err}",
                "The settings cannot be saved ({}): {err}",
                path.display()
            )
        })
    }

    /// Der Arbeitsordner, der gilt, wenn beim Start keiner genannt wird.
    /// `PIXELART_HOME` geht vor, dann die Einstellung, dann die Vorgabe.
    pub fn effective_home(&self) -> Option<PathBuf> {
        let env = std::env::var_os("PIXELART_HOME").filter(|v| !v.is_empty());
        if env.is_some() {
            return pixel_core::Workspace::default_home();
        }
        self.home
            .clone()
            .or_else(pixel_core::Workspace::default_home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_saving_and_broken_files_give_defaults() {
        let dir = std::env::temp_dir().join(format!("pixelart-settings-{}", uuid::Uuid::new_v4()));
        let path = dir.join("sub").join(FILE);
        assert_eq!(Settings::load_from(&path), Settings::default());
        let s = Settings {
            home: Some(PathBuf::from(r"C:\Users\Max\Pixel")),
            lang: Lang::En,
        };
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path), s);
        std::fs::write(&path, "{kaputt").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        std::fs::write(&path, r#"{"language": "fr", "home": ""}"#).unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
