use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::computer_use::ComputerAppGrant;
use crate::custom_commands::CustomCommand;
use crate::eval::EvalSettings;
use crate::model::ProviderKind;
use crate::routing::RouteClassMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(default)]
pub struct DaemonSettings {
    pub computer_use_enabled: bool,
    /// Experimental opt-in that exposes Computer Use at all: its settings
    /// page, permission probing, and driver helper all stay off while this
    /// is off. Defaults on in development builds, opt-in in release builds.
    pub computer_use_experiment_enabled: bool,
    pub computer_use_allowed_apps: Vec<ComputerAppGrant>,
    /// Whether agents running inside this daemon's provider sessions may
    /// create and prompt other Waku tasks through the scoped agent
    /// credential. Off by default: the daemon rejects those two commands
    /// outright while the always-on settings surface stays reachable.
    pub agent_tools_enabled: bool,
    /// Whether agents running inside this daemon's provider sessions may
    /// write the user-facing settings surface — custom commands today —
    /// through the scoped agent credential. On by default; agent writes are
    /// attributed to their task and announced to every client.
    pub agent_settings_enabled: bool,
    /// User-owned terminal commands surfaced in the command palette. They
    /// live here rather than in a client's app file so every attached
    /// client — and the agent settings surface — shares one list, and
    /// because the scripts execute on the daemon host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_commands: Vec<CustomCommand>,
    pub disabled_providers: Vec<ProviderKind>,
    /// Experimental: inject named subagents into every session's harness.
    /// Off by default in release builds, on in debug builds (`bun run dev`);
    /// toggling affects only sessions started afterwards.
    pub subagents_enabled: bool,
    /// Experimental: prepend a token-budgeted structural map of the session's
    /// workspace to the first prompt of every new session, so providers skip
    /// cold repo exploration. Off by default in release builds, on in debug
    /// builds; affects only sessions started afterwards.
    pub project_map_enabled: bool,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub provider_binary_overrides: HashMap<ProviderKind, String>,
    /// Hosted evaluation-model configuration (backend + BYOK credentials).
    /// `None` means no eval feature can run — callers degrade to their
    /// default path rather than erroring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval: Option<EvalSettings>,
    /// The user's model-routing map: which provider/model/effort each task
    /// class starts on. Classes absent here leave routed sessions on their
    /// `last_used` default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub route_classes: RouteClassMap,
    /// Experimental opt-in for project memory: the daemon maintains a
    /// `.goddard/memory/` store per project, distills finished turns into it
    /// in the background, and injects it into each session's first prompt.
    /// Defaults on in development builds, opt-in in release builds.
    pub memory_experiment_enabled: bool,
    /// Experimental opt-in for the MCP integrations pane and the daemon's
    /// local MCP proxy. Defaults on in development builds, opt-in in release.
    pub integrations_enabled: bool,
    /// Connected integrations: which services are set up, on which variant,
    /// and which providers receive them. Credentials never live here — the
    /// daemon's secret store owns them; `auth` only records the state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrations: Vec<crate::integrations::IntegrationSetting>,
    /// Bearer that agents present to the daemon's local MCP proxy. Minted
    /// lazily; local-only, it authorizes proxy access and nothing upstream.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub integrations_proxy_token: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Experiments default on in development builds (`bun run dev`); release
/// builds keep them opt-in. An explicit `false` in the document still wins.
fn default_experiment_enabled() -> bool {
    cfg!(debug_assertions)
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            computer_use_enabled: false,
            computer_use_experiment_enabled: default_experiment_enabled(),
            computer_use_allowed_apps: Vec::new(),
            agent_tools_enabled: false,
            agent_settings_enabled: true,
            custom_commands: Vec::new(),
            disabled_providers: Vec::new(),
            subagents_enabled: default_experiment_enabled(),
            project_map_enabled: default_experiment_enabled(),
            provider_binary_overrides: HashMap::new(),
            eval: None,
            route_classes: RouteClassMap::new(),
            memory_experiment_enabled: default_experiment_enabled(),
            integrations_enabled: default_experiment_enabled(),
            integrations: Vec::new(),
            integrations_proxy_token: String::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl DaemonSettings {
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(crate::identity::HOME_DIRECTORY_NAME)
            .join("settings.json")
    }

    pub fn discard_legacy_app_keys(&mut self) {
        for key in [
            "analytics_enabled",
            "favorite_models",
            "theme",
            "language",
            "sidebar_transparency",
            "sidebar_transparency_amount",
        ] {
            self.extra.remove(key);
        }
    }
}
