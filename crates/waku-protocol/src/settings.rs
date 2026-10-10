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
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub provider_binary_overrides: HashMap<ProviderKind, String>,
    /// Hosted evaluation-model configuration (provider pick only — the
    /// credentials it runs on live in `inference`). `None` means no eval
    /// feature can run — callers degrade to their default path rather than
    /// erroring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval: Option<EvalSettings>,
    /// The shared inference providers — TypeSafe, the Vercel AI Gateway,
    /// Cloudflare Workers AI, OpenRouter — that eval-driven features and
    /// voice briefings draw on. Credentials never live in this document: each
    /// entry's `api_key` is a write-only slot the daemon moves into its
    /// secret store, and `credential_configured` reports the result.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inference:
        BTreeMap<crate::inference::InferenceProvider, crate::inference::InferenceProviderSettings>,
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
    /// Experimental opt-in for the Boss assistant, employee management,
    /// personas, and plans surfaces. Defaults on in development builds and
    /// opt-in in release builds.
    pub boss_experiment_enabled: bool,
    /// Disable automatic rotation of settled Boss chats. Off by default, including release builds.
    #[serde(default)]
    pub boss_rotation_disabled: bool,
    /// Context fraction that makes a settled Boss session eligible to rotate.
    #[serde(default = "default_boss_rotation_threshold")]
    pub boss_rotation_context_threshold: f64,
    /// Days of employee-chat history the command palette's "Search employee
    /// chats" view reaches back — archived records included. `0` hides the
    /// command; any other value bounds the search to chats with activity in
    /// that window.
    #[serde(default = "default_employee_chat_search_days")]
    pub employee_chat_search_days: u32,
    /// Experimental opt-in for cross-session composer drafts. Defaults on in
    /// development builds and off in release builds.
    #[serde(default = "default_experiment_enabled")]
    pub composer_drafts_experiment_enabled: bool,
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
    /// User-declared external MCP servers — stdio commands delivered to
    /// providers directly and remote servers exposed through the local
    /// proxy — governed by the same `integrations_enabled` opt-in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<crate::integrations::McpServerSetting>,
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
    /// Experimental opt-in for planning-session wireframes — the emit
    /// affordance and the file viewer's themed preview stay hidden while
    /// this is off, and emitted `.wireframe.json` files persist either
    /// way. Defaults on in development builds, opt-in in release builds.
    #[serde(default = "default_experiment_enabled")]
    pub wireframes_experiment_enabled: bool,
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

fn default_boss_rotation_threshold() -> f64 {
    0.8
}

/// The shipped employee-chat search window — recent work without a scan
/// that reaches into stale records by default.
pub const DEFAULT_EMPLOYEE_CHAT_SEARCH_DAYS: u32 = 3;

fn default_employee_chat_search_days() -> u32 {
    DEFAULT_EMPLOYEE_CHAT_SEARCH_DAYS
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
            provider_binary_overrides: HashMap::new(),
            eval: None,
            inference: BTreeMap::new(),
            route_classes: RouteClassMap::new(),
            provider_route_classes: ProviderRouteClassMap::new(),
            auto_prompts: crate::auto_prompts::default_rules(),
            boss_experiment_enabled: default_experiment_enabled(),
            boss_rotation_disabled: false,
            boss_rotation_context_threshold: default_boss_rotation_threshold(),
            employee_chat_search_days: default_employee_chat_search_days(),
            composer_drafts_experiment_enabled: default_experiment_enabled(),
            title_models: BTreeMap::new(),
            integrations_enabled: default_experiment_enabled(),
            integrations: Vec::new(),
            integrations_proxy_token: String::new(),
            mcp_servers: Vec::new(),
            sandbox_experiment_enabled: default_experiment_enabled(),
            sandbox_default_enabled: false,
            wireframes_experiment_enabled: default_experiment_enabled(),
            runtime_idle_timeout_secs: None,
            qa_branch: DEFAULT_QA_BRANCH.to_owned(),
            keep_awake: false,
            extra: BTreeMap::new(),
        }
    }
}

impl DaemonSettings {
    /// Whether eval calls can run on the selected provider — it serves the
    /// eval context and its credential plus required config are in place.
    /// The client-side counterpart of the daemon's hydrated resolve.
    pub fn eval_ready(&self) -> bool {
        self.eval
            .as_ref()
            .is_some_and(|eval| eval.ready(&self.inference))
    }

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
            "project_map_enabled",
            // The former opt-in is inert; only the explicit opt-out above
            // disables the default-on runtime path.
            "boss_rotation_enabled",
            // Prompt-cache TTLs no longer gate rotation; a settled chat
            // rotates as soon as its context crosses the threshold.
            "boss_rotation_cache_ttl_secs",
        ] {
            self.extra.remove(key);
        }
    }
}
