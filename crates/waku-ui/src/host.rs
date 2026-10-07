//! Host-supplied platform capabilities.
//!
//! Native window appearance and accessibility flags live in the desktop
//! process (`crate::platform`), which owns platform initialization. The
//! foundation crate declares the narrow surface it needs here; the host
//! installs it once at startup before [`crate::theme::init`] runs.

use std::sync::OnceLock;

use gpui::{Hsla, Window};

/// The OS-level hooks theme application reaches for. Each entry is a plain
/// function the host supplies — the crate stores no window state and never
/// calls back into `Waku`.
pub struct HostPlatform {
    /// The OS "increase contrast" accessibility preference.
    pub increase_contrast: fn() -> bool,
    /// The OS "reduce transparency" accessibility preference.
    pub reduce_transparency: fn() -> bool,
    /// Follow the OS when `dark` is `None`, otherwise force the window's
    /// native appearance (titlebar, vibrancy, menus).
    pub set_window_appearance: fn(&Window, Option<bool>),
    /// Configure the native material behind the sidebar strip.
    pub configure_sidebar_material: fn(&Window, Hsla, bool, bool),
}

static HOST_PLATFORM: OnceLock<HostPlatform> = OnceLock::new();

/// Install the host's platform implementation. Called once at app startup;
/// a second install is ignored.
pub fn install_host_platform(platform: HostPlatform) {
    let _ = HOST_PLATFORM.set(platform);
}

pub(crate) fn host_platform() -> &'static HostPlatform {
    HOST_PLATFORM
        .get()
        .expect("waku_ui::host::install_host_platform must run before theme work")
}

pub(crate) fn increase_contrast() -> bool {
    (host_platform().increase_contrast)()
}

pub(crate) fn reduce_transparency() -> bool {
    (host_platform().reduce_transparency)()
}

pub(crate) fn set_window_appearance(window: &Window, dark: Option<bool>) {
    (host_platform().set_window_appearance)(window, dark)
}

pub(crate) fn configure_sidebar_material(
    window: &Window,
    sidebar: Hsla,
    dark: bool,
    transparent: bool,
) {
    (host_platform().configure_sidebar_material)(window, sidebar, dark, transparent)
}
