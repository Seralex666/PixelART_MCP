use crate::{Character, Result, Sprite};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Gängige Animationsarten mit sinnvollen Standardwerten.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationKind {
    Idle,
    Walk,
    Run,
    Jump,
    Attack,
    Hurt,
    Death,
    Custom(String),
}

impl AnimationKind {
    pub const PRESETS: [AnimationKind; 7] = [
        AnimationKind::Idle,
        AnimationKind::Walk,
        AnimationKind::Run,
        AnimationKind::Jump,
        AnimationKind::Attack,
        AnimationKind::Hurt,
        AnimationKind::Death,
    ];

    pub fn name(&self) -> &str {
        match self {
            AnimationKind::Idle => "idle",
            AnimationKind::Walk => "walk",
            AnimationKind::Run => "run",
            AnimationKind::Jump => "jump",
            AnimationKind::Attack => "attack",
            AnimationKind::Hurt => "hurt",
            AnimationKind::Death => "death",
            AnimationKind::Custom(n) => n,
        }
    }

    /// Empfohlene Anzahl Frames.
    pub fn default_frames(&self) -> usize {
        match self {
            AnimationKind::Idle => 4,
            AnimationKind::Walk => 8,
            AnimationKind::Run => 6,
            AnimationKind::Jump => 5,
            AnimationKind::Attack => 6,
            AnimationKind::Hurt => 3,
            AnimationKind::Death => 7,
            AnimationKind::Custom(_) => 4,
        }
    }

    /// Empfohlene Dauer pro Frame in Millisekunden.
    pub fn default_frame_ms(&self) -> u32 {
        match self {
            AnimationKind::Idle => 200,
            AnimationKind::Walk => 100,
            AnimationKind::Run => 80,
            AnimationKind::Jump => 100,
            AnimationKind::Attack => 70,
            AnimationKind::Hurt => 100,
            AnimationKind::Death => 120,
            AnimationKind::Custom(_) => 100,
        }
    }

    pub fn loops_by_default(&self) -> bool {
        matches!(
            self,
            AnimationKind::Idle | AnimationKind::Walk | AnimationKind::Run
        )
    }

    /// Kurze Beschreibung der typischen Bewegung, als Hilfe für Claude und im
    /// Editor, in der eingestellten Sprache.
    pub fn guide(&self) -> &'static str {
        use crate::lang::tr;
        match self {
            AnimationKind::Idle => tr(
                "Ruhepose mit leichtem Atmen: Oberkörper hebt und senkt sich um 1 Pixel.",
                "Resting pose with light breathing: the upper body rises and falls by 1 pixel.",
            ),
            AnimationKind::Walk => tr(
                "Gehzyklus: Kontakt, Tiefpunkt, Durchgang, Hochpunkt, dann dasselbe mit dem anderen Bein.",
                "Walk cycle: contact, down, passing, up, then the same with the other leg.",
            ),
            AnimationKind::Run => tr(
                "Laufzyklus mit Flugphase: Körper nach vorn geneigt, Arme schwingen stark gegengleich.",
                "Run cycle with a flight phase: body leaning forward, arms swinging strongly in opposition.",
            ),
            AnimationKind::Jump => tr(
                "Hocke, Absprung, Steigen, Scheitelpunkt, Landung.",
                "Crouch, take-off, rise, apex, landing.",
            ),
            AnimationKind::Attack => tr(
                "Ausholen, Schlag mit Bewegungsbogen, Treffer, Zurückkehren in die Grundpose.",
                "Wind-up, strike with a motion arc, hit, return to the base pose.",
            ),
            AnimationKind::Hurt => tr(
                "Rückstoß nach hinten, kurzer Farbblitz, Rückkehr.",
                "Knock-back, short color flash, return.",
            ),
            AnimationKind::Death => tr(
                "Taumeln, Fallen, Aufprall, am Boden liegen bleiben.",
                "Stagger, fall, impact, stay on the ground.",
            ),
            AnimationKind::Custom(_) => tr("Eigene Animation.", "Custom animation."),
        }
    }

    pub fn parse(s: &str) -> AnimationKind {
        match s.to_ascii_lowercase().as_str() {
            "idle" => AnimationKind::Idle,
            "walk" => AnimationKind::Walk,
            "run" => AnimationKind::Run,
            "jump" => AnimationKind::Jump,
            "attack" | "fight" => AnimationKind::Attack,
            "hurt" | "hit" => AnimationKind::Hurt,
            "death" | "die" => AnimationKind::Death,
            _ => AnimationKind::Custom(s.to_string()),
        }
    }
}

/// Eine Animation einer Figur. Sie hat immer die Größe und die Palette der Figur.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub id: Uuid,
    pub name: String,
    pub kind: AnimationKind,
    pub character_id: Uuid,
    pub looping: bool,
    pub sprite: Sprite,
}

impl Animation {
    /// Legt eine Animation an. Jeder Frame startet als Kopie der Grundpose,
    /// damit die Figur von Anfang an exakt dieselbe ist.
    pub fn from_character(
        character: &Character,
        kind: AnimationKind,
        frames: Option<usize>,
    ) -> Result<Self> {
        let n = frames.unwrap_or_else(|| kind.default_frames()).max(1);
        let mut sprite = character.base.clone();
        // Nur den ersten Frame der Grundpose als Startbild verwenden.
        while sprite.frames().len() > 1 {
            let last = sprite.frames().len() - 1;
            sprite.remove_frame(last)?;
        }
        sprite.frame_mut(0)?.duration_ms = kind.default_frame_ms();
        for _ in 1..n {
            sprite.duplicate_frame(0)?;
        }
        Ok(Self {
            id: Uuid::new_v4(),
            name: format!("{} {}", character.name, kind.name()),
            looping: kind.loops_by_default(),
            kind,
            character_id: character.id,
            sprite,
        })
    }

    pub fn total_duration_ms(&self) -> u32 {
        self.sprite.frames().iter().map(|f| f.duration_ms).sum()
    }
}
