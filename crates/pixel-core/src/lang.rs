//! Sprache der Meldungen: Deutsch oder Englisch.
//!
//! Die Sprache gilt für den ganzen Prozess. Der Editor stellt sie nach den
//! Einstellungen ein, der MCP-Server immer auf Englisch, weil Sprachmodelle
//! englische Werkzeugtexte am verlässlichsten lesen.
//!
//! Texte stehen an Ort und Stelle in beiden Sprachen, [`tr`] wählt aus:
//! `tr("Speichern", "Save")`. Für Texte mit Werten gibt es [`trf!`](crate::trf),
//! das wie `format!` arbeitet: `trf!("{n} Farben", "{n} colors")`.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    De,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::De, Lang::En];

    /// Kürzel für Einstellungsdateien: "de" oder "en".
    pub fn code(self) -> &'static str {
        match self {
            Lang::De => "de",
            Lang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "de" | "deutsch" | "german" => Some(Lang::De),
            "en" | "english" | "englisch" => Some(Lang::En),
            _ => None,
        }
    }

    /// Der Name der Sprache in ihr selbst.
    pub fn name(self) -> &'static str {
        match self {
            Lang::De => "Deutsch",
            Lang::En => "English",
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Stellt die Sprache für den ganzen Prozess ein.
pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn get() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::De,
    }
}

pub fn is_en() -> bool {
    get() == Lang::En
}

/// Wählt den Text in der eingestellten Sprache.
pub fn tr(de: &'static str, en: &'static str) -> &'static str {
    if is_en() {
        en
    } else {
        de
    }
}

/// Wie `format!`, aber mit einem deutschen und einem englischen Text.
#[macro_export]
macro_rules! trf {
    ($de:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::lang::is_en() {
            format!($en $(, $arg)*)
        } else {
            format!($de $(, $arg)*)
        }
    };
}
