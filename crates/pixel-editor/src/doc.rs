//! Das geöffnete Projekt im Editor: Bearbeiten, Rückgängig, Speichern und der
//! Abgleich mit Änderungen, die der MCP-Server in dieselbe Datei schreibt.

use pixel_core::workspace::{check_asset_name, ExportFormat, Exported, NameClash, Workspace};
use pixel_core::{Animation, AnimationKind, Character, ColorDepth, Palette, Project, Rgba, Sprite};
use std::time::SystemTime;
use uuid::Uuid;

pub type Res<T = ()> = Result<T, String>;

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// So viele Schritte lassen sich höchstens rückgängig machen.
const MAX_UNDO_STEPS: usize = 100;
/// Speichergrenze für die Rückgängig-Liste (große Bilder brauchen viel Platz).
const MAX_UNDO_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetKind {
    Character,
    Animation,
    Image,
}

impl AssetKind {
    pub fn label(self) -> &'static str {
        match self {
            AssetKind::Character => "Figur",
            AssetKind::Animation => "Animation",
            AssetKind::Image => "Bild",
        }
    }
}

/// Verweis auf ein Asset im Projekt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AssetId {
    pub kind: AssetKind,
    pub id: Uuid,
}

impl AssetId {
    pub fn new(kind: AssetKind, id: Uuid) -> Self {
        Self { kind, id }
    }
}

/// Zeile für die Asset-Liste.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetEntry {
    pub id: AssetId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub frames: usize,
    pub bits: u8,
    /// Bei Animationen: die Figur, zu der sie gehört.
    pub character: Option<Uuid>,
}

/// Was sich geändert hat. Beim Speichern wird nur das in die Datei übernommen,
/// falls der MCP-Server sie inzwischen ebenfalls geändert hat.
enum Change {
    Upsert(Vec<AssetId>),
    Remove(AssetId),
}

struct UndoStep {
    asset: AssetId,
    sprite: Sprite,
}

/// Änderungszeit und Größe der Projektdatei.
type Stamp = Option<(SystemTime, u64)>;

pub struct Doc {
    ws: Workspace,
    name: String,
    project: Project,
    stamp: Stamp,
    undo: Vec<UndoStep>,
    redo: Vec<UndoStep>,
    revision: u64,
}

fn sprite_of(p: &Project, id: AssetId) -> Option<&Sprite> {
    match id.kind {
        AssetKind::Character => p.character(id.id).ok().map(|c| &c.base),
        AssetKind::Animation => p.animation(id.id).ok().map(|a| &a.sprite),
        AssetKind::Image => p.image(id.id).ok().map(|i| &i.sprite),
    }
}

fn sprite_bytes(s: &Sprite) -> usize {
    s.frames()
        .iter()
        .map(|f| f.layers.len() * s.width() as usize * s.height() as usize * 4)
        .sum()
}

impl Doc {
    pub fn open(ws: &Workspace, name: &str) -> Res<Self> {
        let project = ws.load(name).map_err(e)?;
        let mut doc = Self {
            ws: ws.clone(),
            name: name.trim().to_string(),
            project,
            stamp: None,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        };
        doc.stamp = doc.disk_stamp();
        Ok(doc)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Zählt bei jeder Änderung hoch; die Oberfläche erneuert damit ihre Texturen.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn disk_stamp(&self) -> Stamp {
        let path = self.ws.project_path(&self.name).ok()?;
        let meta = std::fs::metadata(path).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    // ------------------------------------------------------------ Lesen

    pub fn assets(&self) -> Vec<AssetEntry> {
        let entry = |id: AssetId, name: &str, s: &Sprite, character: Option<Uuid>| AssetEntry {
            id,
            name: name.to_string(),
            width: s.width(),
            height: s.height(),
            frames: s.frames().len(),
            bits: s.depth.bits(),
            character,
        };
        let p = &self.project;
        let mut out = Vec::new();
        for c in &p.characters {
            out.push(entry(
                AssetId::new(AssetKind::Character, c.id),
                &c.name,
                &c.base,
                None,
            ));
        }
        for a in &p.animations {
            out.push(entry(
                AssetId::new(AssetKind::Animation, a.id),
                &a.name,
                &a.sprite,
                Some(a.character_id),
            ));
        }
        for i in &p.images {
            out.push(entry(
                AssetId::new(AssetKind::Image, i.id),
                &i.name,
                &i.sprite,
                None,
            ));
        }
        out
    }

    pub fn sprite(&self, id: AssetId) -> Option<&Sprite> {
        sprite_of(&self.project, id)
    }

    pub fn asset_name(&self, id: AssetId) -> Option<&str> {
        let p = &self.project;
        match id.kind {
            AssetKind::Character => p.character(id.id).ok().map(|c| c.name.as_str()),
            AssetKind::Animation => p.animation(id.id).ok().map(|a| a.name.as_str()),
            AssetKind::Image => p.image(id.id).ok().map(|i| i.name.as_str()),
        }
    }

    /// Die Figur, deren gesperrte Palette für dieses Asset gilt.
    pub fn character_of(&self, id: AssetId) -> Option<Uuid> {
        match id.kind {
            AssetKind::Character => Some(id.id),
            AssetKind::Animation => self.project.animation(id.id).ok().map(|a| a.character_id),
            AssetKind::Image => None,
        }
    }

    fn sprite_mut(&mut self, id: AssetId) -> Res<&mut Sprite> {
        let p = &mut self.project;
        match id.kind {
            AssetKind::Character => p.character_mut(id.id).map(|c| &mut c.base),
            AssetKind::Animation => p.animation_mut(id.id).map(|a| &mut a.sprite),
            AssetKind::Image => p.image_mut(id.id).map(|i| &mut i.sprite),
        }
        .map_err(e)
    }

    // ------------------------------------------------------------ Bearbeiten

    /// Merkt den jetzigen Stand eines Assets für „Rückgängig“.
    fn snapshot(&mut self, id: AssetId) -> Res {
        let sprite = self
            .sprite(id)
            .ok_or("Das Asset gibt es nicht mehr")?
            .clone();
        self.undo.push(UndoStep { asset: id, sprite });
        self.redo.clear();
        self.trim_undo();
        Ok(())
    }

    fn trim_undo(&mut self) {
        let mut bytes: usize = self.undo.iter().map(|u| sprite_bytes(&u.sprite)).sum();
        while self.undo.len() > 1 && (self.undo.len() > MAX_UNDO_STEPS || bytes > MAX_UNDO_BYTES) {
            bytes -= sprite_bytes(&self.undo.remove(0).sprite);
        }
    }

    /// Beginnt einen Pinselstrich: Stand merken, danach wird über
    /// [`Self::stroke_sprite`] gezeichnet und mit [`Self::end_stroke`] gespeichert.
    pub fn begin_stroke(&mut self, id: AssetId) -> Res {
        self.poll(Some(id));
        self.snapshot(id)
    }

    /// Das Sprite während eines Strichs. Jeder Zugriff gilt als Änderung.
    pub fn stroke_sprite(&mut self, id: AssetId) -> Res<&mut Sprite> {
        self.revision += 1;
        self.sprite_mut(id)
    }

    /// Beendet einen Strich und speichert. Hat sich nichts geändert, fällt der
    /// gemerkte Stand wieder weg.
    pub fn end_stroke(&mut self, id: AssetId) -> Res {
        let unchanged = match (self.undo.last(), self.sprite(id)) {
            (Some(step), Some(now)) => step.asset == id && &step.sprite == now,
            _ => false,
        };
        if unchanged {
            self.undo.pop();
            return Ok(());
        }
        self.commit(Change::Upsert(vec![id]))
    }

    /// Eine einzelne Änderung an einem Asset, mit Rückgängig und Speichern.
    pub fn edit<T>(&mut self, id: AssetId, f: impl FnOnce(&mut Sprite) -> Res<T>) -> Res<T> {
        self.begin_stroke(id)?;
        match f(self.stroke_sprite(id)?) {
            Ok(value) => {
                self.end_stroke(id)?;
                Ok(value)
            }
            Err(err) => {
                // Halb ausgeführte Änderung zurücknehmen.
                if let Some(step) = self.undo.pop() {
                    if let Ok(s) = self.sprite_mut(id) {
                        *s = step.sprite;
                    }
                }
                Err(err)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Macht den letzten Schritt rückgängig und gibt das betroffene Asset zurück.
    pub fn undo(&mut self) -> Res<Option<AssetId>> {
        self.step(true)
    }

    pub fn redo(&mut self) -> Res<Option<AssetId>> {
        self.step(false)
    }

    fn step(&mut self, undo: bool) -> Res<Option<AssetId>> {
        self.poll(None);
        loop {
            let step = if undo {
                self.undo.pop()
            } else {
                self.redo.pop()
            };
            let Some(UndoStep { asset, mut sprite }) = step else {
                return Ok(None);
            };
            // Das Asset kann inzwischen gelöscht worden sein: nächsten Schritt nehmen.
            let Ok(current) = self.sprite_mut(asset) else {
                continue;
            };
            // Eine gesperrte Palette wächst nur; der ältere Stand passt immer hinein.
            if current.palette.locked {
                sprite.palette = current.palette.clone();
            }
            let previous = std::mem::replace(current, sprite);
            let other = UndoStep {
                asset,
                sprite: previous,
            };
            if undo {
                self.redo.push(other);
            } else {
                self.undo.push(other);
            }
            self.commit(Change::Upsert(vec![asset]))?;
            return Ok(Some(asset));
        }
    }

    // ------------------------------------------------------------ Datei

    /// Lädt das Projekt neu, wenn die Datei von außen geändert wurde (z. B. weil
    /// Claude über den MCP-Server gezeichnet hat). Hat sich dabei `watch`
    /// geändert, lässt sich das mit „Rückgängig“ zurücknehmen.
    pub fn poll(&mut self, watch: Option<AssetId>) -> bool {
        let now = self.disk_stamp();
        if now == self.stamp {
            return false;
        }
        if now.is_none() {
            // Datei verschwunden: den Stand im Speicher behalten.
            self.stamp = now;
            return false;
        }
        let Ok(disk) = self.ws.load(&self.name) else {
            return false;
        };
        if let Some(id) = watch {
            if let (Some(old), Some(new)) = (self.sprite(id), sprite_of(&disk, id)) {
                if old != new {
                    let sprite = old.clone();
                    self.undo.push(UndoStep { asset: id, sprite });
                    self.redo.clear();
                    self.trim_undo();
                }
            }
        }
        self.project = disk;
        self.stamp = now;
        self.revision += 1;
        true
    }

    /// Speichert. Wurde die Datei inzwischen von außen geändert, wird nur unsere
    /// Änderung in den neuen Stand übernommen, statt ihn zu überschreiben.
    fn commit(&mut self, change: Change) -> Res {
        if self.disk_stamp() != self.stamp {
            if let Ok(mut disk) = self.ws.load(&self.name) {
                match &change {
                    Change::Upsert(ids) => {
                        for &id in ids {
                            upsert(&mut disk, &self.project, id);
                        }
                    }
                    Change::Remove(id) => remove(&mut disk, *id),
                }
                self.project = disk;
            }
        }
        self.revision += 1;
        self.ws.save(&self.name, &self.project).map_err(e)?;
        self.stamp = self.disk_stamp();
        Ok(())
    }

    // ------------------------------------------------------------ Assets

    fn check_name(&self, name: &str, except: Option<AssetId>) -> Res<String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err("Der Name braucht 1 bis 64 Zeichen".into());
        }
        let taken: Vec<String> = self
            .assets()
            .into_iter()
            .filter(|a| Some(a.id) != except)
            .map(|a| a.name)
            .collect();
        match check_asset_name(&taken, name) {
            Ok(()) => Ok(name.to_string()),
            Err(NameClash::Same(t)) => Err(format!("„{t}“ gibt es in diesem Projekt schon")),
            Err(NameClash::Similar(t)) => Err(format!(
                "„{name}“ ist „{t}“ zu ähnlich: beide ergäben beim Export denselben Dateinamen"
            )),
        }
    }

    pub fn create_image(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        depth: ColorDepth,
        palette: Palette,
    ) -> Res<AssetId> {
        self.poll(None);
        let name = self.check_name(name, None)?;
        let mut palette = palette;
        // Palettenfarben an die Farbtiefe anpassen, damit sie exakt zeichenbar sind.
        palette.conform_to(depth);
        let sprite = Sprite::new(width, height, depth, palette).map_err(e)?;
        let id = AssetId::new(AssetKind::Image, self.project.add_image(name, sprite));
        self.commit(Change::Upsert(vec![id]))?;
        Ok(id)
    }

    pub fn create_character(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        depth: ColorDepth,
        palette: Palette,
    ) -> Res<AssetId> {
        self.poll(None);
        let name = self.check_name(name, None)?;
        let character = Character::new(name, width, height, depth, palette).map_err(e)?;
        let id = AssetId::new(AssetKind::Character, self.project.add_character(character));
        self.commit(Change::Upsert(vec![id]))?;
        Ok(id)
    }

    /// Legt eine Animation an; jeder Frame startet als Kopie der Grundpose der Figur.
    pub fn create_animation(
        &mut self,
        character: Uuid,
        kind: AnimationKind,
        frames: usize,
        name: &str,
    ) -> Res<AssetId> {
        self.poll(None);
        let character = self.project.character(character).map_err(e)?;
        let mut anim = Animation::from_character(character, kind, Some(frames)).map_err(e)?;
        let wanted = if name.trim().is_empty() {
            anim.name.clone()
        } else {
            name.to_string()
        };
        anim.name = self.check_name(&wanted, None)?;
        let id = AssetId::new(AssetKind::Animation, anim.id);
        self.project.animations.push(anim);
        self.commit(Change::Upsert(vec![id]))?;
        Ok(id)
    }

    pub fn rename(&mut self, id: AssetId, name: &str) -> Res {
        self.poll(None);
        let name = self.check_name(name, Some(id))?;
        let p = &mut self.project;
        match id.kind {
            AssetKind::Character => p.character_mut(id.id).map_err(e)?.name = name,
            AssetKind::Animation => p.animation_mut(id.id).map_err(e)?.name = name,
            AssetKind::Image => p.image_mut(id.id).map_err(e)?.name = name,
        }
        self.commit(Change::Upsert(vec![id]))
    }

    /// Löscht ein Asset. Mit einer Figur verschwinden auch ihre Animationen.
    pub fn delete(&mut self, id: AssetId) -> Res {
        self.poll(None);
        remove(&mut self.project, id);
        self.undo.retain(|u| u.asset != id);
        self.redo.retain(|u| u.asset != id);
        self.commit(Change::Remove(id))
    }

    /// Nimmt eine Farbe in die gesperrte Palette einer Figur auf. Sie gilt dann
    /// auch in allen Animationen der Figur.
    pub fn add_character_color(&mut self, character: Uuid, color: Rgba) -> Res {
        self.poll(None);
        self.project
            .add_character_colors(character, &[color])
            .map_err(e)?;
        let mut changed = vec![AssetId::new(AssetKind::Character, character)];
        changed.extend(
            self.project
                .animations_of(character)
                .map(|a| AssetId::new(AssetKind::Animation, a.id)),
        );
        self.commit(Change::Upsert(changed))
    }

    pub fn export(
        &self,
        id: AssetId,
        format: ExportFormat,
        frame: Option<usize>,
        scale: u32,
    ) -> Res<Exported> {
        let sprite = self.sprite(id).ok_or("Das Asset gibt es nicht mehr")?;
        let name = self.asset_name(id).unwrap_or("unnamed");
        self.ws
            .export(&self.name, name, sprite, format, frame, scale)
            .map_err(e)
    }
}

/// Übernimmt ein Asset aus `from` nach `into` (ersetzen oder anhängen).
fn upsert(into: &mut Project, from: &Project, id: AssetId) {
    match id.kind {
        AssetKind::Character => {
            if let Ok(ours) = from.character(id.id) {
                match into.character_mut(id.id) {
                    Ok(theirs) => *theirs = ours.clone(),
                    Err(_) => into.characters.push(ours.clone()),
                }
            }
        }
        AssetKind::Animation => {
            if let Ok(ours) = from.animation(id.id) {
                match into.animation_mut(id.id) {
                    Ok(theirs) => *theirs = ours.clone(),
                    Err(_) => into.animations.push(ours.clone()),
                }
            }
        }
        AssetKind::Image => {
            if let Ok(ours) = from.image(id.id) {
                match into.image_mut(id.id) {
                    Ok(theirs) => *theirs = ours.clone(),
                    Err(_) => into.images.push(ours.clone()),
                }
            }
        }
    }
}

fn remove(p: &mut Project, id: AssetId) {
    // Fehlt das Asset schon, ist das Ziel erreicht.
    match id.kind {
        AssetKind::Character => drop(p.remove_character(id.id)),
        AssetKind::Animation => drop(p.remove_animation(id.id)),
        AssetKind::Image => drop(p.remove_image(id.id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixel_core::draw::{self, Target};

    const RED: Rgba = Rgba::rgb(255, 0, 0);
    const T: Target = Target::new(0, 0);

    /// Arbeitsordner in einem frischen Temp-Verzeichnis mit einem Projekt "Test".
    fn workspace() -> Workspace {
        let dir = std::env::temp_dir().join(format!("pixelart-editor-test-{}", Uuid::new_v4()));
        let ws = Workspace::open(dir).unwrap();
        ws.create("Test").unwrap();
        ws
    }

    fn dot(s: &mut Sprite, x: i64, y: i64) -> Res {
        draw::pixels(s, T, &[(x, y)], RED).map_err(e)
    }

    fn pixel(doc: &Doc, id: AssetId, x: i64, y: i64) -> Rgba {
        doc.sprite(id)
            .unwrap()
            .layer(0, 0)
            .unwrap()
            .get(x, y)
            .unwrap()
    }

    fn image(doc: &mut Doc, name: &str) -> AssetId {
        doc.create_image(name, 8, 8, ColorDepth::Rgba32, Palette::default_16())
            .unwrap()
    }

    #[test]
    fn edits_are_saved_and_can_be_undone_and_redone() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let id = image(&mut doc, "Bild");
        doc.edit(id, |s| dot(s, 1, 1)).unwrap();
        // Auf der Platte steht dasselbe wie im Editor.
        let disk = Doc::open(&ws, "Test").unwrap();
        assert_eq!(pixel(&disk, id, 1, 1), RED);

        assert_eq!(doc.undo().unwrap(), Some(id));
        assert_eq!(pixel(&doc, id, 1, 1), Rgba::TRANSPARENT);
        assert_eq!(
            pixel(&Doc::open(&ws, "Test").unwrap(), id, 1, 1),
            Rgba::TRANSPARENT
        );
        assert_eq!(doc.redo().unwrap(), Some(id));
        assert_eq!(pixel(&doc, id, 1, 1), RED);
        assert!(!doc.can_redo());
    }

    #[test]
    fn a_failed_or_empty_edit_leaves_no_undo_step() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let id = image(&mut doc, "Bild");
        assert!(doc
            .edit(id, |s| {
                dot(s, 2, 2)?;
                Err::<(), _>("abgebrochen".to_string())
            })
            .is_err());
        assert_eq!(pixel(&doc, id, 2, 2), Rgba::TRANSPARENT);
        assert!(!doc.can_undo());
        // Ein Strich, der nichts ändert, zählt nicht.
        doc.begin_stroke(id).unwrap();
        doc.end_stroke(id).unwrap();
        assert!(!doc.can_undo());
    }

    #[test]
    fn changes_from_outside_are_loaded_and_can_be_undone() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let id = image(&mut doc, "Bild");
        assert!(!doc.poll(Some(id)));

        // Der MCP-Server zeichnet in dieselbe Datei.
        let mut other = Doc::open(&ws, "Test").unwrap();
        other.edit(id, |s| dot(s, 3, 3)).unwrap();

        let before = doc.revision();
        assert!(doc.poll(Some(id)));
        assert!(doc.revision() > before);
        assert_eq!(pixel(&doc, id, 3, 3), RED);
        // „Rückgängig“ nimmt auch die Änderung von außen zurück.
        assert_eq!(doc.undo().unwrap(), Some(id));
        assert_eq!(pixel(&doc, id, 3, 3), Rgba::TRANSPARENT);
    }

    #[test]
    fn saving_keeps_what_was_added_outside_in_the_meantime() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let ours = image(&mut doc, "Unseres");
        doc.begin_stroke(ours).unwrap();

        // Während des Strichs legt der MCP-Server ein Bild an und zeichnet darin.
        let mut other = Doc::open(&ws, "Test").unwrap();
        let theirs = image(&mut other, "Fremdes");
        other.edit(theirs, |s| dot(s, 4, 4)).unwrap();

        dot(doc.stroke_sprite(ours).unwrap(), 5, 5).unwrap();
        doc.end_stroke(ours).unwrap();

        let disk = Doc::open(&ws, "Test").unwrap();
        assert_eq!(pixel(&disk, ours, 5, 5), RED);
        assert_eq!(pixel(&disk, theirs, 4, 4), RED);
        // Auch der Editor selbst kennt das fremde Bild jetzt.
        assert_eq!(pixel(&doc, theirs, 4, 4), RED);
    }

    #[test]
    fn names_must_be_unique_also_as_file_names() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let id = image(&mut doc, "Tile A");
        let make = |doc: &mut Doc, name: &str| {
            doc.create_image(name, 8, 8, ColorDepth::Rgba32, Palette::default_16())
        };
        assert!(make(&mut doc, "tile a").unwrap_err().contains("schon"));
        assert!(make(&mut doc, "Tile_A").unwrap_err().contains("ähnlich"));
        assert!(make(&mut doc, "  ").is_err());
        // Umbenennen auf den eigenen Namen (andere Schreibweise) ist erlaubt.
        doc.rename(id, "TILE A").unwrap();
        assert_eq!(doc.asset_name(id), Some("TILE A"));
    }

    #[test]
    fn character_palette_grows_for_all_animations_and_survives_undo() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let hero = doc
            .create_character("Held", 8, 8, ColorDepth::Rgba32, Palette::grayscale(4))
            .unwrap();
        let walk = doc
            .create_animation(hero.id, AnimationKind::Walk, 3, "")
            .unwrap();
        assert_eq!(doc.asset_name(walk), Some("Held walk"));
        assert_eq!(doc.sprite(walk).unwrap().frames().len(), 3);

        // Rot gibt es in der Palette nicht: gezeichnet wird die ähnlichste Farbe.
        doc.edit(walk, |s| dot(s, 0, 0)).unwrap();
        assert_ne!(pixel(&doc, walk, 0, 0), RED);

        doc.add_character_color(hero.id, RED).unwrap();
        assert!(doc.sprite(hero).unwrap().palette.contains(RED));
        assert!(doc.sprite(walk).unwrap().palette.contains(RED));
        doc.edit(walk, |s| dot(s, 0, 0)).unwrap();
        assert_eq!(pixel(&doc, walk, 0, 0), RED);

        // Rückgängig bis vor die neue Farbe: Die Palette bleibt gewachsen,
        // damit Figur und Animation weiter zusammenpassen.
        doc.undo().unwrap();
        doc.undo().unwrap();
        assert!(doc.sprite(walk).unwrap().palette.contains(RED));
        assert!(doc.project().validate().is_empty());
    }

    #[test]
    fn deleting_a_character_removes_its_animations() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let hero = doc
            .create_character("Held", 8, 8, ColorDepth::Indexed8, Palette::default_16())
            .unwrap();
        let idle = doc
            .create_animation(hero.id, AnimationKind::Idle, 2, "Warten")
            .unwrap();
        doc.edit(idle, |s| dot(s, 1, 1)).unwrap();
        doc.delete(hero).unwrap();
        assert!(doc.assets().is_empty());
        assert!(Doc::open(&ws, "Test").unwrap().assets().is_empty());
        // Schritte zu gelöschten Assets sind weg.
        assert_eq!(doc.undo().unwrap(), None);
    }

    #[test]
    fn export_writes_into_the_project_folder() {
        let ws = workspace();
        let mut doc = Doc::open(&ws, "Test").unwrap();
        let id = image(&mut doc, "Mein Bild");
        doc.edit(id, |s| dot(s, 0, 0)).unwrap();
        let out = doc.export(id, ExportFormat::Png, None, 4).unwrap();
        assert_eq!(
            out.files,
            vec![ws.exports_dir("Test").join("Mein_Bild@4x.png")]
        );
        let (w, h, _) = pixel_core::export::decode(&std::fs::read(&out.files[0]).unwrap()).unwrap();
        assert_eq!((w, h), (32, 32));
    }
}
