use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::computer_use::ComputerAppGrant;
use crate::custom_commands::CustomCommand;
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
/// so the same `waku-fast` agent can be a cheap model on every harness at
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
    /// Named subagent tiers injected into every session's harness, keyed by
    /// tier name ("explore", "fast", "medium", "heavy"). Empty → only the
    /// built-in read-only `waku-explore` agent is injected.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subagent_tiers: BTreeMap<String, SubagentTier>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub provider_binary_overrides: HashMap<ProviderKind, String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            computer_use_enabled: false,
            computer_use_allowed_apps: Vec::new(),
            agent_tools_enabled: false,
            agent_settings_enabled: true,
            custom_commands: Vec::new(),
            disabled_providers: Vec::new(),
            subagent_tiers: BTreeMap::new(),
            provider_binary_overrides: HashMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl DaemonSettings {
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".waku")
            .join("settings.json")
    }

    pub fn discard_legacy_app_keys(&mut self) {
        for key in [
            "analytics_enabled",
            "favorite_models",
            "theme",
            "language",
            "sidebar_transparency",
        ] {
            self.extra.remove(key);
        }
    }
}
