#![recursion_limit = "256"]

//! Goddard's embedded translation catalog.
//!
//! This crate owns the single `rust_i18n` backend compiled from `locales/` and
//! registers it with `waku-protocol` at load, so wire types like
//! `WireTranslation` render in the running process's locale without
//! `waku-protocol` depending on catalog data. `waku`, `waku-core` and
//! `waku-client` resolve `tr!`, `tr_cow!`, `localized!` and `keyed!` through
//! these functions instead of embedding their own copy of the catalog.

use std::borrow::Cow;
use std::sync::Once;

rust_i18n::i18n!("../../locales", fallback = "en");

// `i18n!` reads these files in a proc macro, which Cargo does not always
// discover as an input when only a YAML file changes. Keep explicit source
// dependencies so the watcher rebuilds the translation registry itself.
const _LOCALE_SOURCES: [&str; 3] = [
    include_str!("../../../locales/app.yml"),
    include_str!("../../../locales/zh-CN.yml"),
    include_str!("../../../locales/ja.yml"),
];

/// Translate `key` in the process's current locale. A lookup miss returns the
/// key itself, matching `rust_i18n::t!` semantics.
pub fn translate(key: &str) -> String {
    install();
    translate_in(&rust_i18n::locale(), key)
}

/// Translate `key` in `locale`, falling back through the shipped locale chain.
/// A lookup miss returns the key itself.
pub fn translate_in(locale: &str, key: &str) -> String {
    install();
    crate::_rust_i18n_try_translate(locale, key)
        .map_or_else(|| key.to_owned(), |text| text.into_owned())
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

/// Borrow the catalog's translated string on hot render paths; interpolation
/// uses `translate_args` because formatted messages necessarily allocate.
pub fn translate_cow(key: &'static str) -> Cow<'static, str> {
    install();
    crate::_rust_i18n_try_translate(&rust_i18n::locale(), key)
        .unwrap_or(Cow::Borrowed(key))
}

/// The locales this catalog ships, sorted.
pub fn available_locales() -> Vec<Cow<'static, str>> {
    crate::_rust_i18n_available_locales()
}

struct EmbeddedCatalog;

impl waku_protocol::i18n::TranslationCatalog for EmbeddedCatalog {
    fn try_translate(&self, locale: &str, key: &str) -> Option<String> {
        crate::_rust_i18n_try_translate(locale, key).map(|text| text.into_owned())
    }
}

static CATALOG: EmbeddedCatalog = EmbeddedCatalog;

/// Register this crate's embedded catalog as `waku-protocol`'s translator.
///
/// `#[ctor]` schedules a call when the process loads, so binaries install the
/// catalog without an explicit init step; the linker keeps that initializer
/// whenever it keeps `install`, which every other entry point here references.
/// Explicit calls remain safe — the registration runs once.
#[ctor::ctor(unsafe)]
pub fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        waku_protocol::i18n::install_catalog(&CATALOG);
    });
}

#[cfg(test)]
mod tests {
    use waku_protocol::i18n::AppLanguage;

    #[test]
    fn language_locale_ids_are_supported() {
        assert_eq!(AppLanguage::English.locale(), "en");
        assert_eq!(AppLanguage::SimplifiedChinese.locale(), "zh-CN");
        assert_eq!(AppLanguage::Japanese.locale(), "ja");
        let locales = crate::available_locales();
        assert_eq!(locales.len(), 3);
        assert!(locales.iter().any(|locale| locale.as_ref() == "en"));
        assert!(locales.iter().any(|locale| locale.as_ref() == "zh-CN"));
        assert!(locales.iter().any(|locale| locale.as_ref() == "ja"));
    }

    #[test]
    fn translations_are_complete_and_interpolate_naturally() {
        assert_eq!(&*rust_i18n::t!("settings.daemon", locale = "en"), "Daemon");
        assert_eq!(
            &*rust_i18n::t!("daemon.expose_title", locale = "en"),
            "Expose managed daemon"
        );
        assert_eq!(
            &*rust_i18n::t!("settings.general", locale = "zh-CN"),
            "通用"
        );
        assert_eq!(
            &*rust_i18n::t!(
                "computer_use.allow_control",
                locale = "zh-CN",
                app = "Finder"
            ),
            "允许 Goddard 控制“Finder”吗？"
        );
        assert_eq!(
            &*rust_i18n::t!("session.rewound", locale = "zh-CN", turn = 3),
            "已回退到第 3 轮任务之前"
        );
        assert_eq!(&*rust_i18n::t!("settings.general", locale = "ja"), "一般");
        assert_eq!(
            &*rust_i18n::t!("computer_use.allow_control", locale = "ja", app = "Finder"),
            "Goddard に「Finder」の操作を許可しますか？"
        );
        assert_eq!(
            &*rust_i18n::t!("session.rewound", locale = "ja", turn = 3),
            "タスクをターン 3 の前まで巻き戻しました"
        );
    }

    #[test]
    fn task_creation_copy_uses_task_terminology() {
        assert_eq!(&*rust_i18n::t!("menu.new_task", locale = "en"), "New Task");
        assert_eq!(
            &*rust_i18n::t!("command_palette.new_task", locale = "en"),
            "New task"
        );
        assert_eq!(
            &*rust_i18n::t!("providers.disabled_for_new_tasks", locale = "en"),
            "Disabled for new tasks"
        );
    }

    #[test]
    fn installed_catalog_serves_protocol_translation() {
        crate::install();
        assert_eq!(
            waku_protocol::i18n::translate_in("en", "settings.daemon"),
            "Daemon"
        );
        assert_eq!(
            waku_protocol::i18n::translate_in("zh-CN", "settings.general"),
            "通用"
        );
        assert_eq!(
            waku_protocol::i18n::translate("missing.key"),
            "missing.key"
        );
    }
}
