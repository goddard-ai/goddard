//! The two font families the app is configured with — an interface face and
//! a code face — plus the cached list of installed families the Appearance
//! settings pickers draw from.

use std::sync::Arc;

use gpui::{App, Global, SharedString};
use parking_lot::RwLock;

/// The interface face: chrome text and markdown prose. `.SystemUIFont` is
/// GPUI's alias for the platform's UI font, so it always resolves.
pub const DEFAULT_UI_FAMILY: &str = ".SystemUIFont";
/// The code face: the file editor, diffs, code blocks, tool output, and the
/// terminal. JetBrains Mono ships inside the binary, so the default survives
/// machines that never installed it.
pub const DEFAULT_CODE_FAMILY: &str = "JetBrains Mono";

/// The families a render pass draws with. `ui` covers prose and chrome,
/// `code` every monospace surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fonts {
    pub ui: SharedString,
    pub code: SharedString,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            ui: SharedString::from(DEFAULT_UI_FAMILY),
            code: SharedString::from(DEFAULT_CODE_FAMILY),
        }
    }
}

struct ActiveFonts(Fonts);
impl Global for ActiveFonts {}

/// The currently resolved families — the defaults until `install` runs.
pub fn current(cx: &App) -> Fonts {
    if cx.has_global::<ActiveFonts>() {
        cx.global::<ActiveFonts>().0.clone()
    } else {
        Fonts::default()
    }
}

/// Publish the families persisted settings resolve to.
pub fn install(ui: Option<&str>, code: Option<&str>, cx: &mut App) {
    let resolve = |family: Option<&str>, default| {
        family
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .map_or_else(|| SharedString::from(default), SharedString::from)
    };
    cx.set_global(ActiveFonts(Fonts {
        ui: resolve(ui, DEFAULT_UI_FAMILY),
        code: resolve(code, DEFAULT_CODE_FAMILY),
    }));
}

#[derive(Default)]
struct FontList {
    names: Option<Vec<SharedString>>,
    loading: bool,
}

/// `all_font_names` enumerates every family the platform knows — too slow to
/// run inside a frame — so the result lives here, filled once by a
/// background prefetch at startup.
#[derive(Default)]
struct InstalledFonts(Arc<RwLock<FontList>>);
impl Global for InstalledFonts {}

/// The cached family names; `None` until the prefetch lands. The bundled
/// face and the system alias are guaranteed present even if the platform
/// listing omits them.
pub fn installed(cx: &App) -> Option<Vec<SharedString>> {
    cx.try_global::<InstalledFonts>()
        .and_then(|fonts| fonts.0.read().names.clone())
}

/// Fill the family cache on a background thread. Idempotent while a load is
/// already in flight.
pub fn prefetch(cx: &mut App) {
    let cache = cx.default_global::<InstalledFonts>().0.clone();
    {
        let mut list = cache.write();
        if list.names.is_some() || list.loading {
            return;
        }
        list.loading = true;
    }
    let text_system = cx.text_system().clone();
    cx.background_executor()
        .spawn(async move {
            let mut names = text_system
                .all_font_names()
                .into_iter()
                .map(SharedString::from)
                .collect::<Vec<_>>();
            for default in [DEFAULT_UI_FAMILY, DEFAULT_CODE_FAMILY] {
                if !names.iter().any(|name| name.as_ref() == default) {
                    names.push(SharedString::from(default));
                }
            }
            names.sort_unstable();
            cache.write().names = Some(names);
        })
        .detach();
}
