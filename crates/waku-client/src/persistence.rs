//! Desktop-owned preferences and RPC proxies for daemon-owned task state.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::daemons::{DaemonKey, DaemonMap};
use crate::{Command, DaemonExposureSettings, DaemonSettings, DaemonSupervisor, ResponsePayload};
use waku_protocol::computer_use::ComputerAppGrant;
use waku_protocol::i18n::AppLanguage;
use waku_protocol::identity::DATA_DIRECTORY_NAME;
use waku_protocol::model::{
    AgentSession, FavoriteModel, Project, ProviderKind, ProviderResumeCursor,
    ProviderSessionHistory, ProviderSessionSummary, RuntimeMode, SessionWorkspace,
};
use waku_protocol::theme::ThemeSettings;

pub use waku_protocol::custom_commands::{CustomCommand, CustomCommandIcon};
pub use waku_protocol::persistence::{
    ComposerDraft, ComposerDraftAnnotation, ComposerDraftAnnotationSpan, ComposerDraftAttachment,
    ComposerDraftChange, ComposerDraftFileAnnotation, ComposerDraftKey, ComposerDraftTarget,
    ComposerDrafts, SessionMessageMatch, SessionMessageSearchScope,
};

const STATE_VERSION: u32 = 5;
const APP_STATE_VERSION: u32 = 1;

pub const DEFAULT_SIDEBAR_WIDTH: f32 = 252.0;
pub const DEFAULT_RIGHT_PANEL_WIDTH: f32 = 460.0;
pub const DEFAULT_GIT_PANEL_TOP_HEIGHT: f32 = 280.0;

/// How the desktop groups task history in the sidebar.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarGrouping {
    Project,
    #[default]
    #[serde(alias = "updated")]
    Date,
}

/// Which timestamp orders task history inside the sidebar's current grouping,
/// always most recent first.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarOrdering {
    /// The task's last agent reply, falling back to its creation.
    #[default]
    #[serde(alias = "newest")]
    LastUpdated,
    /// The task's creation.
    #[serde(alias = "oldest")]
    LastCreated,
}

/// Where selection moves after the viewed task is archived.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveNavigation {
    /// The topmost unread completion, then the idle rotation, then a fresh
    /// task — the same landing GoToNextUnreadCompletion drains to.
    #[default]
    NextUnread,
    /// The next non-busy session at-or-below the departed row's slot in
    /// sidebar order, wrapping to the top.
    NextSession,
    /// The project's New task composer.
    NewTask,
}

impl ArchiveNavigation {
    pub const ALL: [Self; 3] = [Self::NextUnread, Self::NextSession, Self::NewTask];

    /// The option names are sentences, so they localize like the setting's
    /// own label.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::NextUnread => "settings.archive_navigation_next_unread",
            Self::NextSession => "settings.archive_navigation_next_session",
            Self::NewTask => "settings.archive_navigation_new_task",
        }
    }
}

/// One of the bundled sounds the desktop can play when a task the user is
/// not looking at finishes its turn.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionSound {
    Bleep,
    #[default]
    Gentle,
    Bubble,
    Chime,
    Retro,
}

impl CompletionSound {
    pub const ALL: [Self; 5] = [
        Self::Bleep,
        Self::Gentle,
        Self::Bubble,
        Self::Chime,
        Self::Retro,
    ];

    /// Sound names are product names and stay untranslated.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bleep => "Bleep",
            Self::Gentle => "Gentle",
            Self::Bubble => "Bubble",
            Self::Chime => "Chime",
            Self::Retro => "Retro",
        }
    }
}

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

fn default_ui_font_size() -> f32 {
    DEFAULT_UI_FONT_SIZE
}

fn default_code_font_size() -> f32 {
    DEFAULT_CODE_FONT_SIZE
}

fn default_render_math() -> bool {
    true
}

fn default_open_at_last_prompt() -> bool {
    true
}

/// Sidebar transparency rides on macOS Sidebar vibrancy — a real backdrop
/// blur. Where no blur exists the feature stays off by default.
fn default_sidebar_transparency() -> bool {
    cfg!(target_os = "macos")
}

fn default_sidebar_transparency_amount() -> f32 {
    DEFAULT_SIDEBAR_TRANSPARENCY_AMOUNT
}

fn default_border_intensity() -> f32 {
    DEFAULT_BORDER_INTENSITY
}

fn default_sidebar_shortcut_tags() -> bool {
    true
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

fn default_completion_sound_volume() -> f32 {
    DEFAULT_COMPLETION_SOUND_VOLUME
}

fn default_sidebar_width() -> f32 {
    DEFAULT_SIDEBAR_WIDTH
}

fn default_right_panel_width() -> f32 {
    DEFAULT_RIGHT_PANEL_WIDTH
}

fn default_git_panel_top_height() -> f32 {
    DEFAULT_GIT_PANEL_TOP_HEIGHT
}

/// A daemon reachable over the network, shown in the same window as the
/// local catalog. The record id is the stable identity the merged catalog
/// claims rows against, so renames and re-pointed addresses never orphan
/// projects. `token` is a bearer secret; it lives in app.json beside the
/// local daemon's `daemon_exposure` token, which the file's 0600 write mode
/// already covers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteHost {
    pub id: Uuid,
    pub name: String,
    /// A WebSocket URL or `host:port` for a daemon reachable directly. Sits
    /// empty on SSH hosts — their forwarded endpoint is ephemeral.
    pub address: String,
    /// The daemon's bearer token. Empty on SSH hosts: the token lives on the
    /// remote in `~/.waku/daemon-token` and is read during provisioning.
    pub token: String,
    /// A `user@host` destination (or `~/.ssh/config` Host alias). When set,
    /// the connection runs over the platform `ssh` binary instead of a
    /// direct WebSocket and `address`/`token` are unused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_destination: Option<String>,
}

/// One daemon's catalog contribution: its project list plus the list-only
/// session projection carried by `LoadTaskState`. Cached per remote host so
/// an unreachable daemon's rows still render, marked offline.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TaskCatalog {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub sessions: Vec<AgentSession>,
}

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

/// The remembered (effort, tier, window) triple for a provider/model, from a
/// snapshot — the same lookup [`PersistedState::model_traits_for`] performs,
/// usable where the whole state is not at hand.
pub fn remembered_model_traits_for(
    traits: &[RememberedModelTraits],
    provider: ProviderKind,
    model: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    traits
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

/// Remote composer-draft proxy. Draft bytes and attachments remain owned by
/// the daemon even though the desktop keeps an in-memory editing snapshot.
/// With several daemons connected, each draft routes to the daemon that owns
/// its session or project; the snapshot is kept per daemon so diffs never
/// cross hosts.
#[derive(Clone)]
pub struct ComposerDraftStore {
    daemons: DaemonMap,
    save_state: Arc<Mutex<ComposerDraftSaveState>>,
}

#[derive(Default)]
struct ComposerDraftSaveState {
    snapshots: HashMap<DaemonKey, ComposerDrafts>,
    latest_generation: u64,
}

impl ComposerDraftStore {
    pub fn remote(daemons: DaemonMap) -> Self {
        Self {
            daemons,
            save_state: Arc::new(Mutex::new(ComposerDraftSaveState::default())),
        }
    }

    /// The subset of `drafts` whose targets `key` owns.
    fn drafts_for(&self, key: DaemonKey, drafts: &ComposerDrafts) -> ComposerDrafts {
        ComposerDrafts {
            sessions: drafts
                .sessions
                .iter()
                .filter(|(id, _)| self.daemons.session_owner(**id) == key)
                .map(|(id, draft)| (*id, draft.clone()))
                .collect(),
            new_sessions: drafts
                .new_sessions
                .iter()
                .filter(|(id, _)| self.daemons.project_owner(**id) == key)
                .map(|(id, draft)| (*id, draft.clone()))
                .collect(),
        }
    }

    pub fn load(&self) -> io::Result<ComposerDrafts> {
        let mut merged = ComposerDrafts::default();
        let mut loaded = HashMap::new();
        let mut first_error = None;
        for (key, daemon) in self.daemons.connected() {
            match daemon
                .client()
                .request(Uuid::nil(), Uuid::nil(), Command::LoadComposerDrafts)
                .map_err(to_io_error)
            {
                Ok(ResponsePayload::ComposerDrafts { drafts }) => {
                    merged
                        .sessions
                        .extend(drafts.sessions.iter().map(|(k, v)| (*k, v.clone())));
                    merged
                        .new_sessions
                        .extend(drafts.new_sessions.iter().map(|(k, v)| (*k, v.clone())));
                    loaded.insert(key, drafts);
                }
                Ok(_) => {
                    return Err(io::Error::other(
                        "Goddard daemon returned an invalid composer-drafts response",
                    ));
                }
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        self.save_state.lock().snapshots = loaded;
        match first_error {
            // A daemon that is still connecting contributes nothing yet; its
            // drafts diff in on the first save after its snapshot arrives.
            Some(error) if merged.sessions.is_empty() && merged.new_sessions.is_empty() => {
                Err(error)
            }
            _ => Ok(merged),
        }
    }

    pub fn save(&self, drafts: ComposerDrafts, generation: u64) -> io::Result<()> {
        // Serialize this client's detached background saves and compare only
        // against its last local snapshot. A second client may add or remove
        // other keys without those changes being interpreted as ours.
        let mut state = self.save_state.lock();
        if generation < state.latest_generation {
            return Ok(());
        }
        let mut first_error = None;
        for (key, daemon) in self.daemons.connected() {
            let owned = self.drafts_for(key, &drafts);
            let previous = state.snapshots.get(&key).cloned().unwrap_or_default();
            let changes = composer_draft_changes(&previous, &owned);
            if changes.is_empty() {
                state.snapshots.insert(key, owned);
                continue;
            }
            match daemon
                .client()
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    Command::ApplyComposerDraftChanges { changes },
                )
                .map_err(to_io_error)
            {
                Ok(ResponsePayload::Ack) => {
                    state.snapshots.insert(key, owned);
                }
                Ok(_) => {
                    return Err(io::Error::other(
                        "Goddard daemon returned an invalid composer-drafts save response",
                    ));
                }
                // Keep this daemon's previous snapshot so its missed changes
                // diff out again on the next save.
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        state.latest_generation = generation;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn composer_draft_changes(
    previous: &ComposerDrafts,
    next: &ComposerDrafts,
) -> Vec<ComposerDraftChange> {
    let mut changes = Vec::new();
    collect_composer_draft_changes(
        &previous.new_sessions,
        &next.new_sessions,
        |project_id| ComposerDraftTarget::NewSession { project_id },
        &mut changes,
    );
    collect_composer_draft_changes(
        &previous.sessions,
        &next.sessions,
        |session_id| ComposerDraftTarget::Session { session_id },
        &mut changes,
    );
    changes
}

fn collect_composer_draft_changes(
    previous: &HashMap<Uuid, ComposerDraft>,
    next: &HashMap<Uuid, ComposerDraft>,
    target: impl Fn(Uuid) -> ComposerDraftTarget,
    changes: &mut Vec<ComposerDraftChange>,
) {
    for (id, draft) in next {
        if previous.get(id) != Some(draft) {
            changes.push(ComposerDraftChange {
                target: target(*id),
                draft: Some(draft.clone()),
            });
        }
    }
    for id in previous.keys() {
        if !next.contains_key(id) {
            changes.push(ComposerDraftChange {
                target: target(*id),
                draft: None,
            });
        }
    }
}

/// A draft the user explicitly parked out of a composer, listed on the
/// Drafts page until it is used or deleted. Unlike the automatic
/// per-composer draft — which belongs to a session's slot and silently
/// reappears there — a saved draft is named user data with its own
/// lifetime, so it is app-local like the rest of `AppState`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SavedDraft {
    pub id: Uuid,
    /// The composer slot the text was written in: a started task, or a
    /// project's new-task draft.
    pub target: ComposerDraftTarget,
    /// The owning task's project — the card's context label and the
    /// fallback landing spot when `target`'s task no longer exists.
    pub project_id: Uuid,
    pub draft: ComposerDraft,
    pub created_at: u64,
    /// Hidden drafts leave the default list and the composer's count badge;
    /// the page's Hidden view is the only way back to them.
    #[serde(default, skip_serializing_if = "waku_protocol::model::is_false")]
    pub hidden: bool,
}

/// Last observed main-window frame in logical pixels. GPUI window bounds are
/// relative to the display the window sits on, so the frame only means
/// something together with `display` — the stable display UUID (Zed persists
/// the same pair; display *ids* renumber across reboots and replugs). While
/// the window is maximized or fullscreen these bounds keep the floating frame
/// the window returns to, not the screen-filling one.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PersistedWindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Uuid>,
}

/// A spot in the main column's back/forward history that survives relaunch.
/// Terminals never appear: their PTYs die with the process, so a stored
/// entry could only point at a dead surface.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedNavigationLocation {
    Task(Uuid),
    ProjectsPage(Uuid),
    DraftsPage,
    AutomationsPage,
}

/// A virtualized list's logical scroll position — row index plus the pixel
/// offset inside that row. Mirrors `gpui::ListOffset` without the gpui type.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PersistedListOffset {
    pub item_ix: usize,
    pub offset_in_item: f32,
}

/// A right-panel tab that can be reopened without runtime objects. Terminal,
/// browser, and background-work surfaces are omitted: their PTYs, webviews,
/// and output buffers die with the app.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedRightPanelSurface {
    Files,
    Diff,
    File(String),
    PullRequest { number: u64 },
    GitHub(Uuid),
}

/// A right-panel surface maximized over the window, if one was.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedFullscreenSurface {
    pub surface: PersistedRightPanelSurface,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The Settings page left open across a relaunch, if any. Mirrors
/// `app::SettingsPage`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedSettingsPage {
    General,
    Providers,
    Skills,
    Friends,
    Archived,
    Usage,
    Daemon,
    ComputerUse,
    Commands,
    Appearance,
    Git,
    Experiments,
    Keybindings,
}

/// The Review surface's chosen diff source — mirrors `review_diff::Source`
/// so the app crate can keep its own enum.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedDiffSource {
    LastTurn {
        session_id: Uuid,
        turn_id: Uuid,
        turn_count: usize,
    },
    Uncommitted,
    Unstaged,
    Staged,
    Committed,
    Branch,
    Commit,
}

/// The right panel's per-session state that survives relaunch: which tabs
/// were open and the file-tree/diff browsing state around them. Editor
/// contents and diff snapshots reload from disk on demand.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct PersistedRightPanelState {
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<PersistedRightPanelSurface>,
    /// Index into `surfaces` — already remapped past dropped runtime tabs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_surface: Option<usize>,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub expanded_paths: HashSet<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_selected_path: Option<String>,
    /// `None` keeps the default width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_tree_width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_selected_file: Option<usize>,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub diff_expanded_paths: HashSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_source: Option<PersistedDiffSource>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppSettings {
    pub analytics_enabled: bool,
    pub favorite_models: Vec<FavoriteModel>,
    pub theme: ThemeSettings,
    pub language: AppLanguage,
    /// Base text size for the interface, in pixels: chrome and prose are
    /// authored against the 14px default and scale from it. Hand-edited
    /// values are clamped when applied.
    pub ui_font_size: f32,
    /// Text size for code surfaces — the file editor, diffs, code blocks,
    /// and tool output — in pixels. Hand-edited values are clamped when
    /// applied.
    pub code_font_size: f32,
    /// Text size for the integrated terminal, in pixels. `None` — and any
    /// value equal to `code_font_size` — follows the code size; the terminal
    /// keeps its own size only once the two differ. Hand-edited values are
    /// clamped when applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_font_size: Option<f32>,
    /// Family name for the interface face: chrome and markdown prose.
    /// `None` keeps the platform's system UI font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_font_family: Option<String>,
    /// Family name for every monospace surface — the file editor, diffs,
    /// code blocks, tool output, and the terminal. `None` keeps the bundled
    /// JetBrains Mono.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_font_family: Option<String>,
    pub render_math: bool,
    /// Append an estimated "N tok/s" readout to settled response footers.
    pub show_response_token_speed: bool,
    /// Open a task that is not mid-turn scrolled to its last prompt instead
    /// of the end of the transcript.
    pub open_at_last_prompt: bool,
    /// Integrate upstream changes with `git pull --no-rebase` (a merge)
    /// instead of `git pull --rebase` when a checkout is synced from the new
    /// task area.
    pub sync_with_merge: bool,
    /// A sync that stops on conflicts skips the Resolve-in-chat button and
    /// starts a fresh chat on the checkout with the resolution prompt
    /// already sent.
    pub auto_resolve_in_chat: bool,
    /// When a land stops on rebase or merge conflicts, send the resolution
    /// prompt to the owning task's chat instead of showing the conflict
    /// dialog.
    pub auto_resolve_land_conflicts: bool,
    /// Fork a planned worktree from the repository's default branch instead
    /// of reopening the base branch last picked for the project.
    pub new_worktree_default_branch: bool,
    /// Fast-forward the local default branch to its tracking branch before a
    /// new worktree bases on it.
    pub new_worktree_sync_default_branch: bool,
    /// Additional local branches that get the same fast-forward when one is
    /// a new worktree's base.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_worktree_sync_branches: Vec<String>,
    /// macOS-only: blend the desktop behind the sidebar through vibrancy
    /// instead of painting a solid fill.
    pub sidebar_transparency: bool,
    /// How much of the vibrancy shows through the sidebar's tint when
    /// `sidebar_transparency` is on: 0.0 is a solid fill, higher values let
    /// more of the desktop blur through. Hand-edited values are clamped to
    /// `MAX_SIDEBAR_TRANSPARENCY` when applied.
    pub sidebar_transparency_amount: f32,
    /// Draw borders and separators a full pixel thick instead of the default
    /// half-pixel hairline.
    pub thick_borders: bool,
    /// How strongly borders and separators read: 1.0 is the solved contrast
    /// the palettes ship with, 0.0 erases the lines entirely. Hand-edited
    /// values are clamped to `MAX_BORDER_INTENSITY` when applied.
    pub border_intensity: f32,
    /// Solve border tiers against wider contrast floors, putting component
    /// outlines on WCAG's 3:1 non-text floor.
    pub high_contrast: bool,
    /// macOS-only: move back and forward between tasks with a three-finger
    /// horizontal trackpad swipe.
    pub three_finger_swipe_navigation: bool,
    /// Tag the sidebar's first tasks with their ⌘n chords while the shortcut
    /// modifier is held. The chords keep working with the tags off.
    pub sidebar_shortcut_tags: bool,
    /// Where selection lands after the viewed task is archived.
    pub archive_navigation: ArchiveNavigation,
    pub daemon_exposure: DaemonExposureSettings,
    /// Preferred target of the header's "open project in app" control, by
    /// catalog id. `None` — and an id no longer installed — fall back to the
    /// platform file manager.
    pub open_in_app: Option<String>,
    /// Play `completion_sound` when a session that is not selected finishes
    /// its turn.
    pub completion_sound_enabled: bool,
    pub completion_sound: CompletionSound,
    /// Volume the completion sound plays at relative to its recorded level:
    /// 1.0 is the sound as bundled, up to 2.0 plays it louder. Hand-edited
    /// values are clamped when applied.
    pub completion_sound_volume: f32,
    /// User-owned terminal commands surfaced in the command palette.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_commands: Vec<CustomCommand>,
    /// Experimental: ⌘0 full-window grid of session cards with live
    /// transcripts. Defaults on in debug builds.
    pub big_picture_enabled: bool,
    /// Experimental: the Git panel (⌘⌥G) commit graph and diffs. Defaults on
    /// in debug builds.
    pub git_panel_enabled: bool,
    /// Experimental: GitHub issues and pull requests on the Projects page,
    /// sidebar rows, and the right panel. Defaults on in debug builds.
    pub github_enabled: bool,
    /// Experimental: the Projects page (⌘⇧P) — a project's worktrees,
    /// branches, issues, and pull requests in one place. Defaults on in
    /// debug builds.
    pub projects_page_enabled: bool,
    /// Experimental: the Settings → Friends page and friend-to-friend file
    /// transfers. Defaults on in debug builds.
    pub friends_enabled: bool,
    /// Experimental: Auto in the model picker routes a task's first prompt
    /// through the daemon's evaluation model and starts on the resolved
    /// provider/model. Defaults on in debug builds.
    pub model_router_enabled: bool,
    /// Experimental: each assistant turn that ends while its session is on
    /// screen is scored by the evaluation model against a fixed marker set;
    /// markers that clear their threshold render as chips on the response
    /// footer. Defaults on in debug builds.
    pub status_markers_enabled: bool,
    /// Experimental: the Automations page — daemon-scheduled prompts that
    /// run as tasks whether or not the app is open. Defaults on in debug
    /// builds.
    pub automations_enabled: bool,
    /// Saved remote daemons connected alongside the local one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_hosts: Vec<RemoteHost>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            analytics_enabled: default_analytics_enabled(),
            favorite_models: Vec::new(),
            theme: ThemeSettings::default(),
            language: AppLanguage::default(),
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            code_font_size: DEFAULT_CODE_FONT_SIZE,
            terminal_font_size: None,
            ui_font_family: None,
            code_font_family: None,
            render_math: true,
            show_response_token_speed: false,
            open_at_last_prompt: true,
            sync_with_merge: false,
            auto_resolve_in_chat: false,
            auto_resolve_land_conflicts: false,
            new_worktree_default_branch: false,
            new_worktree_sync_default_branch: false,
            new_worktree_sync_branches: Vec::new(),
            sidebar_transparency: default_sidebar_transparency(),
            sidebar_transparency_amount: DEFAULT_SIDEBAR_TRANSPARENCY_AMOUNT,
            thick_borders: false,
            border_intensity: DEFAULT_BORDER_INTENSITY,
            high_contrast: false,
            three_finger_swipe_navigation: false,
            sidebar_shortcut_tags: true,
            archive_navigation: ArchiveNavigation::default(),
            daemon_exposure: DaemonExposureSettings::default(),
            open_in_app: None,
            completion_sound_enabled: false,
            completion_sound: CompletionSound::default(),
            completion_sound_volume: DEFAULT_COMPLETION_SOUND_VOLUME,
            custom_commands: Vec::new(),
            big_picture_enabled: default_experiment_enabled(),
            git_panel_enabled: default_experiment_enabled(),
            github_enabled: default_experiment_enabled(),
            projects_page_enabled: default_experiment_enabled(),
            friends_enabled: default_experiment_enabled(),
            model_router_enabled: default_experiment_enabled(),
            status_markers_enabled: default_experiment_enabled(),
            automations_enabled: default_experiment_enabled(),
            remote_hosts: Vec::new(),
        }
    }
}

pub const DEFAULT_UI_FONT_SIZE: f32 = 14.0;
pub const DEFAULT_CODE_FONT_SIZE: f32 = 13.0;
pub const DEFAULT_COMPLETION_SOUND_VOLUME: f32 = 1.0;
/// The completion sound's relative volume tops out at twice its recorded level.
pub const MAX_COMPLETION_SOUND_VOLUME: f32 = 2.0;
/// Fraction of the Sidebar vibrancy let through the sidebar's tint by
/// default — visible without competing with row text.
pub const DEFAULT_SIDEBAR_TRANSPARENCY_AMOUNT: f32 = 0.25;
/// The vibrancy past ~60% of the mix starts losing text legibility on busy
/// backdrops, so the slider stops there.
pub const MAX_SIDEBAR_TRANSPARENCY: f32 = 0.6;
/// Border weight out of the box: visibly fainter than the solved floors the
/// slider's 100% restores — chrome stays quiet by default.
pub const DEFAULT_BORDER_INTENSITY: f32 = 0.6;
/// The slider tops out at the palettes' authored border contrast.
pub const MAX_BORDER_INTENSITY: f32 = 1.0;

/// Bounds a possibly hand-edited font size to something the layout survives.
fn sanitized_font_size(size: f32, fallback: f32) -> f32 {
    if size.is_finite() {
        size.clamp(9.0, 24.0)
    } else {
        fallback
    }
}

pub fn sanitized_ui_font_size(size: f32) -> f32 {
    sanitized_font_size(size, DEFAULT_UI_FONT_SIZE)
}

pub fn sanitized_code_font_size(size: f32) -> f32 {
    sanitized_font_size(size, DEFAULT_CODE_FONT_SIZE)
}

/// `None` stays unset — the terminal then follows the code font size.
pub fn sanitized_terminal_font_size(size: Option<f32>) -> Option<f32> {
    size.filter(|size| size.is_finite())
        .map(|size| size.clamp(9.0, 24.0))
}

/// Bounds a possibly hand-edited volume to the slider's relative range.
pub fn sanitized_completion_sound_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, MAX_COMPLETION_SOUND_VOLUME)
    } else {
        DEFAULT_COMPLETION_SOUND_VOLUME
    }
}

/// Bounds a possibly hand-edited amount to the slider's range.
pub fn sanitized_sidebar_transparency_amount(amount: f32) -> f32 {
    if amount.is_finite() {
        amount.clamp(0.0, MAX_SIDEBAR_TRANSPARENCY)
    } else {
        DEFAULT_SIDEBAR_TRANSPARENCY_AMOUNT
    }
}

/// Bounds a possibly hand-edited intensity to the slider's range.
pub fn sanitized_border_intensity(intensity: f32) -> f32 {
    if intensity.is_finite() {
        intensity.clamp(0.0, MAX_BORDER_INTENSITY)
    } else {
        DEFAULT_BORDER_INTENSITY
    }
}

/// A blank family name is no choice at all — treat it as unset so a
/// whitespace-only `app.json` value still resolves to the default face.
/// An unknown name is kept: font resolution falls back per glyph anyway.
pub fn sanitized_font_family(family: Option<String>) -> Option<String> {
    family.and_then(|family| {
        let family = family.trim();
        (!family.is_empty()).then(|| family.to_owned())
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AppState {
    app_state_version: u32,
    #[serde(default = "Uuid::new_v4")]
    analytics_id: Uuid,
    #[serde(default)]
    selected_project: Option<Uuid>,
    #[serde(default)]
    selected_session: Option<Uuid>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    unseen_completions: HashMap<Uuid, u64>,
    #[serde(default = "default_provider")]
    last_provider: ProviderKind,
    /// The last model pick was the router's Auto row — the next draft keeps
    /// Auto selected instead of inheriting the routed provider/model.
    #[serde(default, skip_serializing_if = "waku_protocol::model::is_false")]
    last_auto_route: bool,
    #[serde(default)]
    last_runtime_mode: RuntimeMode,
    #[serde(default, skip_serializing_if = "waku_protocol::model::is_false")]
    last_sandboxed: bool,
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
    /// The workspace mode last chosen for a draft in each project, applied
    /// to that project's next fresh task.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    project_workspaces: HashMap<Uuid, SessionWorkspace>,
    #[serde(default = "default_sidebar_visibility")]
    sidebar_visible: bool,
    #[serde(default = "default_right_panel_visibility")]
    right_panel_visible: bool,
    #[serde(default)]
    git_panel_visible: bool,
    #[serde(default = "default_sidebar_width")]
    sidebar_width: f32,
    #[serde(default)]
    sidebar_grouping: SidebarGrouping,
    #[serde(default)]
    sidebar_ordering: SidebarOrdering,
    #[serde(default = "default_right_panel_width")]
    right_panel_width: f32,
    /// Whether markdown files in the right panel open as a rendered preview
    /// instead of source. One global mode, not per file.
    #[serde(default)]
    markdown_preview: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_state: Option<PersistedWindowState>,
    /// Main-column back/forward history. `Terminal` entries are dropped on
    /// save — see [`PersistedNavigationLocation`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    navigation_back: Vec<PersistedNavigationLocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    navigation_forward: Vec<PersistedNavigationLocation>,
    /// Reading position each task's transcript held when last on screen.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    transcript_scroll_positions: HashMap<Uuid, PersistedListOffset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_scroll: Option<PersistedListOffset>,
    /// The Projects page claiming the main column, if it was on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projects_page: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    settings_page: Option<PersistedSettingsPage>,
    /// Parked right-panel state per task, plus the selected task's live one.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    right_panel_sessions: HashMap<Uuid, PersistedRightPanelState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fullscreen_surface: Option<PersistedFullscreenSurface>,
    /// Drafts parked from a composer via "Create draft", newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    saved_drafts: Vec<SavedDraft>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PersistedState {
    pub version: u32,
    #[serde(default = "Uuid::new_v4")]
    pub analytics_id: Uuid,
    #[serde(default = "default_analytics_enabled")]
    pub analytics_enabled: bool,
    pub projects: Vec<Project>,
    pub sessions: Vec<AgentSession>,
    pub selected_project: Option<Uuid>,
    pub selected_session: Option<Uuid>,
    /// Tasks whose turn settled while another task was on screen, stamped
    /// with the finish time; the sidebar draws an unread dot until the task
    /// is activated. App-local like the selection, not task state.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub unseen_completions: HashMap<Uuid, u64>,
    pub last_provider: ProviderKind,
    /// The last model pick was the router's Auto row — the next draft keeps
    /// Auto selected instead of inheriting the routed provider/model.
    #[serde(default, skip_serializing_if = "waku_protocol::model::is_false")]
    pub last_auto_route: bool,
    #[serde(default)]
    pub last_runtime_mode: RuntimeMode,
    #[serde(default, skip_serializing_if = "waku_protocol::model::is_false")]
    pub last_sandboxed: bool,
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
    /// The workspace mode last chosen for a draft in each project, applied
    /// to that project's next fresh task. Only `Local` and `NewWorktree`
    /// are stored; a materialized worktree is a result, not a choice.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub project_workspaces: HashMap<Uuid, SessionWorkspace>,
    #[serde(default)]
    pub favorite_models: Vec<FavoriteModel>,
    #[serde(default)]
    pub theme: ThemeSettings,
    #[serde(default)]
    pub language: AppLanguage,
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: f32,
    #[serde(default = "default_code_font_size")]
    pub code_font_size: f32,
    /// `None` — and any value equal to `code_font_size` — follows the code
    /// size; a different value is the terminal's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_font_size: Option<f32>,
    /// Family name for the interface face; `None` is the system UI font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_font_family: Option<String>,
    /// Family name for monospace surfaces; `None` is the bundled
    /// JetBrains Mono.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_font_family: Option<String>,
    #[serde(default = "default_render_math")]
    pub render_math: bool,
    /// Append an estimated "N tok/s" readout to settled response footers.
    #[serde(default)]
    pub show_response_token_speed: bool,
    #[serde(default = "default_open_at_last_prompt")]
    pub open_at_last_prompt: bool,
    /// Integrate upstream changes with `git pull --no-rebase` (a merge)
    /// instead of `git pull --rebase` when a checkout is synced from the new
    /// task area.
    #[serde(default)]
    pub sync_with_merge: bool,
    /// A sync that stops on conflicts skips the Resolve-in-chat button and
    /// starts a fresh chat on the checkout with the resolution prompt
    /// already sent.
    #[serde(default)]
    pub auto_resolve_in_chat: bool,
    /// When a land stops on rebase or merge conflicts, send the resolution
    /// prompt to the owning task's chat instead of showing the conflict
    /// dialog.
    #[serde(default)]
    pub auto_resolve_land_conflicts: bool,
    /// Fork a planned worktree from the repository's default branch instead
    /// of reopening the base branch last picked for the project.
    #[serde(default)]
    pub new_worktree_default_branch: bool,
    /// Fast-forward the local default branch to its tracking branch before a
    /// new worktree bases on it.
    #[serde(default)]
    pub new_worktree_sync_default_branch: bool,
    /// Additional local branches that get the same fast-forward when one is
    /// a new worktree's base.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_worktree_sync_branches: Vec<String>,
    /// macOS-only: blend the desktop behind the sidebar through vibrancy
    /// instead of painting a solid fill.
    #[serde(default = "default_sidebar_transparency")]
    pub sidebar_transparency: bool,
    /// How much of the vibrancy shows through the sidebar's tint when
    /// `sidebar_transparency` is on.
    #[serde(default = "default_sidebar_transparency_amount")]
    pub sidebar_transparency_amount: f32,
    /// Draw borders and separators a full pixel thick instead of the default
    /// half-pixel hairline.
    #[serde(default)]
    pub thick_borders: bool,
    /// How strongly borders and separators read: 1.0 is the solved contrast
    /// the palettes ship with, 0.0 erases the lines entirely.
    #[serde(default = "default_border_intensity")]
    pub border_intensity: f32,
    /// Solve border tiers against wider contrast floors, putting component
    /// outlines on WCAG's 3:1 non-text floor.
    #[serde(default)]
    pub high_contrast: bool,
    /// macOS-only, opt-in: move back and forward between tasks with a
    /// three-finger horizontal trackpad swipe.
    #[serde(default)]
    pub three_finger_swipe_navigation: bool,
    /// Whether holding the shortcut modifier tags the sidebar's first tasks
    /// with their ⌘n chords. The chords keep working with the tags off.
    #[serde(default = "default_sidebar_shortcut_tags")]
    pub sidebar_shortcut_tags: bool,
    /// Where selection lands after the viewed task is archived.
    #[serde(default)]
    pub archive_navigation: ArchiveNavigation,
    #[serde(default)]
    pub daemon_exposure: DaemonExposureSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_in_app: Option<String>,
    #[serde(default)]
    pub completion_sound_enabled: bool,
    #[serde(default)]
    pub completion_sound: CompletionSound,
    #[serde(default = "default_completion_sound_volume")]
    pub completion_sound_volume: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_commands: Vec<CustomCommand>,
    /// Experimental feature opt-ins from the Experiments settings page.
    /// Each defaults on in debug builds; an explicit `false` still wins.
    #[serde(default = "default_experiment_enabled")]
    pub big_picture_enabled: bool,
    #[serde(default = "default_experiment_enabled")]
    pub git_panel_enabled: bool,
    #[serde(default = "default_experiment_enabled")]
    pub github_enabled: bool,
    #[serde(default = "default_experiment_enabled")]
    pub projects_page_enabled: bool,
    #[serde(default = "default_experiment_enabled")]
    pub friends_enabled: bool,
    #[serde(default = "default_experiment_enabled")]
    pub model_router_enabled: bool,
    #[serde(default = "default_experiment_enabled")]
    pub status_markers_enabled: bool,
    #[serde(default = "default_experiment_enabled")]
    pub automations_enabled: bool,
    /// Saved remote daemons connected alongside the local one; app-owned,
    /// persisted through `app_settings`/`apply_app_settings`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_hosts: Vec<RemoteHost>,
    #[serde(default = "default_sidebar_visibility")]
    pub sidebar_visible: bool,
    #[serde(default = "default_right_panel_visibility")]
    pub right_panel_visible: bool,
    /// The Git panel shares the right panel's slot; both are never visible.
    #[serde(default)]
    pub git_panel_visible: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default)]
    pub sidebar_grouping: SidebarGrouping,
    #[serde(default)]
    pub sidebar_ordering: SidebarOrdering,
    #[serde(default = "default_right_panel_width")]
    pub right_panel_width: f32,
    /// Height of the Git panel's top region — the commit box, or an open
    /// commit's file tree — split from the commit log by a drag handle.
    #[serde(default = "default_git_panel_top_height")]
    pub git_panel_top_height: f32,
    /// Whether markdown files in the right panel open as a rendered preview
    /// instead of source. One global mode, not per file.
    #[serde(default)]
    pub markdown_preview: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_state: Option<PersistedWindowState>,
    /// Main-column back/forward history, sans terminal entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub navigation_back: Vec<PersistedNavigationLocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub navigation_forward: Vec<PersistedNavigationLocation>,
    /// Reading position each task's transcript held when last on screen.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub transcript_scroll_positions: HashMap<Uuid, PersistedListOffset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_scroll: Option<PersistedListOffset>,
    /// The Projects page claiming the main column, if it was on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects_page: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_page: Option<PersistedSettingsPage>,
    /// Parked right-panel state per task, plus the selected task's live one.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub right_panel_sessions: HashMap<Uuid, PersistedRightPanelState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fullscreen_surface: Option<PersistedFullscreenSurface>,
    /// Drafts parked from a composer via "Create draft", newest first.
    /// App-local — they persist through `AppState`, not the daemon.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_drafts: Vec<SavedDraft>,
    #[serde(default = "default_computer_use_enabled")]
    pub computer_use_enabled: bool,
    /// Experimental opt-in gating Computer Use entirely. Daemon-owned;
    /// mirrored here so clients can render the toggle and the gated page.
    #[serde(default = "default_experiment_enabled")]
    pub computer_use_experiment_enabled: bool,
    #[serde(default)]
    pub computer_use_allowed_apps: Vec<ComputerAppGrant>,
    #[serde(default)]
    pub disabled_providers: Vec<ProviderKind>,
    #[serde(default)]
    pub provider_binary_overrides: HashMap<ProviderKind, String>,
    #[serde(default)]
    pub agent_tools_enabled: bool,
    #[serde(default = "default_agent_settings_enabled")]
    pub agent_settings_enabled: bool,
    /// Experimental: whether sessions get named subagents injected. Daemon-
    /// owned; mirrored here so clients can render the toggle.
    #[serde(default = "default_experiment_enabled")]
    pub subagents_enabled: bool,
    /// Named subagent tiers injected into every session's harness. Daemon-
    /// owned; mirrored here so clients can render what will be injected.
    #[serde(default)]
    pub subagent_tiers: BTreeMap<String, waku_protocol::settings::SubagentTier>,
    /// Hosted evaluation-model settings. Daemon-owned; mirrored in memory so
    /// the settings surface can read and edit it. Never written into the
    /// client's own state — the credential-bearing document is the daemon's
    /// `settings.json`.
    #[serde(skip)]
    pub eval: Option<waku_protocol::eval::EvalSettings>,
    #[serde(skip)]
    daemon_settings_extra: BTreeMap<String, serde_json::Value>,
    #[serde(skip)]
    dirty_sessions: HashSet<Uuid>,
}

impl PersistedState {
    pub fn session_mut(&mut self, id: Uuid) -> Option<&mut AgentSession> {
        let session = self.sessions.iter_mut().find(|session| session.id == id)?;
        self.dirty_sessions.insert(id);
        Some(session)
    }

    /// The terminal's effective text size — its own once it differs from
    /// `code_font_size`, the code size while they match.
    pub fn terminal_font_size(&self) -> f32 {
        self.terminal_font_size.unwrap_or(self.code_font_size)
    }

    pub fn mark_session_dirty(&mut self, id: Uuid) {
        self.dirty_sessions.insert(id);
    }

    pub fn push_session(&mut self, session: AgentSession) {
        self.dirty_sessions.insert(session.id);
        self.sessions.push(session);
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
            unseen_completions: HashMap::new(),
            last_provider: ProviderKind::Codex,
            last_auto_route: false,
            last_runtime_mode: RuntimeMode::default(),
            last_sandboxed: false,
            last_model: None,
            last_reasoning_effort: None,
            last_service_tier: None,
            last_context_window: None,
            remembered_model_traits: Vec::new(),
            recent_model_uses: Vec::new(),
            project_workspaces: HashMap::new(),
            favorite_models: Vec::new(),
            theme: ThemeSettings::default(),
            language: AppLanguage::default(),
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            code_font_size: DEFAULT_CODE_FONT_SIZE,
            terminal_font_size: None,
            ui_font_family: None,
            code_font_family: None,
            render_math: true,
            show_response_token_speed: false,
            open_at_last_prompt: true,
            sync_with_merge: false,
            auto_resolve_in_chat: false,
            auto_resolve_land_conflicts: false,
            new_worktree_default_branch: false,
            new_worktree_sync_default_branch: false,
            new_worktree_sync_branches: Vec::new(),
            sidebar_transparency: default_sidebar_transparency(),
            sidebar_transparency_amount: DEFAULT_SIDEBAR_TRANSPARENCY_AMOUNT,
            thick_borders: false,
            border_intensity: DEFAULT_BORDER_INTENSITY,
            high_contrast: false,
            three_finger_swipe_navigation: false,
            sidebar_shortcut_tags: true,
            archive_navigation: ArchiveNavigation::default(),
            daemon_exposure: DaemonExposureSettings::default(),
            open_in_app: None,
            completion_sound_enabled: false,
            completion_sound: CompletionSound::default(),
            completion_sound_volume: DEFAULT_COMPLETION_SOUND_VOLUME,
            custom_commands: Vec::new(),
            big_picture_enabled: default_experiment_enabled(),
            git_panel_enabled: default_experiment_enabled(),
            github_enabled: default_experiment_enabled(),
            projects_page_enabled: default_experiment_enabled(),
            friends_enabled: default_experiment_enabled(),
            model_router_enabled: default_experiment_enabled(),
            status_markers_enabled: default_experiment_enabled(),
            automations_enabled: default_experiment_enabled(),
            remote_hosts: Vec::new(),
            sidebar_visible: true,
            right_panel_visible: false,
            git_panel_visible: false,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_grouping: SidebarGrouping::Date,
            sidebar_ordering: SidebarOrdering::LastUpdated,
            right_panel_width: DEFAULT_RIGHT_PANEL_WIDTH,
            git_panel_top_height: DEFAULT_GIT_PANEL_TOP_HEIGHT,
            markdown_preview: false,
            window_state: None,
            navigation_back: Vec::new(),
            navigation_forward: Vec::new(),
            transcript_scroll_positions: HashMap::new(),
            sidebar_scroll: None,
            projects_page: None,
            settings_page: None,
            right_panel_sessions: HashMap::new(),
            fullscreen_surface: None,
            saved_drafts: Vec::new(),
            computer_use_enabled: false,
            computer_use_experiment_enabled: default_experiment_enabled(),
            computer_use_allowed_apps: Vec::new(),
            disabled_providers: Vec::new(),
            provider_binary_overrides: HashMap::new(),
            agent_tools_enabled: false,
            agent_settings_enabled: true,
            subagents_enabled: default_experiment_enabled(),
            subagent_tiers: BTreeMap::new(),
            eval: None,
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
        session.runtime_mode = self.last_runtime_mode;
        session.sandboxed = self.last_sandboxed;
        // An Auto pick carries to the next draft like the provider/model do;
        // the seeded provider/model stay as the route's last-used hint.
        session.auto_route = self.last_auto_route && self.model_router_enabled;
        if provider == self.last_provider {
            session.model.clone_from(&self.last_model);
            session
                .reasoning_effort
                .clone_from(&self.last_reasoning_effort);
            session.service_tier.clone_from(&self.last_service_tier);
            session.context_window.clone_from(&self.last_context_window);
        }
        session.workspace = self.workspace_for_new_session(project_id);
        session
    }

    /// Records the workspace mode chosen for a draft in `project_id`; the
    /// project's next fresh task reopens with it. A materialized `Worktree`
    /// is the result of a choice, not a choice, so it is never stored.
    pub fn remember_workspace(&mut self, project_id: Uuid, workspace: &SessionWorkspace) {
        match workspace {
            SessionWorkspace::Local | SessionWorkspace::NewWorktree { .. } => {
                self.project_workspaces
                    .insert(project_id, workspace.clone());
            }
            SessionWorkspace::Worktree { .. } => {}
        }
    }

    /// Base branch a planned worktree in `project_id` reopens with, when one
    /// was picked before. `new_worktree_default_branch` keeps the
    /// repository's default branch in charge, so a remembered base does not
    /// apply while it is on.
    pub fn remembered_base_branch(&self, project_id: Uuid) -> Option<String> {
        if self.new_worktree_default_branch {
            return None;
        }
        match self.project_workspaces.get(&project_id) {
            Some(SessionWorkspace::NewWorktree { base_branch }) => base_branch.clone(),
            _ => None,
        }
    }

    /// The workspace mode a fresh draft for `project_id` opens with — the
    /// last one chosen there. Projectless and unknown projects stay local:
    /// they have no repository to fork a worktree from.
    pub fn workspace_for_new_session(&self, project_id: Uuid) -> SessionWorkspace {
        let has_repository = self
            .projects
            .iter()
            .any(|project| project.id == project_id && !project.is_projectless());
        if !has_repository {
            return SessionWorkspace::Local;
        }
        match self.project_workspaces.get(&project_id) {
            Some(SessionWorkspace::NewWorktree { base_branch }) => SessionWorkspace::NewWorktree {
                // `None` resolves the repository's default branch on the
                // daemon; `new_worktree_default_branch` drops the base last
                // picked so a fresh task always forks from it.
                base_branch: if self.new_worktree_default_branch {
                    None
                } else {
                    base_branch.clone()
                },
            },
            Some(workspace @ SessionWorkspace::Local) => workspace.clone(),
            _ => SessionWorkspace::Local,
        }
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
        remembered_model_traits_for(&self.remembered_model_traits, provider, model)
    }

    /// Every remembered triple, for callers that must look up a model they
    /// do not know yet — a routed start resolves its target off-thread.
    pub fn remembered_model_traits(&self) -> &[RememberedModelTraits] {
        &self.remembered_model_traits
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
                used_at: waku_protocol::model::unix_time(),
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

    pub fn daemon_settings(&self) -> DaemonSettings {
        DaemonSettings {
            computer_use_enabled: self.computer_use_enabled,
            computer_use_experiment_enabled: self.computer_use_experiment_enabled,
            computer_use_allowed_apps: self.computer_use_allowed_apps.clone(),
            disabled_providers: self.disabled_providers.clone(),
            provider_binary_overrides: self.provider_binary_overrides.clone(),
            agent_tools_enabled: self.agent_tools_enabled,
            agent_settings_enabled: self.agent_settings_enabled,
            subagents_enabled: self.subagents_enabled,
            subagent_tiers: self.subagent_tiers.clone(),
            custom_commands: self.custom_commands.clone(),
            eval: self.eval.clone(),
            extra: self.daemon_settings_extra.clone(),
        }
    }

    /// Merge the daemon's settings document into this state. Custom commands
    /// move with it: they are daemon-owned, so whatever the document carries
    /// replaces the local mirror — including an empty list after another
    /// client or an agent removed the last one.
    pub fn apply_daemon_settings(&mut self, settings: DaemonSettings) {
        self.computer_use_enabled = settings.computer_use_enabled;
        self.computer_use_experiment_enabled = settings.computer_use_experiment_enabled;
        self.computer_use_allowed_apps = settings.computer_use_allowed_apps;
        self.disabled_providers = settings.disabled_providers;
        self.provider_binary_overrides = settings.provider_binary_overrides;
        self.agent_tools_enabled = settings.agent_tools_enabled;
        self.agent_settings_enabled = settings.agent_settings_enabled;
        self.subagents_enabled = settings.subagents_enabled;
        self.subagent_tiers = settings.subagent_tiers;
        self.custom_commands = settings.custom_commands;
        self.eval = settings.eval;
        self.daemon_settings_extra = settings.extra;
    }

    fn app_settings(&self) -> AppSettings {
        AppSettings {
            analytics_enabled: self.analytics_enabled,
            favorite_models: self.favorite_models.clone(),
            theme: self.theme,
            language: self.language,
            ui_font_size: self.ui_font_size,
            code_font_size: self.code_font_size,
            terminal_font_size: self.terminal_font_size,
            ui_font_family: self.ui_font_family.clone(),
            code_font_family: self.code_font_family.clone(),
            render_math: self.render_math,
            show_response_token_speed: self.show_response_token_speed,
            open_at_last_prompt: self.open_at_last_prompt,
            sync_with_merge: self.sync_with_merge,
            auto_resolve_in_chat: self.auto_resolve_in_chat,
            auto_resolve_land_conflicts: self.auto_resolve_land_conflicts,
            new_worktree_default_branch: self.new_worktree_default_branch,
            new_worktree_sync_default_branch: self.new_worktree_sync_default_branch,
            new_worktree_sync_branches: self.new_worktree_sync_branches.clone(),
            sidebar_transparency: self.sidebar_transparency,
            sidebar_transparency_amount: self.sidebar_transparency_amount,
            thick_borders: self.thick_borders,
            border_intensity: self.border_intensity,
            high_contrast: self.high_contrast,
            three_finger_swipe_navigation: self.three_finger_swipe_navigation,
            sidebar_shortcut_tags: self.sidebar_shortcut_tags,
            archive_navigation: self.archive_navigation,
            daemon_exposure: self.daemon_exposure.clone(),
            open_in_app: self.open_in_app.clone(),
            completion_sound_enabled: self.completion_sound_enabled,
            completion_sound: self.completion_sound,
            completion_sound_volume: self.completion_sound_volume,
            custom_commands: self.custom_commands.clone(),
            big_picture_enabled: self.big_picture_enabled,
            git_panel_enabled: self.git_panel_enabled,
            github_enabled: self.github_enabled,
            projects_page_enabled: self.projects_page_enabled,
            friends_enabled: self.friends_enabled,
            model_router_enabled: self.model_router_enabled,
            status_markers_enabled: self.status_markers_enabled,
            automations_enabled: self.automations_enabled,
            remote_hosts: self.remote_hosts.clone(),
        }
    }

    fn app_state(&self) -> AppState {
        AppState {
            app_state_version: APP_STATE_VERSION,
            analytics_id: self.analytics_id,
            selected_project: self.selected_project,
            selected_session: self.persistable_selected_session(),
            unseen_completions: self.unseen_completions.clone(),
            last_provider: self.last_provider,
            last_auto_route: self.last_auto_route,
            last_runtime_mode: self.last_runtime_mode,
            last_sandboxed: self.last_sandboxed,
            last_model: self.last_model.clone(),
            last_reasoning_effort: self.last_reasoning_effort.clone(),
            last_service_tier: self.last_service_tier.clone(),
            last_context_window: self.last_context_window.clone(),
            remembered_model_traits: self.remembered_model_traits.clone(),
            recent_model_uses: self.recent_model_uses.clone(),
            project_workspaces: self.project_workspaces.clone(),
            sidebar_visible: self.sidebar_visible,
            right_panel_visible: self.right_panel_visible,
            git_panel_visible: self.git_panel_visible,
            sidebar_width: self.sidebar_width,
            sidebar_grouping: self.sidebar_grouping,
            sidebar_ordering: self.sidebar_ordering,
            right_panel_width: self.right_panel_width,
            markdown_preview: self.markdown_preview,
            window_state: self.window_state,
            navigation_back: self.navigation_back.clone(),
            navigation_forward: self.navigation_forward.clone(),
            transcript_scroll_positions: self.transcript_scroll_positions.clone(),
            sidebar_scroll: self.sidebar_scroll,
            projects_page: self.projects_page,
            settings_page: self.settings_page,
            right_panel_sessions: self.right_panel_sessions.clone(),
            fullscreen_surface: self.fullscreen_surface.clone(),
            saved_drafts: self.saved_drafts.clone(),
        }
    }

    fn apply_app_settings(&mut self, settings: AppSettings) {
        self.analytics_enabled = settings.analytics_enabled;
        self.favorite_models = settings.favorite_models;
        self.theme = settings.theme;
        self.language = settings.language;
        self.ui_font_size = sanitized_ui_font_size(settings.ui_font_size);
        self.code_font_size = sanitized_code_font_size(settings.code_font_size);
        self.terminal_font_size = sanitized_terminal_font_size(settings.terminal_font_size);
        // Equal means linked: a stored override matching the code size would
        // silently stop following it.
        if self.terminal_font_size == Some(self.code_font_size) {
            self.terminal_font_size = None;
        }
        self.ui_font_family = sanitized_font_family(settings.ui_font_family);
        self.code_font_family = sanitized_font_family(settings.code_font_family);
        self.render_math = settings.render_math;
        self.show_response_token_speed = settings.show_response_token_speed;
        self.open_at_last_prompt = settings.open_at_last_prompt;
        self.sync_with_merge = settings.sync_with_merge;
        self.auto_resolve_in_chat = settings.auto_resolve_in_chat;
        self.auto_resolve_land_conflicts = settings.auto_resolve_land_conflicts;
        self.new_worktree_default_branch = settings.new_worktree_default_branch;
        self.new_worktree_sync_default_branch = settings.new_worktree_sync_default_branch;
        self.new_worktree_sync_branches = settings.new_worktree_sync_branches;
        self.sidebar_transparency = settings.sidebar_transparency;
        self.sidebar_transparency_amount =
            sanitized_sidebar_transparency_amount(settings.sidebar_transparency_amount);
        self.thick_borders = settings.thick_borders;
        self.border_intensity = sanitized_border_intensity(settings.border_intensity);
        self.high_contrast = settings.high_contrast;
        self.three_finger_swipe_navigation = settings.three_finger_swipe_navigation;
        self.sidebar_shortcut_tags = settings.sidebar_shortcut_tags;
        self.archive_navigation = settings.archive_navigation;
        self.daemon_exposure = settings.daemon_exposure;
        self.open_in_app = settings.open_in_app;
        self.completion_sound_enabled = settings.completion_sound_enabled;
        self.completion_sound = settings.completion_sound;
        self.completion_sound_volume =
            sanitized_completion_sound_volume(settings.completion_sound_volume);
        self.custom_commands = settings.custom_commands;
        self.big_picture_enabled = settings.big_picture_enabled;
        self.git_panel_enabled = settings.git_panel_enabled;
        self.github_enabled = settings.github_enabled;
        self.projects_page_enabled = settings.projects_page_enabled;
        self.friends_enabled = settings.friends_enabled;
        self.model_router_enabled = settings.model_router_enabled;
        self.status_markers_enabled = settings.status_markers_enabled;
        self.automations_enabled = settings.automations_enabled;
        self.remote_hosts = settings.remote_hosts;
    }

    fn apply_app_state(&mut self, app_state: AppState) {
        self.analytics_id = app_state.analytics_id;
        self.selected_project = app_state.selected_project;
        self.selected_session = app_state.selected_session;
        self.unseen_completions = app_state.unseen_completions;
        self.last_provider = app_state.last_provider;
        self.last_auto_route = app_state.last_auto_route;
        self.last_runtime_mode = app_state.last_runtime_mode;
        self.last_sandboxed = app_state.last_sandboxed;
        self.last_model = app_state.last_model;
        self.last_reasoning_effort = app_state.last_reasoning_effort;
        self.last_service_tier = app_state.last_service_tier;
        self.last_context_window = app_state.last_context_window;
        self.remembered_model_traits = app_state.remembered_model_traits;
        self.recent_model_uses = app_state.recent_model_uses;
        self.project_workspaces = app_state.project_workspaces;
        self.sidebar_visible = app_state.sidebar_visible;
        self.right_panel_visible = app_state.right_panel_visible;
        self.git_panel_visible = app_state.git_panel_visible;
        self.sidebar_width = app_state.sidebar_width;
        self.sidebar_grouping = app_state.sidebar_grouping;
        self.sidebar_ordering = app_state.sidebar_ordering;
        self.right_panel_width = app_state.right_panel_width;
        self.markdown_preview = app_state.markdown_preview;
        self.window_state = app_state.window_state;
        self.navigation_back = app_state.navigation_back;
        self.navigation_forward = app_state.navigation_forward;
        self.transcript_scroll_positions = app_state.transcript_scroll_positions;
        self.sidebar_scroll = app_state.sidebar_scroll;
        self.projects_page = app_state.projects_page;
        self.settings_page = app_state.settings_page;
        self.right_panel_sessions = app_state.right_panel_sessions;
        self.fullscreen_surface = app_state.fullscreen_surface;
        self.saved_drafts = app_state.saved_drafts;
    }

    fn persistable_selected_session(&self) -> Option<Uuid> {
        self.selected_session.filter(|selected| {
            self.sessions
                .iter()
                .any(|session| session.id == *selected && session.has_started())
        })
    }

    fn ensure_runtime_session(&mut self) {
        if self
            .selected_session
            .is_some_and(|selected| self.sessions.iter().any(|session| session.id == selected))
        {
            return;
        }
        self.selected_session = None;
        let Some(project_id) = self
            .selected_project
            .filter(|selected| self.projects.iter().any(|project| project.id == *selected))
        else {
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
                    .is_none_or(waku_protocol::model::Checkpoint::totals_are_current)
            });
            let before = (
                session.turns.len(),
                session.last_reply_at,
                session.provider_cursor.is_some(),
            );
            session.migrate_legacy_state();
            session.backfill_last_reply_at();
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
        // Unread stamps for tasks deleted while the app was away would leave
        // the bell pointing at nothing; only live tasks can be unseen.
        let live: HashSet<Uuid> = self.sessions.iter().map(|session| session.id).collect();
        self.unseen_completions
            .retain(|session_id, _| live.contains(session_id));
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

fn configuration_directory() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(waku_protocol::identity::HOME_DIRECTORY_NAME)
}

fn default_app_settings_path() -> PathBuf {
    if cfg!(debug_assertions) {
        StateStore::default_path().with_file_name("app.json")
    } else {
        configuration_directory().join("app.json")
    }
}

fn default_app_state_path() -> PathBuf {
    StateStore::default_path().with_file_name("state.json")
}

/// Where the materialized shell scripts for [`CustomCommand`]s live. The
/// file name is a hash of the script text, so identical scripts share one
/// file no matter how many commands or invocations reference it.
pub fn custom_command_scripts_directory() -> PathBuf {
    if cfg!(debug_assertions) {
        StateStore::default_path().with_file_name("commands")
    } else {
        configuration_directory().join("commands")
    }
}

/// Where the bundled shell-integration scripts live once materialized —
/// one file per supported shell, rewritten when the bundled text changes.
pub fn shell_integration_scripts_directory() -> PathBuf {
    if cfg!(debug_assertions) {
        StateStore::default_path().with_file_name("shell-integration")
    } else {
        configuration_directory().join("shell-integration")
    }
}

fn read_app_state_file(path: &Path) -> Option<AppState> {
    let bytes = fs::read(path).ok()?;
    let app_state = serde_json::from_slice::<AppState>(&bytes).ok()?;
    (app_state.app_state_version == APP_STATE_VERSION).then_some(app_state)
}

/// Read the last persisted window frame ahead of the full state load. The
/// main window opens before the daemon connection and `StateStore` exist, and
/// `WindowOptions` needs its frame at `open_window` time.
pub fn load_window_state() -> Option<PersistedWindowState> {
    read_app_state_file(&default_app_state_path())?.window_state
}

fn default_legacy_settings_paths() -> Vec<PathBuf> {
    if cfg!(debug_assertions) {
        vec![StateStore::default_path().with_file_name("settings.json")]
    } else {
        vec![configuration_directory().join("settings.json")]
    }
}

fn read_app_settings_source(
    app_settings_path: &Path,
    legacy_settings_paths: &[PathBuf],
) -> io::Result<Option<(Vec<u8>, bool)>> {
    match fs::read(app_settings_path) {
        Ok(bytes) => return Ok(Some((bytes, true))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    for path in legacy_settings_paths {
        match fs::read(path) {
            Ok(bytes) => return Ok(Some((bytes, false))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// Load the app-owned launch settings before the managed daemon starts.
/// Missing settings are initialized immediately so its bearer token remains
/// stable between this process launch and the later UI state load.
pub fn load_or_create_app_settings() -> io::Result<AppSettings> {
    let path = default_app_settings_path();
    let source = read_app_settings_source(&path, &default_legacy_settings_paths())?;
    let loaded_from_primary = source.as_ref().is_some_and(|(_, primary)| *primary);
    let token_was_persisted = source
        .as_ref()
        .and_then(|(bytes, _)| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .and_then(|value| {
            value
                .get("daemon_exposure")
                .and_then(|daemon| daemon.get("token"))
                .and_then(serde_json::Value::as_str)
                .map(|token| !token.trim().is_empty())
        })
        .unwrap_or(false);
    let mut settings: AppSettings = source
        .map(|(bytes, _)| serde_json::from_slice::<AppSettings>(&bytes).map_err(to_io_error))
        .transpose()?
        .unwrap_or_default();
    let generated_token = settings.daemon_exposure.ensure_token();
    if !loaded_from_primary || !token_was_persisted || generated_token {
        write_json_atomically(&path, &settings)?;
    }
    Ok(settings)
}

/// Desktop state store: app files stay local, task data crosses RPC.
pub struct StateStore {
    path: PathBuf,
    app_state_path: PathBuf,
    app_settings_path: PathBuf,
    legacy_settings_paths: Vec<PathBuf>,
    remote_catalogs_path: PathBuf,
    daemons: DaemonMap,
    remote_default_cwd: Mutex<Option<PathBuf>>,
    /// A task snapshot may only be written after this client has successfully
    /// loaded the daemon's authoritative state. Falling back to an empty UI
    /// after a transient RPC failure must never turn the next ordinary save
    /// into a destructive replacement of the daemon database. Per remote
    /// host, `DaemonMap::catalog_loaded` gates the same way.
    task_state_loaded: AtomicBool,
}

impl StateStore {
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

    pub fn remote(daemon: DaemonSupervisor) -> Self {
        Self {
            app_state_path: default_app_state_path(),
            app_settings_path: default_app_settings_path(),
            legacy_settings_paths: default_legacy_settings_paths(),
            remote_catalogs_path: default_app_state_path().with_file_name("remote-catalogs.json"),
            path: Self::default_path(),
            daemons: DaemonMap::new(daemon),
            remote_default_cwd: Mutex::new(None),
            task_state_loaded: AtomicBool::new(false),
        }
    }

    /// Shared daemon registry — the app claims catalog rows and resolves
    /// per-session/per-project routing through the same map this store
    /// partitions writes by.
    pub fn daemons(&self) -> DaemonMap {
        self.daemons.clone()
    }

    /// Last catalog each remote host reported, for seeding the merged view
    /// while its supervisor is still connecting.
    pub fn load_remote_catalogs(&self) -> HashMap<Uuid, TaskCatalog> {
        fs::read(&self.remote_catalogs_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn write_remote_catalogs(&self, catalogs: &HashMap<Uuid, TaskCatalog>) -> io::Result<()> {
        write_json_atomically(&self.remote_catalogs_path, catalogs)
    }

    pub fn remote_catalogs_path(&self) -> PathBuf {
        self.remote_catalogs_path.clone()
    }

    pub fn session_message_search(
        &self,
        query: String,
        limit: usize,
        scope: SessionMessageSearchScope,
    ) -> impl FnOnce() -> io::Result<Vec<SessionMessageMatch>> + Send + 'static {
        let daemons = self.daemons.clone();
        move || {
            let mut matches = Vec::new();
            let mut first_error = None;
            for (_, daemon) in daemons.connected() {
                match daemon.client().request(
                    Uuid::nil(),
                    Uuid::nil(),
                    Command::SearchSessionMessages {
                        query: query.clone(),
                        limit,
                        scope,
                    },
                ) {
                    Ok(ResponsePayload::SessionMessageMatches { matches: found }) => {
                        matches.extend(found)
                    }
                    Ok(_) => {
                        return Err(io::Error::other(
                            "Goddard daemon returned an invalid message-search response",
                        ));
                    }
                    Err(error) if first_error.is_none() => {
                        first_error = Some(io::Error::other(error.to_string()));
                    }
                    Err(_) => {}
                }
            }
            matches.truncate(limit);
            match (matches.is_empty(), first_error) {
                (true, Some(error)) => Err(error),
                _ => Ok(matches),
            }
        }
    }

    pub fn provider_sessions(
        &self,
        provider: ProviderKind,
        limit: usize,
    ) -> impl FnOnce() -> io::Result<Vec<(DaemonKey, ProviderSessionSummary)>> + Send + 'static
    {
        let daemons = self.daemons.clone();
        move || {
            let mut sessions = Vec::new();
            let mut first_error = None;
            for (key, daemon) in daemons.connected() {
                match daemon.client().request(
                    Uuid::nil(),
                    Uuid::nil(),
                    Command::ListProviderSessions { provider, limit },
                ) {
                    Ok(ResponsePayload::ProviderSessions { sessions: reported }) => {
                        sessions.extend(reported.into_iter().map(|session| (key, session)))
                    }
                    Ok(_) => {
                        return Err(io::Error::other(
                            "Goddard daemon returned an invalid provider-session response",
                        ));
                    }
                    Err(error) if first_error.is_none() => {
                        first_error = Some(io::Error::other(error.to_string()));
                    }
                    Err(_) => {}
                }
            }
            sessions.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
            sessions.truncate(limit);
            match (sessions.is_empty(), first_error) {
                (true, Some(error)) => Err(error),
                _ => Ok(sessions),
            }
        }
    }

    /// Load one provider-native conversation. `key` routes to the daemon the
    /// session's project belongs to — provider history lives on the host that
    /// ran it.
    pub fn provider_session_history(
        &self,
        key: DaemonKey,
        cursor: ProviderResumeCursor,
        cwd: PathBuf,
    ) -> impl FnOnce() -> io::Result<ProviderSessionHistory> + Send + 'static {
        let daemon = self.daemons.supervisor(key);
        move || match daemon {
            Some(daemon) => match daemon
                .client()
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    Command::LoadProviderSession { cursor, cwd },
                )
                .map_err(to_io_error)?
            {
                ResponsePayload::ProviderSessionHistory { history } => Ok(history),
                _ => Err(io::Error::other(
                    "Goddard daemon returned an invalid provider-session history response",
                )),
            },
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the session's daemon is not connected",
            )),
        }
    }

    pub fn load_or_fresh(&self, cwd: PathBuf) -> PersistedState {
        let mut state = match self.load() {
            Ok(state) => {
                self.task_state_loaded.store(true, Ordering::Release);
                state
            }
            Err(_) if cwd.parent().is_none() => PersistedState::empty(),
            Err(_) => PersistedState::fresh(cwd),
        };
        if state.projects.is_empty()
            && let Some(cwd) = self.remote_default_cwd.lock().clone()
            && cwd.parent().is_some()
        {
            let project = Project::from_path(cwd);
            let session = state.new_session(project.id, state.last_provider);
            state.selected_project = Some(project.id);
            state.selected_session = Some(session.id);
            state.projects.push(project);
            state.push_session(session);
        }
        state.ensure_runtime_session();
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

    pub fn load(&self) -> io::Result<PersistedState> {
        let (projects, mut sessions, default_cwd) = match self
            .daemons
            .local()
            .client()
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .map_err(to_io_error)?
        {
            ResponsePayload::TaskState {
                projects,
                sessions,
                default_cwd,
                projectless_root,
            } => {
                waku_protocol::projectless::set_workspace_root(projectless_root);
                (projects, sessions, default_cwd)
            }
            _ => {
                return Err(io::Error::other(
                    "Goddard daemon returned an invalid task-state response",
                ));
            }
        };
        restore_task_state_skeletons(&mut sessions);
        *self.remote_default_cwd.lock() = Some(default_cwd);
        let mut state = PersistedState::empty();
        state.projects = projects;
        state.sessions = sessions;
        let app_settings_missing = !self.app_settings_path.is_file();
        if let Some(settings) = self.read_app_settings()? {
            state.apply_app_settings(settings);
        }
        let app_state = read_app_state_file(&self.app_state_path);
        let app_state_missing = app_state.is_none();
        if let Some(app_state) = app_state {
            state.apply_app_state(app_state);
        }
        state.migrate_loaded();
        if app_settings_missing {
            let _ = self.write_app_settings(&state.app_settings());
        }
        if app_state_missing {
            let _ = self.write_app_state(&state.app_state());
        }
        Ok(state)
    }

    pub fn hydrate(&self, session: &mut AgentSession) -> io::Result<()> {
        if session.detail_loaded {
            return Ok(());
        }
        let Some(daemon) = self.daemons.daemon_for_session(session.id) else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the session's daemon is not connected",
            ));
        };
        match hydrate_session(&daemon, session.id)? {
            Some(stored) => {
                *session = stored;
                Ok(())
            }
            None => {
                session.detail_loaded = true;
                Ok(())
            }
        }
    }

    /// Partition the merged catalog back to its owning daemons: each receives
    /// only its own projects, live session ids, and dirty rows, so one
    /// daemon's data never lands in another's database. A daemon that rejects
    /// or is unreachable keeps its dirty ids queued for the next save while
    /// the rest still commit.
    pub fn save(&self, state: &mut PersistedState) -> io::Result<()> {
        self.write_app_settings(&state.app_settings())?;
        self.write_app_state(&state.app_state())?;
        if !self.task_state_loaded.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "task state was not loaded; refusing to overwrite daemon data",
            ));
        }
        let dirty_ids = state.dirty_sessions.clone();
        let mut saved_ids = HashSet::new();
        let mut first_error = None;
        for (key, daemon) in self.daemons.connected() {
            if !self.daemons.catalog_loaded(key) {
                continue;
            }
            let owned = |id: &Uuid| match key {
                DaemonKey::Local => !self.daemons.is_remote_session(*id),
                DaemonKey::Remote(_) => self.daemons.session_owner(*id) == key,
            };
            let projects = state
                .projects
                .iter()
                .filter(|project| match key {
                    DaemonKey::Local => !self.daemons.is_remote_project(project.id),
                    DaemonKey::Remote(_) => self.daemons.project_owner(project.id) == key,
                })
                .cloned()
                .collect();
            let live_session_ids = state
                .sessions
                .iter()
                .filter(|session| owned(&session.id))
                .map(|session| session.id)
                .collect();
            // Drafts stay local until their first prompt: a session that has
            // not started owns no daemon row, and shipping one would
            // catalogue it as a phantom "New task" skeleton in every client's
            // next load.
            let sessions: Vec<AgentSession> = state
                .sessions
                .iter()
                .filter(|session| {
                    dirty_ids.contains(&session.id) && session.has_started() && owned(&session.id)
                })
                .cloned()
                .collect();
            let dirty_for_daemon: HashSet<Uuid> =
                sessions.iter().map(|session| session.id).collect();
            match daemon.client().notify(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects,
                    live_session_ids,
                    sessions,
                },
            ) {
                Ok(()) => saved_ids.extend(dirty_for_daemon),
                Err(error) if first_error.is_none() => {
                    first_error = Some(to_io_error(error));
                }
                Err(_) => {}
            }
        }
        // Skipped drafts remain dirty so the save after their first prompt
        // still publishes them even if that path forgets to re-mark them.
        let remaining_ids = state
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<HashSet<_>>();
        state
            .dirty_sessions
            .retain(|id| remaining_ids.contains(id) && !saved_ids.contains(id));
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn remove_session(&self, session_id: Uuid) -> io::Result<()> {
        let result = match self.daemons.daemon_for_session(session_id) {
            Some(daemon) => daemon
                .client()
                .notify(session_id, Uuid::nil(), Command::RemoveSession)
                .map_err(to_io_error),
            // The owning daemon is gone or unreachable; the row is already
            // unclaimed locally and there is nobody left to tell.
            None => Ok(()),
        };
        self.daemons.drop_session(session_id);
        result
    }

    pub fn blob_sweep(&self) -> impl FnOnce() + Send + 'static {
        let daemons = self.daemons.clone();
        move || {
            for (_, daemon) in daemons.connected() {
                let _ = daemon
                    .client()
                    .request(Uuid::nil(), Uuid::nil(), Command::SweepBlobs);
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_app_settings(&self) -> io::Result<Option<AppSettings>> {
        let source =
            read_app_settings_source(&self.app_settings_path, &self.legacy_settings_paths)?;
        source
            .map(|(bytes, _)| serde_json::from_slice(&bytes).map_err(to_io_error))
            .transpose()
    }

    fn write_app_settings(&self, settings: &AppSettings) -> io::Result<()> {
        write_json_atomically(&self.app_settings_path, settings)
    }

    fn write_app_state(&self, state: &AppState) -> io::Result<()> {
        write_json_atomically(&self.app_state_path, state)
    }
}

/// Fetches one daemon-owned session. Remote image materialization is kept
/// separate so presentation code can show the transcript before large blobs
/// finish crossing the daemon boundary.
pub fn hydrate_session(
    daemon: &DaemonSupervisor,
    session_id: Uuid,
) -> io::Result<Option<AgentSession>> {
    match daemon
        .client()
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::HydrateSession { session_id },
        )
        .map_err(to_io_error)?
    {
        ResponsePayload::Session { session } => Ok(session),
        _ => Err(io::Error::other(
            "Goddard daemon returned an invalid session-hydration response",
        )),
    }
}

fn write_json_atomically(path: &Path, value: &impl Serialize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(value).map_err(to_io_error)?;
    let temporary = path.with_extension("json.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary)?;
    file.write_all(&data)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn restore_task_state_skeletons(sessions: &mut [AgentSession]) {
    for session in sessions {
        session.detail_loaded = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_transparency_defaults_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.sidebar_transparency, cfg!(target_os = "macos"));
        let mut state = PersistedState::empty();
        assert_eq!(state.sidebar_transparency, cfg!(target_os = "macos"));
        state.sidebar_transparency = false;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["sidebar_transparency"], false);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("sidebar_transparency")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(!restored.sidebar_transparency);
    }

    #[test]
    fn sidebar_transparency_amount_defaults_persists_and_clamps() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            defaults.sidebar_transparency_amount,
            DEFAULT_SIDEBAR_TRANSPARENCY_AMOUNT
        );
        let mut state = PersistedState::empty();
        state.sidebar_transparency_amount = 0.4;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(
            settings["sidebar_transparency_amount"].as_f64().unwrap() as f32,
            0.4
        );
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("sidebar_transparency_amount")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert_eq!(restored.sidebar_transparency_amount, 0.4);
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(AppSettings {
            sidebar_transparency_amount: 9.0,
            ..Default::default()
        });
        assert_eq!(
            restored.sidebar_transparency_amount,
            MAX_SIDEBAR_TRANSPARENCY
        );
    }

    #[test]
    fn border_intensity_defaults_persists_and_clamps() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.border_intensity, DEFAULT_BORDER_INTENSITY);
        let mut state = PersistedState::empty();
        assert_eq!(state.border_intensity, DEFAULT_BORDER_INTENSITY);
        state.border_intensity = 0.25;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(
            settings["border_intensity"].as_f64().unwrap() as f32,
            0.25
        );
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("border_intensity")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert_eq!(restored.border_intensity, 0.25);
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(AppSettings {
            border_intensity: 9.0,
            ..Default::default()
        });
        assert_eq!(restored.border_intensity, MAX_BORDER_INTENSITY);
    }

    #[test]
    fn math_rendering_defaults_on_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(defaults.render_math);
        let mut state = PersistedState::empty();
        assert!(state.render_math);
        state.render_math = false;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["render_math"], false);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("render_math")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(!restored.render_math);
    }

    #[test]
    fn response_token_speed_defaults_off_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.show_response_token_speed);
        let mut state = PersistedState::empty();
        assert!(!state.show_response_token_speed);
        state.show_response_token_speed = true;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["show_response_token_speed"], true);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("show_response_token_speed")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(restored.show_response_token_speed);
    }

    #[test]
    fn merge_sync_defaults_off_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.sync_with_merge);
        let mut state = PersistedState::empty();
        assert!(!state.sync_with_merge);
        state.sync_with_merge = true;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["sync_with_merge"], true);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("sync_with_merge")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(restored.sync_with_merge);
    }

    #[test]
    fn auto_resolve_land_conflicts_defaults_off_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.auto_resolve_land_conflicts);
        let mut state = PersistedState::empty();
        assert!(!state.auto_resolve_land_conflicts);
        state.auto_resolve_land_conflicts = true;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["auto_resolve_land_conflicts"], true);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("auto_resolve_land_conflicts")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(restored.auto_resolve_land_conflicts);
    }

    #[test]
    fn new_worktree_default_branch_defaults_off_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.new_worktree_default_branch);
        let mut state = PersistedState::empty();
        assert!(!state.new_worktree_default_branch);
        state.new_worktree_default_branch = true;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["new_worktree_default_branch"], true);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("new_worktree_default_branch")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(restored.new_worktree_default_branch);
    }

    #[test]
    fn new_worktree_sync_default_branch_defaults_off_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.new_worktree_sync_default_branch);
        let mut state = PersistedState::empty();
        assert!(!state.new_worktree_sync_default_branch);
        assert!(state.new_worktree_sync_branches.is_empty());
        state.new_worktree_sync_default_branch = true;
        state.new_worktree_sync_branches = vec!["develop".to_owned()];
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["new_worktree_sync_default_branch"], true);
        assert_eq!(
            settings["new_worktree_sync_branches"],
            serde_json::json!(["develop"])
        );
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("new_worktree_sync_default_branch")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(restored.new_worktree_sync_default_branch);
        assert_eq!(restored.new_worktree_sync_branches, ["develop"]);
    }

    #[test]
    fn three_finger_swipe_navigation_defaults_off_and_persists_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.three_finger_swipe_navigation);
        let mut state = PersistedState::empty();
        assert!(!state.three_finger_swipe_navigation);
        state.three_finger_swipe_navigation = true;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["three_finger_swipe_navigation"], true);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("three_finger_swipe_navigation")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(restored.three_finger_swipe_navigation);
    }

    #[test]
    fn sidebar_shortcut_tags_default_on_and_persist_as_an_app_preference() {
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(defaults.sidebar_shortcut_tags);
        let mut state = PersistedState::empty();
        assert!(state.sidebar_shortcut_tags);
        state.sidebar_shortcut_tags = false;
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["sidebar_shortcut_tags"], false);
        assert!(
            serde_json::to_value(state.app_state())
                .unwrap()
                .get("sidebar_shortcut_tags")
                .is_none()
        );
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert!(!restored.sidebar_shortcut_tags);
    }

    #[test]
    fn terminal_font_size_follows_code_size_until_it_differs() {
        // Settings files written before the terminal size existed carry no
        // value, and the resolved size is the code size.
        let defaults: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.terminal_font_size, None);
        let state = PersistedState::empty();
        assert_eq!(state.terminal_font_size, None);
        assert_eq!(state.terminal_font_size(), DEFAULT_CODE_FONT_SIZE);

        // A distinct size round-trips through app.json as an override.
        let mut state = PersistedState::empty();
        state.terminal_font_size = Some(16.0);
        let settings = serde_json::to_value(state.app_settings()).unwrap();
        assert_eq!(settings["terminal_font_size"], 16.0);
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert_eq!(restored.terminal_font_size(), 16.0);

        // A stored value equal to the code size collapses back to the linked
        // `None`, so it keeps following later code-size changes.
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(
            serde_json::from_str(r#"{"code_font_size": 15.0, "terminal_font_size": 15.0}"#)
                .unwrap(),
        );
        assert_eq!(restored.terminal_font_size, None);
        restored.code_font_size = 18.0;
        assert_eq!(restored.terminal_font_size(), 18.0);
    }

    #[test]
    fn custom_commands_round_trip_through_app_settings() {
        let mut state = PersistedState::empty();
        let mut command = CustomCommand::new("git status".to_owned());
        command.name = Some("Status".to_owned());
        command.icon = CustomCommandIcon::Zap;
        command.shell = Some("/bin/zsh".to_owned());
        command.close_on_success = true;
        state.custom_commands = vec![command.clone()];

        let settings = serde_json::to_value(state.app_settings()).unwrap();
        let mut restored = PersistedState::empty();
        restored.apply_app_settings(serde_json::from_value(settings).unwrap());
        assert_eq!(restored.custom_commands, vec![command]);

        // Settings files written before commands existed carry no list.
        let legacy: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(legacy.custom_commands.is_empty());

        // Commands written before icons existed default to the terminal.
        let legacy: CustomCommand = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "script": "echo hi"
        }))
        .unwrap();
        assert_eq!(legacy.icon, CustomCommandIcon::Terminal);
    }

    #[test]
    fn custom_command_display_name_falls_back_to_the_script() {
        let mut command = CustomCommand::new("make test".to_owned());
        assert_eq!(command.display_name(), "make test");
        command.name = Some("   ".to_owned());
        assert_eq!(command.display_name(), "make test");
        command.name = Some("Tests".to_owned());
        assert_eq!(command.display_name(), "Tests");
    }

    #[test]
    fn desktop_settings_paths_are_build_specific() {
        let app_settings_path = default_app_settings_path();
        let legacy_settings_paths = default_legacy_settings_paths();

        #[cfg(debug_assertions)]
        {
            let state_path = StateStore::default_path();
            assert_eq!(app_settings_path, state_path.with_file_name("app.json"));
            assert_eq!(
                legacy_settings_paths,
                [state_path.with_file_name("settings.json")]
            );
        }

        #[cfg(not(debug_assertions))]
        {
            assert_eq!(
                app_settings_path,
                configuration_directory().join("app.json")
            );
            assert_eq!(
                legacy_settings_paths,
                [configuration_directory().join("settings.json")]
            );
        }
    }

    #[test]
    fn legacy_app_state_defaults_sidebar_presentation() {
        let state: AppState = serde_json::from_str(r#"{"app_state_version":1}"#).unwrap();

        assert_eq!(state.sidebar_grouping, SidebarGrouping::Date);
        assert_eq!(state.sidebar_ordering, SidebarOrdering::LastUpdated);
        assert_eq!(state.last_runtime_mode, RuntimeMode::FullAccess);

        let state: AppState = serde_json::from_str(
            r#"{"app_state_version":1,"sidebar_grouping":"updated","sidebar_ordering":"oldest"}"#,
        )
        .unwrap();
        assert_eq!(state.sidebar_grouping, SidebarGrouping::Date);
        assert_eq!(state.sidebar_ordering, SidebarOrdering::LastCreated);
    }

    #[test]
    fn new_tasks_inherit_the_remembered_access_mode() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        state.last_runtime_mode = RuntimeMode::Ask;

        let session = state.new_session(state.projects[0].id, ProviderKind::OpenCode);

        assert_eq!(session.runtime_mode, RuntimeMode::Ask);
        assert_eq!(state.app_state().last_runtime_mode, RuntimeMode::Ask);
    }

    #[test]
    fn new_tasks_reopen_the_workspace_last_chosen_for_their_project() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let project_id = state.projects[0].id;
        let other = Project::from_path(PathBuf::from("/tmp/other"));
        let other_id = other.id;
        state.projects.push(other);

        state.remember_workspace(
            project_id,
            &SessionWorkspace::NewWorktree {
                base_branch: Some("develop".to_owned()),
            },
        );

        let session = state.new_session(project_id, ProviderKind::Codex);
        assert_eq!(
            session.workspace,
            SessionWorkspace::NewWorktree {
                base_branch: Some("develop".to_owned())
            }
        );
        assert_eq!(
            state.remembered_base_branch(project_id),
            Some("develop".to_owned())
        );

        // A project without a remembered choice keeps the default.
        let session = state.new_session(other_id, ProviderKind::Codex);
        assert_eq!(session.workspace, SessionWorkspace::Local);
        assert_eq!(state.remembered_base_branch(other_id), None);
    }

    #[test]
    fn new_worktree_default_branch_skips_the_remembered_base() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let project_id = state.projects[0].id;
        state.remember_workspace(
            project_id,
            &SessionWorkspace::NewWorktree {
                base_branch: Some("develop".to_owned()),
            },
        );

        state.new_worktree_default_branch = true;

        // `None` resolves the repository's default branch on the daemon.
        let session = state.new_session(project_id, ProviderKind::Codex);
        assert_eq!(
            session.workspace,
            SessionWorkspace::NewWorktree { base_branch: None }
        );
        assert_eq!(state.remembered_base_branch(project_id), None);
    }

    #[test]
    fn switching_back_to_local_overwrites_the_remembered_worktree() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let project_id = state.projects[0].id;

        state.remember_workspace(
            project_id,
            &SessionWorkspace::NewWorktree { base_branch: None },
        );
        state.remember_workspace(project_id, &SessionWorkspace::Local);

        let session = state.new_session(project_id, ProviderKind::Codex);
        assert_eq!(session.workspace, SessionWorkspace::Local);
        assert_eq!(state.remembered_base_branch(project_id), None);
    }

    #[test]
    fn materialized_worktrees_and_projectless_projects_are_not_remembered() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let project_id = state.projects[0].id;

        // A materialized worktree is the result of a choice, not a choice.
        state.remember_workspace(
            project_id,
            &SessionWorkspace::Worktree {
                path: PathBuf::from("/tmp/worktree"),
                name: "worktree".to_owned(),
                branch: Some("feature".to_owned()),
                base_branch: None,
            },
        );
        let session = state.new_session(project_id, ProviderKind::Codex);
        assert_eq!(session.workspace, SessionWorkspace::Local);

        // A remembered worktree must not reach a projectless task: it has
        // no repository to fork from.
        let projectless_root =
            waku_protocol::projectless::workspace_root().expect("workspace root is initialized");
        let projectless = Project::from_path(projectless_root.join("2026-09-13/task"));
        let projectless_id = projectless.id;
        state.projects.push(projectless);
        state.remember_workspace(
            projectless_id,
            &SessionWorkspace::NewWorktree { base_branch: None },
        );
        let session = state.new_session(projectless_id, ProviderKind::Codex);
        assert_eq!(session.workspace, SessionWorkspace::Local);
    }

    #[test]
    fn unseen_completions_survive_an_app_state_round_trip() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = state.sessions[0].id;
        state.unseen_completions.insert(session_id, 42);
        state.unseen_completions.insert(Uuid::new_v4(), 7);

        let app_state = serde_json::to_value(state.app_state()).unwrap();
        let mut restored = PersistedState::empty();
        restored.sessions.clone_from(&state.sessions);
        restored.apply_app_state(serde_json::from_value(app_state).unwrap());
        restored.migrate_loaded();

        // The live task keeps its stamp; the stamp for a task deleted while
        // the app was away is pruned on load.
        assert_eq!(restored.unseen_completions.len(), 1);
        assert_eq!(restored.unseen_completions[&session_id], 42);
    }

    #[test]
    fn project_workspaces_survive_an_app_state_round_trip() {
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let project_id = state.projects[0].id;
        state.remember_workspace(
            project_id,
            &SessionWorkspace::NewWorktree {
                base_branch: Some("main".to_owned()),
            },
        );

        let app_state = serde_json::to_value(state.app_state()).unwrap();
        let mut restored = PersistedState::empty();
        restored.projects.clone_from(&state.projects);
        restored.apply_app_state(serde_json::from_value(app_state).unwrap());

        let session = restored.new_session(project_id, ProviderKind::Codex);
        assert_eq!(
            session.workspace,
            SessionWorkspace::NewWorktree {
                base_branch: Some("main".to_owned())
            }
        );
    }

    #[test]
    fn daemon_task_state_becomes_list_only_after_crossing_the_client_boundary() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.detail_loaded = false;
        assert!(
            session.has_started(),
            "a stored skeleton is visible history"
        );

        let encoded = serde_json::to_vec(&ResponsePayload::TaskState {
            projects: Vec::new(),
            sessions: vec![session],
            default_cwd: PathBuf::from("/daemon/project"),
            projectless_root: None,
        })
        .unwrap();
        let ResponsePayload::TaskState { mut sessions, .. } =
            serde_json::from_slice(&encoded).unwrap()
        else {
            panic!("task state response changed shape");
        };

        assert!(
            !sessions[0].detail_loaded && sessions[0].has_started(),
            "the skeleton marker must survive the wire, or a saved-back \
             projection looks like an empty loaded session"
        );
        restore_task_state_skeletons(&mut sessions);
        assert!(!sessions[0].detail_loaded);
        assert!(sessions[0].has_started());
    }

    #[test]
    fn auto_route_pick_seeds_the_next_draft() {
        let mut state = PersistedState::empty();
        state.model_router_enabled = true;
        state.last_auto_route = true;

        let app_state = serde_json::to_value(state.app_state()).unwrap();
        let mut restored = PersistedState::empty();
        restored.model_router_enabled = true;
        restored.apply_app_state(serde_json::from_value(app_state).unwrap());
        assert!(restored.last_auto_route);

        // The routed provider/model stay the draft's carryover hint; the
        // Auto flag is what the picker selection restores.
        let session = restored.new_session(Uuid::new_v4(), ProviderKind::Claude);
        assert!(session.auto_route);
        assert_eq!(session.provider, ProviderKind::Claude);

        // Without the experiment the remembered flag cannot arm a draft.
        restored.model_router_enabled = false;
        let session = restored.new_session(Uuid::new_v4(), ProviderKind::Claude);
        assert!(!session.auto_route);
    }
}

/// Reads one daemon-owned binary payload. Clients decide how to present the
/// bytes; this transport layer deliberately creates no client-side files.
pub fn read_remote_reference(
    reference: &str,
    daemon_path: Option<&Path>,
    daemon: &DaemonSupervisor,
) -> Option<Vec<u8>> {
    let command = if waku_protocol::blob::is_reference(reference) {
        Command::ReadBlob {
            reference: reference.to_owned(),
        }
    } else if reference.starts_with(waku_protocol::attachments::ATTACHMENT_SCHEME) {
        let path = daemon_path?;
        Command::ReadAttachment {
            reference: reference.to_owned(),
            path: path.to_owned(),
        }
    } else {
        return None;
    };
    let Ok(ResponsePayload::BlobData { bytes }) =
        daemon.client().request(Uuid::nil(), Uuid::nil(), command)
    else {
        return None;
    };
    Some(bytes)
}

fn normalize_computer_app_grants(grants: &mut Vec<ComputerAppGrant>) {
    let mut seen_bundle_ids = HashSet::new();
    grants.retain(|grant| {
        !grant.bundle_id.trim().is_empty() && seen_bundle_ids.insert(grant.bundle_id.clone())
    });
}

fn to_io_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
