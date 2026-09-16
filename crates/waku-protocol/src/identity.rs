//! Shared application identity used by the daemon and desktop client.

#[cfg(debug_assertions)]
pub const APP_NAME: &str = "Goddard Debug";
#[cfg(not(debug_assertions))]
pub const APP_NAME: &str = "Goddard";

#[cfg(debug_assertions)]
pub const APP_ID: &str = "org.goddardai.app.debug";
#[cfg(not(debug_assertions))]
pub const APP_ID: &str = "org.goddardai.app";

#[cfg(debug_assertions)]
pub const DATA_DIRECTORY_NAME: &str = "Goddard Debug";
#[cfg(not(debug_assertions))]
pub const DATA_DIRECTORY_NAME: &str = "Goddard";

/// Per-user configuration directory beneath the home directory. The daemon
/// and release desktop both anchor settings and projectless workspaces here.
pub const HOME_DIRECTORY_NAME: &str = ".goddard";

/// Identity carried by pre-Goddard builds. Startup migration copies each of
/// these directories to its current name once, then leaves the source alone.
#[cfg(debug_assertions)]
pub const LEGACY_DATA_DIRECTORY_NAME: &str = "Waku Debug";
#[cfg(not(debug_assertions))]
pub const LEGACY_DATA_DIRECTORY_NAME: &str = "Waku";

pub const LEGACY_HOME_DIRECTORY_NAME: &str = ".waku";
