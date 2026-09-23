//! Daemon-owned task storage plus desktop preference/state serialization.
//!
//! Sessions and projects live in SQLite (`app.db`), app-managed UI state in
//! `state.json`, desktop preferences in `temp/app.json` for Debug or
//! `~/.goddard/app.json` for Release, daemon preferences in
//! `~/.goddard/settings.json`, and binary payloads in [`crate::blob_store`].
//! Of the configuration documents, only the desktop file is written here;
//! daemon settings cross the RPC boundary and are persisted by `goddard-daemon`.
//!
//! A save writes only the rows whose contents changed, so a streaming turn
//! costs a few kilobytes no matter how much history exists. Fields the sidebar
//! sorts on are promoted to columns so listing sessions never has to
//! deserialize a transcript. The schema is defined in `db/schema.ts` and
//! applied by [`apply_migrations`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::blob_store::BlobStore;
use crate::computer_use::ComputerAppGrant;
use crate::i18n::AppLanguage;
use crate::identity::DATA_DIRECTORY_NAME;
use crate::model::{
    AgentSession, FavoriteModel, Message, MessageAtom, MessageAttachment, MessageRole, Project,
    ProviderKind, RuntimeEventCursor, RuntimeMode, SessionWorkspace, TranscriptNotice,
};
use crate::theme::ThemeSettings;
use waku_protocol::custom_commands::CustomCommand;
pub use waku_protocol::persistence::{
    ComposerDraft, ComposerDraftAttachment, ComposerDraftChange, ComposerDraftKey,
    ComposerDraftTarget, ComposerDrafts, SessionMessageMatch, SessionMessageSearchScope,
};

const STATE_VERSION: u32 = 5;
const APP_STATE_VERSION: u32 = 1;
const COMPOSER_DRAFTS_FILENAME: &str = "composer-drafts.json";
/// Uploading a blob and committing the reference are separate operations. A
/// sweep may reclaim abandoned payloads, but never ones a client could still
/// be handing off to a draft or task save.
const ASSET_SWEEP_GRACE_PERIOD: Duration = Duration::from_secs(60 * 60);

pub const DEFAULT_SIDEBAR_WIDTH: f32 = 252.0;
pub const DEFAULT_RIGHT_PANEL_WIDTH: f32 = 460.0;

fn default_sidebar_visibility() -> bool {
    true
}

fn default_right_panel_visibility() -> bool {
    false
}

fn default_computer_use_enabled() -> bool {
    false
}

/// Experiments default on in development builds (`bun run dev`); release
/// builds keep them opt-in. An explicit `false` in the file wins either way.
fn default_experiment_enabled() -> bool {
    cfg!(debug_assertions)
}

fn default_analytics_enabled() -> bool {
    true
}

fn default_agent_settings_enabled() -> bool {
    true
}

fn default_provider() -> ProviderKind {
    ProviderKind::Codex
}

fn default_sidebar_width() -> f32 {
    DEFAULT_SIDEBAR_WIDTH
}

fn default_right_panel_width() -> f32 {
    DEFAULT_RIGHT_PANEL_WIDTH
}

/// Explicit trait choices remembered for one provider model.
///
/// Reasoning effort and service tier are model capabilities, so their option
/// ids must not leak into another provider merely because that provider uses
/// the same strings. Keeping the key beside the values lets the model picker
/// restore them when the user returns to the model that owns them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RememberedModelTraits {
    provider: ProviderKind,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_window: Option<String>,
}

/// A model+effort selection a session was actually started with, most recent
/// first. The model picker reads list position as the recency rank.
///
/// `fast` is a remembered flag, not part of the entry's identity: starting a
/// session with `model-effort` and later `model-effort-fast` updates the same
/// slot rather than occupying two, so only the most recently used variant of
/// an effort ever carries the rank.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecentModelUse {
    pub provider: ProviderKind,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub fast: bool,
    pub used_at: u64,
}

/// How many selections the picker keeps in its recent section.
const RECENT_MODEL_USES_LIMIT: usize = 32;

/// Small, independently persisted composer state.
///
/// Session storage intentionally excludes blank sessions. Keeping drafts in a
/// separate atomic JSON document preserves that lifecycle and also lets the
/// app debounce writes onto the background executor without cloning the
/// transcript database state.
#[derive(Clone)]
pub struct ComposerDraftStore {
    path: PathBuf,
    latest_write: Arc<Mutex<u64>>,
}

impl ComposerDraftStore {
    pub fn for_state_path(state_path: &Path) -> Self {
        let directory = state_path.parent().unwrap_or_else(|| Path::new("."));
        Self {
            path: directory.join(COMPOSER_DRAFTS_FILENAME),
            latest_write: Arc::new(Mutex::new(0)),
        }
    }

    pub fn load(&self) -> io::Result<ComposerDrafts> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ComposerDrafts::default());
            }
            Err(error) => return Err(error),
        };
        serde_json::from_slice(&bytes).map_err(to_io_error)
    }

    /// Write a complete snapshot atomically. Older background jobs become
    /// no-ops if a newer generation reached the store first.
    pub fn save(&self, drafts: ComposerDrafts, generation: u64) -> io::Result<()> {
        let data = serde_json::to_vec_pretty(&drafts).map_err(to_io_error)?;
        let mut latest_write = self.latest_write.lock();
        if generation < *latest_write {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, data)?;
        fs::rename(temporary, &self.path)?;
        *latest_write = generation;
        Ok(())
    }

    /// Apply only the drafts a client actually changed. The mutation and
    /// atomic file replacement share the same lock as legacy snapshot writes,
    /// so concurrent clients cannot clobber unrelated draft keys.
    pub fn apply_changes(&self, changes: Vec<ComposerDraftChange>) -> io::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut latest_write = self.latest_write.lock();
        let mut drafts = self.load()?;
        let mut changed = false;
        for change in changes {
            let key = ComposerDraftKey::from(change.target);
            changed |= match change.draft {
                Some(draft) => drafts.set(key, draft),
                None => drafts.remove(key),
            };
        }
        if !changed {
            return Ok(());
        }
        let data = serde_json::to_vec_pretty(&drafts).map_err(to_io_error)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, data)?;
        fs::rename(temporary, &self.path)?;
        *latest_write = latest_write.saturating_add(1);
        Ok(())
    }
}

/// Desktop-owned, user-editable configuration.
///
/// This deliberately excludes navigation, panel geometry, and other values
/// that the app changes as a side effect of ordinary use. Both builds keep it
/// at `~/.goddard/app.json` without exposing app-managed state or daemon-owned
/// provider policy.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppSettings {
    pub analytics_enabled: bool,
    pub favorite_models: Vec<FavoriteModel>,
    pub theme: ThemeSettings,
    pub language: AppLanguage,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            analytics_enabled: default_analytics_enabled(),
            favorite_models: Vec::new(),
            theme: ThemeSettings::default(),
            language: AppLanguage::default(),
        }
    }
}

/// App-managed state that should never appear in the user settings file.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct AppState {
    app_state_version: u32,
    /// Random installation-scoped analytics identity. It is deliberately
    /// unrelated to provider accounts, projects, or session content.
    #[serde(default = "Uuid::new_v4")]
    analytics_id: Uuid,
    #[serde(default)]
    selected_project: Option<Uuid>,
    #[serde(default)]
    selected_session: Option<Uuid>,
    #[serde(default = "default_provider")]
    last_provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_context_window: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    remembered_model_traits: Vec<RememberedModelTraits>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    recent_model_uses: Vec<RecentModelUse>,
    #[serde(default = "default_sidebar_visibility")]
    sidebar_visible: bool,
    #[serde(default = "default_right_panel_visibility")]
    right_panel_visible: bool,
    #[serde(default = "default_sidebar_width")]
    sidebar_width: f32,
    #[serde(default = "default_right_panel_width")]
    right_panel_width: f32,
}

/// The complete in-memory model hydrated from settings, app state, and SQLite.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PersistedState {
    pub version: u32,
    /// Random installation-scoped analytics identity. See [`AppState`].
    #[serde(default = "Uuid::new_v4")]
    pub analytics_id: Uuid,
    #[serde(default = "default_analytics_enabled")]
    pub analytics_enabled: bool,
    pub projects: Vec<Project>,
    pub sessions: Vec<AgentSession>,
    pub selected_project: Option<Uuid>,
    pub selected_session: Option<Uuid>,
    pub last_provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_context_window: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remembered_model_traits: Vec<RememberedModelTraits>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_model_uses: Vec<RecentModelUse>,
    #[serde(default)]
    pub favorite_models: Vec<FavoriteModel>,
    #[serde(default)]
    pub theme: ThemeSettings,
    #[serde(default)]
    pub language: AppLanguage,
    #[serde(default = "default_sidebar_visibility")]
    pub sidebar_visible: bool,
    #[serde(default = "default_right_panel_visibility")]
    pub right_panel_visible: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default = "default_right_panel_width")]
    pub right_panel_width: f32,
    #[serde(default = "default_computer_use_enabled")]
    pub computer_use_enabled: bool,
    /// Experimental opt-in gating Computer Use entirely, mirrored from the
    /// settings document. Off in release builds unless the user turns it on.
    #[serde(default = "default_experiment_enabled")]
    pub computer_use_experiment_enabled: bool,
    #[serde(default)]
    pub computer_use_allowed_apps: Vec<ComputerAppGrant>,
    /// Providers switched off for new sessions in the Providers settings.
    #[serde(default)]
    pub disabled_providers: Vec<ProviderKind>,
    /// Per-provider binary overrides from the Providers settings; empty means
    /// detect from PATH.
    #[serde(default)]
    pub provider_binary_overrides: HashMap<ProviderKind, String>,
    /// Whether agent harnesses may create and prompt other tasks through
    /// their scoped credentials. Defaults off.
    #[serde(default)]
    pub agent_tools_enabled: bool,
    /// Whether agent harnesses may write the user-facing settings surface —
    /// custom commands — through their scoped credentials. Defaults on.
    #[serde(default = "default_agent_settings_enabled")]
    pub agent_settings_enabled: bool,
    /// Whether the daemon's host refuses sleep so remote clients stay
    /// reachable, mirrored from the settings document.
    #[serde(default)]
    pub keep_awake: bool,
    /// Daemon-owned custom commands mirrored from the settings document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_commands: Vec<CustomCommand>,
    /// Experimental: whether sessions get named subagents injected, mirrored
    /// from the settings document.
    #[serde(default = "default_experiment_enabled")]
    pub subagents_enabled: bool,
    /// Experimental: whether the daemon maintains project memory, mirrored
    /// from the settings document.
    #[serde(default = "default_experiment_enabled")]
    pub memory_experiment_enabled: bool,
    /// Per-provider memory-distillation model overrides, mirrored from the
    /// settings document.
    #[serde(skip)]
    pub memory_models: BTreeMap<ProviderKind, String>,
    /// Per-provider model for background title rewrites, mirrored from daemon settings.
    #[serde(skip)]
    pub title_models: BTreeMap<ProviderKind, String>,
    /// Experimental: whether new sessions get the workspace project map
    /// prepended to their first prompt, mirrored from the settings document.
    #[serde(default = "default_experiment_enabled")]
    pub project_map_enabled: bool,
    /// Hosted evaluation-model settings mirrored from the settings document.
    /// Kept out of the on-disk state deliberately: the credential-bearing
    /// document is the daemon's `settings.json`, and this copy exists so the
    /// settings surface can read and edit it without duplicating secrets.
    #[serde(skip)]
    pub eval: Option<waku_protocol::eval::EvalSettings>,
    /// The user's class-level routing map (provider/model/effort per task
    /// class), mirrored from the settings document.
    #[serde(skip)]
    pub route_classes: waku_protocol::routing::RouteClassMap,
    /// Unknown daemon settings survive edits made by this desktop version.
    #[serde(skip)]
    daemon_settings_extra: BTreeMap<String, serde_json::Value>,
    /// Sessions changed since the last save.
    ///
    /// The app knows what it touched, so it says so rather than making the
    /// store rediscover it. Every `&mut AgentSession` is handed out by
    /// [`Self::session_mut`], which records the id here; a save then writes
    /// exactly these rows instead of re-serializing the whole history to work
    /// out what moved.
    #[serde(skip)]
    dirty_sessions: HashSet<Uuid>,
}

impl PersistedState {
    /// The only way to get a mutable session. Marks it for the next save.
    pub fn session_mut(&mut self, id: Uuid) -> Option<&mut AgentSession> {
        let session = self.sessions.iter_mut().find(|session| session.id == id)?;
        self.dirty_sessions.insert(id);
        Some(session)
    }

    /// Records a session as changed without borrowing it, for the few paths
    /// that mutate through a slice or add a session outright.
    pub fn mark_session_dirty(&mut self, id: Uuid) {
        self.dirty_sessions.insert(id);
    }

    /// Drops the given dirty marks, for a save that claimed them into a
    /// snapshot. A session re-dirtied afterwards keeps its flag.
    pub fn unmark_sessions_dirty(&mut self, ids: &HashSet<Uuid>) {
        self.dirty_sessions.retain(|id| !ids.contains(id));
    }

    pub fn push_session(&mut self, session: AgentSession) {
        self.dirty_sessions.insert(session.id);
        self.sessions.push(session);
    }

    /// Frees the resident transcripts of hydrated sessions that are persisted,
    /// unmodified, and not among the newest `keep` by `updated_at`.
    ///
    /// A daemon serving a long-lived agent session may otherwise adopt the
    /// full transcript of every session its clients have ever touched and hold
    /// them all in memory until it restarts. Hydration is a pure cache — every
    /// consumer reloads from the store when `detail_loaded` is false — so the
    /// daemon keeps only a small recency window resident. Pinned sessions
    /// (live runtimes) and dirty sessions (unsaved work) are never released.
    /// Incognito sessions are never released either: nothing on disk could
    /// restore them, so a release would silently drop the live transcript.
    ///
    /// Returns the number of transcripts released.
    pub fn trim_idle_transcripts(&mut self, pinned: &HashSet<Uuid>, keep: usize) -> usize {
        let mut candidates = self
            .sessions
            .iter()
            .enumerate()
            .filter_map(|(index, session)| {
                let has_transcript = !(session.messages.is_empty()
                    && session.transcript_blocks.is_empty()
                    && session.turns.is_empty()
                    && session.queued_messages.is_empty());
                if session.detail_loaded
                    && has_transcript
                    && !session.incognito
                    && !self.dirty_sessions.contains(&session.id)
                    && !pinned.contains(&session.id)
                {
                    Some((index, session.updated_at))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if candidates.len() <= keep {
            return 0;
        }
        // Newest by `updated_at` first; stable order keeps ties deterministic.
        candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut released = 0;
        for (index, _) in candidates.into_iter().skip(keep) {
            self.sessions[index].release_transcript();
            released += 1;
        }
        released
    }

    pub fn empty() -> Self {
        Self {
            version: STATE_VERSION,
            analytics_id: Uuid::new_v4(),
            analytics_enabled: true,
            projects: Vec::new(),
            sessions: Vec::new(),
            selected_project: None,
            selected_session: None,
            last_provider: ProviderKind::Codex,
            last_model: None,
            last_reasoning_effort: None,
            last_service_tier: None,
            last_context_window: None,
            remembered_model_traits: Vec::new(),
            recent_model_uses: Vec::new(),
            favorite_models: Vec::new(),
            theme: ThemeSettings::default(),
            language: AppLanguage::default(),
            sidebar_visible: true,
            right_panel_visible: false,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            right_panel_width: DEFAULT_RIGHT_PANEL_WIDTH,
            computer_use_enabled: false,
            computer_use_experiment_enabled: default_experiment_enabled(),
            computer_use_allowed_apps: Vec::new(),
            disabled_providers: Vec::new(),
            provider_binary_overrides: HashMap::new(),
            agent_tools_enabled: false,
            agent_settings_enabled: true,
            keep_awake: false,
            custom_commands: Vec::new(),
            subagents_enabled: default_experiment_enabled(),
            memory_experiment_enabled: default_experiment_enabled(),
            memory_models: Default::default(),
            title_models: Default::default(),
            project_map_enabled: default_experiment_enabled(),
            eval: None,
            route_classes: Default::default(),
            daemon_settings_extra: BTreeMap::new(),
            dirty_sessions: HashSet::new(),
        }
    }

    pub fn fresh(cwd: PathBuf) -> Self {
        let project = Project::from_path(cwd);
        let session = AgentSession::new(project.id, ProviderKind::Codex);
        Self {
            selected_project: Some(project.id),
            selected_session: Some(session.id),
            projects: vec![project],
            sessions: vec![session],
            ..Self::empty()
        }
    }

    pub fn new_session(&self, project_id: Uuid, provider: ProviderKind) -> AgentSession {
        let mut session = AgentSession::new(project_id, provider);
        if provider == self.last_provider {
            session.model.clone_from(&self.last_model);
            session
                .reasoning_effort
                .clone_from(&self.last_reasoning_effort);
            session.service_tier.clone_from(&self.last_service_tier);
            session.context_window.clone_from(&self.last_context_window);
        }
        session
    }

    pub fn remember_model_traits(
        &mut self,
        provider: ProviderKind,
        model: &str,
        reasoning_effort: Option<String>,
        service_tier: Option<String>,
        context_window: Option<String>,
    ) {
        let existing = self
            .remembered_model_traits
            .iter()
            .position(|traits| traits.provider == provider && traits.model == model);
        if reasoning_effort.is_none() && service_tier.is_none() && context_window.is_none() {
            if let Some(index) = existing {
                self.remembered_model_traits.remove(index);
            }
            return;
        }
        if let Some(index) = existing {
            let traits = &mut self.remembered_model_traits[index];
            traits.reasoning_effort = reasoning_effort;
            traits.service_tier = service_tier;
            traits.context_window = context_window;
        } else {
            self.remembered_model_traits.push(RememberedModelTraits {
                provider,
                model: model.to_owned(),
                reasoning_effort,
                service_tier,
                context_window,
            });
        }
    }

    pub fn model_traits_for(
        &self,
        provider: ProviderKind,
        model: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
        self.remembered_model_traits
            .iter()
            .find(|traits| traits.provider == provider && traits.model == model)
            .map(|traits| {
                (
                    traits.reasoning_effort.clone(),
                    traits.service_tier.clone(),
                    traits.context_window.clone(),
                )
            })
            .unwrap_or_default()
    }

    /// Moves `provider`/`model`/`effort` to the front of the recent list. The
    /// fast flag keys nothing — a rerun of the same selection on the other
    /// tier replaces the entry in place, so one effort slot ever holds rank.
    pub fn record_model_use(
        &mut self,
        provider: ProviderKind,
        model: &str,
        effort: Option<String>,
        fast: bool,
    ) {
        if let Some(index) = self.recent_model_uses.iter().position(|use_| {
            use_.provider == provider && use_.model == model && use_.effort == effort
        }) {
            self.recent_model_uses.remove(index);
        }
        self.recent_model_uses.insert(
            0,
            RecentModelUse {
                provider,
                model: model.to_owned(),
                effort,
                fast,
                used_at: crate::model::unix_time(),
            },
        );
        self.recent_model_uses.truncate(RECENT_MODEL_USES_LIMIT);
    }

    /// The row's recency rank, when this exact selection — fast flag included —
    /// is the variant last started. The sibling tier carries no rank, so only
    /// one of `effort` and `effort-fast` ever sorts into the recent section.
    pub fn recent_model_rank(
        &self,
        provider: ProviderKind,
        model: &str,
        effort: Option<&str>,
        fast: bool,
    ) -> Option<usize> {
        self.recent_model_uses.iter().position(|use_| {
            use_.provider == provider
                && use_.model == model
                && use_.effort.as_deref() == effort
                && use_.fast == fast
        })
    }

    fn app_settings(&self) -> AppSettings {
        AppSettings {
            analytics_enabled: self.analytics_enabled,
            favorite_models: self.favorite_models.clone(),
            theme: self.theme,
            language: self.language,
        }
    }

    pub fn daemon_settings(&self) -> crate::DaemonSettings {
        crate::DaemonSettings {
            computer_use_enabled: self.computer_use_enabled,
            computer_use_experiment_enabled: self.computer_use_experiment_enabled,
            computer_use_allowed_apps: self.computer_use_allowed_apps.clone(),
            disabled_providers: self.disabled_providers.clone(),
            provider_binary_overrides: self.provider_binary_overrides.clone(),
            agent_tools_enabled: self.agent_tools_enabled,
            agent_settings_enabled: self.agent_settings_enabled,
            keep_awake: self.keep_awake,
            custom_commands: self.custom_commands.clone(),
            subagents_enabled: self.subagents_enabled,
            memory_experiment_enabled: self.memory_experiment_enabled,
            memory_models: self.memory_models.clone(),
            title_models: self.title_models.clone(),
            project_map_enabled: self.project_map_enabled,
            eval: self.eval.clone(),
            route_classes: self.route_classes.clone(),
            auto_prompts: Vec::new(),
            // Integrations postdate the legacy combined document; defaults
            // are the correct migration (on in dev builds, opt-in in release).
            integrations_enabled: cfg!(debug_assertions),
            integrations: Vec::new(),
            integrations_proxy_token: String::new(),
            // The sandbox experiment postdates the document the same way.
            sandbox_experiment_enabled: cfg!(debug_assertions),
            sandbox_default_enabled: false,
            // Idle eviction postdates it too; absent means the default.
            runtime_idle_timeout_secs: None,
            // So does the review-train branch name — `qa` is the
            // migration.
            qa_branch: waku_protocol::settings::DEFAULT_QA_BRANCH.to_owned(),
            extra: self.daemon_settings_extra.clone(),
        }
    }

    fn app_state(&self) -> AppState {
        AppState {
            app_state_version: APP_STATE_VERSION,
            analytics_id: self.analytics_id,
            selected_project: self.selected_project,
            selected_session: self.persistable_selected_session(),
            last_provider: self.last_provider,
            last_model: self.last_model.clone(),
            last_reasoning_effort: self.last_reasoning_effort.clone(),
            last_service_tier: self.last_service_tier.clone(),
            last_context_window: self.last_context_window.clone(),
            remembered_model_traits: self.remembered_model_traits.clone(),
            recent_model_uses: self.recent_model_uses.clone(),
            sidebar_visible: self.sidebar_visible,
            right_panel_visible: self.right_panel_visible,
            sidebar_width: self.sidebar_width,
            right_panel_width: self.right_panel_width,
        }
    }

    fn apply_app_settings(&mut self, settings: AppSettings) {
        self.analytics_enabled = settings.analytics_enabled;
        self.favorite_models = settings.favorite_models;
        self.theme = settings.theme;
        self.language = settings.language;
    }

    pub fn apply_daemon_settings(&mut self, settings: crate::DaemonSettings) {
        self.computer_use_enabled = settings.computer_use_enabled;
        self.computer_use_experiment_enabled = settings.computer_use_experiment_enabled;
        self.computer_use_allowed_apps = settings.computer_use_allowed_apps;
        self.disabled_providers = settings.disabled_providers;
        self.provider_binary_overrides = settings.provider_binary_overrides;
        self.agent_tools_enabled = settings.agent_tools_enabled;
        self.agent_settings_enabled = settings.agent_settings_enabled;
        self.keep_awake = settings.keep_awake;
        self.custom_commands = settings.custom_commands;
        self.subagents_enabled = settings.subagents_enabled;
        self.memory_experiment_enabled = settings.memory_experiment_enabled;
        self.memory_models = settings.memory_models;
        self.title_models = settings.title_models;
        self.project_map_enabled = settings.project_map_enabled;
        self.eval = settings.eval;
        self.route_classes = settings.route_classes;
        self.daemon_settings_extra = settings.extra;
    }

    fn apply_app_state(&mut self, app_state: AppState) {
        self.analytics_id = app_state.analytics_id;
        self.selected_project = app_state.selected_project;
        self.selected_session = app_state.selected_session;
        self.last_provider = app_state.last_provider;
        self.last_model = app_state.last_model;
        self.last_reasoning_effort = app_state.last_reasoning_effort;
        self.last_service_tier = app_state.last_service_tier;
        self.last_context_window = app_state.last_context_window;
        self.remembered_model_traits = app_state.remembered_model_traits;
        self.recent_model_uses = app_state.recent_model_uses;
        self.sidebar_visible = app_state.sidebar_visible;
        self.right_panel_visible = app_state.right_panel_visible;
        self.sidebar_width = app_state.sidebar_width;
        self.right_panel_width = app_state.right_panel_width;
    }

    /// A session only earns a row once it has started; drafts stay in memory.
    /// A draft selection is stored as no selection at all, so relaunching
    /// recreates a draft and lands on the new-session page the user quit from.
    fn persistable_selected_session(&self) -> Option<Uuid> {
        self.selected_session.filter(|selected| {
            self.sessions
                .iter()
                .any(|session| session.id == *selected && session.has_started())
        })
    }

    fn ensure_runtime_session(&mut self) {
        if self.selected_session.is_some_and(|selected_session| {
            self.sessions
                .iter()
                .any(|session| session.id == selected_session)
        }) {
            return;
        }
        self.selected_session = None;
        let Some(project_id) = self.selected_project.filter(|selected_project| {
            self.projects
                .iter()
                .any(|project| project.id == *selected_project)
        }) else {
            return;
        };
        let session = self.new_session(project_id, self.last_provider);
        self.selected_session = Some(session.id);
        self.sessions.push(session);
    }

    fn migrate_loaded(&mut self) {
        for session in &mut self.sessions {
            let checkpoint_totals_current = session.turns.iter().all(|turn| {
                turn.checkpoint
                    .as_ref()
                    .is_none_or(crate::model::Checkpoint::totals_are_current)
            });
            let before = (
                session.turns.len(),
                session.last_reply_at,
                session.provider_cursor.is_some(),
            );
            session.migrate_legacy_state();
            session.backfill_last_reply_at();
            // Migration rewrote this session, so the stored row is stale.
            if !checkpoint_totals_current
                || before
                    != (
                        session.turns.len(),
                        session.last_reply_at,
                        session.provider_cursor.is_some(),
                    )
            {
                self.dirty_sessions.insert(session.id);
            }
        }
        self.version = STATE_VERSION;
        normalize_computer_app_grants(&mut self.computer_use_allowed_apps);
        self.backfill_remembered_selection();
    }

    fn backfill_remembered_selection(&mut self) {
        let Some(session) = self
            .selected_session
            .and_then(|selected| self.sessions.iter().find(|session| session.id == selected))
            .cloned()
        else {
            return;
        };
        if self.last_model.is_none() {
            self.last_model = session.model;
        }
        if self.last_reasoning_effort.is_none() {
            self.last_reasoning_effort = session.reasoning_effort;
        }
        if self.last_service_tier.is_none() {
            self.last_service_tier = session.service_tier;
        }
        if self.last_context_window.is_none() {
            self.last_context_window = session.context_window;
        }
    }
}

/// Rewrites inline `data:` payloads into blob references, in place.
///
/// Done on the way to disk so a screenshot is written once and then dropped
/// from memory: the transcript keeps a short reference, and rendering loads the
/// file through GPUI's image cache instead of base64-decoding on every frame.
fn externalize_blobs<'a>(
    sessions: impl IntoIterator<Item = &'a mut AgentSession>,
    blobs: &BlobStore,
) {
    for session in sessions {
        for block in &mut session.transcript_blocks {
            for activity in &mut block.activities {
                for image in &mut activity.image_urls {
                    if crate::blob_store::is_blob_reference(image) {
                        continue;
                    }
                    let stored = blobs.store_data_url(image);
                    if stored.len() < image.len() {
                        *image = stored;
                    }
                }
            }
        }
    }
}

/// Every blob reference named by any stored session.
///
/// Read from the database rather than from memory: a session that has not been
/// hydrated has an empty transcript in memory, and treating that as "owns no
/// images" would delete screenshots that are still in use.
fn live_blob_references(connection: &Connection) -> io::Result<HashSet<String>> {
    live_references(connection, crate::blob_store::BLOB_SCHEME)
}

fn live_attachment_references(connection: &Connection) -> io::Result<HashSet<String>> {
    live_references(connection, crate::attachments::ATTACHMENT_SCHEME)
}

fn live_references(connection: &Connection, scheme: &str) -> io::Result<HashSet<String>> {
    let mut statement = connection
        .prepare(
            "SELECT data FROM session_details
             UNION ALL
             SELECT attachments FROM messages WHERE attachments != '[]'",
        )
        .map_err(to_io_error)?;
    let mut references = HashSet::new();
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(to_io_error)?;
    for data in rows.filter_map(Result::ok) {
        collect_references(&data, scheme, &mut references);
    }
    Ok(references)
}

/// Scanning raw JSON keeps blob retention independent of how deeply a
/// reference is nested in transcript or composer-draft metadata.
fn collect_blob_references(data: &str, references: &mut HashSet<String>) {
    collect_references(data, crate::blob_store::BLOB_SCHEME, references);
}

fn collect_attachment_references(data: &str, references: &mut HashSet<String>) {
    collect_references(data, crate::attachments::ATTACHMENT_SCHEME, references);
}

fn collect_references(data: &str, scheme: &str, references: &mut HashSet<String>) {
    let mut rest = data;
    while let Some(start) = rest.find(scheme) {
        rest = &rest[start..];
        let end = rest.find('"').unwrap_or(rest.len());
        references.insert(rest[..end].to_owned());
        rest = &rest[end..];
    }
}

fn fingerprint(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn to_io_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

const SESSION_SEARCH_SNIPPET_CHARS: usize = 240;
const SESSION_SEARCH_CONTEXT_BEFORE_CHARS: usize = 72;

fn build_session_search_snippet(text: &str, query: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let char_count = normalized.chars().count();
    if char_count <= SESSION_SEARCH_SNIPPET_CHARS {
        return normalized;
    }

    // ASCII folding preserves UTF-8 byte offsets while covering the provider
    // and source-code text people search most often. Non-ASCII queries still
    // match exactly, including Simplified Chinese.
    let match_byte = normalized
        .to_ascii_lowercase()
        .find(&query.to_ascii_lowercase())
        .unwrap_or(0);
    let match_char = normalized[..match_byte].chars().count();
    let body_chars = SESSION_SEARCH_SNIPPET_CHARS.saturating_sub(4);
    let ideal_start = match_char.saturating_sub(SESSION_SEARCH_CONTEXT_BEFORE_CHARS);
    let start = ideal_start.min(char_count.saturating_sub(body_chars));
    let end = (start + body_chars).min(char_count);
    let body = normalized
        .chars()
        .skip(start)
        .take(end - start)
        .collect::<String>();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        body,
        if end < char_count { "…" } else { "" }
    )
}

fn search_session_messages(
    path: &Path,
    query: &str,
    limit: usize,
    scope: SessionMessageSearchScope,
    session_ids: Option<Vec<Uuid>>,
    last_turns: Option<usize>,
) -> io::Result<Vec<SessionMessageMatch>> {
    let query = query.trim();
    // An empty needle still scans when a session-id allowlist narrows the
    // corpus — the daemon sends that for filter-only queries like
    // `status:idle` — and each session's newest user message becomes the
    // excerpt. Callers guard truly blank queries.
    if limit == 0 || session_ids.as_ref().is_some_and(Vec::is_empty) {
        return Ok(Vec::new());
    }

    // The writer uses WAL, so an independent read-only connection can scan
    // history without taking the StateStore mutex or delaying a streaming save.
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(to_io_error)?;
    // The two surfaces are complementary: the palette scans active tasks, the
    // Archived settings page scans the archive. `archived:any` lifts that
    // partition for filtered queries.
    let archive_clause = match scope {
        SessionMessageSearchScope::Active => "sessions.archived_at IS NULL",
        SessionMessageSearchScope::Archived => "sessions.archived_at IS NOT NULL",
        SessionMessageSearchScope::Any => "1",
    };
    // Allowlist values are validated Uuids — their Display form is hex and
    // dashes only, so inlining them cannot inject SQL.
    let session_id_list = session_ids.map(|ids| {
        ids.iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",")
    });
    let session_clause = session_id_list
        .as_deref()
        .map(|ids| format!("sessions.id IN ({ids})"))
        .unwrap_or_else(|| "1".to_owned());
    // `last_turns` confines the scan to each session's N most recent turns:
    // rank the session's stored turn ids by their latest message position
    // and join the survivors, so messages belonging to no turn never match.
    let (turn_cte, turn_join) = match last_turns {
        Some(_) => (
            format!(
                "recent_turns AS (
                     SELECT session_id, turn_id
                       FROM (
                           SELECT session_id,
                                  turn_id,
                                  ROW_NUMBER() OVER (
                                      PARTITION BY session_id
                                      ORDER BY MAX(position) DESC
                                  ) AS turn_rank
                             FROM messages
                            WHERE turn_id IS NOT NULL{turn_filter}
                            GROUP BY session_id, turn_id
                       )
                      WHERE turn_rank <= ?3
                 ), ",
                turn_filter = session_id_list
                    .as_deref()
                    .map(|ids| format!(" AND session_id IN ({ids})"))
                    .unwrap_or_default(),
            ),
            "INNER JOIN recent_turns
                    ON recent_turns.session_id = messages.session_id
                   AND recent_turns.turn_id = messages.turn_id"
                .to_owned(),
        ),
        None => (String::new(), String::new()),
    };
    let mut statement = connection
        .prepare(&format!(
            "WITH {turn_cte}ranked AS (
                 SELECT messages.session_id,
                        messages.role,
                        messages.content,
                        messages.created_at,
                        sessions.updated_at AS session_updated_at,
                        CASE messages.role WHEN 'user' THEN 0 ELSE 1 END AS source_rank,
                        ROW_NUMBER() OVER (
                            PARTITION BY messages.session_id
                            ORDER BY CASE messages.role WHEN 'user' THEN 0 ELSE 1 END,
                                     messages.created_at DESC,
                                     messages.position DESC
                        ) AS session_match_rank
                   FROM messages
                   INNER JOIN sessions ON sessions.id = messages.session_id
                   {turn_join}
                  WHERE messages.streaming = 0
                    AND {archive_clause}
                    AND {session_clause}
                    AND messages.role IN ('user', 'assistant')
                    AND messages.hidden = 0
                    AND messages.notice IS NULL
                    AND instr(lower(messages.content), lower(?1)) > 0
             )
             SELECT session_id, role, content
               FROM ranked
              WHERE session_match_rank = 1
              ORDER BY source_rank, session_updated_at DESC, session_id
              LIMIT ?2",
        ))
        .map_err(to_io_error)?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let read_row = |row: &rusqlite::Row<'_>| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    };
    let rows = match last_turns {
        Some(turns) => {
            let turns = i64::try_from(turns).unwrap_or(i64::MAX);
            statement.query_map(params![query, limit, turns], read_row)
        }
        None => statement.query_map(params![query, limit], read_row),
    }
    .map_err(to_io_error)?;

    let mut matches = Vec::new();
    for row in rows {
        let (session_id, role, content) = row.map_err(to_io_error)?;
        let Ok(session_id) = Uuid::parse_str(&session_id) else {
            continue;
        };
        let source = match role.as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            _ => continue,
        };
        matches.push(SessionMessageMatch {
            session_id,
            source,
            snippet: build_session_search_snippet(&content, query),
        });
    }
    Ok(matches)
}

include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

const MIGRATIONS_TABLE: &str = "CREATE TABLE IF NOT EXISTS migrations (
         tag        TEXT PRIMARY KEY,
         applied_at INTEGER NOT NULL
     )";

/// Brings a database up to the latest schema.
///
/// Migrations are authored in `db/schema.ts` and generated by
/// `bun run db:generate`; `build.rs` embeds the resulting SQL in filename
/// order. Each one that is not already named in `migrations` runs in its own
/// transaction and is recorded, so applying is idempotent.
pub fn apply_migrations(connection: &Connection) -> io::Result<usize> {
    connection
        .execute_batch(MIGRATIONS_TABLE)
        .map_err(to_io_error)?;
    let mut applied = 0;
    for (tag, sql) in MIGRATIONS {
        let already_applied: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM migrations WHERE tag = ?1)",
                params![tag],
                |row| row.get(0),
            )
            .map_err(to_io_error)?;
        if already_applied {
            continue;
        }
        let transaction = connection.unchecked_transaction().map_err(to_io_error)?;
        transaction
            .execute_batch(sql)
            .map_err(|error| io::Error::other(format!("migration {tag} failed: {error}")))?;
        transaction
            .execute(
                "INSERT INTO migrations(tag, applied_at) VALUES(?1, ?2)",
                params![tag, crate::model::unix_time() as i64],
            )
            .map_err(to_io_error)?;
        transaction.commit().map_err(to_io_error)?;
        applied += 1;
    }
    Ok(applied)
}

struct Storage {
    connection: Connection,
    /// Sessions known to have a row. Used to spot deletions and to catch a
    /// session that became persistable without being marked dirty.
    persisted_sessions: HashSet<Uuid>,
    /// Per session, the fingerprint of each message row as this connection last
    /// wrote it, so a save only touches the messages that actually changed.
    /// See [`write_messages`].
    written_messages: HashMap<Uuid, HashMap<Uuid, u64>>,
    saved_projects: u64,
    saved_app_settings: u64,
    saved_app_state: u64,
}

pub struct StateStore {
    path: PathBuf,
    /// Client-local navigation and layout state stored beside the preview
    /// cache. It is never read by the daemon.
    app_state_path: PathBuf,
    /// Desktop-owned preferences. Debug stays isolated in the checkout while
    /// Release uses the explicit cross-client Goddard configuration directory.
    app_settings_path: PathBuf,
    /// Read-only migration sources for the former combined settings document.
    legacy_settings_paths: Vec<PathBuf>,
    storage: Mutex<Option<Storage>>,
    blobs: Arc<BlobStore>,
    desktop_files: bool,
}

impl StateStore {
    /// Where the database lives.
    ///
    /// Debug builds keep it in the checkout's gitignored `temp/`, so
    /// development never touches the installed app's data and a bad state is
    /// thrown away by deleting one directory. Release builds use the usual
    /// per-user application support directory.
    pub fn default_path() -> PathBuf {
        if cfg!(debug_assertions) {
            // `GODDARD_DATA_DIR` lets a second debug instance run beside the
            // first (friend-sharing smoke tests, isolated experiments)
            // without colliding on `temp/`.
            if let Some(dir) = std::env::var_os("GODDARD_DATA_DIR").filter(|dir| !dir.is_empty()) {
                return PathBuf::from(dir).join("app.db");
            }
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
                .join("temp")
                .join("app.db")
        } else {
            dirs::data_local_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join(DATA_DIRECTORY_NAME)
                .join("app.db")
        }
    }

    pub fn new(path: PathBuf) -> Self {
        let directory = path.parent().unwrap_or_else(|| Path::new(".")).to_owned();
        let configuration_directory = dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(crate::identity::HOME_DIRECTORY_NAME);
        let (app_settings_path, legacy_settings_paths) = if cfg!(debug_assertions) {
            (
                directory.join("app.json"),
                vec![directory.join("settings.json")],
            )
        } else {
            (
                configuration_directory.join("app.json"),
                vec![configuration_directory.join("settings.json")],
            )
        };
        Self::with_settings_paths(path, app_settings_path, legacy_settings_paths)
    }

    /// Local database owner used inside `goddard-daemon`. It never reads or
    /// writes desktop-only `app.json` or client navigation state.
    pub fn daemon(path: PathBuf) -> Self {
        let mut store = Self::new(path);
        store.desktop_files = false;
        store
    }

    fn with_settings_paths(
        path: PathBuf,
        app_settings_path: PathBuf,
        legacy_settings_paths: Vec<PathBuf>,
    ) -> Self {
        let directory = path.parent().unwrap_or_else(|| Path::new(".")).to_owned();
        let root = directory.join("blobs");
        let blobs = Arc::new(BlobStore::new(root));
        Self {
            app_state_path: directory.join("state.json"),
            app_settings_path,
            legacy_settings_paths,
            path,
            storage: Mutex::new(None),
            blobs,
            desktop_files: true,
        }
    }

    /// Builds a transcript-search job for the background executor.
    ///
    /// Constructing the job only clones the database path; opening SQLite and
    /// scanning message text happen when the returned closure runs off-thread.
    /// `session_ids` is the caller-resolved allowlist behind `project:` and
    /// `status:` filters — `None` scans every session in `scope`.
    /// `last_turns` confines the scan to each session's N most recent turns.
    pub fn session_message_search(
        &self,
        query: String,
        limit: usize,
        scope: SessionMessageSearchScope,
        session_ids: Option<Vec<Uuid>>,
        last_turns: Option<usize>,
    ) -> impl FnOnce() -> io::Result<Vec<SessionMessageMatch>> + Send + 'static {
        let path = self.path.clone();
        move || search_session_messages(&path, &query, limit, scope, session_ids, last_turns)
    }

    pub fn blobs(&self) -> Arc<BlobStore> {
        Arc::clone(&self.blobs)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn open(&self) -> io::Result<Connection> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.path).map_err(to_io_error)?;
        // WAL keeps a streaming save from blocking on readers, and NORMAL
        // sync is the right durability trade for per-second UI state.
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
            .map_err(to_io_error)?;
        apply_migrations(&connection)?;
        Ok(connection)
    }

    pub fn load_or_fresh(&self, cwd: PathBuf) -> PersistedState {
        let mut state = self.load().unwrap_or_else(|_| {
            if cwd.parent().is_none() {
                PersistedState::empty()
            } else {
                PersistedState::fresh(cwd)
            }
        });
        state.ensure_runtime_session();
        // The session that opens on launch is the one session whose transcript
        // is needed immediately; the rest stay as list rows until selected.
        if let Some(selected) = state.selected_session
            && let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.id == selected)
        {
            let _ = self.hydrate(session);
        }
        state
    }

    fn read_app_settings(&self) -> io::Result<Option<AppSettings>> {
        let source = match fs::read(&self.app_settings_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut migrated = None;
                for path in &self.legacy_settings_paths {
                    match fs::read(path) {
                        Ok(bytes) => {
                            migrated = Some(bytes);
                            break;
                        }
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
                migrated
            }
            Err(error) => return Err(error),
        };
        let Some(bytes) = source else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(to_io_error)
    }

    fn read_app_state(&self) -> io::Result<Option<AppState>> {
        let Ok(bytes) = fs::read(&self.app_state_path) else {
            return Ok(None);
        };
        // `state.json` used to be the pre-SQLite all-in-one store. Requiring a
        // format-specific version key makes that document (and malformed
        // app-managed state) safely reset instead of being migrated.
        let Ok(app_state) = serde_json::from_slice::<AppState>(&bytes) else {
            return Ok(None);
        };
        if app_state.app_state_version != APP_STATE_VERSION {
            return Ok(None);
        }
        Ok(Some(app_state))
    }

    fn write_app_settings(&self, settings: &AppSettings) -> io::Result<()> {
        if let Some(parent) = self.app_settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(settings).map_err(to_io_error)?;
        let temporary = self.app_settings_path.with_extension("json.tmp");
        fs::write(&temporary, data)?;
        fs::rename(temporary, &self.app_settings_path)
    }

    fn write_app_state(&self, app_state: &AppState) -> io::Result<()> {
        if let Some(parent) = self.app_state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(app_state).map_err(to_io_error)?;
        let temporary = self.app_state_path.with_extension("json.tmp");
        fs::write(&temporary, data)?;
        fs::rename(temporary, &self.app_state_path)
    }

    pub fn load(&self) -> io::Result<PersistedState> {
        let connection = self.open()?;
        let mut state = PersistedState::empty();

        // Missing JSON files mean defaults; the database remains the source of
        // truth for projects and sessions.
        let app_settings_missing = self.desktop_files && !self.app_settings_path.is_file();
        let app_settings = if self.desktop_files {
            self.read_app_settings()?
        } else {
            None
        };
        if let Some(settings) = app_settings {
            state.apply_app_settings(settings);
        }
        let app_state = if self.desktop_files {
            self.read_app_state()?
        } else {
            None
        };
        let app_state_missing = app_state.is_none();
        if let Some(app_state) = app_state {
            state.apply_app_state(app_state);
        }

        let mut projects = connection
            .prepare("SELECT id, name, path, created_at, bookmark, temporary, starred FROM projects ORDER BY position")
            .map_err(to_io_error)?;
        state.projects = projects
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<Vec<u8>>>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, bool>(6)?,
                ))
            })
            .map_err(to_io_error)?
            .filter_map(Result::ok)
            .filter_map(
                |(id, name, path, created_at, bookmark, temporary, starred)| {
                    Some(Project {
                        id: Uuid::parse_str(&id).ok()?,
                        name,
                        path: PathBuf::from(path),
                        bookmark,
                        created_at: created_at as u64,
                        temporary,
                        starred,
                    })
                },
            )
            .collect();
        drop(projects);

        // Only the columns the session list needs. Transcripts and messages are
        // fetched per session by `hydrate`, so startup cost does not grow with
        // how much history exists.
        let mut sessions = connection
            .prepare(
                "SELECT id, project_id, title, auto_title, provider, model, status,
                        created_at, updated_at, last_reply_at, archived_at, pinned_at,
                        dormant_at, dormant_exempt_until, landed_at, workspace, side_chat_of,
                        agent_rename_allowed, runtime_event_cursor
                 FROM sessions ORDER BY updated_at",
            )
            .map_err(to_io_error)?;
        let mut persisted_sessions = HashSet::new();
        state.sessions = sessions
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                    row.get::<_, Option<i64>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                    row.get::<_, bool>(17)?,
                    row.get::<_, Option<String>>(18)?,
                ))
            })
            .map_err(to_io_error)?
            .filter_map(Result::ok)
            .filter_map(|row| {
                let session = session_skeleton(row)?;
                persisted_sessions.insert(session.id);
                Some(session)
            })
            .collect();
        drop(sessions);

        state.migrate_loaded();
        let app_settings = state.app_settings();
        let app_settings_are_saved = !self.desktop_files
            || !app_settings_missing
            || self.write_app_settings(&app_settings).is_ok();
        let app_state = state.app_state();
        let app_state_is_saved = if !self.desktop_files {
            true
        } else if app_state_missing {
            // Persist the random installation ID before the first analytics
            // event is sent. Failure must not discard valid database state.
            self.write_app_state(&app_state).is_ok()
        } else {
            true
        };

        *self.storage.lock() = Some(Storage {
            connection,
            persisted_sessions,
            // A fresh connection has written nothing yet. Sessions loaded here
            // are skeletons anyway, so the first save of one is a full write.
            written_messages: HashMap::new(),
            saved_projects: 0,
            saved_app_settings: if app_settings_are_saved {
                fingerprint(&serde_json::to_string(&app_settings).map_err(to_io_error)?)
            } else {
                0
            },
            saved_app_state: if app_state_is_saved {
                fingerprint(&serde_json::to_string(&app_state).map_err(to_io_error)?)
            } else {
                0
            },
        });
        Ok(state)
    }

    /// Fills in a session's transcript, turns and messages.
    ///
    /// Startup loads only list columns, so this runs when a session is first
    /// selected. It reads one row plus that session's messages — cheap enough
    /// to do inline, and a no-op once the session is already loaded.
    pub fn hydrate(&self, session: &mut AgentSession) -> io::Result<()> {
        if session.detail_loaded {
            return Ok(());
        }
        match self.load_session_detail(session.id)? {
            Some(stored) => apply_session_detail(session, stored),
            // A session with no row has nothing stored to load; it is already
            // whole.
            None => session.detail_loaded = true,
        }
        Ok(())
    }

    /// Reads one session's stored detail row and messages.
    ///
    /// The read runs on its own connection, so it never queues behind the
    /// shared storage lock a save is holding for its write transaction — WAL
    /// gives the reader a consistent snapshot either way. The returned
    /// session is the stored blob verbatim plus its messages: its list
    /// columns may lag the sessions table (which gets column-only writes), so
    /// callers merge with [`apply_session_detail`] instead of adopting it.
    pub fn load_session_detail(&self, session_id: Uuid) -> io::Result<Option<AgentSession>> {
        let connection = self.open()?;
        let id = session_id.to_string();

        let data: Option<String> = connection
            .query_row(
                "SELECT data FROM session_details WHERE session_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(to_io_error)?;
        let Some(data) = data else {
            return Ok(None);
        };
        let mut stored = serde_json::from_str::<AgentSession>(&data).map_err(to_io_error)?;
        // Detail rows predate worktree names; the list row's migration is
        // overwritten when the stored workspace is copied over it.
        stored.workspace.backfill_worktree_name();

        let mut statement = connection
            .prepare(
                "SELECT id, turn_id, role, content, display_content, attachments, atoms,
                        created_at, streaming, sent_by_task, hidden, notice
                 FROM messages WHERE session_id = ?1 ORDER BY position",
            )
            .map_err(to_io_error)?;
        stored.messages = statement
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            })
            .map_err(to_io_error)?
            .filter_map(Result::ok)
            .filter_map(message_from_row)
            .collect();
        Ok(Some(stored))
    }

    /// Persists whatever the app marked as changed, so a streaming turn writes
    /// one session row and a selection change writes no rows at all.
    pub fn save(&self, state: &mut PersistedState) -> io::Result<()> {
        let batch = self.save_batch(state);
        self.write_batch(&batch)?;
        state.dirty_sessions.clear();
        Ok(())
    }

    /// Snapshots everything one save writes while the caller still holds the
    /// state lock, so the write itself — serialization plus the SQLite
    /// transaction — can run without it. Sessions entered state through a
    /// path that marks them dirty, so the dirty set is the complete write set.
    ///
    /// Callers that drop the state lock between `save_batch` and `write_batch`
    /// should remove `batch.dirty_ids` from the live dirty set up front and
    /// re-mark them if the write fails: a session re-dirtied mid-write then
    /// keeps its flag instead of losing it to a stale clear.
    pub(crate) fn save_batch(&self, state: &mut PersistedState) -> SaveBatch {
        // Only changed sessions can hold a new inline payload, so the blob walk
        // follows the same set rather than every transcript on every save.
        // Incognito sessions are excluded here too — externalizing would leak
        // message payloads to the blob store even with no session row written.
        let dirty = state.dirty_sessions.clone();
        externalize_blobs(
            state
                .sessions
                .iter_mut()
                .filter(|session| dirty.contains(&session.id) && !session.incognito),
            &self.blobs,
        );

        // A draft that has not started yet owns no row, so it counts as
        // removed until it does. Incognito sessions never own rows: they stay
        // out of `live_ids` (harmless — the delete sweep only walks
        // `persisted_sessions`, which they never enter) and skip both upserts.
        let live_ids = state
            .sessions
            .iter()
            .filter(|session| session.has_started() && !session.incognito)
            .map(|session| session.id)
            .collect();
        let persisted = self.persisted_session_ids();
        let sessions = state
            .sessions
            .iter()
            .filter(|session| {
                if !session.has_started() || session.incognito {
                    return false;
                }
                // A skeleton's empty transcript means "not fetched", not
                // "empty" — it writes list columns only, and only when dirty.
                if !session.detail_loaded {
                    return dirty.contains(&session.id);
                }
                dirty.contains(&session.id) || !persisted.contains(&session.id)
            })
            .cloned()
            .collect();
        SaveBatch {
            app_settings: state.app_settings(),
            app_state: state.app_state(),
            projects: state.projects.clone(),
            live_ids,
            sessions,
            dirty_ids: dirty,
        }
    }

    /// The ids this connection has already written, snapshot under the
    /// storage lock — a save's write set is dirty sessions plus any session
    /// the store has never seen.
    fn persisted_session_ids(&self) -> HashSet<Uuid> {
        self.storage
            .lock()
            .as_ref()
            .map(|storage| storage.persisted_sessions.clone())
            .unwrap_or_default()
    }

    /// Writes one snapshotted batch. Holds only the storage lock, so it can
    /// run after the caller's state lock is released.
    pub(crate) fn write_batch(&self, batch: &SaveBatch) -> io::Result<()> {
        let mut guard = self.storage.lock();
        if guard.is_none() {
            *guard = Some(Storage {
                connection: self.open()?,
                persisted_sessions: HashSet::new(),
                written_messages: HashMap::new(),
                saved_projects: 0,
                saved_app_settings: 0,
                saved_app_state: 0,
            });
        }
        let storage = guard.as_mut().expect("storage opened above");

        if self.desktop_files {
            let app_settings_fingerprint =
                fingerprint(&serde_json::to_string(&batch.app_settings).map_err(to_io_error)?);
            if app_settings_fingerprint != storage.saved_app_settings {
                self.write_app_settings(&batch.app_settings)?;
                storage.saved_app_settings = app_settings_fingerprint;
            }

            let app_state_fingerprint =
                fingerprint(&serde_json::to_string(&batch.app_state).map_err(to_io_error)?);
            if app_state_fingerprint != storage.saved_app_state {
                self.write_app_state(&batch.app_state)?;
                storage.saved_app_state = app_state_fingerprint;
            }
        }

        let transaction = storage
            .connection
            .unchecked_transaction()
            .map_err(to_io_error)?;

        let projects = serde_json::to_string(&batch.projects).map_err(to_io_error)?;
        let projects_fingerprint = fingerprint(&projects);
        if projects_fingerprint != storage.saved_projects {
            transaction
                .execute("DELETE FROM projects", [])
                .map_err(to_io_error)?;
            for (position, project) in batch.projects.iter().enumerate() {
                transaction
                    .execute(
                        INSERT_PROJECT,
                        params![
                            project.id.to_string(),
                            project.name,
                            project.path.to_string_lossy(),
                            project.bookmark,
                            position as i64,
                            project.created_at as i64,
                            project.temporary,
                            project.starred
                        ],
                    )
                    .map_err(to_io_error)?;
            }
            storage.saved_projects = projects_fingerprint;
        }

        // Applied only after the commit below, so a transaction that rolls back
        // does not leave this connection believing rows it never wrote are on
        // disk — which would make the next save skip them for good.
        let mut written_messages = Vec::new();
        for session in &batch.sessions {
            // A skeleton's empty transcript means "not fetched", not "empty".
            // Its promoted list columns may still have changed (for example,
            // an inactive sidebar row was renamed), so update only those and
            // leave the detail and message rows untouched.
            if !session.detail_loaded {
                transaction
                    .execute(
                        UPSERT_SESSION,
                        rusqlite::params_from_iter(session_params(session)),
                    )
                    .map_err(to_io_error)?;
                storage.persisted_sessions.insert(session.id);
                continue;
            }
            let data = session_data(session)?;
            transaction
                .execute(
                    UPSERT_SESSION,
                    rusqlite::params_from_iter(session_params(session)),
                )
                .map_err(to_io_error)?;
            transaction
                .execute(UPSERT_SESSION_DETAIL, params![session.id.to_string(), data])
                .map_err(to_io_error)?;
            written_messages.push((
                session.id,
                write_messages(
                    &transaction,
                    session,
                    storage.written_messages.get(&session.id).unwrap_or(&EMPTY),
                )?,
            ));
            storage.persisted_sessions.insert(session.id);
        }

        let removed = storage
            .persisted_sessions
            .iter()
            .copied()
            .filter(|id| !batch.live_ids.contains(id))
            .collect::<Vec<_>>();
        for id in removed {
            let key = id.to_string();
            transaction
                .execute("DELETE FROM sessions WHERE id = ?1", params![key])
                .map_err(to_io_error)?;
            transaction
                .execute(
                    "DELETE FROM session_details WHERE session_id = ?1",
                    params![key],
                )
                .map_err(to_io_error)?;
            transaction
                .execute("DELETE FROM messages WHERE session_id = ?1", params![key])
                .map_err(to_io_error)?;
            storage.persisted_sessions.remove(&id);
            storage.written_messages.remove(&id);
        }

        transaction.commit().map_err(to_io_error)?;
        // Now that the rows are durable, and not before.
        for (session_id, fingerprints) in written_messages {
            storage.written_messages.insert(session_id, fingerprints);
        }
        Ok(())
    }

    /// Builds a blob sweep.
    ///
    /// Both halves are filesystem and database work, so the whole thing runs on
    /// a background executor; it opens its own connection rather than borrowing
    /// the store's.
    pub fn blob_sweep(&self) -> impl FnOnce() + Send + 'static {
        let blobs = Arc::clone(&self.blobs);
        let path = self.path.clone();
        let attachments_root = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("attachments");
        let drafts_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(COMPOSER_DRAFTS_FILENAME);
        move || {
            let cutoff = SystemTime::now()
                .checked_sub(ASSET_SWEEP_GRACE_PERIOD)
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let Ok(connection) = Connection::open(&path) else {
                return;
            };
            let Ok(mut live) = live_blob_references(&connection) else {
                return;
            };
            if let Ok(drafts) = fs::read_to_string(drafts_path) {
                collect_blob_references(&drafts, &mut live);
            }
            let _ = blobs.retain_unreferenced_older_than(&live, cutoff);
            let Ok(mut live_attachments) = live_attachment_references(&connection) else {
                return;
            };
            if let Ok(drafts) = fs::read_to_string(
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(COMPOSER_DRAFTS_FILENAME),
            ) {
                collect_attachment_references(&drafts, &mut live_attachments);
            }
            let _ = crate::attachments::AttachmentStore::new(attachments_root)
                .retain_unreferenced_older_than(&live_attachments, cutoff);
        }
    }
}

type SessionColumns = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
);

/// Builds a list-only session from its columns. `messages`,
/// `transcript_blocks` and `turns` stay empty until [`StateStore::hydrate`].
///
/// Built field by field rather than through `AgentSession::new`, which would
/// spend a random-number syscall per row on an id that is then overwritten.
fn session_skeleton(row: SessionColumns) -> Option<AgentSession> {
    let (
        id,
        project_id,
        title,
        auto_title,
        provider,
        model,
        status,
        created_at,
        updated_at,
        last_reply_at,
        archived_at,
        pinned_at,
        dormant_at,
        dormant_exempt_until,
        landed_at,
        workspace,
        side_chat_of,
        agent_rename_allowed,
        runtime_event_cursor,
    ) = row;
    // The column duplicates the detail blob's workspace so list rows can show
    // it. Rows migrated before the column existed or whose JSON fails to parse
    // still come up local; `hydrate` restores the authoritative value.
    let mut workspace = workspace
        .and_then(|workspace| serde_json::from_str::<SessionWorkspace>(&workspace).ok())
        .unwrap_or_default();
    workspace.backfill_worktree_name();
    // Same duplication story as `workspace`: runtime attach resumes replay
    // dedup from the column without hydrating the session.
    let runtime_event_cursor = runtime_event_cursor
        .and_then(|cursor| serde_json::from_str::<RuntimeEventCursor>(&cursor).ok());
    Some(AgentSession {
        id: Uuid::parse_str(&id).ok()?,
        title,
        auto_title,
        project_id: Uuid::parse_str(&project_id).ok()?,
        workspace,
        workspace_moved_from: None,
        // Same duplication story as `workspace`: the daemon's side-chat
        // cascade and launch environment need it without a hydrate.
        side_chat_of: side_chat_of.and_then(|id| Uuid::parse_str(&id).ok()),
        agent_rename_allowed,
        provider: serde_json::from_value(serde_json::Value::String(provider)).ok()?,
        model,
        // Hydration replaces these; the list never reads them.
        runtime_mode: RuntimeMode::default(),
        environment: crate::model::SessionEnvironment::Local,
        sandboxed: false,
        reasoning_effort: None,
        service_tier: None,
        context_window: None,
        agent_preset: None,
        auto_route: false,
        route_decision: None,
        phase: None,
        status: serde_json::from_value(serde_json::Value::String(status)).ok()?,
        created_at: created_at as u64,
        updated_at: updated_at as u64,
        last_reply_at: last_reply_at.map(|at| at as u64),
        archived_at: archived_at.map(|at| at as u64),
        pinned_at: pinned_at.map(|at| at as u64),
        dormant_at: dormant_at.map(|at| at as u64),
        dormant_exempt_until: dormant_exempt_until.map(|at| at as u64),
        // Skeletons must not look quarantined: clients echo list projections
        // back through SaveTaskState, and a `true` here would poison real
        // sessions. The daemon gate reads full task_state; the UI card
        // requires detail_loaded.
        quarantined: false,
        landed_at: landed_at.map(|at| at as u64),
        provider_cursor: None,
        suspended_provider_sessions: Vec::new(),
        pending_provider_context: None,
        available_commands: Vec::new(),
        thread_goal: None,
        context_usage: None,
        runtime_event_cursor,
        provider_session_id: None,
        messages: Vec::new(),
        transcript_blocks: Vec::new(),
        turns: Vec::new(),
        queued_messages: Vec::new(),
        detail_loaded: false,
        // Stored rows are always ordinary sessions — incognito ones never
        // reach the store, so nothing persisted can deserialize as one.
        incognito: false,
    })
}

/// Moves a stored detail row's fields into `session`, keeping the live
/// session's list columns.
///
/// The field list is everything [`StateStore::hydrate`] fills in, kept in one
/// place so a session loaded on a background thread and one hydrated in place
/// cannot drift apart.
pub(crate) fn apply_session_detail(session: &mut AgentSession, stored: AgentSession) {
    // Resolved before the field moves — `environment()` borrows `stored`.
    let stored_environment = stored.environment();
    session.transcript_blocks = stored.transcript_blocks;
    session.turns = stored.turns;
    session.queued_messages = stored.queued_messages;
    session.workspace = stored.workspace;
    session.side_chat_of = stored.side_chat_of;
    session.provider_cursor = stored.provider_cursor;
    session.runtime_mode = stored.runtime_mode;
    session.reasoning_effort = stored.reasoning_effort;
    session.service_tier = stored.service_tier;
    session.context_window = stored.context_window;
    session.context_usage = stored.context_usage;
    session.runtime_event_cursor = stored.runtime_event_cursor;
    // Quarantine is detail, not a list column — the skeleton's flag is a
    // placeholder and the stored blob carries the real value.
    session.quarantined = stored.quarantined;
    session.environment = stored_environment;
    session.messages = stored.messages;
    session.detail_loaded = true;
}

/// Everything one save writes, snapshotted by [`StateStore::save_batch`] while
/// the caller's state lock is held so [`StateStore::write_batch`] can run its
/// serialization and SQLite transaction without it.
pub(crate) struct SaveBatch {
    app_settings: AppSettings,
    app_state: AppState,
    projects: Vec<Project>,
    /// Every started, non-incognito session id — the delete sweep's live set.
    live_ids: HashSet<Uuid>,
    /// The dirty sessions to write. Skeletons carry no detail and write their
    /// list columns only.
    sessions: Vec<AgentSession>,
    /// The dirty set as captured. Callers racing the write remove these marks
    /// up front and re-add them on failure, so a session re-dirtied mid-write
    /// keeps its flag.
    pub(crate) dirty_ids: HashSet<Uuid>,
}

type MessageColumns = (
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    String,
    String,
    i64,
    i64,
    Option<String>,
    i64,
    Option<String>,
);

fn message_from_row(row: MessageColumns) -> Option<Message> {
    let (
        id,
        turn_id,
        role,
        content,
        display_content,
        attachments,
        atoms,
        created_at,
        streaming,
        sent_by_task,
        hidden,
        notice,
    ) = row;
    Some(Message {
        id: Uuid::parse_str(&id).ok()?,
        turn_id: turn_id.as_deref().and_then(|id| Uuid::parse_str(id).ok()),
        role: serde_json::from_value(serde_json::Value::String(role)).ok()?,
        content,
        notice: notice.and_then(|json| serde_json::from_str::<TranscriptNotice>(&json).ok()),
        display_content,
        attachments: serde_json::from_str::<Vec<MessageAttachment>>(&attachments)
            .unwrap_or_default(),
        atoms: serde_json::from_str::<Vec<MessageAtom>>(&atoms).unwrap_or_default(),
        created_at: created_at as u64,
        streaming: streaming != 0,
        sent_by_task: sent_by_task
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok()),
        hidden: hidden != 0,
    })
}

/// Serializes a session for the `data` column, omitting `messages`.
///
/// They are rows in `messages` instead, so there is no copy in `data` that
/// could go stale.
fn session_data(session: &AgentSession) -> io::Result<String> {
    let mut value = serde_json::to_value(session).map_err(to_io_error)?;
    if let Some(object) = value.as_object_mut() {
        object.remove("messages");
    }
    serde_json::to_string(&value).map_err(to_io_error)
}

const UPSERT_MESSAGE: &str = "INSERT INTO messages(
         id, session_id, turn_id, position, role, content, display_content,
         attachments, atoms, created_at, streaming, sent_by_task, hidden, notice
     ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
     ON CONFLICT(id) DO UPDATE SET
         session_id = excluded.session_id,
         turn_id    = excluded.turn_id,
         position   = excluded.position,
         role       = excluded.role,
         content    = excluded.content,
         display_content = excluded.display_content,
         attachments = excluded.attachments,
         atoms      = excluded.atoms,
         created_at = excluded.created_at,
         streaming  = excluded.streaming,
         sent_by_task = excluded.sent_by_task,
         hidden     = excluded.hidden,
         notice     = excluded.notice";

/// Replaces a session's messages with the given list.
///
/// Appending during a turn touches only the new rows; the delete clears any
/// tail left behind when a conversation is forked or truncated.
/// Writes the messages whose stored row would actually differ.
///
/// A streaming turn saves once a second, and every one of those saves used to
/// re-upsert the whole transcript: a `to_string` of the id, a clone of the
/// body, a `serde_json` round-trip for the role, and a statement, per message.
/// At two thousand messages that is upwards of 15ms — several frames — spent
/// rewriting rows that are byte-for-byte what SQLite already holds.
///
/// `written` is what the connection was last told, keyed by message id, so the
/// comparison costs one hash of each body instead of a write of it. Returns the
/// map to remember for next time; the caller installs it only once the
/// transaction commits, so a rolled-back write is not recorded as done.
fn write_messages(
    transaction: &Connection,
    session: &AgentSession,
    written: &HashMap<Uuid, u64>,
) -> io::Result<HashMap<Uuid, u64>> {
    use rusqlite::types::Value;
    let session_id = session.id.to_string();
    let mut current = HashMap::with_capacity(session.messages.len());
    for (position, message) in session.messages.iter().enumerate() {
        let fingerprint = message_fingerprint(message, position);
        current.insert(message.id, fingerprint);
        if written.get(&message.id) == Some(&fingerprint) {
            continue;
        }
        let attachments = if message.attachments.is_empty() {
            "[]".to_owned()
        } else {
            serde_json::to_string(&message.attachments).map_err(to_io_error)?
        };
        let atoms = if message.atoms.is_empty() {
            "[]".to_owned()
        } else {
            serde_json::to_string(&message.atoms).map_err(to_io_error)?
        };
        let notice = message
            .notice
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(to_io_error)?;
        transaction
            .execute(
                UPSERT_MESSAGE,
                rusqlite::params_from_iter([
                    Value::Text(message.id.to_string()),
                    Value::Text(session_id.clone()),
                    message
                        .turn_id
                        .map_or(Value::Null, |id| Value::Text(id.to_string())),
                    Value::Integer(position as i64),
                    Value::Text(tag_of(message.role)),
                    Value::Text(message.content.clone()),
                    message
                        .display_content
                        .clone()
                        .map_or(Value::Null, Value::Text),
                    Value::Text(attachments),
                    Value::Text(atoms),
                    Value::Integer(message.created_at as i64),
                    Value::Integer(i64::from(message.streaming)),
                    message
                        .sent_by_task
                        .map_or(Value::Null, |id| Value::Text(id.to_string())),
                    Value::Integer(i64::from(message.hidden)),
                    notice.map_or(Value::Null, Value::Text),
                ]),
            )
            .map_err(to_io_error)?;
    }
    transaction
        .execute(
            "DELETE FROM messages WHERE session_id = ?1 AND position >= ?2",
            params![session_id, session.messages.len() as i64],
        )
        .map_err(to_io_error)?;
    Ok(current)
}

/// Fingerprint of every column [`write_messages`] stores for a message.
///
/// Covers the body as well as the metadata: an edit that preserves length —
/// a typo fix — has to be caught, so length and position alone will not do.
/// Folded a word at a time because this runs over the whole transcript on
/// every save, which is what [`fingerprint`]'s byte-at-a-time loop is too slow
/// for.
/// Stand-in for "this connection has written nothing for that session yet".
static EMPTY: std::sync::LazyLock<HashMap<Uuid, u64>> = std::sync::LazyLock::new(HashMap::new);

fn message_fingerprint(message: &Message, position: usize) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut fold = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(PRIME).rotate_left(23);
    };

    fold(position as u64);
    fold(message.created_at);
    fold(u64::from(message.streaming));
    fold(u64::from(message.hidden));
    fold(fingerprint(&tag_of(message.role)));
    let (high, low) = message.id.as_u64_pair();
    fold(high);
    fold(low);
    match message.turn_id {
        Some(turn_id) => {
            let (high, low) = turn_id.as_u64_pair();
            fold(high);
            fold(low);
        }
        // Distinct from a turn id that happens to be zero.
        None => fold(u64::MAX),
    }
    match message.sent_by_task {
        Some(sent_by_task) => {
            let (high, low) = sent_by_task.as_u64_pair();
            fold(high);
            fold(low);
        }
        None => fold(u64::MAX),
    }

    let bytes = message.content.as_bytes();
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        fold(u64::from_le_bytes(
            chunk.try_into().expect("chunks_exact yields 8 bytes"),
        ));
    }
    let mut tail = [0u8; 8];
    let remainder = chunks.remainder();
    tail[..remainder.len()].copy_from_slice(remainder);
    fold(u64::from_le_bytes(tail));
    fold(bytes.len() as u64);
    if let Some(display_content) = &message.display_content {
        fold(1);
        fold(fingerprint(display_content));
    } else {
        fold(0);
    }
    // Notices are rare enough that serializing one to compare beats hashing
    // every message's empty case differently.
    if let Some(notice) = &message.notice {
        fold(1);
        if let Ok(json) = serde_json::to_string(notice) {
            fold(fingerprint(&json));
        }
    } else {
        fold(0);
    }
    fold(message.attachments.len() as u64);
    for attachment in &message.attachments {
        fold(fingerprint(&attachment.path.to_string_lossy()));
        fold(fingerprint(&attachment.mention));
        fold(fingerprint(&attachment.name));
        fold(u64::from(attachment.is_dir));
        fold(u64::from(attachment.is_image));
        if let Some(reference) = &attachment.blob_reference {
            fold(1);
            fold(fingerprint(reference));
        } else {
            fold(0);
        }
    }
    for atom in &message.atoms {
        fold(fingerprint(&atom.label));
        fold(fingerprint(&atom.payload));
        match atom.session_id {
            Some(session_id) => {
                let (high, low) = session_id.as_u64_pair();
                fold(high);
                fold(low);
            }
            None => fold(u64::MAX),
        }
    }
    hash
}

/// Columns the sidebar sorts and filters on are stored alongside the JSON so
/// listing sessions never has to deserialize a transcript.
const UPSERT_SESSION: &str = "INSERT INTO sessions(
         id, project_id, title, auto_title, provider, model, status,
         created_at, updated_at, last_reply_at, archived_at, pinned_at,
         dormant_at, dormant_exempt_until, landed_at, workspace, side_chat_of,
         agent_rename_allowed, runtime_event_cursor
     ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
     ON CONFLICT(id) DO UPDATE SET
         project_id    = excluded.project_id,
         title         = excluded.title,
         auto_title    = excluded.auto_title,
         provider      = excluded.provider,
         model         = excluded.model,
         status        = excluded.status,
         created_at    = excluded.created_at,
         updated_at    = excluded.updated_at,
         last_reply_at = excluded.last_reply_at,
         archived_at   = excluded.archived_at,
         pinned_at     = excluded.pinned_at,
         dormant_at    = excluded.dormant_at,
         dormant_exempt_until = excluded.dormant_exempt_until,
         landed_at     = excluded.landed_at,
         workspace     = excluded.workspace,
         side_chat_of  = excluded.side_chat_of,
         agent_rename_allowed = excluded.agent_rename_allowed,
         runtime_event_cursor = excluded.runtime_event_cursor";

const INSERT_PROJECT: &str =
    "INSERT INTO projects(id, name, path, bookmark, position, created_at, temporary, starred)
     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
     ON CONFLICT(id) DO UPDATE SET
         name       = excluded.name,
         path       = excluded.path,
         bookmark   = excluded.bookmark,
         position   = excluded.position,
         created_at = excluded.created_at,
         temporary  = excluded.temporary,
         starred    = excluded.starred";

/// The transcript, written alongside the list row it belongs to.
const UPSERT_SESSION_DETAIL: &str = "INSERT INTO session_details(session_id, data)
     VALUES(?1, ?2)
     ON CONFLICT(session_id) DO UPDATE SET data = excluded.data";

/// Serializes an enum to the same string the JSON blob uses, so a column and
/// its JSON counterpart can never disagree about spelling.
fn tag_of(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn session_params(session: &AgentSession) -> Vec<rusqlite::types::Value> {
    use rusqlite::types::Value;
    vec![
        Value::Text(session.id.to_string()),
        Value::Text(session.project_id.to_string()),
        Value::Text(session.title.clone()),
        session.auto_title.clone().map_or(Value::Null, Value::Text),
        Value::Text(tag_of(session.provider)),
        session.model.clone().map_or(Value::Null, Value::Text),
        Value::Text(tag_of(session.status)),
        Value::Integer(session.created_at as i64),
        Value::Integer(session.updated_at as i64),
        session
            .last_reply_at
            .map_or(Value::Null, |at| Value::Integer(at as i64)),
        session
            .archived_at
            .map_or(Value::Null, |at| Value::Integer(at as i64)),
        session
            .pinned_at
            .map_or(Value::Null, |at| Value::Integer(at as i64)),
        session
            .dormant_at
            .map_or(Value::Null, |at| Value::Integer(at as i64)),
        session
            .dormant_exempt_until
            .map_or(Value::Null, |at| Value::Integer(at as i64)),
        session
            .landed_at
            .map_or(Value::Null, |at| Value::Integer(at as i64)),
        // Local stays NULL the way the detail blob omits it, so the column is
        // only ever populated for sessions that live in a worktree.
        if session.workspace.is_local() {
            Value::Null
        } else {
            serde_json::to_string(&session.workspace).map_or(Value::Null, Value::Text)
        },
        session
            .side_chat_of
            .map_or(Value::Null, |id| Value::Text(id.to_string())),
        Value::Integer(i64::from(session.agent_rename_allowed)),
        // NULL until the session first streams events, like the workspace
        // column: duplicated detail so attach need not hydrate.
        session
            .runtime_event_cursor
            .as_ref()
            .and_then(|cursor| serde_json::to_string(cursor).ok())
            .map_or(Value::Null, Value::Text),
    ]
}

fn normalize_computer_app_grants(grants: &mut Vec<ComputerAppGrant>) {
    let mut seen_bundle_ids = HashSet::new();
    grants.retain(|grant| {
        !grant.bundle_id.trim().is_empty() && seen_bundle_ids.insert(grant.bundle_id.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ActivityItem, ActivityKind, FavoriteModel, MessageRole, ReasoningBlock, TranscriptBlock,
        TranscriptNoticeStatus,
    };
    use base64::Engine as _;

    fn temporary_directory() -> PathBuf {
        std::env::temp_dir().join(format!("waku-state-{}", Uuid::new_v4()))
    }

    fn store_in(directory: &Path) -> StateStore {
        StateStore::with_settings_paths(
            directory.join("app.db"),
            directory.join("app.json"),
            vec![directory.join("settings.json")],
        )
    }

    /// `load` returns list-only sessions by design; tests that assert on
    /// transcripts fetch them the way the app does when a session is opened.
    fn load_hydrated(store: &StateStore) -> PersistedState {
        let mut state = store.load().unwrap();
        for session in &mut state.sessions {
            store.hydrate(session).unwrap();
        }
        state
    }

    fn text_draft(text: &str) -> ComposerDraft {
        ComposerDraft {
            text: text.to_owned(),
            attachments: Vec::new(),
            annotations: Vec::new(),
            inline_atoms: Vec::new(),
        }
    }

    #[test]
    fn panel_visibility_defaults_keep_only_the_sidebar_open() {
        let state = PersistedState::empty();
        assert!(state.sidebar_visible);
        assert!(!state.right_panel_visible);

        let mut app_state = serde_json::to_value(state.app_state()).unwrap();
        let app_state = app_state.as_object_mut().unwrap();
        app_state.remove("sidebar_visible");
        app_state.remove("right_panel_visible");
        let restored: AppState = serde_json::from_value(app_state.clone().into()).unwrap();

        assert!(restored.sidebar_visible);
        assert!(!restored.right_panel_visible);
    }

    #[test]
    fn analytics_preference_and_identity_use_their_respective_files() {
        let mut state = PersistedState::empty();
        state.analytics_enabled = false;
        let analytics_id = state.analytics_id;
        let mut settings = serde_json::to_value(state.app_settings()).unwrap();

        let restored: AppSettings = serde_json::from_value(settings.clone()).unwrap();
        assert!(!restored.analytics_enabled);
        assert!(settings.get("analytics_id").is_none());

        let app_state: AppState =
            serde_json::from_value(serde_json::to_value(state.app_state()).unwrap()).unwrap();
        assert_eq!(app_state.analytics_id, analytics_id);

        settings
            .as_object_mut()
            .unwrap()
            .remove("analytics_enabled");
        let backfilled: AppSettings = serde_json::from_value(settings).unwrap();
        assert!(backfilled.analytics_enabled);
    }

    #[test]
    fn missing_settings_and_app_state_are_created_during_load() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let restored = store.load().unwrap();
        let settings_path = directory.join("app.json");
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
        let app_state: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("state.json")).unwrap()).unwrap();

        assert_eq!(settings["analytics_enabled"], true);
        assert!(settings.get("analytics_id").is_none());
        assert_eq!(app_state["analytics_id"], restored.analytics_id.to_string());
        assert!(app_state.get("analytics_enabled").is_none());

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn legacy_combined_settings_migrate_app_fields_without_claiming_daemon_fields() {
        let directory = temporary_directory();
        fs::create_dir_all(&directory).unwrap();
        let legacy_path = directory.join("settings.json");
        let legacy = r#"{
            "theme": "dark",
            "analytics_enabled": false,
            "computer_use_enabled": true,
            "disabled_providers": ["claude"]
        }"#;
        fs::write(&legacy_path, legacy).unwrap();

        let restored = store_in(&directory).load().unwrap();
        assert_eq!(
            restored.theme,
            ThemeSettings {
                mode: crate::theme::ThemeMode::Dark,
                ..ThemeSettings::default()
            }
        );
        assert!(!restored.analytics_enabled);

        let app: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("app.json")).unwrap()).unwrap();
        assert_eq!(
            app["theme"],
            serde_json::json!({"mode": "dark", "light": "light", "dark": "dark"})
        );
        assert!(app.get("computer_use_enabled").is_none());
        assert!(app.get("disabled_providers").is_none());
        assert_eq!(fs::read_to_string(legacy_path).unwrap(), legacy);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn computer_use_defaults_to_disabled() {
        let state = PersistedState::empty();
        assert!(!state.computer_use_enabled);

        let mut settings = serde_json::to_value(state.daemon_settings()).unwrap();
        settings
            .as_object_mut()
            .unwrap()
            .remove("computer_use_enabled");
        let restored: crate::DaemonSettings = serde_json::from_value(settings).unwrap();

        assert!(!restored.computer_use_enabled);
    }

    #[test]
    fn settings_accept_a_partial_user_authored_document() {
        let settings: AppSettings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();

        assert_eq!(
            settings.theme,
            ThemeSettings {
                mode: crate::theme::ThemeMode::Dark,
                ..ThemeSettings::default()
            }
        );
        assert_eq!(settings.language, AppLanguage::System);
        assert!(settings.analytics_enabled);
    }

    #[test]
    fn legacy_all_in_one_state_is_not_migrated() {
        let directory = temporary_directory();
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("state.json"),
            r#"{"version":1,"sidebar_width":333.0,"right_panel_visible":true}"#,
        )
        .unwrap();

        let restored = store_in(&directory).load().unwrap();
        assert_eq!(restored.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
        assert!(!restored.right_panel_visible);

        let rewritten: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("state.json")).unwrap()).unwrap();
        assert_eq!(rewritten["app_state_version"], APP_STATE_VERSION);
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn new_session_drafts_follow_the_project_across_runtime_session_ids() {
        let project_id = Uuid::new_v4();
        let first_runtime_session = AgentSession::new(project_id, ProviderKind::Codex);
        let relaunched_runtime_session = AgentSession::new(project_id, ProviderKind::Codex);
        assert_ne!(first_runtime_session.id, relaunched_runtime_session.id);

        let mut drafts = ComposerDrafts::default();
        let draft = text_draft("unfinished new task");
        assert!(drafts.set(
            ComposerDraftKey::for_session(&first_runtime_session),
            draft.clone()
        ));
        assert_eq!(
            drafts.get_for(&relaunched_runtime_session),
            Some(&draft),
            "the blank session's transient UUID must not own its draft"
        );
    }

    #[test]
    fn existing_session_drafts_are_isolated_by_session_id() {
        let project_id = Uuid::new_v4();
        let mut first = AgentSession::new(project_id, ProviderKind::Codex);
        let mut second = AgentSession::new(project_id, ProviderKind::Codex);
        first.begin_turn("first task");
        second.begin_turn("second task");

        let mut drafts = ComposerDrafts::default();
        let first_draft = text_draft("follow up one");
        let second_draft = text_draft("follow up two");
        drafts.set(ComposerDraftKey::for_session(&first), first_draft.clone());
        drafts.set(ComposerDraftKey::for_session(&second), second_draft.clone());

        assert_eq!(drafts.get_for(&first), Some(&first_draft));
        assert_eq!(drafts.get_for(&second), Some(&second_draft));
    }

    #[test]
    fn composer_project_change_moves_a_draft_only_to_an_empty_destination() {
        let source = ComposerDraftKey::NewSession(Uuid::new_v4());
        let destination = ComposerDraftKey::NewSession(Uuid::new_v4());
        let draft = text_draft("keep this prompt");
        let mut drafts = ComposerDrafts::default();
        drafts.set(source, draft.clone());

        assert!(drafts.move_to_empty(source, destination));
        assert!(drafts.get(source).is_none());
        assert_eq!(drafts.get(destination), Some(&draft));

        let occupied = ComposerDraftKey::NewSession(Uuid::new_v4());
        let parked = text_draft("already parked here");
        drafts.set(occupied, parked.clone());
        assert!(!drafts.move_to_empty(destination, occupied));
        assert_eq!(drafts.get(destination), Some(&draft));
        assert_eq!(drafts.get(occupied), Some(&parked));
    }

    #[test]
    fn composer_drafts_round_trip_text_and_attachment_metadata() {
        let directory = temporary_directory();
        let store = ComposerDraftStore::for_state_path(&directory.join("app.db"));
        let project_id = Uuid::new_v4();
        let draft = ComposerDraft {
            text: "compare these".to_owned(),
            attachments: vec![ComposerDraftAttachment {
                path: PathBuf::from("/tmp/reference image.png"),
                mention: "/tmp/reference image.png".to_owned(),
                name: "reference image.png".to_owned(),
                is_dir: false,
                is_image: true,
                blob_reference: None,
                pasted_text_preview: None,
                session_id: None,
            }],
            annotations: Vec::new(),
            inline_atoms: Vec::new(),
        };
        let mut drafts = ComposerDrafts::default();
        drafts.set(ComposerDraftKey::NewSession(project_id), draft.clone());

        store.save(drafts, 1).unwrap();
        let restored = store.load().unwrap();
        assert_eq!(
            restored.get(ComposerDraftKey::NewSession(project_id)),
            Some(&draft)
        );
    }

    #[test]
    fn composer_draft_changes_preserve_unrelated_client_keys() {
        let directory = temporary_directory();
        let store = ComposerDraftStore::for_state_path(&directory.join("app.db"));
        let desktop_session_id = Uuid::new_v4();
        let web_project_id = Uuid::new_v4();
        let desktop_draft = text_draft("desktop follow-up");
        let web_draft = text_draft("web new task");
        let mut initial = ComposerDrafts::default();
        initial.set(
            ComposerDraftKey::Session(desktop_session_id),
            desktop_draft.clone(),
        );
        store.save(initial, 1).unwrap();

        store
            .apply_changes(vec![ComposerDraftChange {
                target: ComposerDraftTarget::NewSession {
                    project_id: web_project_id,
                },
                draft: Some(web_draft.clone()),
            }])
            .unwrap();

        let restored = store.load().unwrap();
        assert_eq!(
            restored.get(ComposerDraftKey::Session(desktop_session_id)),
            Some(&desktop_draft)
        );
        assert_eq!(
            restored.get(ComposerDraftKey::NewSession(web_project_id)),
            Some(&web_draft)
        );
    }

    #[test]
    fn empty_and_older_composer_drafts_cannot_resurface() {
        let directory = temporary_directory();
        let store = ComposerDraftStore::for_state_path(&directory.join("app.db"));
        let session_id = Uuid::new_v4();
        let key = ComposerDraftKey::Session(session_id);
        let mut latest = ComposerDrafts::default();
        latest.set(key, text_draft("latest"));
        store.save(latest, 2).unwrap();

        let mut stale = ComposerDrafts::default();
        stale.set(key, text_draft("stale"));
        store.save(stale, 1).unwrap();
        assert_eq!(store.load().unwrap().get(key), Some(&text_draft("latest")));

        let mut removed = store.load().unwrap();
        assert!(removed.set(key, ComposerDraft::default()));
        assert!(!removed.set(key, ComposerDraft::default()));
        store.save(removed, 3).unwrap();
        assert!(store.load().unwrap().get(key).is_none());
    }

    #[test]
    fn projects_round_trip_as_columns_with_created_at() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/some project"));
        state.projects[0].starred = true;
        let project = state.projects[0].clone();
        assert!(project.created_at > 0, "a new project is dated");
        store.save(&mut state).unwrap();

        // Stored as columns, not as a JSON blob.
        let connection = Connection::open(directory.join("app.db")).unwrap();
        let (name, path, created_at): (String, String, i64) = connection
            .query_row(
                "SELECT name, path, created_at FROM projects WHERE id = ?1",
                params![project.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, project.name);
        assert_eq!(path, project.path.to_string_lossy());
        assert_eq!(created_at as u64, project.created_at);
        drop(connection);

        let restored = store_in(&directory).load().unwrap();
        assert_eq!(restored.projects[0].name, project.name);
        assert_eq!(restored.projects[0].path, project.path);
        assert_eq!(restored.projects[0].created_at, project.created_at);
        assert!(restored.projects[0].starred);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn incognito_sessions_never_reach_the_store() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));

        let mut incognito = AgentSession::new(state.projects[0].id, ProviderKind::Codex);
        incognito.incognito = true;
        incognito.begin_turn("off the record");
        incognito.push_message(MessageRole::Assistant, "a secret answer");
        incognito.finish_active_turn(crate::model::TurnStatus::Completed);
        let incognito_id = incognito.id;
        state.push_session(incognito);
        store.save(&mut state).unwrap();

        // No session row, and no message rows either — the transcript went
        // nowhere near SQLite.
        let connection = Connection::open(directory.join("app.db")).unwrap();
        let session_rows: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = ?1",
                params![incognito_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_rows, 0);
        let secret_rows: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE content LIKE '%secret%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(secret_rows, 0);
        drop(connection);

        let restored = store_in(&directory).load().unwrap();
        assert!(
            !restored
                .sessions
                .iter()
                .any(|session| session.id == incognito_id),
            "an incognito session must not survive a store round-trip"
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn runtime_event_cursor_round_trips_on_the_skeleton() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Ask");
        let cursor = RuntimeEventCursor {
            runtime_id: Uuid::new_v4(),
            epoch: Uuid::new_v4(),
            sequence: 7,
        };
        state.sessions[0].runtime_event_cursor = Some(cursor);
        store.save(&mut state).unwrap();

        let mut restored = store_in(&directory).load().unwrap();
        // Attach reads the cursor straight off the list row — no hydrate.
        assert!(!restored.sessions[0].detail_loaded);
        assert_eq!(restored.sessions[0].runtime_event_cursor, Some(cursor));

        // Hydrating over it keeps the same detail value.
        let reopened = store_in(&directory);
        reopened.hydrate(&mut restored.sessions[0]).unwrap();
        assert_eq!(restored.sessions[0].runtime_event_cursor, Some(cursor));

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn load_returns_list_columns_and_hydrate_fills_the_transcript() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let id = state.sessions[0].id;
        state.sessions[0].auto_title = Some("Investigate".into());
        state.sessions[0].workspace = SessionWorkspace::Worktree {
            path: PathBuf::from("/tmp/worktrees/investigate"),
            name: "investigate".into(),
            branch: Some("waku/investigate".into()),
            base_branch: None,
        };
        state.sessions[0].begin_turn("Ask");
        state.sessions[0].push_message(MessageRole::Assistant, "an answer");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        let session = &restored.sessions[0];
        // The list has everything it renders — including the workspace, a
        // column of its own so the sidebar's worktree state survives restart
        // without hydrating every session.
        assert_eq!(session.title, AgentSession::DEFAULT_TITLE);
        assert_eq!(session.auto_title.as_deref(), Some("Investigate"));
        assert_eq!(session.display_title(), "Investigate");
        assert_eq!(session.id, id);
        assert!(session.last_reply_at.is_some());
        assert_eq!(
            session.workspace,
            SessionWorkspace::Worktree {
                path: PathBuf::from("/tmp/worktrees/investigate"),
                name: "investigate".into(),
                branch: Some("waku/investigate".into()),
                base_branch: None,
            }
        );
        // ...and none of what it does not.
        assert!(!session.detail_loaded);
        assert!(session.messages.is_empty());
        assert!(session.turns.is_empty());
        // A skeleton still counts as started, since only started sessions
        // are stored at all.
        assert!(session.has_started());
        // Quarantine is detail, not a list column: ordinary sessions must
        // never list as quarantined (the Trust card keys off this).
        assert!(!session.quarantined);

        reopened.hydrate(&mut restored.sessions[0]).unwrap();
        let session = &restored.sessions[0];
        assert!(session.detail_loaded);
        assert_eq!(session.turns.len(), 1);
        assert_eq!(
            session.workspace,
            SessionWorkspace::Worktree {
                path: PathBuf::from("/tmp/worktrees/investigate"),
                name: "investigate".into(),
                branch: Some("waku/investigate".into()),
                base_branch: None,
            }
        );
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.content == "an answer")
        );
        assert!(!session.quarantined);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn hydrate_reads_on_its_own_connection_while_a_save_holds_the_storage_lock() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Ask");
        state.sessions[0].push_message(MessageRole::Assistant, "an answer");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        assert!(!restored.sessions[0].detail_loaded);

        // The shared connection stays mid-transaction with its mutex held —
        // exactly what a session click meets while a save is writing.
        let mut guard = reopened.storage.lock();
        let transaction = guard
            .as_mut()
            .unwrap()
            .connection
            .unchecked_transaction()
            .unwrap();
        transaction
            .execute("UPDATE sessions SET title = title", [])
            .unwrap();

        reopened.hydrate(&mut restored.sessions[0]).unwrap();
        assert!(restored.sessions[0].detail_loaded);
        assert_eq!(restored.sessions[0].turns.len(), 1);

        transaction.rollback().unwrap();
        drop(guard);
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_released_batch_keeps_marks_for_sessions_dirtied_during_the_write() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = state.sessions[0].id;
        state.sessions[0].begin_turn("Ask");
        state.sessions[0].push_message(MessageRole::Assistant, "an answer");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);

        let batch = store.save_batch(&mut state);
        state.unmark_sessions_dirty(&batch.dirty_ids);
        // The mark lands while the batch is owned by the in-flight write —
        // clearing it here would lose the follow-up save.
        state.mark_session_dirty(session_id);
        store.write_batch(&batch).unwrap();
        assert!(state.dirty_sessions.contains(&session_id));

        // The batch itself still wrote the session it captured.
        let mut restored = store.load().unwrap();
        store.hydrate(&mut restored.sessions[0]).unwrap();
        assert!(
            restored.sessions[0]
                .messages
                .iter()
                .any(|message| message.content == "an answer")
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_side_chat_round_trips_its_parent_link_in_the_list_projection() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let parent_id = state.sessions[0].id;
        state.sessions[0].begin_turn("Ask");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let mut side_chat = AgentSession::new(state.sessions[0].project_id, ProviderKind::Codex);
        side_chat.side_chat_of = Some(parent_id);
        side_chat.begin_turn("look at the parent");
        side_chat.finish_active_turn(crate::model::TurnStatus::Completed);
        let side_chat_id = side_chat.id;
        state.sessions.push(side_chat);
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let restored = reopened.load().unwrap();
        // The list projection carries the link without hydrating — the
        // daemon's cascade and the app's list filters both read it there.
        let side_chat = restored
            .sessions
            .iter()
            .find(|session| session.id == side_chat_id)
            .unwrap();
        assert_eq!(side_chat.side_chat_of, Some(parent_id));
        assert!(side_chat.is_side_chat());
        let parent = restored
            .sessions
            .iter()
            .find(|session| session.id == parent_id)
            .unwrap();
        assert_eq!(parent.side_chat_of, None);
        assert!(!parent.is_side_chat());

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn trim_idle_transcripts_releases_only_clean_unpinned_sessions_and_keeps_them_reloadable() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let project_id = state.sessions[0].project_id;
        let started = 1_700_000_000_u64;
        let ids = (0..5)
            .map(|index| {
                let mut session = AgentSession::new(project_id, ProviderKind::Codex);
                session.updated_at = started + index;
                session.begin_turn(format!("prompt {index}"));
                session.push_message(
                    crate::model::MessageRole::Assistant,
                    format!("answer {index}"),
                );
                session.finish_active_turn(crate::model::TurnStatus::Completed);
                let id = session.id;
                state.push_session(session);
                id
            })
            .collect::<Vec<_>>();
        let [
            dirty_id,
            pinned_id,
            oldest_releasable_id,
            middle_releasable_id,
            newest_id,
        ] = ids[..]
        else {
            panic!("expected five sessions");
        };
        store.save(&mut state).unwrap();

        // A pinned (live runtime) and a dirty (unsaved work) session are
        // never released; of the remaining three clean residents only the
        // newest survives a keep-one trim.
        let pinned: HashSet<Uuid> = HashSet::from([pinned_id]);
        state.mark_session_dirty(dirty_id);
        let released = state.trim_idle_transcripts(&pinned, 1);
        assert_eq!(released, 2);
        for session in state.sessions.iter().filter(|session| {
            session.id == oldest_releasable_id || session.id == middle_releasable_id
        }) {
            assert!(!session.detail_loaded);
            assert!(session.messages.is_empty());
            assert!(session.turns.is_empty());
            assert!(session.transcript_blocks.is_empty());
        }
        let newest = state
            .sessions
            .iter()
            .find(|session| session.id == newest_id)
            .unwrap();
        assert!(newest.detail_loaded);
        assert_eq!(newest.turns.len(), 1);

        // Released transcripts are still fully persisted: hydrating restores
        // one, and saving its skeleton later cannot erase the detail row.
        let releasable_index = state
            .sessions
            .iter()
            .position(|session| session.id == oldest_releasable_id)
            .unwrap();
        store
            .hydrate(&mut state.sessions[releasable_index])
            .unwrap();
        assert!(
            state.sessions[releasable_index]
                .messages
                .iter()
                .any(|message| message.content == "answer 2")
        );
        state.mark_session_dirty(oldest_releasable_id);
        store.save(&mut state).unwrap();
        let reopened = store_in(&directory);
        let mut reloaded = reopened.load().unwrap();
        let reloaded_index = reloaded
            .sessions
            .iter()
            .position(|session| session.id == oldest_releasable_id)
            .expect("released session still has a row");
        reopened
            .hydrate(&mut reloaded.sessions[reloaded_index])
            .unwrap();
        assert!(
            reloaded.sessions[reloaded_index]
                .messages
                .iter()
                .any(|message| message.content == "answer 2")
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn sent_attachment_presentation_round_trips_with_message_rows() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let attachment = MessageAttachment {
            path: PathBuf::from("/tmp/reference.png"),
            mention: "/tmp/reference.png".to_owned(),
            name: "reference.png".to_owned(),
            is_dir: false,
            is_image: true,
            blob_reference: Some("waku-blob:abcdef.png".to_owned()),
            pasted_text_preview: None,
            session_id: None,
        };
        state.sessions[0].begin_turn_with_presentation(
            "compare @/tmp/reference.png",
            Some("compare".to_owned()),
            vec![attachment.clone()],
            Vec::new(),
        );
        store.save(&mut state).unwrap();

        let restored = load_hydrated(&store);
        let message = &restored.sessions[0].messages[0];
        assert_eq!(message.content, "compare @/tmp/reference.png");
        assert_eq!(message.visible_content(), "compare");
        assert_eq!(message.attachments, vec![attachment]);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn atom_presentation_round_trips_with_message_rows() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = Uuid::new_v4();
        let display = format!(
            "fix {OPEN}{id}session:Big refactor{END} and {OPEN}Pasted text{END}",
            OPEN = waku_protocol::model::MESSAGE_ATOM_OPEN,
            END = waku_protocol::model::MESSAGE_ATOM_END,
            id = waku_protocol::model::encode_atom_session_id(session_id),
        );
        let atoms = vec![
            MessageAtom {
                label: "session:Big refactor".to_owned(),
                payload: format!("[session \"Big refactor\" (task_id: {session_id})]"),
                session_id: Some(session_id),
            },
            MessageAtom {
                label: "Pasted text".to_owned(),
                payload: "the pasted body".to_owned(),
                session_id: None,
            },
        ];
        state.sessions[0].begin_turn_with_presentation(
            "fix [session \"Big refactor\" (task_id: id)] and the pasted body",
            Some(display.clone()),
            Vec::new(),
            atoms.clone(),
        );
        store.save(&mut state).unwrap();

        let restored = load_hydrated(&store);
        let message = &restored.sessions[0].messages[0];
        assert_eq!(message.visible_content(), display);
        assert_eq!(message.atoms, atoms);

        fs::remove_dir_all(directory).ok();
    }

    /// Rows this store's connection has inserted, updated or deleted.
    fn rows_written(store: &StateStore) -> u64 {
        store
            .storage
            .lock()
            .as_ref()
            .expect("the store has saved at least once")
            .connection
            .total_changes()
    }

    /// A streaming turn saves once a second, and every one of those saves used
    /// to rewrite the entire transcript — measurably several frames of work at
    /// a couple of thousand messages. Counted in rows, because rows written is
    /// exactly what used to grow with history.
    #[test]
    fn a_save_only_writes_the_messages_that_changed() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let id = state.sessions[0].id;
        for turn in 0..20 {
            state.sessions[0].begin_turn(format!("prompt {turn}"));
            state.sessions[0].push_message(MessageRole::Assistant, format!("reply {turn}"));
            state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        }
        store.save(&mut state).unwrap();
        assert!(state.sessions[0].messages.len() >= 40);

        // A dirty session always rewrites its own two rows; that is the floor
        // the message count is measured against.
        let before = rows_written(&store);
        state.mark_session_dirty(id);
        store.save(&mut state).unwrap();
        let floor = rows_written(&store) - before;

        let before = rows_written(&store);
        state
            .session_mut(id)
            .unwrap()
            .push_message(MessageRole::Assistant, "one more");
        store.save(&mut state).unwrap();
        assert_eq!(
            rows_written(&store) - before,
            floor + 1,
            "a new message costs one row, not the whole transcript"
        );

        // Length is not enough to tell rows apart: a typo fix keeps it.
        let before = rows_written(&store);
        let session = state.session_mut(id).unwrap();
        let original = session.messages[3].content.clone();
        let edited = original.chars().rev().collect::<String>();
        assert_eq!(edited.len(), original.len());
        assert_ne!(edited, original);
        session.messages[3].content = edited.clone();
        store.save(&mut state).unwrap();
        assert_eq!(
            rows_written(&store) - before,
            floor + 1,
            "an edit that preserves length is still written"
        );

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        reopened.hydrate(&mut restored.sessions[0]).unwrap();
        let messages = &restored.sessions[0].messages;
        assert_eq!(messages[3].content, edited, "the edit reached disk");
        assert_eq!(
            messages.last().unwrap().content,
            "one more",
            "and so did the append"
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_skeleton_is_never_written_back_over_stored_history() {
        // The failure this guards against is silent and total: saving a session
        // whose transcript was never fetched would replace it with nothing.
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Ask");
        state.sessions[0].push_message(MessageRole::User, "keep me");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        assert!(!restored.sessions[0].detail_loaded);
        // Mark it dirty anyway, the worst case.
        let id = restored.sessions[0].id;
        restored.mark_session_dirty(id);
        reopened.save(&mut restored).unwrap();

        let checked = load_hydrated(&store_in(&directory));
        assert_eq!(checked.sessions[0].turns.len(), 1, "turns survived");
        assert!(
            checked.sessions[0]
                .messages
                .iter()
                .any(|message| message.content == "keep me"),
            "messages survived"
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn blob_sweep_keeps_images_of_sessions_that_are_not_loaded() {
        // Sweeping from memory would treat an unhydrated session as owning no
        // images and delete screenshots that are still referenced.
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let payload = vec![4u8; 32 * 1024];
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&payload)
        );
        let id = state.sessions[0].id;
        state.sessions[0].begin_turn("Screenshot");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state
            .session_mut(id)
            .unwrap()
            .transcript_blocks
            .push(TranscriptBlock {
                after_message: 0,
                turn_id: None,
                activities: vec![
                    ActivityItem::new(None, ActivityKind::Tool, "Screenshot", None, true)
                        .with_image_urls(vec![data_url]),
                ],
            });
        store.save(&mut state).unwrap();

        // Reopen without hydrating anything, then sweep.
        let reopened = store_in(&directory);
        let restored = reopened.load().unwrap();
        assert!(!restored.sessions[0].detail_loaded);
        reopened.blob_sweep()();

        let checked = load_hydrated(&store_in(&directory));
        let activities = &checked.sessions[0].transcript_blocks[0].activities;
        let path = store
            .blobs()
            .path_for(&activities[0].image_urls[0])
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), payload, "the image survived");

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn blob_sweep_keeps_clipboard_attachments_in_composer_drafts() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        // Open the database so the sweep has a migrated source to scan.
        store.load().unwrap();
        let payload = vec![9u8; 1024];
        let reference = store
            .blobs()
            .store_image_bytes("image/png", &payload)
            .unwrap();
        let path = store.blobs().path_for(&reference).unwrap();

        let project_id = Uuid::new_v4();
        let mut drafts = ComposerDrafts::default();
        drafts.set(
            ComposerDraftKey::NewSession(project_id),
            ComposerDraft {
                text: String::new(),
                attachments: vec![ComposerDraftAttachment {
                    path: path.clone(),
                    mention: path.display().to_string(),
                    name: "image.png".to_owned(),
                    is_dir: false,
                    is_image: true,
                    blob_reference: Some(reference),
                    pasted_text_preview: None,
                    session_id: None,
                }],
                annotations: Vec::new(),
                inline_atoms: Vec::new(),
            },
        );
        ComposerDraftStore::for_state_path(&directory.join("app.db"))
            .save(drafts, 1)
            .unwrap();

        store.blob_sweep()();

        assert_eq!(fs::read(path).unwrap(), payload);
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn default_path_is_build_specific() {
        let path = StateStore::default_path();
        assert_eq!(path.file_name(), Some(std::ffi::OsStr::new("app.db")));
        let directory = path.parent().and_then(Path::file_name);
        let store = StateStore::new(path.clone());
        assert_eq!(store.app_state_path, path.with_file_name("state.json"));

        // Debug files stay inside the checkout so development cannot read or
        // write the installed app's settings.
        #[cfg(debug_assertions)]
        {
            assert_eq!(directory, Some(std::ffi::OsStr::new("temp")));
            let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .unwrap();
            assert!(path.starts_with(checkout));
            assert_eq!(store.app_settings_path, path.with_file_name("app.json"));
            assert_eq!(
                store.legacy_settings_paths,
                [path.with_file_name("settings.json")]
            );
        }
        #[cfg(not(debug_assertions))]
        {
            assert_eq!(directory, Some(std::ffi::OsStr::new("Goddard")));
            let configuration_directory = dirs::home_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join(crate::identity::HOME_DIRECTORY_NAME);
            assert_eq!(
                store.app_settings_path,
                configuration_directory.join("app.json")
            );
            assert_eq!(
                store.legacy_settings_paths,
                [configuration_directory.join("settings.json")]
            );
        }
    }

    #[test]
    fn state_round_trips() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].model = Some("gpt-5.6-luna".into());
        state.last_model = Some("gpt-5.6-luna".into());
        state.sessions[0].reasoning_effort = Some("xhigh".into());
        state.last_reasoning_effort = Some("xhigh".into());
        state.sessions[0].service_tier = Some("fast".into());
        state.last_service_tier = Some("fast".into());
        state.sessions[0].context_window = Some("1m".into());
        state.last_context_window = Some("1m".into());
        state.remember_model_traits(
            ProviderKind::Codex,
            "gpt-5.6-luna",
            Some("xhigh".into()),
            Some("fast".into()),
            Some("1m".into()),
        );
        state.sessions[0].runtime_mode = crate::model::RuntimeMode::Auto;
        state.favorite_models.push(FavoriteModel {
            provider: ProviderKind::Codex,
            model: "gpt-5.6-luna".into(),
            effort: None,
            fast: false,
        });
        state.theme = ThemeSettings {
            mode: crate::theme::ThemeMode::Light,
            ..ThemeSettings::default()
        };
        state.language = AppLanguage::SimplifiedChinese;
        state.sidebar_visible = false;
        state.right_panel_visible = false;
        state.sidebar_width = 318.0;
        state.right_panel_width = 612.0;
        state.computer_use_enabled = false;
        state.computer_use_allowed_apps.push(ComputerAppGrant {
            bundle_id: "com.apple.Safari".into(),
            app_name: "Safari".into(),
        });
        state.sessions[0].begin_turn("Persist this session");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].transcript_blocks.push(TranscriptBlock {
            after_message: 1,
            turn_id: None,
            activities: vec![
                ActivityItem::from_reasoning(
                    ReasoningBlock {
                        content: "Checking the source".into(),
                        started_at_ms: 1_000,
                        finished_at_ms: 2_500,
                    },
                    true,
                ),
                ActivityItem::new(
                    Some("tool-1".into()),
                    ActivityKind::Search,
                    "Read src/main.rs",
                    Some("{\"path\":\"src/main.rs\"}".into()),
                    true,
                ),
            ],
        });
        let daemon_settings = state.daemon_settings();
        store.save(&mut state).unwrap();

        let mut restored = load_hydrated(&store_in(&directory));
        restored.apply_daemon_settings(daemon_settings);
        assert_eq!(restored.projects[0].name, "project");
        assert_eq!(restored.sessions.len(), 1);
        assert_eq!(restored.sessions[0].model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(restored.last_model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(restored.last_reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(restored.last_service_tier.as_deref(), Some("fast"));
        assert_eq!(
            restored.sessions[0].reasoning_effort.as_deref(),
            Some("xhigh")
        );
        assert_eq!(restored.sessions[0].service_tier.as_deref(), Some("fast"));
        assert_eq!(restored.last_context_window.as_deref(), Some("1m"));
        assert_eq!(restored.sessions[0].context_window.as_deref(), Some("1m"));
        assert_eq!(
            restored.model_traits_for(ProviderKind::Codex, "gpt-5.6-luna"),
            (Some("xhigh".into()), Some("fast".into()), Some("1m".into()))
        );
        assert_eq!(
            restored.sessions[0].runtime_mode,
            crate::model::RuntimeMode::Auto
        );
        assert_eq!(restored.favorite_models, state.favorite_models);
        assert_eq!(restored.theme, state.theme);
        assert_eq!(restored.language, AppLanguage::SimplifiedChinese);
        assert!(!restored.sidebar_visible);
        assert!(!restored.right_panel_visible);
        assert_eq!(restored.sidebar_width, 318.0);
        assert_eq!(restored.right_panel_width, 612.0);
        assert!(!restored.computer_use_enabled);
        assert_eq!(
            restored.computer_use_allowed_apps,
            state.computer_use_allowed_apps
        );
        assert_eq!(restored.sessions[0].transcript_blocks.len(), 1);
        assert_eq!(
            restored.sessions[0].transcript_blocks[0].activities.len(),
            2
        );
        assert_eq!(
            restored.sessions[0].transcript_blocks[0].activities[0]
                .reasoning
                .as_ref()
                .map(|reasoning| reasoning.content.as_str()),
            Some("Checking the source")
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn unchanged_sessions_are_not_rewritten() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("First");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let quiet = {
            let mut session = state.new_session(state.projects[0].id, ProviderKind::Codex);
            session.begin_turn("Quiet");
            session.finish_active_turn(crate::model::TurnStatus::Completed);
            session
        };
        let quiet_id = quiet.id;
        state.sessions.push(quiet);
        store.save(&mut state).unwrap();

        // Stamp the quiet session's row so any write would overwrite the mark.
        let connection = Connection::open(directory.join("app.db")).unwrap();
        connection
            .execute(
                "UPDATE sessions SET title = 'untouched' WHERE id = ?1",
                params![quiet_id.to_string()],
            )
            .unwrap();

        // Change the other session, through the accessor that marks it dirty.
        let active_id = state.sessions[0].id;
        let session = state.session_mut(active_id).unwrap();
        session.begin_turn("Second");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let quiet_title: String = connection
            .query_row(
                "SELECT title FROM sessions WHERE id = ?1",
                params![quiet_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            quiet_title, "untouched",
            "a session nobody touched was not rewritten"
        );
        drop(connection);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_session_changed_without_the_accessor_is_not_written() {
        // The dirty set is the contract: bypassing `session_mut` means the
        // change does not reach disk. This pins that so the invariant is
        // visible rather than discovered later as data loss.
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let id = state.sessions[0].id;
        state.sessions[0].begin_turn("First");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        state.sessions[0].title = "bypassed".into();
        store.save(&mut state).unwrap();
        assert_eq!(
            store_in(&directory).load().unwrap().sessions[0].title,
            "New task",
            "an unmarked change stays in memory"
        );

        // Going through the accessor persists it.
        state.session_mut(id).unwrap().title = "marked".into();
        store.save(&mut state).unwrap();
        assert_eq!(
            store_in(&directory).load().unwrap().sessions[0].title,
            "marked"
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn renaming_a_skeleton_updates_metadata_without_erasing_its_transcript() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let id = state.sessions[0].id;
        state.sessions[0].begin_turn("Keep this transcript");
        state.sessions[0].push_message(MessageRole::Assistant, "still here");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        assert!(!restored.sessions[0].detail_loaded);
        assert!(restored.session_mut(id).unwrap().set_title("Renamed task"));
        reopened.save(&mut restored).unwrap();

        let checked = load_hydrated(&store_in(&directory));
        assert_eq!(checked.sessions[0].title, "Renamed task");
        assert!(
            checked.sessions[0]
                .messages
                .iter()
                .any(|message| message.content == "still here")
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_new_session_is_written_even_without_being_marked() {
        // Safety net: a session with no row yet is always written, so a missed
        // mark can never lose a whole session.
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Unmarked");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.dirty_sessions.clear();

        store.save(&mut state).unwrap();

        assert_eq!(store_in(&directory).load().unwrap().sessions.len(), 1);
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn app_settings_and_app_managed_state_live_in_separate_json_files() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.theme = ThemeSettings {
            mode: crate::theme::ThemeMode::Light,
            ..ThemeSettings::default()
        };
        state.language = AppLanguage::SimplifiedChinese;
        state.sidebar_width = 301.0;
        store.save(&mut state).unwrap();

        let settings = directory.join("app.json");
        let text = fs::read_to_string(&settings).unwrap();
        assert!(
            text.contains('\n'),
            "settings are pretty-printed for editing"
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["theme"],
            serde_json::json!({"mode": "light", "light": "light", "dark": "dark"})
        );
        assert_eq!(value["language"], "simplified-chinese");
        for daemon_key in [
            "computer_use_enabled",
            "computer_use_experiment_enabled",
            "computer_use_allowed_apps",
            "disabled_providers",
            "provider_binary_overrides",
        ] {
            assert!(
                value.get(daemon_key).is_none(),
                "{daemon_key} leaked into app.json"
            );
        }
        for app_managed_key in [
            "version",
            "app_state_version",
            "analytics_id",
            "selected_project",
            "selected_session",
            "last_provider",
            "last_model",
            "last_reasoning_effort",
            "last_service_tier",
            "last_context_window",
            "remembered_model_traits",
            "sidebar_visible",
            "right_panel_visible",
            "sidebar_width",
            "right_panel_width",
        ] {
            assert!(
                value.get(app_managed_key).is_none(),
                "{app_managed_key} leaked into app.json"
            );
        }

        let app_state: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("state.json")).unwrap()).unwrap();
        assert_eq!(app_state["sidebar_width"], 301.0);
        assert_eq!(app_state["app_state_version"], APP_STATE_VERSION);
        for setting_key in [
            "analytics_enabled",
            "favorite_models",
            "theme",
            "language",
            "computer_use_enabled",
            "computer_use_experiment_enabled",
            "computer_use_allowed_apps",
            "disabled_providers",
            "provider_binary_overrides",
        ] {
            assert!(
                app_state.get(setting_key).is_none(),
                "{setting_key} leaked into state.json"
            );
        }

        // A hand edit is picked up on the next load.
        let edited = text.replace("simplified-chinese", "english");
        fs::write(&settings, edited).unwrap();
        let restored = store_in(&directory).load().unwrap();
        assert_eq!(restored.language, AppLanguage::English);
        assert_eq!(restored.sidebar_width, 301.0);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn app_state_changes_do_not_rewrite_app_settings() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        store.save(&mut state).unwrap();

        let settings_path = directory.join("app.json");
        let mut user_document: serde_json::Value =
            serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
        user_document["future_setting"] = serde_json::Value::Bool(true);
        let user_document = serde_json::to_vec(&user_document).unwrap();
        fs::write(&settings_path, &user_document).unwrap();

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        restored.sidebar_width = 333.0;
        reopened.save(&mut restored).unwrap();

        assert_eq!(fs::read(&settings_path).unwrap(), user_document);
        let app_state: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("state.json")).unwrap()).unwrap();
        assert_eq!(app_state["sidebar_width"], 333.0);
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn migrations_run_once_and_are_recorded() {
        let connection = Connection::open_in_memory().unwrap();

        assert_eq!(
            apply_migrations(&connection).unwrap(),
            MIGRATIONS.len(),
            "all run on a fresh database"
        );

        let recorded: Vec<String> = connection
            .prepare("SELECT tag FROM migrations ORDER BY tag")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            recorded,
            MIGRATIONS
                .iter()
                .map(|(tag, _)| tag.to_string())
                .collect::<Vec<_>>()
        );

        // Re-running is a no-op; a second CREATE TABLE would otherwise error.
        assert_eq!(apply_migrations(&connection).unwrap(), 0);
    }

    #[test]
    fn recorded_migrations_are_skipped() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATIONS_TABLE).unwrap();
        // Claim every migration already ran. The tables do not exist, so
        // anything that did run would fail loudly.
        for (tag, _) in MIGRATIONS {
            connection
                .execute(
                    "INSERT INTO migrations(tag, applied_at) VALUES(?1, 0)",
                    params![tag],
                )
                .unwrap();
        }

        assert_eq!(apply_migrations(&connection).unwrap(), 0);
        assert!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
                .is_err(),
            "nothing ran, so the schema was never created"
        );
    }

    #[test]
    fn a_half_applied_run_resumes_from_where_it_stopped() {
        let connection = Connection::open_in_memory().unwrap();
        apply_migrations(&connection).unwrap();

        // Drop the record of the last migration without dropping its tables,
        // as an interrupted run would leave things. The last migration with
        // real statements — trailing no-ops replay cleanly and prove nothing.
        let (last, _) = MIGRATIONS
            .iter()
            .rev()
            .find(|(_, sql)| {
                sql.lines().any(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with("--")
                })
            })
            .expect("at least one migration with statements");
        connection
            .execute("DELETE FROM migrations WHERE tag = ?1", params![last])
            .unwrap();

        // It re-runs and fails loudly rather than silently skipping, because
        // the tables it creates already exist.
        let error = apply_migrations(&connection).unwrap_err();
        assert!(
            error.to_string().contains(last),
            "the failure names the migration: {error}"
        );
    }

    #[test]
    fn auto_title_migration_preserves_existing_generated_titles_as_fallbacks() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATIONS_TABLE).unwrap();
        connection.execute_batch(MIGRATIONS[0].1).unwrap();
        connection
            .execute(
                "INSERT INTO migrations(tag, applied_at) VALUES(?1, 0)",
                params![MIGRATIONS[0].0],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sessions(
                    id, project_id, title, provider, model, status,
                    created_at, updated_at, last_reply_at
                 ) VALUES('session-1', 'project-1', 'Investigate the parser',
                          'codex', NULL, 'idle', 1, 1, NULL)",
                [],
            )
            .unwrap();

        assert_eq!(apply_migrations(&connection).unwrap(), MIGRATIONS.len() - 1);
        let (title, auto_title): (String, Option<String>) = connection
            .query_row(
                "SELECT title, auto_title FROM sessions WHERE id = 'session-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, AgentSession::DEFAULT_TITLE);
        assert_eq!(auto_title.as_deref(), Some("Investigate the parser"));
    }

    #[test]
    fn workspace_migration_backfills_the_column_from_session_details() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATIONS_TABLE).unwrap();
        // Every migration except the one that adds the workspace column,
        // recorded as applied. Later migrations must keep running — the
        // workspace backfill is no longer the last entry.
        let workspace_migration = MIGRATIONS
            .iter()
            .position(|(_, sql)| sql.contains("ADD `workspace`"))
            .expect("a migration adds the workspace column");
        for (tag, sql) in MIGRATIONS
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != workspace_migration)
            .map(|(_, migration)| migration)
        {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO migrations(tag, applied_at) VALUES(?1, 0)",
                    params![tag],
                )
                .unwrap();
        }
        for id in ["session-1", "session-2"] {
            connection
                .execute(
                    "INSERT INTO sessions(
                        id, project_id, title, provider, status,
                        created_at, updated_at
                     ) VALUES(?1, 'project-1', 'Task', 'codex', 'idle', 1, 1)",
                    params![id],
                )
                .unwrap();
        }
        // A stored worktree session carries the workspace inside the detail
        // blob; a local session omits the field entirely.
        connection
            .execute(
                "INSERT INTO session_details(session_id, data) VALUES(
                    'session-1',
                    '{\"workspace\":{\"kind\":\"worktree\",\"path\":\"/tmp/wt/task\",\"name\":\"task\",\"branch\":\"wt/task\"}}'
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session_details(session_id, data) VALUES(
                    'session-2', '{}'
                 )",
                [],
            )
            .unwrap();

        assert_eq!(apply_migrations(&connection).unwrap(), 1);
        let workspace: Option<String> = connection
            .query_row(
                "SELECT workspace FROM sessions WHERE id = 'session-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let parsed: SessionWorkspace =
            serde_json::from_str(&workspace.expect("workspace backfilled")).unwrap();
        assert_eq!(
            parsed,
            SessionWorkspace::Worktree {
                path: PathBuf::from("/tmp/wt/task"),
                name: "task".into(),
                branch: Some("wt/task".into()),
                base_branch: None,
            }
        );
        let local: Option<String> = connection
            .query_row(
                "SELECT workspace FROM sessions WHERE id = 'session-2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(local, None);
    }

    #[test]
    fn messages_round_trip_through_their_own_table() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Ask");
        state.sessions[0].push_message(MessageRole::User, "how do I center a div");
        state.sessions[0].push_message(MessageRole::Assistant, "flexbox");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].begin_hidden_turn("Continue the current task if able.");
        state.sessions[0].push_message(MessageRole::Assistant, "kept going");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let expected = state.sessions[0].messages.clone();
        store.save(&mut state).unwrap();

        // The JSON column must not carry a second copy that could drift.
        let connection = Connection::open(directory.join("app.db")).unwrap();
        let data: String = connection
            .query_row("SELECT data FROM session_details LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(
            !serde_json::from_str::<serde_json::Value>(&data)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("messages"),
            "messages live only in their own table"
        );
        drop(connection);

        let restored = load_hydrated(&store_in(&directory));
        let messages = &restored.sessions[0].messages;
        assert_eq!(messages.len(), expected.len());
        assert!(expected.len() >= 2, "the turn and both replies are present");
        for (restored, expected) in messages.iter().zip(&expected) {
            assert_eq!(restored.id, expected.id);
            assert_eq!(restored.role, expected.role);
            assert_eq!(restored.content, expected.content);
            assert_eq!(restored.turn_id, expected.turn_id);
            assert_eq!(restored.created_at, expected.created_at);
            assert_eq!(restored.streaming, expected.streaming);
            assert_eq!(restored.hidden, expected.hidden);
        }
        assert!(messages[3].hidden, "the continue nudge survives the save");

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn message_search_reads_skeleton_history_and_prefers_user_matches() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let project_id = state.projects[0].id;
        let user_match_id = state.sessions[0].id;
        state.sessions[0].begin_turn("Older user needle at 100%_literal");
        state.sessions[0].push_message(MessageRole::Assistant, "Newer assistant needle");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);

        let mut assistant_match = AgentSession::new(project_id, ProviderKind::Codex);
        let assistant_match_id = assistant_match.id;
        assistant_match.begin_turn("Ordinary prompt");
        assistant_match.push_message(MessageRole::System, "System needle is private");
        assistant_match.push_message(MessageRole::Assistant, "Streaming needle");
        assistant_match.messages.last_mut().unwrap().streaming = true;
        assistant_match.push_message(MessageRole::Assistant, "Final assistant needle");
        assistant_match.finish_active_turn(crate::model::TurnStatus::Completed);
        assistant_match.push_notice_message(
            MessageRole::Assistant,
            "the Goddard daemon restarted and this turn could not be reattached",
            TranscriptNotice::Status {
                kind: TranscriptNoticeStatus::Error,
            },
        );
        assistant_match.begin_hidden_turn("Hidden continue needle");
        assistant_match.finish_active_turn(crate::model::TurnStatus::Interrupted);
        state.sessions.push(assistant_match);
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let skeletons = reopened.load().unwrap();
        assert!(
            skeletons
                .sessions
                .iter()
                .all(|session| !session.detail_loaded)
        );
        assert!(
            skeletons
                .sessions
                .iter()
                .all(|session| session.messages.is_empty())
        );

        let matches = reopened.session_message_search(
            "needle".into(),
            50,
            SessionMessageSearchScope::Active,
            None,
            None,
        )()
        .unwrap();
        assert_eq!(
            matches
                .iter()
                .map(|matched| (matched.session_id, matched.source))
                .collect::<Vec<_>>(),
            vec![
                (user_match_id, MessageRole::User),
                (assistant_match_id, MessageRole::Assistant),
            ]
        );
        assert!(matches[0].snippet.contains("user needle"));
        assert!(matches[1].snippet.contains("Final assistant needle"));
        assert!(!matches[1].snippet.contains("Streaming"));
        assert!(!matches[1].snippet.contains("System"));
        assert_eq!(
            reopened.session_message_search(
                "100%_literal".into(),
                50,
                SessionMessageSearchScope::Active,
                None,
                None,
            )()
            .unwrap()
            .iter()
            .map(|matched| matched.session_id)
            .collect::<Vec<_>>(),
            vec![user_match_id],
            "SQL wildcard characters are searched literally"
        );
        assert!(
            reopened.session_message_search(
                "Hidden continue".into(),
                50,
                SessionMessageSearchScope::Active,
                None,
                None,
            )()
            .unwrap()
            .is_empty(),
            "a hidden prompt never surfaces in search"
        );
        assert!(
            reopened.session_message_search(
                "reattached".into(),
                50,
                SessionMessageSearchScope::Active,
                None,
                None,
            )()
            .unwrap()
            .is_empty(),
            "a synthesized notice never surfaces in search"
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn message_search_can_be_confined_to_the_last_turns() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = state.sessions[0].id;
        state.sessions[0].begin_turn("the needle opens the first turn");
        state.sessions[0].push_message(MessageRole::Assistant, "early answer");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].begin_turn("a quiet middle turn");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].begin_turn("the needle returns in the last turn");
        state.sessions[0].push_message(MessageRole::Assistant, "latest needle answer");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        // Pushed without a running turn, this row belongs to no turn at all.
        state.sessions[0].push_message(MessageRole::Assistant, "unturned needle");
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let hits = |needle: &str, last_turns: Option<usize>| {
            reopened.session_message_search(
                needle.into(),
                50,
                SessionMessageSearchScope::Active,
                None,
                last_turns,
            )()
            .unwrap()
        };

        // The whole transcript matches either turn; the ranked excerpt is
        // the newest user hit — turn three's prompt.
        let matched = hits("needle", None);
        assert_eq!(matched.len(), 1);
        assert!(matched[0].snippet.contains("last turn"));
        // Only the last turn is scanned, and it still carries a needle.
        let matched = hits("needle", Some(1));
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].session_id, session_id);
        assert!(matched[0].snippet.contains("last turn"));

        // A phrase unique to the first turn falls out of the last-one and
        // last-two windows and comes back once every turn is in scope.
        assert_eq!(hits("opens the first turn", None).len(), 1);
        assert!(hits("opens the first turn", Some(1)).is_empty());
        assert!(hits("opens the first turn", Some(2)).is_empty());
        assert_eq!(hits("opens the first turn", Some(3)).len(), 1);

        // An unturned message is never inside a last-N window, and a
        // zero-turn window matches nothing.
        assert_eq!(hits("unturned needle", None).len(), 1);
        assert!(hits("unturned needle", Some(3)).is_empty());
        assert!(hits("needle", Some(0)).is_empty());

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn message_search_snippets_center_the_literal_query_and_stay_bounded() {
        let text = format!("{}100%_needle{}", "before ".repeat(80), " after".repeat(80));
        let snippet = build_session_search_snippet(&text, "100%_needle");
        assert!(snippet.starts_with('…'));
        assert!(snippet.ends_with('…'));
        assert!(snippet.contains("100%_needle"));
        assert!(snippet.chars().count() <= SESSION_SEARCH_SNIPPET_CHARS);
    }

    #[test]
    fn truncating_a_conversation_drops_the_orphaned_message_rows() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("First");
        state.sessions[0].push_message(MessageRole::User, "one");
        state.sessions[0].push_message(MessageRole::Assistant, "two");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let id = state.sessions[0].id;
        state.session_mut(id).unwrap().messages.truncate(1);
        store.save(&mut state).unwrap();

        let connection = Connection::open(directory.join("app.db")).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "the tail row was deleted, not left behind");
        drop(connection);

        assert_eq!(
            load_hydrated(&store_in(&directory)).sessions[0]
                .messages
                .len(),
            1
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn deleting_a_session_removes_its_messages() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Keep");
        state.sessions[0].push_message(MessageRole::User, "keep me");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let mut extra = state.new_session(state.projects[0].id, ProviderKind::Codex);
        extra.begin_turn("Remove");
        extra.push_message(MessageRole::User, "delete me");
        extra.finish_active_turn(crate::model::TurnStatus::Completed);
        let removed_id = extra.id;
        state.sessions.push(extra);
        store.save(&mut state).unwrap();

        state.sessions.retain(|session| session.id != removed_id);
        store.save(&mut state).unwrap();

        let connection = Connection::open(directory.join("app.db")).unwrap();
        let orphans: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
                params![removed_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_message_edit_alone_marks_the_session_dirty() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Ask");
        state.sessions[0].push_message(MessageRole::User, "before");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();

        // Nothing outside the message list changes, so the session JSON is
        // identical; only the message row differs.
        let id = state.sessions[0].id;
        state.session_mut(id).unwrap().messages[0].content = "after".into();
        store.save(&mut state).unwrap();

        assert_eq!(
            load_hydrated(&store_in(&directory)).sessions[0].messages[0].content,
            "after"
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn promoted_columns_match_the_json_payload() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].title = "Investigate the parser".into();
        state.sessions[0].auto_title = Some("Provider fallback".into());
        state.sessions[0].model = Some("gpt-5.6-luna".into());
        state.sessions[0].begin_turn("Go");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let session = state.sessions[0].clone();
        store.save(&mut state).unwrap();

        let connection = Connection::open(directory.join("app.db")).unwrap();
        let columns = connection
            .query_row(
                "SELECT title, auto_title, provider, model, status,
                        created_at, updated_at, last_reply_at
                 FROM sessions WHERE id = ?1",
                params![session.id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .unwrap();
        let (title, auto_title, provider, model, status, created, updated, last_reply) = columns;

        assert_eq!(title, "Investigate the parser");
        assert_eq!(auto_title.as_deref(), Some("Provider fallback"));
        assert_eq!(provider, tag_of(session.provider));
        assert_eq!(model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(status, tag_of(session.status));
        assert_eq!(created as u64, session.created_at);
        assert_eq!(updated as u64, session.updated_at);
        assert_eq!(last_reply.map(|at| at as u64), session.last_reply_at);
        assert!(last_reply.is_some(), "a submitted turn sets last_reply_at");

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn last_reply_at_tracks_turn_activity_not_every_edit() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        assert!(session.last_reply_at.is_none(), "no turn yet");

        session.begin_turn("Ask");
        let submitted_at = session.last_reply_at.expect("submission recorded");
        assert_eq!(submitted_at, session.turns.last().unwrap().started_at);
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        let replied_at = session.last_reply_at.expect("reply recorded");
        assert!(replied_at >= submitted_at);

        // A later edit moves updated_at but must not look like a new reply.
        session.title = "Renamed".into();
        session.updated_at = replied_at + 500;
        assert_eq!(session.last_reply_at, Some(replied_at));

        // A second turn moves it immediately, before that turn finishes.
        session.begin_turn("Again");
        assert!(session.last_reply_at >= Some(replied_at));
        session.finish_active_turn(crate::model::TurnStatus::Failed);
        assert!(session.last_reply_at >= Some(replied_at));
    }

    #[test]
    fn last_reply_at_is_derived_for_sessions_stored_without_it() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Ask");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        let completed_at = session.turns.last().unwrap().completed_at.unwrap();

        // Drop the field, as a session written before it existed would be.
        session.last_reply_at = None;
        session.backfill_last_reply_at();
        assert_eq!(session.last_reply_at, Some(completed_at));

        // A session that never ran has nothing to derive.
        let mut fresh = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        fresh.backfill_last_reply_at();
        assert!(fresh.last_reply_at.is_none());

        // A running legacy turn still has submission activity to recover.
        fresh.begin_turn("Ask");
        let started_at = fresh.turns.last().unwrap().started_at;
        fresh.last_reply_at = None;
        fresh.backfill_last_reply_at();
        assert_eq!(fresh.last_reply_at, Some(started_at));
    }

    #[test]
    fn sessions_can_be_listed_without_deserializing_transcripts() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("First");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let mut second = state.new_session(state.projects[0].id, ProviderKind::Codex);
        second.title = "Newer".into();
        second.begin_turn("Second");
        second.finish_active_turn(crate::model::TurnStatus::Completed);
        second.updated_at = state.sessions[0].updated_at + 100;
        state.sessions.push(second);
        store.save(&mut state).unwrap();

        let connection = Connection::open(directory.join("app.db")).unwrap();
        let mut statement = connection
            .prepare("SELECT title FROM sessions ORDER BY updated_at DESC")
            .unwrap();
        let titles: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();

        assert_eq!(titles.first().map(String::as_str), Some("Newer"));
        assert_eq!(titles.len(), 2);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn archived_sessions_round_trip_and_leave_message_search() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = state.sessions[0].id;
        state.sessions[0].begin_turn("a needle prompt");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();
        assert_eq!(
            store.session_message_search(
                "needle".into(),
                50,
                SessionMessageSearchScope::Active,
                None,
                None
            )()
            .unwrap()
            .len(),
            1
        );

        let archived_at = 1_700_000_000;
        state.session_mut(session_id).unwrap().archived_at = Some(archived_at);
        store.save(&mut state).unwrap();

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        assert_eq!(restored.sessions[0].archived_at, Some(archived_at));
        assert!(
            reopened.session_message_search(
                "needle".into(),
                50,
                SessionMessageSearchScope::Active,
                None,
                None
            )()
            .unwrap()
            .is_empty(),
            "archived sessions are hidden from the active transcript search"
        );
        assert_eq!(
            reopened.session_message_search(
                "needle".into(),
                50,
                SessionMessageSearchScope::Archived,
                None,
                None,
            )()
            .unwrap()
            .len(),
            1,
            "the archived scope finds the same transcript"
        );

        restored.session_mut(session_id).unwrap().archived_at = None;
        reopened.save(&mut restored).unwrap();

        let reopened = store_in(&directory);
        assert_eq!(reopened.load().unwrap().sessions[0].archived_at, None);
        assert_eq!(
            reopened.session_message_search(
                "needle".into(),
                50,
                SessionMessageSearchScope::Active,
                None,
                None
            )()
            .unwrap()
            .len(),
            1
        );
        assert!(
            reopened.session_message_search(
                "needle".into(),
                50,
                SessionMessageSearchScope::Archived,
                None,
                None,
            )()
            .unwrap()
            .is_empty(),
            "an unarchived session leaves the archived scope"
        );

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn reopening_does_not_rewrite_untouched_sessions() {
        let directory = temporary_directory();
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Stored");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store_in(&directory).save(&mut state).unwrap();

        // A fresh store reloads and then saves without any edits in between.
        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        assert!(
            restored.dirty_sessions.is_empty(),
            "loading marks nothing dirty"
        );

        let connection = Connection::open(directory.join("app.db")).unwrap();
        connection
            .execute_batch("UPDATE sessions SET title = 'untouched'")
            .unwrap();
        reopened.save(&mut restored).unwrap();

        let title: String = connection
            .query_row("SELECT title FROM sessions LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            title, "untouched",
            "no row was rewritten after a plain load"
        );
        drop(connection);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn large_images_are_externalized_and_referenced() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let payload = vec![9u8; 64 * 1024];
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&payload)
        );
        state.sessions[0].begin_turn("Screenshot");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let id = state.sessions[0].id;
        state
            .session_mut(id)
            .unwrap()
            .transcript_blocks
            .push(TranscriptBlock {
                after_message: 0,
                turn_id: None,
                activities: vec![
                    ActivityItem::new(None, ActivityKind::Tool, "Screenshot", None, true)
                        .with_image_urls(vec![data_url]),
                ],
            });

        store.save(&mut state).unwrap();

        let restored = load_hydrated(&store_in(&directory));
        let activities = &restored.sessions[0].transcript_blocks[0].activities;
        let reference = &activities[0].image_urls[0];
        assert!(crate::blob_store::is_blob_reference(reference));
        let path = store.blobs().path_for(reference).unwrap();
        assert_eq!(fs::read(path).unwrap(), payload);

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn legacy_signed_computer_grants_migrate_to_bundle_ids() {
        let legacy = serde_json::json!({
            "bundleId": "net.imput.helium",
            "teamId": "S4Q33XPHB4",
            "appName": "Helium"
        });
        let grant: ComputerAppGrant = serde_json::from_value(legacy).unwrap();
        assert_eq!(grant.key(), "net.imput.helium");

        let mut grants = vec![
            grant,
            ComputerAppGrant {
                bundle_id: "net.imput.helium".into(),
                app_name: "Helium Preview".into(),
            },
            ComputerAppGrant {
                bundle_id: String::new(),
                app_name: "Missing identity".into(),
            },
        ];
        normalize_computer_app_grants(&mut grants);
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].app_name, "Helium");

        let saved = serde_json::to_value(&grants[0]).unwrap();
        assert_eq!(
            saved.get("bundleId").and_then(|value| value.as_str()),
            Some("net.imput.helium")
        );
        assert!(saved.get("teamId").is_none());
    }

    #[test]
    fn blank_sessions_stay_runtime_only() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));

        store.save(&mut state).unwrap();
        let restored = store_in(&directory).load().unwrap();

        assert!(restored.sessions.is_empty());
        assert!(restored.selected_session.is_none());
        assert_eq!(restored.selected_project, state.selected_project);
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn quitting_on_a_draft_relaunches_to_the_new_session_page() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let started_id = state.sessions[0].id;
        state.sessions[0].begin_turn("Persist this session");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let draft = state.new_session(state.projects[0].id, ProviderKind::Codex);
        state.selected_session = Some(draft.id);
        state.sessions.push(draft);

        store.save(&mut state).unwrap();
        let restored = store_in(&directory).load().unwrap();

        // Only the started session earned a row, and the draft selection was
        // stored as no selection so launch recreates the new-session page.
        assert_eq!(restored.sessions.len(), 1);
        assert_eq!(restored.sessions[0].id, started_id);
        assert_eq!(restored.selected_session, None);

        let relaunched = store_in(&directory).load_or_fresh(PathBuf::from("/tmp/project"));
        let selected = relaunched.selected_session.expect("draft selected");
        assert_ne!(selected, started_id);
        let session = relaunched
            .sessions
            .iter()
            .find(|session| session.id == selected)
            .expect("draft exists");
        assert!(!session.has_started());
        assert_eq!(session.project_id, relaunched.selected_project.unwrap());
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn deleted_sessions_lose_their_row() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.sessions[0].begin_turn("Keep");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let mut extra = state.new_session(state.projects[0].id, ProviderKind::Codex);
        extra.begin_turn("Remove");
        extra.finish_active_turn(crate::model::TurnStatus::Completed);
        let removed_id = extra.id;
        state.sessions.push(extra);
        store.save(&mut state).unwrap();

        state.sessions.retain(|session| session.id != removed_id);
        store.save(&mut state).unwrap();

        let restored = store_in(&directory).load().unwrap();
        assert_eq!(restored.sessions.len(), 1);
        assert!(restored.sessions.iter().all(|s| s.id != removed_id));
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn sessions_without_transcript_blocks_remain_compatible() {
        let session = AgentSession::new(Uuid::new_v4(), ProviderKind::Grok);
        let mut value = serde_json::to_value(session).unwrap();
        value.as_object_mut().unwrap().remove("transcript_blocks");

        let restored = serde_json::from_value::<AgentSession>(value).unwrap();
        assert!(restored.transcript_blocks.is_empty());
    }

    #[test]
    fn selected_model_and_traits_are_used_for_new_sessions() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.last_provider = ProviderKind::Grok;
        state.last_model = Some("grok-code-fast-1".into());
        state.last_reasoning_effort = Some("high".into());
        state.last_service_tier = Some("fast".into());

        let remembered = state.new_session(state.projects[0].id, ProviderKind::Grok);
        let other_provider = state.new_session(state.projects[0].id, ProviderKind::Codex);

        assert_eq!(remembered.model.as_deref(), Some("grok-code-fast-1"));
        assert_eq!(remembered.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(remembered.service_tier.as_deref(), Some("fast"));
        assert!(other_provider.model.is_none());
        assert!(other_provider.reasoning_effort.is_none());
        assert!(other_provider.service_tier.is_none());
    }

    #[test]
    fn model_traits_are_remembered_by_provider_and_model() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.remember_model_traits(
            ProviderKind::Codex,
            "gpt-5.6-sol",
            Some("max".into()),
            Some("fast".into()),
            None,
        );

        assert_eq!(
            state.model_traits_for(ProviderKind::Claude, "claude-opus-5"),
            (None, None, None),
            "a different provider starts from its own defaults"
        );
        assert_eq!(
            state.model_traits_for(ProviderKind::Codex, "gpt-5.6-terra"),
            (None, None, None),
            "a different model starts from its own defaults"
        );
        assert_eq!(
            state.model_traits_for(ProviderKind::Codex, "gpt-5.6-sol"),
            (Some("max".into()), Some("fast".into()), None),
            "switching back restores both explicit choices"
        );
    }

    #[test]
    fn missing_remembered_selection_is_backfilled_from_selected_session() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let id = state.sessions[0].id;
        state.sessions[0].begin_turn("Started");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let session = state.session_mut(id).unwrap();
        session.model = Some("gpt-5.6-luna".into());
        session.reasoning_effort = Some("xhigh".into());
        session.service_tier = Some("fast".into());
        store.save(&mut state).unwrap();

        // Drop the remembered selection from app state, as a file written
        // before those fields existed would have.
        let app_state_path = directory.join("state.json");
        let mut app_state: serde_json::Value =
            serde_json::from_slice(&fs::read(&app_state_path).unwrap()).unwrap();
        for key in ["last_model", "last_reasoning_effort", "last_service_tier"] {
            app_state.as_object_mut().unwrap().remove(key);
        }
        fs::write(&app_state_path, serde_json::to_vec(&app_state).unwrap()).unwrap();

        let reopened = store_in(&directory);
        let mut restored = reopened.load().unwrap();
        reopened.hydrate(&mut restored.sessions[0]).unwrap();
        restored.backfill_remembered_selection();

        assert_eq!(restored.last_model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(restored.last_reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(restored.last_service_tier.as_deref(), Some("fast"));
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn app_bundle_root_directory_starts_with_onboarding() {
        let directory = temporary_directory();
        let state = store_in(&directory).load_or_fresh(PathBuf::from("/"));
        assert!(state.projects.is_empty());
        assert!(state.selected_session.is_none());
        fs::remove_dir_all(directory).ok();
    }
}
