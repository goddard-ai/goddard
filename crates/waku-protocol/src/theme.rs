//! Process-neutral theme settings persisted in the desktop settings file.
//!
//! The file holds a mode — follow the OS appearance or pin one side — plus
//! the palette chosen for each polarity. Builds before the named-theme
//! picker wrote a single string (`"system"`, `"dark"`, `"gruvbox_dark"`, …);
//! [`ThemeSettings`]'s deserializer still accepts that form and migrates it.

use serde::{Deserialize, Deserializer, Serialize};

/// Which palette slot is live.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    /// Follow the OS appearance, switching between the two slots.
    #[default]
    System,
    /// Always use the light slot.
    Light,
    /// Always use the dark slot.
    Dark,
}

/// A named palette. Each theme belongs to exactly one polarity — the
/// settings pickers are filtered by [`ThemeName::is_dark`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeName {
    #[serde(rename = "light")]
    DefaultLight,
    #[serde(rename = "dark")]
    DefaultDark,
    GruvboxLightHard,
    GruvboxDark,
    EverforestDark,
    EverforestLight,
    /// Kanagawa's light variant is officially named Lotus.
    KanagawaLight,
    ZenburnDark,
    PoimandresDark,
    GithubLight,
    GithubDark,
    DraculaDark,
    RosePineDawn,
    RosePineMoon,
    KansoZen,
    KansoPearl,
    WarmBurnoutLight,
    WarmBurnoutDark,
}

impl ThemeName {
    /// Light-slot choices, in picker order.
    pub const LIGHT: [Self; 8] = [
        Self::DefaultLight,
        Self::GithubLight,
        Self::GruvboxLightHard,
        Self::RosePineDawn,
        Self::EverforestLight,
        Self::KanagawaLight,
        Self::KansoPearl,
        Self::WarmBurnoutLight,
    ];
    /// Dark-slot choices, in picker order.
    pub const DARK: [Self; 10] = [
        Self::DefaultDark,
        Self::DraculaDark,
        Self::GithubDark,
        Self::GruvboxDark,
        Self::RosePineMoon,
        Self::EverforestDark,
        Self::ZenburnDark,
        Self::PoimandresDark,
        Self::KansoZen,
        Self::WarmBurnoutDark,
    ];

    pub fn is_dark(self) -> bool {
        Self::DARK.contains(&self)
    }

    pub fn label(self) -> String {
        match self {
            Self::DefaultLight => crate::i18n::translate("settings.theme_light"),
            Self::DefaultDark => crate::i18n::translate("settings.theme_dark"),
            // Proper nouns stay untranslated.
            Self::GruvboxLightHard => "Gruvbox Light Hard".to_owned(),
            Self::GruvboxDark => "Gruvbox Dark".to_owned(),
            Self::EverforestDark => "Everforest Dark".to_owned(),
            Self::EverforestLight => "Everforest Light".to_owned(),
            Self::KanagawaLight => "Kanagawa Light".to_owned(),
            Self::ZenburnDark => "Zenburn".to_owned(),
            Self::PoimandresDark => "Poimandres".to_owned(),
            Self::GithubLight => "GitHub Light".to_owned(),
            Self::GithubDark => "GitHub Dark".to_owned(),
            Self::DraculaDark => "Dracula".to_owned(),
            Self::RosePineDawn => "Rosé Pine Dawn".to_owned(),
            Self::RosePineMoon => "Rosé Pine Moon".to_owned(),
            Self::KansoZen => "Kansō Zen".to_owned(),
            Self::KansoPearl => "Kansō Pearl".to_owned(),
            Self::WarmBurnoutLight => "Warm Burnout Light".to_owned(),
            Self::WarmBurnoutDark => "Warm Burnout Dark".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ThemeSettings {
    pub mode: ThemeMode,
    /// Palette used while the light slot is live.
    pub light: ThemeName,
    /// Palette used while the dark slot is live.
    pub dark: ThemeName,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            mode: ThemeMode::System,
            light: ThemeName::DefaultLight,
            dark: ThemeName::DefaultDark,
        }
    }
}

/// The single-string form older builds wrote for `theme`.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyThemePreference {
    System,
    Light,
    Dark,
    GruvboxLightHard,
    GruvboxDark,
    EverforestDark,
    EverforestLight,
    KanagawaLight,
    ZenburnDark,
    /// A value written by a newer build; resolve like `System`.
    #[serde(other)]
    Unknown,
}

impl ThemeSettings {
    /// A legacy named theme pinned the app to it outright; preserve that by
    /// pinning the mode to the theme's own side.
    fn from_legacy(legacy: LegacyThemePreference) -> Self {
        use LegacyThemePreference as Legacy;
        let name = match legacy {
            Legacy::System | Legacy::Unknown => return Self::default(),
            Legacy::Light => ThemeName::DefaultLight,
            Legacy::Dark => ThemeName::DefaultDark,
            Legacy::GruvboxLightHard => ThemeName::GruvboxLightHard,
            Legacy::GruvboxDark => ThemeName::GruvboxDark,
            Legacy::EverforestDark => ThemeName::EverforestDark,
            Legacy::EverforestLight => ThemeName::EverforestLight,
            Legacy::KanagawaLight => ThemeName::KanagawaLight,
            Legacy::ZenburnDark => ThemeName::ZenburnDark,
        };
        let mut settings = Self {
            mode: if name.is_dark() {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            ..Self::default()
        };
        if name.is_dark() {
            settings.dark = name;
        } else {
            settings.light = name;
        }
        settings
    }
}

/// A field value a newer build may have written: keep what parses, drop what
/// doesn't so the whole settings read survives it.
fn maybe_known<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Maybe<T> {
        Known(T),
        Ignored(serde::de::IgnoredAny),
    }
    Ok(match Maybe::deserialize(deserializer)? {
        Maybe::Known(value) => Some(value),
        Maybe::Ignored(_) => None,
    })
}

impl<'de> Deserialize<'de> for ThemeSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            /// The current object form; every field is optional.
            Current {
                #[serde(default, deserialize_with = "maybe_known")]
                mode: Option<ThemeMode>,
                #[serde(default, deserialize_with = "maybe_known")]
                light: Option<ThemeName>,
                #[serde(default, deserialize_with = "maybe_known")]
                dark: Option<ThemeName>,
            },
            /// The legacy string form.
            Legacy(LegacyThemePreference),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Current { mode, light, dark } => {
                let defaults = ThemeSettings::default();
                Ok(ThemeSettings {
                    mode: mode.unwrap_or(defaults.mode),
                    light: light.unwrap_or(defaults.light),
                    dark: dark.unwrap_or(defaults.dark),
                })
            }
            Repr::Legacy(legacy) => Ok(ThemeSettings::from_legacy(legacy)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_object_form_round_trips() {
        let settings = ThemeSettings {
            mode: ThemeMode::System,
            light: ThemeName::KanagawaLight,
            dark: ThemeName::ZenburnDark,
        };
        let value = serde_json::to_value(settings).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "mode": "system",
                "light": "kanagawa_light",
                "dark": "zenburn_dark"
            })
        );
        assert_eq!(
            serde_json::from_value::<ThemeSettings>(value).unwrap(),
            settings
        );
    }

    #[test]
    fn legacy_strings_migrate_to_their_polarity() {
        assert_eq!(
            serde_json::from_str::<ThemeSettings>(r#""system""#).unwrap(),
            ThemeSettings::default()
        );
        assert_eq!(
            serde_json::from_str::<ThemeSettings>(r#""dark""#).unwrap(),
            ThemeSettings {
                mode: ThemeMode::Dark,
                ..ThemeSettings::default()
            }
        );
        assert_eq!(
            serde_json::from_str::<ThemeSettings>(r#""gruvbox_dark""#).unwrap(),
            ThemeSettings {
                mode: ThemeMode::Dark,
                dark: ThemeName::GruvboxDark,
                ..ThemeSettings::default()
            }
        );
        assert_eq!(
            serde_json::from_str::<ThemeSettings>(r#""kanagawa_light""#).unwrap(),
            ThemeSettings {
                mode: ThemeMode::Light,
                light: ThemeName::KanagawaLight,
                ..ThemeSettings::default()
            }
        );
    }

    #[test]
    fn values_from_other_builds_fall_back_instead_of_failing() {
        assert_eq!(
            serde_json::from_str::<ThemeSettings>(r#""some_future_theme""#).unwrap(),
            ThemeSettings::default()
        );
        assert_eq!(
            serde_json::from_str::<ThemeSettings>(
                r#"{"mode":"system","light":"some_future_theme","dark":"zenburn_dark"}"#
            )
            .unwrap(),
            ThemeSettings {
                dark: ThemeName::ZenburnDark,
                ..ThemeSettings::default()
            }
        );
    }
}
