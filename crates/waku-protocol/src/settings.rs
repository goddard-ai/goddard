use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::computer_use::ComputerAppGrant;
use crate::custom_commands::CustomCommand;
use crate::eval::EvalSettings;
use crate::model::ProviderKind;

/// A provider-native model/effort target for one subagent tier. Either side
/// may be absent — an absent model inherits the session's model, an absent
/// effort the provider's default.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
#[serde(default)]
pub struct SubagentTierTarget {
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// One harness-neutral tier ("fast", "medium", "heavy"), mapped per provider
/// so the same `goddard-fast` agent can be a cheap model on every harness at
/// once. A provider missing from the map gets the tier's prompt with the
/// session's own model.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
#[serde(default)]
pub struct SubagentTier {
    pub providers: HashMap<ProviderKind, SubagentTierTarget>,
}

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
    /// Named subagent tiers injected into every session's harness, keyed by
    /// tier name ("explore", "fast", "medium", "heavy"). Empty → only the
    /// built-in read-only `goddard-explore` agent is injected. Ignored while
    /// `subagents_enabled` is off.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subagent_tiers: BTreeMap<String, SubagentTier>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub provider_binary_overrides: HashMap<ProviderKind, String>,
    /// Hosted evaluation-model configuration (backend + BYOK credentials).
    /// `None` means no eval feature can run — callers degrade to their
    /// default path rather than erroring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval: Option<EvalSettings>,
    /// Experimental opt-in for project memory: the daemon maintains a
    /// `.goddard/memory/` store per project, distills finished turns into it
    /// in the background, and injects it into each session's first prompt.
    /// Defaults on in development builds, opt-in in release builds.
    pub memory_experiment_enabled: bool,
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
            subagent_tiers: BTreeMap::new(),
            provider_binary_overrides: HashMap::new(),
            eval: None,
            memory_experiment_enabled: default_experiment_enabled(),
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
