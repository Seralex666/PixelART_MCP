//! Der Arbeitsordner: hier liegen die Projektdateien und die Exporte.
//! Editor und MCP-Server benutzen denselben Ordner und dieselben Regeln.

use crate::export::{self, SheetMeta};
use crate::project::PROJECT_EXT;
use crate::{Error, Project, Result, Rgba, Sprite};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Name des Standard-Arbeitsordners (unter „Dokumente“).
pub const DEFAULT_FOLDER: &str = "PixelArt Studio";
/// Liegt ein Ordner dieses Namens neben dem Programm, ist er der Arbeitsordner.
pub const PORTABLE_FOLDER: &str = "Projekte";

#[derive(Clone, Debug)]
pub struct Workspace {
    home: PathBuf,
}

/// Was exportiert werden soll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExportFormat {
    Png,
    /// JPEG kennt keine Transparenz; sie wird durch `background` ersetzt.
    Jpg {
        background: Rgba,
        quality: u8,
    },
    /// Spritesheet aus allen Frames. `columns = 0` legt alle in eine Zeile.
    Sheet {
        columns: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    pub files: Vec<PathBuf>,
    pub sheet: Option<SheetMeta>,
}

/// Warum ein Asset-Name nicht vergeben werden kann.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameClash {
    /// Ein Asset heißt bereits so.
    Same(String),
    /// Ein Asset ergäbe beim Export denselben Dateinamen (z. B. „Tile A“ und „Tile_A“).
    Similar(String),
}

impl Workspace {
    /// Öffnet den Arbeitsordner und legt ihn bei Bedarf an.
    pub fn open(home: impl Into<PathBuf>) -> Result<Self> {
        let home = home.into();
        std::fs::create_dir_all(&home)?;
        Ok(Self { home })
    }

    /// Der Standard-Arbeitsordner, in dieser Reihenfolge:
    /// 1. Umgebungsvariable `PIXELART_HOME`
    /// 2. ein Ordner `Projekte` neben dem Programm, falls es ihn gibt
    ///    (so bleibt alles zusammen in einem Ordner)
    /// 3. `Dokumente/PixelArt Studio`
    pub fn default_home() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os("PIXELART_HOME").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(dir));
        }
        let beside_exe = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join(PORTABLE_FOLDER)))
            .filter(|dir| dir.is_dir());
        if beside_exe.is_some() {
            return beside_exe;
        }
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .map(|d| d.join(DEFAULT_FOLDER))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Pfad der Projektdatei. Der Name darf nur Buchstaben, Ziffern, Leerzeichen,
    /// `-` und `_` enthalten, damit er nie aus dem Arbeitsordner herausführt.
    pub fn project_path(&self, name: &str) -> Result<PathBuf> {
        let name = name.trim();
        match check_project_name(name) {
            Ok(()) => Ok(self.home.join(format!("{name}.{PROJECT_EXT}"))),
            Err(ProjectNameError::Invalid) => Err(Error::InvalidName(format!(
                "Ungültiger Projektname '{name}': erlaubt sind 1 bis 64 Buchstaben, Ziffern, Leerzeichen, '-' und '_'"
            ))),
            Err(ProjectNameError::Reserved) => Err(Error::InvalidName(format!(
                "Ungültiger Projektname '{name}': dieser Name ist unter Windows reserviert"
            ))),
        }
    }

    /// Namen aller Projekte im Arbeitsordner, alphabetisch.
    pub fn project_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.home)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()?.to_str()? != PROJECT_EXT {
                    return None;
                }
                path.file_stem()?.to_str().map(String::from)
            })
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    pub fn exists(&self, name: &str) -> bool {
        self.project_path(name).is_ok_and(|p| p.exists())
    }

    /// Legt ein neues, leeres Projekt an.
    pub fn create(&self, name: &str) -> Result<Project> {
        let path = self.project_path(name)?;
        if path.exists() {
            return Err(Error::InvalidName(format!(
                "Ein Projekt namens '{}' gibt es schon",
                name.trim()
            )));
        }
        let project = Project::new(name.trim());
        project.save(&path)?;
        Ok(project)
    }

    pub fn load(&self, name: &str) -> Result<Project> {
        Project::load(&self.project_path(name)?)
    }

    pub fn save(&self, name: &str, project: &Project) -> Result<()> {
        project.save(&self.project_path(name)?)
    }

    /// Zeitpunkt der letzten Änderung der Projektdatei, um Änderungen von außen zu bemerken.
    pub fn modified(&self, name: &str) -> Option<SystemTime> {
        std::fs::metadata(self.project_path(name).ok()?)
            .and_then(|m| m.modified())
            .ok()
    }

    /// Ordner, in dem die Exporte eines Projekts landen.
    pub fn exports_dir(&self, project: &str) -> PathBuf {
        // Der Projektname ist als Dateiname geprüft und eindeutig.
        self.home.join("exports").join(project.trim())
    }

    /// Exportiert ein Sprite in den Export-Ordner des Projekts.
    ///
    /// `frame = None` exportiert alle Frames (bei PNG/JPG je eine Datei).
    /// `scale` vergrößert ganzzahlig ohne Glättung.
    pub fn export(
        &self,
        project: &str,
        asset_name: &str,
        sprite: &Sprite,
        format: ExportFormat,
        frame: Option<usize>,
        scale: u32,
    ) -> Result<Exported> {
        self.project_path(project)?;
        if !(1..=export::MAX_SCALE).contains(&scale) {
            return Err(Error::InvalidName(format!(
                "Vergrößerung muss zwischen 1 und {} liegen",
                export::MAX_SCALE
            )));
        }
        if let Some(f) = frame {
            sprite.frame(f)?;
        }
        let dir = self.exports_dir(project);
        std::fs::create_dir_all(&dir)?;
        let base = file_safe(asset_name);
        let suffix = if scale > 1 {
            format!("@{scale}x")
        } else {
            String::new()
        };
        let frames: Vec<usize> = match frame {
            Some(f) => vec![f],
            None => (0..sprite.frames().len()).collect(),
        };
        let numbered = sprite.frames().len() > 1;
        let file = |f: usize, ext: &str| {
            if numbered {
                dir.join(format!("{base}-{f:02}{suffix}.{ext}"))
            } else {
                dir.join(format!("{base}{suffix}.{ext}"))
            }
        };
        let mut files = Vec::new();
        let mut sheet = None;
        match format {
            ExportFormat::Png => {
                for &f in &frames {
                    let path = file(f, "png");
                    std::fs::write(&path, export::frame_png(sprite, f, scale)?)?;
                    files.push(path);
                }
            }
            ExportFormat::Jpg {
                background,
                quality,
            } => {
                for &f in &frames {
                    let path = file(f, "jpg");
                    let bytes = export::frame_jpeg(sprite, f, scale, background, quality)?;
                    std::fs::write(&path, bytes)?;
                    files.push(path);
                }
            }
            ExportFormat::Sheet { columns } => {
                let stem = dir.join(format!("{base}-sheet{suffix}"));
                sheet = Some(export::save_sheet(sprite, &stem, columns, scale)?);
                files.push(with_ext(&stem, "png"));
                files.push(with_ext(&stem, "json"));
            }
        }
        Ok(Exported { files, sheet })
    }
}

/// Warum ein Projektname nicht geht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectNameError {
    /// Leer, zu lang oder mit Zeichen, die in Dateinamen Ärger machen.
    Invalid,
    /// Unter Windows reservierter Gerätename (CON, NUL, ...).
    Reserved,
}

/// Prüft einen Projektnamen (ohne Leerraum außen herum).
pub fn check_project_name(name: &str) -> std::result::Result<(), ProjectNameError> {
    let name = name.trim();
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'));
    if !ok {
        Err(ProjectNameError::Invalid)
    } else if is_reserved_file_name(name) {
        Err(ProjectNameError::Reserved)
    } else {
        Ok(())
    }
}

/// Hängt eine Endung an, ohne Punkte im Namen als Endung zu deuten ("a@2x" bleibt ganz).
fn with_ext(stem: &Path, ext: &str) -> PathBuf {
    let mut s = stem.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

/// Prüft, ob ein neuer Asset-Name neben den vorhandenen bestehen kann.
pub fn check_asset_name(taken: &[String], name: &str) -> std::result::Result<(), NameClash> {
    let key = name_key(name);
    match taken.iter().find(|t| name_key(t) == key) {
        Some(t) if t.eq_ignore_ascii_case(name.trim()) => Err(NameClash::Same(t.clone())),
        Some(t) => Err(NameClash::Similar(t.clone())),
        None => Ok(()),
    }
}

/// Vergleichsschlüssel für Namen: gleich, wenn beim Export derselbe Dateiname
/// entstünde. Kleinschreibung, weil Windows Dateinamen so vergleicht.
pub fn name_key(name: &str) -> String {
    file_safe(name).to_lowercase()
}

/// Macht aus einem Asset-Namen einen sicheren Dateinamen (ohne Endung).
pub fn file_safe(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "unnamed".into()
    } else if is_reserved_file_name(&s) {
        format!("{s}_")
    } else {
        s
    }
}

/// Gerätenamen wie CON oder NUL lassen sich unter Windows nicht als Datei anlegen.
pub fn is_reserved_file_name(name: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    RESERVED.contains(&name.to_ascii_lowercase().as_str())
}
