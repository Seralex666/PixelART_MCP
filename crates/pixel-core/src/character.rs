use crate::{ColorDepth, Error, Palette, Result, Sprite};
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
    /// Farbvarianten (Skins, Teams), die für die Figur und alle Animationen gelten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<crate::variants::ColorVariant>,
}

impl Character {
    /// Legt eine leere Figur an. Die Palette wird auf die Farbtiefe gebracht und gesperrt.
    pub fn new(
        name: impl Into<String>,
        width: u32,
        height: u32,
        depth: ColorDepth,
        palette: Palette,
    ) -> Result<Self> {
        let mut palette = palette;
        palette.conform_to(depth);
        if palette.is_empty() {
            return Err(Error::EmptyPalette);
        }
        palette.locked = true;
        let mut base = Sprite::new(width, height, depth, palette)?;
        base.rename_layer(0, crate::lang::tr("Körper", "body"))?;
        Ok(Self::with_base(name.into(), base))
    }

    /// Erstellt eine Figur aus einem fertigen Sprite (z. B. von Claude gezeichnet).
    ///
    /// Die Palette wird auf die Farbtiefe gebracht, um alle im Sprite benutzten
    /// Farben ergänzt und dann gesperrt. So liegt jeder vorhandene Pixel sicher in
    /// der Palette, und kein Pixel ändert beim Übermalen unerwartet seine Farbe.
    /// Hat das Sprite mehr Farben, als eine Palette fasst, schlägt das fehl.
    pub fn from_sprite(name: impl Into<String>, sprite: Sprite) -> Result<Self> {
        sprite.check()?;
        let mut base = sprite;
        base.palette.locked = false;
        // Pixel und Palette auf die Farbtiefe bringen, dann fehlende Farben aufnehmen.
        base.set_depth(base.depth);
        for c in base.used_colors() {
            base.palette.add(c)?;
        }
        if base.palette.is_empty() {
            return Err(Error::EmptyPalette);
        }
        base.palette.locked = true;
        Ok(Self::with_base(name.into(), base))
    }

    fn with_base(name: String, base: Sprite) -> Self {
        let (w, h) = (base.width(), base.height());
        Self {
            id: Uuid::new_v4(),
            name,
            description: String::new(),
            base,
            anchor: (w / 2, h.saturating_sub(1)),
            tags: Vec::new(),
            variants: Vec::new(),
        }
    }

    pub fn palette(&self) -> &Palette {
        &self.base.palette
    }

    pub fn size(&self) -> (u32, u32) {
        (self.base.width(), self.base.height())
    }
}
