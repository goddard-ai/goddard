use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use crossbeam_channel::{Receiver, Sender, unbounded};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, ClickEvent, ClipboardEntry, ClipboardItem,
    Context, Div, Entity, EntityId, ExternalPaths, FocusHandle, Focusable, FontWeight,
    HitboxBehavior, Hsla, IntoElement, KeyDownEvent, ListAlignment, ListOffset, ListState,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    ObjectFit, PathPromptOptions, Pixels, Render, ScrollHandle, SharedString, Stateful,
    StyleRefinement,
    TextRun, WeakEntity, Window, WindowBounds, canvas, deferred, div, ease_out_quint, fill, font,
    img, linear_color_stop, linear_gradient, list, point, prelude::*, pulsating_between, px, rgb,
};
use uuid::Uuid;

use crate::checkpoint;
use crate::composer_complete::{ComposerWorkItem, FileEntry, SlashCommand};
use crate::computer_use::{
    ComputerPermissions, ComputerTarget, ComputerUsePhase, ComputerUseState,
    PendingComputerApproval,
};
use crate::driver::{self, DriverHandle, DriverStartOptions, SessionOptions};
use crate::git_branch::BranchSnapshot;
use crate::input::{
    ComposerAttachmentPaste, ComposerEvent, ComposerInput, ComposerTextPaste, InputEvent, TextInput,
    Undo,
};
use crate::md;
use crate::model::{
    ActivityItem, ActivityKind, AgentSession, BackgroundWorkEvent, BackgroundWorkItem,
    BackgroundWorkKey, BackgroundWorkKind, BackgroundWorkStatus, Checkpoint, CheckpointStatus,
    ContextUsage, DriverEvent, FavoriteModel, Message, MessageAttachment, MessageRole,
    PendingPermission, Project, ProviderKind, ProviderModel, ProviderProbe, ProviderResumeCursor,
    ProviderSessionHistory, ProviderSessionSummary, QueuedMessage, ReasoningBlock, RuntimeMode,
    SessionStatus, SessionWorkspace, TranscriptBlock, TranscriptNotice, TurnStatus, UserInputAnswer,
    UserInputQuestion, compact_path, unix_time, unix_time_millis,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::md::render::{
    Ctx as MarkdownCtx, MarkdownView, Metrics as MarkdownMetrics, Palette as MarkdownPalette,
    TranscriptSelection,
};
use crate::md::selection::{Annotations, TranscriptAnnotation};
use crate::ui::menu::{
    ConfirmEntry, ContextMenuHandle, DismissMenu, FloatingSurface, MenuAlign, MenuItem,
    SelectNextEntry, SelectNextTab, SelectPreviousEntry, SelectPreviousTab, context_menu,
    dropdown_menu, dropdown_menu_on_hover, popover,
};
use crate::ui::scrollbar::{self, ScrollbarState};
use crate::ui::slider::{self, SliderState};
use crate::ui::tooltip::Tooltip;

use crate::browser::BrowserView;
use crate::persistence::{
    ArchiveNavigation, CompletionSound, ComposerDraftStore, ComposerDrafts, CustomCommand,
    CustomCommandIcon,
    DEFAULT_GIT_PANEL_TOP_HEIGHT, DEFAULT_RIGHT_PANEL_WIDTH, DEFAULT_SIDEBAR_WIDTH,
    PersistedDiffSource,
    PersistedFullscreenSurface, PersistedListOffset, PersistedNavigationLocation,
    PersistedRightPanelState, PersistedRightPanelSurface, PersistedSettingsPage, PersistedState,
    PersistedWindowState, RecentModelUse, SidebarGrouping, SidebarOrdering, StateStore,
};
use crate::query::{Query, QueryCache};
use crate::review_diff::{Snapshot as ReviewDiffSnapshot, Source as ReviewDiffSource};
use crate::terminal::{TerminalLaunch, TerminalView, TerminalViewEvent};
use crate::theme::{Theme, ThemeMode, hairline, sp};
use crate::ui::text_field::TextField;
use crate::ui::{
    MenuChip, ProjectNameSelector, activity_noun, activity_row_icon, column_resize, contain_scroll,
    file_icon, goddard_logo, icon, icon_button, motion, progress_ring, provider_color,
    provider_mark, rem_scale, status_color, thinking, toggle_switch,
};
use crate::{
    AddToChat, ArchiveSession, CancelProjectSwitch, CancelTaskSwitch, CancelTurn, CloseFind,
    CloseWindow, ConfirmProjectSwitch, ConfirmTaskSwitch, CopySelection, CopyWorkingDirectory,
    CycleReasoningEffort, DismissDraftsLayer, DismissInbox, DismissProjectsLayer,
    EffortCycleDirection, ExitPanelFullscreen, FindNext, FindPrevious, FocusComposer,
    FocusProjectsFilter, FocusTerminal, GoToNextTurn, GoToNextUnreadCompletion, GoToPreviousTurn,
    MarkSessionUnread, MarkUnreadAndGoToNextIdle, NavigateBack, NavigateForward, NewProject,
    NewSession, NewTaskIn, NewTerminal, OpenFind, OpenFindReplace, OpenGoToLine, OpenResumePicker,
    OpenSettings, ReplaceAllMatches, RunProjectScript, SaveFile, SelectAllProjectsRows,
    SelectFavoriteModel, SelectFirstProject, SelectFirstTask, SelectLastProject, SelectLastTask,
    SelectProjectsTab, SelectSidebarSession, SwitchProjectBackward, SwitchProjectForward,
    SwitchTaskBackward, SwitchTaskForward, SyncBranch, ToggleBigPicture, ToggleBranchPicker,
    ToggleCommandPalette, ToggleEnvironment, ToggleFileFinder, ToggleFindCaseSensitive,
    ToggleFindRegex, ToggleFindWholeWord, ToggleFpsCounter, ToggleGitPanel, ToggleInboxPage,
    ToggleModelPicker, ToggleProjectsPage, ToggleRightPanel,
    ToggleRuntimeModePicker, ToggleSessionPin, ToggleSidebar, ToggleTerminals, ToggleUsagePanel,
    ToggleWorkspace,
};

#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_CLEARANCE: f32 = 86.0;
#[cfg(not(target_os = "macos"))]
const TRAFFIC_LIGHT_CLEARANCE: f32 = 8.0;
const CONTENT_MAX_WIDTH: f32 = 720.0;
/// The composer assembly — queued messages, input card, and workspace footer —
/// overhangs the scrollable transcript column by this much on each side.
const COMPOSER_OVERHANG: f32 = 12.0;
/// Menu-registry id of the composer's model picker, shared by its render site
/// and the primary-modifier `/` toggle action.
const MODEL_PICKER_MENU_ID: &str = "provider-model-picker";
const BRANCH_PICKER_MENU_ID: &str = "workspace-branch-picker";
const RUNTIME_MODE_MENU_ID: &str = "runtime-mode";
const BRANCH_PICKER_ROW_HEIGHT: f32 = 26.0;
const SIDEBAR_MIN_WIDTH: f32 = 180.0;
const SIDEBAR_MAX_WIDTH: f32 = 420.0;
/// The left-edge hover strip that reveals the peek sidebar while the docked
/// one is closed.
const SIDEBAR_PEEK_STRIP: f32 = 5.0;
/// The peek overlay sits a touch wider than the docked sidebar.
const SIDEBAR_PEEK_WIDTH_FACTOR: f32 = 1.15;
/// The peek nudge: the overlay appears and vanishes without fading, only
/// drifting these few px into place on reveal and back out before unmount.
const SIDEBAR_PEEK_NUDGE: f32 = 8.0;
const SIDEBAR_PEEK_SLIDE: Duration = Duration::from_millis(150);
const UPDATER_BUTTON_COLLAPSED_WIDTH: f32 = 20.0;
const UPDATER_BUTTON_EXPANDED_WIDTH: f32 = 58.0;
const RIGHT_PANEL_MIN_WIDTH: f32 = 280.0;
const RIGHT_PANEL_MAX_WIDTH: f32 = 1000.0;
/// The Git panel column's fixed header.
const GIT_PANEL_HEADER_HEIGHT: f32 = 44.0;
/// Drag bounds for the Git panel's top region — the commit box or an open
/// commit's file tree — split from the commit log.
const GIT_PANEL_TOP_MIN_HEIGHT: f32 = 140.0;
const GIT_PANEL_TOP_MAX_HEIGHT: f32 = 1200.0;
/// The commit log keeps at least this much room under the top region.
const GIT_PANEL_COMMITS_MIN_HEIGHT: f32 = 140.0;
const DEFAULT_FILE_TREE_WIDTH: f32 = 184.0;
const FILE_TREE_MIN_WIDTH: f32 = 140.0;
const FILE_TREE_MAX_WIDTH: f32 = 360.0;
const FILE_EDITOR_MIN_WIDTH: f32 = 140.0;
const FILE_EDITOR_INITIAL_WIDTH: f32 = 500.0;
const REVIEW_INITIAL_WIDTH: f32 = 820.0;
const MAIN_PANEL_MIN_WIDTH: f32 = 360.0;
const FOLLOWUP_TURN_TOP_GAP: f32 = 48.0;
const NAVIGATION_RAIL_WIDTH: f32 = 44.0;
const NAVIGATION_RAIL_LEFT: f32 = 16.0;
const NAVIGATION_RAIL_CONTENT_GAP: f32 = 16.0;
const NAVIGATION_RAIL_VIEWPORT_HEIGHT_RATIO: f32 = 0.80;
const NAVIGATION_RAIL_TICK_WIDTH: f32 = 32.0;
const NAVIGATION_RAIL_TICK_HEIGHT: f32 = 2.0;
const NAVIGATION_RAIL_TICK_GAP: f32 = 10.0;
const NAVIGATION_RAIL_INACTIVE_OPACITY: f32 = 0.45;
const NAVIGATION_RAIL_TURN_HEIGHT: f32 = NAVIGATION_RAIL_TICK_HEIGHT + NAVIGATION_RAIL_TICK_GAP;
const NAVIGATION_RAIL_FADE_HEIGHT: f32 = 20.0;
const NAVIGATION_RAIL_ANIMATION_DURATION: Duration = Duration::from_millis(300);
const ESCAPE_STOP_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(3);
/// Presentation pacing only. The app sleeps until a provider or background
/// result wakes it, then uses this cadence while streamed chunks remain.
/// Chunks queue for a full interval and fold into one drain → one notify →
/// one remeasure, so the per-commit parse/flatten/highlight work runs at ~8 Hz
/// regardless of the provider's chunk rate, and the veil dissolve spans the
/// gap so streamed text still reads as continuous.
const STREAM_FRAME_INTERVAL: Duration = Duration::from_millis(120);
/// How long a session may sit untouched before its provider process is released.
/// Codex and Pi stay resident between turns, so without this an afternoon of
/// abandoned tasks is an afternoon of idle agent processes.
const IDLE_SESSION_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const IDLE_SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const BACKGROUND_WORK_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const BACKGROUND_WORK_TICK_INTERVAL: Duration = Duration::from_secs(1);
const PLAN_USAGE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);
const STREAM_SAVE_INTERVAL: Duration = Duration::from_secs(1);
/// Zed keeps status toasts on screen for ten seconds, pausing the countdown
/// while the pointer is over the toast so a long message remains readable.
const DEFAULT_TOAST_DURATION: Duration = Duration::from_secs(5);
/// Residual lifetime of an operation's spinner toast. While its owner is in
/// flight — a command run mirroring its output tail, a `/land` waiting on
/// the daemon — the dismiss clock never arms, so this only applies when the
/// owner disappears without ever reporting a result.
const PROGRESS_TOAST_DURATION: Duration = Duration::from_millis(2_500);
/// How much of a running command's screen its toast mirrors.
const COMMAND_RUN_TAIL_LINES: usize = 3;
/// Tail publishes ride the stream-commit cadence — a PTY burst dirties the
/// terminal every 24ms poll, and each publish costs a full-window frame.
const COMMAND_RUN_TAIL_INTERVAL: Duration = Duration::from_millis(125);
const MINIMUM_TOAST_RESUME_DURATION: Duration = Duration::from_millis(800);
const TOAST_ANIMATION_DURATION: Duration = Duration::from_millis(150);
const TASK_NOTIFICATION_TAG_PREFIX: &str = "waku-task:";

pub(crate) fn task_notification_tag(session_id: Uuid) -> String {
    format!("{TASK_NOTIFICATION_TAG_PREFIX}{session_id}")
}

pub(crate) fn task_id_from_notification_tag(tag: &str) -> Option<Uuid> {
    tag.strip_prefix(TASK_NOTIFICATION_TAG_PREFIX)?.parse().ok()
}

fn signal_event_pump(wake: &smol::channel::Sender<()>) {
    let _ = wake.try_send(());
}

/// Source bytes of parsed messages kept across session switches.
///
/// Measured at ~17x expansion into parsed structures, plus flattened text and
/// shaped runs on top, so this is bounded by source size rather than entry
/// count — one long message costs far more than several short ones. 512 KB
/// holds several sessions' transcripts for a few MB of structures.
const MAX_CACHED_MESSAGE_SOURCE_BYTES: usize = 512 * 1024;
/// Projects whose workspace lookups are remembered — branch, diff listing,
/// working tree. A window rarely has more than a handful open, and the diff
/// and tree caches are invalidated on every refresh, so they hold one entry in
/// practice. 8 is generous and caps the tree cache, the only large one, at a
/// few hundred KB.
const MAX_CACHED_WORKSPACES: usize = 8;
const STREAM_REMEASURE_TAIL_ROWS: usize = 3;
/// Top-level markdown blocks the live reasoning peek renders, counted from
/// the tail. The peek is a 400 px viewport pinned to the newest thought, so
/// this only bounds how far a mid-stream scrollback reaches — the full trace
/// renders once the turn settles. 48 blocks is far more than the viewport
/// shows and keeps a long think from costing O(document) per pulse tick.
const LIVE_REASONING_TAIL_BLOCKS: usize = 48;
/// Source bytes the live reasoning peek keeps parsed, counted from the tail.
/// Markdown cost is O(rendered source) per pulse tick regardless of block
/// shape — a wall-of-text think is one giant paragraph and a bulleted think
/// one giant list, so the block cap above bounds neither. Six KB is several
/// viewports of scrollback; the full trace renders once the turn settles.
const LIVE_REASONING_WINDOW_TARGET: usize = 6 * 1024;
/// Slide hysteresis: the window re-anchors (and the peek reparses from a
/// fresh view) only once the tail outgrows this. Fast reasoning can append
/// several KB per commit, so the gap to the target is deliberately wide —
/// a slide costs a full window rebuild, and sliding every commit would pay
/// it at commit rate.
const LIVE_REASONING_WINDOW_MAX: usize = 18 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamPhase {
    Text,
    Reasoning,
    Activity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamDeltaKind {
    Text,
    Reasoning,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BranchPickerMode {
    #[default]
    Browse,
    Create,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BranchPickerAction {
    Checkout(String),
    Create,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsPage {
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

impl SettingsPage {
    /// Computer Use and Friends are still experimental, so their navigation
    /// entry points only appear once the Experiments opt-in is on. Keeping
    /// this decision on the page itself makes the Settings sidebar and
    /// command palette use the same gate.
    fn is_visible_in_navigation(
        self,
        computer_use_experiment_enabled: bool,
        friends_enabled: bool,
    ) -> bool {
        match self {
            Self::ComputerUse => computer_use_experiment_enabled,
            Self::Friends => friends_enabled,
            Self::Keybindings => crate::keybindings::manager_enabled(),
            _ => true,
        }
    }

    /// A persisted page whose navigation gate closed falls back to General
    /// rather than rendering a surface the sidebar no longer lists.
    fn into_visible(self, computer_use_experiment_enabled: bool, friends_enabled: bool) -> Self {
        if self.is_visible_in_navigation(computer_use_experiment_enabled, friends_enabled) {
            self
        } else {
            Self::General
        }
    }
}

/// Which presentation the Usage page shows: the daily dashboard, the monthly
/// statement, or the per-project ranking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UsageViewMode {
    Daily,
    Monthly,
    Projects,
}

/// Which unit the Usage page's headline and chart read in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UsageMetric {
    Cost,
    Tokens,
}

/// Which table the Usage page's breakdown section shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UsageBreakdown {
    Model,
    Day,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PanelResizeTarget {
    Sidebar,
    RightPanel,
    FileTree,
    /// The horizontal divider between the Git panel's top region and its
    /// commit log — the one drag that moves on the y axis.
    GitPanelTop,
}

#[derive(Clone, Copy, Debug)]
struct PanelResizeDrag {
    target: PanelResizeTarget,
    start_mouse_x: f32,
    start_mouse_y: f32,
    /// Width for the vertical edges, height for `GitPanelTop`.
    start_size: f32,
}

#[derive(Debug)]
struct ToastState {
    message: String,
    /// Extra lines under the title — today only a running command's
    /// output tail.
    detail: Option<Vec<String>>,
    tone: ToastTone,
    action: Option<ToastAction>,
    id: u64,
    timer_generation: u64,
    duration_remaining: Duration,
    timer_started: Option<Instant>,
    hovered: bool,
    /// Set for the persistent "a port is live" toast: the terminal the URL
    /// came from and the URL itself, for one-toast-per-terminal bookkeeping
    /// and the open actions. `None` toasts are transient.
    localhost: Option<LocalhostToast>,
}

/// What a localhost toast is offering to open. The terminal entity id keeps
/// the toast accountable to its source: one pending toast per terminal.
#[derive(Clone, Debug)]
struct LocalhostToast {
    terminal: EntityId,
    url: SharedString,
}

/// A localhost URL a terminal printed while the toast slot was busy, queued
/// behind whatever is showing. At most one entry per terminal — a fresher
/// detection rewrites the pending one.
#[derive(Debug)]
struct PendingLocalhostUrl {
    terminal: EntityId,
    url: SharedString,
}

/// A toast button that does more than dismiss.
#[derive(Clone, Debug)]
struct ToastAction {
    label: SharedString,
    kind: ToastActionKind,
}

#[derive(Clone, Debug)]
enum ToastActionKind {
    /// The unarchive confirmation's "View now" jump to the restored task.
    Session(Uuid),
    /// Open the detected localhost URL — externally, or in a browser tab
    /// when shift is held.
    LocalhostUrl,
    /// A missing project folder's "Locate Folder…" picker.
    RelocateProject(Uuid),
    /// Open the created issue — the in-app GitHub browser when its project
    /// and number are known, the URL otherwise. `project` keys the browser.
    GitHubIssue {
        project: Option<Uuid>,
        number: Option<u64>,
        url: SharedString,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ToastTone {
    Alert,
    Success,
    /// A custom command still running — a spinner riding the shared pulse
    /// clock.
    Progress,
    /// A custom command that reported a nonzero exit — a red x, distinct
    /// from the warning triangle `Alert` uses.
    Failure,
    Notice,
}

/// A custom command launched without revealing the right panel. Its
/// spinner toast reports the launch; the launch line's exit-code sentinel
/// resolves it — under the same toast while that is still on screen, as a
/// fresh toast when the run outlived the spinner's short window.
struct PendingCommandRun {
    name: String,
    /// The toast reporting this run. Stale once it dismisses or a newer
    /// toast replaces it, which is what hands a late result a fresh toast.
    toast_id: u64,
    /// Bottom screen lines last read from the run's terminal, waiting to
    /// be mirrored into the toast or already there.
    tail: Vec<String>,
    /// Bounds tail publishes to the stream-commit cadence. `None` until
    /// the first one.
    tail_published_at: Option<Instant>,
    /// A trailing publish timer is in flight for the buffered tail.
    tail_flush_armed: bool,
}

fn paused_toast_duration(remaining: Duration, elapsed: Duration) -> Duration {
    remaining
        .saturating_sub(elapsed)
        .max(MINIMUM_TOAST_RESUME_DURATION)
}

/// A file dropped onto the composer, staged as a chip until the next
/// submission carries it as an `@` mention.
#[derive(Clone, Debug)]
struct ComposerAttachment {
    /// Materialized path on the daemon host. This is the only path sent to a
    /// provider or persisted with a task.
    path: PathBuf,
    /// Ephemeral decoded client image used only for an immediate preview after
    /// upload. It is never persisted or sent to the daemon.
    client_preview_image: Option<Arc<gpui::Image>>,
    /// What the submission sends: relative to the project root when the file
    /// is inside it, absolute otherwise, directories with a trailing slash.
    mention: String,
    /// Basename drawn on the chip.
    name: SharedString,
    is_dir: bool,
    /// Whether the chip shows a thumbnail. Decided by extension at drop time
    /// so render never touches the filesystem.
    is_image: bool,
    /// Daemon-issued durable reference retained by task persistence.
    blob_reference: Option<String>,
    /// When set, the attachment is a large paste stored as a `.txt` blob and
    /// this holds its leading characters for the chip's hover preview. `Some`
    /// doubles as the pasted-text marker — the chip reads "Pasted text".
    pasted_text_preview: Option<String>,
    /// When set, the chip references another task rather than a file: `name`
    /// holds its title and `mention` its provider-facing token.
    session_id: Option<Uuid>,
}

#[derive(Clone, Debug)]
enum RemoteImageState {
    Loading,
    Ready(Arc<gpui::Image>),
    Unavailable,
}

/// One accepted composer submission. `prompt` preserves the composer and
/// transcript syntax; provider-specific command syntax resolves only at the
/// transport boundary. Presentation metadata keeps appended attachment
/// mentions out of the user bubble.
#[derive(Clone, Debug)]
struct ComposerSubmission {
    prompt: String,
    display_content: Option<String>,
    /// The user's own words for titles and a restored draft, kept apart from
    /// `display_content` when annotations put quote blocks in the bubble.
    human_content: Option<String>,
    attachments: Vec<MessageAttachment>,
    /// Collapsed paste blocks already folded into `prompt`, kept so a failed
    /// submission can restore them as composer cards rather than inline text.
    pasted_blocks: Vec<String>,
    /// Transcript annotations already folded into `prompt`'s header, kept so a
    /// failed submission can restore them alongside the draft text.
    annotations: Vec<TranscriptAnnotation>,
    /// Provider-facing text no transcript renders — the internal "continue"
    /// nudge. Never user input: no title, no restored draft, no bubble.
    hidden: bool,
}

/// The provider-facing prompt an empty-composer continue sends to an
/// interrupted turn. `Message::hidden` keeps it out of every transcript.
const CONTINUE_PROMPT: &str = "Continue the current task if able.";

impl ComposerSubmission {
    fn plain(prompt: String) -> Self {
        Self {
            prompt,
            display_content: None,
            human_content: None,
            attachments: Vec::new(),
            pasted_blocks: Vec::new(),
            annotations: Vec::new(),
            hidden: false,
        }
    }

    fn hidden_continue() -> Self {
        Self {
            hidden: true,
            ..Self::plain(CONTINUE_PROMPT.to_owned())
        }
    }

    fn into_queued_message(self) -> QueuedMessage {
        let mut message =
            QueuedMessage::with_presentation(self.prompt, self.display_content, self.attachments);
        message.hidden = self.hidden;
        message
    }

    fn from_queued_message(message: QueuedMessage) -> Self {
        Self {
            prompt: message.content,
            display_content: message.display_content,
            human_content: None,
            attachments: message.attachments,
            // A queued message carries no block split — the paste text is
            // already inside `content`, so editing pulls it back inline.
            pasted_blocks: Vec::new(),
            // The annotation header already lives inside `content`; the
            // structured set rides `queued_annotations` and the caller
            // reattaches it. Queueing counts as sent, so the highlights stay
            // cleared.
            annotations: Vec::new(),
            hidden: message.hidden,
        }
    }

    /// Human-facing task text for titles and generated worktree names. An
    /// attachment-only submission uses basenames instead of its transport
    /// paths; providers still receive `prompt` unchanged.
    fn human_prompt(&self) -> String {
        let visible = self
            .human_content
            .as_deref()
            .or(self.display_content.as_deref())
            .unwrap_or(&self.prompt)
            .trim();
        if !visible.is_empty() {
            return visible.to_owned();
        }
        if !self.attachments.is_empty() {
            return self
                .attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>()
                .join(" ");
        }
        self.prompt.trim().to_owned()
    }
}

/// Whether an untouched session's provider process may be released.
///
/// A session mid-turn is not idle however long it has been quiet: a slow tool
/// call, or an approval waiting on the user, must not have its agent pulled out
/// from under it.
fn session_is_reapable(
    session: Option<&AgentSession>,
    idle_for: Duration,
    has_live_background_work: bool,
) -> bool {
    !has_live_background_work
        && idle_for >= IDLE_SESSION_TIMEOUT
        && session.is_none_or(|session| {
            session.active_turn_id().is_none()
                && matches!(session.status, SessionStatus::Idle | SessionStatus::Failed)
        })
}

fn sanitize_panel_width(width: f32, default: f32, min: f32, max: f32) -> f32 {
    if width.is_finite() {
        width.clamp(min, max)
    } else {
        default
    }
}

fn persisted_window_state(
    bounds: Bounds<Pixels>,
    maximized: bool,
    display: Option<Uuid>,
) -> PersistedWindowState {
    PersistedWindowState {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
        display,
    }
}

fn fitted_file_tree_width(panel_width: f32, file_tree_width: f32) -> f32 {
    let maximum = FILE_TREE_MAX_WIDTH
        .min(panel_width - FILE_EDITOR_MIN_WIDTH)
        .max(FILE_TREE_MIN_WIDTH);
    sanitize_panel_width(
        file_tree_width,
        DEFAULT_FILE_TREE_WIDTH.clamp(FILE_TREE_MIN_WIDTH, maximum),
        FILE_TREE_MIN_WIDTH,
        maximum,
    )
}

fn widened_panel_width_for_file_editor(panel_width: f32, file_tree_width: f32) -> f32 {
    let panel_width = sanitize_panel_width(
        panel_width,
        DEFAULT_RIGHT_PANEL_WIDTH,
        RIGHT_PANEL_MIN_WIDTH,
        RIGHT_PANEL_MAX_WIDTH,
    );
    let file_tree_width = sanitize_panel_width(
        file_tree_width,
        DEFAULT_FILE_TREE_WIDTH,
        FILE_TREE_MIN_WIDTH,
        FILE_TREE_MAX_WIDTH,
    );
    panel_width
        .max(file_tree_width + FILE_EDITOR_INITIAL_WIDTH)
        .min(RIGHT_PANEL_MAX_WIDTH)
}

fn widened_panel_width_for_review(panel_width: f32) -> f32 {
    sanitize_panel_width(
        panel_width,
        DEFAULT_RIGHT_PANEL_WIDTH,
        RIGHT_PANEL_MIN_WIDTH,
        RIGHT_PANEL_MAX_WIDTH,
    )
    .max(REVIEW_INITIAL_WIDTH)
}

/// The Git panel's top region as laid out this frame: the stored height
/// clamped so the commit log below it keeps its minimum room.
fn fitted_git_panel_top_height(viewport_height: f32, height: f32) -> f32 {
    let maximum = (viewport_height - GIT_PANEL_HEADER_HEIGHT - GIT_PANEL_COMMITS_MIN_HEIGHT)
        .clamp(GIT_PANEL_TOP_MIN_HEIGHT, GIT_PANEL_TOP_MAX_HEIGHT);
    sanitize_panel_width(
        height,
        DEFAULT_GIT_PANEL_TOP_HEIGHT.clamp(GIT_PANEL_TOP_MIN_HEIGHT, maximum),
        GIT_PANEL_TOP_MIN_HEIGHT,
        maximum,
    )
}

fn fitted_panel_widths(
    viewport_width: f32,
    sidebar_visible: bool,
    right_panel_visible: bool,
    sidebar_width: f32,
    right_panel_width: f32,
) -> (f32, f32) {
    let sidebar_min = if sidebar_visible {
        SIDEBAR_MIN_WIDTH
    } else {
        0.0
    };
    let right_panel_min = if right_panel_visible {
        RIGHT_PANEL_MIN_WIDTH
    } else {
        0.0
    };
    let mut sidebar = if sidebar_visible {
        sanitize_panel_width(
            sidebar_width,
            DEFAULT_SIDEBAR_WIDTH,
            SIDEBAR_MIN_WIDTH,
            SIDEBAR_MAX_WIDTH,
        )
    } else {
        0.0
    };
    let mut right_panel = if right_panel_visible {
        sanitize_panel_width(
            right_panel_width,
            DEFAULT_RIGHT_PANEL_WIDTH,
            RIGHT_PANEL_MIN_WIDTH,
            RIGHT_PANEL_MAX_WIDTH,
        )
    } else {
        0.0
    };

    let available = (viewport_width - MAIN_PANEL_MIN_WIDTH).max(0.0);
    let mut overflow = (sidebar + right_panel - available).max(0.0);
    let right_reduction = overflow.min((right_panel - right_panel_min).max(0.0));
    right_panel -= right_reduction;
    overflow -= right_reduction;
    let sidebar_reduction = overflow.min((sidebar - sidebar_min).max(0.0));
    sidebar -= sidebar_reduction;
    overflow -= sidebar_reduction;

    // The configured minimum window easily fits both panel minima. This final
    // fallback only protects layout if the host temporarily reports a smaller
    // viewport during a resize or display transition.
    if overflow > 0.0 {
        let right_reduction = overflow.min(right_panel);
        right_panel -= right_reduction;
        overflow -= right_reduction;
        sidebar = (sidebar - overflow).max(0.0);
    }

    (sidebar, right_panel)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RightPanelSurface {
    Browser(Uuid),
    Terminal(Uuid),
    BackgroundWork {
        key: BackgroundWorkKey,
        title: String,
    },
    /// A pull request the session's branch links to, opened from the
    /// header's pull-request chip. The session supplies the project; the
    /// number names the detail.
    PullRequest {
        number: u64,
    },
    Files,
    Diff,
    File(String),
    /// A project's issue/pull-request detail. Which item it shows lives in
    /// `GitHubBrowser::detail`, so one tab serves every item in the repo.
    GitHub(Uuid),
}

/// The closed sidebar's left-edge hover peek: the real sidebar pane mounted
/// as an overlay that takes no space in the layout, nudged in on reveal and
/// back out just before it unmounts. Scroll offset and all sidebar state
/// live on [`Waku`], so the overlay and the docked panel never drift apart.
#[derive(Clone, Copy)]
enum SidebarPeek {
    Hidden,
    /// On screen; `entered` drives the nudge-in.
    Shown {
        entered: Instant,
    },
    /// Hover lost; the nudge-out runs and the overlay unmounts at its end.
    Exiting {
        started: Instant,
    },
}

/// A turn whose checkpoint still has to be captured.
struct PendingCheckpointCapture {
    session_id: Uuid,
    turn_count: usize,
    project_path: PathBuf,
}

/// Sessions between accepting a submission and handing it to a provider.
///
/// Worktree creation and the pre-turn checkpoint both run off the UI thread,
/// but neither operation has a safe interrupt contract. Keeping this separate
/// from [`SessionStatus`] lets the composer distinguish that non-cancellable
/// preparation window from a connecting provider that can already be stopped.
struct PreparedSubmission {
    workspace: SessionWorkspace,
    checkpoint_warning: Option<String>,
    /// The session's worktree directory was missing and got recreated from
    /// its branch or latest checkpoint — worth a toast, since uncommitted
    /// work past that point is gone.
    worktree_restored: bool,
    /// `None` reuses an already-live runtime. `Some` contains the result of a
    /// provider process start performed on the background executor.
    driver: Option<anyhow::Result<PreparedDriver>>,
    /// The routing decision an Auto submission produced — `None` on direct
    /// starts and on route-RPC failures (which fall back to the draft's own
    /// provider).
    route_decision: Option<waku_protocol::routing::RouteDecision>,
}

/// Everything needed to start a provider process, captured while the session
/// is still on the UI thread. `cwd` is replaced with the materialized
/// worktree path by the background preparation task.
struct DriverStartRequest {
    session_id: Uuid,
    provider: ProviderKind,
    options: DriverStartOptions,
    event_wake: smol::channel::Sender<()>,
    daemon: waku_client::DaemonSupervisor,
}

/// A provider process that has started off-thread but is not installed into
/// Goddard's runtime map yet. Its event receiver safely buffers early events.
struct PreparedDriver {
    handle: DriverHandle,
    events: Receiver<DriverEvent>,
}

/// One daemon's catalog contribution to the merged project/session lists,
/// tagged with the daemon it came from so writes route back to the owner.
type RemoteTaskStateSnapshot = waku_client::persistence::TaskCatalog;

/// Fold a cached remote catalog into the merged view before the host's
/// supervisor exists. Rows are claimed to the host immediately so a local
/// `SaveTaskState` never claims them and RPCs route correctly the moment the
/// host connects.
fn seed_remote_catalog(
    state: &mut PersistedState,
    daemons: &waku_client::DaemonMap,
    host: Uuid,
    catalog: &RemoteTaskStateSnapshot,
) {
    state.projects.extend(catalog.projects.iter().cloned());
    state.sessions.extend(catalog.sessions.iter().cloned());
    daemons.replace_remote_catalog(
        host,
        &catalog
            .projects
            .iter()
            .map(|project| project.id)
            .collect::<Vec<_>>(),
        &catalog
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EscapeStopTarget {
    session_id: Uuid,
    turn_id: Option<Uuid>,
}

impl EscapeStopTarget {
    fn for_session(session: &AgentSession) -> Self {
        Self {
            session_id: session.id,
            turn_id: session.active_turn_id(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EscapeStopPress {
    Arm(EscapeStopArm),
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EscapeStopArm {
    target: EscapeStopTarget,
    expires_at: Instant,
}

#[derive(Default)]
struct EscapeStopConfirmation {
    arm: Option<EscapeStopArm>,
}

impl EscapeStopConfirmation {
    fn press(&mut self, target: EscapeStopTarget, now: Instant) -> EscapeStopPress {
        if self
            .arm
            .is_some_and(|arm| arm.target == target && now < arm.expires_at)
        {
            self.arm = None;
            EscapeStopPress::Stop
        } else {
            let arm = EscapeStopArm {
                target,
                expires_at: now + ESCAPE_STOP_CONFIRMATION_TIMEOUT,
            };
            self.arm = Some(arm);
            EscapeStopPress::Arm(arm)
        }
    }

    fn is_armed_for(&self, target: EscapeStopTarget, now: Instant) -> bool {
        self.arm
            .is_some_and(|arm| arm.target == target && now < arm.expires_at)
    }

    fn expire(&mut self, arm: EscapeStopArm) -> bool {
        if self.arm != Some(arm) {
            return false;
        }
        self.arm = None;
        true
    }

    fn clear(&mut self) {
        self.arm = None;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventPumpSchedule {
    Idle,
    StreamFrame,
    BackgroundOutput(Duration),
}

/// One cached island of the root view: a region rendered by delegating back
/// into [`Waku`] under its own view identity.
///
/// All state stays on the root entity; what the island buys is scope for
/// gpui's cached-view machinery. The pulse clock and the streaming veil lease
/// `window.current_view()`, so their ~30 fps ticks dirty only the island
/// hosting the animation while every sibling island replays its cached
/// subtree instead of rebuilding. Observing the root preserves the old
/// invalidation semantics exactly — any root notify still re-renders every
/// island — so caching cannot show state the single-view architecture would
/// have repainted.
struct WakuPane {
    waku: Option<WeakEntity<Waku>>,
    content: fn(&mut Waku, &mut Window, &mut Context<Waku>) -> AnyElement,
}

impl WakuPane {
    fn new(
        content: fn(&mut Waku, &mut Window, &mut Context<Waku>) -> AnyElement,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|_| Self {
            waku: None,
            content,
        })
    }

    fn bind(&mut self, waku: &Entity<Waku>, cx: &mut Context<Self>) {
        self.waku = Some(waku.downgrade());
        cx.observe(waku, |_, waku, cx| {
            // A panel slide notifies the root at display rate for its 200ms,
            // and this fan-out would price every one of those ticks at a
            // three-island rebuild. Skipping it hands the decision to the
            // cached-view keys: the sliding panel (its clip moves) and the
            // transcript (its bounds move) miss their caches and re-render
            // with fresh state anyway, while the island nothing is moving
            // re-plays its cached subtree. Root-state changes it displays
            // can wait out the slide: updates born inside an island
            // (terminal output, pulse leases) dirty their ancestor pane
            // without this observer, and the slide's retirement notify
            // below re-runs the fan-out, so nothing outlasts the 200ms.
            if !waku.read(cx).panels_sliding() {
                cx.notify();
            }
        })
        .detach();
    }
}

impl Render for WakuPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(waku) = self.waku.as_ref().and_then(WeakEntity::upgrade) else {
            return gpui::div().into_any_element();
        };
        let content = self.content;
        waku.update(cx, |waku, cx| content(waku, window, cx))
    }
}

/// What a `Cmd+P` confirm hands to a file editor: which file takes keyboard
/// focus on the first frame its entity exists, and optionally the 1-based
/// `(line, column)` a `path:line[:column]` query asked the caret to land on.
struct PendingFileFocus {
    path: String,
    position: Option<(usize, usize)>,
}

struct RightPanelFileEditor {
    state: Entity<TextInput>,
    disk_content: String,
    writable: bool,
    dirty: bool,
    /// A text read has landed at least once. SVGs open in preview mode, so
    /// this separates "empty file" from "never read" when the source toggle
    /// asks for the editor.
    text_loaded: bool,
    /// Decoded image preview from `ReadBinaryFile`: `None` until the read
    /// lands, `Some(Err)` on failure so the pane shows a fallback instead of
    /// re-requesting every frame.
    image: Option<Result<Arc<gpui::Image>, String>>,
    /// Image-pixels → screen-pixels scale; `0.0` means "unset" until the
    /// first layout can compute the fit-to-view zoom.
    image_zoom: f32,
    /// The image's top offset inside the viewport — `0` when it's shorter
    /// than the pane (which then centers it), clamped into
    /// `[viewport − scaled height, 0]` while it's taller.
    image_pan_y: Pixels,
    /// Viewport bounds and decoded pixel size recorded during prepaint —
    /// the wheel handler needs both to clamp pan and zoom around the cursor.
    image_viewport: Option<Bounds<Pixels>>,
    image_natural: Option<(f32, f32)>,
    /// SVG only: edit the source instead of viewing the rendered preview.
    /// Other image formats have no meaningful text view.
    show_source: bool,
    /// A read is in flight on the background executor. Set from the moment the
    /// editor is created, because `render` may not touch the filesystem: until
    /// the first read lands the editor is empty and locked, and that means
    /// "not read yet", never "empty file".
    reading: bool,
    /// Bumped whenever the editor's idea of the file changes, so a read that
    /// started earlier cannot apply over a newer truth — a save in particular,
    /// which makes any read already in flight describe the pre-save file.
    read_epoch: u64,
    /// A `(line, column)` jump target from the finder, waiting on the file's
    /// read — the caret cannot land on a line the editor does not have yet.
    pending_position: Option<(usize, usize)>,
    /// Pinned selection highlights with comments — this editor's share of the
    /// session's annotation set. Painted inside the field, counted in the
    /// composer chip, drained into the next submission alongside the
    /// transcript's; `RefCell` matches the transcript store's access shape.
    annotations: Rc<RefCell<Annotations>>,
}

struct RightPanelSessionState {
    visible: bool,
    surfaces: Vec<RightPanelSurface>,
    active_surface: Option<usize>,
    /// Terminal that last held keyboard focus, so `FocusTerminal` can return
    /// to it rather than whichever surface happens to be active.
    last_focused_terminal: Option<Uuid>,
    tabs_scroll_handle: ScrollHandle,
    pending_tab_reveal: Option<usize>,
    expanded_paths: HashSet<PathBuf>,
    files_selected_path: Option<String>,
    file_tree_width: f32,
    file_editors: HashMap<String, RightPanelFileEditor>,
    diff_source: ReviewDiffSource,
    diff_snapshot: Option<Arc<ReviewDiffSnapshot>>,
    diff_selected_file: Option<usize>,
    diff_expanded_paths: HashSet<String>,
}

impl RightPanelSessionState {
    fn empty(visible: bool) -> Self {
        Self {
            visible,
            surfaces: Vec::new(),
            active_surface: None,
            last_focused_terminal: None,
            tabs_scroll_handle: ScrollHandle::new(),
            pending_tab_reveal: None,
            expanded_paths: HashSet::new(),
            files_selected_path: None,
            file_tree_width: DEFAULT_FILE_TREE_WIDTH,
            file_editors: HashMap::new(),
            diff_source: ReviewDiffSource::default(),
            diff_snapshot: None,
            diff_selected_file: None,
            diff_expanded_paths: HashSet::new(),
        }
    }

    fn take_or_closed(states: &mut HashMap<Uuid, Self>, session_id: Uuid) -> Self {
        states
            .remove(&session_id)
            .unwrap_or_else(|| Self::empty(false))
    }
}

/// One choice in the model-traits menu: a label, with a check on the
/// current selection.
fn traits_choice(theme: Theme, label: String, selected: bool) -> MenuItem {
    MenuItem::custom(move |_, _| {
        div()
            .w(px(190.0))
            .py(px(2.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(theme.text_secondary)
                    .child(label.clone()),
            )
            .when(selected, |element| {
                element.child(icon("icons/check.svg", 11.0, theme.text_tertiary))
            })
            .into_any_element()
    })
}

#[derive(Clone, Copy, Debug)]
struct UserMessageAction {
    session_id: Uuid,
    /// The prompt that opened the turn. A turn can hold several user messages
    /// once a steer lands in it, and only the opening one is a rewind
    /// boundary, so this is what the editor and the rewind both address.
    message_id: Uuid,
    turn_count: usize,
}

#[derive(Clone, Copy, Debug)]
struct AssistantMessageAction {
    session_id: Uuid,
    turn_count: usize,
    enabled: bool,
    preparing: bool,
}

#[derive(Clone)]
struct MessageEdit {
    session_id: Uuid,
    /// Identifies the edited bubble outright. Matching by turn alone would
    /// open this one input on every user message the turn holds.
    message_id: Uuid,
    turn_count: usize,
    input: Entity<ComposerInput>,
    attachments: Vec<MessageAttachment>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TranscriptAnchor {
    session_id: Uuid,
    turn_id: Uuid,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct NavigationRailVisualState {
    emphasized_turn: Option<Uuid>,
}

struct SessionRuntime {
    driver: DriverHandle,
    /// Invalidates stale ApplyOptions responses when settings change again or
    /// this runtime is replaced while the RPC is in flight.
    options_generation: u64,
    events: Receiver<DriverEvent>,
    pending_events: VecDeque<DriverEvent>,
    /// Presentation metadata for steering messages awaiting the provider's
    /// accepted/rejected acknowledgement, in transport order.
    pending_steers: VecDeque<ComposerSubmission>,
    stream_phase: Option<StreamPhase>,
    /// Newline-only reasoning deltas buffered while a reasoning block streams.
    /// Some providers (e.g. GLM via OpenRouter) emit every text chunk followed
    /// by a newline-only chunk; joined verbatim that renders one token per
    /// line, so the run is collapsed instead — see `push_reasoning_delta`.
    pending_reasoning_newlines: usize,
    /// The parked-turn notification has fired for the turn in flight, so a
    /// wake that parks again does not repeat it. Cleared when the turn ends.
    park_announced: bool,
    stream_remeasure_pending: bool,
    pending_permission: Option<PendingPermission>,
    pending_user_input: Option<PendingUserInput>,
    pending_computer_approval: Option<PendingComputerApproval>,
    /// Back-to-front stack of window previews captured during the active turn.
    computer_use_previews: Vec<ComputerUsePreview>,
    computer_session_grants: HashSet<String>,
    last_driver_error: Option<String>,
    /// When this session last sent or received anything, for idle reaping.
    last_active_at: Instant,
    /// Background-process snapshots are provider IPC. Keep the polling clock
    /// on the runtime so switching tasks never creates duplicate probes.
    last_background_refresh_at: Instant,
}

#[derive(Clone)]
struct PendingUserInput {
    request_id: String,
    questions: Vec<UserInputQuestion>,
    question_index: usize,
    selections: HashMap<String, Vec<String>>,
    custom_answers: HashMap<String, String>,
}

impl PendingUserInput {
    fn new(request_id: String, questions: Vec<UserInputQuestion>) -> Self {
        Self {
            request_id,
            questions,
            question_index: 0,
            selections: HashMap::new(),
            custom_answers: HashMap::new(),
        }
    }

    fn current_question(&self) -> Option<&UserInputQuestion> {
        self.questions.get(self.question_index)
    }

    fn answers(&self) -> Vec<UserInputAnswer> {
        self.questions
            .iter()
            .map(|question| {
                let custom = self
                    .custom_answers
                    .get(&question.id)
                    .map(|answer| answer.trim())
                    .filter(|answer| !answer.is_empty());
                UserInputAnswer {
                    question_id: question.id.clone(),
                    answers: custom.map_or_else(
                        || {
                            self.selections
                                .get(&question.id)
                                .cloned()
                                .unwrap_or_default()
                        },
                        |answer| vec![answer.to_owned()],
                    ),
                }
            })
            .collect()
    }
}

struct ComputerUsePreview {
    target: Option<ComputerTarget>,
    phase: ComputerUsePhase,
    visible: bool,
    frames: crate::computer_use::PreviewFrames<crate::computer_use::PreviewImage>,
    decode_task: Option<gpui::Task<()>>,
}

/// One spot back/forward history can point at. A task transcript, a
/// full-width terminal, and the Projects page share the main column, so
/// they share the one history; the page entry remembers which project it
/// was scoped to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationLocation {
    Task(Uuid),
    Terminal(Uuid),
    ProjectsPage(Uuid),
    DraftsPage,
}

#[derive(Debug, Default)]
struct SessionNavigation {
    back: Vec<NavigationLocation>,
    forward: Vec<NavigationLocation>,
    /// The most recently selected unstarted task. The global New Task entry
    /// may reuse it only when it belongs to the currently selected project.
    new_task: Option<Uuid>,
}

impl SessionNavigation {
    fn visit(&mut self, current: Option<NavigationLocation>, next: NavigationLocation) {
        // `next` becomes the current location, so it can no longer be a
        // back/forward target — an entry pointing at it is a dead hop.
        self.back.retain(|entry| *entry != next);
        self.forward.retain(|entry| *entry != next);
        if let Some(current) = current.filter(|current| *current != next) {
            // `current` is pushed exactly once; earlier visits to it are
            // folded away so the stack never repeats a location.
            self.back.retain(|entry| *entry != current);
            self.back.push(current);
            self.forward.clear();
        }
    }

    fn go_back(&mut self, current: NavigationLocation) -> Option<NavigationLocation> {
        let target = self.back.pop()?;
        self.forward.push(current);
        Some(target)
    }

    fn back_target(&self) -> Option<NavigationLocation> {
        self.back.last().copied()
    }

    fn go_forward(&mut self, current: NavigationLocation) -> Option<NavigationLocation> {
        let target = self.forward.pop()?;
        self.back.push(current);
        Some(target)
    }

    fn forward_target(&self) -> Option<NavigationLocation> {
        self.forward.last().copied()
    }

    fn remove(&mut self, session_id: Uuid) {
        self.back
            .retain(|entry| *entry != NavigationLocation::Task(session_id));
        self.forward
            .retain(|entry| *entry != NavigationLocation::Task(session_id));
        if self.new_task == Some(session_id) {
            self.new_task = None;
        }
    }

    fn remove_terminal(&mut self, terminal_id: Uuid) {
        self.back
            .retain(|entry| *entry != NavigationLocation::Terminal(terminal_id));
        self.forward
            .retain(|entry| *entry != NavigationLocation::Terminal(terminal_id));
    }

    fn remember_new_task(&mut self, session_id: Uuid) {
        self.new_task = Some(session_id);
    }

    fn remembered_new_task(
        &self,
        sessions: &[AgentSession],
        current_project_id: Uuid,
    ) -> Option<Uuid> {
        self.new_task.filter(|session_id| {
            sessions.iter().any(|session| {
                session.id == *session_id
                    && session.project_id == current_project_id
                    && !session.has_started()
            })
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionActivationTransition {
    Visit,
    Back { from: NavigationLocation },
    Forward { from: NavigationLocation },
}

/// The watcher-facing dev flag file's `auto_restart` value — `false` when
/// the file is absent or unreadable.
fn read_auto_restart(path: &Path) -> bool {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("auto_restart")?.as_bool())
        .unwrap_or(false)
}

/// Flip `auto_restart` in the dev-state file, keeping any other keys a future
/// writer added.
fn write_auto_restart(path: &Path, enabled: bool) -> std::io::Result<()> {
    let mut document = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    document.insert("auto_restart".to_owned(), enabled.into());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec(&document)?)
}

/// Bridges between live navigation/panel enums and the persisted mirrors in
/// `waku_client::persistence`. Terminal and browser surfaces have no
/// persisted form — their processes die with the app — so they drop out of
/// history and tab strips rather than restore to a dead surface.
fn persisted_location(location: NavigationLocation) -> Option<PersistedNavigationLocation> {
    match location {
        NavigationLocation::Task(id) => Some(PersistedNavigationLocation::Task(id)),
        NavigationLocation::ProjectsPage(id) => Some(PersistedNavigationLocation::ProjectsPage(id)),
        NavigationLocation::DraftsPage => Some(PersistedNavigationLocation::DraftsPage),
        NavigationLocation::Terminal(_) => None,
    }
}

fn persisted_list_offset(offset: ListOffset) -> PersistedListOffset {
    PersistedListOffset {
        item_ix: offset.item_ix,
        offset_in_item: f32::from(offset.offset_in_item),
    }
}

fn list_offset_from_persisted(offset: PersistedListOffset) -> ListOffset {
    ListOffset {
        item_ix: offset.item_ix,
        offset_in_item: px(offset.offset_in_item),
    }
}

fn persisted_panel_surface(surface: &RightPanelSurface) -> Option<PersistedRightPanelSurface> {
    match surface {
        RightPanelSurface::Files => Some(PersistedRightPanelSurface::Files),
        RightPanelSurface::Diff => Some(PersistedRightPanelSurface::Diff),
        RightPanelSurface::File(path) => Some(PersistedRightPanelSurface::File(path.clone())),
        RightPanelSurface::PullRequest { number } => {
            Some(PersistedRightPanelSurface::PullRequest { number: *number })
        }
        RightPanelSurface::GitHub(project_id) => {
            Some(PersistedRightPanelSurface::GitHub(*project_id))
        }
        RightPanelSurface::Browser(_)
        | RightPanelSurface::Terminal(_)
        | RightPanelSurface::BackgroundWork { .. } => None,
    }
}

fn panel_surface_from_persisted(surface: &PersistedRightPanelSurface) -> RightPanelSurface {
    match surface {
        PersistedRightPanelSurface::Files => RightPanelSurface::Files,
        PersistedRightPanelSurface::Diff => RightPanelSurface::Diff,
        PersistedRightPanelSurface::File(path) => RightPanelSurface::File(path.clone()),
        PersistedRightPanelSurface::PullRequest { number } => {
            RightPanelSurface::PullRequest { number: *number }
        }
        PersistedRightPanelSurface::GitHub(project_id) => RightPanelSurface::GitHub(*project_id),
    }
}

fn persisted_diff_source(source: ReviewDiffSource) -> PersistedDiffSource {
    match source {
        ReviewDiffSource::LastTurn {
            session_id,
            turn_id,
            turn_count,
        } => PersistedDiffSource::LastTurn {
            session_id,
            turn_id,
            turn_count,
        },
        ReviewDiffSource::Uncommitted => PersistedDiffSource::Uncommitted,
        ReviewDiffSource::Unstaged => PersistedDiffSource::Unstaged,
        ReviewDiffSource::Staged => PersistedDiffSource::Staged,
        ReviewDiffSource::Committed => PersistedDiffSource::Committed,
        ReviewDiffSource::Branch => PersistedDiffSource::Branch,
        ReviewDiffSource::Commit => PersistedDiffSource::Commit,
    }
}

fn diff_source_from_persisted(source: PersistedDiffSource) -> ReviewDiffSource {
    match source {
        PersistedDiffSource::LastTurn {
            session_id,
            turn_id,
            turn_count,
        } => ReviewDiffSource::LastTurn {
            session_id,
            turn_id,
            turn_count,
        },
        PersistedDiffSource::Uncommitted => ReviewDiffSource::Uncommitted,
        PersistedDiffSource::Unstaged => ReviewDiffSource::Unstaged,
        PersistedDiffSource::Staged => ReviewDiffSource::Staged,
        PersistedDiffSource::Committed => ReviewDiffSource::Committed,
        PersistedDiffSource::Branch => ReviewDiffSource::Branch,
        PersistedDiffSource::Commit => ReviewDiffSource::Commit,
    }
}

fn persisted_settings_page(page: SettingsPage) -> PersistedSettingsPage {
    match page {
        SettingsPage::General => PersistedSettingsPage::General,
        SettingsPage::Providers => PersistedSettingsPage::Providers,
        SettingsPage::Skills => PersistedSettingsPage::Skills,
        SettingsPage::Friends => PersistedSettingsPage::Friends,
        SettingsPage::Archived => PersistedSettingsPage::Archived,
        SettingsPage::Usage => PersistedSettingsPage::Usage,
        SettingsPage::Daemon => PersistedSettingsPage::Daemon,
        SettingsPage::ComputerUse => PersistedSettingsPage::ComputerUse,
        SettingsPage::Commands => PersistedSettingsPage::Commands,
        SettingsPage::Appearance => PersistedSettingsPage::Appearance,
        SettingsPage::Git => PersistedSettingsPage::Git,
        SettingsPage::Experiments => PersistedSettingsPage::Experiments,
        SettingsPage::Keybindings => PersistedSettingsPage::Keybindings,
    }
}

fn settings_page_from_persisted(page: PersistedSettingsPage) -> SettingsPage {
    match page {
        PersistedSettingsPage::General => SettingsPage::General,
        PersistedSettingsPage::Providers => SettingsPage::Providers,
        PersistedSettingsPage::Skills => SettingsPage::Skills,
        PersistedSettingsPage::Friends => SettingsPage::Friends,
        PersistedSettingsPage::Archived => SettingsPage::Archived,
        PersistedSettingsPage::Usage => SettingsPage::Usage,
        PersistedSettingsPage::Daemon => SettingsPage::Daemon,
        PersistedSettingsPage::ComputerUse => SettingsPage::ComputerUse,
        PersistedSettingsPage::Commands => SettingsPage::Commands,
        PersistedSettingsPage::Appearance => SettingsPage::Appearance,
        PersistedSettingsPage::Git => SettingsPage::Git,
        PersistedSettingsPage::Experiments => SettingsPage::Experiments,
        PersistedSettingsPage::Keybindings => SettingsPage::Keybindings,
    }
}

/// Project a session's right-panel state — parked or live — into its
/// persisted form. The active-tab index is remapped past the runtime-only
/// tabs that `persisted_panel_surface` drops; when the active tab itself
/// drops, the strip reopens on its first remaining surface.
fn persist_right_panel_state(
    visible: bool,
    surfaces: &[RightPanelSurface],
    active_surface: Option<usize>,
    expanded_paths: &HashSet<PathBuf>,
    files_selected_path: &Option<String>,
    file_tree_width: f32,
    diff_selected_file: Option<usize>,
    diff_expanded_paths: &HashSet<String>,
    diff_source: ReviewDiffSource,
) -> PersistedRightPanelState {
    let mut kept = Vec::new();
    let mut remap = vec![None; surfaces.len()];
    for (index, surface) in surfaces.iter().enumerate() {
        if let Some(persisted) = persisted_panel_surface(surface) {
            remap[index] = Some(kept.len());
            kept.push(persisted);
        }
    }
    PersistedRightPanelState {
        visible,
        active_surface: active_surface.and_then(|index| remap[index]),
        surfaces: kept,
        expanded_paths: expanded_paths.clone(),
        files_selected_path: files_selected_path.clone(),
        file_tree_width: Some(file_tree_width),
        diff_selected_file,
        diff_expanded_paths: diff_expanded_paths.clone(),
        diff_source: Some(persisted_diff_source(diff_source)),
    }
}

fn right_panel_state_from_persisted(state: &PersistedRightPanelState) -> RightPanelSessionState {
    let mut restored = RightPanelSessionState::empty(state.visible);
    restored.surfaces = state
        .surfaces
        .iter()
        .map(panel_surface_from_persisted)
        .collect();
    restored.active_surface = state
        .active_surface
        .filter(|index| *index < state.surfaces.len());
    restored.expanded_paths = state.expanded_paths.clone();
    restored.files_selected_path = state.files_selected_path.clone();
    if let Some(width) = state.file_tree_width {
        restored.file_tree_width = width;
    }
    restored.diff_selected_file = state.diff_selected_file;
    restored.diff_expanded_paths = state.diff_expanded_paths.clone();
    if let Some(source) = state.diff_source {
        restored.diff_source = diff_source_from_persisted(source);
    }
    restored
}

/// Where a session activation parks the transcript.
#[derive(Clone, Copy)]
enum TranscriptLanding {
    /// The reading position the session held when the reader left it;
    /// back/forward history restores it.
    Position(ListOffset),
    /// The top of the final turn — the same spot the navigation rail's last
    /// button jumps to.
    LastTurn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingSessionActivation {
    session_id: Uuid,
    transition: SessionActivationTransition,
}

#[derive(Clone)]
struct ActivityScrollViewport {
    scroll_handle: ScrollHandle,
    scrollbar: Rc<ScrollbarState>,
    follow_tail: Rc<Cell<bool>>,
    last_scrolled: Rc<Cell<Option<Pixels>>>,
    last_max_offset: Rc<Cell<Option<Pixels>>>,
}

#[derive(Clone, Default)]
struct UserMessageScrollViewport {
    /// Tracked for measurement, not scrolling: the bubble clips at
    /// `overflow_hidden`, and transcript search shifts this offset to reveal a
    /// match inside the clipped region.
    scroll_handle: ScrollHandle,
    /// Whether the capped viewport measured more content than fits as of the
    /// last prepaint; the bubble's "Show more" button renders from this.
    overflowing: Rc<Cell<bool>>,
}

/// Hover bookkeeping for one changed-files row and the floating diff card it
/// can open. Row and card hover are tracked separately so the preview
/// survives the pointer crossing the overlap between them.
struct ChangedFilesDiffHover {
    turn_id: Uuid,
    path: String,
    row_hovered: bool,
    card_hovered: bool,
    open: bool,
    scroll_handle: ScrollHandle,
    scrollbar: Rc<ScrollbarState>,
}

impl ChangedFilesDiffHover {
    fn targets(&self, turn_id: Uuid, path: &str) -> bool {
        self.turn_id == turn_id && self.path == path
    }
}

/// One turn diff fetched for the changed-files preview. `file_lines` maps
/// each `snapshot.files` index to its rendered line positions — the
/// file-header rows excluded — so a frame only indexes stored rows.
enum ChangedFilesDiff {
    Loading,
    Ready {
        snapshot: Arc<ReviewDiffSnapshot>,
        file_lines: Rc<Vec<Vec<usize>>>,
    },
    Failed(SharedString),
}

impl Default for ActivityScrollViewport {
    fn default() -> Self {
        Self {
            scroll_handle: ScrollHandle::new(),
            scrollbar: ScrollbarState::new(),
            follow_tail: Rc::new(Cell::new(true)),
            last_scrolled: Rc::new(Cell::new(None)),
            last_max_offset: Rc::new(Cell::new(None)),
        }
    }
}

pub struct Waku {
    /// Owns the headless provider process for exactly as long as the desktop
    /// app entity. Debug builds can replace it independently after a rebuild;
    /// all live driver handles below are lightweight RPC proxies.
    daemon: waku_client::DaemonSupervisor,
    /// Every daemon this window shows — the local supervisor plus one per
    /// connected remote host — and which daemon owns each catalog row. Shared
    /// with `store` and `composer_draft_store` so reads, writes, and routing
    /// agree.
    daemons: waku_client::DaemonMap,
    /// Remote hosts that have not connected yet or dropped their supervisor,
    /// host id → last connection error. Cleared on a successful install.
    remote_errors: HashMap<Uuid, String>,
    /// Daemon settings mirrored per remote host. Binary overrides and custom
    /// commands are host-local, so `state`'s copy stays the local daemon's.
    remote_daemon_settings: HashMap<Uuid, waku_client::DaemonSettings>,
    /// Each remote host's last applied catalog, persisted so an unreachable
    /// host still renders its rows at launch.
    remote_catalogs: HashMap<Uuid, RemoteTaskStateSnapshot>,
    remote_catalogs_path: std::path::PathBuf,
    /// Live ssh channels under connected remote hosts, host id → transport
    /// plus the local forward port the supervisor dials.
    #[cfg(unix)]
    ssh_transports: HashMap<Uuid, runtime::SshLink>,
    /// The askpass responder thread is a singleton; prompts from every ssh
    /// child funnel through the one queue fifo it serves.
    #[cfg(unix)]
    ssh_askpass_started: bool,
    /// A password/passphrase prompt ssh is waiting on, rendered as a modal.
    #[cfg(unix)]
    pending_ssh_prompt: Option<runtime::SshPrompt>,
    /// Cached once at construction for the Daemon settings connection URL;
    /// rendering must not query account or network configuration.
    daemon_hostname: String,
    /// Session details currently being fetched from the daemon. Sidebar rows
    /// stay usable while the selected transcript hydrates asynchronously.
    session_hydrations: HashSet<Uuid>,
    /// Selection is committed only after this target's transcript arrives, so
    /// the currently visible task stays intact during daemon latency.
    pending_session_activation: Option<PendingSessionActivation>,
    analytics: crate::analytics::Analytics,
    state: PersistedState,
    store: StateStore,
    /// Cached before rendering so path labels can abbreviate the home prefix
    /// without consulting the environment or account database in a frame.
    home_directory: Option<PathBuf>,
    composer: Entity<ComposerInput>,
    user_input_answer: Entity<TextInput>,
    /// Drafts are independent of transcript persistence: started tasks key by
    /// session id, while blank New Task pages key by project id.
    composer_drafts: ComposerDrafts,
    composer_draft_store: ComposerDraftStore,
    composer_draft_save_generation: u64,
    command_palette: command_palette::CommandPaletteUi,
    file_finder: file_finder::FileFinderUi,
    /// The ⌘S "Sync branch…" picker — a modal over the session's repository.
    sync_branch: sync_branch::SyncBranchUi,
    task_switcher: task_switcher::TaskSwitcherUi,
    project_switcher: project_switcher::ProjectSwitcherUi,
    big_picture: big_picture::BigPictureUi,
    model_search: Entity<TextInput>,
    /// The routing class picker's filter field — one shared set serves all
    /// three class menus; only one can be open at a time.
    route_class_search: Entity<TextInput>,
    settings_search: Entity<TextInput>,
    /// The Appearance page's two font pickers — one per configurable face.
    ui_font_selector: settings::FontSelector,
    code_font_selector: settings::FontSelector,
    daemon_port_input: Entity<TextInput>,
    daemon_origins_input: Entity<TextInput>,
    /// The worktree fast-forward branch whitelist, edited live on the
    /// General settings page.
    worktree_sync_branches_input: Entity<TextInput>,
    daemon_reconfigure_pending: bool,
    daemon_token_revealed: bool,
    /// The evaluation credentials editor's fields — one set per eval backend,
    /// seeded from the daemon's settings mirror when the routing section
    /// first shows. Secrets stay masked and never render elsewhere.
    eval_typesafe_key_input: Entity<TextInput>,
    eval_vercel_key_input: Entity<TextInput>,
    eval_vercel_team_input: Entity<TextInput>,
    eval_cloudflare_account_input: Entity<TextInput>,
    eval_cloudflare_token_input: Entity<TextInput>,
    eval_inputs_seeded: bool,
    settings_focus: FocusHandle,
    onboarding_add_project_focus: FocusHandle,
    onboarding_projectless_focus: FocusHandle,
    /// Mirror of Sparkle's persisted automatic-check setting. Refreshed when
    /// settings opens and on toggle, so frames never read user defaults —
    /// that lookup can reach cfprefsd.
    automatic_updates_enabled: bool,
    updater_status: crate::updater::UpdateStatus,
    updater_button_focus: FocusHandle,
    updater_button_hovered: bool,
    updater_button_focused: bool,
    updater_button_width: Rc<Cell<f32>>,
    updater_button_label_reveal: Rc<Cell<f32>>,
    updater_button_animation_from_width: f32,
    updater_button_animation_from_reveal: f32,
    updater_button_animation_generation: u64,
    probes: Vec<ProviderProbe>,
    provider_probe_tx: Sender<ProviderProbe>,
    provider_probe_events: Receiver<ProviderProbe>,
    provider_model_discoveries: HashSet<ProviderKind>,
    provider_model_discoveries_pending: HashSet<ProviderKind>,
    /// CLI version per provider, probed off-thread. Missing key means the
    /// probe has not answered yet; `None` means it ran and found nothing.
    provider_versions: HashMap<ProviderKind, Option<String>>,
    provider_version_tx: Sender<(ProviderKind, Option<String>)>,
    provider_version_events: Receiver<(ProviderKind, Option<String>)>,
    /// Providers with a version probe in flight, so a re-detect cannot stack
    /// a second subprocess on one that has not answered.
    provider_version_probes_pending: HashSet<ProviderKind>,
    /// Fast provider detection results from the daemon, including its cached
    /// model catalog. Live discovery revalidates these probes afterward.
    provider_detection_tx: Sender<ProviderProbe>,
    provider_detection_events: Receiver<ProviderProbe>,
    /// Providers the running re-detection has not answered for yet; empty
    /// means no re-detection is in flight.
    provider_detection_remaining: usize,
    /// When provider detection last completed, for the page's "Checked" label.
    provider_detection_checked_at: Option<Instant>,
    /// The provider row expanded on the Providers page, if any. The binary
    /// override input below edits this provider's entry.
    expanded_provider_settings: Option<ProviderKind>,
    provider_path_input: Entity<TextInput>,
    computer_permissions: ComputerPermissions,
    computer_permission_tx: Sender<Result<ComputerPermissions, String>>,
    computer_permission_events: Receiver<Result<ComputerPermissions, String>>,
    computer_permission_request_pending: bool,
    /// Account rate-limit meters per provider, fetched off-thread (Claude,
    /// Codex, and OpenCode Go over HTTPS; Grok through a stdio probe) and
    /// refreshed live by Codex's own stream. Frames read only this snapshot.
    plan_usage: HashMap<ProviderKind, crate::usage::PlanUsage>,
    /// Why a provider's last fetch failed, kept alongside stale data for the
    /// meter's tooltip. Cleared by that provider's next success.
    plan_usage_error: HashMap<ProviderKind, String>,
    plan_usage_tx: Sender<(
        ProviderKind,
        Result<Option<crate::usage::PlanUsage>, String>,
    )>,
    plan_usage_events: Receiver<(
        ProviderKind,
        Result<Option<crate::usage::PlanUsage>, String>,
    )>,
    plan_usage_pending: HashSet<ProviderKind>,
    /// Fetchable providers with no matching account credential. Unlike a
    /// request failure, this hides the plan section until a later refresh
    /// discovers a newly configured account.
    plan_usage_unconfigured: HashSet<ProviderKind>,
    /// When each provider's last fetch settled, successful or not — the
    /// refresh backoff measures from here.
    plan_usage_checked_at: HashMap<ProviderKind, Instant>,
    /// Providers whose turn settled since the last fetch, so the meters have
    /// moved.
    plan_usage_stale: HashSet<ProviderKind>,
    /// The settings Usage page's snapshot: historical token/cost usage
    /// scanned from provider transcripts off-thread. Frames read only this.
    usage_history: Option<crate::usage_history::UsageHistory>,
    /// Per-daemon slices the merged `usage_history` is built from, so a host
    /// that goes offline keeps contributing its last-known usage.
    usage_history_parts: HashMap<waku_client::DaemonKey, crate::usage_history::UsageHistory>,
    /// The window a scan is currently in flight for, so a repeat request for
    /// the same window coalesces while a changed window supersedes it.
    usage_history_pending_for: Option<crate::usage_history::UsageWindow>,
    /// Bumped per scan; a result from a superseded scan is discarded.
    usage_history_generation: u64,
    /// When the current snapshot landed, for the reopen-staleness check.
    usage_history_scanned_at: Option<Instant>,
    usage_view: UsageViewMode,
    /// The selected window for the daily and project views; the statement
    /// view fixes its own.
    usage_window: crate::usage_history::UsageWindow,
    usage_metric: UsageMetric,
    usage_breakdown: UsageBreakdown,
    /// Drag-resized widths for the usage breakdown tables' fixed columns —
    /// model (cost, share, tokens) and day (per-provider, total, tokens).
    usage_model_col_widths: [f32; 3],
    usage_day_col_widths: [f32; 4],
    usage_col_resize: Rc<column_resize::ColumnResize>,
    /// Scroll position of the monthly statement card, which scrolls
    /// internally like the projects card so the two list views feel alike.
    usage_months_scroll: ScrollHandle,
    usage_months_scrollbar: Rc<ScrollbarState>,
    /// Filter query over the Usage page's project rows.
    usage_project_filter: Entity<TextInput>,
    /// Virtualized list over the filtered project rows, so only visible rows
    /// build elements no matter how many working directories have usage.
    usage_projects_list: ListState,
    usage_projects_scrollbar: Rc<ScrollbarState>,
    /// Indices into `usage_history.projects` the filter leaves visible — the
    /// row builder reads only this.
    usage_projects_rows: RefCell<Vec<usize>>,
    /// `(peak value, rank-by-cost)` for the visible rows' bars, refreshed
    /// once per frame rather than per row.
    usage_projects_scale: Cell<(f64, bool)>,
    /// Hovered or keyboard-selected day index on the Usage page's chart.
    usage_chart_hover: Option<usize>,
    /// The chart plot's window bounds, written during paint so the mouse-move
    /// handler can map positions to day indices.
    usage_chart_bounds: Rc<Cell<Option<gpui::Bounds<Pixels>>>>,
    computer_use_app_icons: RefCell<HashMap<String, Option<std::sync::Arc<gpui::Image>>>>,
    computer_use_app_icon_loads: RefCell<HashSet<String>>,
    /// Installed folder-capable apps for the header's "open project in"
    /// control, icons included, resolved once at launch on the background
    /// executor. Render only reads this; empty means not resolved yet (or
    /// nothing to offer) and hides the control.
    open_in_apps: Rc<Vec<crate::platform::ExternalApp>>,
    /// Keyboard cursor over the model picker's filtered rows. `None` means the
    /// keyboard has not moved yet, so `enter` takes the first row.
    model_picker_highlight: Option<usize>,
    model_picker_list: ListState,
    model_picker_scrollbar: Rc<ScrollbarState>,
    /// The class-target picker's drawn selection and list state — same shape
    /// as the model picker's, shared by the three class menus.
    route_class_highlight: Option<usize>,
    route_class_scroll: ScrollHandle,
    route_class_scrollbar: Rc<ScrollbarState>,
    /// The class whose target picker is open — routes `enter` and the
    /// empty-query reveal to the right policy slot.
    route_class_picker: Option<waku_protocol::routing::TaskClass>,
    /// Focus for the picker's no-providers state. The panel takes focus on
    /// open so `escape` has a focused descendant to dispatch up from, and
    /// normally that is the filter field — which the empty state does not
    /// draw, so its one button holds focus instead.
    model_picker_empty_focus: FocusHandle,
    branch_search: Entity<TextInput>,
    branch_create_input: Entity<TextInput>,
    branch_picker_mode: BranchPickerMode,
    /// Keyboard cursor over the branch picker's enabled actions. Disabled
    /// rows remain visible but never enter this index.
    branch_picker_highlight: Option<usize>,
    branch_picker_list_state: ListState,
    branch_picker_row_cache: RefCell<Vec<crate::git_branch::BranchEntry>>,
    /// Name field in the composer worktree picker; empty means the daemon
    /// generates a random name.
    worktree_name_input: Entity<TextInput>,
    /// Keyboard cursor over the worktree picker's actions. `None` means the
    /// keyboard has not moved yet, so `enter` takes the first row.
    worktree_picker_highlight: Option<usize>,
    /// An eager worktree creation is in flight on the daemon.
    worktree_creation_pending: bool,
    /// Tasks with an in-flight move into a newly created worktree.
    worktree_move_pending: HashSet<Uuid>,
    /// Git subprocess results per concrete workspace path. Render only reads
    /// this in-memory cache; misses are fulfilled on the background executor.
    branch_snapshots: QueryCache<PathBuf, Result<Option<BranchSnapshot>, String>>,
    /// Stale-while-revalidate value for the selected path, avoiding label
    /// flicker when app activation invalidates the query.
    visible_branch_snapshot: Option<(PathBuf, BranchSnapshot)>,
    branch_operation_pending: bool,
    /// Window-modal Git commit/push UI. Its repository snapshot is filled
    /// off-thread; frames only read this in-memory value.
    commit_dialog: Option<commit_dialog::CommitDialogState>,
    /// Window-modal new-GitHub-issue UI opened from the command palette.
    /// The template scan runs in the palette; this is just the form.
    issue_dialog: Option<issue_dialog::IssueDialogState>,
    /// The issue the last `gh issue create` landed — what the success
    /// toast's "View" and ⌘⌥I open.
    last_created_issue: Option<issue_dialog::CreatedIssue>,
    /// The archive confirmation shown when a checkout still holds
    /// uncommitted or unpushed work; `archive_preview_pending` dedupes the
    /// background inspection that decides whether it opens.
    archive_dialog: Option<archive_dialog::ArchiveDialogState>,
    archive_preview_pending: HashSet<Uuid>,
    /// The keyboard-shortcut cheatsheet opened from the sidebar footer.
    shortcuts_dialog: Option<shortcuts_dialog::ShortcutsDialogState>,
    goal_dialog: Option<goal_dialog::GoalDialogState>,
    goal_dialog_request: Option<goal_dialog::GoalDialogRequest>,
    /// Goal operations accepted before the session's runtime exists. Goals
    /// attach to the provider thread, not to any turn, so `/goal` on a fresh
    /// task starts the provider and these drain once it installs.
    pending_goal_operations: HashMap<Uuid, Vec<crate::model::GoalOperation>>,
    /// Sessions whose runtime is being started by a goal operation rather
    /// than a submission. Submissions queue behind this instead of racing a
    /// second provider process into existence.
    goal_runtime_starts: HashSet<Uuid>,
    /// When each session's goal accounting was last reported. The chip adds
    /// the wall clock since then while an active goal's turn runs, so elapsed
    /// pursuit time ticks live the way the Codex CLI shows it.
    goal_observed_at: HashMap<Uuid, Instant>,
    /// Commit-message generation and Git mutation outlive the modal that
    /// started them. Keeping the operation on the app also lets every
    /// Environment surface reflect and gate the same in-flight action.
    commit_operation: Option<commit_dialog::CommitOperationState>,
    /// Slash commands discovered per (provider, project root, CLI override).
    /// Filesystem and CLI probes live off the UI thread; frames read this cache.
    slash_commands: QueryCache<(ProviderKind, PathBuf, Option<String>), Vec<SlashCommand>>,
    /// The merged command list the autocomplete popup draws, and the key it
    /// was built for — a stale key means "no commands", never another
    /// provider's list.
    slash_command_index: Rc<Vec<SlashCommand>>,
    slash_command_index_key: Option<(ProviderKind, PathBuf, Option<String>)>,
    slash_command_index_loading: bool,
    /// Workspace file index per project root, for `@` mentions.
    mention_files: QueryCache<PathBuf, Vec<FileEntry>>,
    mention_file_index: Rc<Vec<FileEntry>>,
    mention_file_index_path: Option<PathBuf>,
    mention_file_index_loading: bool,
    /// `#` work-item mention state per workspace root: resolved repo, latest
    /// landed search, and the number→item map for expansion and chips.
    work_item_mentions: HashMap<PathBuf, autocomplete::WorkItemMentions>,
    /// Set when a driver reports its command registry mid-drain; the drain
    /// has no `Context` to rebuild the drawn index itself.
    composer_sources_stale: bool,
    composer_autocomplete: autocomplete::AutocompleteUi,
    /// Files dropped onto the composer, drawn as chips above the input and
    /// drained into the next submission.
    composer_attachments: Vec<ComposerAttachment>,
    /// Text pastes too large for the field, held as collapsible cards above
    /// the input and spliced back into the next submission verbatim. Purely
    /// view state: drafts capture their text inline instead.
    composer_pasted_blocks: Vec<String>,
    /// Window-modal expansion of an image attachment. The path is already
    /// cached attachment metadata; render never probes the filesystem.
    image_preview: Option<image_preview::ImagePreviewState>,
    image_preview_generation: u64,
    /// In-memory GPUI images for daemon-owned bytes. A missing entry schedules
    /// one background fetch only when a visible row asks to render it; the
    /// desktop never creates another attachment file.
    remote_images: RefCell<HashMap<String, RemoteImageState>>,
    /// Coalesced edge trigger for provider and background result queues. The
    /// payloads stay in their typed channels; this channel only wakes the UI.
    event_wake_tx: smol::channel::Sender<()>,
    task_state_sync_tx: Sender<(
        waku_client::DaemonKey,
        Result<RemoteTaskStateSnapshot, String>,
    )>,
    task_state_sync_events: Receiver<(
        waku_client::DaemonKey,
        Result<RemoteTaskStateSnapshot, String>,
    )>,
    /// `settingsChanged` broadcasts forwarded by the task-state sync worker:
    /// the authoritative daemon document each time another client — or an
    /// agent — rewrites it.
    daemon_settings_tx: Sender<(waku_client::DaemonKey, waku_client::DaemonSettings)>,
    daemon_settings_events: Receiver<(waku_client::DaemonKey, waku_client::DaemonSettings)>,
    /// `friendsChanged` broadcasts forwarded by the task-state sync worker:
    /// the authoritative daemon document each time a friend request, offer,
    /// or transfer update lands.
    friends_state: waku_client::friends::FriendsState,
    friends_tx: Sender<waku_client::friends::FriendsState>,
    friends_events: Receiver<waku_client::friends::FriendsState>,
    /// The Settings → Friends "add friend" code field.
    friend_code_input: Entity<TextInput>,
    /// The Settings → Friends display-name field — the name friends see on
    /// our requests and offers.
    friend_name_input: Entity<TextInput>,
    /// Shared single-line editor for a friend's local nickname; one friend
    /// row borrows it at a time.
    friend_nickname_input: Entity<TextInput>,
    /// Node id of the friend whose nickname is being edited, if any.
    editing_friend_nickname: Option<String>,
    /// Generation guard for the while-open presence re-probe loop — a new
    /// loop (or leaving the page) retires the previous one.
    friends_probe_generation: Cell<u64>,
    /// GetRoutePolicy answers (and SetRouteClassTarget write+refreshes)
    /// landing for the settings surface's routing section.
    route_policy_tx: Sender<Result<waku_protocol::routing::RoutePolicyView, String>>,
    route_policy_events: Receiver<Result<waku_protocol::routing::RoutePolicyView, String>>,
    /// The newest policy view the settings page has; `None` until a fetch
    /// answers, which also covers "routing not supported yet".
    route_policy: Option<waku_protocol::routing::RoutePolicyView>,
    route_policy_pending: bool,
    runtimes: HashMap<Uuid, SessionRuntime>,
    runtime_attach_pending: HashSet<Uuid>,
    runtime_attach_misses: HashMap<Uuid, u8>,
    /// Provider-neutral session work which may remain live after a turn ends.
    /// Runtime-only by design: providers reconcile their authoritative state
    /// when the resident transport reconnects.
    background_work: HashMap<Uuid, BackgroundWorkRegistry>,
    last_background_work_tick: Instant,
    /// Accepted submissions still creating their workspace/checkpoint, or an
    /// edited past message still rewinding its workspace and provider. The
    /// session is busy immediately, while the composer draws a spinner until
    /// the non-cancellable preparation is complete.
    submission_preparations: HashSet<Uuid>,
    /// First Escape press for the current turn. A matching second press stops
    /// the response; otherwise this returns to the ordinary Stop icon after a
    /// short timeout.
    escape_stop_confirmation: EscapeStopConfirmation,
    /// Response fork target per source session while its provider-native
    /// branch and Git checkpoint refs are being prepared off the UI thread.
    /// A source can have only one in flight because Pi temporarily changes
    /// its resident session while producing a branch.
    response_fork_preparations: HashMap<Uuid, usize>,
    /// Sessions whose just-settled turn should start the next queued
    /// follow-up. The request stays here until the ending checkpoint lands, so
    /// the next provider cannot edit the worktree while that snapshot is still
    /// being collected; it then reuses the runtime after the event drain has
    /// re-inserted it.
    pending_queue_drains: Vec<Uuid>,
    /// Archived sessions whose worktrees still need snapshotting and removal.
    /// Entries wait here while the session could still write into the
    /// worktree — a settling turn, an in-flight submission preparation, live
    /// detached work, or a queued turn-checkpoint capture — and drain once it
    /// goes quiet. Unarchived or removed sessions drop out on the next pass.
    pending_workspace_cleanups: HashSet<Uuid>,
    stream_state_dirty: bool,
    last_stream_save: Instant,
    /// User expansion overrides keyed by persisted transcript block index.
    activities_expanded: HashMap<usize, bool>,
    /// Per-item disclosure overrides. Reasoning starts open while live; tool
    /// details start closed, so the stored bool must preserve either choice.
    expanded_activity_items: HashMap<Uuid, bool>,
    /// Settled turns whose folded work the user has reopened.
    expanded_turns: HashSet<Uuid>,
    /// Per-response file cards the user expanded beyond their three-file
    /// preview. Runtime-only, like the other transcript disclosures.
    expanded_changed_files: HashSet<Uuid>,
    /// The changed-files row under the pointer — and its floating diff card
    /// once open — or `None` when neither holds the pointer.
    changed_files_diff_hover: Option<ChangedFilesDiffHover>,
    /// Per-turn parsed diffs backing the hover preview. One fetch covers
    /// every file row in a card and is reused across hovers.
    changed_files_diffs: HashMap<Uuid, ChangedFilesDiff>,
    /// Guards the delayed open and the grace-period close: a timer whose
    /// generation no longer matches must not act.
    changed_files_diff_generation: u64,
    /// Stable focus identities for controls inside virtualized transcript and
    /// diff rows. Recreating a handle on every row build would drop keyboard
    /// focus whenever GPUI re-renders the list.
    transcript_control_focuses: RefCell<HashMap<String, FocusHandle>>,
    session_navigation: SessionNavigation,
    /// Sidebar task currently showing its inline rename field.
    session_rename: Option<Uuid>,
    /// Sidebar terminal currently showing its inline rename field — the
    /// same `session_rename_input` editor serves both rows.
    terminal_rename: Option<Uuid>,
    /// One stable field reused across sidebar rows so virtualization never
    /// replaces the focused editor while a rename is in progress.
    session_rename_input: Entity<TextInput>,
    /// Groups the user has folded in either sidebar view. This is
    /// intentionally runtime-only, like transcript disclosure state.
    sidebar_collapsed_groups: HashSet<SidebarGroup>,
    /// While the primary modifier is held past
    /// [`sidebar::SIDEBAR_SHORTCUT_HOLD_DELAY`], the sidebar's first nine
    /// visible tasks wear their ⌘1–⌘9 shortcut chips.
    sidebar_shortcut_hints: bool,
    /// Bumped on every primary-modifier change so a reveal timer armed before
    /// release cannot fire after it.
    sidebar_shortcut_hint_generation: u64,
    /// Set when a ⌘-modified keystroke lands mid-hold: the modifier was spent
    /// on a chord, so the chips stay down until ⌘ comes all the way back up.
    sidebar_shortcut_hint_chord_used: bool,
    /// Number of older sessions revealed inside each project section. This is
    /// runtime-only so every launch starts with the recent three-day view.
    sidebar_project_reveal_counts: HashMap<SidebarGroup, usize>,
    /// Stable keyboard focus for each virtualized sidebar group header and
    /// its hover-revealed New Task control.
    sidebar_group_header_focuses: RefCell<HashMap<SidebarGroup, FocusHandle>>,
    sidebar_group_compose_focuses: RefCell<HashMap<SidebarGroup, FocusHandle>>,
    /// Stable keyboard focus for each session row's hover-revealed archive
    /// control.
    sidebar_session_archive_focuses: RefCell<HashMap<Uuid, FocusHandle>>,
    /// Stable keyboard focus for each session row's hover-revealed pin
    /// control.
    sidebar_session_pin_focuses: RefCell<HashMap<Uuid, FocusHandle>>,
    /// Stable keyboard focus for each terminal row's hover-revealed close
    /// control.
    sidebar_terminal_close_focuses: RefCell<HashMap<Uuid, FocusHandle>>,
    /// Stable keyboard focus for each terminal row's hover-revealed pin
    /// control.
    sidebar_terminal_pin_focuses: RefCell<HashMap<Uuid, FocusHandle>>,
    /// Stable keyboard focus for each virtualized project-history reveal row.
    sidebar_show_more_focuses: RefCell<HashMap<SidebarGroup, FocusHandle>>,
    sidebar_visible: bool,
    sidebar_width: f32,
    right_panel_visible: bool,
    right_panel_width: f32,
    /// The Git panel shares the right panel's slot and never shows with it:
    /// opening one dismisses the other. See `git_panel.rs`.
    git_panel_visible: bool,
    /// The persisted height of the panel's top region — commit box or open
    /// commit's file tree — before the frame's viewport clamp applies.
    git_panel_top_height: f32,
    git_panel: Option<git_panel::GitPanelState>,
    /// The commit/push/sync the panel's action button is running, if any.
    git_panel_operation: Option<git_panel::GitPanelOperation>,
    /// Guards snapshot/commit fetches against superseded panel state.
    git_panel_generation: u64,
    /// Per-file hover previews, keyed by path and section. A partially staged
    /// file caches its staged and unstaged previews separately.
    git_panel_file_diffs: HashMap<(String, bool), git_panel::GitPanelFileDiff>,
    git_panel_hover: Option<git_panel::GitPanelDiffHover>,
    git_panel_hover_generation: u64,
    git_panel_commit_hover: Option<git_panel::GitPanelCommitHover>,
    git_panel_commit_hover_generation: u64,
    /// A commit SHA under the transcript pointer, and the metadata fetched
    /// for SHAs seen in the selected session.
    transcript_commit_hover: Option<git_panel::TranscriptCommitHover>,
    transcript_commit_details: HashMap<String, git_panel::TranscriptCommitDetail>,
    transcript_commit_press: Option<git_panel::TranscriptCommitPress>,
    /// The conflicted-sync modal: which integration is stopped mid-flight.
    git_panel_sync_conflict: Option<git_panel::SyncConflict>,
    /// Scroll state for the conflict modal's file list.
    git_panel_conflict_files_scroll: ScrollHandle,
    git_panel_conflict_files_scrollbar: Rc<ScrollbarState>,
    /// The "nothing staged" prompt's open flag.
    git_panel_unstaged_prompt: bool,
    /// The commit-diff modal, when a commit row is open.
    git_panel_commit_diff: Option<git_panel::GitPanelCommitDiff>,
    /// Focus target the Git panel's modals share — only one is ever open.
    git_panel_modal_focus: FocusHandle,
    /// Focus targets for the modal buttons so each is tabbable; distinct
    /// handles per modal keep a stacked pair from fighting over one.
    git_panel_unstaged_cancel_focus: FocusHandle,
    git_panel_unstaged_confirm_focus: FocusHandle,
    git_panel_conflict_abort_focus: FocusHandle,
    git_panel_conflict_merge_focus: FocusHandle,
    git_panel_conflict_resolve_focus: FocusHandle,
    /// A Git panel file row sent to the Review surface; applied to the
    /// surface's next snapshot landing.
    right_panel_pending_diff_file: Option<String>,
    /// The show/hide slide each panel is in the middle of, if any. Driven by
    /// hand from `render` (see [`motion::WidthTween`]) because the width these
    /// produce is what the transcript column between them is laid out against.
    sidebar_slide: Option<motion::WidthTween>,
    right_panel_slide: Option<motion::WidthTween>,
    /// Width each panel actually occupied in the last frame — where a toggle
    /// starts its slide from, and what the transcript measures itself against
    /// while one is running.
    sidebar_rendered_width: f32,
    right_panel_rendered_width: f32,
    /// The closed sidebar's hover-peek overlay — see [`SidebarPeek`].
    sidebar_peek: SidebarPeek,
    /// A hover exit the peek overlay deferred because a menu card was open
    /// above it. Settled once no menu is open by re-checking the pointer.
    sidebar_peek_menu_hold: bool,
    /// A row action (pin, archive) ran from the peek-mounted sidebar, so the
    /// deferred exit at menu close is suppressed — the overlay stays until
    /// the pointer next enters and leaves it.
    sidebar_peek_action_hold: bool,
    /// The right-panel surface currently maximized over the window, if any —
    /// runtime-only; the docked layout it covers comes back exactly as it
    /// was. The path of the file shown at entry rides alongside so a
    /// Files-surface reselection also ends the mode.
    fullscreen_surface: Option<(RightPanelSurface, Option<String>)>,
    /// The slide animating the fullscreen layer's width in or out, if any.
    panel_fullscreen_slide: Option<motion::WidthTween>,
    /// Width the fullscreen layer occupies this frame — the docked content
    /// width while inactive, so a toggle starts its slide from the panel's
    /// real edge.
    panel_fullscreen_rendered_width: f32,
    fps_counter_visible: bool,
    panel_resize_drag: Option<PanelResizeDrag>,
    /// Window-relative PiP position, independent of incoming preview frames.
    computer_use_preview_position: Option<gpui::Point<Pixels>>,
    right_panel_session_states: HashMap<Uuid, RightPanelSessionState>,
    /// Panel state parked while no task owns the strip — a full-width
    /// terminal or the Projects page. Swapped in and out exactly like a
    /// session's, so the detached context keeps its own tabs and visibility.
    right_panel_detached_state: RightPanelSessionState,
    right_panel_surfaces: Vec<RightPanelSurface>,
    right_panel_active_surface: Option<usize>,
    right_panel_tabs_scroll_handle: ScrollHandle,
    right_panel_files_scroll_handle: ScrollHandle,
    right_panel_files_scrollbar: Rc<ScrollbarState>,
    right_panel_diff_filter: Entity<TextInput>,
    /// Unified diff rows and changed-file tree rows are independently
    /// virtualized. Large generated patches stay proportional to what is on
    /// screen rather than the size of the repository change.
    right_panel_diff_list_state: ListState,
    right_panel_diff_scrollbar: Rc<ScrollbarState>,
    /// Selection spans and visible glyph geometry for the Review surface.
    /// Kept separate from the transcript because both surfaces paint at once.
    right_panel_diff_selection: TranscriptSelection,
    right_panel_diff_tree_list_state: ListState,
    right_panel_diff_tree_scrollbar: Rc<ScrollbarState>,
    right_panel_editor_scroll_handle: ScrollHandle,
    right_panel_editor_scrollbar: Rc<ScrollbarState>,
    /// Rendered-markdown preview of the visible file editor, cached per path
    /// the way `skills_detail_markdown` caches the skill document.
    file_preview_markdown: RefCell<Option<(String, MarkdownView)>>,
    file_preview_selection: TranscriptSelection,
    file_preview_scroll_handle: ScrollHandle,
    file_preview_scrollbar: Rc<ScrollbarState>,
    right_panel_pending_tab_reveal: Option<usize>,
    /// A file the `Cmd+P` finder just opened — or a `file:line` link or
    /// go-to-line jump aimed at — whose editor should take keyboard focus on
    /// the first frame the entity exists, carrying the `line[:column]` jump
    /// target when the requester had one.
    right_panel_pending_file_focus: Option<PendingFileFocus>,
    right_panel_pending_terminal_focus: Option<Uuid>,
    /// Terminal surface that most recently held focus. Swapped in and out with
    /// the rest of the per-session panel state.
    right_panel_last_focused_terminal: Option<Uuid>,
    right_panel_expanded_paths: HashSet<PathBuf>,
    right_panel_files_selected_path: Option<String>,
    right_panel_file_tree_width: f32,
    right_panel_file_editors: HashMap<String, RightPanelFileEditor>,
    /// Per-tab chrome for open pull-request surfaces, keyed `(session id, PR
    /// number)` — scroll, focus, and the tab's own comment composer. Not
    /// swapped through `RightPanelSessionState`; the key scopes it instead.
    right_panel_pr_states: HashMap<(Uuid, u64), github::GitHubDetailChrome>,
    /// Find-and-replace over the visible file editor. Created on first use of
    /// the primary find shortcut and kept for the window's lifetime so the
    /// query and toggles survive closing the bar; `open` says whether it shows.
    file_search: Option<file_search::FileSearch>,
    /// The ctrl-g "go to line" bar over the visible file editor — same
    /// lifecycle as `file_search`.
    go_to_line: Option<go_to_line::GoToLine>,
    right_panel_diff_source: ReviewDiffSource,
    right_panel_diff_snapshot: Option<Arc<ReviewDiffSnapshot>>,
    right_panel_diff_loading: bool,
    right_panel_diff_error: Option<String>,
    right_panel_diff_generation: u64,
    right_panel_diff_selected_file: Option<usize>,
    right_panel_diff_expanded_paths: HashSet<String>,
    right_panel_diff_tree_rows: RefCell<Vec<right_panel::ReviewDiffTreeRow>>,
    right_panel_diff_tree_cursor: Option<usize>,
    /// The working tree as currently drawn. Held so a refresh can redraw the
    /// previous listing instead of blanking the panel.
    right_panel_working_tree: Vec<right_panel::WorkingTreeEntry>,
    /// Working tree per project path. Walking it is filesystem I/O and must
    /// never happen in a frame.
    working_trees: QueryCache<PathBuf, Vec<right_panel::WorkingTreeEntry>>,
    /// Set when a turn finishes; the drain loop drops the workspace queries,
    /// since the event handler has no `Context` to refresh them itself.
    workspace_queries_stale: bool,
    right_panel_terminals: HashMap<Uuid, Entity<TerminalView>>,
    /// Every terminal the sidebar's Terminals group lists — session-scoped
    /// and global alike — keyed by the terminal surface's id.
    /// `terminal_order` carries the flat list's creation order.
    terminal_records: HashMap<Uuid, TerminalRecord>,
    terminal_order: Vec<Uuid>,
    /// Terminals whose last command finished successfully while the
    /// surface was off-screen — the sidebar row's unread dot until the
    /// terminal next takes focus.
    unseen_terminal_completions: HashSet<Uuid>,
    /// The terminal filling the main area. Set only while no session is
    /// selected; activating a chat clears it and folds the group.
    selected_terminal: Option<Uuid>,
    /// The terminal last shown in the main area. Survives the group's
    /// collapse so re-expanding lands on it again.
    last_visible_terminal: Option<Uuid>,
    /// The custom command a terminal surface was opened for, keyed by the
    /// surface's terminal id. Absent for plain shell terminals; the entry
    /// tells `ensure_right_panel_terminal` how to spawn the PTY and carries
    /// the command's close-on-success choice.
    right_panel_terminal_commands: HashMap<Uuid, CustomCommand>,
    /// Custom commands launched with the panel kept closed, keyed by the
    /// surface's terminal id. The entry retires when the command reports
    /// its exit or the terminal goes away.
    custom_command_runs: HashMap<Uuid, PendingCommandRun>,
    /// Install/sign-in terminals running inside the Providers settings page,
    /// keyed by the provider they set up. They live outside the right panel
    /// surfaces because Settings covers the workspace while they run.
    provider_setup_terminals: HashMap<ProviderKind, Entity<TerminalView>>,
    /// The PTY running an Antigravity session's TUI, keyed by session id.
    /// It is the session's main surface — not a right-panel tab — and it
    /// exists only while the process does.
    agy_terminals: HashMap<Uuid, Entity<TerminalView>>,
    /// The last time each live Antigravity terminal was on screen. The idle
    /// sweep frees a process only once this exceeds the grace window while
    /// the session is deselected and idle.
    agy_last_visible: HashMap<Uuid, Instant>,
    /// Spawn time per live Antigravity terminal — the bound conversation-id
    /// discovery compares summary rows against.
    agy_spawned_at: HashMap<Uuid, u64>,
    /// Sessions whose TUI spawn ran before provider detection finished and
    /// found no `agy` probe yet; the poll tick retries them once probes land.
    agy_pending_spawns: HashSet<Uuid>,
    /// Poller results land here like every other background queue; a single
    /// `agy_poll_pending` flag keeps one poll in flight at a time.
    agy_poll_tx: Sender<agy::AgyPollUpdate>,
    agy_poll_events: Receiver<agy::AgyPollUpdate>,
    agy_poll_pending: bool,
    right_panel_browsers: HashMap<Uuid, Entity<BrowserView>>,
    /// A Browser surface was just opened; the next right panel render moves
    /// focus into its address bar.
    right_panel_pending_browser_focus: Option<Uuid>,
    /// GPUI is compositing deferred draws on a plane above native views, so
    /// menus render over the live webview and no snapshot occlusion is needed.
    /// When the overlay could not be enabled, the browser falls back to
    /// swapping in frozen page pixels while an overlay is open.
    scene_overlay_enabled: bool,
    settings_page: Option<SettingsPage>,
    /// `GODDARD_DEV_STATE` — set only when the dev watcher launched this app.
    /// The command palette's auto-restart toggle writes to this file, which
    /// the watcher reads after each rebuild.
    dev_state_path: Option<PathBuf>,
    /// The toggle's current value, mirrored from `dev_state_path` at launch
    /// and on each palette flip.
    auto_restart_enabled: bool,
    /// Lazily built with the Keybindings page so the persistence service and
    /// search subscription only exist while the surface is in use.
    keybindings: Option<keybindings_page::KeybindingsUi>,
    /// The Commands settings page's open editor; `None` shows the list.
    custom_command_editor: Option<settings::CustomCommandEditor>,
    /// The Daemon page's open remote-host editor; `None` shows the list.
    remote_host_editor: Option<settings::RemoteHostEditor>,
    /// The Skills page's library snapshot, scanned off-thread. Frames read
    /// only this; `None` means the first scan has not landed yet.
    skills_catalog: Option<Rc<crate::skills::SkillsCatalog>>,
    /// Per-daemon catalog slices the merged `skills_catalog` is rebuilt
    /// from, so an offline host keeps its last-known rows.
    skills_catalogs: HashMap<waku_client::DaemonKey, Rc<crate::skills::SkillsCatalog>>,
    /// Which daemon owns each skill's primary directory — mutations route
    /// through it and rows badge remote entries.
    skill_hosts: HashMap<PathBuf, waku_client::DaemonKey>,
    /// Bumped per scan; a result from a superseded scan is discarded.
    skills_scan_generation: u64,
    skills_scan_pending: bool,
    /// When the current catalog landed, for the reopen-staleness check.
    skills_scanned_at: Option<Instant>,
    /// Filter query over the Skills page's rows.
    skills_search: Entity<TextInput>,
    /// Virtualized list over the filtered skill rows.
    skills_list_state: ListState,
    skills_scrollbar: Rc<ScrollbarState>,
    /// The rows the list currently draws — sections and catalog indices —
    /// refreshed once per frame rather than per row.
    skills_rows: RefCell<Vec<skills_page::SkillsRow>>,
    /// The skill directory the detail pane shows. `None` falls back to the
    /// first visible row, so the pane never opens empty.
    skills_selected: Option<PathBuf>,
    /// Parsed markdown for the selected skill's document, keyed by the skill
    /// directory it was built from. One entry: only one detail shows at once.
    skills_detail_markdown: RefCell<Option<(PathBuf, MarkdownView)>>,
    /// Text selection over the detail pane's rendered document. Its own
    /// registry, like the toast's, so it can never join a drag to another
    /// surface's text.
    skills_selection: TranscriptSelection,
    /// Scroll position of the detail pane, tracked so it can draw a
    /// scrollbar and land at the top when the selection moves.
    skills_detail_scroll: ScrollHandle,
    skills_detail_scrollbar: Rc<ScrollbarState>,
    /// Source the list is narrowed to; `None` shows every ecosystem.
    skills_source_filter: Option<crate::skills::SkillSource>,
    /// The skill directory whose delete button is armed for its confirming
    /// second click.
    skills_delete_arming: Option<PathBuf>,
    /// Scroll position of the settings content column, tracked so the pane
    /// can draw a scrollbar and mark the titlebar boundary once content
    /// slides under it.
    settings_scroll: ScrollHandle,
    settings_scrollbar: Rc<ScrollbarState>,
    /// Sections the settings search rendered, in scroll order — each is a
    /// direct child of the content scroll element, so its index here is the
    /// `scroll_to_top_of_item` target the sidebar and arrow keys use.
    settings_search_sections: Vec<SettingsPage>,
    /// The section the last search-mode navigation landed on; arrow cycling
    /// steps from it rather than from the selected page.
    settings_search_target: Option<SettingsPage>,
    /// Filter query over the Archived Chats page's rows.
    archived_search: Entity<TextInput>,
    /// Project the archived list is narrowed to; `None` shows every project.
    archived_project_filter: Option<Uuid>,
    /// Virtualized list over the filtered archived rows, so only visible
    /// rows build elements no matter how long the archive grows.
    archived_sessions_list: ListState,
    archived_sessions_scrollbar: Rc<ScrollbarState>,
    /// Session ids the search and project filters leave visible, newest
    /// archived first — the row builder reads only this.
    archived_session_rows: RefCell<Vec<Uuid>>,
    /// The completion-volume slider's in-flight drag, kept on the entity so a
    /// repaint mid-gesture cannot drop it.
    completion_volume_slider: Rc<SliderState>,
    /// The sidebar-transparency slider's in-flight drag, same reason.
    sidebar_transparency_slider: Rc<SliderState>,
    /// The border-intensity slider's in-flight drag, same reason.
    border_intensity_slider: Rc<SliderState>,
    /// Set while a settings menu is previewing a theme it has not committed;
    /// the persisted settings go back on screen when the menu dismisses.
    theme_preview_active: bool,
    /// The Appearance page's code/chat sample, opened on demand; a theme
    /// selector opening also reveals it for the duration of the pick.
    theme_preview_expanded: bool,
    header_drag_armed: bool,
    toast: Option<ToastState>,
    toast_generation: u64,
    /// Localhost URLs detected in terminal output while another toast owns
    /// the slot. Promoted in order as the slot frees.
    pending_localhost_toasts: VecDeque<PendingLocalhostUrl>,
    copied_control_feedback: HashMap<String, u64>,
    copied_control_generation: u64,
    copied_message_feedback: HashMap<Uuid, u64>,
    copied_message_generation: u64,
    copied_activity_feedback: HashMap<(Uuid, ActivityDisclosureSectionKind), u64>,
    copied_activity_generation: u64,
    message_edit: Option<MessageEdit>,
    transcript_rows: ListState,
    /// Active turns use top alignment so row remeasurement cannot invoke the
    /// bottom-aligned list's implicit pin and displace the sent-message anchor.
    anchored_transcript_rows: ListState,
    /// Virtualized list backing the sidebar session history, so only visible
    /// rows are built and laid out regardless of how many sessions exist.
    sidebar_list_state: ListState,
    sidebar_scrollbar: Rc<ScrollbarState>,
    /// Snapshot of the sidebar rows the list state currently corresponds to.
    sidebar_row_cache: RefCell<Vec<SidebarRow>>,
    /// Fingerprint + snapshot pair backing `sidebar_rows_cached`.
    sidebar_rows_fingerprint: Cell<Option<u64>>,
    sidebar_rows_snapshot: RefCell<Rc<Vec<SidebarRow>>>,
    /// Member session ids per collapsed group, rebuilt with the row
    /// snapshot so a folded header can aggregate its hidden rows' unread
    /// state without re-running the grouping.
    sidebar_collapsed_group_members: RefCell<Rc<HashMap<SidebarGroup, Vec<Uuid>>>>,
    /// Branch labels for ordinary local project paths, resolved together on a
    /// background executor so sidebar rows only read memory.
    sidebar_branch_labels: RefCell<HashMap<PathBuf, SharedString>>,
    sidebar_branch_scan_fingerprint: Cell<Option<u64>>,
    sidebar_branch_scan_generation: Cell<u64>,
    /// Nearest repository root per terminal working directory — `None` for
    /// a directory outside any checkout — resolved together on a
    /// background executor so sidebar rows only read memory.
    sidebar_terminal_repo_roots: RefCell<HashMap<PathBuf, Option<PathBuf>>>,
    sidebar_terminal_repo_scan_fingerprint: Cell<Option<u64>>,
    sidebar_terminal_repo_scan_generation: Cell<u64>,
    /// Dirty flag + unpushed commit count per session checkout or worktree
    /// path, resolved together on a background executor so sidebar rows only
    /// read memory. Git state drifts without any session-set change, so the
    /// scan also reruns on a cadence — `sidebar_checkout_scanned_at`.
    sidebar_checkout_statuses: RefCell<HashMap<PathBuf, crate::git_commit::CheckoutStatus>>,
    sidebar_checkout_scan_fingerprint: Cell<Option<u64>>,
    sidebar_checkout_scan_generation: Cell<u64>,
    sidebar_checkout_scanned_at: Cell<Option<Instant>>,
    /// Pull requests resolved per session on a background executor, keyed by
    /// session id. A session absent from the map means "not known yet" — the
    /// row renders no badge, same as a session with no pull requests.
    sidebar_pull_requests:
        RefCell<HashMap<Uuid, Rc<Vec<waku_protocol::workspace::PullRequestSummary>>>>,
    sidebar_pull_request_scan_fingerprint: Cell<Option<u64>>,
    sidebar_pull_request_scan_generation: Cell<u64>,
    /// GitHub list/detail state keyed by project id, backing the Projects
    /// page's Issues and Pull Requests tabs — state survives the view
    /// toggling back to the transcript.
    github_browsers: HashMap<Uuid, github::GitHubBrowser>,
    /// The GitHub notification inbox — user-level, not project-level, so it
    /// lives here rather than in `github_browsers`. See
    /// [`notifications::Inbox`].
    notifications: notifications::Inbox,
    /// The Projects page's own project selection — `Some` while the page
    /// claims the main column — independent of `state.selected_project`.
    projects_page: Option<Uuid>,
    /// The project the page last showed, so reopening lands where the user
    /// left it instead of falling back to task recency.
    last_projects_page_project: Option<Uuid>,
    /// Per-project page state kept across page toggles.
    projects_page_states: HashMap<Uuid, projects::ProjectsPageState>,
    /// The Drafts page claiming the main column, like `projects_page`.
    drafts_page: bool,
    /// Drafts whose "Use" is still undoable: each was consumed from the
    /// list, and ⌘Z puts it back and returns to the page.
    draft_use_undos: Vec<saved_drafts::DraftUseUndo>,
    /// Filter query over the Drafts page's cards.
    drafts_search: Entity<TextInput>,
    /// The page's shown/hidden switch — hidden drafts only appear under
    /// the Hidden view.
    drafts_show_hidden: bool,
    /// Virtualized list over the filtered draft cards.
    drafts_list_state: ListState,
    drafts_scrollbar: Rc<ScrollbarState>,
    /// The draft ids the current filter leaves visible, newest first —
    /// refreshed once per frame so card builders read only this.
    drafts_rows: RefCell<Vec<Uuid>>,
    /// The card in inline-edit mode; its field lives in
    /// `drafts_edit_input`.
    drafts_editing: Option<Uuid>,
    drafts_edit_input: Entity<TextInput>,
    /// The Settings → Git page's project selection — which repo's worktrees
    /// and branches the page lists.
    settings_git_project: Option<Uuid>,
    /// Set when the Git page's data should be (re)fetched on its next
    /// render — opening the page or switching its project. Cleared once
    /// `projects_refresh` runs with state in place.
    git_page_refresh_pending: bool,
    /// Projects whose stored path the last `refresh_project_locations` pass
    /// could not find — by missing folder, not by missing entity. The
    /// Projects page and sidebar badge read the set; submissions check the
    /// path again rather than trusting it.
    missing_projects: HashSet<Uuid>,
    /// Guards `refresh_project_locations`: a pass that started before a
    /// manual relocation discards its stale outcomes.
    project_location_generation: Cell<u64>,
    transcript_row_kinds: RefCell<Vec<TranscriptRowKind>>,
    /// Fingerprint of the transcript inputs `transcript_row_kinds` was folded
    /// from, so an unchanged transcript costs nothing on a frame. `None` until
    /// the first fold. See `transcript_rows_fingerprint`.
    transcript_row_kinds_fingerprint: Cell<Option<u64>>,
    /// The session whose last fold included the working indicator — the
    /// settle transition that arms the fade is "same session, indicator
    /// dropped". `None` once the row retires or the session switches.
    working_indicator_session: Cell<Option<Uuid>>,
    /// A settled turn's indicator stays mounted for
    /// [`WORKING_INDICATOR_FADE_OUT`] while the row renderer fades it out and
    /// schedules the splice that retires it.
    working_indicator_fade: Cell<Option<WorkingIndicatorFade>>,
    /// The navigation rail's turn list, shared by `Rc` so a frame hands the
    /// rail a pointer instead of re-extracting every turn's snippets. Rebuilt
    /// by `navigation_turns` when the row-kinds fingerprint moves.
    transcript_navigation_turns: RefCell<Rc<Vec<TranscriptNavigationTurn>>>,
    /// The row-kinds fingerprint `transcript_navigation_turns` was built from.
    transcript_navigation_turns_fingerprint: Cell<Option<u64>>,
    /// Response-footer copy content and completion time per message index,
    /// rebuilt when the row-kinds fingerprint moves. The row builder asks for
    /// every visible row on every frame, and the underlying turn walk and
    /// answer join are O(session). Footers exist only for settled turns,
    /// whose parts are immutable, and settling moves the fingerprint.
    assistant_footer_cache: RefCell<HashMap<usize, (Option<SharedString>, Option<u64>)>>,
    /// The row-kinds fingerprint `assistant_footer_cache` was built under.
    assistant_footer_fingerprint: Cell<Option<u64>>,
    /// The response row currently under the pointer. Response footers are
    /// separate virtual-list rows, so GPUI's ancestor-scoped `group_hover`
    /// cannot reveal them when a sibling response row is hovered.
    hovered_response_row: Option<(Uuid, TranscriptRowKind)>,
    /// Checkpoint-ref existence per (session, retained turn count), filled by
    /// `prefetch_checkpoint_refs` on the background executor. Rows read only
    /// this cache: resolving a ref forks a `git` subprocess, which must stay
    /// off the frame path.
    checkpoint_ref_cache: RefCell<HashMap<(Uuid, usize), bool>>,
    /// Bumped whenever checkpoint refs may have changed. A prefetch launched
    /// under an older generation is stale and discarded on arrival.
    checkpoint_ref_generation: Cell<u64>,
    /// The (session, generation) the latest scheduled prefetch covers.
    checkpoint_ref_prefetch: Cell<Option<(Uuid, u64)>>,
    /// Turn checkpoints asked for but not started yet.
    ///
    /// `capture_turn` is upwards of ten `git` invocations, one of them a
    /// `git add -A` over the whole worktree, and the driver-event drain that
    /// asks for it shares the UI thread with rendering. Requests queue here and
    /// `start_pending_checkpoint_captures` runs them on the background executor.
    pending_checkpoint_captures: Vec<PendingCheckpointCapture>,
    /// The (session, turn) captures currently running, so a repeated request —
    /// a turn that finishes while its own capture is still going — does not
    /// fork a second `git add -A` over the same worktree.
    checkpoint_captures_in_flight: HashSet<(Uuid, usize)>,
    /// Clock for the idle-session sweep, so the check costs one comparison per
    /// frame instead of a scan.
    last_idle_session_sweep: Instant,
    transcript_anchor: Cell<Option<TranscriptAnchor>>,
    transcript_anchor_end_space: Rc<Cell<Pixels>>,
    transcript_anchor_following: Rc<Cell<bool>>,
    /// A wheel scroll has landed and where it came to rest is not classified
    /// yet. The first frame that can measure the tail consumes this and
    /// re-engages following when the reader scrolled back onto it; a frame that
    /// cannot measure the tail leaves it set, so a stream remeasure cannot
    /// swallow the re-engage.
    transcript_tail_recheck: Rc<Cell<bool>>,
    transcript_is_scrolled: Rc<Cell<bool>>,
    /// The last wheel scroll on either transcript list. Scrolling slides rows
    /// under a stationary pointer and those hover transitions are not intent,
    /// so the changed-files diff preview waits for scroll quiet before opening
    /// or retargeting.
    transcript_last_wheel_scroll: Rc<Cell<Option<Instant>>>,
    /// The scroll position each session held when the reader left it, so
    /// back/forward history can restore where the transcript was instead of
    /// picking a fresh landing.
    transcript_scroll_positions: HashMap<Uuid, ListOffset>,
    /// The landing the current session's activation chose. A runtime attach
    /// that lands after activation resets the rows again, so the same landing
    /// is re-applied there rather than snapping the transcript to its tail.
    transcript_landing: Option<(Uuid, TranscriptLanding)>,
    /// Sessions whose persisted scroll position still claims the first
    /// post-launch activation; consumed by `apply_transcript_landing` so a
    /// later plain visit lands at the last prompt like usual.
    startup_scroll_restores: HashSet<Uuid>,
    /// The sidebar scroll offset waiting for the list's first rows.
    pending_sidebar_scroll: Cell<Option<ListOffset>>,
    /// Last decided visibility of the scroll-to-tail affordance. The tail's
    /// position is unknowable on the frames a stream commit remeasures it, and
    /// those arrive at commit cadence — deciding "show" from that silence
    /// strobes the button against the frames in between.
    transcript_scroll_to_bottom_visible: Cell<bool>,
    /// Whether the transcript's scrollbar thumb was held at the last frame, so
    /// render can notice a drag starting and ending.
    transcript_scrollbar_dragging: Cell<bool>,
    transcript_layout_width: Cell<Pixels>,
    /// Parsed markdown per assistant message, keeping each response's
    /// incremental parse and flattened blocks alive across frames.
    message_markdown: RefCell<HashMap<Uuid, MarkdownView>>,
    /// Stable offsets for capped user bubbles, including across virtualized row rebuilds.
    user_message_viewports: RefCell<HashMap<Uuid, UserMessageScrollViewport>>,
    /// User prompts whose height cap the reader lifted via "Show more".
    expanded_user_messages: HashSet<Uuid>,
    /// Focus handles for each bubble's "Show more" button, kept so focus
    /// survives the virtualized row rebuilds.
    user_message_expand_focuses: RefCell<HashMap<Uuid, FocusHandle>>,
    /// Parsed markdown for reasoning activities, keyed by stable activity id.
    activity_markdown: RefCell<HashMap<Uuid, MarkdownView>>,
    /// Byte offsets live reasoning peeks render from, slid forward as the
    /// thought grows; see `live_reasoning_window_start`.
    reasoning_window_starts: RefCell<HashMap<Uuid, usize>>,
    /// Independent capped viewports for expanded thoughts and command output.
    /// Keeping these stable preserves scroll position through virtualization.
    activity_scroll_viewports: RefCell<HashMap<Uuid, ActivityScrollViewport>>,
    /// Positioned, syntax-tokenized diff rows for expanded file-change
    /// activities. Built once when the activity is expanded and dropped when it
    /// collapses or its changes are replaced, so a frame only indexes rows.
    activity_diffs: RefCell<HashMap<Uuid, Rc<activity_diff::Diff>>>,
    /// Viewports for those diffs. Separate from `activity_scroll_viewports`
    /// because a failed edit shows both its diff and the error it returned.
    activity_diff_viewports: RefCell<HashMap<Uuid, ActivityScrollViewport>>,
    /// One allocation for every transcript markdown context to share. The
    /// callback knows about the active workspace; the renderer deliberately
    /// does not.
    markdown_link_handler: md::render::LinkHandler,
    /// Transcript-wide text selection, spanning messages and tool output. Its
    /// `annotations` handle holds the commented highlights of the session on
    /// screen.
    transcript_selection: TranscriptSelection,
    /// Annotation sets parked while their session is off screen. The live set
    /// travels inside `transcript_selection.annotations`; switching sessions
    /// swaps the two under `annotation_session`. The durable copy rides the
    /// session's composer draft — this map is the in-memory fallback for the
    /// swap.
    transcript_annotations: HashMap<Uuid, Vec<TranscriptAnnotation>>,
    /// Which session's annotations are currently loaded into
    /// `transcript_selection.annotations`.
    annotation_session: Option<Uuid>,
    annotation_next_id: u64,
    /// The floating comment editor's session state: which annotation is open
    /// and whether it has ever been confirmed.
    annotation_editor: Option<annotations::AnnotationEditor>,
    annotation_comment_input: Entity<TextInput>,
    /// Highlight under the pointer; `visible` once the hover delay elapsed.
    annotation_hover: Option<annotations::AnnotationHover>,
    /// A mouse-down that landed on a highlight, pending its mouse-up.
    annotation_press: Option<annotations::AnnotationPress>,
    /// Annotation sets that already shipped in a submission, parked per
    /// session as `(user message id, set)` pairs in send order. An agent
    /// reply citing "Annotation N" resolves against the most recent set
    /// before it — see `annotation_ref_set`. In-memory only, like the live
    /// set.
    sent_annotations: HashMap<Uuid, Vec<(Uuid, Rc<Vec<TranscriptAnnotation>>)>>,
    /// Sets drained into a queued follow-up, keyed by `QueuedMessage::id`;
    /// the message picks them back up when it leaves the queue.
    queued_annotations: HashMap<Uuid, Vec<TranscriptAnnotation>>,
    /// File annotations from the selected session's draft whose editor does
    /// not exist yet, keyed by workspace-relative path. Seeded into the
    /// editor when the file opens (or comes back with parked panel state);
    /// until then they still count in the composer chip and drain into
    /// submissions. The draft is their durable home — this is only the
    /// materialization for the session on screen.
    pending_file_annotations: HashMap<String, Vec<TranscriptAnnotation>>,
    /// `Annotation N` citation under the pointer; `visible` once the hover
    /// delay elapsed.
    annotation_ref_hover: Option<annotations::AnnotationRefHover>,
    /// Per-assistant-message citation resolution — the set each reply's
    /// "Annotation N" labels point at — rebuilt under the row-kinds
    /// fingerprint like the response footers.
    annotation_ref_sets: RefCell<HashMap<Uuid, Rc<Vec<TranscriptAnnotation>>>>,
    annotation_ref_sets_fingerprint: Cell<Option<u64>>,
    /// The app's window handle, for focus restore from contexts (entity
    /// subscriptions) that carry no `&mut Window`.
    window_handle: gpui::AnyWindowHandle,
    /// Programmatic focus for the transcript canvas. Clicking the transcript
    /// moves focus here so the shared find action can distinguish it from the
    /// right-panel file editor without putting the canvas in the tab order.
    transcript_focus: FocusHandle,
    /// Find-in-page state for the selected transcript, created lazily on the
    /// first primary-modifier F press.
    transcript_search: Option<transcript_search::TranscriptSearch>,
    /// Independent selection for the transient toast message. Keeping it out
    /// of the transcript registry prevents an overlay from joining a drag to
    /// whatever happens to be painted beneath it.
    toast_selection: TranscriptSelection,
    transcript_scrollbar: Rc<ScrollbarState>,
    /// Last measured height of the docked composer lane (queued prompts +
    /// composer + workspace footer). Big Picture remounts the composer entity
    /// inside its own layer; while it is open the column keeps a spacer at
    /// this height so the transcript's frame — and its scroll position —
    /// never moves.
    composer_lane_height: Rc<Cell<f32>>,
    /// Every menu site in the app, keyed by a stable id. Handles are created on
    /// first use and live as long as the window.
    menus: RefCell<HashMap<SharedString, ContextMenuHandle>>,
    navigation_rail: Entity<ConversationNavigationRail>,
    navigation_rail_reset_generation: Cell<u64>,
    /// Cached islands of the root view; see [`WakuPane`].
    sidebar_pane: Entity<WakuPane>,
    transcript_pane: Entity<WakuPane>,
    right_panel_pane: Entity<WakuPane>,
    git_panel_pane: Entity<WakuPane>,
    /// The unix second the pending time-label wake-up targets, or `None` when
    /// none is armed. See `schedule_time_label_wake`.
    time_label_wake: Cell<Option<u64>>,
    /// Bumped per (re)arm so a superseded wake-up discards itself.
    time_label_wake_generation: Cell<u64>,
    /// Live frames-per-second measurement for the header counter.
    fps_last_frame: Instant,
    fps_frame_count: u64,
    fps_value: u32,
}

mod activity_diff;
mod agy;
mod annotations;
mod archive_dialog;
mod autocomplete;
mod background_work;
mod big_picture;
mod branches;
mod command_palette;
mod commit_dialog;
mod components;
mod composer;
mod drafts;
mod element_inspector;
mod file_finder;
mod file_search;
mod friends;
mod git_panel;
mod github;
mod github_media;
mod go_to_line;
mod goal_dialog;
mod keybindings_page;
mod image_preview;
mod issue_dialog;
mod notifications;
mod project_switcher;
mod projects;
mod relocate;
mod render;
mod right_panel;
mod routing;
mod run_script;
mod runtime;
mod saved_drafts;
mod sessions;
mod settings;
mod shortcuts_dialog;
mod sidebar;
mod skills_page;
mod streaming;
mod sync_branch;
mod task_switcher;
mod terminals;
mod transcript;
mod transcript_search;
mod transcript_view;
mod usage_meter;
mod usage_page;
mod window_chrome;
mod worktrees;

pub use annotations::init as init_annotation_keys;
pub use archive_dialog::init as init_archive_dialog_keys;
pub use autocomplete::init as init_composer_autocomplete;
use background_work::{
    BackgroundWorkRegistry, work_kind_icon, work_status_color, work_status_label,
};
pub use big_picture::init as init_big_picture_keys;
pub use command_palette::init as init_command_palette;
pub use commit_dialog::init as init_commit_dialog_keys;
use components::*;
pub use element_inspector::init as init_element_inspector;
pub use file_finder::init as init_file_finder;
pub use git_panel::init as init_git_panel_keys;
pub use goal_dialog::init as init_goal_dialog_keys;
pub use image_preview::init as init_image_preview_keys;
pub use issue_dialog::init as init_issue_dialog_keys;
pub use saved_drafts::init as init_drafts_keys;
pub use settings::init as init_settings_keys;
pub use shortcuts_dialog::init as init_shortcuts_dialog_keys;
pub use sidebar::init as init_sidebar_keys;
use sidebar::{SidebarGroup, SidebarRow, format_time_ago, mix_str};
pub use skills_page::init as init_skills_keys;
pub use sync_branch::init as init_sync_branch;

// Re-exported for the keybinding catalog (`crate::keybindings`), which needs
// every dispatchable action by path without making each module public.
pub use archive_dialog::{ConfirmArchiveDialog, DismissArchiveDialog};
pub use big_picture::{
    BigPictureConfirm, BigPictureLeft, BigPictureRight, DismissBigPicture, SelectBigPictureCard,
};
pub use command_palette::{
    Confirm, Dismiss, SelectFirst, SelectLast, SelectNext, SelectPageDown, SelectPageUp,
    SelectPrevious,
};
pub use commit_dialog::{ConfirmCommitDialog, DismissCommitDialog};
pub use git_panel::{ConfirmGitPanelModal, DismissGitPanelModal, GitPanelPrimaryAction};
pub use goal_dialog::{ConfirmGoalDialog, DismissGoalDialog};
pub use image_preview::DismissImagePreview;
pub use settings::{FocusNext, FocusPrevious};
pub use shortcuts_dialog::DismissShortcutsDialog;
pub use sidebar::CancelSessionRename;
use streaming::*;
use terminals::TerminalRecord;
use transcript::*;
use transcript_view::ConversationNavigationRail;

/// Collapse provider- or page-supplied text into a label that cannot contain
/// hard line breaks. GPUI's `truncate()` prevents wrapping, but explicit
/// newlines still produce multiple visual lines.
fn single_line_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Seconds until any session's time label next changes value, or `None` when
/// no label is on the clock at all. A reply's "5m"/"3h"/"2d" moves only at
/// its unit boundary, so the wake-up this feeds gets rarer as the history
/// ages.
pub(super) fn next_time_label_change(sessions: &[AgentSession], now: u64) -> Option<u64> {
    let mut next: Option<u64> = None;
    for session in sessions {
        if let Some(last_reply_at) = session.last_reply_at {
            let elapsed = now.saturating_sub(last_reply_at);
            let step = match elapsed {
                0..=3_599 => 60,
                3_600..=86_399 => 3_600,
                _ => 86_400,
            };
            let remaining = (step - elapsed % step).max(1);
            next = Some(next.map_or(remaining, |next| next.min(remaining)));
        }
    }
    next
}

fn migrate_legacy_projectless_projects(
    state: &mut PersistedState,
    workspace: &waku_client::WorkspaceClient,
) -> (bool, Option<anyhow::Error>) {
    let legacy_indices = state
        .projects
        .iter()
        .enumerate()
        .filter_map(|(index, project)| {
            crate::projectless::needs_migration(&project.path).then_some(index)
        })
        .collect::<Vec<_>>();
    if legacy_indices.is_empty() {
        return (false, None);
    }

    let mut changed = false;
    for index in legacy_indices {
        let path = state.projects[index].path.clone();
        let response = workspace
            .request(waku_client::WorkspaceOperation::MigrateProjectlessWorkspace { path });
        let cwd = match response {
            Ok(waku_client::WorkspaceResult::ProjectlessWorkspace { cwd }) => cwd,
            Ok(_) => {
                return (
                    changed,
                    Some(anyhow::anyhow!(
                        "the daemon returned an invalid projectless response"
                    )),
                );
            }
            Err(error) => return (changed, Some(error)),
        };
        state.projects[index].name = Project::PROJECTLESS_NAME.to_owned();
        state.projects[index].path = cwd;
        changed = true;
    }
    (changed, None)
}

impl Waku {
    fn updater_button_expanded(&self) -> bool {
        self.updater_button_hovered || self.updater_button_focused
    }

    fn begin_updater_button_animation(&mut self, cx: &mut Context<Self>) {
        self.updater_button_animation_from_width = self.updater_button_width.get();
        self.updater_button_animation_from_reveal = self.updater_button_label_reveal.get();
        self.updater_button_animation_generation = self
            .updater_button_animation_generation
            .wrapping_add(1)
            .max(1);
        cx.notify();
    }

    fn set_updater_button_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.updater_button_hovered == hovered {
            return;
        }
        let was_expanded = self.updater_button_expanded();
        self.updater_button_hovered = hovered;
        if was_expanded != self.updater_button_expanded() {
            self.begin_updater_button_animation(cx);
        }
    }

    fn set_updater_button_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.updater_button_focused == focused {
            return;
        }
        let was_expanded = self.updater_button_expanded();
        self.updater_button_focused = focused;
        if was_expanded != self.updater_button_expanded() {
            self.begin_updater_button_animation(cx);
        }
    }

    fn reset_updater_button_animation(&mut self) {
        self.updater_button_hovered = false;
        self.updater_button_focused = false;
        self.updater_button_width
            .set(UPDATER_BUTTON_COLLAPSED_WIDTH);
        self.updater_button_label_reveal.set(0.0);
        self.updater_button_animation_from_width = UPDATER_BUTTON_COLLAPSED_WIDTH;
        self.updater_button_animation_from_reveal = 0.0;
        self.updater_button_animation_generation = 0;
    }

    fn handle_updater_event(
        &mut self,
        event: crate::updater::UpdaterEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            crate::updater::UpdaterEvent::StatusChanged(status) => {
                self.updater_status = status;
                self.reset_updater_button_animation();
            }
            crate::updater::UpdaterEvent::UpToDate => {
                self.updater_status = crate::updater::UpdateStatus::Idle;
                self.reset_updater_button_animation();
                self.show_success_toast(tr!("updater.up_to_date"));
            }
            crate::updater::UpdaterEvent::Failed(error) => {
                self.updater_status = crate::updater::UpdateStatus::Idle;
                self.reset_updater_button_animation();
                self.show_toast(tr!("updater.failed", error = error));
            }
            #[cfg(target_os = "linux")]
            crate::updater::UpdaterEvent::QuitAndInstall => {
                // The helper has already validated both prefixes and now
                // waits for GPUI's normal asynchronous quit hooks to finish
                // saving drafts and window state before it swaps them.
                cx.quit();
            }
        }
        cx.notify();
    }

    pub(super) fn show_toast(&mut self, message: impl Into<String>) {
        self.show_toast_with_tone(message, ToastTone::Alert, None);
    }

    pub(super) fn show_success_toast(&mut self, message: impl Into<String>) {
        self.show_toast_with_tone(message, ToastTone::Success, None);
    }

    /// Confirms an unarchive with a "View now" jump to the restored task.
    pub(super) fn show_unarchived_toast(&mut self, session_id: Uuid) {
        self.show_toast_with_tone(
            tr!("session.unarchived"),
            ToastTone::Success,
            Some(ToastAction {
                label: tr!("session.view_now").into(),
                kind: ToastActionKind::Session(session_id),
            }),
        );
    }

    /// Confirms a `gh issue create` — "View" deep-links into the in-app
    /// GitHub browser, matching what ⌘⌥I does while the toast is up.
    pub(super) fn show_issue_created_toast(&mut self, created: &issue_dialog::CreatedIssue) {
        let message = match created.number {
            Some(number) => tr!("issue.created_numbered", number = number),
            None => tr!("issue.created"),
        };
        self.show_toast_with_tone(
            message,
            ToastTone::Success,
            Some(ToastAction {
                label: tr!("issue.view").into(),
                kind: ToastActionKind::GitHubIssue {
                    project: created.project,
                    number: created.number,
                    url: created.url.clone().into(),
                },
            }),
        );
    }

    /// A toast's session action: leave settings, open the task, and retire
    /// the toast.
    pub(super) fn open_toast_session(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.hide_toast();
        self.settings_page = None;
        self.select_session(session_id, cx);
    }

    fn show_toast_with_tone(
        &mut self,
        message: impl Into<String>,
        tone: ToastTone,
        action: Option<ToastAction>,
    ) {
        self.show_toast_for(message, tone, action, DEFAULT_TOAST_DURATION);
    }

    /// The spinner a custom command launches with. Returns the toast's id
    /// so the run can tell whether its result still has a toast to land on.
    pub(super) fn show_progress_toast(
        &mut self,
        message: impl Into<String>,
        duration: Duration,
    ) -> u64 {
        self.show_toast_for(message, ToastTone::Progress, None, duration);
        self.toast_generation
    }

    fn show_toast_for(
        &mut self,
        message: impl Into<String>,
        tone: ToastTone,
        action: Option<ToastAction>,
        duration: Duration,
    ) {
        // A displaced localhost toast re-queues ahead of other pending
        // detections: it was first in line when something else took the slot.
        if let Some(localhost) = self.toast.take().and_then(|toast| toast.localhost) {
            self.pending_localhost_toasts
                .push_front(PendingLocalhostUrl {
                    terminal: localhost.terminal,
                    url: localhost.url,
                });
        }
        self.set_toast(ToastState {
            message: message.into(),
            detail: None,
            tone,
            action,
            id: 0,
            timer_generation: 0,
            duration_remaining: duration,
            timer_started: None,
            hovered: false,
            localhost: None,
        });
    }

    /// Resolve the visible toast in place — same element, new tone and
    /// message, fresh dismiss clock. `id` stays put so the spinner's swap
    /// to a result does not replay the entrance animation; only
    /// `timer_generation` moves, which retires the in-flight timer.
    pub(super) fn update_toast(&mut self, message: impl Into<String>, tone: ToastTone) {
        let Some(toast) = self.toast.as_mut() else {
            return;
        };
        toast.message = message.into();
        toast.detail = None;
        toast.tone = tone;
        toast.duration_remaining = DEFAULT_TOAST_DURATION;
        toast.timer_started = None;
        self.toast_generation = self.toast_generation.wrapping_add(1);
        toast.timer_generation = self.toast_generation;
    }

    /// A terminal printed a localhost URL. While its toast is up a better
    /// URL for the same terminal replaces it; anything else queues behind
    /// the current toast — one toast per terminal at a time.
    pub(super) fn on_localhost_url_detected(
        &mut self,
        view: &Entity<TerminalView>,
        url: String,
        cx: &mut Context<Self>,
    ) {
        let terminal = view.entity_id();
        let url = SharedString::from(url);
        if let Some(active) = self
            .toast
            .as_ref()
            .and_then(|toast| toast.localhost.as_ref())
            && active.terminal == terminal
        {
            if active.url != url
                && crate::terminal::localhost_url_rank(&url)
                    >= crate::terminal::localhost_url_rank(&active.url)
            {
                self.show_localhost_toast(terminal, url);
            }
            return;
        }
        match self
            .pending_localhost_toasts
            .iter_mut()
            .find(|pending| pending.terminal == terminal)
        {
            Some(pending)
                if pending.url != url
                    && crate::terminal::localhost_url_rank(&url)
                        >= crate::terminal::localhost_url_rank(&pending.url) =>
            {
                pending.url = url;
            }
            Some(_) => {}
            None => self
                .pending_localhost_toasts
                .push_back(PendingLocalhostUrl { terminal, url }),
        }
        self.promote_localhost_toast();
        cx.notify();
    }

    /// Surface the oldest queued detection once the toast slot is free,
    /// skipping entries whose terminal has since closed.
    fn promote_localhost_toast(&mut self) {
        if self.toast.is_some() {
            return;
        }
        while let Some(pending) = self.pending_localhost_toasts.pop_front() {
            let alive = self
                .right_panel_terminals
                .values()
                .any(|view| view.entity_id() == pending.terminal);
            if alive {
                self.show_localhost_toast(pending.terminal, pending.url);
                return;
            }
        }
    }

    /// The persistent port toast: no dismiss timer — it stays until the user
    /// opens the URL or dismisses it.
    fn show_localhost_toast(&mut self, terminal: EntityId, url: SharedString) {
        self.set_toast(ToastState {
            message: tr!("terminal.localhost_detected", url = url.as_ref()),
            tone: ToastTone::Notice,
            action: Some(ToastAction {
                label: tr!("terminal.localhost_open").into(),
                kind: ToastActionKind::LocalhostUrl,
            }),
            detail: None,
            id: 0,
            timer_generation: 0,
            duration_remaining: DEFAULT_TOAST_DURATION,
            timer_started: None,
            hovered: false,
            localhost: Some(LocalhostToast {
                terminal,
                url: url.clone(),
            }),
        });
    }

    /// The toast's open affordance and its key bindings land here: the
    /// displayed localhost toast wins, otherwise the newest queued detection.
    /// `in_browser_tab` is the shift-modified form.
    pub(super) fn open_detected_localhost_url(
        &mut self,
        in_browser_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (terminal, url) = match self
            .toast
            .as_ref()
            .and_then(|toast| toast.localhost.clone())
        {
            Some(localhost) => {
                self.hide_toast();
                (localhost.terminal, localhost.url)
            }
            None => match self.pending_localhost_toasts.pop_back() {
                Some(pending) => (pending.terminal, pending.url),
                None => return,
            },
        };
        self.pending_localhost_toasts
            .retain(|pending| pending.terminal != terminal);
        if in_browser_tab {
            self.settings_page = None;
            self.open_url_in_browser_tab(url.to_string(), window, cx);
        } else {
            cx.open_url(&url);
        }
        cx.notify();
    }

    fn open_localhost_url_action(
        &mut self,
        _: &crate::OpenLocalhostUrl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_detected_localhost_url(false, window, cx);
    }

    fn open_localhost_url_in_tab_action(
        &mut self,
        _: &crate::OpenLocalhostUrlInTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_detected_localhost_url(true, window, cx);
    }

    /// The unarchive toast's "View now" without the mouse. ⌘⌥O is shared
    /// with `OpenLocalhostUrl`: while a session toast is up this jumps to
    /// the task, anything else falls through to the localhost open.
    fn open_toast_session_action(
        &mut self,
        _: &crate::OpenToastSession,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session_id = match self.toast.as_ref().and_then(|toast| toast.action.as_ref()) {
            Some(ToastAction {
                kind: ToastActionKind::Session(session_id),
                ..
            }) => *session_id,
            _ => {
                cx.propagate();
                return;
            }
        };
        self.open_toast_session(session_id, cx);
    }

    fn set_toast(&mut self, mut toast: ToastState) {
        self.toast_selection.selection.borrow_mut().clear();
        self.toast_selection.registry.borrow_mut().clear();
        self.toast_generation = self.toast_generation.wrapping_add(1);
        toast.id = self.toast_generation;
        toast.timer_generation = self.toast_generation;
        self.toast = Some(toast);
    }

    /// Arm one wake-up for the moment a time-derived label next changes —
    /// the sidebar's relative reply times and every "Working for Ns" elapsed.
    ///
    /// There is deliberately no standing timer. Render calls this each frame;
    /// while the scheduled instant is unchanged it is a `Cell` comparison and
    /// nothing spawns. The timer fires exactly when a visible label rolls to
    /// its next value, notifies once, and the frame that draws the new value
    /// arms the next boundary. An idle window with hour-old sessions wakes
    /// once an hour; with nothing to show it wakes never. (T3 Code's
    /// equivalent is one minute-aligned interval gated on subscribers; label
    /// boundaries make even that unnecessary.) A busy session pins the chain
    /// to one-second steps — that is what keeps its elapsed counters moving
    /// under reduce-motion, where the pulse animations that normally drive
    /// frames are suppressed, and while a background turn sits between
    /// stream events.
    fn schedule_time_label_wake(&self, cx: &mut Context<Self>) {
        let now = unix_time();
        let target = next_time_label_change(&self.state.sessions, now).map(|seconds| now + seconds);
        // Terminal rows carry the same "…ago" labels; fold their next
        // boundary in so the shared wake covers them.
        let target = self
            .next_terminal_time_label_change(now, cx)
            .map_or(target, |seconds| {
                Some(target.map_or(now + seconds, |t| t.min(now + seconds)))
            });
        if self.time_label_wake.get() == target {
            return;
        }
        self.time_label_wake.set(target);
        let generation = self.time_label_wake_generation.get().wrapping_add(1);
        self.time_label_wake_generation.set(generation);
        let Some(target) = target else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let delay = target.saturating_sub(unix_time()).max(1);
            cx.background_executor()
                .timer(Duration::from_secs(delay))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.time_label_wake_generation.get() != generation {
                    return;
                }
                // Consumed: the notified frame re-arms the next boundary.
                this.time_label_wake.set(None);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn hide_toast(&mut self) {
        if self.toast.take().is_some() {
            self.toast_selection.selection.borrow_mut().clear();
            self.toast_selection.registry.borrow_mut().clear();
            // Detached timers are deliberately cheap, but their generation
            // must stop them from dismissing a newer toast.
            self.toast_generation = self.toast_generation.wrapping_add(1);
            self.promote_localhost_toast();
        }
    }

    fn start_toast_dismiss_timer(&mut self, cx: &mut Context<Self>) {
        let Some(toast) = self.toast.as_mut() else {
            return;
        };
        // A localhost toast is persistent: it leaves only when acted on.
        if toast.localhost.is_some() || toast.hovered || toast.timer_started.is_some() {
            return;
        }
        // A pending command run owns its toast until the result lands —
        // the output tail it mirrors would be cut short by the dismiss
        // clock. A `/land` in flight owns its spinner the same way.
        if self
            .custom_command_runs
            .values()
            .any(|run| run.toast_id == toast.id)
            || self
                .git_panel_operation
                .as_ref()
                .is_some_and(|operation| operation.toast_id == Some(toast.id))
        {
            return;
        }

        let duration = toast.duration_remaining;
        let generation = toast.timer_generation;
        toast.timer_started = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .toast
                    .as_ref()
                    .is_some_and(|toast| toast.timer_generation == generation && !toast.hovered)
                {
                    this.hide_toast();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_toast_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let Some(toast) = self.toast.as_ref() else {
            return;
        };
        if toast.hovered == hovered {
            return;
        }

        self.toast_generation = self.toast_generation.wrapping_add(1);
        let generation = self.toast_generation;
        let toast = self.toast.as_mut().expect("toast checked above");
        toast.timer_generation = generation;
        toast.hovered = hovered;
        if hovered {
            if let Some(started) = toast.timer_started.take() {
                toast.duration_remaining =
                    paused_toast_duration(toast.duration_remaining, started.elapsed());
            }
        } else {
            toast.timer_started = None;
            self.start_toast_dismiss_timer(cx);
        }
    }

    pub fn new(
        window: &mut Window,
        cx: &mut App,
        daemon: waku_client::DaemonSupervisor,
    ) -> Entity<Self> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        // Set only when `scripts/dev.ts` launched this app — the file it names
        // is the auto-restart toggle the watcher reads after each build.
        let dev_state_path = std::env::var_os("GODDARD_DEV_STATE")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty());
        let auto_restart_enabled = dev_state_path
            .as_deref()
            .map(read_auto_restart)
            .unwrap_or(false);
        let store = StateStore::remote(daemon.clone());
        let daemons = store.daemons();
        let daemon_hostname = crate::daemon::local_hostname().unwrap_or_else(|| "this-mac".into());
        let composer_draft_store = ComposerDraftStore::remote(daemons.clone());
        let composer_drafts = composer_draft_store.load().unwrap_or_default();
        let mut state = store.load_or_fresh(cwd);
        // Remote hosts connect in the background after the entity exists;
        // seed each one's cached catalog now so its projects and sessions are
        // on screen — read-only until live — before the first frame.
        let mut remote_catalogs = store.load_remote_catalogs();
        remote_catalogs
            .retain(|host, _| state.remote_hosts.iter().any(|remote| remote.id == *host));
        let remote_host_ids = state
            .remote_hosts
            .iter()
            .map(|host| host.id)
            .collect::<Vec<_>>();
        for host in remote_host_ids {
            if let Some(catalog) = remote_catalogs.get(&host) {
                seed_remote_catalog(&mut state, &daemons, host, catalog);
            }
        }
        let remote_catalogs_path = store.remote_catalogs_path();
        let home_directory = crate::projectless::home_directory();
        // Custom commands moved from this app's settings file into the
        // daemon's document. Seed the daemon list with any file-side commands
        // it does not already carry — id-keyed, so the import is a no-op once
        // every client has migrated.
        let mut daemon_settings = daemon.settings();
        if !state.custom_commands.is_empty() {
            let known: HashSet<Uuid> = daemon_settings
                .custom_commands
                .iter()
                .map(|command| command.id)
                .collect();
            daemon_settings.custom_commands.extend(
                state
                    .custom_commands
                    .iter()
                    .filter(|command| !known.contains(&command.id))
                    .cloned(),
            );
        }
        state.apply_daemon_settings(daemon_settings);
        if let Err(error) = daemon.update_settings(state.daemon_settings()) {
            eprintln!("could not normalize daemon settings after migration: {error:#}");
        }
        crate::i18n::set_language(state.language);
        // Chrome text is authored in `sp` rems against the default UI font
        // size, so the window's rem size *is* the UI font size setting.
        window.set_rem_size(px(waku_client::persistence::sanitized_ui_font_size(
            state.ui_font_size,
        )));
        crate::fonts::install(
            state.ui_font_family.as_deref(),
            state.code_font_family.as_deref(),
            cx,
        );
        crate::terminal::install_font_size(state.terminal_font_size(), cx);
        let analytics = crate::analytics::Analytics::new(
            state.language.locale(),
            state.analytics_id,
            state.analytics_enabled,
        );
        analytics.track(crate::analytics::Event::AppLaunched {
            task_count: state
                .sessions
                .iter()
                .filter(|session| session.has_started())
                .count(),
            project_count: state
                .projects
                .iter()
                .filter(|project| !project.is_projectless())
                .count(),
        });

        let composer = cx.new(|cx| {
            ComposerInput::new(window, cx)
                .padding_x(px(14.0), cx)
                .collapsed_paste(cx)
        });
        let user_input_answer = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("a11y.answer"))
                .placeholder(tr!("user_input.other_placeholder"))
        });
        let annotation_comment_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .submit_on_enter()
                .auto_height()
                .max_lines(8)
                .accessibility_label(tr!("a11y.comment"))
                .placeholder(tr!("annotations.comment_placeholder"))
        });
        let command_palette_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("a11y.command_palette"))
                .placeholder(tr!("command_palette.placeholder"))
        });
        let file_finder_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("a11y.file_finder"))
                .placeholder(tr!("file_finder.placeholder"))
        });
        let sync_branch_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("a11y.sync_branch"))
                .placeholder(tr!("input.search_branches"))
        });
        let model_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("input.search_models"))
                .placeholder(tr!("input.search_models"))
        });
        let route_class_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("input.search_models"))
                .placeholder(tr!("input.search_models"))
        });
        let branch_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("input.search_branches"))
                .placeholder(tr!("input.search_branches"))
        });
        let branch_create_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("input.new_branch_name"))
                .placeholder(tr!("input.new_branch_name"))
        });
        let worktree_name_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("input.worktree_name"))
                .placeholder(tr!("input.worktree_name"))
        });
        let settings_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("settings.search"))
                .placeholder(tr!("settings.search"))
        });
        let friend_code_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("friends.code_placeholder"))
                .placeholder(tr!("friends.code_placeholder"))
        });
        let friend_name_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("friends.display_name"))
                .placeholder(tr!("friends.display_name_placeholder"))
        });
        let friend_nickname_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("friends.nickname"))
                .placeholder(tr!("friends.nickname_placeholder"))
        });
        let archived_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("settings.archived_search"))
                .placeholder(tr!("settings.archived_search"))
        });
        let ui_font_selector = settings::FontSelector::new(window, cx);
        let code_font_selector = settings::FontSelector::new(window, cx);
        let daemon_port = state.daemon_exposure.port.to_string();
        let daemon_origins = state.daemon_exposure.allowed_origins_text();
        let daemon_port_input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .select_all_on_focus_click()
                .accessibility_label(tr!("daemon.port"))
                .placeholder(tr!("daemon.port_placeholder"));
            input.set_content(daemon_port, cx);
            input
        });
        let daemon_origins_input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .select_all_on_focus_click()
                .accessibility_label(tr!("daemon.allowed_origins"))
                .placeholder(tr!("daemon.allowed_origins_placeholder"));
            input.set_content(daemon_origins, cx);
            input
        });
        let worktree_sync_branches_input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .select_all_on_focus_click()
                .accessibility_label(tr!("settings.new_worktree_sync_branches"))
                .placeholder(tr!("settings.new_worktree_sync_branches_placeholder"));
            input.set_content(state.new_worktree_sync_branches.join(", "), cx);
            input
        });
        let mut eval_secret_input =
            |cx: &mut App, label: SharedString, placeholder: SharedString| {
                cx.new(|cx| {
                    TextInput::new(window, cx)
                        .masked()
                        .select_all_on_focus_click()
                        .accessibility_label(label)
                        .placeholder(placeholder)
                })
            };
        let eval_typesafe_key_input = eval_secret_input(
            cx,
            tr!("routing.typesafe_key").into(),
            tr!("routing.typesafe_key_placeholder").into(),
        );
        let eval_vercel_key_input = eval_secret_input(
            cx,
            tr!("routing.vercel_key").into(),
            tr!("routing.vercel_key_placeholder").into(),
        );
        let eval_cloudflare_token_input = eval_secret_input(
            cx,
            tr!("routing.cloudflare_token").into(),
            tr!("routing.cloudflare_token_placeholder").into(),
        );
        let eval_vercel_team_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .select_all_on_focus_click()
                .accessibility_label(tr!("routing.vercel_team"))
                .placeholder(tr!("routing.optional"))
        });
        let eval_cloudflare_account_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .select_all_on_focus_click()
                .accessibility_label(tr!("routing.cloudflare_account"))
                .placeholder(tr!("routing.cloudflare_account_placeholder"))
        });
        let skills_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("skills.search"))
                .placeholder(tr!("skills.search"))
        });
        let drafts_search = cx.new(|cx| {
            TextInput::new(window, cx)
                .clear_on_escape()
                .accessibility_label(tr!("drafts.search"))
                .placeholder(tr!("drafts.search"))
        });
        let drafts_edit_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .auto_height()
                .max_lines(12)
                .accessibility_label(tr!("a11y.draft_edit"))
                .placeholder(tr!("drafts.edit_placeholder"))
        });
        let session_rename_input =
            cx.new(|cx| TextInput::new(window, cx).accessibility_label(tr!("a11y.task_name")));
        let provider_path_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .select_all_on_focus_click()
                .accessibility_label(tr!("providers.binary_path"))
                .placeholder(tr!("input.detected_automatically"))
        });
        let usage_project_filter = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("input.filter_projects"))
                .placeholder(tr!("input.filter_projects"))
        });
        let right_panel_diff_filter = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("diff.filter_files"))
                .placeholder(tr!("diff.filter_files"))
        });
        let navigation_rail = cx.new(|_| ConversationNavigationRail::new());
        let sidebar_pane = WakuPane::new(Waku::sidebar_pane_content, cx);
        let transcript_pane = WakuPane::new(Waku::transcript_pane_content, cx);
        let right_panel_pane = WakuPane::new(Waku::right_panel_pane_content, cx);
        let git_panel_pane = WakuPane::new(Waku::git_panel_pane_content, cx);
        let workspace_client = waku_client::WorkspaceClient::new(daemon.client());
        let (projectless_migrated, projectless_migration_error) =
            migrate_legacy_projectless_projects(&mut state, &workspace_client);
        let projectless_save_error = projectless_migrated
            .then(|| store.save(&mut state).err())
            .flatten();
        let startup_toast = projectless_migration_error
            .map(|error| tr!("errors.move_projectless_task", error = error))
            .or_else(|| {
                projectless_save_error
                    .map(|error| tr!("errors.save_projectless_migration", error = error))
            });
        let sidebar_visible = state.sidebar_visible;
        let right_panel_visible = state.right_panel_visible;
        let git_panel_visible = state.git_panel_visible && !right_panel_visible;
        let sidebar_width = sanitize_panel_width(
            state.sidebar_width,
            DEFAULT_SIDEBAR_WIDTH,
            SIDEBAR_MIN_WIDTH,
            SIDEBAR_MAX_WIDTH,
        );
        let right_panel_width = sanitize_panel_width(
            state.right_panel_width,
            DEFAULT_RIGHT_PANEL_WIDTH,
            RIGHT_PANEL_MIN_WIDTH,
            RIGHT_PANEL_MAX_WIDTH,
        );
        let git_panel_top_height = sanitize_panel_width(
            state.git_panel_top_height,
            DEFAULT_GIT_PANEL_TOP_HEIGHT,
            GIT_PANEL_TOP_MIN_HEIGHT,
            GIT_PANEL_TOP_MAX_HEIGHT,
        );
        state.sidebar_width = sidebar_width;
        state.right_panel_width = right_panel_width;
        state.git_panel_top_height = git_panel_top_height;
        // First launch has no persisted frame yet; seed from the freshly
        // opened window so an immediate zoom or fullscreen still has a
        // floating frame to restore to. The bounds observer keeps it current
        // from here.
        if state.window_state.is_none() {
            state.window_state = Some(persisted_window_state(
                window.bounds(),
                false,
                window.display(cx).and_then(|display| display.uuid().ok()),
            ));
        }
        crate::theme::set_thick_borders(state.thick_borders);
        crate::theme::set_border_intensity(state.border_intensity);
        crate::theme::set_high_contrast(state.high_contrast);
        crate::theme::apply_theme_preference(
            state.theme,
            state.sidebar_transparency,
            state.sidebar_transparency_amount,
            window,
            cx,
        );
        crate::platform::set_sidebar_material_width(window, sidebar_width);
        crate::platform::set_trackpad_navigation_swipe_enabled(
            window,
            state.three_finger_swipe_navigation,
        );
        let project_paths = state
            .projects
            .iter()
            .map(|project| (project.id, project.path.clone()))
            .collect::<HashMap<_, _>>();
        let mut startup_live_session_ids = state
            .sessions
            .iter()
            .filter(|session| session.status.is_busy())
            .map(|session| session.id)
            .collect::<Vec<_>>();
        if let Some(selected) = state.selected_session
            && state
                .sessions
                .iter()
                .find(|session| session.id == selected)
                .is_some_and(AgentSession::has_started)
            && !startup_live_session_ids.contains(&selected)
        {
            startup_live_session_ids.push(selected);
        }
        let mut interrupted_turn_checkpoints = Vec::new();
        for session in &mut state.sessions {
            session.migrate_legacy_state();
            // A provider runtime belongs to the daemon and may still be
            // streaming after this desktop process restarted. Leave its
            // persisted projection intact until the background attachment
            // check proves there is no live runtime to resume.
            if session.status.is_busy() {
                continue;
            }
            if session.status != SessionStatus::Idle {
                session.status = SessionStatus::Idle;
            }
            let interrupted_turn = if let Some(turn) = session
                .turns
                .last_mut()
                .filter(|turn| turn.status == TurnStatus::Running)
            {
                turn.status = TurnStatus::Interrupted;
                turn.completed_at = Some(unix_time());
                Some(turn.turn_count)
            } else {
                None
            };
            // A crash mid-turn leaves work in the tree worth checkpointing, but
            // one `capture_turn` per interrupted session is upwards of ten
            // `git` invocations each — paid here, before the window has drawn
            // once. Queue them and let the first frames go out first.
            if let Some(turn_count) = interrupted_turn
                && let Some(project_path) = session
                    .workspace
                    .path()
                    .map(std::path::Path::to_path_buf)
                    .or_else(|| project_paths.get(&session.project_id).cloned())
            {
                interrupted_turn_checkpoints.push(PendingCheckpointCapture {
                    session_id: session.id,
                    turn_count,
                    project_path,
                });
            }
            for message in &mut session.messages {
                message.streaming = false;
            }
            for block in &mut session.transcript_blocks {
                block.activities.retain(|activity| {
                    activity
                        .reasoning
                        .as_ref()
                        .is_none_or(|reasoning| !reasoning.content.trim().is_empty())
                });
                for activity in &mut block.activities {
                    activity.complete = true;
                }
            }
            session
                .transcript_blocks
                .retain(|block| !block.activities.is_empty());
        }
        let initial_composer_draft = state
            .selected_session
            .and_then(|selected| state.sessions.iter().find(|session| session.id == selected))
            .and_then(|session| composer_drafts.get_for(session))
            .cloned()
            .unwrap_or_default();
        let crate::persistence::ComposerDraft {
            text: initial_composer_text,
            attachments: initial_composer_attachments,
            annotations: initial_composer_annotations,
        } = initial_composer_draft;
        if !initial_composer_text.is_empty() {
            composer.update(cx, |input, cx| input.set_content(initial_composer_text, cx));
        }
        let composer_attachments = initial_composer_attachments
            .into_iter()
            .map(ComposerAttachment::from)
            .collect();
        // The saved draft's annotations reopen with the selected session:
        // transcript highlights go straight into the selection's painted
        // store, file ones wait for their editor to exist.
        let transcript_selection = TranscriptSelection::default();
        let mut pending_file_annotations: HashMap<String, Vec<TranscriptAnnotation>> =
            HashMap::new();
        let mut annotation_next_id = 1u64;
        {
            let mut items = Vec::new();
            for annotation in initial_composer_annotations {
                let annotation = TranscriptAnnotation::from(annotation);
                annotation_next_id = annotation_next_id.max(annotation.id.saturating_add(1));
                if let Some(file) = &annotation.file {
                    pending_file_annotations
                        .entry(file.path.clone())
                        .or_default()
                        .push(annotation);
                } else {
                    items.push(annotation);
                }
            }
            transcript_selection.annotations.borrow_mut().items = items;
        }
        let probes = ProviderKind::ALL
            .into_iter()
            .map(|provider| ProviderProbe {
                provider,
                installed: false,
                path: None,
                models: crate::model_catalog::fallback_models(provider),
                agent_presets: crate::model_catalog::fallback_agent_presets(provider),
            })
            .collect::<Vec<_>>();
        let (provider_probe_tx, provider_probe_events) = unbounded();
        let (provider_version_tx, provider_version_events) = unbounded();
        let (provider_detection_tx, provider_detection_events) = unbounded();
        let (computer_permission_tx, computer_permission_events) = unbounded();
        let (plan_usage_tx, plan_usage_events) = unbounded();
        let (agy_poll_tx, agy_poll_events) = unbounded();
        let (event_wake_tx, event_wake_events) = smol::channel::bounded(1);
        let (task_state_sync_tx, task_state_sync_events) = unbounded();
        let (daemon_settings_tx, daemon_settings_events) = unbounded();
        let (friends_tx, friends_events) = unbounded();
        let (route_policy_tx, route_policy_events) = unbounded();
        #[cfg(target_os = "macos")]
        if state.computer_use_experiment_enabled {
            let computer_permission_tx = computer_permission_tx.clone();
            let event_wake = event_wake_tx.clone();
            let daemon = daemon.client();
            std::thread::Builder::new()
                .name("waku-computer-permission-probe".into())
                .spawn(move || {
                    let result = match daemon.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::ProbeComputerPermissions { prompt: false },
                    ) {
                        Ok(waku_client::ResponsePayload::ComputerPermissions { permissions }) => {
                            Ok(permissions)
                        }
                        Ok(_) => Err("the daemon returned an invalid permission response".into()),
                        Err(error) => Err(error.to_string()),
                    };
                    if computer_permission_tx.send(result).is_ok() {
                        signal_event_pump(&event_wake);
                    }
                })
                .ok();
        }
        let mut session_navigation = SessionNavigation::default();
        if let Some(session_id) = state.selected_session.filter(|session_id| {
            state
                .sessions
                .iter()
                .any(|session| session.id == *session_id && !session.has_started())
        }) {
            session_navigation.remember_new_task(session_id);
        }
        // Measure visible rows only, with a generous overdraw — the same shape
        // Zed's own agent chat uses. `measure_all` lays out every row in the
        // session on the first frame and again after any structural splice,
        // which a long transcript cannot afford.
        let transcript_rows = ListState::new(0, ListAlignment::Bottom, px(2048.0));
        let anchored_transcript_rows = ListState::new(0, ListAlignment::Top, px(2048.0));
        let sidebar_list_state = ListState::new(0, ListAlignment::Top, px(256.0));
        let usage_projects_list = ListState::new(0, ListAlignment::Top, px(256.0));
        let branch_picker_list_state = ListState::new(0, ListAlignment::Top, px(152.0));
        let transcript_is_scrolled = Rc::new(Cell::new(false));
        let transcript_anchor_following = Rc::new(Cell::new(false));
        let transcript_tail_recheck = Rc::new(Cell::new(false));
        let transcript_last_wheel_scroll = Rc::new(Cell::new(None));
        // A wheel scroll drops tail following and asks the next measured frame
        // whether it landed back on the tail. GPUI re-engages its own tail pin
        // when a bottom-aligned list reaches the end — it represents that end as
        // no logical offset — but a turn renders through the top-aligned
        // anchored list, whose end is an ordinary offset, so only this can.
        transcript_rows.set_scroll_handler({
            let transcript_is_scrolled = transcript_is_scrolled.clone();
            let transcript_anchor_following = transcript_anchor_following.clone();
            let transcript_tail_recheck = transcript_tail_recheck.clone();
            let transcript_last_wheel_scroll = transcript_last_wheel_scroll.clone();
            move |event, window, _| {
                transcript_is_scrolled.set(event.is_scrolled);
                transcript_anchor_following.set(false);
                transcript_tail_recheck.set(true);
                transcript_last_wheel_scroll.set(Some(Instant::now()));
                window.refresh();
            }
        });
        anchored_transcript_rows.set_scroll_handler({
            let transcript_is_scrolled = transcript_is_scrolled.clone();
            let transcript_anchor_following = transcript_anchor_following.clone();
            let transcript_tail_recheck = transcript_tail_recheck.clone();
            let transcript_last_wheel_scroll = transcript_last_wheel_scroll.clone();
            move |event, window, _| {
                transcript_is_scrolled.set(event.is_scrolled);
                transcript_anchor_following.set(false);
                transcript_tail_recheck.set(true);
                transcript_last_wheel_scroll.set(Some(Instant::now()));
                window.refresh();
            }
        });
        // Enable GPUI's experimental overlay plane so deferred draws (menus,
        // tooltips, popovers) composite above native content — without it the
        // browser surface would cover them.
        //
        // Both backends of the pinned fork implement it, and both browser
        // hosts render somewhere it can reach: a sibling NSView below GPUI's
        // overlay layer on macOS, a DirectComposition visual between GPUI's
        // base and overlay planes on Windows. When it is unavailable the
        // surface falls back to freezing the page to a bitmap while an
        // overlay is open.
        let scene_overlay_enabled = window.enable_scene_overlay().is_ok();
        let (updater_status, updater_events) = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|state| state.0.as_ref())
            .map(|updater| (updater.status(), Some(updater.events())))
            .unwrap_or_default();
        let entity = cx.new(|cx| {
            let settings_focus = cx.focus_handle();
            let onboarding_add_project_focus = cx.focus_handle();
            let onboarding_projectless_focus = cx.focus_handle();
            let updater_button_focus = cx.focus_handle();
            let model_picker_empty_focus = cx.focus_handle();
            let task_switcher_focus = cx.focus_handle();
            cx.on_focus_out(
                &task_switcher_focus,
                window,
                |this: &mut Self, _, window, cx| {
                    this.cancel_task_switcher(window, cx);
                },
            )
            .detach();
            let mut task_switcher = task_switcher::TaskSwitcherUi::new(task_switcher_focus);
            if let Some(selected_session) = state.selected_session {
                task_switcher.record_access(selected_session);
            }

            let project_switcher_focus = cx.focus_handle();
            cx.on_focus_out(
                &project_switcher_focus,
                window,
                |this: &mut Self, _, window, cx| {
                    this.cancel_project_switcher(window, cx);
                },
            )
            .detach();
            let project_switcher = project_switcher::ProjectSwitcherUi::new(project_switcher_focus);
            let big_picture = big_picture::BigPictureUi::new(cx.focus_handle());

            cx.on_focus(&updater_button_focus, window, |this: &mut Self, _, cx| {
                this.set_updater_button_focused(true, cx);
            })
            .detach();
            cx.on_blur(&updater_button_focus, window, |this: &mut Self, _, cx| {
                this.set_updater_button_focused(false, cx);
            })
            .detach();

            if let Some(updater_events) = updater_events {
                cx.spawn(async move |this: WeakEntity<Self>, cx| {
                    while let Ok(event) = updater_events.recv().await {
                        if this
                            .update(cx, |this: &mut Self, cx| {
                                this.handle_updater_event(event, cx)
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }

            cx.observe_window_appearance(window, |this: &mut Self, window, cx| {
                if this.state.theme.mode == ThemeMode::System {
                    crate::theme::apply_theme_preference(
                        this.state.theme,
                        this.state.sidebar_transparency,
                        this.state.sidebar_transparency_amount,
                        window,
                        cx,
                    );
                    cx.notify();
                }
            })
            .detach();

            cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                this.capture_window_state(window, cx);
            })
            .detach();

            cx.observe_window_activation(window, |this: &mut Self, window, cx| {
                if window.is_window_active() {
                    this.reload_clean_right_panel_file_editors(cx);
                    // The working tree and branch may have moved while another
                    // app had focus — a checkout in a terminal, an edit in an
                    // editor. Coming back is the moment to re-check.
                    this.invalidate_workspace_queries(cx);
                    if this.settings_page == Some(SettingsPage::ComputerUse) {
                        this.request_computer_permissions(false, cx);
                    }
                    // Skill files are routinely edited in another app; coming
                    // back to the window is the moment to re-read them.
                    if this.settings_page == Some(SettingsPage::Skills) {
                        this.ensure_skills_catalog(true, cx);
                    }
                    // Provider CLIs get installed, upgraded, and removed in a
                    // terminal; coming back is the moment to re-detect them.
                    if this.settings_page == Some(SettingsPage::Providers) {
                        this.refresh_provider_detection(None);
                    }
                } else {
                    this.sidebar_shortcuts_window_deactivated(cx);
                }
            })
            .detach();

            // A closed surface can take the window's focus down with it —
            // closing a browser tab drops the focused address input — and
            // with nothing focused, action availability walks only the root
            // dispatch node, so every app menu item greys out. When focus
            // dies with its element, send it home to the composer, the way
            // Zed's workspace refocuses itself.
            cx.on_focus_lost(window, |this: &mut Self, window, cx| {
                let focus = this.composer_focus(cx);
                window.focus(&focus, cx);
            })
            .detach();

            // Edits, not raw notifies: a field also notifies for caret blinks
            // and selection changes, and none of the app chrome depends on
            // those — re-rendering the window twice a second for a blinking
            // caret is exactly what the Performance guidance forbids.
            cx.subscribe(
                &composer,
                |this: &mut Self, _, event: &ComposerEvent, cx| match event {
                    ComposerEvent::Submit(prompt) => {
                        let typed_only =
                            prompt.trim().is_empty() && this.composer_pasted_blocks.is_empty();
                        if this.big_picture.is_open() {
                            // Big Picture routes by its own target — a card's
                            // session or a new task — not the selection.
                            if !typed_only
                                && let Some(submission) =
                                    this.submission_with_attachments(prompt, cx)
                            {
                                this.submit_big_picture_submission(submission, cx);
                            }
                        } else if this.projects_page.is_some() {
                            // The docked composer is the same entity; on the
                            // page Enter means "new task on this project".
                            this.projects_submit(prompt, cx);
                        } else if let Some(session_id) =
                            this.selected_session().and_then(|session| {
                                this.response_fork_preparations
                                    .contains_key(&session.id)
                                    .then_some(session.id)
                            })
                        {
                            this.defer_restore_composer_after_fork(session_id, prompt.clone(), cx);
                        } else if prompt.trim().is_empty()
                            && this.composer_attachments.is_empty()
                            && this.composer_pasted_blocks.is_empty()
                            && !this.has_annotations()
                            && this
                                .selected_session()
                                .is_some_and(composer::session_awaits_continue)
                        {
                            // Enter on an empty composer over a stopped turn
                            // is the same affordance as the play button.
                            this.continue_interrupted_session(cx);
                        } else if let Some(submission) =
                            this.submission_with_attachments(prompt, cx)
                        {
                            this.submit_composer_submission(submission, cx);
                        }
                    }
                    ComposerEvent::SubmitSteer(prompt) => {
                        // An empty field is only an empty draft when nothing
                        // is staged alongside it — attachments, pasted
                        // blocks, and annotations all steer as a submission.
                        let empty_draft = prompt.trim().is_empty()
                            && this.composer_attachments.is_empty()
                            && this.composer_pasted_blocks.is_empty()
                            && !this.has_annotations();
                        if this.big_picture.is_open() && !empty_draft {
                            if let Some(submission) =
                                this.submission_with_attachments(prompt, cx)
                            {
                                this.steer_big_picture_submission(submission, cx);
                            }
                        } else if this.projects_page.is_some() {
                            // Nothing on the page can be steered — a steered
                            // draft is a send there.
                            this.projects_submit(prompt, cx);
                        } else if empty_draft {
                            if this
                                .composer_session()
                                .is_some_and(composer::session_awaits_continue)
                            {
                                // Cmd+Enter on an empty composer continues a
                                // stopped turn too; its queued follow-ups
                                // still drain once that turn settles.
                                this.continue_interrupted_session(cx);
                            } else {
                                // A truly empty composer activates the
                                // oldest queued follow-up's Steer control.
                                this.steer_oldest_queued_message(cx);
                            }
                        } else if let Some(session_id) =
                            this.selected_session().and_then(|session| {
                                this.response_fork_preparations
                                    .contains_key(&session.id)
                                    .then_some(session.id)
                            })
                        {
                            this.defer_restore_composer_after_fork(session_id, prompt.clone(), cx);
                        } else if let Some(submission) =
                            this.submission_with_attachments(prompt, cx)
                        {
                            this.steer_composer_submission(submission, cx);
                        }
                    }
                    ComposerEvent::Edited => {
                        this.schedule_composer_draft_save(cx);
                        cx.notify();
                    }
                    ComposerEvent::Focus => {}
                    ComposerEvent::BackspaceOnEmpty => {
                        if this.composer_pasted_blocks.pop().is_some()
                            || this.composer_attachments.pop().is_some()
                        {
                            this.schedule_composer_draft_save(cx);
                            cx.notify();
                        }
                    }
                },
            )
            .detach();

            cx.subscribe(
                &user_input_answer,
                |this: &mut Self, input, event: &InputEvent, cx| match event {
                    InputEvent::Submit(answer) => {
                        this.submit_user_input_custom_answer(answer.clone(), cx);
                    }
                    InputEvent::Edited => {
                        let answer = input.read(cx).content().to_owned();
                        this.update_user_input_custom_answer(answer, cx);
                    }
                    InputEvent::Focus | InputEvent::BackspaceOnEmpty => {}
                },
            )
            .detach();

            cx.subscribe(
                &annotation_comment_input,
                |this: &mut Self, _, event: &InputEvent, cx| match event {
                    InputEvent::Submit(_) => this.commit_annotation_editor(cx),
                    InputEvent::Edited | InputEvent::Focus | InputEvent::BackspaceOnEmpty => {}
                },
            )
            .detach();

            // Clipboard images and Finder file copies are attachment payloads,
            // not text paths. The input owns representation priority; Goddard
            // owns durable staging and composer/session state.
            cx.subscribe(
                &composer,
                |this: &mut Self, _, event: &ComposerAttachmentPaste, cx| {
                    this.stage_pasted_attachments(event.0.clone(), cx);
                },
            )
            .detach();

            // A text paste too large for the field collapses into a chip;
            // the composer's text stays the user's own typing.
            cx.subscribe(
                &composer,
                |this: &mut Self, _, event: &ComposerTextPaste, cx| {
                    this.stage_pasted_text(event.0.clone(), cx);
                },
            )
            .detach();

            // A normal Cmd-Q waits briefly for this future, so even an edit
            // made inside the debounce window is durable before the process
            // exits. Filesystem work still stays off the UI thread.
            cx.on_app_quit(|this, cx| {
                this.capture_current_composer_draft(cx);
                this.composer_draft_save_generation =
                    this.composer_draft_save_generation.saturating_add(1);
                let generation = this.composer_draft_save_generation;
                let store = this.composer_draft_store.clone();
                let drafts = this.composer_drafts.clone();
                let save = cx
                    .background_executor()
                    .spawn(async move { store.save(drafts, generation) });
                async move {
                    let _ = save.await;
                }
            })
            .detach();

            // Window-frame changes are only mirrored in memory; the quit save
            // is what lands the final position and size on disk.
            cx.on_app_quit(|this, _| {
                this.save();
                async {}
            })
            .detach();

            // A changed query re-filters the picker rows and renumbers them,
            // so the drawn selection cannot carry over. While a filter is
            // active the cursor lands on the first match so `enter` has a
            // visible target; clearing the query returns to the opening
            // state — nothing highlighted, the current model's row in view.
            cx.subscribe(
                &model_search,
                |this: &mut Self, search, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        if search.read(cx).content().trim().is_empty() {
                            this.model_picker_highlight = None;
                            this.reveal_selected_picker_model();
                        } else {
                            this.model_picker_highlight = Some(0);
                            this.model_picker_list.scroll_to(ListOffset {
                                item_ix: 0,
                                offset_in_item: Pixels::ZERO,
                            });
                        }
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &route_class_search,
                |this: &mut Self, search, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        // Same contract as the model picker: a live filter
                        // pins the cursor to the first row so `enter` has a
                        // visible target; clearing returns to the opening
                        // state — nothing highlighted, the class's target
                        // row back in view.
                        if search.read(cx).content().trim().is_empty() {
                            this.route_class_highlight = None;
                            if let Some(class) = this.route_class_picker {
                                this.reveal_route_class_target(class);
                            }
                        } else {
                            this.route_class_highlight = Some(0);
                            this.route_class_scroll.scroll_to_item(0);
                        }
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &command_palette_search,
                |this: &mut Self, search, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        let query = search.read(cx).content().to_owned();
                        this.command_palette_query_edited(&query, cx);
                    }
                },
            )
            .detach();
            cx.subscribe(
                &file_finder_search,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        this.file_finder_query_edited(cx);
                    }
                },
            )
            .detach();
            cx.subscribe(
                &sync_branch_search,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        this.sync_branch_query_edited(cx);
                    }
                },
            )
            .detach();
            cx.subscribe(
                &branch_search,
                |this: &mut Self, search, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited)
                        && this.branch_picker_mode == BranchPickerMode::Browse
                    {
                        if search.read(cx).content().trim().is_empty() {
                            this.branch_picker_highlight = None;
                        } else {
                            this.branch_picker_highlight = Some(0);
                            this.branch_picker_list_state.scroll_to_reveal_item(0);
                        }
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &branch_create_input,
                |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &settings_search,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        // A new query rebuilds the result list, so the scroll
                        // offset and any arrow-key target no longer apply.
                        this.settings_scroll.set_offset(point(px(0.0), px(0.0)));
                        this.settings_search_target = None;
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &archived_search,
                |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &friend_code_input,
                |this: &mut Self, _, event: &InputEvent, cx| match event {
                    InputEvent::Submit(_) => this.send_friend_request(cx),
                    // Repaint so the Send button's enabled state tracks the
                    // field while typing.
                    InputEvent::Edited => cx.notify(),
                    _ => {}
                },
            )
            .detach();
            cx.subscribe(
                &friend_name_input,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Submit(_)) {
                        this.save_friend_display_name(cx);
                    }
                },
            )
            .detach();
            cx.subscribe(
                &friend_nickname_input,
                |this: &mut Self, _, event: &InputEvent, cx| match event {
                    InputEvent::Submit(_) => this.commit_friend_nickname(cx),
                    InputEvent::Edited => cx.notify(),
                    _ => {}
                },
            )
            .detach();
            for (target, search) in [
                (settings::FontTarget::Ui, ui_font_selector.search.clone()),
                (
                    settings::FontTarget::Code,
                    code_font_selector.search.clone(),
                ),
            ] {
                cx.subscribe(
                    &search,
                    move |this: &mut Self, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Edited) {
                            this.font_selector_query_edited(target, cx);
                        }
                    },
                )
                .detach();
            }
            for input in [&daemon_port_input, &daemon_origins_input] {
                cx.subscribe(
                    input,
                    |this: &mut Self, _, event: &InputEvent, cx| match event {
                        InputEvent::Submit(_) => this.apply_daemon_exposure_fields(cx),
                        InputEvent::Edited => cx.notify(),
                        _ => {}
                    },
                )
                .detach();
            }
            cx.subscribe(
                &worktree_sync_branches_input,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        this.apply_worktree_sync_branches(cx);
                    }
                },
            )
            .detach();
            for input in [
                &eval_typesafe_key_input,
                &eval_vercel_key_input,
                &eval_vercel_team_input,
                &eval_cloudflare_account_input,
                &eval_cloudflare_token_input,
            ] {
                cx.subscribe(
                    input,
                    |this: &mut Self, _, event: &InputEvent, cx| match event {
                        InputEvent::Submit(_) => this.save_eval_credentials(cx),
                        InputEvent::Edited => cx.notify(),
                        _ => {}
                    },
                )
                .detach();
            }
            cx.subscribe(&skills_search, |_: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Edited) {
                    cx.notify();
                }
            })
            .detach();
            cx.subscribe(&drafts_search, |_: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Edited) {
                    cx.notify();
                }
            })
            .detach();
            cx.subscribe(
                &session_rename_input,
                |this: &mut Self, _, event: &InputEvent, cx| match event {
                    InputEvent::Submit(_) => {
                        this.commit_session_rename(cx);
                        this.commit_terminal_rename(cx);
                    }
                    InputEvent::Edited
                        if this.session_rename.is_some() || this.terminal_rename.is_some() =>
                    {
                        cx.notify()
                    }
                    _ => {}
                },
            )
            .detach();
            cx.subscribe(
                &usage_project_filter,
                |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &right_panel_diff_filter,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) {
                        this.sync_right_panel_diff_tree_rows(cx);
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &provider_path_input,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Submit(_)) {
                        this.apply_provider_path_override(cx);
                    }
                },
            )
            .detach();

            // Like T3 Code's adapter subscriptions feeding its ingestion
            // worker, provider threads push an edge into this bounded wake
            // channel. The UI does no standing scan: the short follow-up tick
            // exists only to remeasure Markdown after a changed text frame.
            cx.spawn(async move |this, cx| {
                while event_wake_events.recv().await.is_ok() {
                    loop {
                        // The typed queues are drained below, so all wake edges
                        // already represented by those payloads can coalesce.
                        while event_wake_events.try_recv().is_ok() {}
                        let schedule = match this.update(cx, |this, cx| this.drain_event_pump(cx)) {
                            Ok(schedule) => schedule,
                            Err(_) => return,
                        };
                        match schedule {
                            EventPumpSchedule::Idle => break,
                            EventPumpSchedule::StreamFrame => {
                                // Deliberately not raced against the wake
                                // channel: waking per chunk made the notify
                                // rate equal the provider's chunk rate, and
                                // every notify is a full re-render. Chunks
                                // queue during the sleep and fold into the
                                // next drain's single batch.
                                cx.background_executor().timer(STREAM_FRAME_INTERVAL).await;
                            }
                            EventPumpSchedule::BackgroundOutput(delay) => {
                                // A log cache has its own 100 ms batching
                                // cadence. A new provider edge interrupts that
                                // wait; it must not wait behind log rendering.
                                futures_lite::future::race(
                                    async {
                                        let _ = event_wake_events.recv().await;
                                    },
                                    async {
                                        cx.background_executor().timer(delay).await;
                                    },
                                )
                                .await;
                            }
                        }
                    }
                }
            })
            .detach();

            // Maintenance clocks are intentionally independent of provider
            // ingestion and run at the slowest cadence their UI requires.
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(BACKGROUND_WORK_TICK_INTERVAL)
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            this.maybe_refresh_background_work(cx);
                            this.maybe_poll_notifications(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();

            cx.spawn(async move |this, cx| {
                loop {
                    if this
                        .update(cx, |this, cx| this.maybe_refresh_plan_usage(cx))
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor()
                        .timer(PLAN_USAGE_MAINTENANCE_INTERVAL)
                        .await;
                }
            })
            .detach();

            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(IDLE_SESSION_SWEEP_INTERVAL)
                        .await;
                    if this
                        .update(cx, |this, _| this.reap_idle_sessions())
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();

            // Antigravity's TUI owns its sessions, so the CLI's own
            // summaries db is the only status source. The poll early-outs
            // while no Antigravity session needs it.
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(agy::AGY_POLL_INTERVAL)
                        .await;
                    if this
                        .update(cx, |this, cx| this.maybe_poll_agy_sessions(cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();

            let markdown_link_handler: md::render::LinkHandler = {
                let waku = cx.entity().downgrade();
                Rc::new(move |target, _, cx| {
                    let handled = waku
                        .update(cx, |waku, cx| waku.open_transcript_link(target, cx))
                        .unwrap_or(false);
                    if !handled {
                        cx.open_url(target);
                    }
                })
            };

            // Read before `state` moves into the struct literal below.
            let initial_session = state.selected_session;

            Self {
                daemon,
                daemons,
                remote_errors: HashMap::new(),
                remote_daemon_settings: HashMap::new(),
                #[cfg(unix)]
                ssh_transports: HashMap::new(),
                #[cfg(unix)]
                ssh_askpass_started: false,
                #[cfg(unix)]
                pending_ssh_prompt: None,
                remote_catalogs,
                remote_catalogs_path,
                daemon_hostname,
                session_hydrations: HashSet::new(),
                pending_session_activation: None,
                analytics,
                state,
                store,
                home_directory,
                composer,
                user_input_answer,
                composer_drafts,
                composer_draft_store,
                composer_draft_save_generation: 0,
                command_palette: command_palette::CommandPaletteUi::new(command_palette_search),
                file_finder: file_finder::FileFinderUi::new(file_finder_search),
                sync_branch: sync_branch::SyncBranchUi::new(sync_branch_search),
                task_switcher,
                project_switcher,
                big_picture,
                model_search,
                route_class_search,
                branch_search,
                branch_create_input,
                worktree_name_input,
                worktree_picker_highlight: None,
                worktree_creation_pending: false,
                worktree_move_pending: HashSet::new(),
                settings_search,
                friend_code_input,
                friend_name_input,
                friend_nickname_input,
                editing_friend_nickname: None,
                ui_font_selector,
                code_font_selector,
                daemon_port_input,
                daemon_origins_input,
                worktree_sync_branches_input,
                daemon_reconfigure_pending: false,
                daemon_token_revealed: false,
                eval_typesafe_key_input,
                eval_vercel_key_input,
                eval_vercel_team_input,
                eval_cloudflare_account_input,
                eval_cloudflare_token_input,
                eval_inputs_seeded: false,
                settings_focus,
                onboarding_add_project_focus,
                onboarding_projectless_focus,
                automatic_updates_enabled: cx
                    .try_global::<crate::updater::UpdaterState>()
                    .and_then(|updater| updater.0.as_ref())
                    .is_some_and(|updater| updater.automatically_checks_for_updates()),
                updater_status,
                updater_button_focus,
                updater_button_hovered: false,
                updater_button_focused: false,
                updater_button_width: Rc::new(Cell::new(UPDATER_BUTTON_COLLAPSED_WIDTH)),
                updater_button_label_reveal: Rc::new(Cell::new(0.0)),
                updater_button_animation_from_width: UPDATER_BUTTON_COLLAPSED_WIDTH,
                updater_button_animation_from_reveal: 0.0,
                updater_button_animation_generation: 0,
                probes,
                provider_probe_tx,
                provider_probe_events,
                provider_model_discoveries: HashSet::new(),
                provider_model_discoveries_pending: HashSet::new(),
                provider_versions: HashMap::new(),
                provider_version_tx,
                provider_version_events,
                provider_version_probes_pending: HashSet::new(),
                provider_detection_tx,
                provider_detection_events,
                provider_detection_remaining: 0,
                provider_detection_checked_at: None,
                expanded_provider_settings: None,
                provider_path_input,
                computer_permissions: ComputerPermissions::default(),
                computer_permission_tx,
                computer_permission_events,
                computer_permission_request_pending: false,
                plan_usage: HashMap::new(),
                plan_usage_error: HashMap::new(),
                plan_usage_tx,
                plan_usage_events,
                plan_usage_pending: HashSet::new(),
                plan_usage_unconfigured: HashSet::new(),
                plan_usage_checked_at: HashMap::new(),
                plan_usage_stale: HashSet::new(),
                usage_history: None,
                usage_history_parts: HashMap::new(),
                usage_history_pending_for: None,
                usage_history_generation: 0,
                usage_history_scanned_at: None,
                usage_view: UsageViewMode::Daily,
                usage_window: crate::usage_history::UsageWindow::TrailingDays(30),
                usage_metric: UsageMetric::Cost,
                usage_breakdown: UsageBreakdown::Model,
                usage_model_col_widths: [84.0, 64.0, 84.0],
                usage_day_col_widths: [84.0; 4],
                usage_col_resize: column_resize::ColumnResize::new(),
                usage_months_scroll: ScrollHandle::new(),
                usage_months_scrollbar: ScrollbarState::new(),
                usage_project_filter,
                usage_projects_list,
                usage_projects_scrollbar: ScrollbarState::new(),
                usage_projects_rows: RefCell::new(Vec::new()),
                usage_projects_scale: Cell::new((0.0, true)),
                usage_chart_hover: None,
                usage_chart_bounds: Rc::default(),
                computer_use_app_icons: RefCell::new(HashMap::new()),
                computer_use_app_icon_loads: RefCell::new(HashSet::new()),
                open_in_apps: Rc::new(Vec::new()),
                model_picker_highlight: None,
                model_picker_list: ListState::new(0, ListAlignment::Top, px(512.0))
                    .with_uniform_item_height(composer::MODEL_PICKER_ROW_HEIGHT),
                model_picker_scrollbar: ScrollbarState::new(),
                route_class_highlight: None,
                route_class_scroll: ScrollHandle::new(),
                route_class_scrollbar: ScrollbarState::new(),
                route_class_picker: None,
                model_picker_empty_focus,
                branch_picker_mode: BranchPickerMode::Browse,
                branch_picker_highlight: None,
                branch_picker_list_state,
                branch_picker_row_cache: RefCell::new(Vec::new()),
                branch_snapshots: QueryCache::new(MAX_CACHED_WORKSPACES),
                visible_branch_snapshot: None,
                branch_operation_pending: false,
                commit_dialog: None,
                issue_dialog: None,
                last_created_issue: None,
                archive_dialog: None,
                archive_preview_pending: HashSet::new(),
                shortcuts_dialog: None,
                goal_dialog: None,
                goal_dialog_request: None,
                pending_goal_operations: HashMap::new(),
                goal_runtime_starts: HashSet::new(),
                goal_observed_at: HashMap::new(),
                commit_operation: None,
                // Providers × workspaces; both scans are small, the cache
                // only exists to keep them off the frame path.
                slash_commands: QueryCache::new(2 * MAX_CACHED_WORKSPACES),
                slash_command_index: Rc::new(Vec::new()),
                slash_command_index_key: None,
                slash_command_index_loading: false,
                mention_files: QueryCache::new(MAX_CACHED_WORKSPACES),
                mention_file_index: Rc::new(Vec::new()),
                mention_file_index_path: None,
                mention_file_index_loading: false,
                work_item_mentions: HashMap::new(),
                composer_sources_stale: false,
                composer_autocomplete: autocomplete::AutocompleteUi::new(),
                composer_attachments,
                composer_pasted_blocks: Vec::new(),
                image_preview: None,
                image_preview_generation: 0,
                remote_images: RefCell::new(HashMap::new()),
                event_wake_tx,
                task_state_sync_tx,
                task_state_sync_events,
                daemon_settings_tx,
                daemon_settings_events,
                friends_state: waku_client::friends::FriendsState::default(),
                friends_tx,
                friends_events,
                route_policy_tx,
                route_policy_events,
                route_policy: None,
                route_policy_pending: false,
                runtimes: HashMap::new(),
                runtime_attach_pending: HashSet::new(),
                runtime_attach_misses: HashMap::new(),
                background_work: HashMap::new(),
                last_background_work_tick: Instant::now(),
                submission_preparations: HashSet::new(),
                escape_stop_confirmation: EscapeStopConfirmation::default(),
                response_fork_preparations: HashMap::new(),
                pending_queue_drains: Vec::new(),
                pending_workspace_cleanups: HashSet::new(),
                stream_state_dirty: false,
                last_stream_save: Instant::now(),
                activities_expanded: HashMap::new(),
                expanded_activity_items: HashMap::new(),
                expanded_turns: HashSet::new(),
                expanded_changed_files: HashSet::new(),
                changed_files_diff_hover: None,
                changed_files_diffs: HashMap::new(),
                changed_files_diff_generation: 0,
                transcript_control_focuses: RefCell::new(HashMap::new()),
                session_navigation,
                session_rename: None,
                terminal_rename: None,
                session_rename_input,
                // The Terminals group starts folded every launch — its rows
                // are opt-in, unlike the session history below them.
                sidebar_collapsed_groups: HashSet::from([SidebarGroup::Terminals]),
                sidebar_shortcut_hints: false,
                sidebar_shortcut_hint_generation: 0,
                sidebar_shortcut_hint_chord_used: false,
                sidebar_project_reveal_counts: HashMap::new(),
                sidebar_group_header_focuses: RefCell::new(HashMap::new()),
                sidebar_group_compose_focuses: RefCell::new(HashMap::new()),
                sidebar_session_archive_focuses: RefCell::new(HashMap::new()),
                sidebar_session_pin_focuses: RefCell::new(HashMap::new()),
                sidebar_terminal_close_focuses: RefCell::new(HashMap::new()),
                sidebar_terminal_pin_focuses: RefCell::new(HashMap::new()),
                sidebar_show_more_focuses: RefCell::new(HashMap::new()),
                sidebar_visible,
                sidebar_width,
                right_panel_visible,
                right_panel_width,
                git_panel_visible,
                git_panel_top_height,
                git_panel: None,
                git_panel_operation: None,
                git_panel_generation: 0,
                git_panel_file_diffs: HashMap::new(),
                git_panel_hover: None,
                git_panel_hover_generation: 0,
                git_panel_commit_hover: None,
                git_panel_commit_hover_generation: 0,
                transcript_commit_hover: None,
                transcript_commit_details: HashMap::new(),
                transcript_commit_press: None,
                git_panel_sync_conflict: None,
                git_panel_conflict_files_scroll: ScrollHandle::new(),
                git_panel_conflict_files_scrollbar: ScrollbarState::new(),
                git_panel_unstaged_prompt: false,
                git_panel_commit_diff: None,
                git_panel_modal_focus: cx.focus_handle(),
                git_panel_unstaged_cancel_focus: cx.focus_handle(),
                git_panel_unstaged_confirm_focus: cx.focus_handle(),
                git_panel_conflict_abort_focus: cx.focus_handle(),
                git_panel_conflict_merge_focus: cx.focus_handle(),
                git_panel_conflict_resolve_focus: cx.focus_handle(),
                right_panel_pending_diff_file: None,
                sidebar_slide: None,
                right_panel_slide: None,
                sidebar_rendered_width: if sidebar_visible { sidebar_width } else { 0.0 },
                right_panel_rendered_width: if right_panel_visible || git_panel_visible {
                    right_panel_width
                } else {
                    0.0
                },
                sidebar_peek: SidebarPeek::Hidden,
                sidebar_peek_menu_hold: false,
                sidebar_peek_action_hold: false,
                fullscreen_surface: None,
                panel_fullscreen_slide: None,
                panel_fullscreen_rendered_width: if right_panel_visible || git_panel_visible {
                    right_panel_width
                } else {
                    0.0
                },
                fps_counter_visible: false,
                panel_resize_drag: None,
                computer_use_preview_position: None,
                right_panel_session_states: HashMap::new(),
                right_panel_detached_state: RightPanelSessionState::empty(false),
                right_panel_surfaces: Vec::new(),
                right_panel_active_surface: None,
                right_panel_tabs_scroll_handle: ScrollHandle::new(),
                right_panel_files_scroll_handle: ScrollHandle::new(),
                right_panel_files_scrollbar: ScrollbarState::new(),
                right_panel_diff_filter,
                right_panel_diff_list_state: ListState::new(0, ListAlignment::Top, px(512.0)),
                right_panel_diff_scrollbar: ScrollbarState::new(),
                right_panel_diff_selection: TranscriptSelection::default(),
                right_panel_diff_tree_list_state: ListState::new(0, ListAlignment::Top, px(180.0))
                    .with_uniform_item_height(px(30.0)),
                right_panel_diff_tree_scrollbar: ScrollbarState::new(),
                right_panel_editor_scroll_handle: ScrollHandle::new(),
                right_panel_editor_scrollbar: ScrollbarState::new(),
                file_preview_markdown: RefCell::new(None),
                file_preview_selection: TranscriptSelection::default(),
                file_preview_scroll_handle: ScrollHandle::new(),
                file_preview_scrollbar: ScrollbarState::new(),
                right_panel_pending_tab_reveal: None,
                right_panel_pending_file_focus: None,
                right_panel_pending_terminal_focus: None,
                right_panel_last_focused_terminal: None,
                right_panel_expanded_paths: HashSet::new(),
                right_panel_files_selected_path: None,
                right_panel_file_tree_width: DEFAULT_FILE_TREE_WIDTH,
                right_panel_file_editors: HashMap::new(),
                right_panel_pr_states: HashMap::new(),
                file_search: None,
                go_to_line: None,
                right_panel_diff_source: ReviewDiffSource::default(),
                right_panel_diff_snapshot: None,
                right_panel_diff_loading: false,
                right_panel_diff_error: None,
                right_panel_diff_generation: 0,
                right_panel_diff_selected_file: None,
                right_panel_diff_expanded_paths: HashSet::new(),
                right_panel_diff_tree_rows: RefCell::new(Vec::new()),
                right_panel_diff_tree_cursor: None,
                right_panel_working_tree: Vec::new(),
                working_trees: QueryCache::new(MAX_CACHED_WORKSPACES),
                workspace_queries_stale: false,
                right_panel_terminals: HashMap::new(),
                terminal_records: HashMap::new(),
                terminal_order: Vec::new(),
                unseen_terminal_completions: HashSet::new(),
                selected_terminal: None,
                last_visible_terminal: None,
                right_panel_terminal_commands: HashMap::new(),
                custom_command_runs: HashMap::new(),
                provider_setup_terminals: HashMap::new(),
                agy_terminals: HashMap::new(),
                agy_last_visible: HashMap::new(),
                agy_spawned_at: HashMap::new(),
                agy_poll_tx,
                agy_poll_events,
                agy_pending_spawns: HashSet::new(),
                agy_poll_pending: false,
                right_panel_browsers: HashMap::new(),
                right_panel_pending_browser_focus: None,
                scene_overlay_enabled,
                settings_page: None,
                dev_state_path,
                auto_restart_enabled,
                keybindings: None,
                custom_command_editor: None,
                remote_host_editor: None,
                skills_catalog: None,
                skills_catalogs: HashMap::new(),
                skill_hosts: HashMap::new(),
                skills_scan_generation: 0,
                skills_scan_pending: false,
                skills_scanned_at: None,
                skills_search,
                skills_list_state: ListState::new(0, ListAlignment::Top, px(512.0)),
                skills_scrollbar: ScrollbarState::new(),
                skills_rows: RefCell::new(Vec::new()),
                skills_selected: None,
                skills_detail_markdown: RefCell::new(None),
                skills_selection: TranscriptSelection::default(),
                skills_detail_scroll: ScrollHandle::new(),
                skills_detail_scrollbar: ScrollbarState::new(),
                skills_source_filter: None,
                skills_delete_arming: None,
                settings_scroll: ScrollHandle::new(),
                settings_scrollbar: ScrollbarState::new(),
                settings_search_sections: Vec::new(),
                settings_search_target: None,
                archived_search,
                archived_project_filter: None,
                archived_sessions_list: ListState::new(0, ListAlignment::Top, px(256.0)),
                archived_sessions_scrollbar: ScrollbarState::new(),
                archived_session_rows: RefCell::new(Vec::new()),
                completion_volume_slider: SliderState::new(),
                sidebar_transparency_slider: SliderState::new(),
                border_intensity_slider: SliderState::new(),
                theme_preview_active: false,
                theme_preview_expanded: false,
                header_drag_armed: false,
                toast: startup_toast.map(|message| ToastState {
                    message,
                    detail: None,
                    tone: ToastTone::Alert,
                    action: None,
                    id: 0,
                    timer_generation: 0,
                    duration_remaining: DEFAULT_TOAST_DURATION,
                    timer_started: None,
                    hovered: false,
                    localhost: None,
                }),
                toast_generation: 0,
                pending_localhost_toasts: VecDeque::new(),
                copied_control_feedback: HashMap::new(),
                copied_control_generation: 0,
                copied_message_feedback: HashMap::new(),
                copied_message_generation: 0,
                copied_activity_feedback: HashMap::new(),
                copied_activity_generation: 0,
                message_edit: None,
                transcript_rows,
                anchored_transcript_rows,
                sidebar_list_state,
                sidebar_scrollbar: ScrollbarState::new(),
                sidebar_row_cache: RefCell::new(Vec::new()),
                sidebar_rows_fingerprint: Cell::new(None),
                sidebar_rows_snapshot: RefCell::new(Rc::new(Vec::new())),
                sidebar_collapsed_group_members: RefCell::new(Rc::new(HashMap::new())),
                sidebar_branch_labels: RefCell::new(HashMap::new()),
                sidebar_branch_scan_fingerprint: Cell::new(None),
                sidebar_branch_scan_generation: Cell::new(0),
                friends_probe_generation: Cell::new(0),
                sidebar_terminal_repo_roots: RefCell::new(HashMap::new()),
                sidebar_terminal_repo_scan_fingerprint: Cell::new(None),
                sidebar_terminal_repo_scan_generation: Cell::new(0),
                sidebar_checkout_statuses: RefCell::new(HashMap::new()),
                sidebar_checkout_scan_fingerprint: Cell::new(None),
                sidebar_checkout_scan_generation: Cell::new(0),
                sidebar_checkout_scanned_at: Cell::new(None),
                sidebar_pull_requests: RefCell::new(HashMap::new()),
                sidebar_pull_request_scan_fingerprint: Cell::new(None),
                sidebar_pull_request_scan_generation: Cell::new(0),
                github_browsers: HashMap::new(),
                notifications: notifications::Inbox::new(window, cx),
                projects_page: None,
                last_projects_page_project: None,
                projects_page_states: HashMap::new(),
                drafts_page: false,
                draft_use_undos: Vec::new(),
                drafts_search,
                drafts_show_hidden: false,
                drafts_list_state: ListState::new(0, ListAlignment::Top, px(640.0)),
                drafts_scrollbar: ScrollbarState::new(),
                drafts_rows: RefCell::new(Vec::new()),
                drafts_editing: None,
                drafts_edit_input,
                settings_git_project: None,
                git_page_refresh_pending: false,
                missing_projects: HashSet::new(),
                project_location_generation: Cell::new(0),
                transcript_row_kinds: RefCell::new(Vec::new()),
                transcript_row_kinds_fingerprint: Cell::new(None),
                working_indicator_session: Cell::new(None),
                working_indicator_fade: Cell::new(None),
                transcript_navigation_turns: RefCell::new(Rc::new(Vec::new())),
                transcript_navigation_turns_fingerprint: Cell::new(None),
                assistant_footer_cache: RefCell::new(HashMap::new()),
                assistant_footer_fingerprint: Cell::new(None),
                hovered_response_row: None,
                checkpoint_ref_cache: RefCell::new(HashMap::new()),
                checkpoint_ref_generation: Cell::new(0),
                checkpoint_ref_prefetch: Cell::new(None),
                pending_checkpoint_captures: interrupted_turn_checkpoints,
                checkpoint_captures_in_flight: HashSet::new(),
                last_idle_session_sweep: Instant::now(),
                transcript_anchor: Cell::new(None),
                transcript_anchor_end_space: Rc::new(Cell::new(Pixels::ZERO)),
                transcript_anchor_following,
                transcript_tail_recheck,
                transcript_is_scrolled,
                transcript_last_wheel_scroll,
                transcript_scroll_positions: HashMap::new(),
                transcript_landing: None,
                startup_scroll_restores: HashSet::new(),
                pending_sidebar_scroll: Cell::new(None),
                transcript_scroll_to_bottom_visible: Cell::new(false),
                transcript_scrollbar_dragging: Cell::new(false),
                transcript_layout_width: Cell::new(Pixels::ZERO),
                message_markdown: RefCell::new(HashMap::new()),
                user_message_viewports: RefCell::new(HashMap::new()),
                expanded_user_messages: HashSet::new(),
                user_message_expand_focuses: RefCell::new(HashMap::new()),
                activity_markdown: RefCell::new(HashMap::new()),
                reasoning_window_starts: RefCell::new(HashMap::new()),
                activity_scroll_viewports: RefCell::new(HashMap::new()),
                activity_diffs: RefCell::new(HashMap::new()),
                activity_diff_viewports: RefCell::new(HashMap::new()),
                markdown_link_handler,
                transcript_selection,
                transcript_annotations: HashMap::new(),
                annotation_session: initial_session,
                annotation_next_id,
                annotation_editor: None,
                annotation_comment_input,
                annotation_hover: None,
                annotation_press: None,
                sent_annotations: HashMap::new(),
                queued_annotations: HashMap::new(),
                pending_file_annotations,
                annotation_ref_hover: None,
                annotation_ref_sets: RefCell::new(HashMap::new()),
                annotation_ref_sets_fingerprint: Cell::new(None),
                window_handle: window.window_handle(),
                transcript_focus: cx.focus_handle(),
                transcript_search: None,
                toast_selection: TranscriptSelection::default(),
                transcript_scrollbar: ScrollbarState::new(),
                composer_lane_height: Rc::new(Cell::new(0.0)),
                menus: RefCell::new(HashMap::new()),
                navigation_rail: navigation_rail.clone(),
                navigation_rail_reset_generation: Cell::new(0),
                sidebar_pane: sidebar_pane.clone(),
                transcript_pane: transcript_pane.clone(),
                right_panel_pane: right_panel_pane.clone(),
                git_panel_pane: git_panel_pane.clone(),
                time_label_wake: Cell::new(None),
                time_label_wake_generation: Cell::new(0),
                fps_last_frame: Instant::now(),
                fps_frame_count: 0,
                fps_value: 0,
            }
        });
        navigation_rail.update(cx, |rail, _| rail.set_waku(entity.downgrade()));
        for pane in [
            &sidebar_pane,
            &transcript_pane,
            &right_panel_pane,
            &git_panel_pane,
        ] {
            pane.update(cx, |pane, cx| pane.bind(&entity, cx));
        }
        let initial_row_count = entity.read(cx).transcript_row_count();
        entity.read(cx).reset_transcript_rows(initial_row_count);
        // Everything launch needs from `git` or the filesystem, started now
        // that there is an entity to notify and deliberately not before the
        // first frame.
        entity.update(cx, |this, cx| {
            // First, before anything else can save over the persisted copy:
            // the UI state carried across the last quit — history, scroll
            // positions, open panels.
            this.restore_ui_state(window, cx);
            this.start_task_state_sync(waku_client::DaemonKey::Local, this.daemon.clone());
            this.connect_remote_hosts(cx);
            for session_id in startup_live_session_ids {
                // Antigravity's runtime is the app-local TUI terminal, not a
                // daemon attachment. It respawns lazily — only the selected
                // session's surface exists at launch.
                let is_agy = this
                    .state
                    .sessions
                    .iter()
                    .any(|session| {
                        session.id == session_id
                            && session.provider == ProviderKind::Antigravity
                    });
                if is_agy {
                    if this.state.selected_session == Some(session_id) {
                        this.ensure_agy_terminal(session_id, cx);
                    }
                } else {
                    this.start_runtime_attachment(session_id, cx);
                }
            }
            this.start_pending_checkpoint_captures(cx);
            // The autocomplete indexes prefetch alongside, so typing `/` or
            // `@` into the very first prompt already has data to draw.
            this.refresh_composer_sources(cx);
            // Re-detect providers after resolving the user's login-shell
            // environment off-thread. Detection then starts model and version
            // discovery for every CLI it finds, including nvm/fnm-managed
            // installs.
            this.refresh_provider_detection(None);
            // The skill library too: the Skills settings page must open onto
            // data, not a scan.
            this.ensure_skills_catalog(false, cx);
            // And the header's "open project in app" targets, so its menu
            // lists installed apps and icons without ever probing on a frame.
            this.detect_open_in_apps(cx);
            // Reconcile project paths with the filesystem: bookmark backfill,
            // rename auto-heal, and missing-folder marking.
            this.refresh_project_locations(cx);
        });
        entity
    }
}

#[cfg(test)]
mod tests;
