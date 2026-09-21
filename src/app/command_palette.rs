//! The window-wide command palette opened with the platform's primary shortcut + K.
//!
//! The search field keeps real focus while the list cursor is drawn, matching
//! native pickers and Zed's command palette. Metadata results rebuild when the
//! query changes; persisted transcript matches join from a debounced background
//! SQLite scan. Caret blinks therefore repaint one in-memory snapshot instead
//! of re-fuzzy-matching history or touching storage every frame.

use gpui::{Action, KeyBinding, StyledText, TextRun, actions};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
use waku_protocol::workspace::{GitHubAvailability, GitHubRepoRef};

use super::*;
use crate::ui::shortcut::ShortcutHint;

actions!(
    waku_command_palette,
    [
        SelectNext,
        SelectPrevious,
        SelectFirst,
        SelectLast,
        SelectPageDown,
        SelectPageUp,
        Confirm,
        Dismiss
    ]
);

const SEARCH_CONTEXT: &str = "CommandPalette > TextInput";
const MAX_TASK_RESULTS: usize = 12;
const MAX_RESUME_RESULTS: usize = 30;
const PROVIDER_SESSION_CATALOG_LIMIT: usize = 250;
const MESSAGE_SEARCH_LIMIT: usize = 50;
const MESSAGE_SEARCH_CACHE_CAPACITY: usize = 24;
const PAGE_STEP: isize = 7;
const MESSAGE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(90);
const SEARCH_ROW_HEIGHT: f32 = 60.0;
const SECTION_HEADER_HEIGHT: f32 = 30.0;
const PROVIDER_SECTION_TOP_MARGIN: f32 = 8.0;
const RESULT_ROW_HEIGHT: f32 = 44.0;
const CONTENT_RESULT_ROW_HEIGHT: f32 = 60.0;
const EMPTY_RESULTS_HEIGHT: f32 = 180.0;
const RESULTS_BOTTOM_PADDING: f32 = 8.0;
/// How far below each search root the "New task in…" scan descends — deep
/// enough for `~/dev/group/repo`, shallow enough to stay out of dependency
/// and generated trees.
const DIRECTORY_SEARCH_DEPTH: usize = 4;
const DIRECTORY_SEARCH_CAP: usize = 5_000;
const MAX_DIRECTORY_RESULTS: usize = 50;
const FOOTER_HEIGHT: f32 = 30.0;
const MAX_CARD_HEIGHT: f32 = 480.0;

/// Bind list navigation beneath the focused one-line input. This is registered
/// after the input's bindings, although the more-specific key context would
/// win either way.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(SEARCH_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(SEARCH_CONTEXT)),
        KeyBinding::new("ctrl-n", SelectNext, Some(SEARCH_CONTEXT)),
        KeyBinding::new("ctrl-p", SelectPrevious, Some(SEARCH_CONTEXT)),
        KeyBinding::new("tab", SelectNext, Some(SEARCH_CONTEXT)),
        KeyBinding::new("shift-tab", SelectPrevious, Some(SEARCH_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(SEARCH_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(SEARCH_CONTEXT)),
        KeyBinding::new("pagedown", SelectPageDown, Some(SEARCH_CONTEXT)),
        KeyBinding::new("pageup", SelectPageUp, Some(SEARCH_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(SEARCH_CONTEXT)),
        // Bound at the palette, not the field: the query field's own
        // clear-on-escape outranks this (deeper context) while it has text,
        // and an empty field propagates the keystroke down to it.
        KeyBinding::new("escape", Dismiss, Some("CommandPalette")),
    ]);
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PaletteSection {
    Suggested,
    Tasks,
    Sessions,
    Providers,
    Commands,
    CustomCommands,
    Prompts,
    Settings,
    // Drill-in sections; they never appear in Commands view.
    Projects,
    Scripts,
    // The "New task in…" picker's directory listing; never in Commands view.
    Directories,
    Templates,
    // The "Change base branch" picker's branch listing; never in Commands
    // view.
    Branches,
}

impl PaletteSection {
    fn label(self) -> String {
        crate::i18n::translate(match self {
            Self::Suggested => "command_palette.suggested",
            Self::Tasks => "command_palette.tasks",
            Self::Sessions => "command_palette.sessions",
            Self::Providers => "command_palette.providers",
            Self::Commands => "command_palette.commands",
            Self::CustomCommands => "command_palette.custom_commands",
            Self::Prompts => "command_palette.prompts",
            Self::Settings => "command_palette.settings",
            Self::Projects => "command_palette.projects",
            Self::Scripts => "command_palette.scripts",
            Self::Directories => "command_palette.directories",
            Self::Templates => "command_palette.templates",
            Self::Branches => "command_palette.branches",
        })
    }

    fn query_rank(self) -> usize {
        match self {
            Self::Commands | Self::Suggested | Self::Sessions | Self::Providers => 0,
            Self::CustomCommands | Self::Prompts => 1,
            Self::Tasks => 2,
            Self::Settings => 3,
            Self::Projects
            | Self::Scripts
            | Self::Directories
            | Self::Templates
            | Self::Branches => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaletteIcon {
    Asset(&'static str),
    Provider(ProviderKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaletteIdentifier {
    TaskId,
    AgentCliThreadId,
}

impl PaletteIdentifier {
    const ALL: [Self; 2] = [Self::TaskId, Self::AgentCliThreadId];

    fn value(self, session: Option<&AgentSession>) -> Option<String> {
        let session = session?;
        match self {
            Self::TaskId => Some(session.id.to_string()),
            Self::AgentCliThreadId => session.provider_native_id().map(str::to_owned),
        }
    }

    fn label(self) -> String {
        tr!(match self {
            Self::TaskId => "command_palette.copy_task_id",
            Self::AgentCliThreadId => "command_palette.copy_agent_cli_thread_id",
        })
    }

    fn copied_message(self) -> String {
        tr!(match self {
            Self::TaskId => "command_palette.task_id_copied",
            Self::AgentCliThreadId => "command_palette.agent_cli_thread_id_copied",
        })
    }

    fn keywords(self) -> &'static str {
        match self {
            Self::TaskId => "copy task id uuid identifier session debug",
            Self::AgentCliThreadId => {
                "copy agent cli thread id native session uuid identifier codex claude debug"
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PaletteAction {
    NewTask,
    NewTaskIn,
    NewTaskInDirectory(PathBuf),
    NewTaskInSameWorktree,
    Resume,
    ChooseResumeProvider,
    SelectResumeProvider(ProviderKind),
    ResumeProviderSession(waku_client::DaemonKey, ProviderSessionSummary),
    OpenProject,
    OpenRemoveProject,
    RemoveProject(Uuid),
    FocusComposer,
    CreateDraft,
    ViewDrafts,
    CopyIdentifier(PaletteIdentifier),
    ChooseModel,
    ToggleWorkspace,
    OpenOnGitHub,
    MoveToWorktree,
    LandChanges,
    ChangeBaseBranch,
    RebaseOntoBranch {
        workspace: PathBuf,
        branch: String,
        onto: Option<String>,
    },
    SyncBranch,
    CompactContext,
    ToggleUsage,
    CheckForUpdates,
    CollapseSidebarGroups,
    GoToNextUnreadCompletion,
    MarkAllSessionsRead,
    ToggleSidebar,
    ToggleRightPanel,
    OpenSettings(SettingsPage),
    SelectTask(Uuid),
    RunCustomCommand(Uuid),
    NewCustomCommand,
    InsertPromptTemplate(SlashCommand),
    OpenSavePrompt,
    SavePromptAs(String),
    RevealPromptTemplates,
    InspectElements,
    InspectColors,
    ToggleAutoRestart,
    OpenRunScript,
    ChooseRunScriptProject(Uuid),
    RunScript {
        project: Uuid,
        script: run_script::ProjectScript,
    },
    CreateGitHubIssue,
    ChooseIssueProject(Uuid),
    ChooseIssueTemplate(waku_protocol::workspace::IssueTemplate),
    NewBlankIssue,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CommandPaletteView {
    #[default]
    Commands,
    Resume,
    ResumeProviders,
    RunScriptProjects,
    RunScripts,
    /// The "Remove project…" project picker.
    RemoveProject,
    /// The "New task in…" directory picker.
    NewTaskIn,
    IssueProjects,
    IssueTemplates,
    /// The "Change base branch" branch picker.
    RebaseBase,
    /// The "Save as prompt template" step: the query field is the file's
    /// command name, confirmed with Enter.
    SavePrompt,
}

/// What a resolved template fetch does next: a repo with templates shows
/// the picker, a templateless one opens the form directly, and a repo
/// `gh` cannot talk to stops the flow with its reason.
enum IssueTemplateFetch {
    ShowTemplates,
    OpenDialog,
    Toast(String),
}

/// The "Change base branch" drill-in's target and branch scan.
/// `current_base` is the session's recorded base — the row the picker
/// marks, and the `rebase --onto` upstream when another branch is picked.
struct RebaseBasePicker {
    workspace: PathBuf,
    current_base: Option<String>,
    /// The branch checked out in the worktree — it can never be a base.
    current_branch: Option<String>,
    branches: Vec<crate::git_branch::BranchEntry>,
    pending: bool,
}

#[derive(Clone, Debug)]
pub(super) struct CommandPaletteItem {
    section: PaletteSection,
    label: String,
    detail: Option<String>,
    icon: PaletteIcon,
    shortcut: Option<ShortcutHint>,
    action: PaletteAction,
    content_match: Option<crate::persistence::SessionMessageMatch>,
    search_text: String,
    order: usize,
    recency: u64,
}

impl CommandPaletteItem {
    fn command(
        section: PaletteSection,
        label: String,
        icon: &'static str,
        shortcut: Option<ShortcutHint>,
        action: PaletteAction,
        keywords: &'static str,
        order: usize,
    ) -> Self {
        let search_text = format!("{label} {keywords}");
        Self {
            section,
            label,
            detail: None,
            icon: PaletteIcon::Asset(icon),
            shortcut,
            action,
            content_match: None,
            search_text,
            order,
            recency: 0,
        }
    }
}

struct ScoredPaletteItem {
    score: u32,
    item: CommandPaletteItem,
}

/// Groups results by section, ordering the sections by each one's best score
/// so a strong match lifts its section above weak matches elsewhere — a
/// keyword-stuffed command must not outrank a literal custom-command hit just
/// because of a fixed section order. Ties fall back to browsing order, and the
/// stable sort preserves each section's internal score order.
fn order_sections_by_best_score(scored_results: &mut [ScoredPaletteItem]) {
    let mut best: Vec<(PaletteSection, u32)> = Vec::new();
    for scored in scored_results.iter() {
        match best
            .iter_mut()
            .find(|(section, _)| *section == scored.item.section)
        {
            Some((_, score)) => *score = (*score).max(scored.score),
            None => best.push((scored.item.section, scored.score)),
        }
    }
    best.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.query_rank().cmp(&b.0.query_rank()))
            .then(a.0.cmp(&b.0))
    });
    scored_results.sort_by_key(|scored| {
        best.iter()
            .position(|(section, _)| *section == scored.item.section)
            .unwrap_or(usize::MAX)
    });
}

/// The user-level template directory — scanned into the composer's command
/// index by `assemble_slash_commands` alongside the project's `.goddard`.
fn prompt_templates_dir(home: &Path) -> PathBuf {
    home.join(".config/goddard/commands")
}

/// A typed name to the file stem it saves as and the `/` command it becomes:
/// lowercase alphanumeric runs joined by single dashes.
fn slugify_prompt_name(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>()
        .trim_end_matches('-')
        .to_owned()
}

pub(super) fn next_selection_index(selected: usize, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let selected = selected.min(len - 1);
    Some(if delta == isize::MIN {
        0
    } else if delta == isize::MAX {
        len - 1
    } else if delta.unsigned_abs() > 1 {
        (selected as isize + delta).clamp(0, len.saturating_sub(1) as isize) as usize
    } else {
        (selected as isize + delta).rem_euclid(len as isize) as usize
    })
}

pub(super) fn palette_content_match_text(
    matched: &crate::persistence::SessionMessageMatch,
    query: &str,
    window: &Window,
    theme: Theme,
) -> StyledText {
    let (source_label, source_color) = match matched.source {
        MessageRole::User => (tr!("command_palette.you"), theme.gauge),
        MessageRole::Assistant | MessageRole::System => {
            (tr!("command_palette.agent"), theme.success)
        }
    };
    let label = format!("{source_label}: ");
    let mut text = String::with_capacity(label.len() + matched.snippet.len());
    text.push_str(&label);
    text.push_str(&matched.snippet);

    let mut normal_font = window.text_style().font();
    normal_font.weight = FontWeight::NORMAL;
    let mut emphasized_font = normal_font.clone();
    emphasized_font.weight = FontWeight::SEMIBOLD;
    let mut runs = vec![TextRun {
        len: label.len(),
        font: emphasized_font.clone(),
        color: source_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }];

    let query = query.trim();
    let match_range = (!query.is_empty())
        .then(|| {
            matched
                .snippet
                .to_ascii_lowercase()
                .find(&query.to_ascii_lowercase())
                .map(|start| start..start + query.len())
        })
        .flatten();
    let mut push = |len: usize, font, color| {
        if len > 0 {
            runs.push(TextRun {
                len,
                font,
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
    };
    if let Some(range) = match_range {
        push(range.start, normal_font.clone(), theme.text_tertiary);
        push(range.len(), emphasized_font, theme.text_secondary);
        push(
            matched.snippet.len().saturating_sub(range.end),
            normal_font,
            theme.text_tertiary,
        );
    } else {
        push(matched.snippet.len(), normal_font, theme.text_tertiary);
    }
    StyledText::new(text).with_runs(runs)
}

fn command_palette_row_height(item: &CommandPaletteItem) -> f32 {
    if item.content_match.is_some() {
        CONTENT_RESULT_ROW_HEIGHT
    } else {
        RESULT_ROW_HEIGHT
    }
}

fn should_show_command_palette_empty_state(result_count: usize, search_pending: bool) -> bool {
    result_count == 0 && !search_pending
}

fn should_keep_previous_command_palette_results(
    next_result_count: usize,
    search_pending: bool,
    previous_result_count: usize,
) -> bool {
    next_result_count == 0 && search_pending && previous_result_count > 0
}

fn same_provider_session(left: &ProviderResumeCursor, right: &ProviderResumeCursor) -> bool {
    left.provider() == right.provider() && left.native_id() == right.native_id()
}

fn command_palette_results_height(results: &[CommandPaletteItem], show_empty_state: bool) -> f32 {
    let content_height = if show_empty_state {
        EMPTY_RESULTS_HEIGHT
    } else {
        let mut previous_section = None;
        results
            .iter()
            .map(|item| {
                let section_leading_height = if previous_section != Some(item.section) {
                    if item.section == PaletteSection::Providers {
                        PROVIDER_SECTION_TOP_MARGIN
                    } else {
                        SECTION_HEADER_HEIGHT
                    }
                } else {
                    0.0
                };
                if previous_section != Some(item.section) {
                    previous_section = Some(item.section);
                }
                section_leading_height + command_palette_row_height(item)
            })
            .sum()
    };
    content_height + RESULTS_BOTTOM_PADDING
}

pub(super) struct CommandPaletteUi {
    search: Entity<TextInput>,
    open: bool,
    focus_generation: u64,
    previous_focus: Option<FocusHandle>,
    view: CommandPaletteView,
    results: Vec<CommandPaletteItem>,
    message_searches: QueryCache<String, Vec<crate::persistence::SessionMessageMatch>>,
    active_message_query: Option<String>,
    message_matches_query: Option<String>,
    message_matches: HashMap<Uuid, crate::persistence::SessionMessageMatch>,
    message_search_pending: bool,
    provider_sessions: Vec<(waku_client::DaemonKey, ProviderSessionSummary)>,
    resume_provider: ProviderKind,
    provider_sessions_pending: bool,
    provider_session_import: Option<ProviderResumeCursor>,
    provider_session_error: Option<String>,
    provider_session_status: ProviderSessionCatalogStatus,
    provider_session_generation: u64,
    /// The project the run-script drill-in scoped to, and the scan it
    /// produced.
    run_script_project: Option<Uuid>,
    run_scripts: Vec<run_script::ProjectScript>,
    run_scripts_pending: bool,
    run_script_generation: u64,
    /// The daemon's directory scan behind the "New task in…" picker, fetched
    /// once per view entry and filtered per keystroke.
    new_task_directories: Vec<PathBuf>,
    new_task_directories_pending: bool,
    new_task_directories_generation: u64,
    /// The new-issue flow's resolved repository and the template scan it
    /// kicked off. `issue_repo` doubles as the availability gate: the
    /// dialog only opens once the host answered a repo.
    issue_target: Option<issue_dialog::IssueTarget>,
    issue_repo: Option<(Option<GitHubRepoRef>, GitHubAvailability)>,
    issue_templates: Vec<waku_protocol::workspace::IssueTemplate>,
    issue_templates_pending: bool,
    issue_blank_enabled: bool,
    issue_generation: u64,
    /// The worktree the "Change base branch" drill-in is rebasing, and the
    /// daemon's branch scan behind the picker.
    rebase_base: Option<RebaseBasePicker>,
    rebase_generation: u64,
    /// The composer text the "Save as prompt template" step parks on entry —
    /// captured up front so a composer draft change mid-pick can't alter
    /// what lands in the file.
    save_prompt_body: Option<String>,
    selected: usize,
    scroll: ScrollHandle,
    matcher: Matcher,
}

impl CommandPaletteUi {
    pub(super) fn new(search: Entity<TextInput>) -> Self {
        Self {
            search,
            open: false,
            focus_generation: 0,
            previous_focus: None,
            view: CommandPaletteView::Commands,
            results: Vec::new(),
            message_searches: QueryCache::new(MESSAGE_SEARCH_CACHE_CAPACITY),
            active_message_query: None,
            message_matches_query: None,
            message_matches: HashMap::new(),
            message_search_pending: false,
            provider_sessions: Vec::new(),
            resume_provider: ProviderKind::default(),
            provider_sessions_pending: false,
            provider_session_import: None,
            provider_session_error: None,
            provider_session_status: ProviderSessionCatalogStatus::Ready,
            provider_session_generation: 0,
            run_script_project: None,
            run_scripts: Vec::new(),
            run_scripts_pending: false,
            run_script_generation: 0,
            new_task_directories: Vec::new(),
            new_task_directories_pending: false,
            new_task_directories_generation: 0,
            issue_target: None,
            issue_repo: None,
            issue_templates: Vec::new(),
            issue_templates_pending: false,
            issue_blank_enabled: true,
            issue_generation: 0,
            rebase_base: None,
            rebase_generation: 0,
            save_prompt_body: None,
            selected: 0,
            scroll: ScrollHandle::new(),
            // Plain config: `match_paths` biases toward path basenames, which
            // is right for file pickers but skews label/keyword scoring here.
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }
}

impl Waku {
    pub(super) fn open_resume_picker_action(
        &mut self,
        _: &OpenResumePicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.command_palette.open {
            self.open_command_palette(window, cx);
        }
        self.open_command_palette_resume_view(None, cx);
    }

    /// ⌘R — the run-a-script drill-in opens straight onto its project step.
    /// A remote daemon has no local PTY, so the flow stops before the modal.
    pub(super) fn run_project_script_action(
        &mut self,
        _: &RunProjectScript,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Scripts run in a desktop terminal, so only local-daemon projects
        // qualify — refuse outright when none exist.
        if self
            .state
            .projects
            .iter()
            .all(|project| self.is_remote_project(project.id))
        {
            self.show_toast(tr!("command_palette.run_script_remote"));
            cx.notify();
            return;
        }
        if !self.command_palette.open {
            self.open_command_palette(window, cx);
        }
        self.open_command_palette_run_script_projects_view(cx);
    }

    /// ⌘⇧N — the "New task in…" drill-in opens straight onto its directory
    /// picker: fuzzy-search a directory, and the new task gets a temporary
    /// project there.
    pub(super) fn new_task_in_action(
        &mut self,
        _: &NewTaskIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.command_palette.open {
            self.open_command_palette(window, cx);
        }
        self.open_command_palette_new_task_view(cx);
    }

    pub(super) fn toggle_command_palette_action(
        &mut self,
        _: &ToggleCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.command_palette.open {
            self.close_command_palette(window, cx);
        } else {
            self.open_command_palette(window, cx);
        }
    }

    fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // One picker at a time; closing first restores the finder's recorded
        // focus so the palette captures the element that was really focused.
        if self.file_finder.is_open() {
            self.close_file_finder(window, cx);
        }
        let open_menus = self
            .menus
            .borrow()
            .values()
            .filter(|menu| menu.is_open())
            .cloned()
            .collect::<Vec<_>>();
        // A focused picker input disappears when its menu closes. Restore to a
        // durable surface instead of remembering that soon-detached handle.
        self.command_palette.previous_focus = if open_menus.is_empty() {
            window.focused(cx)
        } else if self.settings_page.is_some() {
            Some(self.settings_focus.clone())
        } else {
            Some(self.composer_focus(cx))
        };

        self.command_palette.open = true;
        self.command_palette.view = CommandPaletteView::Commands;
        self.command_palette.focus_generation =
            self.command_palette.focus_generation.wrapping_add(1);
        self.command_palette.message_searches.clear();
        self.command_palette.active_message_query = None;
        self.command_palette.message_matches_query = None;
        self.command_palette.message_matches.clear();
        self.command_palette.message_search_pending = false;
        self.command_palette.provider_sessions.clear();
        self.command_palette.provider_sessions_pending = false;
        self.command_palette.provider_session_import = None;
        self.command_palette.provider_session_error = None;
        self.command_palette.provider_session_generation = self
            .command_palette
            .provider_session_generation
            .wrapping_add(1);
        self.command_palette.run_script_project = None;
        self.command_palette.run_scripts.clear();
        self.command_palette.run_scripts_pending = false;
        self.command_palette.run_script_generation =
            self.command_palette.run_script_generation.wrapping_add(1);
        self.command_palette.new_task_directories.clear();
        self.command_palette.new_task_directories_pending = false;
        self.command_palette.new_task_directories_generation = self
            .command_palette
            .new_task_directories_generation
            .wrapping_add(1);
        self.command_palette.issue_target = None;
        self.command_palette.issue_repo = None;
        self.command_palette.issue_templates.clear();
        self.command_palette.issue_templates_pending = false;
        self.command_palette.issue_blank_enabled = true;
        self.command_palette.issue_generation =
            self.command_palette.issue_generation.wrapping_add(1);
        self.command_palette.save_prompt_body = None;
        let focus_generation = self.command_palette.focus_generation;
        self.command_palette
            .search
            .update(cx, |input, cx| input.clear(cx));
        // The Prompts section draws from the composer's command index —
        // kick discovery now so a cold open doesn't show an empty library.
        self.refresh_composer_sources(cx);
        self.refresh_command_palette_results("", false, cx);

        // Closing an open GPUI menu can call its toggle observers back into
        // this entity, so release this action listener's mutable borrow first.
        if !open_menus.is_empty() {
            window.defer(cx, move |window, cx| {
                for menu in open_menus {
                    menu.close(window, cx);
                }
            });
        }

        // The palette is deferred onto GPUI's overlay plane. Wait for that
        // subtree to join the dispatch tree before handing focus to its input.
        let focus = self.command_palette.search.read(cx).focus();
        let weak = cx.entity().downgrade();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let mut should_focus = false;
                let _ = weak.update(cx, |this, _| {
                    should_focus = this.command_palette.open
                        && this.command_palette.focus_generation == focus_generation;
                });
                if should_focus {
                    window.focus(&focus, cx);
                }
            });
        });
        cx.notify();
    }

    pub(super) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.command_palette.open {
            return;
        }
        self.command_palette.open = false;
        self.command_palette.focus_generation =
            self.command_palette.focus_generation.wrapping_add(1);
        self.command_palette.active_message_query = None;
        self.command_palette.message_search_pending = false;
        self.command_palette.provider_sessions_pending = false;
        self.command_palette.provider_session_import = None;
        self.command_palette.provider_session_generation = self
            .command_palette
            .provider_session_generation
            .wrapping_add(1);
        self.command_palette.run_scripts_pending = false;
        self.command_palette.run_script_generation =
            self.command_palette.run_script_generation.wrapping_add(1);
        self.command_palette.new_task_directories_pending = false;
        self.command_palette.new_task_directories_generation = self
            .command_palette
            .new_task_directories_generation
            .wrapping_add(1);
        self.command_palette.issue_templates_pending = false;
        self.command_palette.issue_generation =
            self.command_palette.issue_generation.wrapping_add(1);
        if let Some(previous_focus) = self.command_palette.previous_focus.take() {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    fn leave_command_palette_resume_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::Commands;
        self.command_palette.provider_sessions.clear();
        self.command_palette.provider_sessions_pending = false;
        self.command_palette.provider_session_import = None;
        self.command_palette.provider_session_error = None;
        self.command_palette.provider_session_generation = self
            .command_palette
            .provider_session_generation
            .wrapping_add(1);
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    fn leave_command_palette_resume_provider_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::Resume;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.resume_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    fn open_command_palette_run_script_projects_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::RunScriptProjects;
        self.command_palette.run_script_project = None;
        self.command_palette.run_scripts.clear();
        self.command_palette.run_scripts_pending = false;
        self.command_palette.run_script_generation =
            self.command_palette.run_script_generation.wrapping_add(1);
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.run_script_project_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    fn open_command_palette_remove_project_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::RemoveProject;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.remove_project_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    /// The project is picked: swap the picker to its scripts and scan its
    /// root off the UI thread, generation-guarded like the resume fetch.
    fn open_command_palette_run_scripts_view(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };
        let root = project.path.clone();
        self.command_palette.view = CommandPaletteView::RunScripts;
        self.command_palette.run_script_project = Some(project_id);
        self.command_palette.run_scripts.clear();
        self.command_palette.run_scripts_pending = true;
        self.command_palette.run_script_generation =
            self.command_palette.run_script_generation.wrapping_add(1);
        let generation = self.command_palette.run_script_generation;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.run_script_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();

        cx.spawn(async move |waku, cx| {
            let scripts = cx
                .background_executor()
                .spawn(async move { run_script::discover_project_scripts(&root) })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if !waku.command_palette.open
                    || waku.command_palette.run_script_generation != generation
                    || waku.command_palette.run_script_project != Some(project_id)
                {
                    return;
                }
                waku.command_palette.run_scripts_pending = false;
                waku.command_palette.run_scripts = scripts;
                if waku.command_palette.view == CommandPaletteView::RunScripts {
                    let query = waku.command_palette.search.read(cx).content().to_owned();
                    waku.refresh_command_palette_results(&query, false, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The "Change base branch" drill-in: snapshot the composer session's
    /// worktree and recorded base, then scan its branches off the UI
    /// thread, generation-guarded like the run-script fetch.
    fn open_command_palette_rebase_base_view(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.composer_session() else {
            return;
        };
        if !session.has_started() || session.is_busy() {
            return;
        }
        let SessionWorkspace::Worktree { base_branch, .. } = &session.workspace else {
            return;
        };
        let current_base = base_branch.clone();
        let Some(workspace) = self
            .workspace_path_for_session(session)
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let generation = self.command_palette.rebase_generation.wrapping_add(1);
        self.command_palette.rebase_generation = generation;
        self.command_palette.rebase_base = Some(RebaseBasePicker {
            workspace: workspace.clone(),
            current_base: current_base.clone(),
            current_branch: None,
            branches: Vec::new(),
            pending: true,
        });
        self.command_palette.view = CommandPaletteView::RebaseBase;
        let placeholder = match &current_base {
            Some(base) => tr!(
                "command_palette.rebase_base_placeholder",
                base = base.clone()
            ),
            None => tr!("command_palette.rebase_base_placeholder_unknown"),
        };
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();

        let Some(client) = self.workspace_client_for_path(&workspace) else {
            if let Some(picker) = self.command_palette.rebase_base.as_mut() {
                picker.pending = false;
            }
            self.refresh_command_palette_results("", false, cx);
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(waku_client::WorkspaceOperation::InspectBranches {
                        cwd: workspace,
                    })
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if !waku.command_palette.open
                    || waku.command_palette.rebase_generation != generation
                    || waku.command_palette.view != CommandPaletteView::RebaseBase
                {
                    return;
                }
                let Some(picker) = waku.command_palette.rebase_base.as_mut() else {
                    return;
                };
                picker.pending = false;
                match result {
                    Ok(waku_client::WorkspaceResult::Branches {
                        snapshot: Some(snapshot),
                    }) => {
                        picker.current_branch = snapshot.current;
                        picker.branches = snapshot.branches;
                    }
                    Ok(_) => {}
                    Err(error) => waku.show_toast(error.to_string()),
                }
                let query = waku.command_palette.search.read(cx).content().to_owned();
                waku.refresh_command_palette_results(&query, false, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Esc on a drill-in's entry step lands on the regular palette,
    /// matching the resume picker's drill-out.
    fn leave_command_palette_drill_in_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::Commands;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    /// "Save as prompt template" parks the composer's text and switches the
    /// field to naming — the query becomes the command's file stem.
    fn open_command_palette_save_prompt_view(&mut self, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).content(cx).trim().to_owned();
        if body.is_empty() {
            self.show_toast(tr!("prompt_templates.nothing_to_save"));
            cx.notify();
            return;
        }
        self.command_palette.save_prompt_body = Some(body);
        self.command_palette.view = CommandPaletteView::SavePrompt;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.save_prompt_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    /// One row whose label tracks the typed name — the slug the file and the
    /// `/` command will share — so Enter always confirms the visible choice.
    fn refresh_command_palette_save_prompt_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let slug = slugify_prompt_name(query);
        let (label, detail) = if slug.is_empty() {
            (
                tr!("command_palette.save_prompt"),
                tr!("command_palette.save_prompt_name_hint"),
            )
        } else {
            let detail = self
                .home_directory
                .as_deref()
                .map(|home| prompt_templates_dir(home).join(format!("{slug}.md")))
                .map(|path| {
                    tr!(
                        "command_palette.save_prompt_path_hint",
                        path =
                            settings::abbreviate_home_path(&path, self.home_directory.as_deref())
                    )
                })
                .unwrap_or_else(|| tr!("command_palette.save_prompt_name_hint"));
            (
                tr!("command_palette.save_prompt_named", name = slug.clone()),
                detail,
            )
        };
        self.command_palette.results = vec![CommandPaletteItem {
            section: PaletteSection::Prompts,
            label,
            detail: Some(detail),
            icon: PaletteIcon::Asset("icons/plus.svg"),
            shortcut: None,
            action: PaletteAction::SavePromptAs(slug),
            content_match: None,
            search_text: String::new(),
            order: 0,
            recency: 0,
        }];
        self.finish_drill_in_refresh(selected_action.flatten(), None);
    }

    /// Write the parked composer text as `<slug>.md` under the user commands
    /// directory. Validation failures keep the naming step open so the typed
    /// name — and the parked draft — survive the toast.
    fn save_prompt_template(&mut self, slug: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(home) = self.home_directory.clone() else {
            self.close_command_palette(window, cx);
            self.show_toast(tr!("prompt_templates.no_home"));
            cx.notify();
            return;
        };
        if slug.is_empty() {
            self.show_toast(tr!("prompt_templates.name_required"));
            cx.notify();
            return;
        }
        let Some(body) = self.command_palette.save_prompt_body.clone() else {
            self.close_command_palette(window, cx);
            self.show_toast(tr!("prompt_templates.nothing_to_save"));
            cx.notify();
            return;
        };
        let dir = prompt_templates_dir(&home);
        let path = dir.join(format!("{slug}.md"));
        if path.exists() {
            self.show_toast(tr!("prompt_templates.exists", name = slug.to_owned()));
            cx.notify();
            return;
        }
        self.close_command_palette(window, cx);
        let slug = slug.to_owned();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(&path, body)
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                match result {
                    Ok(()) => {
                        waku.invalidate_composer_sources(cx);
                        waku.show_success_toast(tr!("prompt_templates.saved", name = slug.clone()));
                    }
                    Err(error) => {
                        waku.show_toast(tr!(
                            "prompt_templates.save_failed",
                            error = error.to_string()
                        ));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Entering "New task in…" starts one directory scan — the daemon walks
    /// the search roots, the picker fuzzy-filters the result per keystroke.
    /// The generation guard keeps a superseded scan from landing after Esc
    /// or a re-entry.
    fn open_command_palette_new_task_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::NewTaskIn;
        self.command_palette.new_task_directories.clear();
        self.command_palette.new_task_directories_pending = true;
        self.command_palette.new_task_directories_generation = self
            .command_palette
            .new_task_directories_generation
            .wrapping_add(1);
        let generation = self.command_palette.new_task_directories_generation;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.new_task_in_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();

        // Home covers the usual layouts (~/dev/repo, ~/repos/x); each known
        // project's parent adds whatever lives outside it.
        let mut roots = self.home_directory.iter().cloned().collect::<Vec<_>>();
        roots.extend(
            self.state
                .projects
                .iter()
                .filter(|project| !project.is_projectless())
                .filter_map(|project| project.path.parent().map(Path::to_path_buf)),
        );
        roots.sort();
        roots.dedup();
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace.request(waku_client::WorkspaceOperation::SearchDirectories {
                        roots,
                        max_depth: DIRECTORY_SEARCH_DEPTH,
                        cap: DIRECTORY_SEARCH_CAP,
                    })? {
                        waku_client::WorkspaceResult::Directories { paths } => Ok(paths),
                        _ => anyhow::bail!("the daemon returned an invalid directory response"),
                    }
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if !waku.command_palette.open
                    || waku.command_palette.new_task_directories_generation != generation
                {
                    return;
                }
                waku.command_palette.new_task_directories_pending = false;
                match result {
                    Ok(paths) => waku.command_palette.new_task_directories = paths,
                    Err(error) => {
                        waku.show_toast(tr!(
                            "command_palette.directory_search_failed",
                            error = error
                        ));
                    }
                }
                if waku.command_palette.view == CommandPaletteView::NewTaskIn {
                    let query = waku.command_palette.search.read(cx).content().to_owned();
                    waku.refresh_command_palette_results(&query, false, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "New GitHub Issue": infer the repository from the active surface and
    /// go straight to the template step; only when nothing maps to a repo
    /// does the project step show.
    fn start_issue_flow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.infer_issue_target(cx) {
            Some(target) => self.begin_issue_template_fetch(target, window, cx),
            None => self.open_command_palette_issue_projects_view(cx),
        }
    }

    /// Where a new issue lands, decided by what the user is looking at: the
    /// selected terminal first, then the selected session, then the
    /// selected project. A local candidate that is not inside a git
    /// repository does not count as an inference; a remote project's
    /// repo-ness is its daemon's to confirm during the fetch.
    fn infer_issue_target(&self, cx: &App) -> Option<issue_dialog::IssueTarget> {
        if let Some(terminal_id) = self.selected_terminal {
            // Session-owned terminals file under their task's workspace.
            let session_id = self
                .terminal_records
                .get(&terminal_id)
                .and_then(|record| record.session);
            if let Some(target) = session_id
                .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
                .and_then(|session| self.issue_target_for_session(session))
            {
                return Some(target);
            }
            if let Some(target) = self
                .terminal_cwd(terminal_id, cx)
                .and_then(|cwd| self.issue_target_for_path(&cwd, None))
            {
                return Some(target);
            }
        }
        if let Some(target) = self
            .selected_session()
            .and_then(|session| self.issue_target_for_session(session))
        {
            return Some(target);
        }
        self.selected_project()
            .filter(|project| !project.is_projectless())
            .and_then(|project| self.issue_target_for_path(&project.path, Some(project.id)))
    }

    /// A session's workspace — its worktree checkout when it has one —
    /// rooted at the enclosing repository. The project id comes along so
    /// the created issue can deep-link into the GitHub browser.
    fn issue_target_for_session(
        &self,
        session: &AgentSession,
    ) -> Option<issue_dialog::IssueTarget> {
        let project = self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id)?;
        if project.is_projectless() {
            return None;
        }
        let workspace = self.workspace_path_for_session(session)?;
        // A remote project's repository lives on its host's filesystem —
        // `nearest_repo_root` cannot see it, so the workspace path goes
        // straight to the daemon and the fetch validates the repo there.
        let cwd = if self.is_remote_project(project.id) {
            workspace.to_path_buf()
        } else {
            terminals::nearest_repo_root(workspace)?
        };
        Some(issue_dialog::IssueTarget {
            cwd,
            project: Some(project.id),
        })
    }

    /// `path`'s enclosing repository, plus the project that owns it when
    /// one's root contains — or is contained by — the repo root. A checkout
    /// outside every project still works; the "View" toast then falls back
    /// to the browser.
    fn issue_target_for_path(
        &self,
        path: &Path,
        project: Option<Uuid>,
    ) -> Option<issue_dialog::IssueTarget> {
        let remote = project.is_some_and(|id| self.is_remote_project(id));
        let root = if remote {
            path.to_path_buf()
        } else {
            terminals::nearest_repo_root(path)?
        };
        let project = project.or_else(|| {
            self.state
                .projects
                .iter()
                .filter(|project| !project.is_projectless())
                .filter(|project| {
                    root.starts_with(&project.path) || project.path.starts_with(&root)
                })
                .max_by_key(|project| project.path.components().count())
                .map(|project| project.id)
        });
        Some(issue_dialog::IssueTarget { cwd: root, project })
    }

    /// The template step doubles as the availability gate: the repo
    /// resolve and the template scan land together, and only a real
    /// GitHub repo reaches the picker or the dialog.
    fn begin_issue_template_fetch(
        &mut self,
        target: issue_dialog::IssueTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_palette.issue_target = Some(target.clone());
        self.command_palette.issue_repo = None;
        self.command_palette.issue_templates.clear();
        self.command_palette.issue_templates_pending = true;
        self.command_palette.issue_blank_enabled = true;
        self.command_palette.issue_generation =
            self.command_palette.issue_generation.wrapping_add(1);
        let generation = self.command_palette.issue_generation;
        self.command_palette.view = CommandPaletteView::IssueTemplates;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.issue_template_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();

        // The request routes to the daemon that owns the path — an offline
        // remote owner gets the disconnected treatment, never the local
        // daemon's filesystem.
        let Some(workspace) = self.workspace_client_for_path(&target.cwd) else {
            self.close_command_palette(window, cx);
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        let window_handle = window.window_handle();
        let cwd = target.cwd.clone();
        cx.spawn(async move |waku, cx| {
            let (templates, repo, availability) = cx
                .background_executor()
                .spawn(async move {
                    let templates = match workspace.request(
                        waku_client::WorkspaceOperation::ListIssueTemplates { cwd: cwd.clone() },
                    ) {
                        Ok(waku_client::WorkspaceResult::IssueTemplates {
                            entries,
                            blank_issues_enabled,
                        }) => Some((entries, blank_issues_enabled)),
                        _ => None,
                    };
                    let (repo, availability) = match workspace
                        .request(waku_client::WorkspaceOperation::ResolveGitHubRepo { cwd })
                    {
                        Ok(waku_client::WorkspaceResult::GitHubRepo { repo, availability }) => {
                            (repo, availability)
                        }
                        _ => (None, GitHubAvailability::Ready),
                    };
                    (templates, repo, availability)
                })
                .await;
            let outcome = waku.update(cx, |waku, cx| {
                if !waku.command_palette.open || waku.command_palette.issue_generation != generation
                {
                    return None;
                }
                waku.command_palette.issue_templates_pending = false;
                match availability {
                    GitHubAvailability::MissingCli => {
                        return Some(IssueTemplateFetch::Toast(tr!("github.install_gh")));
                    }
                    GitHubAvailability::Unauthenticated => {
                        return Some(IssueTemplateFetch::Toast(tr!("github.auth_gh")));
                    }
                    GitHubAvailability::Ready => {}
                }
                let Some(repo) = repo else {
                    return Some(IssueTemplateFetch::Toast(tr!("github.not_a_repo")));
                };
                waku.command_palette.issue_repo = Some((Some(repo), availability));
                // A template-read failure degrades to the plain form — the
                // create submit surfaces real errors itself.
                let (entries, blank_issues_enabled) = templates.unwrap_or_default();
                waku.command_palette.issue_blank_enabled = blank_issues_enabled;
                if entries.is_empty() {
                    return Some(IssueTemplateFetch::OpenDialog);
                }
                waku.command_palette.issue_templates = entries;
                if waku.command_palette.view == CommandPaletteView::IssueTemplates {
                    let query = waku.command_palette.search.read(cx).content().to_owned();
                    waku.refresh_command_palette_results(&query, false, cx);
                }
                Some(IssueTemplateFetch::ShowTemplates)
            });
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = waku.update(cx, |waku, cx| match outcome.ok().flatten() {
                    Some(IssueTemplateFetch::Toast(message)) => {
                        waku.close_command_palette(window, cx);
                        waku.show_toast(message);
                        cx.notify();
                    }
                    Some(IssueTemplateFetch::OpenDialog) => {
                        waku.close_command_palette(window, cx);
                        waku.open_issue_dialog_from_palette(None, window, cx);
                    }
                    Some(IssueTemplateFetch::ShowTemplates) | None => {}
                });
            });
        })
        .detach();
    }

    /// Esc on the directory step lands on the regular palette, matching the
    /// run-script drill-out.
    fn leave_command_palette_new_task_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::Commands;
        self.command_palette.new_task_directories_pending = false;
        self.command_palette.new_task_directories_generation = self
            .command_palette
            .new_task_directories_generation
            .wrapping_add(1);
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    fn open_command_palette_issue_projects_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::IssueProjects;
        self.command_palette.issue_target = None;
        self.command_palette.issue_templates.clear();
        self.command_palette.issue_templates_pending = false;
        self.command_palette.issue_generation =
            self.command_palette.issue_generation.wrapping_add(1);
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.issue_project_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    /// The picker picked — or skipped — a template; the dialog is a
    /// separate surface, so the palette hands it the resolved target and
    /// repo after closing.
    fn open_issue_dialog_from_palette(
        &mut self,
        template: Option<waku_protocol::workspace::IssueTemplate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.command_palette.issue_target.clone() else {
            return;
        };
        let repo = self
            .command_palette
            .issue_repo
            .as_ref()
            .and_then(|(repo, _)| repo.clone());
        self.open_issue_dialog(target, repo, template, window, cx);
    }

    /// A YAML form template's only path — `gh` cannot render GitHub's form
    /// schema — is `issues/new?template=` on the repo's host.
    fn issue_template_web_url(
        &self,
        template: &waku_protocol::workspace::IssueTemplate,
    ) -> Option<String> {
        let repo = self.command_palette.issue_repo.as_ref()?.0.as_ref()?;
        Some(format!(
            "{}/issues/new?template={}",
            repo.web_url.trim_end_matches('/'),
            template.filename
        ))
    }

    fn dismiss_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.command_palette.view {
            CommandPaletteView::Commands => self.close_command_palette(window, cx),
            CommandPaletteView::Resume => self.leave_command_palette_resume_view(cx),
            CommandPaletteView::ResumeProviders => {
                self.leave_command_palette_resume_provider_view(cx)
            }
            CommandPaletteView::RunScriptProjects => self.leave_command_palette_drill_in_view(cx),
            // Esc on the scripts step backs up to the project step.
            CommandPaletteView::RunScripts => {
                self.open_command_palette_run_script_projects_view(cx)
            }
            CommandPaletteView::RemoveProject => self.leave_command_palette_drill_in_view(cx),
            CommandPaletteView::NewTaskIn => self.leave_command_palette_new_task_view(cx),
            CommandPaletteView::IssueProjects => self.leave_command_palette_drill_in_view(cx),
            // Esc on the templates step backs up to the project step —
            // also the way to override an inferred repository.
            CommandPaletteView::IssueTemplates => self.open_command_palette_issue_projects_view(cx),
            CommandPaletteView::RebaseBase => self.leave_command_palette_drill_in_view(cx),
            CommandPaletteView::SavePrompt => self.leave_command_palette_drill_in_view(cx),
        }
    }

    pub(super) fn refresh_command_palette_localized_text(&mut self, cx: &mut Context<Self>) {
        let placeholder = match self.command_palette.view {
            CommandPaletteView::Commands => tr!("command_palette.placeholder"),
            CommandPaletteView::Resume => tr!("command_palette.resume_placeholder"),
            CommandPaletteView::ResumeProviders => {
                tr!("command_palette.resume_provider_placeholder")
            }
            CommandPaletteView::RunScriptProjects => {
                tr!("command_palette.run_script_project_placeholder")
            }
            CommandPaletteView::RunScripts => tr!("command_palette.run_script_placeholder"),
            CommandPaletteView::RemoveProject => tr!("command_palette.remove_project_placeholder"),
            CommandPaletteView::NewTaskIn => tr!("command_palette.new_task_in_placeholder"),
            CommandPaletteView::IssueProjects => {
                tr!("command_palette.issue_project_placeholder")
            }
            CommandPaletteView::IssueTemplates => {
                tr!("command_palette.issue_template_placeholder")
            }
            CommandPaletteView::RebaseBase => {
                match self
                    .command_palette
                    .rebase_base
                    .as_ref()
                    .and_then(|picker| picker.current_base.as_deref())
                {
                    Some(base) => {
                        tr!(
                            "command_palette.rebase_base_placeholder",
                            base = base.to_owned()
                        )
                    }
                    None => tr!("command_palette.rebase_base_placeholder_unknown"),
                }
            }
            CommandPaletteView::SavePrompt => tr!("command_palette.save_prompt_placeholder"),
        };
        self.command_palette.search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.command_palette"), cx);
            input.set_placeholder(placeholder, cx);
        });
        if self.command_palette.open {
            let query = self.command_palette.search.read(cx).content().to_owned();
            self.refresh_command_palette_results(&query, true, cx);
        }
    }

    pub(super) fn command_palette_query_edited(&mut self, query: &str, cx: &mut Context<Self>) {
        if !self.command_palette.open {
            return;
        }
        if matches!(
            self.command_palette.view,
            CommandPaletteView::Resume
                | CommandPaletteView::ResumeProviders
                | CommandPaletteView::RunScriptProjects
                | CommandPaletteView::RunScripts
                | CommandPaletteView::RemoveProject
                | CommandPaletteView::NewTaskIn
                | CommandPaletteView::IssueProjects
                | CommandPaletteView::IssueTemplates
                | CommandPaletteView::RebaseBase
                | CommandPaletteView::SavePrompt
        ) {
            self.refresh_command_palette_results(query, false, cx);
            cx.notify();
            return;
        }
        let query = query.trim().to_owned();
        self.command_palette.active_message_query = (!query.is_empty()).then(|| query.clone());
        let fetch = if query.is_empty() {
            self.command_palette.message_matches_query = None;
            self.command_palette.message_matches.clear();
            self.command_palette.message_search_pending = false;
            None
        } else {
            match self.command_palette.message_searches.read(&query) {
                Query::Ready(matches) => {
                    self.command_palette.message_matches_query = Some(query.clone());
                    self.command_palette.message_matches = matches
                        .iter()
                        .cloned()
                        .map(|matched| (matched.session_id, matched))
                        .collect();
                    self.command_palette.message_search_pending = false;
                    None
                }
                Query::Pending => {
                    self.command_palette.message_search_pending = true;
                    None
                }
                Query::Missing(token) => {
                    self.command_palette.message_search_pending = true;
                    Some(token)
                }
            }
        };
        self.refresh_command_palette_results(&query, false, cx);
        cx.notify();

        let Some(token) = fetch else {
            return;
        };
        let search = self.store.session_message_search(
            query.clone(),
            MESSAGE_SEARCH_LIMIT,
            crate::persistence::SessionMessageSearchScope::Active,
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(MESSAGE_SEARCH_DEBOUNCE)
                .await;
            let current = this
                .update(cx, |this, _| {
                    this.command_palette.open
                        && this.command_palette.active_message_query.as_deref()
                            == Some(query.as_str())
                })
                .unwrap_or(false);
            if !current {
                let _ = this.update(cx, |this, _| {
                    this.command_palette.message_searches.abandon(token)
                });
                return;
            }

            let matches = cx
                .background_executor()
                .spawn(async move { search().unwrap_or_default() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this
                    .command_palette
                    .message_searches
                    .fulfill(token, matches)
                {
                    return;
                }
                if !this.command_palette.open
                    || this.command_palette.active_message_query.as_deref() != Some(query.as_str())
                {
                    return;
                }
                let Query::Ready(matches) = this.command_palette.message_searches.read(&query)
                else {
                    return;
                };
                this.command_palette.message_matches_query = Some(query.clone());
                this.command_palette.message_matches = matches
                    .iter()
                    .cloned()
                    .map(|matched| (matched.session_id, matched))
                    .collect();
                this.command_palette.message_search_pending = false;
                this.refresh_command_palette_results(&query, false, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn command_palette_commands(
        &self,
        searching: bool,
        updater_available: bool,
    ) -> Vec<CommandPaletteItem> {
        let display_section = |suggested| {
            if searching {
                PaletteSection::Commands
            } else {
                suggested
            }
        };
        let mut order = 0usize;
        let mut next = || {
            let current = order;
            order += 1;
            current
        };
        let mut commands = vec![
            CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.new_task"),
                "icons/pencil.svg",
                Some(ShortcutHint::action(&NewSession).shadowed_by(&SwitchProjectForward)),
                PaletteAction::NewTask,
                "new task session chat conversation start",
                next(),
            ),
            CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.new_task_in"),
                "icons/folder-search.svg",
                Some(ShortcutHint::action(&NewTaskIn)),
                PaletteAction::NewTaskIn,
                "new task in directory folder temporary project search",
                next(),
            ),
            CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.resume"),
                "icons/rotate-cw.svg",
                None,
                PaletteAction::Resume,
                "resume continue restore import external terminal cli session conversation",
                next(),
            ),
            CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.open_project"),
                "icons/folder.svg",
                Some(ShortcutHint::action(&NewProject)),
                PaletteAction::OpenProject,
                "open add folder project workspace repository repo",
                next(),
            ),
            CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.run_script"),
                "icons/terminal.svg",
                Some(ShortcutHint::action(&RunProjectScript)),
                PaletteAction::OpenRunScript,
                "run project script npm make just package makefile build dev test target recipe",
                next(),
            ),
            CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.new_github_issue"),
                "icons/github.svg",
                None,
                PaletteAction::CreateGitHubIssue,
                "new create file github issue bug report ticket template",
                next(),
            ),
        ];

        if self
            .state
            .projects
            .iter()
            .any(|project| !project.is_projectless())
        {
            commands.push(CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.remove_project"),
                "icons/trash.svg",
                None,
                PaletteAction::OpenRemoveProject,
                "remove delete project workspace repository repo folder",
                next(),
            ));
        }

        let can_choose_model = self
            .selected_session()
            .is_some_and(|session| session.can_choose_model(session.provider));
        if can_choose_model {
            commands.push(CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.choose_model"),
                "icons/bot.svg",
                Some(ShortcutHint::action(&ToggleModelPicker)),
                PaletteAction::ChooseModel,
                "choose change select model provider agent",
                next(),
            ));
        }

        // Same gate as the "Work in" chip: an unstarted draft in a real
        // project is the only session whose workspace can still flip.
        let can_toggle_workspace = self
            .selected_session()
            .is_some_and(|session| !session.has_started() && !session.is_busy())
            && self
                .selected_project()
                .is_some_and(|project| !project.is_projectless());
        if can_toggle_workspace {
            commands.push(CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("menu.toggle_workspace"),
                "icons/fork.svg",
                Some(ShortcutHint::action(&ToggleWorkspace)),
                PaletteAction::ToggleWorkspace,
                "toggle switch workspace worktree local checkout draft",
                next(),
            ));
        }
        if self
            .selected_session()
            .is_some_and(|session| self.can_move_session_to_worktree(session.id))
        {
            commands.push(CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("session.move_to_worktree"),
                "icons/fork.svg",
                None,
                PaletteAction::MoveToWorktree,
                "move transfer worktree workspace checkout task",
                next(),
            ));
        }
        if let Some(worktree_name) =
            self.selected_session()
                .and_then(|session| match &session.workspace {
                    SessionWorkspace::Worktree { name, .. } if session.has_started() => {
                        Some(name.clone())
                    }
                    _ => None,
                })
        {
            let mut item = CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.new_task_in_same_worktree"),
                "icons/fork.svg",
                None,
                PaletteAction::NewTaskInSameWorktree,
                "new task session chat worktree same current shared checkout branch",
                next(),
            );
            item.detail = Some(format!("#{worktree_name}"));
            commands.push(item);
        }

        // Same gate as the composer's `/land`: the session the composer
        // answers to runs in a worktree — local checkouts have no base to
        // land on — and no panel operation is already running.
        if self.git_panel_operation.is_none()
            && self.composer_session().is_some_and(|session| {
                matches!(&session.workspace, SessionWorkspace::Worktree { .. })
                    && self.workspace_path_for_session(session).is_some()
            })
        {
            let mut item = CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.land_changes"),
                "icons/git-merge.svg",
                None,
                PaletteAction::LandChanges,
                "land changes commits onto base branch rebase merge integrate fast-forward worktree git",
                next(),
            );
            item.detail = self
                .composer_session()
                .and_then(|session| match &session.workspace {
                    SessionWorkspace::Worktree {
                        base_branch: Some(base),
                        ..
                    } => Some(format!("→ {base}")),
                    _ => None,
                });
            commands.push(item);
        }

        // Rebase is a started worktree's base change: drafts still pick
        // theirs in the branch picker, and a busy session or a sync in
        // flight must not have its history rewritten under it.
        if self.git_panel_operation.is_none()
            && self.git_panel_sync_conflict.is_none()
            && !self.branch_operation_pending
            && self.composer_session().is_some_and(|session| {
                session.has_started()
                    && !session.is_busy()
                    && matches!(&session.workspace, SessionWorkspace::Worktree { .. })
                    && self.workspace_path_for_session(session).is_some()
            })
        {
            let mut item = CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.change_base_branch"),
                "icons/git-branch.svg",
                None,
                PaletteAction::ChangeBaseBranch,
                "change base branch rebase onto worktree git history",
                next(),
            );
            item.detail = self
                .composer_session()
                .and_then(|session| match &session.workspace {
                    SessionWorkspace::Worktree {
                        base_branch: Some(base),
                        ..
                    } => Some(tr!("command_palette.based_on", base = base.clone())),
                    _ => None,
                });
            commands.push(item);
        }

        // Same reachability as ⌘S outside the file editor: a session's
        // workspace or a selected project supplies the repository. The chord
        // itself belongs to SaveFile — its handler falls through to the
        // branch picker — so the hint advertises that action's binding.
        if self.selected_workspace_path().is_some() || self.selected_project().is_some() {
            commands.push(CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.sync_branch"),
                "icons/git-branch.svg",
                Some(ShortcutHint::action(&SaveFile)),
                PaletteAction::SyncBranch,
                "sync branch pull rebase merge upstream tracking checkout update git",
                next(),
            ));
        }

        // Same gate as the composer's `/compact`: the session has a compact
        // path — the reserved Waku entry or a provider-reported builtin.
        if self.composer_session().is_some_and(|session| {
            session.provider.supports_compact()
                || crate::composer_complete::has_compact_path(&self.slash_command_index)
        }) {
            commands.push(CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("commands.compact_context"),
                "icons/minimize.svg",
                None,
                PaletteAction::CompactContext,
                "compact compress context window tokens reduce shrink summarize",
                next(),
            ));
        }

        if self
            .selected_branch_snapshot()
            .and_then(branches::github_branch_url)
            .is_some()
        {
            commands.push(CommandPaletteItem::command(
                display_section(PaletteSection::Suggested),
                tr!("command_palette.open_on_github"),
                "icons/github.svg",
                None,
                PaletteAction::OpenOnGitHub,
                "open github remote repository repo branch browser",
                next(),
            ));
        }

        commands.push(CommandPaletteItem::command(
            PaletteSection::Commands,
            tr!("menu.focus_composer"),
            "icons/pencil.svg",
            Some(ShortcutHint::action(&FocusComposer)),
            PaletteAction::FocusComposer,
            "focus composer prompt input message",
            next(),
        ));
        commands.push(CommandPaletteItem::command(
            PaletteSection::Commands,
            tr!("command_palette.create_draft"),
            "icons/compose.svg",
            None,
            PaletteAction::CreateDraft,
            "create save draft park stash composer message text",
            next(),
        ));
        commands.push(CommandPaletteItem::command(
            PaletteSection::Commands,
            tr!("command_palette.view_drafts"),
            "icons/compose.svg",
            None,
            PaletteAction::ViewDrafts,
            "view open drafts list page saved",
            next(),
        ));
        for identifier in PaletteIdentifier::ALL {
            if identifier.value(self.selected_session()).is_some() {
                commands.push(CommandPaletteItem::command(
                    PaletteSection::Commands,
                    identifier.label(),
                    "icons/copy.svg",
                    None,
                    PaletteAction::CopyIdentifier(identifier),
                    identifier.keywords(),
                    next(),
                ));
            }
        }
        if self.usage_meter_available() {
            commands.push(CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!("menu.toggle_usage_panel"),
                "icons/command.svg",
                Some(ShortcutHint::action(&ToggleUsagePanel)),
                PaletteAction::ToggleUsage,
                "toggle usage limits rate quota panel",
                next(),
            ));
        }
        if updater_available {
            commands.push(CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!("menu.check_for_updates"),
                "icons/download.svg",
                None,
                PaletteAction::CheckForUpdates,
                "check for updates update version upgrade latest",
                next(),
            ));
        }
        commands.push(CommandPaletteItem::command(
            PaletteSection::Commands,
            tr!("command_palette.collapse_sidebar_groups"),
            "icons/command.svg",
            None,
            PaletteAction::CollapseSidebarGroups,
            "collapse close fold all sidebar groups projects dates history",
            next(),
        ));
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let selected = self.state.selected_session;
        let pending = self
            .pending_session_activation
            .map(|pending| pending.session_id);
        if sessions::next_unread_completion(
            &self.state.sessions,
            &self.state.unseen_completions,
            &rows,
            selected,
            pending,
            None,
        )
        .or_else(|| {
            sessions::next_idle_session(&self.state.sessions, &rows, selected, pending, None)
        })
        .is_some()
        {
            commands.push(CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!("command_palette.go_to_next_unread_completion"),
                "icons/corner-down-right.svg",
                Some(ShortcutHint::action(&GoToNextUnreadCompletion)),
                PaletteAction::GoToNextUnreadCompletion,
                "go to next unseen unread blocked waiting completed finished failed turn task session jump navigate",
                next(),
            ));
        }
        if !self.state.unseen_completions.is_empty() {
            commands.push(CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!("command_palette.mark_all_tasks_read"),
                "icons/check.svg",
                None,
                PaletteAction::MarkAllSessionsRead,
                "mark all tasks sessions read seen clear unseen unread completions dots inbox zero dismiss",
                next(),
            ));
        }
        commands.extend([
            CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!(if self.sidebar_visible {
                    "command_palette.hide_sidebar"
                } else {
                    "command_palette.show_sidebar"
                }),
                "icons/panel-left.svg",
                Some(ShortcutHint::action(&ToggleSidebar)),
                PaletteAction::ToggleSidebar,
                "toggle show hide left sidebar history tasks",
                next(),
            ),
            CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!(if self.right_panel_visible {
                    "command_palette.hide_right_panel"
                } else {
                    "command_palette.show_right_panel"
                }),
                "icons/panel-right.svg",
                Some(ShortcutHint::action(&ToggleRightPanel)),
                PaletteAction::ToggleRightPanel,
                "toggle show hide right panel files diff terminal browser",
                next(),
            ),
        ]);
        // gpui compiles its inspector out of release builds.
        if cfg!(debug_assertions) {
            commands.push(CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!("command_palette.inspect_elements"),
                "icons/cursor-spark.svg",
                None,
                PaletteAction::InspectElements,
                "goddard inspect elements ui label source location identify pick hover",
                next(),
            ));
            commands.push(CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!("command_palette.inspect_colors"),
                "icons/eye.svg",
                None,
                PaletteAction::InspectColors,
                "goddard inspect colors theme token hsla pick hover",
                next(),
            ));
        }
        // GODDARD_DEV_STATE only exists when the dev watcher launched this
        // app; a release build or a bare debug binary must not offer the
        // toggle.
        if self.dev_state_path.is_some() {
            commands.push(CommandPaletteItem::command(
                PaletteSection::Commands,
                tr!(if self.auto_restart_enabled {
                    "command_palette.disable_auto_restart"
                } else {
                    "command_palette.enable_auto_restart"
                }),
                "icons/rotate-cw.svg",
                None,
                PaletteAction::ToggleAutoRestart,
                "auto restart relaunch rebuild dev watcher toggle enable disable",
                next(),
            ));
        }

        // Same spot the Commands settings page puts its "New command" row:
        // first under the section, above the commands themselves.
        commands.push(CommandPaletteItem::command(
            PaletteSection::CustomCommands,
            tr!("command_palette.new_custom_command"),
            "icons/plus.svg",
            None,
            PaletteAction::NewCustomCommand,
            "new create add custom command terminal shell run script settings",
            next(),
        ));

        for command in &self.state.custom_commands {
            let label = command.display_name().to_owned();
            let mut detail = command
                .script
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned();
            if command.script.lines().nth(1).is_some() {
                detail.push('…');
            }
            commands.push(CommandPaletteItem {
                section: PaletteSection::CustomCommands,
                search_text: format!(
                    "{label} {} {} custom command terminal shell run script",
                    command.script,
                    command.shell.as_deref().unwrap_or_default(),
                ),
                label,
                detail: Some(detail),
                icon: PaletteIcon::Asset(crate::custom_commands::icon_path(command.icon)),
                shortcut: None,
                action: PaletteAction::RunCustomCommand(command.id),
                content_match: None,
                order: next(),
                recency: 0,
            });
        }

        // The prompt library: the same file-backed templates the composer
        // expands on `/name` submission, offered here so they're browsable
        // and insertable without memorizing names.
        commands.push(CommandPaletteItem::command(
            PaletteSection::Prompts,
            tr!("command_palette.save_prompt"),
            "icons/plus.svg",
            None,
            PaletteAction::OpenSavePrompt,
            "save create add prompt template composer draft reusable",
            next(),
        ));
        commands.push(CommandPaletteItem::command(
            PaletteSection::Prompts,
            tr!("command_palette.open_prompts_folder"),
            "icons/folder-open.svg",
            None,
            PaletteAction::RevealPromptTemplates,
            "open reveal prompts templates folder commands files edit",
            next(),
        ));
        for command in self
            .slash_command_index
            .iter()
            .filter(|command| command.template.is_some())
        {
            let mut detail = command.scope.label();
            if !command.description.is_empty() {
                detail = format!("{detail} · {}", command.description);
            }
            if let Some(hint) = &command.argument_hint {
                detail = format!("{detail} · {hint}");
            }
            commands.push(CommandPaletteItem {
                section: PaletteSection::Prompts,
                label: format!("/{}", command.name),
                detail: Some(detail),
                icon: PaletteIcon::Asset("icons/slash.svg"),
                shortcut: None,
                action: PaletteAction::InsertPromptTemplate(command.clone()),
                content_match: None,
                search_text: format!(
                    "/{} {} {} prompt template slash saved reusable",
                    command.name,
                    command.description,
                    command.scope.label()
                ),
                order: next(),
                recency: 0,
            });
        }

        for (page, label_key, icon, keywords) in [
            (
                SettingsPage::General,
                "settings.general",
                "icons/settings.svg",
                "settings preferences general local privacy updates",
            ),
            (
                SettingsPage::Appearance,
                "settings.appearance",
                "icons/appearance.svg",
                "settings preferences appearance theme language light dark",
            ),
            (
                SettingsPage::Providers,
                "settings.providers",
                "icons/bot.svg",
                "settings preferences providers agents models cli",
            ),
            (
                SettingsPage::Skills,
                "settings.skills",
                "icons/package.svg",
                "settings preferences skills library create disable agent skill",
            ),
            (
                SettingsPage::Git,
                "settings.git",
                "icons/git-branch.svg",
                "settings preferences git worktrees branches repository checkout",
            ),
            (
                SettingsPage::Keybindings,
                "keybind.title",
                "icons/keyboard.svg",
                "keybindings keyboard shortcuts hotkeys keys remap manager",
            ),
            (
                SettingsPage::Commands,
                "settings.commands",
                "icons/terminal.svg",
                "settings preferences custom commands terminal shell run script palette",
            ),
            (
                SettingsPage::Terminal,
                "settings.terminal",
                "icons/terminal-square.svg",
                "settings preferences terminal shell font size link modifier click copy select",
            ),
            (
                SettingsPage::Usage,
                "settings.usage",
                "icons/chart-column.svg",
                "settings preferences usage tokens cost history",
            ),
            (
                SettingsPage::Daemon,
                "settings.daemon",
                "icons/server.svg",
                "settings preferences daemon server remote web network origin token port",
            ),
            (
                SettingsPage::ComputerUse,
                "settings.computer_use",
                "icons/cursor-spark.svg",
                "settings preferences computer use accessibility screen recording",
            ),
            (
                SettingsPage::Jev,
                "settings.jev",
                "icons/provider-typesafe.svg",
                "settings preferences jev typesafe eval evaluation model auto routing router backend",
            ),
            (
                SettingsPage::Integrations,
                "settings.integrations",
                "icons/globe.svg",
                "settings integrations mcp servers tools linear github notion connect oauth",
            ),
            (
                SettingsPage::Experiments,
                "settings.experiments",
                "icons/beaker.svg",
                "settings preferences experiments experimental beta opt in unfinished preview",
            ),
        ] {
            if !page.is_visible_in_navigation(
                self.state.computer_use_experiment_enabled,
                self.state.friends_enabled,
                self.state.model_router_enabled,
                self.state.integrations_enabled,
            ) {
                continue;
            }
            commands.push(CommandPaletteItem::command(
                PaletteSection::Settings,
                crate::i18n::translate(label_key),
                icon,
                (page == SettingsPage::General).then_some(ShortcutHint::action(&OpenSettings)),
                PaletteAction::OpenSettings(page),
                keywords,
                next(),
            ));
        }
        if !self.resume_available() {
            commands.retain(|item| item.action != PaletteAction::Resume);
        }
        commands
    }

    /// Whether any provider could offer a resumable session — enabled,
    /// catalog-capable, and detected with a binary by the daemon's provider
    /// probe.
    fn resume_available(&self) -> bool {
        ProviderKind::ALL.iter().any(|provider| {
            provider.supports_session_catalog()
                && !self.state.disabled_providers.contains(provider)
                && self
                    .provider_probe(*provider)
                    .is_some_and(|probe| probe.installed)
        })
    }

    fn command_palette_task_candidates(&self) -> Vec<CommandPaletteItem> {
        let projects = self
            .state
            .projects
            .iter()
            .map(|project| {
                (
                    project.id,
                    (
                        project.display_name(),
                        project.path.to_string_lossy().into_owned(),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        self.state
            .sessions
            .iter()
            .filter(|session| {
                session.has_started() && session.archived_at.is_none() && !session.is_side_chat()
            })
            .enumerate()
            .map(|(order, session)| {
                let (project, project_path) = projects
                    .get(&session.project_id)
                    .cloned()
                    .unwrap_or_else(|| (tr!("project.no_project_name"), String::new()));
                let (workspace_path, workspace_label, branch) = match &session.workspace {
                    SessionWorkspace::Local => (String::new(), None, None),
                    SessionWorkspace::NewWorktree { base_branch } => {
                        (String::new(), base_branch.as_deref(), None)
                    }
                    SessionWorkspace::Worktree {
                        path, name, branch, ..
                    } => (
                        path.to_string_lossy().into_owned(),
                        Some(name.as_str()),
                        branch.as_deref(),
                    ),
                };
                let mut details = vec![project.clone()];
                if let Some(label) = workspace_label {
                    details.push(format!("#{label}"));
                }
                if Some(session.id) == self.state.selected_session {
                    details.push(tr!("command_palette.current"));
                }
                let detail = details.join(" · ");
                let label = session.display_title().to_owned();
                let content_match = self
                    .command_palette
                    .message_matches
                    .get(&session.id)
                    .cloned();
                CommandPaletteItem {
                    section: PaletteSection::Tasks,
                    search_text: format!(
                        "{label} {project} {project_path} {workspace_path} {} {} {} {} {} task session chat conversation",
                        workspace_label.unwrap_or_default(),
                        branch.unwrap_or_default(),
                        session.provider.short_name(),
                        session.provider.display_name(),
                        session.model.as_deref().unwrap_or_default(),
                    ),
                    label,
                    detail: Some(detail),
                    icon: PaletteIcon::Provider(session.provider),
                    shortcut: None,
                    action: PaletteAction::SelectTask(session.id),
                    content_match,
                    order,
                    recency: session.updated_at,
                }
            })
            .collect()
    }

    fn command_palette_resume_candidates(&self) -> Vec<CommandPaletteItem> {
        let now = unix_time();
        self.command_palette
            .provider_sessions
            .iter()
            .filter(|(_, native)| {
                !self.state.sessions.iter().any(|session| {
                    session.provider_cursor.as_ref().is_some_and(|cursor| {
                        cursor.provider() == native.provider()
                            && cursor.native_id() == native.cursor.native_id()
                    })
                })
            })
            .enumerate()
            .map(|(order, (key, native))| {
                let provider = native.provider();
                let age = super::sidebar::format_time_ago(now.saturating_sub(native.updated_at));
                let path = native.cwd.to_string_lossy();
                let host = match key {
                    waku_client::DaemonKey::Local => None,
                    waku_client::DaemonKey::Remote(host) => self
                        .remote_host_name(*host)
                        .map(|name| format!("{name} · ")),
                }
                .unwrap_or_default();
                let missing = if native.cwd_missing {
                    format!(" · {}", tr!("command_palette.resume_missing_folder"))
                } else {
                    String::new()
                };
                CommandPaletteItem {
                    section: PaletteSection::Sessions,
                    label: native.title.clone(),
                    detail: Some(format!(
                        "{}{} · {} · {age}{missing}",
                        host,
                        provider.short_name(),
                        path
                    )),
                    icon: PaletteIcon::Provider(provider),
                    shortcut: None,
                    action: PaletteAction::ResumeProviderSession(*key, native.clone()),
                    content_match: None,
                    search_text: format!(
                        "{} {} {} {} {} resume continue terminal cli session conversation",
                        native.title,
                        path,
                        provider.short_name(),
                        provider.display_name(),
                        native.cursor.native_id(),
                    ),
                    order,
                    recency: native.updated_at,
                }
            })
            .collect()
    }

    fn command_palette_resume_provider_selector(&self) -> CommandPaletteItem {
        let provider = self.command_palette.resume_provider;
        CommandPaletteItem {
            section: PaletteSection::Providers,
            label: provider.display_name().to_owned(),
            detail: Some(tr!("command_palette.change_provider")),
            icon: PaletteIcon::Provider(provider),
            shortcut: None,
            action: PaletteAction::ChooseResumeProvider,
            content_match: None,
            search_text: format!(
                "{} {} change select provider agent cli",
                provider.short_name(),
                provider.display_name()
            ),
            order: 0,
            recency: u64::MAX,
        }
    }

    fn command_palette_resume_provider_candidates(&self) -> Vec<CommandPaletteItem> {
        ProviderKind::ALL
            .into_iter()
            .filter(|provider| {
                provider.supports_session_catalog()
                    && !self.state.disabled_providers.contains(provider)
            })
            .enumerate()
            .map(|(order, provider)| CommandPaletteItem {
                section: PaletteSection::Providers,
                label: provider.display_name().to_owned(),
                detail: (provider == self.command_palette.resume_provider)
                    .then(|| tr!("command_palette.current_provider")),
                icon: PaletteIcon::Provider(provider),
                shortcut: None,
                action: PaletteAction::SelectResumeProvider(provider),
                content_match: None,
                search_text: format!(
                    "{} {} provider agent cli terminal session",
                    provider.short_name(),
                    provider.display_name()
                ),
                order,
                recency: 0,
            })
            .collect()
    }

    fn command_palette_run_script_project_candidates(&self) -> Vec<CommandPaletteItem> {
        // Scripts launch in a desktop terminal — only local-daemon projects
        // have a path it can open.
        let projects = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless() && !self.is_remote_project(project.id))
            .collect::<Vec<_>>();
        let current = self
            .selected_session()
            .map(|session| session.project_id)
            .filter(|id| projects.iter().any(|project| project.id == *id));
        let recent = self.task_switcher.recent_project_ids(&self.state.sessions);
        run_script::run_script_project_order(current, &recent, &projects)
            .into_iter()
            .enumerate()
            .filter_map(|(order, project_id)| {
                let project = projects.iter().find(|project| project.id == project_id)?;
                let mut detail =
                    settings::abbreviate_home_path(&project.path, self.home_directory.as_deref());
                if current == Some(project_id) {
                    detail = format!("{detail} · {}", tr!("command_palette.current"));
                }
                let label = project.display_name();
                Some(CommandPaletteItem {
                    section: PaletteSection::Projects,
                    search_text: format!("{label} {} project", project.path.to_string_lossy()),
                    label,
                    detail: Some(detail),
                    icon: PaletteIcon::Asset("icons/folder.svg"),
                    shortcut: None,
                    action: PaletteAction::ChooseRunScriptProject(project_id),
                    content_match: None,
                    order,
                    recency: 0,
                })
            })
            .collect()
    }

    fn command_palette_remove_project_candidates(&self) -> Vec<CommandPaletteItem> {
        let projects = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless())
            .collect::<Vec<_>>();
        let current = self
            .selected_session()
            .map(|session| session.project_id)
            .filter(|id| projects.iter().any(|project| project.id == *id));
        let recent = self.task_switcher.recent_project_ids(&self.state.sessions);
        run_script::run_script_project_order(current, &recent, &projects)
            .into_iter()
            .enumerate()
            .filter_map(|(order, project_id)| {
                let project = projects.iter().find(|project| project.id == project_id)?;
                let mut detail =
                    settings::abbreviate_home_path(&project.path, self.home_directory.as_deref());
                if let waku_client::DaemonKey::Remote(host) = self.project_host(project_id)
                    && let Some(host) = self.remote_host_name(host)
                {
                    detail = format!("{detail} · {host}");
                }
                if current == Some(project_id) {
                    detail = format!("{detail} · {}", tr!("command_palette.current"));
                }
                let label = project.display_name();
                Some(CommandPaletteItem {
                    section: PaletteSection::Projects,
                    search_text: format!(
                        "{label} {} project remove delete",
                        project.path.to_string_lossy()
                    ),
                    label,
                    detail: Some(detail),
                    icon: PaletteIcon::Asset("icons/trash.svg"),
                    shortcut: None,
                    action: PaletteAction::RemoveProject(project_id),
                    content_match: None,
                    order,
                    recency: 0,
                })
            })
            .collect()
    }

    fn command_palette_run_script_candidates(&self) -> Vec<CommandPaletteItem> {
        let Some(project_id) = self.command_palette.run_script_project else {
            return Vec::new();
        };
        self.command_palette
            .run_scripts
            .iter()
            .enumerate()
            .map(|(order, script)| CommandPaletteItem {
                section: PaletteSection::Scripts,
                label: script.name.clone(),
                detail: Some(format!(
                    "{} · {}",
                    script.source.label(),
                    script.detail_or_command()
                )),
                icon: PaletteIcon::Asset(script.source.icon()),
                shortcut: None,
                action: PaletteAction::RunScript {
                    project: project_id,
                    script: script.clone(),
                },
                content_match: None,
                search_text: format!(
                    "{} {} {} {} script run",
                    script.name,
                    script.command,
                    script.detail,
                    script.source.label()
                ),
                order,
                recency: 0,
            })
            .collect()
    }

    /// The fuzzy pass the run-script steps share: score `search_text`, then
    /// order by score with listing order as the tiebreak.
    fn score_run_script_items(
        &mut self,
        candidates: Vec<CommandPaletteItem>,
        query: &str,
    ) -> Vec<CommandPaletteItem> {
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut utf32 = Vec::new();
        let mut scored = candidates
            .into_iter()
            .filter_map(|item| {
                pattern
                    .score(
                        Utf32Str::new(&item.search_text, &mut utf32),
                        &mut self.command_palette.matcher,
                    )
                    .map(|score| ScoredPaletteItem { score, item })
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.score.cmp(&a.score).then(a.item.order.cmp(&b.item.order)));
        scored.into_iter().map(|scored| scored.item).collect()
    }

    /// Keep the selection the same action occupied. On a project step the
    /// fallback is the selected task's project so Enter needs no extra
    /// step — `project_fallback` is that step's pick action; a `None`
    /// fallback selects the first row.
    fn finish_drill_in_refresh(
        &mut self,
        selected_action: Option<PaletteAction>,
        project_fallback: Option<fn(Uuid) -> PaletteAction>,
    ) {
        self.command_palette.selected = selected_action
            .and_then(|action| {
                self.command_palette
                    .results
                    .iter()
                    .position(|item| item.action == action)
            })
            .or_else(|| {
                let project_action = project_fallback?;
                let current = self.selected_session().map(|session| session.project_id)?;
                self.command_palette
                    .results
                    .iter()
                    .position(|item| item.action == project_action(current))
            })
            .unwrap_or(0);
        self.command_palette
            .scroll
            .scroll_to_item(self.command_palette_scroll_index(self.command_palette.selected));
    }

    fn refresh_command_palette_run_script_project_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_run_script_project_candidates();
        if !query.is_empty() {
            candidates = self.score_run_script_items(candidates, query);
        }
        self.command_palette.results = candidates;
        self.finish_drill_in_refresh(
            selected_action.flatten(),
            Some(PaletteAction::ChooseRunScriptProject),
        );
    }

    fn refresh_command_palette_remove_project_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_remove_project_candidates();
        if !query.is_empty() {
            candidates = self.score_run_script_items(candidates, query);
        }
        self.command_palette.results = candidates;
        self.finish_drill_in_refresh(
            selected_action.flatten(),
            Some(PaletteAction::RemoveProject),
        );
    }

    fn refresh_command_palette_run_script_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_run_script_candidates();
        if !query.is_empty() {
            candidates = self.score_run_script_items(candidates, query);
        }
        self.command_palette.results = candidates;
        self.finish_drill_in_refresh(selected_action.flatten(), None);
    }

    /// The change-base picker's rows: the recorded base leads, marked as
    /// current — picking it again is a no-op the executor turns into a
    /// notice — then every other local branch in the composer's recency
    /// order. The worktree's own checked-out branch can never be a base.
    fn command_palette_rebase_base_candidates(&self) -> Vec<CommandPaletteItem> {
        let Some(picker) = self.command_palette.rebase_base.as_ref() else {
            return Vec::new();
        };
        let selected = picker.current_base.clone().unwrap_or_default();
        let branches =
            composer::visible_branch_entries(&picker.branches, &selected, "", unix_time());
        let mut order = 0usize;
        let mut items = Vec::new();
        if let Some(base) = &picker.current_base {
            items.push(CommandPaletteItem {
                section: PaletteSection::Branches,
                label: base.clone(),
                detail: Some(tr!("command_palette.current_base")),
                icon: PaletteIcon::Asset("icons/git-branch.svg"),
                shortcut: None,
                action: PaletteAction::RebaseOntoBranch {
                    workspace: picker.workspace.clone(),
                    branch: base.clone(),
                    onto: picker.current_base.clone(),
                },
                content_match: None,
                search_text: format!("{base} base branch rebase onto"),
                order,
                recency: 0,
            });
            order += 1;
        }
        items.extend(
            branches
                .into_iter()
                .filter(|branch| {
                    Some(&branch.name) != picker.current_base.as_ref()
                        && Some(&branch.name) != picker.current_branch.as_ref()
                })
                .map(|branch| {
                    let item = CommandPaletteItem {
                        section: PaletteSection::Branches,
                        label: branch.name.clone(),
                        detail: None,
                        icon: PaletteIcon::Asset("icons/git-branch.svg"),
                        shortcut: None,
                        action: PaletteAction::RebaseOntoBranch {
                            workspace: picker.workspace.clone(),
                            branch: branch.name.clone(),
                            onto: picker.current_base.clone(),
                        },
                        content_match: None,
                        search_text: format!("{} base branch rebase onto", branch.name),
                        order,
                        recency: 0,
                    };
                    order += 1;
                    item
                }),
        );
        items
    }

    fn refresh_command_palette_rebase_base_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_rebase_base_candidates();
        if !query.is_empty() {
            candidates = self.score_run_script_items(candidates, query);
        }
        self.command_palette.results = candidates;
        self.finish_drill_in_refresh(selected_action.flatten(), None);
    }

    /// Same ordering as the run-script step — current, then recent, then
    /// newly added — minus projects that cannot host an issue at all.
    /// A remote project's repo check is its daemon's, not this filesystem's,
    /// so remote rows always make the list and the fetch validates them.
    fn command_palette_issue_project_candidates(&self) -> Vec<CommandPaletteItem> {
        let projects = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless())
            .filter(|project| {
                self.is_remote_project(project.id)
                    || terminals::nearest_repo_root(&project.path).is_some()
            })
            .collect::<Vec<_>>();
        let current = self
            .selected_session()
            .map(|session| session.project_id)
            .filter(|id| projects.iter().any(|project| project.id == *id));
        let recent = self.task_switcher.recent_project_ids(&self.state.sessions);
        run_script::run_script_project_order(current, &recent, &projects)
            .into_iter()
            .enumerate()
            .filter_map(|(order, project_id)| {
                let project = projects.iter().find(|project| project.id == project_id)?;
                let mut detail =
                    settings::abbreviate_home_path(&project.path, self.home_directory.as_deref());
                if current == Some(project_id) {
                    detail = format!("{detail} · {}", tr!("command_palette.current"));
                }
                let label = project.display_name();
                Some(CommandPaletteItem {
                    section: PaletteSection::Projects,
                    search_text: format!(
                        "{label} {} project github issue",
                        project.path.to_string_lossy()
                    ),
                    label,
                    detail: Some(detail),
                    icon: PaletteIcon::Asset("icons/folder.svg"),
                    shortcut: None,
                    action: PaletteAction::ChooseIssueProject(project_id),
                    content_match: None,
                    order,
                    recency: 0,
                })
            })
            .collect()
    }

    /// The template step's rows: "Blank issue" first when `config.yml`
    /// allows it, then Markdown templates, YAML forms, and contact links
    /// — the last two badged as web handoffs. Pending replies give no
    /// rows at all: the loading state doubles as the repo gate, so a
    /// fast Enter cannot reach the form before `gh` answers.
    fn command_palette_issue_template_candidates(&self) -> Vec<CommandPaletteItem> {
        if self.command_palette.issue_templates_pending {
            return Vec::new();
        }
        let mut items = Vec::new();
        if self.command_palette.issue_blank_enabled {
            items.push(CommandPaletteItem {
                section: PaletteSection::Templates,
                label: tr!("command_palette.blank_issue"),
                detail: None,
                icon: PaletteIcon::Asset("icons/file.svg"),
                shortcut: None,
                action: PaletteAction::NewBlankIssue,
                content_match: None,
                search_text: tr!("command_palette.blank_issue_search"),
                order: 0,
                recency: 0,
            });
        }
        let web_hint = tr!("command_palette.opens_on_web");
        let base = items.len();
        items.extend(self.command_palette.issue_templates.iter().enumerate().map(
            |(index, template)| {
                let web = !matches!(
                    template.kind,
                    waku_protocol::workspace::IssueTemplateKind::Markdown
                );
                let icon = if web {
                    "icons/external-link.svg"
                } else {
                    "icons/file.svg"
                };
                let detail = match (&template.about, web) {
                    (Some(about), true) => Some(format!("{about} · {web_hint}")),
                    (Some(about), false) => Some(about.clone()),
                    (None, true) => Some(web_hint.clone()),
                    (None, false) => None,
                };
                CommandPaletteItem {
                    section: PaletteSection::Templates,
                    label: template.name.clone(),
                    detail,
                    icon: PaletteIcon::Asset(icon),
                    shortcut: None,
                    action: PaletteAction::ChooseIssueTemplate(template.clone()),
                    content_match: None,
                    search_text: format!(
                        "{} {} {} issue template",
                        template.name,
                        template.about.as_deref().unwrap_or(""),
                        template.filename
                    ),
                    order: base + index,
                    recency: 0,
                }
            },
        ));
        items
    }

    fn refresh_command_palette_issue_project_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_issue_project_candidates();
        if !query.is_empty() {
            candidates = self.score_run_script_items(candidates, query);
        }
        self.command_palette.results = candidates;
        self.finish_drill_in_refresh(
            selected_action.flatten(),
            Some(PaletteAction::ChooseIssueProject),
        );
    }

    fn refresh_command_palette_issue_template_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_issue_template_candidates();
        if !query.is_empty() {
            candidates = self.score_run_script_items(candidates, query);
        }
        self.command_palette.results = candidates;
        self.finish_drill_in_refresh(selected_action.flatten(), None);
    }

    /// "New task in…" rows: every registered project first (a pick reuses
    /// it rather than duplicating it), then the daemon's directory scan
    /// minus those same paths. Both resolve through `NewTaskInDirectory`;
    /// the handler decides whether the directory becomes a temporary
    /// project or joins an existing one.
    fn command_palette_new_task_candidates(&self) -> Vec<CommandPaletteItem> {
        let mut order = 0usize;
        let home = self.home_directory.as_deref();
        let directory_item = |order: &mut usize, path: PathBuf, project: Option<&Project>| {
            let current = *order;
            *order += 1;
            let name = project
                .map(|project| project.display_name())
                .unwrap_or_else(|| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned())
                });
            let path_label = settings::abbreviate_home_path(&path, home);
            let detail = match project {
                Some(_) => format!("{} · {}", path_label, tr!("command_palette.project")),
                None => path_label.clone(),
            };
            let icon = match project {
                Some(project) if project.temporary => "icons/folder-clock.svg",
                _ => "icons/folder.svg",
            };
            CommandPaletteItem {
                section: PaletteSection::Directories,
                label: name.clone(),
                detail: Some(detail),
                icon: PaletteIcon::Asset(icon),
                shortcut: None,
                action: PaletteAction::NewTaskInDirectory(path.clone()),
                content_match: None,
                search_text: format!("{name} {path_label} directory folder project"),
                order: current,
                recency: 0,
            }
        };
        let mut items = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless())
            .map(|project| directory_item(&mut order, project.path.clone(), Some(project)))
            .collect::<Vec<_>>();
        let project_paths: HashSet<PathBuf> = self
            .state
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        items.extend(
            self.command_palette
                .new_task_directories
                .iter()
                .filter(|path| !project_paths.contains(*path))
                .map(|path| directory_item(&mut order, path.clone(), None)),
        );
        items
    }

    fn refresh_command_palette_new_task_results(&mut self, query: &str, preserve_selection: bool) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_new_task_candidates();
        if !query.is_empty() {
            candidates = self.score_run_script_items(candidates, query);
        }
        // The daemon scan can return thousands of directories; rows are
        // built eagerly, so the list is capped whether or not a query
        // already narrowed it.
        candidates.truncate(MAX_DIRECTORY_RESULTS);
        self.command_palette.results = candidates;
        self.finish_drill_in_refresh(selected_action.flatten(), None);
    }

    fn refresh_command_palette_resume_results(&mut self, query: &str, preserve_selection: bool) {
        let query = query.trim();
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let mut candidates = self.command_palette_resume_candidates();
        if query.is_empty() {
            candidates.sort_by(|a, b| b.recency.cmp(&a.recency).then(a.order.cmp(&b.order)));
            candidates.truncate(MAX_RESUME_RESULTS);
            self.command_palette.results = candidates;
        } else {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let mut utf32 = Vec::new();
            let mut scored = candidates
                .into_iter()
                .filter_map(|item| {
                    pattern
                        .score(
                            Utf32Str::new(&item.search_text, &mut utf32),
                            &mut self.command_palette.matcher,
                        )
                        .map(|score| ScoredPaletteItem { score, item })
                })
                .collect::<Vec<_>>();
            scored.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then(b.item.recency.cmp(&a.item.recency))
                    .then(a.item.order.cmp(&b.item.order))
            });
            scored.truncate(MAX_RESUME_RESULTS);
            self.command_palette.results = scored.into_iter().map(|scored| scored.item).collect();
        }
        let provider_selector = self.command_palette_resume_provider_selector();
        self.command_palette.results.insert(0, provider_selector);
        self.command_palette.selected = selected_action
            .flatten()
            .and_then(|action| {
                self.command_palette
                    .results
                    .iter()
                    .position(|item| item.action == action)
            })
            .unwrap_or_else(|| usize::from(self.command_palette.results.len() > 1));
        let scroll_index = self.command_palette_scroll_index(self.command_palette.selected);
        self.command_palette.scroll.scroll_to_item(scroll_index);
    }

    fn refresh_command_palette_resume_provider_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
    ) {
        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let query = query.trim();
        let mut candidates = self.command_palette_resume_provider_candidates();
        if !query.is_empty() {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let mut utf32 = Vec::new();
            let mut scored = candidates
                .into_iter()
                .filter_map(|item| {
                    pattern
                        .score(
                            Utf32Str::new(&item.search_text, &mut utf32),
                            &mut self.command_palette.matcher,
                        )
                        .map(|score| ScoredPaletteItem { score, item })
                })
                .collect::<Vec<_>>();
            scored.sort_by(|left, right| {
                right
                    .score
                    .cmp(&left.score)
                    .then(left.item.order.cmp(&right.item.order))
            });
            candidates = scored.into_iter().map(|scored| scored.item).collect();
        }
        self.command_palette.results = candidates;
        self.command_palette.selected = selected_action
            .flatten()
            .and_then(|action| {
                self.command_palette
                    .results
                    .iter()
                    .position(|item| item.action == action)
            })
            .or_else(|| {
                self.command_palette.results.iter().position(|item| {
                    item.action
                        == PaletteAction::SelectResumeProvider(self.command_palette.resume_provider)
                })
            })
            .unwrap_or(0);
        self.command_palette
            .scroll
            .scroll_to_item(self.command_palette_scroll_index(self.command_palette.selected));
    }

    /// Re-run the open palette's current query. The Prompts section reads
    /// the composer's command index, so a late-arriving discovery refresh
    /// calls this to keep the drawn rows honest. A no-op while closed.
    pub(super) fn refresh_open_command_palette(&mut self, cx: &mut Context<Self>) {
        if !self.command_palette.open {
            return;
        }
        let query = self.command_palette.search.read(cx).content().to_owned();
        self.refresh_command_palette_results(&query, true, cx);
    }

    fn refresh_command_palette_results(
        &mut self,
        query: &str,
        preserve_selection: bool,
        cx: &App,
    ) {
        match self.command_palette.view {
            CommandPaletteView::Resume => {
                self.refresh_command_palette_resume_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::ResumeProviders => {
                self.refresh_command_palette_resume_provider_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::RunScriptProjects => {
                self.refresh_command_palette_run_script_project_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::RunScripts => {
                self.refresh_command_palette_run_script_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::RemoveProject => {
                self.refresh_command_palette_remove_project_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::NewTaskIn => {
                self.refresh_command_palette_new_task_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::IssueProjects => {
                self.refresh_command_palette_issue_project_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::IssueTemplates => {
                self.refresh_command_palette_issue_template_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::RebaseBase => {
                self.refresh_command_palette_rebase_base_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::SavePrompt => {
                self.refresh_command_palette_save_prompt_results(query, preserve_selection);
                return;
            }
            CommandPaletteView::Commands => {}
        }
        let updater_available = cx
            .try_global::<crate::updater::UpdaterState>()
            .is_some_and(|state| state.0.is_some());
        let query = query.trim();
        if query.is_empty() {
            self.command_palette.results = self.command_palette_commands(false, updater_available);
            self.command_palette.selected = 0;
            self.command_palette.scroll.scroll_to_item(0);
            return;
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let message_matches_are_current =
            self.command_palette.message_matches_query.as_deref() == Some(query);
        let mut utf32 = Vec::new();
        let mut tasks = self
            .command_palette_task_candidates()
            .into_iter()
            .filter_map(|mut item| {
                let metadata_score = pattern.score(
                    Utf32Str::new(&item.search_text, &mut utf32),
                    &mut self.command_palette.matcher,
                );
                let content_score = item.content_match.as_ref().and_then(|matched| {
                    pattern.score(
                        Utf32Str::new(&matched.snippet, &mut utf32),
                        &mut self.command_palette.matcher,
                    )
                });
                let current_content_match =
                    message_matches_are_current && item.content_match.is_some();
                if content_score.is_none() && !current_content_match {
                    // Pending queries retain the last resolved transcript
                    // snapshot, but an unrelated old excerpt should neither
                    // keep a row alive nor be displayed under the new query.
                    item.content_match = None;
                }
                metadata_score
                    .into_iter()
                    .chain(content_score)
                    .max()
                    .or_else(|| current_content_match.then_some(0))
                    .map(|score| ScoredPaletteItem { score, item })
            })
            .collect::<Vec<_>>();
        tasks.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(b.item.recency.cmp(&a.item.recency))
                .then(a.item.order.cmp(&b.item.order))
        });
        tasks.truncate(MAX_TASK_RESULTS);

        let mut commands = self
            .command_palette_commands(true, updater_available)
            .into_iter()
            .filter_map(|item| {
                pattern
                    .score(
                        Utf32Str::new(&item.search_text, &mut utf32),
                        &mut self.command_palette.matcher,
                    )
                    .map(|score| ScoredPaletteItem { score, item })
            })
            .collect::<Vec<_>>();
        commands.sort_by(|a, b| b.score.cmp(&a.score).then(a.item.order.cmp(&b.item.order)));

        let selected_action = preserve_selection.then(|| {
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|item| item.action.clone())
        });
        let mut scored_results = tasks;
        scored_results.extend(commands);
        order_sections_by_best_score(&mut scored_results);
        let next_results = scored_results
            .into_iter()
            .map(|scored| scored.item)
            .collect::<Vec<_>>();
        // This is the palette equivalent of TanStack Query's
        // `keepPreviousData`: never replace useful rows with a transient blank
        // frame while the transcript query is still in flight.
        if should_keep_previous_command_palette_results(
            next_results.len(),
            self.command_palette.message_search_pending,
            self.command_palette.results.len(),
        ) {
            self.command_palette.selected = 0;
            self.command_palette.scroll.scroll_to_item(0);
            return;
        }
        self.command_palette.results = next_results;
        self.command_palette.selected = selected_action
            .flatten()
            .and_then(|action| {
                self.command_palette
                    .results
                    .iter()
                    .position(|item| item.action == action)
            })
            .unwrap_or(0);
        let scroll_index = self.command_palette_scroll_index(self.command_palette.selected);
        self.command_palette.scroll.scroll_to_item(scroll_index);
    }

    fn command_palette_scroll_index(&self, selected: usize) -> usize {
        let mut headers = 0;
        let mut previous = None;
        for item in self.command_palette.results.iter().take(selected + 1) {
            if previous != Some(item.section) {
                if item.section != PaletteSection::Providers {
                    headers += 1;
                }
                previous = Some(item.section);
            }
        }
        selected + headers
    }

    fn move_command_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.command_palette.results.len();
        let Some(next) = next_selection_index(self.command_palette.selected, len, delta) else {
            return;
        };
        self.command_palette.selected = next;
        self.command_palette
            .scroll
            .scroll_to_item(self.command_palette_scroll_index(next));
        cx.notify();
    }

    fn set_command_palette_selection(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.command_palette.results.len() && self.command_palette.selected != index {
            self.command_palette.selected = index;
            cx.notify();
        }
    }

    fn default_resume_provider(&self) -> ProviderKind {
        self.selected_session()
            .map(|session| session.provider)
            .filter(|provider| {
                provider.supports_session_catalog()
                    && !self.state.disabled_providers.contains(provider)
            })
            .or_else(|| {
                ProviderKind::ALL.into_iter().find(|provider| {
                    provider.supports_session_catalog()
                        && !self.state.disabled_providers.contains(provider)
                })
            })
            .unwrap_or_default()
    }

    fn open_command_palette_resume_provider_view(&mut self, cx: &mut Context<Self>) {
        self.command_palette.view = CommandPaletteView::ResumeProviders;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.resume_provider_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();
    }

    fn open_command_palette_resume_view(
        &mut self,
        provider: Option<ProviderKind>,
        cx: &mut Context<Self>,
    ) {
        let provider = provider.unwrap_or_else(|| self.default_resume_provider());
        self.command_palette.view = CommandPaletteView::Resume;
        self.command_palette.resume_provider = provider;
        self.command_palette.provider_sessions.clear();
        self.command_palette.provider_sessions_pending = true;
        self.command_palette.provider_session_import = None;
        self.command_palette.provider_session_error = None;
        self.command_palette.provider_session_status = ProviderSessionCatalogStatus::Ready;
        self.command_palette.provider_session_generation = self
            .command_palette
            .provider_session_generation
            .wrapping_add(1);
        let generation = self.command_palette.provider_session_generation;
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("command_palette.resume_placeholder"), cx);
            input.clear(cx);
        });
        self.refresh_command_palette_results("", false, cx);
        cx.notify();

        let fetch = self
            .store
            .provider_sessions(provider, PROVIDER_SESSION_CATALOG_LIMIT);
        cx.spawn(async move |waku, cx| {
            let result = cx.background_executor().spawn(async move { fetch() }).await;
            let _ = waku.update(cx, |waku, cx| {
                if !waku.command_palette.open
                    || waku.command_palette.provider_session_generation != generation
                    || waku.command_palette.resume_provider != provider
                {
                    return;
                }
                waku.command_palette.provider_sessions_pending = false;
                match result {
                    Ok(catalog) => {
                        waku.command_palette.provider_sessions = catalog.sessions;
                        waku.command_palette.provider_session_status = catalog.status;
                        waku.command_palette.provider_session_error = None;
                    }
                    Err(error) => {
                        waku.command_palette.provider_sessions.clear();
                        waku.command_palette.provider_session_error = Some(error.to_string());
                    }
                }
                if waku.command_palette.view == CommandPaletteView::Resume {
                    let query = waku.command_palette.search.read(cx).content().to_owned();
                    waku.refresh_command_palette_results(&query, false, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn import_provider_session(
        &mut self,
        key: waku_client::DaemonKey,
        summary: ProviderSessionSummary,
        history: ProviderSessionHistory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session_id) = self
            .state
            .sessions
            .iter()
            .find(|session| {
                session
                    .provider_cursor
                    .as_ref()
                    .is_some_and(|cursor| same_provider_session(cursor, &summary.cursor))
            })
            .map(|session| session.id)
        {
            self.close_command_palette(window, cx);
            self.settings_page = None;
            self.select_session(session_id, cx);
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
            return;
        }

        // Match on path *and* owner: `/repo` locally and `/repo` on a remote
        // host are different projects.
        let project_id = if let Some(project) = self.state.projects.iter().find(|project| {
            project.path == summary.cwd && self.daemons.project_owner(project.id) == key
        }) {
            project.id
        } else {
            let project = Project::from_path(summary.cwd.clone());
            let project_id = project.id;
            self.daemons.claim_project(project_id, key);
            self.state.projects.push(project);
            self.analytics.track(crate::analytics::Event::ProjectAdded);
            project_id
        };
        let provider = summary.provider();
        let runtime_mode = self
            .selected_session()
            .map(|session| session.runtime_mode)
            .unwrap_or(self.state.last_runtime_mode);
        let now = unix_time();
        let created_at = if summary.created_at == 0 {
            now
        } else {
            summary.created_at
        };
        let updated_at = summary.updated_at.max(created_at);
        let has_history = !history.messages.is_empty() || !history.turns.is_empty();
        let mut session = AgentSession::new(project_id, provider);
        session.runtime_mode = runtime_mode;
        session.auto_title = Some(summary.title);
        session.provider_cursor = Some(summary.cursor);
        session.created_at = created_at;
        session.updated_at = updated_at;
        session.last_reply_at = has_history.then_some(updated_at);
        session.messages = history.messages;
        session.turns = history.turns;
        let session_id = session.id;
        self.daemons.claim_session(session_id, key);
        self.track_task_created(&session, "imported");
        self.state.push_session(session);

        self.close_command_palette(window, cx);
        self.settings_page = None;
        self.select_session(session_id, cx);
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    fn load_command_palette_provider_session(
        &mut self,
        key: waku_client::DaemonKey,
        summary: ProviderSessionSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.command_palette.provider_session_import.is_some() {
            return;
        }
        if let Some(session_id) = self
            .state
            .sessions
            .iter()
            .find(|session| {
                session
                    .provider_cursor
                    .as_ref()
                    .is_some_and(|cursor| same_provider_session(cursor, &summary.cursor))
            })
            .map(|session| session.id)
        {
            self.close_command_palette(window, cx);
            self.settings_page = None;
            self.select_session(session_id, cx);
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
            return;
        }

        self.command_palette.provider_session_import = Some(summary.cursor.clone());
        self.command_palette.provider_session_error = None;
        self.command_palette.provider_session_generation = self
            .command_palette
            .provider_session_generation
            .wrapping_add(1);
        let generation = self.command_palette.provider_session_generation;
        let cursor = summary.cursor.clone();
        let fetch = self
            .store
            .provider_session_history(key, cursor.clone(), summary.cwd.clone());
        let window_handle = window.window_handle();
        cx.notify();

        cx.spawn(async move |waku, cx| {
            let result = cx.background_executor().spawn(async move { fetch() }).await;
            let update = waku.update(cx, |waku, cx| {
                if !waku.command_palette.open
                    || waku.command_palette.view != CommandPaletteView::Resume
                    || waku.command_palette.provider_session_generation != generation
                    || waku
                        .command_palette
                        .provider_session_import
                        .as_ref()
                        .is_none_or(|pending| !same_provider_session(pending, &cursor))
                {
                    return None;
                }
                match result {
                    Ok(loaded) => {
                        let mut summary = summary;
                        if let Some(resolved) = loaded.resolved_cwd {
                            summary.cwd = resolved;
                        }
                        Some((summary, loaded.history))
                    }
                    Err(error) => {
                        let error = error.to_string();
                        waku.command_palette.provider_session_import = None;
                        waku.command_palette.provider_session_error = Some(error.clone());
                        waku.show_toast(tr!("command_palette.resume_failed", error = error));
                        cx.notify();
                        None
                    }
                }
            });
            let Ok(Some((summary, history))) = update else {
                return;
            };
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = waku.update(cx, |waku, cx| {
                    if waku.command_palette.open
                        && waku.command_palette.view == CommandPaletteView::Resume
                        && waku.command_palette.provider_session_generation == generation
                    {
                        waku.import_provider_session(key, summary, history, window, cx);
                    }
                });
            });
        })
        .detach();
    }

    fn execute_command_palette_selection(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = index.unwrap_or(self.command_palette.selected);
        let Some(action) = self
            .command_palette
            .results
            .get(index)
            .map(|item| item.action.clone())
        else {
            return;
        };

        match action {
            PaletteAction::Resume => {
                self.open_command_palette_resume_view(None, cx);
                return;
            }
            PaletteAction::ChooseResumeProvider => {
                self.open_command_palette_resume_provider_view(cx);
                return;
            }
            PaletteAction::SelectResumeProvider(provider) => {
                self.open_command_palette_resume_view(Some(provider), cx);
                return;
            }
            PaletteAction::ResumeProviderSession(key, summary) => {
                self.load_command_palette_provider_session(key, summary, window, cx);
                return;
            }
            PaletteAction::OpenRunScript => {
                self.open_command_palette_run_script_projects_view(cx);
                return;
            }
            PaletteAction::OpenRemoveProject => {
                self.open_command_palette_remove_project_view(cx);
                return;
            }
            PaletteAction::NewTaskIn => {
                self.open_command_palette_new_task_view(cx);
                return;
            }
            PaletteAction::ChooseRunScriptProject(project_id) => {
                self.open_command_palette_run_scripts_view(project_id, cx);
                return;
            }
            PaletteAction::CreateGitHubIssue => {
                self.start_issue_flow(window, cx);
                return;
            }
            PaletteAction::ChooseIssueProject(project_id) => {
                if let Some(project) = self
                    .state
                    .projects
                    .iter()
                    .find(|project| project.id == project_id)
                {
                    let target = issue_dialog::IssueTarget {
                        cwd: project.path.clone(),
                        project: Some(project.id),
                    };
                    self.begin_issue_template_fetch(target, window, cx);
                }
                return;
            }
            PaletteAction::OpenSavePrompt => {
                self.open_command_palette_save_prompt_view(cx);
                return;
            }
            PaletteAction::ChangeBaseBranch => {
                self.open_command_palette_rebase_base_view(cx);
                return;
            }
            PaletteAction::SavePromptAs(slug) => {
                // The helper decides whether the naming step stays open —
                // a validation failure keeps the typed name editable.
                self.save_prompt_template(&slug, window, cx);
                return;
            }
            _ => {}
        }

        self.close_command_palette(window, cx);
        match action {
            PaletteAction::NewTask => self.new_session_action(&NewSession, window, cx),
            PaletteAction::NewTaskInDirectory(path) => {
                self.create_task_in_directory(path, window, cx)
            }
            PaletteAction::NewTaskInSameWorktree => self.new_task_in_same_worktree(window, cx),
            PaletteAction::OpenProject => self.new_project_action(&NewProject, window, cx),
            PaletteAction::RemoveProject(project_id) => self.remove_project(project_id, cx),
            PaletteAction::FocusComposer => self.focus_composer_action(&FocusComposer, window, cx),
            PaletteAction::CreateDraft => self.create_saved_draft(window, cx),
            PaletteAction::ViewDrafts => self.open_drafts_page(window, cx),
            PaletteAction::CopyIdentifier(identifier) => {
                if let Some(value) = identifier.value(self.selected_session()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(value));
                    self.show_success_toast(identifier.copied_message());
                    cx.notify();
                }
            }
            PaletteAction::CollapseSidebarGroups => self.collapse_all_sidebar_groups(cx),
            PaletteAction::GoToNextUnreadCompletion => {
                self.go_to_next_unread_completion_action(&GoToNextUnreadCompletion, window, cx)
            }
            PaletteAction::MarkAllSessionsRead => self.mark_all_sessions_read(cx),
            PaletteAction::CheckForUpdates => {
                window.dispatch_action(CheckForUpdates.boxed_clone(), cx)
            }
            PaletteAction::ToggleSidebar => self.toggle_sidebar_action(&ToggleSidebar, window, cx),
            PaletteAction::ToggleRightPanel => {
                self.toggle_right_panel_action(&ToggleRightPanel, window, cx)
            }
            PaletteAction::OpenSettings(page) => {
                self.open_settings_action(&OpenSettings, window, cx);
                self.open_settings_page(page, window, cx);
            }
            PaletteAction::NewCustomCommand => {
                self.open_settings_action(&OpenSettings, window, cx);
                self.open_settings_page(SettingsPage::Commands, window, cx);
                self.open_custom_command_editor(None, window, cx);
                if let Some(editor) = self.custom_command_editor.as_mut() {
                    editor.exit_settings_on_save = true;
                }
            }
            PaletteAction::SelectTask(session_id) => {
                self.settings_page = None;
                self.select_session(session_id, cx);
                let focus = self.composer_focus(cx);
                window.focus(&focus, cx);
            }
            PaletteAction::ToggleWorkspace => {
                // Reachable over Settings; reveal the app so the chip change
                // is visible, then run the same path the keystroke takes.
                self.settings_page = None;
                self.toggle_workspace_action(&ToggleWorkspace, window, cx);
            }
            PaletteAction::OpenOnGitHub => {
                if let Some(url) = self
                    .selected_branch_snapshot()
                    .and_then(|snapshot| branches::github_branch_url(snapshot))
                {
                    cx.open_url(&url);
                }
            }
            PaletteAction::MoveToWorktree => {
                self.settings_page = None;
                if let Some(session_id) = self.state.selected_session {
                    self.move_session_to_worktree(session_id, None, cx);
                }
            }
            PaletteAction::LandChanges => {
                self.settings_page = None;
                self.land_composer_session(waku_client::git::PullStrategy::Rebase, cx);
            }
            PaletteAction::RebaseOntoBranch {
                workspace,
                branch,
                onto,
            } => {
                self.settings_page = None;
                // Re-picking the recorded base changes nothing — say so
                // rather than run a rebase that moves nothing.
                if onto.as_deref() == Some(branch.as_str()) {
                    self.show_toast(tr!("command_palette.rebase_base_unchanged", base = branch));
                } else {
                    self.start_git_panel_rebase(
                        workspace,
                        branch,
                        onto,
                        waku_client::git::PullStrategy::Rebase,
                        true,
                        cx,
                    );
                }
            }
            PaletteAction::SyncBranch => {
                self.settings_page = None;
                self.open_sync_branch(window, cx);
            }
            PaletteAction::CompactContext => {
                self.settings_page = None;
                if let Some(session_id) = self.composer_session_id() {
                    self.compact_session(session_id, cx);
                }
            }
            PaletteAction::RunCustomCommand(command_id) => {
                if let Some(command) = self
                    .state
                    .custom_commands
                    .iter()
                    .find(|command| command.id == command_id)
                    .cloned()
                {
                    self.settings_page = None;
                    self.custom_command_editor = None;
                    self.run_custom_command(command, cx);
                }
            }
            PaletteAction::InsertPromptTemplate(command) => {
                self.settings_page = None;
                let text = if command.argument_hint.is_some() {
                    // Keep the `/name ` spelling so the template's argument
                    // flow still applies at submit.
                    format!("/{} ", command.name)
                } else {
                    // Landing the expanded body turns the pick into
                    // "browse → adapt → send" — the prompt text is visible
                    // in the composer instead of expanded invisibly at send.
                    crate::composer_complete::expand_command_template(
                        command.template.as_deref().unwrap_or_default(),
                        "",
                    )
                };
                self.composer
                    .update(cx, |input, cx| input.insert_text(&text, cx));
                let focus = self.composer_focus(cx);
                window.focus(&focus, cx);
            }
            PaletteAction::RevealPromptTemplates => {
                if let Some(home) = self.home_directory.clone() {
                    let dir = prompt_templates_dir(&home);
                    // Reveal needs the folder to exist; creating it beats a
                    // Finder error on a fresh install.
                    match std::fs::create_dir_all(&dir) {
                        Ok(()) => crate::platform::reveal_in_file_manager(&dir, cx),
                        Err(error) => self.show_toast(tr!(
                            "prompt_templates.open_failed",
                            error = error.to_string()
                        )),
                    }
                    cx.notify();
                }
            }
            PaletteAction::ChooseModel | PaletteAction::ToggleUsage => {
                // These popovers are rendered by the composer. If the command
                // came from Settings, reveal one normal app frame first so its
                // persistent menu handle and anchor bounds are current.
                self.settings_page = None;
                let focus = self.composer_focus(cx);
                window.focus(&focus, cx);
                let weak = cx.entity().downgrade();
                let choose_model = matches!(action, PaletteAction::ChooseModel);
                window.on_next_frame(move |window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if choose_model {
                            this.toggle_model_picker_action(&ToggleModelPicker, window, cx)
                        } else {
                            this.toggle_usage_panel_action(&ToggleUsagePanel, window, cx)
                        }
                    });
                });
            }
            PaletteAction::InspectElements => {
                element_inspector::start(element_inspector::InspectorMode::Elements, window, cx);
            }
            PaletteAction::InspectColors => {
                element_inspector::start(element_inspector::InspectorMode::Colors, window, cx);
            }
            PaletteAction::ToggleAutoRestart => self.toggle_auto_restart(cx),
            PaletteAction::RunScript { project, script } => {
                self.settings_page = None;
                self.run_project_script(project, script, window, cx);
            }
            PaletteAction::ChooseIssueTemplate(template) => match template.kind {
                waku_protocol::workspace::IssueTemplateKind::Markdown => {
                    self.open_issue_dialog_from_palette(Some(template), window, cx)
                }
                waku_protocol::workspace::IssueTemplateKind::YamlForm => {
                    if let Some(url) = self.issue_template_web_url(&template) {
                        cx.open_url(&url);
                    }
                }
                waku_protocol::workspace::IssueTemplateKind::ContactLink => {
                    if let Some(url) = &template.url {
                        cx.open_url(url);
                    }
                }
            },
            PaletteAction::NewBlankIssue => {
                self.open_issue_dialog_from_palette(None, window, cx);
            }
            PaletteAction::Resume
            | PaletteAction::ChooseResumeProvider
            | PaletteAction::SelectResumeProvider(_)
            | PaletteAction::ResumeProviderSession(..)
            | PaletteAction::OpenRunScript
            | PaletteAction::ChooseRunScriptProject(_)
            | PaletteAction::OpenRemoveProject
            | PaletteAction::NewTaskIn
            | PaletteAction::CreateGitHubIssue
            | PaletteAction::ChooseIssueProject(_)
            | PaletteAction::OpenSavePrompt
            | PaletteAction::ChangeBaseBranch
            | PaletteAction::SavePromptAs(_) => {
                unreachable!("view-navigation actions are handled before closing the palette")
            }
        }
    }

    /// Flip the dev watcher's auto-restart flag — the file it handed this
    /// app through `GODDARD_DEV_STATE`. Only reachable when the path exists.
    fn toggle_auto_restart(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.dev_state_path.clone() else {
            return;
        };
        let enabled = !self.auto_restart_enabled;
        match write_auto_restart(&path, enabled) {
            Ok(()) => {
                self.auto_restart_enabled = enabled;
                self.show_success_toast(tr!(if enabled {
                    "command_palette.auto_restart_enabled"
                } else {
                    "command_palette.auto_restart_disabled"
                }));
            }
            Err(error) => self.show_toast(tr!(
                "command_palette.auto_restart_failed",
                error = error.to_string()
            )),
        }
        cx.notify();
    }

    pub(super) fn render_command_palette(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.command_palette.open {
            return None;
        }
        let theme = Theme::current(cx);
        let viewport_height = f32::from(window.viewport_size().height);
        let top = (viewport_height * 0.09).clamp(48.0, 72.0);
        let card_max_height = (viewport_height - top - 36.0)
            .max(SEARCH_ROW_HEIGHT)
            .min(MAX_CARD_HEIGHT);
        let selected = self
            .command_palette
            .selected
            .min(self.command_palette.results.len().saturating_sub(1));
        let search_query = self.command_palette.search.read(cx).content().to_owned();
        let view = self.command_palette.view;
        let resume_view = view == CommandPaletteView::Resume;
        let run_scripts_view = view == CommandPaletteView::RunScripts;
        let issue_templates_view = view == CommandPaletteView::IssueTemplates;
        let resume_session_count = self
            .command_palette
            .results
            .iter()
            .filter(|item| matches!(&item.action, PaletteAction::ResumeProviderSession(..)))
            .count();
        let results_pending = match view {
            CommandPaletteView::Resume => self.command_palette.provider_sessions_pending,
            CommandPaletteView::Commands => self.command_palette.message_search_pending,
            CommandPaletteView::RunScripts => self.command_palette.run_scripts_pending,
            CommandPaletteView::NewTaskIn => self.command_palette.new_task_directories_pending,
            CommandPaletteView::IssueTemplates => self.command_palette.issue_templates_pending,
            CommandPaletteView::RebaseBase => self
                .command_palette
                .rebase_base
                .as_ref()
                .is_some_and(|picker| picker.pending),
            CommandPaletteView::ResumeProviders
            | CommandPaletteView::RunScriptProjects
            | CommandPaletteView::RemoveProject
            | CommandPaletteView::IssueProjects
            | CommandPaletteView::SavePrompt => false,
        };
        let show_empty_state = should_show_command_palette_empty_state(
            if resume_view {
                resume_session_count
            } else {
                self.command_palette.results.len()
            },
            results_pending,
        );
        let show_loading_state = (resume_view
            && resume_session_count == 0
            && self.command_palette.provider_sessions_pending)
            || (run_scripts_view
                && self.command_palette.results.is_empty()
                && self.command_palette.run_scripts_pending)
            || (view == CommandPaletteView::NewTaskIn
                && self.command_palette.results.is_empty()
                && self.command_palette.new_task_directories_pending)
            || (issue_templates_view
                && self.command_palette.results.is_empty()
                && self.command_palette.issue_templates_pending)
            || (view == CommandPaletteView::RebaseBase
                && self.command_palette.results.is_empty()
                && self
                    .command_palette
                    .rebase_base
                    .as_ref()
                    .is_some_and(|picker| picker.pending));
        let show_placeholder_state = show_empty_state || show_loading_state;
        let results_height =
            command_palette_results_height(&self.command_palette.results, show_placeholder_state)
                .min((card_max_height - SEARCH_ROW_HEIGHT - FOOTER_HEIGHT).max(0.0));
        let card_height = SEARCH_ROW_HEIGHT + results_height + FOOTER_HEIGHT;

        let mut results = div()
            .id("command-palette-results")
            .h(px(results_height))
            .flex_none()
            .overflow_y_scroll()
            .track_scroll(&self.command_palette.scroll)
            .px(px(8.0))
            .pb(px(8.0));

        if show_placeholder_state {
            let error = resume_view
                .then(|| self.command_palette.provider_session_error.clone())
                .flatten();
            let (icon_path, title, hint, spinning) = if show_loading_state {
                (
                    "icons/loader-circle.svg",
                    if run_scripts_view {
                        tr!("command_palette.loading_scripts")
                    } else if view == CommandPaletteView::NewTaskIn {
                        tr!("command_palette.loading_directories")
                    } else if issue_templates_view {
                        tr!("command_palette.loading_issue_templates")
                    } else if view == CommandPaletteView::RebaseBase {
                        tr!("command_palette.loading_branches")
                    } else {
                        tr!("command_palette.loading_sessions")
                    },
                    None,
                    true,
                )
            } else if let Some(error) = error {
                (
                    "icons/alert.svg",
                    tr!("command_palette.could_not_load_sessions"),
                    Some(error),
                    false,
                )
            } else if resume_view
                && self.command_palette.provider_session_status
                    == ProviderSessionCatalogStatus::Unsupported
            {
                (
                    "icons/search.svg",
                    tr!("command_palette.resume_unsupported"),
                    Some(tr!(
                        "command_palette.resume_unsupported_hint",
                        provider = self.command_palette.resume_provider.display_name()
                    )),
                    false,
                )
            } else if resume_view
                && self.command_palette.provider_session_status
                    == ProviderSessionCatalogStatus::BinaryMissing
            {
                (
                    "icons/download.svg",
                    tr!("command_palette.resume_not_installed"),
                    Some(tr!(
                        "command_palette.resume_not_installed_hint",
                        provider = self.command_palette.resume_provider.display_name()
                    )),
                    false,
                )
            } else if resume_view {
                (
                    "icons/search.svg",
                    tr!("command_palette.no_resume_sessions"),
                    Some(tr!("command_palette.no_resume_sessions_hint")),
                    false,
                )
            } else if view == CommandPaletteView::RunScriptProjects {
                (
                    "icons/folder.svg",
                    tr!("command_palette.no_projects"),
                    Some(tr!("command_palette.no_projects_hint")),
                    false,
                )
            } else if view == CommandPaletteView::IssueProjects {
                (
                    "icons/folder.svg",
                    tr!("command_palette.no_projects"),
                    Some(tr!("command_palette.no_issue_projects_hint")),
                    false,
                )
            } else if issue_templates_view {
                (
                    "icons/github.svg",
                    tr!("command_palette.no_issue_templates"),
                    Some(tr!("command_palette.no_issue_templates_hint")),
                    false,
                )
            } else if run_scripts_view {
                (
                    "icons/terminal.svg",
                    tr!("command_palette.no_scripts"),
                    Some(tr!("command_palette.no_scripts_hint")),
                    false,
                )
            } else if view == CommandPaletteView::NewTaskIn {
                (
                    "icons/folder-search.svg",
                    tr!("command_palette.no_directories"),
                    Some(tr!("command_palette.no_directories_hint")),
                    false,
                )
            } else if view == CommandPaletteView::RebaseBase {
                (
                    "icons/git-branch.svg",
                    tr!("command_palette.no_branches"),
                    Some(tr!("command_palette.no_branches_hint")),
                    false,
                )
            } else {
                (
                    "icons/search.svg",
                    tr!("command_palette.no_results"),
                    Some(tr!("command_palette.no_results_hint")),
                    false,
                )
            };
            let empty_icon = icon(icon_path, 18.0, theme.text_ghost);
            results = results.child(
                div()
                    .h(px(EMPTY_RESULTS_HEIGHT))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(if spinning {
                        motion::spin(empty_icon)
                    } else {
                        empty_icon.into_any_element()
                    })
                    .child(
                        div()
                            .mt(px(12.0))
                            .text_size(sp(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child(title),
                    )
                    .when_some(hint, |empty, hint| {
                        empty.child(
                            div()
                                .max_w(px(540.0))
                                .mt(px(5.0))
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(hint),
                        )
                    })
                    .when(show_loading_state && resume_view, |empty| {
                        let provider = self.command_palette.resume_provider;
                        empty.child(
                            div()
                                .mt(px(12.0))
                                .flex()
                                .items_center()
                                .gap(px(7.0))
                                .child(provider_mark(&theme, provider, 13.0, theme.text_secondary))
                                .child(
                                    div()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_secondary)
                                        .child(provider.display_name().to_owned()),
                                ),
                        )
                    })
                    .when(resume_view && !show_loading_state, |empty| {
                        let provider = self.command_palette.resume_provider;
                        empty.child(
                            div()
                                .id("command-palette-resume-provider")
                                .mt(px(12.0))
                                .h(px(28.0))
                                .px(px(9.0))
                                .rounded(px(10.0))
                                .border(hairline())
                                .border_color(theme.border)
                                .flex()
                                .items_center()
                                .gap(px(7.0))
                                .cursor_default()
                                .hover(|button| button.bg(theme.overlay))
                                .active(|button| button.opacity(0.82))
                                .child(provider_mark(&theme, provider, 13.0, theme.text_secondary))
                                .child(
                                    div()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_secondary)
                                        .child(provider.display_name().to_owned()),
                                )
                                .child(icon(
                                    "icons/chevron-down.svg",
                                    11.0,
                                    theme.affordance_icon(),
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_command_palette_resume_provider_view(cx);
                                    cx.stop_propagation();
                                })),
                        )
                    }),
            );
        } else {
            let mut previous_section = None;
            for (index, item) in self.command_palette.results.iter().enumerate() {
                let starts_section = previous_section != Some(item.section);
                if starts_section {
                    if item.section != PaletteSection::Providers {
                        results = results.child(
                            div()
                                .h(px(SECTION_HEADER_HEIGHT))
                                .px(px(9.0))
                                .pt(px(10.0))
                                .flex()
                                .items_center()
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_tertiary)
                                .child(item.section.label()),
                        );
                    }
                    previous_section = Some(item.section);
                }

                let highlighted = index == selected;
                let icon_color = theme.text_secondary;
                // A provider row renders through `provider_mark` so OpenCode 2
                // keeps its badge; an asset row stays a plain tinted icon.
                let row_mark = match item.icon {
                    PaletteIcon::Asset(path) => icon(path, 16.0, icon_color).into_any_element(),
                    PaletteIcon::Provider(provider) => {
                        provider_mark(&theme, provider, 16.0, icon_color).into_any_element()
                    }
                };
                let importing = match &item.action {
                    PaletteAction::ResumeProviderSession(_, summary) => self
                        .command_palette
                        .provider_session_import
                        .as_ref()
                        .is_some_and(|cursor| same_provider_session(cursor, &summary.cursor)),
                    _ => false,
                };
                let detail = if importing {
                    Some(tr!("command_palette.loading_selected_session"))
                } else {
                    item.detail.clone()
                };
                let content_match = item.content_match.clone();
                let shortcut = item
                    .shortcut
                    .as_ref()
                    .and_then(|hint| hint.resolve(window, cx));
                results = results.child(
                    div()
                        .id(SharedString::from(format!("command-palette-row-{index}")))
                        .when(
                            starts_section && item.section == PaletteSection::Providers,
                            |row| row.mt(px(PROVIDER_SECTION_TOP_MARGIN)),
                        )
                        .h(px(command_palette_row_height(item)))
                        .px(px(11.0))
                        .rounded(px(11.0))
                        .border(hairline())
                        .border_color(if highlighted {
                            theme.border_strong
                        } else {
                            gpui::transparent_black()
                        })
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .cursor_default()
                        .when(highlighted, |row| row.bg(theme.overlay_strong))
                        .hover(|row| row.bg(theme.overlay))
                        .active(|row| row.opacity(0.82))
                        .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                            if *hovering {
                                this.set_command_palette_selection(index, cx);
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.execute_command_palette_selection(Some(index), window, cx);
                            cx.stop_propagation();
                        }))
                        .child(
                            div()
                                .size(px(20.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(if importing {
                                    motion::spin(icon("icons/loader-circle.svg", 16.0, icon_color))
                                } else {
                                    row_mark
                                }),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .justify_center()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .items_baseline()
                                        .gap(px(7.0))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(sp(14.0))
                                                .font_weight(if highlighted {
                                                    FontWeight::MEDIUM
                                                } else {
                                                    FontWeight::NORMAL
                                                })
                                                .text_color(if highlighted {
                                                    theme.text
                                                } else {
                                                    theme.text_secondary
                                                })
                                                .child(item.label.clone()),
                                        )
                                        .when_some(detail, |row, detail| {
                                            row.child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(sp(12.5))
                                                    .text_color(theme.text_tertiary)
                                                    .child(detail),
                                            )
                                        }),
                                )
                                .when_some(content_match, |column, matched| {
                                    column.child(
                                        div()
                                            .min_w_0()
                                            .w_full()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_size(sp(12.5))
                                            .child(palette_content_match_text(
                                                &matched,
                                                &search_query,
                                                window,
                                                theme,
                                            )),
                                    )
                                }),
                        )
                        .when_some(shortcut, |row, shortcut| {
                            row.child(
                                div()
                                    .h(px(22.0))
                                    .min_w(px(28.0))
                                    .px(px(7.0))
                                    .rounded(px(9.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(theme.overlay_strong)
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .child(shortcut),
                            )
                        }),
                );
            }
        }

        let card =
            div()
                .id("command-palette-card")
                .key_context("CommandPalette")
                .on_action(cx.listener(Self::toggle_command_palette_action))
                .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                    this.move_command_palette_selection(1, cx)
                }))
                .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                    this.move_command_palette_selection(-1, cx)
                }))
                .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                    this.move_command_palette_selection(isize::MIN, cx)
                }))
                .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                    this.move_command_palette_selection(isize::MAX, cx)
                }))
                .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                    this.move_command_palette_selection(PAGE_STEP, cx)
                }))
                .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                    this.move_command_palette_selection(-PAGE_STEP, cx)
                }))
                .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                    this.execute_command_palette_selection(None, window, cx)
                }))
                .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                    this.dismiss_command_palette(window, cx)
                }))
                .w_full()
                .max_w(px(680.0))
                .h(px(card_height))
                .overflow_hidden()
                .rounded(px(18.0))
                .bg(theme.raised)
                .shadow_xl()
                .relative()
                .flex()
                .flex_col()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .h(px(SEARCH_ROW_HEIGHT))
                        .px(px(19.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .border_b(hairline())
                        .border_color(theme.separator)
                        .text_size(sp(15.5))
                        .text_color(theme.text)
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .child(self.command_palette.search.clone()),
                        ),
                )
                .child(results)
                .child(
                    div()
                        .h(px(FOOTER_HEIGHT))
                        .flex_none()
                        .border_t(hairline())
                        .border_color(theme.separator)
                        .px(px(19.0))
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .children(
                            [
                                ("↑↓", tr!("command_palette.hint_navigate")),
                                ("↵", tr!("command_palette.hint_select")),
                                ("esc", tr!("command_palette.hint_close")),
                            ]
                            .map(|(keys, label)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.0))
                                    .text_size(sp(12.0))
                                    .child(div().text_color(theme.text_tertiary).child(keys))
                                    .child(div().text_color(theme.text_ghost).child(label))
                            }),
                        ),
                );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.26)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.14)
        };
        let layer = div()
            .id("command-palette-layer")
            .absolute()
            .inset_0()
            .occlude()
            .bg(scrim)
            .px(px(24.0))
            .pt(px(top))
            .flex()
            .items_start()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_command_palette(window, cx)),
            )
            .child(motion::modal_enter("command-palette-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("command-palette-layer-enter", layer))
                .with_priority(3)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(query: &str, candidate: &str) -> Option<u32> {
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut matcher = crate::composer_complete::matcher();
        let mut buf = Vec::new();
        pattern.score(Utf32Str::new(candidate, &mut buf), &mut matcher)
    }

    #[test]
    fn fuzzy_search_matches_command_words_and_initials() {
        assert!(score("open proj", "Open project folder repository").is_some());
        assert!(score("cmptr use", "Computer Use settings accessibility").is_some());
        assert!(score("totally absent", "Open project folder repository").is_none());
    }

    #[test]
    fn copy_identifier_commands_use_the_selected_tasks_live_ids() {
        let mut session = AgentSession::new(Uuid::nil(), ProviderKind::Codex);
        session.id = Uuid::parse_str("ed28ee51-43cf-4a83-a52f-04c509ca2c09").unwrap();

        assert_eq!(
            PaletteIdentifier::TaskId.value(Some(&session)).as_deref(),
            Some("ed28ee51-43cf-4a83-a52f-04c509ca2c09")
        );
        assert_eq!(
            PaletteIdentifier::AgentCliThreadId.value(Some(&session)),
            None
        );

        session.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "019cfd7a-6942-78b1-9d47-30576c562321".into(),
        });
        assert_eq!(
            PaletteIdentifier::AgentCliThreadId
                .value(Some(&session))
                .as_deref(),
            Some("019cfd7a-6942-78b1-9d47-30576c562321")
        );
    }

    #[test]
    fn provider_session_identity_uses_provider_and_native_id() {
        let listed = ProviderResumeCursor::Claude {
            session_id: "11111111-1111-4111-8111-111111111111".into(),
            resume_at: None,
        };
        let imported = ProviderResumeCursor::Claude {
            session_id: "11111111-1111-4111-8111-111111111111".into(),
            resume_at: Some("native-message".into()),
        };
        let other = ProviderResumeCursor::Codex {
            thread_id: "11111111-1111-4111-8111-111111111111".into(),
        };

        assert!(same_provider_session(&listed, &imported));
        assert!(!same_provider_session(&listed, &other));
    }

    #[test]
    fn scroll_indexes_include_section_headers() {
        let sections = [
            PaletteSection::Tasks,
            PaletteSection::Tasks,
            PaletteSection::Commands,
            PaletteSection::Settings,
        ];
        let scroll_index = |selected: usize| {
            let mut headers = 0;
            let mut previous = None;
            for section in sections.iter().take(selected + 1) {
                if previous != Some(*section) {
                    headers += 1;
                    previous = Some(*section);
                }
            }
            selected + headers
        };
        assert_eq!(scroll_index(0), 1);
        assert_eq!(scroll_index(1), 2);
        assert_eq!(scroll_index(2), 4);
        assert_eq!(scroll_index(3), 6);
    }

    #[test]
    fn arrows_wrap_while_page_and_boundary_keys_clamp() {
        assert_eq!(next_selection_index(0, 5, -1), Some(4));
        assert_eq!(next_selection_index(4, 5, 1), Some(0));
        assert_eq!(next_selection_index(3, 5, PAGE_STEP), Some(4));
        assert_eq!(next_selection_index(1, 5, -PAGE_STEP), Some(0));
        assert_eq!(next_selection_index(2, 5, isize::MIN), Some(0));
        assert_eq!(next_selection_index(2, 5, isize::MAX), Some(4));
        assert_eq!(next_selection_index(0, 0, 1), None);
    }

    #[test]
    fn prompt_names_slugify_to_command_stems() {
        assert_eq!(slugify_prompt_name("Review Diff"), "review-diff");
        assert_eq!(slugify_prompt_name("  tidy  up  "), "tidy-up");
        assert_eq!(slugify_prompt_name("fix: lint!"), "fix-lint");
        assert_eq!(slugify_prompt_name("--leading--"), "leading");
        assert_eq!(slugify_prompt_name("日本語"), "");
        assert_eq!(slugify_prompt_name(""), "");
    }

    #[test]
    fn sections_follow_their_best_score_when_searching() {
        let item = |section: PaletteSection, order: usize| {
            CommandPaletteItem::command(
                section,
                format!("Item {order}"),
                "icons/search.svg",
                None,
                PaletteAction::NewTask,
                "",
                order,
            )
        };
        // Typing "merge": the literal "Merge into dev" custom command must
        // outrank built-in commands that only matched scattered letters in
        // stuffed keywords, despite Commands' better browsing rank.
        let mut scored = vec![
            ScoredPaletteItem {
                score: 10,
                item: item(PaletteSection::Commands, 0),
            },
            ScoredPaletteItem {
                score: 5,
                item: item(PaletteSection::Commands, 1),
            },
            ScoredPaletteItem {
                score: 500,
                item: item(PaletteSection::CustomCommands, 2),
            },
            ScoredPaletteItem {
                score: 40,
                item: item(PaletteSection::Tasks, 3),
            },
            ScoredPaletteItem {
                score: 40,
                item: item(PaletteSection::Settings, 4),
            },
        ];
        order_sections_by_best_score(&mut scored);
        let sections = scored
            .iter()
            .map(|scored| scored.item.section)
            .collect::<Vec<_>>();
        assert_eq!(
            sections,
            [
                PaletteSection::CustomCommands,
                // Equal scores fall back to browsing rank: Tasks before Settings.
                PaletteSection::Tasks,
                PaletteSection::Settings,
                PaletteSection::Commands,
                PaletteSection::Commands,
            ]
        );
    }

    #[test]
    fn no_results_appears_only_after_background_search_settles() {
        assert!(!should_show_command_palette_empty_state(0, true));
        assert!(should_show_command_palette_empty_state(0, false));
        assert!(!should_show_command_palette_empty_state(1, false));
        assert!(should_keep_previous_command_palette_results(0, true, 1));
        assert!(!should_keep_previous_command_palette_results(0, false, 1));
        assert!(!should_keep_previous_command_palette_results(1, true, 1));
    }

    #[test]
    fn result_height_hugs_rows_and_section_headers() {
        let item = |section, order| {
            CommandPaletteItem::command(
                section,
                format!("Item {order}"),
                "icons/search.svg",
                None,
                PaletteAction::NewTask,
                "",
                order,
            )
        };
        let items = vec![
            item(PaletteSection::Tasks, 0),
            item(PaletteSection::Tasks, 1),
            item(PaletteSection::Commands, 2),
        ];
        assert_eq!(
            command_palette_results_height(&items, false),
            SECTION_HEADER_HEIGHT * 2.0 + RESULT_ROW_HEIGHT * 3.0 + RESULTS_BOTTOM_PADDING
        );
        assert_eq!(
            command_palette_results_height(&[], true),
            EMPTY_RESULTS_HEIGHT + RESULTS_BOTTOM_PADDING
        );
        let providers = vec![
            item(PaletteSection::Providers, 0),
            item(PaletteSection::Providers, 1),
        ];
        assert_eq!(
            command_palette_results_height(&providers, false),
            PROVIDER_SECTION_TOP_MARGIN + RESULT_ROW_HEIGHT * 2.0 + RESULTS_BOTTOM_PADDING
        );
    }

    #[test]
    fn render_reads_only_the_cached_result_snapshot() {
        let source = include_str!("./command_palette.rs");
        let start = source
            .find("\n    pub(super) fn render_command_palette(")
            .expect("render function must exist");
        let end = source[start..]
            // Match from the final newline only so this accepts both LF and
            // CRLF checkouts.
            .find("\n#[cfg(test)]")
            .map(|offset| start + offset)
            .expect("test module marker must exist");
        let render = &source[start..end];
        for forbidden in [
            "refresh_command_palette_results(",
            "command_palette_task_candidates(",
            "session_message_search(",
            "background_executor(",
            "std::fs",
            "Command::new",
            "read_dir",
        ] {
            assert!(
                !render.contains(forbidden),
                "palette render must not call `{forbidden}`"
            );
        }
    }
}
