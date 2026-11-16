use crate::{Animation, AnimationKind, Character, Error, Palette, Result, Sprite};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

/// Aktuelle Version des Projektformats.
pub const FORMAT_VERSION: u32 = 1;
/// Dateiendung für Projekte.
pub const PROJECT_EXT: &str = "pxproj";
/// Dateiendung für einzeln gespeicherte Figuren (zum Übernehmen in andere Projekte).
pub const CHARACTER_EXT: &str = "pxchar";

/// Ein freies Bild im Projekt (Tile, Item, Szene), ohne Bindung an eine Figur.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub id: Uuid,
    pub name: String,
    pub sprite: Sprite,
}

/// Ein Projekt: Figuren, ihre Animationen, freie Bilder und Paletten.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub palettes: Vec<Palette>,
    #[serde(default)]
    pub characters: Vec<Character>,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub images: Vec<Image>,
    /// Der Stil des Projekts. Fehlt in Dateien aus älteren Versionen.
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    pub style: Style,
}

/// Die Lichtrichtungen, die ein Stil kennt.
pub const LIGHT_DIRS: [&str; 8] = [
    "top-left",
    "top",
    "top-right",
    "left",
    "right",
    "bottom-left",
    "bottom",
    "bottom-right",
];

/// Der Stil eines Projekts: was für alle seine Bilder gleich sein soll.
/// Jede Angabe ist freiwillig. Neue Bilder und Figuren starten mit der Palette,
/// und Zeichentechniken nehmen Lichtrichtung und Umrissfarbe von hier, wenn
/// ein Aufruf sie nicht selbst nennt.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// Die Farben des Projekts. Leer heißt: keine Vorgabe.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<crate::Rgba>,
    /// Woher das Licht kommt, einer der Werte aus [`LIGHT_DIRS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_dir: Option<String>,
    /// Die Farbe der Umrisse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<crate::Rgba>,
    /// Freie Notizen zum Stil, etwa "2 Pixel Umriss, keine reinen Schwarztöne".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Eine Abweichung, die die Konstanz einer Figur verletzt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ValidationIssue {
    pub animation: String,
    pub message: String,
}

/// Einzeln gespeicherte Figur samt ihrer Animationen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterBundle {
    pub format_version: u32,
    pub character: Character,
    #[serde(default)]
    pub animations: Vec<Animation>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            id: Uuid::new_v4(),
            name: name.into(),
            palettes: vec![Palette::default_16()],
            characters: Vec::new(),
            animations: Vec::new(),
            images: Vec::new(),
            style: Style::default(),
        }
    }

    // ---- Figuren ----

    pub fn add_character(&mut self, c: Character) -> Uuid {
        let id = c.id;
        self.characters.push(c);
        id
    }

    pub fn character(&self, id: Uuid) -> Result<&Character> {
        self.characters
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| not_found("Figur", id))
    }

    pub fn character_mut(&mut self, id: Uuid) -> Result<&mut Character> {
        self.characters
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| not_found("Figur", id))
    }

    pub fn character_by_name(&self, name: &str) -> Option<&Character> {
        self.characters
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Entfernt eine Figur samt ihrer Animationen.
    pub fn remove_character(&mut self, id: Uuid) -> Result<Character> {
        let i = self
            .characters
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| not_found("Figur", id))?;
        self.animations.retain(|a| a.character_id != id);
        Ok(self.characters.remove(i))
    }

    /// Erweitert die Palette einer Figur um neue Farben. Vorhandene Farben bleiben
    /// unverändert, deshalb bleibt alles bisher Gezeichnete gültig. Die Erweiterung
    /// gilt auch für alle Animationen der Figur. Gibt die neue Palettengröße zurück.
    pub fn add_character_colors(&mut self, id: Uuid, colors: &[crate::Rgba]) -> Result<usize> {
        let depth = self.character(id)?.base.depth;
        let mut palette = self.character(id)?.palette().clone();
        palette.locked = false;
        for &c in colors {
            let c = depth.conform(c);
            if !c.is_transparent() {
                palette.add(c)?;
            }
        }
        palette.locked = true;
        let n = palette.len();
        self.character_mut(id)?.base.palette = palette.clone();
        for a in self.animations.iter_mut().filter(|a| a.character_id == id) {
            a.sprite.palette = palette.clone();
        }
        Ok(n)
    }

    /// Bringt eine Figur und all ihre Animationen gemeinsam auf höchstens `max`
    /// Farben. Die Palette bleibt gesperrt und überall dieselbe.
    pub fn reduce_character_colors(
        &mut self,
        id: Uuid,
        max: usize,
    ) -> Result<crate::quant::Reduced> {
        let at = self
            .characters
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| not_found("Figur", id))?;
        let mut sprites: Vec<&mut Sprite> = vec![&mut self.characters[at].base];
        sprites.extend(
            self.animations
                .iter_mut()
                .filter(|a| a.character_id == id)
                .map(|a| &mut a.sprite),
        );
        crate::quant::reduce(&mut sprites, max)
    }

    // ---- Animationen ----

    /// Legt für eine Figur eine Animation an, deren Frames mit der Grundpose starten.
    pub fn add_animation(
        &mut self,
        character_id: Uuid,
        kind: AnimationKind,
        frames: Option<usize>,
    ) -> Result<Uuid> {
        let anim = Animation::from_character(self.character(character_id)?, kind, frames)?;
        let id = anim.id;
        self.animations.push(anim);
        Ok(id)
    }

    pub fn animation(&self, id: Uuid) -> Result<&Animation> {
        self.animations
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| not_found("Animation", id))
    }

    pub fn animation_mut(&mut self, id: Uuid) -> Result<&mut Animation> {
        self.animations
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| not_found("Animation", id))
    }

    pub fn animations_of(&self, character_id: Uuid) -> impl Iterator<Item = &Animation> {
        self.animations
            .iter()
            .filter(move |a| a.character_id == character_id)
    }

    pub fn remove_animation(&mut self, id: Uuid) -> Result<Animation> {
        let i = self
            .animations
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| not_found("Animation", id))?;
        Ok(self.animations.remove(i))
    }

    // ---- Freie Bilder ----

    pub fn add_image(&mut self, name: impl Into<String>, sprite: Sprite) -> Uuid {
        let id = Uuid::new_v4();
        self.images.push(Image {
            id,
            name: name.into(),
            sprite,
        });
        id
    }

    pub fn image(&self, id: Uuid) -> Result<&Image> {
        self.images
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| not_found("Bild", id))
    }

    pub fn image_mut(&mut self, id: Uuid) -> Result<&mut Image> {
        self.images
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| not_found("Bild", id))
    }

    pub fn remove_image(&mut self, id: Uuid) -> Result<Image> {
        let i = self
            .images
            .iter()
            .position(|i| i.id == id)
            .ok_or_else(|| not_found("Bild", id))?;
        Ok(self.images.remove(i))
    }

    // ---- Konstanz ----

    /// Prüft, ob jede Animation noch zu ihrer Figur passt: gleiche Größe,
    /// gleiche Farbtiefe, gleiche Palette und nur Farben aus dieser Palette.
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        for a in &self.animations {
            let mut issue = |message: String| {
                issues.push(ValidationIssue {
                    animation: a.name.clone(),
                    message,
                })
            };
            let Ok(c) = self.character(a.character_id) else {
                issue("Die zugehörige Figur fehlt im Projekt.".into());
                continue;
            };
            let (w, h) = c.size();
            if (a.sprite.width(), a.sprite.height()) != (w, h) {
                issue(format!(
                    "Größe {}×{} weicht von der Figur ({w}×{h}) ab.",
                    a.sprite.width(),
                    a.sprite.height()
                ));
            }
            if a.sprite.depth != c.base.depth {
                issue("Farbtiefe weicht von der Figur ab.".into());
            }
            if a.sprite.palette.colors != c.palette().colors {
                issue("Palette weicht von der Figur ab.".into());
            }
            let mut foreign = std::collections::BTreeSet::new();
            for f in a.sprite.frames() {
                for l in &f.layers {
                    for &p in l.pixels() {
                        if !c.palette().contains(p) {
                            foreign.insert(p.to_string());
                        }
                    }
                }
            }
            if !foreign.is_empty() {
                let list: Vec<_> = foreign.iter().take(8).cloned().collect();
                issue(format!(
                    "{} Farbe(n) gehören nicht zur Palette der Figur: {}",
                    foreign.len(),
                    list.join(", ")
                ));
            }
        }
        issues
    }

    // ---- Dateien ----

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let p: Project = serde_json::from_str(json)?;
        if p.format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(p.format_version));
        }
        for c in &p.characters {
            c.base.check()?;
        }
        for a in &p.animations {
            a.sprite.check()?;
        }
        for i in &p.images {
            i.sprite.check()?;
        }
        Ok(p)
    }

    /// Speichert das Projekt. Geschrieben wird erst in eine temporäre Datei, die
    /// dann umbenannt wird, damit Editor und MCP-Server nie eine halbe Datei lesen.
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, self.to_json()?.as_bytes())
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    // ---- Figuren zwischen Projekten übernehmen ----

    /// Packt eine Figur mit all ihren Animationen zum Speichern oder Weitergeben.
    pub fn export_character(&self, id: Uuid) -> Result<CharacterBundle> {
        Ok(CharacterBundle {
            format_version: FORMAT_VERSION,
            character: self.character(id)?.clone(),
            animations: self.animations_of(id).cloned().collect(),
        })
    }

    /// Übernimmt eine Figur aus einem anderen Projekt, unverändert und mit derselben ID.
    /// Existiert die Figur schon, wird sie samt Animationen ersetzt.
    pub fn import_character(&mut self, bundle: CharacterBundle) -> Result<Uuid> {
        if bundle.format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(bundle.format_version));
        }
        bundle.character.base.check()?;
        for a in &bundle.animations {
            a.sprite.check()?;
        }
        let id = bundle.character.id;
        if self.characters.iter().any(|c| c.id == id) {
            self.remove_character(id)?;
        }
        self.characters.push(bundle.character);
        self.animations.extend(
            bundle
                .animations
                .into_iter()
                .filter(|a| a.character_id == id),
        );
        Ok(id)
    }
}

impl CharacterBundle {
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, serde_json::to_string(self)?.as_bytes())
    }

    pub fn load(path: &Path) -> Result<Self> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
}

fn not_found(kind: &'static str, id: Uuid) -> Error {
    Error::NotFound {
        kind,
        id: id.to_string(),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}", Uuid::new_v4().simple()));
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}
