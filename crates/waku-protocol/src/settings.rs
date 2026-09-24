use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::auto_prompts::AutoPromptRule;
use crate::computer_use::ComputerAppGrant;
use crate::custom_commands::CustomCommand;
use crate::eval::EvalSettings;
use crate::model::ProviderKind;
use crate::routing::{ProviderRouteClassMap, RouteClassMap};

/// The default shared proposed-work branch the Projects page's Review tab
/// reads — `origin/qa` out of the box.
pub const DEFAULT_QA_BRANCH: &str = "dev";

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
    /// Per-provider routing preferences: which model/effort each task class
    /// resolves to inside that provider. These bound every mid-session model
    /// move — phase downshifts and the evaluator's own picks — so an Auto
    /// session can only land on a model the user approved here or in the
    /// class map.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_route_classes: ProviderRouteClassMap,
    /// User-authorized Jev rules that may send a follow-up after a task turn.
    /// An absent key seeds the shipped defaults; an explicit empty list
    /// means the user removed them, so the field always serializes.
    #[serde(default = "crate::auto_prompts::default_rules")]
    pub auto_prompts: Vec<AutoPromptRule>,
    /// Experimental opt-in for project memory: the daemon maintains a
    /// `.goddard/memory/` store per project, distills finished turns into it
    /// in the background, and injects it into each session's first prompt.
    /// Defaults on in development builds, opt-in in release builds.
    pub memory_experiment_enabled: bool,
    /// Experimental opt-in for cross-session composer drafts. Defaults on in
    /// development builds and off in release builds.
    #[serde(default = "default_experiment_enabled")]
    pub composer_drafts_experiment_enabled: bool,
    /// Per-provider model override for memory distillation runs. A provider
    /// absent here distills on its advertised default model; the value is a
    /// catalog model id handed to that provider's headless driver.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub memory_models: BTreeMap<ProviderKind, String>,
    /// Preferred inexpensive model for background session title rewrites.
    /// Claude and Codex have inexpensive defaults; other supported providers
    /// need a selected model before background title rewrites can run.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub title_models: BTreeMap<ProviderKind, String>,
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
    /// Experimental opt-in for the sandbox environment surface — the access
    /// menu's Environment section, the session badge, and the environment
    /// toggle all stay hidden while this is off. Defaults on in development
    /// builds, opt-in in release builds.
    pub sandbox_experiment_enabled: bool,
    /// Whether fresh tasks start in the Sandbox VM environment instead of
    /// This Mac. The experiment opt-in still gates the surface; this only
    /// changes which environment a new task seeds. A per-task choice in the
    /// access menu still wins for that task.
    #[serde(default)]
    pub sandbox_default_enabled: bool,
    /// Seconds a settled provider runtime may sit idle before the daemon
    /// reclaims it. `None` keeps the built-in default (30 minutes); `0`
    /// disables eviction. A reclaimed runtime restarts lazily from the
    /// session's provider cursor on the next prompt, so the knob trades a
    /// one-time resume delay against resident process memory. Runtimes
    /// whose session is busy or cannot resume are never evicted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_idle_timeout_secs: Option<u64>,
    /// The branch the review queue treats as the shared proposed-work
    /// train: `origin/<name>` is what the Review tab lists, and rejections
    /// push reverts onto it. Daemon-owned so every attached client sees one
    /// train; empty resolves to [`DEFAULT_QA_BRANCH`].
    #[serde(default)]
    pub qa_branch: String,
    /// Keep the daemon's host awake so remote clients — the mobile and web
    /// apps — can still reach it. While on, the daemon holds the platform
    /// sleep assertions `caffeinate -is` would: idle sleep is prevented on
    /// battery and AC, and on AC the host stays awake even with the lid
    /// closed. Off by default — it trades battery for reachability.
    #[serde(default)]
    pub keep_awake: bool,
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
            provider_route_classes: ProviderRouteClassMap::new(),
            auto_prompts: crate::auto_prompts::default_rules(),
            memory_experiment_enabled: default_experiment_enabled(),
            composer_drafts_experiment_enabled: default_experiment_enabled(),
            memory_models: BTreeMap::new(),
            title_models: BTreeMap::new(),
            integrations_enabled: default_experiment_enabled(),
            integrations: Vec::new(),
            integrations_proxy_token: String::new(),
            sandbox_experiment_enabled: default_experiment_enabled(),
            sandbox_default_enabled: false,
            runtime_idle_timeout_secs: None,
            qa_branch: DEFAULT_QA_BRANCH.to_owned(),
            keep_awake: false,
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
