use crate::{ColorDepth, Palette, Result, Sprite};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Eine Figur als festes Asset.
///
/// Die Figur besitzt ihre Grundpose (`base`), eine **gesperrte** Palette und
/// eine feste Größe. Animationen werden aus der Figur abgeleitet und bei
/// [`crate::Project::validate`] gegen sie geprüft. So bleibt die Figur
/// projektübergreifend gleich, egal ob du oder Claude neue Animationen zeichnet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Character {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Grundpose (meist ein Frame, beliebig viele Ebenen).
    pub base: Sprite,
    /// Bezugspunkt der Figur (z. B. Fußmitte), für Engines und für Animationen.
    pub anchor: (u32, u32),
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Character {
    pub fn new(
        name: impl Into<String>,
        width: u32,
        height: u32,
        depth: ColorDepth,
        palette: Palette,
    ) -> Result<Self> {
        let mut palette = palette;
        palette.locked = true;
        let mut base = Sprite::new(width, height, depth, palette)?;
        base.rename_layer(0, "Körper")?;
        Ok(Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: String::new(),
            base,
            anchor: (width / 2, height.saturating_sub(1)),
            tags: Vec::new(),
        })
    }

    /// Erstellt eine Figur aus einem fertigen Sprite (z. B. von Claude gezeichnet).
    /// Die Palette des Sprites wird gesperrt.
    pub fn from_sprite(name: impl Into<String>, sprite: Sprite) -> Result<Self> {
        sprite.check()?;
        let mut base = sprite;
        base.palette.locked = true;
        let (w, h) = (base.width(), base.height());
        Ok(Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: String::new(),
            base,
            anchor: (w / 2, h.saturating_sub(1)),
            tags: Vec::new(),
        })
    }

    pub fn palette(&self) -> &Palette {
        &self.base.palette
    }

    pub fn size(&self) -> (u32, u32) {
        (self.base.width(), self.base.height())
    }
}
