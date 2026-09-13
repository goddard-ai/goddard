//! Process-neutral theme preference persisted in the desktop settings file.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
    GruvboxLightHard,
    GruvboxDark,
    EverforestDark,
    EverforestLight,
    /// Kanagawa's light variant is officially named Lotus.
    KanagawaLight,
    ZenburnDark,
    /// A value written by a newer build; resolves like `System` rather than
    /// failing the whole settings read.
    #[serde(other)]
    Unknown,
}

impl ThemePreference {
    pub const ALL: [Self; 9] = [
        Self::System,
        Self::Light,
        Self::Dark,
        Self::GruvboxLightHard,
        Self::GruvboxDark,
        Self::EverforestDark,
        Self::EverforestLight,
        Self::KanagawaLight,
        Self::ZenburnDark,
    ];

    pub fn label(self) -> String {
        match self {
            Self::System | Self::Unknown => crate::i18n::translate("settings.theme_system"),
            Self::Light => crate::i18n::translate("settings.theme_light"),
            Self::Dark => crate::i18n::translate("settings.theme_dark"),
            // Proper nouns stay untranslated.
            Self::GruvboxLightHard => "Gruvbox Light Hard".to_owned(),
            Self::GruvboxDark => "Gruvbox Dark".to_owned(),
            Self::EverforestDark => "Everforest Dark".to_owned(),
            Self::EverforestLight => "Everforest Light".to_owned(),
            Self::KanagawaLight => "Kanagawa Light".to_owned(),
            Self::ZenburnDark => "Zenburn".to_owned(),
        }
    }
}
