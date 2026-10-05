//! Shared locale selection and translation access.

use serde::{Deserialize, Serialize};

/// The language preference Goddard persists. `System` resolves to one of the
/// locales Goddard deliberately ships today.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppLanguage {
    System,
    English,
    SimplifiedChinese,
    Japanese,
}

impl AppLanguage {
    pub const ALL: [Self; 4] = [
        Self::System,
        Self::English,
        Self::SimplifiedChinese,
        Self::Japanese,
    ];

    pub fn locale(self) -> &'static str {
        match self.resolved() {
            Self::System => unreachable!("system language always resolves to a shipped locale"),
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
            Self::Japanese => "ja",
        }
    }

    /// Explicit language names are autonyms so the selector remains
    /// understandable even when the current locale is unfamiliar.
    pub fn label(self) -> String {
        match self {
            Self::System => translate("language.system"),
            Self::English => "English".to_owned(),
            Self::SimplifiedChinese => "简体中文".to_owned(),
            Self::Japanese => "日本語".to_owned(),
        }
    }

    pub fn resolved(self) -> Self {
        match self {
            Self::System => Self::from_system(),
            explicit => explicit,
        }
    }

    fn from_system() -> Self {
        Self::from_locale_id(&system_locale())
    }

    fn from_locale_id(locale: &str) -> Self {
        let locale = locale.replace('_', "-").to_ascii_lowercase();
        if locale == "zh-cn" || locale == "zh-sg" || locale.starts_with("zh-hans") {
            Self::SimplifiedChinese
        } else if locale == "ja" || locale.starts_with("ja-") {
            Self::Japanese
        } else {
            Self::English
        }
    }
}

impl Default for AppLanguage {
    fn default() -> Self {
        Self::System
    }
}

pub fn set_language(language: AppLanguage) {
    rust_i18n::set_locale(language.locale());
}

/// The translation catalog a process embeds, installed by `waku-localization`
/// (or a test double) at load. Keeping the lookup behind this trait means the
/// wire contract carries no catalog data: locale edits never rebuild
/// `waku-protocol` or its downstream consumers.
pub trait TranslationCatalog: Sync + Send {
    /// Locale chain and shipped-locale fallback applied; `None` is a miss.
    fn try_translate(&self, locale: &str, key: &str) -> Option<String>;
}

static CATALOG: std::sync::OnceLock<&'static dyn TranslationCatalog> =
    std::sync::OnceLock::new();

/// Install the process's catalog. The first install wins; later calls are
/// ignored so the registration is idempotent across entry points.
pub fn install_catalog(catalog: &'static dyn TranslationCatalog) {
    let _ = CATALOG.set(catalog);
}

/// Translate `key` in the process's current locale. A lookup miss — including
/// a process that never installed a catalog — returns the key itself,
/// matching `rust_i18n::t!` semantics.
pub fn translate(key: &str) -> String {
    translate_in(&rust_i18n::locale(), key)
}

/// Translate `key` in `locale` through the installed catalog. A lookup miss
/// returns the key itself.
pub fn translate_in(locale: &str, key: &str) -> String {
    CATALOG
        .get()
        .and_then(|catalog| catalog.try_translate(locale, key))
        .unwrap_or_else(|| key.to_owned())
}

/// Translate `key`, then substitute each `%{name}` placeholder in the
/// translated template (or the key itself on a miss) with the paired value.
pub fn translate_args(key: &str, args: &[(&'static str, String)]) -> String {
    let text = translate(key);
    let mut names = Vec::with_capacity(args.len());
    let mut values = Vec::with_capacity(args.len());
    for (name, value) in args {
        names.push(*name);
        values.push(value.clone());
    }
    rust_i18n::replace_patterns(&text, &names, &values)
}

pub fn uses_east_asian_date_format() -> bool {
    locale_uses_east_asian_date_format(&rust_i18n::locale())
}

fn locale_uses_east_asian_date_format(locale: &str) -> bool {
    matches!(locale, "zh-CN" | "ja")
}

#[cfg(target_os = "macos")]
fn system_locale() -> String {
    use objc2_foundation::NSLocale;

    NSLocale::preferredLanguages()
        .firstObject()
        .map(|locale| locale.to_string())
        .unwrap_or_else(|| "en".to_owned())
}

#[cfg(not(target_os = "macos"))]
fn system_locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en".to_owned())
        .split('.')
        .next()
        .unwrap_or("en")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_names_are_autonyms() {
        assert_eq!(AppLanguage::English.label(), "English");
        assert_eq!(AppLanguage::SimplifiedChinese.label(), "简体中文");
        assert_eq!(AppLanguage::Japanese.label(), "日本語");
    }

    #[test]
    fn system_is_the_default_persisted_preference_and_resolves_to_a_shipped_locale() {
        assert_eq!(AppLanguage::default(), AppLanguage::System);
        assert_eq!(
            serde_json::to_string(&AppLanguage::System).unwrap(),
            r#""system""#
        );
        assert!(matches!(
            AppLanguage::System.locale(),
            "en" | "zh-CN" | "ja"
        ));
    }

    #[test]
    fn japanese_system_locales_are_detected() {
        assert_eq!(AppLanguage::from_locale_id("ja"), AppLanguage::Japanese);
        assert_eq!(AppLanguage::from_locale_id("ja_JP"), AppLanguage::Japanese);
    }

    #[test]
    fn japanese_and_simplified_chinese_use_east_asian_dates() {
        assert!(locale_uses_east_asian_date_format("ja"));
        assert!(locale_uses_east_asian_date_format("zh-CN"));
        assert!(!locale_uses_east_asian_date_format("en"));
    }

    #[test]
    fn simplified_chinese_system_locales_are_detected_without_enabling_traditional_chinese() {
        assert_eq!(
            AppLanguage::from_locale_id("zh-Hans-CN"),
            AppLanguage::SimplifiedChinese
        );
        assert_eq!(
            AppLanguage::from_locale_id("zh_SG"),
            AppLanguage::SimplifiedChinese
        );
        assert_eq!(
            AppLanguage::from_locale_id("zh-Hant-TW"),
            AppLanguage::English
        );
    }

}
