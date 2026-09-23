use std::collections::BTreeMap;

use super::model_picker::{
    ModelPickerPanel, PickerGranularity, PickerRow, PickerRowSpec, PickerSection, PolicyRowId,
    model_default_effort, model_picker_empty_state, model_picker_panel, model_picker_row_body,
    model_picker_row_shell, model_picker_subtitle, next_picker_highlight, picker_lists_provider,
    picker_provider_rail_item, picker_row_section, picker_rows, picker_section_rail_item,
    policy_row_parts, route_class_picker_state,
};
use super::usage_page::format_tokens_compact;
use super::*;
use crate::theme::{ThemeName, ThemeSettings};
use crate::ui::ActivationExt;
use gpui::{ElementId, HighlightStyle, KeyBinding, StyledText, Svg, actions};
use waku_protocol::auto_prompts::{AutoPromptQuestion, AutoPromptRule};
use waku_protocol::integrations::{IntegrationAuthKind, IntegrationAuthState};
use waku_protocol::routing::{ALL_TASK_CLASSES, RouteClassTarget, TaskClass};

pub(super) struct AutoPromptQuestionEditor {
    id: Uuid,
    instructions: Entity<TextInput>,
    weight: Entity<TextInput>,
}

pub(super) struct AutoPromptEditor {
    id: Uuid,
    name: Entity<TextInput>,
    prompt: Entity<TextInput>,
    questions: Vec<AutoPromptQuestionEditor>,
    threshold: Entity<TextInput>,
    advanced: bool,
    pending: bool,
    status: Option<Result<String, String>>,
    preview: Option<(Vec<(String, f64)>, f64, bool)>,
}

const SETTINGS_CONTENT_MAX_WIDTH: f32 = 760.0;

/// The Usage page is a dashboard, not a form; it mirrors T3 Code's wide
/// two-column layout and needs the extra room for the chart.
const SETTINGS_USAGE_MAX_WIDTH: f32 = 1024.0;

/// The Git page's Worktrees/Branches tables carry five-column rows, so it
/// shares the Usage page's wide layout rather than the form width.
const SETTINGS_GIT_MAX_WIDTH: f32 = SETTINGS_USAGE_MAX_WIDTH;

/// Uniform height hint for the virtualized archived-chat rows, so the
/// scrollbar knows the total extent before rows are measured.
const ARCHIVED_SESSION_ROW_HEIGHT: f32 = 45.0;

/// The archived filter's transcript scan uses the command palette's shape:
/// one best match per session, capped well above what one page can show.
const ARCHIVED_MESSAGE_SEARCH_LIMIT: usize = 50;
const ARCHIVED_MESSAGE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(90);
/// Resolved transcript queries kept for the field's lifetime, so backspacing
/// to an earlier query does not re-scan SQLite.
pub(super) const ARCHIVED_MESSAGE_SEARCH_CACHE_CAPACITY: usize = 24;

/// Key context the settings sidebar declares around its search field.
const SETTINGS_SIDEBAR_CONTEXT: &str = "SettingsSidebar";

/// The search field while focused inside the sidebar. The field holds real
/// focus the whole time — the sidebar's selection is only drawn — so `up` and
/// `down` have to be claimed from under it, and only a binding can do that:
/// they arrive as actions, which consume the keystroke before the field sees
/// it.
const SETTINGS_SEARCH_CONTEXT: &str = "SettingsSidebar > TextInput";

/// Every page, editor, and form inside the settings surface is under this
/// identifier — the `tab`/`shift-tab` bindings below traverse all of them
/// rather than letting a field take Tab as text. The surface stamps it next
/// to `Workspace`, which the app-wide bindings take their scope from.
const SETTINGS_CONTEXT: &str = "Settings";

actions!(waku_settings, [FocusNext, FocusPrevious]);

/// A QR module matrix encoded once — dark modules row-major — so the
/// settings paint pass only reads it.
pub(super) struct DaemonQrCode {
    pub width: usize,
    pub dark: Vec<bool>,
}

/// The sidebar's rows in display order, each with the keyword haystack the
/// search field filters against.
const SETTINGS_PAGES: [(SettingsPage, &str, &str, &str); 17] = [
    (
        SettingsPage::General,
        "settings.general",
        "icons/settings.svg",
        "settings.general_keywords",
    ),
    (
        SettingsPage::Appearance,
        "settings.appearance",
        "icons/appearance.svg",
        "settings.appearance_keywords",
    ),
    (
        SettingsPage::Keybindings,
        "keybind.title",
        "icons/keyboard.svg",
        "keybind.keywords",
    ),
    (
        SettingsPage::Providers,
        "settings.providers",
        "icons/bot.svg",
        "settings.providers_keywords",
    ),
    (
        SettingsPage::Skills,
        "settings.skills",
        "icons/package.svg",
        "settings.skills_keywords",
    ),
    (
        SettingsPage::Commands,
        "settings.commands",
        "icons/terminal.svg",
        "settings.commands_keywords",
    ),
    (
        SettingsPage::Terminal,
        "settings.terminal",
        "icons/terminal-square.svg",
        "settings.terminal_keywords",
    ),
    (
        SettingsPage::Git,
        "settings.git",
        "icons/git-branch.svg",
        "settings.git_keywords",
    ),
    (
        SettingsPage::Usage,
        "settings.usage",
        "icons/chart-column.svg",
        "settings.usage_keywords",
    ),
    (
        SettingsPage::Archived,
        "settings.archived",
        "icons/archive.svg",
        "settings.archived_keywords",
    ),
    (
        SettingsPage::Daemon,
        "settings.daemon",
        "icons/server.svg",
        "settings.daemon_keywords",
    ),
    (
        SettingsPage::Friends,
        "settings.friends",
        "icons/send.svg",
        "settings.friends_keywords",
    ),
    (
        SettingsPage::ComputerUse,
        "settings.computer_use",
        "icons/cursor-spark.svg",
        "settings.computer_use_keywords",
    ),
    (
        SettingsPage::Jev,
        "settings.jev",
        "icons/provider-typesafe-padded.svg",
        "settings.jev_keywords",
    ),
    (
        SettingsPage::Integrations,
        "settings.integrations",
        "icons/globe.svg",
        "settings.integrations_keywords",
    ),
    (
        SettingsPage::Experiments,
        "settings.experiments",
        "icons/beaker.svg",
        "settings.experiments_keywords",
    ),
    (
        SettingsPage::Diagnostics,
        "settings.diagnostics",
        "icons/gauge.svg",
        "settings.diagnostics_keywords",
    ),
];

/// Bind the settings fields' navigation keys. Called once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNextEntry, Some(SETTINGS_SEARCH_CONTEXT)),
        KeyBinding::new("up", SelectPreviousEntry, Some(SETTINGS_SEARCH_CONTEXT)),
        KeyBinding::new("tab", FocusNext, Some(SETTINGS_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(SETTINGS_CONTEXT)),
    ]);
}

/// The Commands settings page's open form. Input entities live for the
/// editor's lifetime rather than being pre-created with the other settings
/// fields.
pub(super) struct CustomCommandEditor {
    /// `None` while the form is creating a new command.
    id: Option<Uuid>,
    name: Entity<TextInput>,
    icon: CustomCommandIcon,
    shell: Entity<TextInput>,
    script: Entity<TextInput>,
    close_on_success: bool,
    /// The script text the command had when the editor opened — its hashed
    /// file is dropped on save once no other command still uses it.
    previous_script: Option<String>,
    /// Set when Save was pressed with an empty script.
    script_required: bool,
    /// A new-command editor opened from the command palette leaves Settings
    /// on save instead of landing back on the Commands page.
    pub(super) exit_settings_on_save: bool,
}

pub(super) struct SuggestedPromptEditor {
    id: &'static str,
    input: Entity<TextInput>,
    error: Option<String>,
}

/// The Integrations page's open connect form. The API-key input exists only
/// for services that accept one; OAuth-only services go straight to the
/// browser.
pub(super) struct IntegrationEditor {
    pub(super) id: String,
    variant_id: String,
    providers: HashSet<ProviderKind>,
    api_key: Option<Entity<TextInput>>,
}

/// How the editor reaches the host: over the platform `ssh` with
/// provisioning and a forwarded socket, or a direct WebSocket to an
/// already-running daemon. Non-unix builds only offer `Direct`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RemoteHostTransport {
    Ssh,
    Direct,
}

/// The Daemon settings page's open remote-host form. Input entities live for
/// the editor's lifetime rather than being pre-created with the other
/// settings fields.
pub(super) struct RemoteHostEditor {
    /// `Some` when re-pointing an existing record; `None` adds a host.
    pub(super) id: Option<Uuid>,
    /// Which field group is active — `Ssh` uses `destination`, `Direct`
    /// uses `address`/`token`. Always `Direct` off unix.
    transport: RemoteHostTransport,
    name: Entity<TextInput>,
    address: Entity<TextInput>,
    token: Entity<TextInput>,
    /// `user@host` or a `~/.ssh/config` alias — the SSH transport's only
    /// required field.
    destination: Entity<TextInput>,
    /// Eye-toggle state for the masked token field.
    token_revealed: bool,
    /// `Host` aliases from `~/.ssh/config`, loaded in the background after
    /// the editor opens; each chips-row entry fills the destination field.
    ssh_hosts: Vec<String>,
    /// Set when Save was pressed with the active transport's required
    /// fields incomplete.
    missing_fields: bool,
}

/// The sidebar rows the query leaves visible, in display order. `query` must
/// already be trimmed and lowercased; when it is empty every page matches.
/// Friends and Integrations are experiments — their rows only appear while
/// the opt-in is on; Jev's stays while any eval-backed experiment is on.
pub(super) fn visible_settings_pages(
    query: &str,
    computer_use_experiment_enabled: bool,
    friends_enabled: bool,
    jev_in_use: bool,
    integrations_enabled: bool,
) -> impl Iterator<Item = (SettingsPage, String, &'static str)> + '_ {
    SETTINGS_PAGES
        .into_iter()
        .filter(move |(page, ..)| {
            page.is_visible_in_navigation(
                computer_use_experiment_enabled,
                friends_enabled,
                jev_in_use,
                integrations_enabled,
            )
        })
        .filter_map(move |(page, label_key, icon, keywords_key)| {
            let label = crate::i18n::translate(label_key);
            let keywords = crate::i18n::translate(keywords_key).to_lowercase();
            (query.is_empty() || keywords.contains(query)).then_some((page, label, icon))
        })
}

/// The pages whose render functions carry searchable setting rows, in
/// sidebar order. The other pages are self-contained surfaces (tables,
/// master/detail panes) with their own filters — they never appear in
/// search results rather than rendering degenerate inside a section.
const SEARCHABLE_SETTINGS_PAGES: [SettingsPage; 10] = [
    SettingsPage::General,
    SettingsPage::Appearance,
    SettingsPage::Providers,
    SettingsPage::Commands,
    SettingsPage::Terminal,
    SettingsPage::Daemon,
    SettingsPage::Friends,
    SettingsPage::ComputerUse,
    SettingsPage::Jev,
    SettingsPage::Experiments,
];

/// The Experiments page's groups, in display order — the taxonomy the
/// changelog's topic groups already use.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExperimentGroup {
    Sessions,
    Git,
    Surfaces,
}

impl ExperimentGroup {
    const ALL: [Self; 3] = [Self::Sessions, Self::Git, Self::Surfaces];

    fn title_key(self) -> &'static str {
        match self {
            Self::Sessions => "experiments.group_sessions",
            Self::Git => "experiments.group_git",
            Self::Surfaces => "experiments.group_surfaces",
        }
    }
}

/// One Experiments-page opt-in: its group, card text, current flag value,
/// and the setter the toggle calls. `set` is a method pointer —
/// `Self::set_*_enabled` coerces — so the page's table stays data.
/// `eval_backed` marks the opt-ins that run on the Jev backend — an enabled
/// card warns when no usable backend is configured. `tuning` renders
/// parameter controls under the card while the experiment is on.
struct ExperimentDef {
    group: ExperimentGroup,
    id: &'static str,
    icon: &'static str,
    title_key: &'static str,
    description_key: &'static str,
    enabled: bool,
    set: fn(&mut Waku, bool, &mut Context<Waku>),
    eval_backed: bool,
    tuning: Option<fn(&Waku, Theme, &mut Context<Waku>) -> AnyElement>,
}

/// The query state shared by every settings row built in one render pass.
/// `hits` counts the rows a page keeps under the query so the content column
/// can tell whether a whole section rendered empty without diffing trees.
#[derive(Clone)]
pub(super) struct SettingSearch {
    /// Trimmed, lowercased field content; empty means "not searching".
    query: Rc<str>,
    hits: Rc<Cell<usize>>,
    /// Set when the query matched the section's own title: every row in the
    /// section stays visible instead of only the ones matching the query.
    force: bool,
    /// `matched` calls so far this pass. A row's ordinal is stable between
    /// the results column and its natural page because both run the same
    /// render function in the same order.
    rows: Rc<Cell<usize>>,
    /// The page this pass renders, when the pass participates in
    /// results-column navigation — set for both sides of a jump.
    anchor_page: Option<SettingsPage>,
    scroll: ScrollHandle,
    anchors: Rc<RefCell<HashMap<(SettingsPage, usize), ScrollAnchor>>>,
    /// Set only while the results column renders — turns row titles into
    /// links back to their page.
    visit: Option<WeakEntity<Waku>>,
}

/// One matched row's highlights plus its jump-back context.
pub(super) struct SettingRowMatch {
    pub title_ranges: Vec<Range<usize>>,
    pub description_ranges: Vec<Range<usize>>,
    /// The row's (page, ordinal) when this pass participates in
    /// results-column navigation — the anchor registry key, identical on
    /// both sides of a jump.
    pub key: Option<(SettingsPage, usize)>,
    /// The row's persistent anchor — attached to its title element so the
    /// natural page records where the row painted.
    pub anchor: Option<ScrollAnchor>,
    /// Present only in the results column: clicking the title jumps back.
    pub visit: Option<WeakEntity<Waku>>,
}

impl SettingSearch {
    fn new(query: &str) -> Self {
        Self {
            query: Rc::from(query),
            hits: Rc::new(Cell::new(0)),
            force: false,
            rows: Rc::new(Cell::new(0)),
            anchor_page: None,
            scroll: ScrollHandle::new(),
            anchors: Rc::new(RefCell::new(HashMap::new())),
            visit: None,
        }
    }

    /// The unfiltered mode every row renders under outside a search.
    fn inactive() -> Self {
        Self::new("")
    }

    /// The section-forced mode, used when the query matched the section's
    /// own title — the whole section renders rather than an empty shell.
    fn forced(query: &str) -> Self {
        let mut search = Self::new(query);
        search.force = true;
        search
    }

    /// Point this pass at `page`'s natural render: rows get ordinals and
    /// persistent anchors in the shared registry, so a results-column click
    /// can find where the same row lands outside the search.
    fn for_page(
        mut self,
        page: SettingsPage,
        scroll: &ScrollHandle,
        anchors: &Rc<RefCell<HashMap<(SettingsPage, usize), ScrollAnchor>>>,
    ) -> Self {
        self.anchor_page = Some(page);
        self.scroll = scroll.clone();
        self.anchors = anchors.clone();
        self
    }

    /// Results-column mode: kept rows carry a link that opens their page.
    fn visiting(mut self, weak: WeakEntity<Waku>) -> Self {
        self.visit = Some(weak);
        self
    }

    pub(super) fn active(&self) -> bool {
        !self.query.is_empty()
    }

    pub(super) fn hits(&self) -> usize {
        self.hits.get()
    }

    /// The title and description match ranges when the row stays visible —
    /// every row when `force` is on or the query is empty — else `None`.
    /// Each kept row counts toward the page's hit total while searching.
    /// Every call consumes an ordinal, kept or not, so ordinals match the
    /// natural page's render order exactly.
    pub(super) fn matched(&self, title: &str, description: &str) -> Option<SettingRowMatch> {
        let ordinal = self.rows.get();
        self.rows.set(ordinal + 1);
        let title_ranges = settings_match_ranges(title, &self.query);
        let description_ranges = settings_match_ranges(description, &self.query);
        if self.active() {
            if !self.force && title_ranges.is_empty() && description_ranges.is_empty() {
                return None;
            }
            self.hits.set(self.hits.get() + 1);
        }
        let key = self.anchor_page.map(|page| (page, ordinal));
        let anchor = key.map(|key| {
            self.anchors
                .borrow_mut()
                .entry(key)
                .or_insert_with(|| ScrollAnchor::for_handle(self.scroll.clone()))
                .clone()
        });
        let visit = self.visit.clone();
        Some(SettingRowMatch {
            title_ranges,
            description_ranges,
            key,
            anchor,
            visit,
        })
    }
}

/// Every occurrence of `query` inside `text` as byte ranges, matched
/// case-insensitively. `StyledText` slices the original on these offsets, so
/// when lowercasing changes the text's length (rare, e.g. İ) the whole
/// string counts as one match rather than risking a misaligned range.
pub(super) fn settings_match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let lowered = text.to_lowercase();
    if lowered.len() != text.len() {
        return if lowered.contains(query) {
            vec![0..text.len()]
        } else {
            Vec::new()
        };
    }
    lowered
        .match_indices(query)
        .map(|(index, hit)| index..index + hit.len())
        .collect()
}

/// The marker a search match gets inside a settings row — a warning-tinted
/// wash behind full-strength text so the hit reads in either theme.
fn settings_search_highlight(theme: Theme) -> HighlightStyle {
    HighlightStyle {
        color: Some(theme.text),
        background_color: Some(theme.warning.alpha(0.3)),
        ..Default::default()
    }
}

#[track_caller]
pub(super) fn settings_search_text(
    text: impl Into<SharedString>,
    ranges: Vec<Range<usize>>,
    theme: Theme,
) -> AnyElement {
    let text = text.into();
    if ranges.is_empty() {
        text.into_any_element()
    } else {
        let highlight = settings_search_highlight(theme);
        StyledText::new(text)
            .with_highlights(ranges.into_iter().map(|range| (range, highlight)))
            .into_any_element()
    }
}

/// A result row's title doubles as the jump back to its natural page: the
/// anchor records where the title paints on either side of the search, and
/// while the results column renders the title becomes a keyboard-operable
/// link that opens the page scrolled to that spot.
#[track_caller]
pub(super) fn settings_title_jump(
    title: Div,
    matched: &SettingRowMatch,
    theme: Theme,
) -> AnyElement {
    let Some((page, ordinal)) = matched.key else {
        return title.into_any_element();
    };
    // `anchor_scroll` lives on StatefulInteractiveElement, so the title
    // carries its registry key as an element id on both sides of the jump.
    let title = title
        .id(SharedString::from(format!(
            "setting-jump-{}-{}",
            page as usize, ordinal
        )))
        .anchor_scroll(matched.anchor.clone());
    let Some(weak) = matched.visit.clone() else {
        return title.into_any_element();
    };
    let key_weak = weak.clone();
    title
        .tab_index(0)
        .cursor_pointer()
        .hover(|element| element.text_color(theme.accent))
        .focus_visible(|element| element.bg(theme.focus_highlight()))
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.visit_setting(page, Some(ordinal), window, cx);
            });
            cx.stop_propagation();
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if !event.keystroke.modifiers.modified()
                && matches!(event.keystroke.key.as_str(), "enter" | "space")
            {
                let _ = key_weak.update(cx, |this, cx| {
                    this.visit_setting(page, Some(ordinal), window, cx);
                });
                cx.stop_propagation();
            }
        })
        .into_any_element()
}

/// The title + description column every settings row shares, with the
/// query's matches highlighted. `matched` comes from
/// [`SettingSearch::matched`]; the caller only builds the row it wraps when
/// that returns `Some`.
#[track_caller]
pub(super) fn settings_row_text(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    matched: SettingRowMatch,
    theme: Theme,
) -> Div {
    let title = title.into();
    let description = description.into();
    let title_element = div()
        .text_size(sp(13.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text)
        .child(settings_search_text(
            title,
            matched.title_ranges.clone(),
            theme,
        ));
    div()
        .flex_1()
        .min_w_0()
        .child(settings_title_jump(title_element, &matched, theme))
        .child(
            div()
                .mt(px(5.0))
                .text_size(sp(12.5))
                .line_height(sp(18.0))
                .text_color(theme.text_secondary)
                .child(settings_search_text(
                    description,
                    matched.description_ranges,
                    theme,
                )),
        )
}

/// The lucide glyph leading every settings row, centered in a fixed-width
/// slot so the title columns line up across toggles, pickers, and fields.
#[track_caller]
pub(super) fn settings_row_icon(path: &'static str, theme: Theme) -> Div {
    div()
        .w(px(20.0))
        .flex_none()
        .flex()
        .justify_center()
        .child(icon(path, 16.0, theme.text_tertiary))
}

/// A settings row's leading content: the row's icon beside the text column
/// [`settings_row_text`] builds. Rows that extend the column (inline fields,
/// extra lines) pass the extended div through unchanged.
#[track_caller]
pub(super) fn settings_row_label(icon_path: &'static str, text: Div, theme: Theme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(settings_row_icon(icon_path, theme))
        .child(text)
}

/// The dormancy-threshold picker's row label — "1 day", "N days", or
/// "Never" for the auto-dormancy kill switch.
fn dormant_after_label(days: Option<u32>) -> String {
    match days {
        None => tr!("settings.dormant_after_never"),
        Some(1) => tr!("settings.dormant_after_one_day"),
        Some(days) => tr!("settings.dormant_after_days", count = days),
    }
}

/// The key's product name — Command/Option on macOS, Ctrl/Alt elsewhere.
fn link_modifier_label(modifier: TerminalLinkModifier) -> &'static str {
    match modifier {
        TerminalLinkModifier::CmdOrCtrl => crate::platform::primary_shortcut("Command", "Ctrl"),
        TerminalLinkModifier::Alt => crate::platform::primary_shortcut("Option", "Alt"),
    }
}

/// A standalone-card settings row — the General and Terminal pages' shape —
/// kept or dropped by the search. Pass an empty `div()` as the control for
/// text-only cards. Vertical spacing belongs to the caller's column.
#[track_caller]
fn setting_card(
    icon_path: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    theme: Theme,
    search: &SettingSearch,
) -> Option<AnyElement> {
    let title = title.into();
    let description = description.into();
    let matched = search.matched(&title, &description)?;
    Some(
        div()
            .w_full()
            .min_h(px(60.0))
            .px(px(20.0))
            .py(px(12.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .flex()
            .items_center()
            .gap(px(24.0))
            .child(settings_row_label(
                icon_path,
                settings_row_text(title, description, matched, theme),
                theme,
            ))
            .child(control)
            .into_any_element(),
    )
}

/// One shared card around whichever rows the search kept, hairlines between
/// visible rows only. `None` when the query removed every row, so an empty
/// shell never renders. Vertical spacing belongs to the caller's column.
#[track_caller]
fn settings_row_card(rows: Vec<Option<AnyElement>>, theme: Theme) -> Option<Div> {
    let mut rows = rows.into_iter().flatten().peekable();
    rows.peek()?;
    let mut card = div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(16.0))
        .overflow_hidden()
        .bg(theme.raised);
    while let Some(row) = rows.next() {
        card = card.child(row);
        if rows.peek().is_some() {
            card = card.child(div().mx(px(20.0)).h(hairline()).bg(theme.separator));
        }
    }
    Some(card)
}

/// One labeled cluster of cards on a settings page — a small tertiary header
/// over a card column. `None` when the search emptied the group, so a header
/// never floats over nothing.
fn settings_group(
    title: impl Into<SharedString>,
    cards: Vec<AnyElement>,
    theme: Theme,
) -> Option<AnyElement> {
    if cards.is_empty() {
        return None;
    }
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .px(px(4.0))
                    .text_size(sp(11.5))
                    .text_color(theme.text_tertiary)
                    .child(title.into()),
            )
            .child(div().flex().flex_col().gap(px(10.0)).children(cards))
            .into_any_element(),
    )
}

/// The archived rows the search query and project filter leave visible,
/// preserving the input order (callers sort newest-archived first). `query`
/// must already be trimmed and lowercased, and `project_names` must hold
/// each project id's lowercased display name — title and project both match.
/// `content_matches` carries the transcript hits for this exact query, when
/// the background scan has landed; a session matching on content alone still
/// surfaces.
pub(super) fn filter_archived_sessions(
    sessions: &[&AgentSession],
    query: &str,
    project_filter: Option<Uuid>,
    project_names: &HashMap<Uuid, String>,
    content_matches: Option<&HashMap<Uuid, crate::persistence::SessionMessageMatch>>,
) -> Vec<Uuid> {
    sessions
        .iter()
        .filter(|session| {
            if project_filter.is_some_and(|id| session.project_id != id) {
                return false;
            }
            query.is_empty()
                || session.display_title().to_lowercase().contains(query)
                || project_names
                    .get(&session.project_id)
                    .is_some_and(|name| name.contains(query))
                || content_matches.is_some_and(|matches| matches.contains_key(&session.id))
        })
        .map(|session| session.id)
        .collect()
}

impl Waku {
    pub(super) fn render_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        // The content column renders first: while a search is active it
        // records which sections actually produced rows, and the sidebar
        // filters itself down to exactly that set.
        let content = self.render_settings_content(window, cx);
        let sidebar = self.render_settings_sidebar(window, cx);

        div()
            .key_context("Workspace Settings")
            .track_focus(&self.settings_focus)
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                this.request_window_close(window, cx)
            }))
            .on_action(cx.listener(Self::new_session_action))
            .on_action(cx.listener(Self::new_project_action))
            .on_action(cx.listener(Self::open_settings_action))
            .on_action(cx.listener(Self::toggle_sidebar_action))
            .on_action(cx.listener(Self::toggle_right_panel_action))
            .on_action(cx.listener(Self::toggle_command_palette_action))
            .on_action(cx.listener(Self::toggle_fps_counter_action))
            .on_action(cx.listener(Self::navigate_back_action))
            .on_action(cx.listener(Self::navigate_forward_action))
            .on_action(cx.listener(Self::focus_composer_action))
            .on_action(cx.listener(Self::focus_terminal_action))
            .on_action(cx.listener(Self::cancel_turn_action))
            .on_action(cx.listener(Self::archive_session_action))
            .capture_any_mouse_down(cx.listener(Self::navigation_mouse_down))
            .on_mouse_move(cx.listener(Self::resize_panel_mouse_move))
            .capture_any_mouse_up(cx.listener(Self::finish_panel_resize))
            .size_full()
            .flex()
            .text_color(theme.text)
            .font_family(crate::fonts::current(cx).ui)
            .child(sidebar)
            // The handle straddles the column's left edge — the sidebar's
            // right edge — exactly as it does in the workspace.
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .flex()
                    .child(content)
                    .child(self.render_panel_resize_handle(
                        "settings-sidebar-resize-handle",
                        PanelResizeTarget::Sidebar,
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// The width the settings sidebar paints at: the workspace's panel
    /// fitting with the sidebar forced on, so the shared `sidebar_width`
    /// and the resize drag's clamp agree across both surfaces.
    pub(super) fn settings_sidebar_width(&self, window: &Window) -> f32 {
        fitted_panel_widths(
            f32::from(window.viewport_size().width),
            true,
            self.right_panel_visible || self.git_panel_visible || self.right_panel_slide.is_some(),
            self.sidebar_width,
            self.right_panel_slot_width(),
        )
        .0
    }

    fn render_settings_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let is_resizing = self
            .panel_resize_drag
            .is_some_and(|drag| drag.target == PanelResizeTarget::Sidebar);
        let width = self.settings_sidebar_width(window);
        let current_page = self.settings_page.unwrap_or(SettingsPage::General);
        let query = self.settings_search_query(cx);
        let searching = !query.is_empty();
        let mut navigation = div().flex().flex_col().gap(px(3.0));

        // While searching the sidebar mirrors the results column: only the
        // sections that actually rendered, in scroll order, and none drawn
        // as selected — clicking one scrolls to it instead of switching
        // pages.
        let pages: Vec<(SettingsPage, String, &'static str)> = if searching {
            self.settings_search_sections
                .iter()
                .filter_map(|page| {
                    SETTINGS_PAGES
                        .into_iter()
                        .find(|(candidate, ..)| *candidate == *page)
                        .map(|(page, label_key, icon, _)| {
                            (page, crate::i18n::translate(label_key), icon)
                        })
                })
                .collect()
        } else {
            visible_settings_pages(
                &query,
                self.state.computer_use_experiment_enabled,
                self.state.friends_enabled,
                self.jev_in_use(),
                self.state.integrations_enabled,
            )
            .collect()
        };

        for (page, label, icon_path) in pages {
            let selected = !searching && current_page == page;
            navigation = navigation.child(
                div()
                    .id(SharedString::from(format!(
                        "settings-tab-{}",
                        label.to_lowercase()
                    )))
                    .h(px(36.0))
                    .px(px(11.0))
                    .rounded(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_default()
                    .text_size(sp(13.0))
                    .text_color(if selected {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .when(selected, |element| {
                        element.bg(theme.sidebar_item_background)
                    })
                    .hover(|element| element.bg(theme.sidebar_item_background))
                    .active(|element| element.bg(theme.sidebar_item_background))
                    .child(icon(
                        icon_path,
                        15.0,
                        if selected {
                            theme.text_secondary
                        } else {
                            theme.text_tertiary
                        },
                    ))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if searching {
                            this.scroll_to_settings_section(page, cx);
                        } else {
                            this.open_settings_page(page, window, cx);
                        }
                    })),
            );
        }

        div()
            .key_context(SETTINGS_SIDEBAR_CONTEXT)
            .on_action(cx.listener(|this, _: &SelectNextEntry, window, cx| {
                this.cycle_settings_page("down", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousEntry, window, cx| {
                this.cycle_settings_page("up", window, cx);
            }))
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .relative()
            .bg(if is_resizing {
                theme.sidebar_drag_background
            } else {
                theme.sidebar
            })
            .child(self.render_settings_sidebar_titlebar(window, cx))
            .child(
                div().px(px(12.0)).child(
                    div()
                        .id("settings-back")
                        .h(px(34.0))
                        .px(px(9.0))
                        .rounded(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(9.0))
                        .cursor_default()
                        .text_size(sp(13.0))
                        .text_color(theme.text_secondary)
                        .hover(|element| element.bg(theme.overlay))
                        .active(|element| element.bg(theme.overlay_strong))
                        .child(icon("icons/arrow-left.svg", 15.0, theme.text_tertiary))
                        .child(tr!("settings.back"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.settings_page = None;
                            let focus_handle = this.composer_focus(cx);
                            window.focus(&focus_handle, cx);
                            cx.notify();
                        })),
                ),
            )
            .child(
                div().px(px(12.0)).pt(px(8.0)).child(
                    TextField::new("settings-search-field", self.settings_search.clone())
                        .icon("icons/search.svg", 13.0),
                ),
            )
            .child(div().h(px(18.0)))
            .child(div().px(px(12.0)).child(navigation))
    }

    /// Back/forward between the panes visited this settings visit — the
    /// same hop ⌘[ and ⌘] (or the titlebar arrows) perform. Returns false
    /// when the pane history is exhausted — or while the results column is
    /// up, where a page switch would be invisible — so the caller can fall
    /// back to leaving settings.
    pub(super) fn navigate_settings_history(
        &mut self,
        back: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.settings_search_query(cx).is_empty() {
            return false;
        }
        let Some(current) = self.current_settings_entry() else {
            return false;
        };
        let target = if back {
            self.settings_navigation.go_back(current)
        } else {
            self.settings_navigation.go_forward(current)
        };
        let Some(target) = target else {
            return false;
        };
        self.show_settings_page(target.page, target.offset, window, cx);
        true
    }

    /// The search field's content, normalized the way the page filter expects.
    fn settings_search_query(&self, cx: &App) -> String {
        self.settings_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase()
    }

    /// Scroll the results column so `page`'s section lands at the top. The
    /// sections are the scroll element's direct children, so its index in
    /// `settings_search_sections` is the item index the handle understands.
    fn scroll_to_settings_section(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        if let Some(index) = self
            .settings_search_sections
            .iter()
            .position(|candidate| *candidate == page)
        {
            self.settings_scroll.scroll_to_top_of_item(index);
            self.settings_search_target = Some(page);
            cx.notify();
        }
    }

    /// Leave the results column for `page` itself — a section title jumps to
    /// the page top, a row title (`ordinal`) to the anchor that row recorded
    /// on its last natural-page paint. The anchor read happens a frame out,
    /// once the destination page has laid out.
    fn visit_setting(
        &mut self,
        page: SettingsPage,
        ordinal: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = ordinal.and_then(|ordinal| {
            self.settings_row_anchors
                .borrow()
                .get(&(page, ordinal))
                .cloned()
        });
        self.settings_search.update(cx, |input, cx| input.clear(cx));
        self.open_settings_page(page, window, cx);
        if let Some(anchor) = anchor {
            anchor.scroll_to(window, cx);
        }
    }

    /// Step the selected page through the rows the search leaves visible,
    /// wrapping at both ends. The field keeps focus so typing keeps narrowing
    /// the list; the landing page renders immediately, so there is no separate
    /// confirm step. A selection filtered out by the query re-enters the list
    /// from whichever end matches the key. While searching, the same keys
    /// step through the rendered sections and scroll each into view.
    fn cycle_settings_page(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.settings_search_query(cx);
        if !query.is_empty() {
            let pages = self.settings_search_sections.clone();
            let current = self
                .settings_search_target
                .and_then(|target| pages.iter().position(|page| *page == target));
            let Some(next) = next_picker_highlight(current, pages.len(), key) else {
                return;
            };
            self.scroll_to_settings_section(pages[next], cx);
            return;
        }
        let pages = visible_settings_pages(
            &query,
            self.state.computer_use_experiment_enabled,
            self.state.friends_enabled,
            self.jev_in_use(),
            self.state.integrations_enabled,
        )
        .map(|(page, ..)| page)
        .collect::<Vec<_>>();
        let current_page = self.settings_page.unwrap_or(SettingsPage::General);
        let current = pages.iter().position(|page| *page == current_page);
        let Some(next) = next_picker_highlight(current, pages.len(), key) else {
            return;
        };
        self.open_settings_page(pages[next], window, cx);
    }

    fn render_settings_sidebar_titlebar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let left_window_controls = self.render_client_window_controls(
            super::window_chrome::WindowControlSide::Left,
            window,
            cx,
        );
        // Only as tall as whatever actually sits in it: macOS's native
        // traffic lights, or the client-side buttons a Linux desktop puts on
        // this side. Windows keeps all three on the far side, and a desktop
        // like GNOME keeps none here, so there is nothing to clear and the
        // strip is only somewhere to drag the window by — the content
        // column's own titlebar carries the rest of that job. The 12px form
        // is too short for the back/forward pair, which drops to keyboard
        // and mouse-button hops alone on those platforms.
        let tall_titlebar = cfg!(target_os = "macos") || left_window_controls.is_some();
        let height = if tall_titlebar { 48.0 } else { 12.0 };

        // While the results column is up a page hop would be invisible, so
        // the buttons park along with `navigate_settings_history`.
        let searching = !self.settings_search_query(cx).is_empty();
        let back_enabled = !searching && self.settings_navigation.back_target().is_some();
        let forward_enabled = !searching && self.settings_navigation.forward_target().is_some();

        div()
            .id("settings-sidebar-titlebar")
            .h(px(height))
            .flex_none()
            .flex()
            .items_center()
            .children(left_window_controls)
            .child(
                self.window_drag_region(
                    div()
                        .id("settings-sidebar-traffic-light-drag-region")
                        .w(px(TRAFFIC_LIGHT_CLEARANCE))
                        .h_full()
                        .flex_none(),
                    cx,
                ),
            )
            .when(tall_titlebar, |titlebar| {
                titlebar.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .child(self.render_history_button(
                            "settings-navigate-back",
                            "icons/arrow-left.svg",
                            back_enabled,
                            true,
                            cx,
                        ))
                        .child(self.render_history_button(
                            "settings-navigate-forward",
                            "icons/arrow-right.svg",
                            forward_enabled,
                            false,
                            cx,
                        )),
                )
            })
            .child(
                self.render_settings_drag_region("settings-sidebar-titlebar-drag-region", cx)
                    .h(px(height))
                    .flex_1(),
            )
    }

    fn render_settings_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        // `settings_page` is trusted as written: every setter is either
        // `open_settings_page` (gated, so a persisted page whose opt-in is
        // off falls back to General) or `open_settings_page_direct`, which
        // deliberately renders a page the sidebar doesn't list.
        let query = self.settings_search_query(cx);
        if !query.is_empty() {
            return self.render_settings_search_results(&query, window, cx);
        }
        self.settings_search_sections.clear();
        self.settings_search_target = None;
        let page = self.settings_page.unwrap_or(SettingsPage::General);
        let search = SettingSearch::inactive().for_page(
            page,
            &self.settings_scroll,
            &self.settings_row_anchors,
        );
        let right_window_controls = self.render_client_window_controls(
            super::window_chrome::WindowControlSide::Right,
            window,
            cx,
        );
        // The Skills page is a mail-style split that owns the whole content
        // column — no page title, no titlebar strip, no width cap, no card.
        // Window dragging stays with the sidebar's own titlebar region.
        if page == SettingsPage::Skills {
            return div()
                .flex_1()
                .h_full()
                .min_w_0()
                .flex()
                .flex_col()
                .border_l(hairline())
                .border_color(theme.sidebar_border)
                .bg(theme.surface)
                .children(right_window_controls.map(|controls| {
                    self.render_settings_drag_region("settings-skills-titlebar", cx)
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(controls)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.render_skills_settings(cx)),
                );
        }
        // The Keybinding Manager is a self-contained surface like Skills:
        // fixed keyboard stage over its own virtualized table.
        if page == SettingsPage::Keybindings {
            return div()
                .flex_1()
                .h_full()
                .min_w_0()
                .flex()
                .flex_col()
                .border_l(hairline())
                .border_color(theme.sidebar_border)
                .bg(theme.surface)
                .children(right_window_controls.map(|controls| {
                    self.render_settings_drag_region("settings-keybindings-titlebar", cx)
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(controls)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.render_keybindings_page(window, cx)),
                );
        }
        // The Monthly and Projects list views own their own scrolling, so
        // their pages fill the viewport instead of riding the shared scroll
        // container; the Archived page's virtualized list needs the same.
        let fills_viewport = page == SettingsPage::Archived
            || page == SettingsPage::Git
            || page == SettingsPage::Diagnostics
            || (page == SettingsPage::Usage
                && matches!(
                    self.usage_view,
                    UsageViewMode::Monthly | UsageViewMode::Projects
                ));
        // The titlebar strip is transparent; once content slides under it, a
        // hairline marks the boundary so the clip edge reads as a header
        // rather than a glitch.
        let content_scrolled = !fills_viewport && self.settings_scroll.offset().y < px(-1.0);

        let inner = div()
            .w_full()
            .max_w(px(match page {
                SettingsPage::Usage => SETTINGS_USAGE_MAX_WIDTH,
                SettingsPage::Git => SETTINGS_GIT_MAX_WIDTH,
                _ => SETTINGS_CONTENT_MAX_WIDTH,
            }))
            .mx_auto()
            .when(fills_viewport, |element| {
                element.h_full().min_h_0().flex().flex_col()
            })
            .child(
                div()
                    .pt(px(2.0))
                    .pl(px(6.0))
                    .flex_none()
                    .text_size(sp(18.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(match page {
                        SettingsPage::General => tr!("settings.general"),
                        SettingsPage::Providers => tr!("settings.providers"),
                        SettingsPage::Skills => tr!("settings.skills"),
                        SettingsPage::Friends => tr!("settings.friends"),
                        SettingsPage::Archived => tr!("settings.archived"),
                        SettingsPage::Usage => tr!("settings.usage"),
                        SettingsPage::Daemon => tr!("settings.daemon"),
                        SettingsPage::ComputerUse => tr!("settings.computer_use"),
                        SettingsPage::Commands => tr!("settings.commands"),
                        SettingsPage::Terminal => tr!("settings.terminal"),
                        SettingsPage::Appearance => tr!("settings.appearance"),
                        SettingsPage::Git => tr!("settings.git"),
                        SettingsPage::Jev => tr!("settings.jev"),
                        SettingsPage::Experiments => tr!("settings.experiments"),
                        SettingsPage::Integrations => tr!("settings.integrations"),
                        SettingsPage::Keybindings => tr!("keybind.title"),
                        SettingsPage::Diagnostics => tr!("settings.diagnostics"),
                    }),
            )
            .child(match page {
                SettingsPage::General => self.render_general_settings(&search, cx),
                SettingsPage::Providers => self.render_providers_settings(&search, cx),
                SettingsPage::Skills => self.render_skills_settings(cx),
                SettingsPage::Friends => self.render_friends_settings(&search, cx),
                SettingsPage::Archived => self.render_archived_settings(cx),
                SettingsPage::Usage => self.render_usage_settings(cx),
                SettingsPage::Daemon => self.render_daemon_settings(&search, cx),
                SettingsPage::ComputerUse => self.render_computer_use_settings(&search, cx),
                SettingsPage::Commands => self.render_commands_settings(&search, cx),
                SettingsPage::Terminal => self.render_terminal_settings(&search, cx),
                SettingsPage::Appearance => self.render_appearance_settings(&search, cx),
                SettingsPage::Git => self.render_git_settings(window, cx),
                SettingsPage::Jev => self.render_jev_settings(&search, cx),
                SettingsPage::Experiments => self.render_experiments_settings(&search, cx),
                SettingsPage::Integrations => self.render_integrations_settings(cx),
                SettingsPage::Keybindings => div().into_any_element(),
                SettingsPage::Diagnostics => self.render_diagnostics_settings(cx),
            });

        let git_scrollbar = if page == SettingsPage::Git {
            self.render_git_settings_scrollbar()
        } else {
            None
        };

        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .border_l(hairline())
            .border_color(theme.sidebar_border)
            .bg(theme.surface)
            .child(
                self.render_settings_drag_region("settings-content-titlebar", cx)
                    .flex()
                    .items_center()
                    .justify_end()
                    .children(right_window_controls)
                    .when(content_scrolled, |element| {
                        element.border_b(hairline()).border_color(theme.separator)
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("settings-content-scroll")
                            .size_full()
                            .when(!fills_viewport, |element| {
                                element
                                    .overflow_y_scroll()
                                    .track_scroll(&self.settings_scroll)
                                    .pb(px(48.0))
                            })
                            .when(fills_viewport, |element| {
                                element.min_h_0().flex().flex_col()
                            })
                            .px(px(32.0))
                            .child(inner),
                    )
                    .when(!fills_viewport, |element| {
                        element.child(scrollbar::vertical(
                            &self.settings_scroll,
                            &self.settings_scrollbar,
                        ))
                    })
                    .children(git_scrollbar),
            )
    }

    /// The search results column: every section that has a matching setting
    /// renders its heading plus only the matched rows, in sidebar order.
    /// Each section is a direct child of the scroll element so the sidebar
    /// and arrow keys can `scroll_to_top_of_item` straight to it.
    fn render_settings_search_results(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let right_window_controls = self.render_client_window_controls(
            super::window_chrome::WindowControlSide::Right,
            window,
            cx,
        );
        let content_scrolled = self.settings_scroll.offset().y < px(-1.0);

        self.settings_search_sections.clear();
        let mut sections: Vec<Div> = Vec::new();
        for page in SEARCHABLE_SETTINGS_PAGES {
            if !page.is_visible_in_navigation(
                self.state.computer_use_experiment_enabled,
                self.state.friends_enabled,
                self.jev_in_use(),
                self.state.integrations_enabled,
            ) {
                continue;
            }
            let Some((_, label_key, _, _)) = SETTINGS_PAGES
                .into_iter()
                .find(|(candidate, ..)| *candidate == page)
            else {
                continue;
            };
            let label = crate::i18n::translate(label_key);
            // A query matching the section's own title keeps every row in
            // it — the match is on the heading, not on any one setting.
            let label_ranges = settings_match_ranges(&label, query);
            let search = if label_ranges.is_empty() {
                SettingSearch::new(query)
            } else {
                SettingSearch::forced(query)
            }
            .for_page(page, &self.settings_scroll, &self.settings_row_anchors)
            .visiting(cx.entity().downgrade());
            let content = match page {
                SettingsPage::General => self.render_general_settings(&search, cx),
                SettingsPage::Appearance => self.render_appearance_settings(&search, cx),
                SettingsPage::Providers => self.render_providers_settings(&search, cx),
                SettingsPage::Friends => self.render_friends_settings(&search, cx),
                SettingsPage::Commands => self.render_commands_settings(&search, cx),
                SettingsPage::Terminal => self.render_terminal_settings(&search, cx),
                SettingsPage::Daemon => self.render_daemon_settings(&search, cx),
                SettingsPage::ComputerUse => self.render_computer_use_settings(&search, cx),
                SettingsPage::Jev => self.render_jev_settings(&search, cx),
                SettingsPage::Experiments => self.render_experiments_settings(&search, cx),
                _ => continue,
            };
            if search.hits() == 0 {
                continue;
            }
            self.settings_search_sections.push(page);
            let first = sections.is_empty();
            sections.push(
                div()
                    .when(!first, |element| element.mt(px(28.0)))
                    .px(px(32.0))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(SETTINGS_CONTENT_MAX_WIDTH))
                            .mx_auto()
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "settings-section-{}",
                                        page as usize
                                    )))
                                    .tab_index(0)
                                    .pt(px(2.0))
                                    .pl(px(6.0))
                                    .flex_none()
                                    .cursor_pointer()
                                    .text_size(sp(18.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .hover(|element| element.text_color(theme.accent))
                                    .focus_visible(|element| element.bg(theme.focus_highlight()))
                                    .child(settings_search_text(label, label_ranges, theme))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.visit_setting(page, None, window, cx);
                                    }))
                                    .on_key_down(cx.listener(
                                        move |this, event: &KeyDownEvent, window, cx| {
                                            if !event.keystroke.modifiers.modified()
                                                && matches!(
                                                    event.keystroke.key.as_str(),
                                                    "enter" | "space"
                                                )
                                            {
                                                this.visit_setting(page, None, window, cx);
                                                cx.stop_propagation();
                                            }
                                        },
                                    )),
                            )
                            .child(content),
                    ),
            );
        }

        let scroll = div()
            .id("settings-content-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .pb(px(48.0))
            .children(sections);
        let scroll = if self.settings_search_sections.is_empty() {
            scroll.child(
                div()
                    .mt(px(48.0))
                    .w_full()
                    .flex()
                    .justify_center()
                    .text_size(sp(13.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("settings.search_empty")),
            )
        } else {
            scroll
        };

        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .border_l(hairline())
            .border_color(theme.sidebar_border)
            .bg(theme.surface)
            .child(
                self.render_settings_drag_region("settings-content-titlebar", cx)
                    .flex()
                    .items_center()
                    .justify_end()
                    .children(right_window_controls)
                    .when(content_scrolled, |element| {
                        element.border_b(hairline()).border_color(theme.separator)
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(scroll)
                    .child(scrollbar::vertical(
                        &self.settings_scroll,
                        &self.settings_scrollbar,
                    )),
            )
    }

    /// A single-choice `dropdown_menu` for a preference whose states are
    /// named modes — what a switch imitates when both sides of the bool
    /// deserve a label. `options` lists `(value, label)` in menu order;
    /// `set` receives the picked value.
    fn setting_selector<T>(
        &self,
        id: &'static str,
        options: Vec<(T, String)>,
        current: T,
        width: f32,
        cx: &mut Context<Self>,
        set: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + Copy + 'static,
    ) -> AnyElement
    where
        T: Copy + Eq + 'static,
    {
        let weak = cx.entity().downgrade();
        let handle = self.menu_handle(id, cx);
        let label = options
            .iter()
            .find(|(value, _)| *value == current)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        dropdown_menu(
            MenuChip::new(id)
                .label(label)
                .outlined()
                .selected(handle.is_open())
                .w(px(width))
                .justify_between(),
            ElementId::Name(SharedString::from(format!("{id}-menu"))),
            &handle,
            MenuAlign::BelowRight,
            move |_| {
                options
                    .iter()
                    .map(|(value, label)| {
                        let weak = weak.clone();
                        let value = *value;
                        MenuItem::new(label.clone(), move |window, cx| {
                            let _ = weak.update(cx, |this, cx| set(this, value, window, cx));
                        })
                        .selected(value == current)
                    })
                    .collect()
            },
        )
    }

    fn render_general_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let updater_available = cx
            .try_global::<crate::updater::UpdaterState>()
            .is_some_and(|updater| updater.0.is_some());
        let analytics_enabled = self.state.analytics_enabled;
        let analytics_toggle = toggle_switch(
            "anonymous-analytics-toggle",
            analytics_enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_analytics_enabled(!analytics_enabled, cx),
        );
        // Ungrouped head: what the app is, plus the app-level toggles that
        // belong to no feature area.
        let mut head_cards: Vec<AnyElement> = [
            setting_card(
                "icons/local.svg",
                tr!("settings.local_by_default"),
                tr!("settings.local_by_default_description"),
                div(),
                theme,
                search,
            ),
            setting_card(
                "icons/chart-column.svg",
                tr!("settings.share_anonymous_usage_data"),
                tr!("settings.share_anonymous_usage_data_description"),
                analytics_toggle,
                theme,
                search,
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        if updater_available {
            let enabled = self.automatic_updates_enabled;
            head_cards.extend(setting_card(
                "icons/rotate-cw.svg",
                tr!("settings.automatic_updates"),
                tr!("settings.automatic_updates_description"),
                toggle_switch(
                    "automatic-updates-toggle",
                    enabled,
                    false,
                    theme,
                    cx,
                    move |this, _, cx| this.set_automatic_updates_enabled(!enabled, cx),
                ),
                theme,
                search,
            ));
            let app_build = match option_env!("GODDARD_COMMIT_SHA") {
                Some(commit) => format!("{} · {commit}", env!("CARGO_PKG_VERSION")),
                None => env!("CARGO_PKG_VERSION").to_owned(),
            };
            head_cards.extend(setting_card(
                "icons/download.svg",
                tr!("settings.check_for_updates"),
                tr!(
                    "settings.check_for_updates_description",
                    version = app_build
                ),
                div()
                    .id("check-for-updates-now")
                    .tab_index(0)
                    .h(px(27.0))
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .border(hairline())
                    .border_color(theme.border_strong)
                    .flex()
                    .items_center()
                    .cursor_default()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .hover(|element| element.bg(theme.overlay))
                    .focus_visible(|element| element.bg(theme.focus_highlight()))
                    .child(tr!("settings.check_now"))
                    .on_activation(cx, |_, _, cx| {
                        if let Some(updater) = cx
                            .try_global::<crate::updater::UpdaterState>()
                            .and_then(|state| state.0.as_ref())
                        {
                            updater.check_for_updates();
                        }
                    }),
                theme,
                search,
            ));
            if cfg!(target_os = "macos") {
                let channel = self.state.update_channel;
                head_cards.extend(setting_card(
                    "icons/git-branch.svg",
                    tr!("settings.update_channel"),
                    tr!("settings.update_channel_description"),
                    self.setting_selector(
                        "update-channel-selector",
                        UpdateChannel::ALL
                            .into_iter()
                            .map(|option| (option, tr!(option.label_key())))
                            .collect(),
                        channel,
                        220.0,
                        cx,
                        |this, channel, _window, cx| this.set_update_channel(channel, cx),
                    ),
                    theme,
                    search,
                ));
            }
        }

        let mut session_cards: Vec<AnyElement> = [
            {
                let default_workspace = self.state.default_workspace;
                let weak = cx.entity().downgrade();
                let workspace_handle = self.menu_handle("default-workspace-selector", cx);
                let workspace_selector = dropdown_menu(
                    MenuChip::new("default-workspace-selector")
                        .label(tr!(default_workspace.label_key()))
                        .outlined()
                        .selected(workspace_handle.is_open())
                        .w(px(220.0))
                        .justify_between(),
                    "default-workspace-selector-menu",
                    &workspace_handle,
                    MenuAlign::BelowRight,
                    move |_| {
                        DefaultWorkspace::ALL
                            .into_iter()
                            .map(|option| {
                                let weak = weak.clone();
                                MenuItem::new(tr!(option.label_key()), move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_default_workspace(option, cx);
                                    });
                                })
                                .selected(option == default_workspace)
                            })
                            .collect()
                    },
                );
                setting_card(
                    "icons/folder.svg",
                    tr!("settings.default_workspace"),
                    tr!("settings.default_workspace_description"),
                    workspace_selector,
                    theme,
                    search,
                )
            },
            setting_card(
                "icons/appearance.svg",
                tr!("settings.local_workspace_accent"),
                tr!("settings.local_workspace_accent_description"),
                toggle_switch(
                    "local-workspace-accent-toggle",
                    self.state.local_workspace_accent,
                    false,
                    theme,
                    cx,
                    {
                        let enabled = self.state.local_workspace_accent;
                        move |this, _, cx| this.set_local_workspace_accent(!enabled, cx)
                    },
                ),
                theme,
                search,
            ),
            {
                let navigation = self.state.archive_navigation;
                let weak = cx.entity().downgrade();
                let navigation_handle = self.menu_handle("archive-navigation-selector", cx);
                let navigation_selector = dropdown_menu(
                    MenuChip::new("archive-navigation-selector")
                        .label(tr!(navigation.label_key()))
                        .outlined()
                        .selected(navigation_handle.is_open())
                        .w(px(220.0))
                        .justify_between(),
                    "archive-navigation-selector-menu",
                    &navigation_handle,
                    MenuAlign::BelowRight,
                    move |_| {
                        ArchiveNavigation::ALL
                            .into_iter()
                            .map(|option| {
                                let weak = weak.clone();
                                MenuItem::new(tr!(option.label_key()), move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_archive_navigation(option, cx);
                                    });
                                })
                                .selected(option == navigation)
                            })
                            .collect()
                    },
                );
                setting_card(
                    "icons/archive.svg",
                    tr!("settings.archive_navigation"),
                    tr!("settings.archive_navigation_description"),
                    navigation_selector,
                    theme,
                    search,
                )
            },
            setting_card(
                "icons/archive.svg",
                tr!("settings.archive_continues_unread_sweep"),
                tr!(
                    "settings.archive_continues_unread_sweep_description",
                    jump = crate::platform::primary_shortcut("⌘D", "Ctrl+D")
                ),
                toggle_switch(
                    "archive-continues-unread-sweep-toggle",
                    self.state.archive_continues_unread_sweep,
                    false,
                    theme,
                    cx,
                    {
                        let enabled = self.state.archive_continues_unread_sweep;
                        move |this, _, cx| this.set_archive_continues_unread_sweep(!enabled, cx)
                    },
                ),
                theme,
                search,
            ),
            {
                let dormant_after = self.state.dormant_after_days;
                let weak = cx.entity().downgrade();
                let dormant_handle = self.menu_handle("dormant-after-selector", cx);
                let dormant_selector = dropdown_menu(
                    MenuChip::new("dormant-after-selector")
                        .label(dormant_after_label(dormant_after))
                        .outlined()
                        .selected(dormant_handle.is_open())
                        .w(px(220.0))
                        .justify_between(),
                    "dormant-after-selector-menu",
                    &dormant_handle,
                    MenuAlign::BelowRight,
                    move |_| {
                        waku_client::persistence::DORMANT_AFTER_DAYS_OPTIONS
                            .into_iter()
                            .map(|option| {
                                let weak = weak.clone();
                                MenuItem::new(dormant_after_label(option), move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_dormant_after_days(option, cx);
                                    });
                                })
                                .selected(option == dormant_after)
                            })
                            .collect()
                    },
                );
                setting_card(
                    "icons/hourglass.svg",
                    tr!("settings.dormant_after"),
                    tr!("settings.dormant_after_description"),
                    dormant_selector,
                    theme,
                    search,
                )
            },
            setting_card(
                "icons/keyboard.svg",
                tr!("settings.sidebar_shortcut_tags"),
                tr!(
                    "settings.sidebar_shortcut_tags_description",
                    keys = crate::platform::primary_shortcut("⌘1–⌘9", "Ctrl+1–Ctrl+9"),
                    modifier = crate::platform::primary_shortcut("⌘", "Ctrl")
                ),
                toggle_switch(
                    "sidebar-shortcut-tags-toggle",
                    self.state.sidebar_shortcut_tags,
                    false,
                    theme,
                    cx,
                    {
                        let enabled = self.state.sidebar_shortcut_tags;
                        move |this, _, cx| this.set_sidebar_shortcut_tags(!enabled, cx)
                    },
                ),
                theme,
                search,
            ),
            setting_card(
                "icons/compose.svg",
                tr!("settings.sidebar_composer_drafts"),
                tr!("settings.sidebar_composer_drafts_description"),
                toggle_switch(
                    "sidebar-composer-drafts-toggle",
                    self.state.sidebar_composer_drafts,
                    false,
                    theme,
                    cx,
                    {
                        let enabled = self.state.sidebar_composer_drafts;
                        move |this, _, cx| this.set_sidebar_composer_drafts(!enabled, cx)
                    },
                ),
                theme,
                search,
            ),
            // The color row only exists while previews do — same gating as
            // the transparency amount and completion volume rows.
            self.state
                .sidebar_composer_drafts
                .then(|| {
                    setting_card(
                        "icons/appearance.svg",
                        tr!("settings.sidebar_draft_preview_color"),
                        tr!("settings.sidebar_draft_preview_color_description"),
                        self.setting_selector(
                            "sidebar-draft-preview-color-selector",
                            SidebarDraftPreviewColor::ALL
                                .into_iter()
                                .map(|option| (option, tr!(option.label_key())))
                                .collect(),
                            self.state.sidebar_draft_preview_color,
                            160.0,
                            cx,
                            |this, value, _, cx| this.set_sidebar_draft_preview_color(value, cx),
                        ),
                        theme,
                        search,
                    )
                })
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();
        if cfg!(target_os = "macos") {
            // The platform recognizer reads the trackpad's touch stream,
            // which macOS only hands over when no system gesture claims
            // three-finger horizontal swipes.
            let enabled = self.state.three_finger_swipe_navigation;
            session_cards.extend(setting_card(
                "icons/hand.svg",
                tr!("settings.three_finger_swipe_navigation"),
                tr!("settings.three_finger_swipe_navigation_description"),
                toggle_switch(
                    "three-finger-swipe-toggle",
                    enabled,
                    false,
                    theme,
                    cx,
                    move |this, window, cx| {
                        this.set_three_finger_swipe_navigation(!enabled, window, cx)
                    },
                ),
                theme,
                search,
            ));
        }

        let git_cards: Vec<AnyElement> = [
            setting_card(
                "icons/git-branch.svg",
                tr!("settings.new_worktree_base"),
                tr!("settings.new_worktree_base_description"),
                self.setting_selector(
                    "new-worktree-base-selector",
                    vec![
                        (true, tr!("settings.new_worktree_base_default")),
                        (false, tr!("settings.new_worktree_base_last_used")),
                    ],
                    self.state.new_worktree_default_branch,
                    160.0,
                    cx,
                    |this, value, _, cx| this.set_new_worktree_default_branch(value, cx),
                ),
                theme,
                search,
            ),
            {
                let title = tr!("settings.new_worktree_sync_default_branch");
                let description = tr!("settings.new_worktree_sync_default_branch_description");
                search.matched(&title, &description).map(|matched| {
                    div()
                        .w_full()
                        .min_h(px(60.0))
                        .px(px(20.0))
                        .py(px(12.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .flex()
                        .items_center()
                        .gap(px(24.0))
                        .child(settings_row_label(
                            "icons/rotate-cw.svg",
                            settings_row_text(title, description, matched, theme).child(
                                div().mt(px(9.0)).max_w(px(360.0)).child(
                                    TextField::new(
                                        "worktree-sync-branches-field",
                                        self.worktree_sync_branches_input.clone(),
                                    )
                                    .w_full(),
                                ),
                            ),
                            theme,
                        ))
                        .child(toggle_switch(
                            "new-worktree-sync-default-branch-toggle",
                            self.state.new_worktree_sync_default_branch,
                            false,
                            theme,
                            cx,
                            {
                                let enabled = self.state.new_worktree_sync_default_branch;
                                move |this, _, cx| {
                                    this.set_new_worktree_sync_default_branch(!enabled, cx)
                                }
                            },
                        ))
                        .into_any_element()
                })
            },
            setting_card(
                "icons/git-merge.svg",
                tr!("settings.sync_strategy"),
                tr!("settings.sync_strategy_description"),
                self.setting_selector(
                    "sync-strategy-selector",
                    vec![
                        (false, tr!("settings.sync_strategy_rebase")),
                        (true, tr!("settings.sync_strategy_merge")),
                    ],
                    self.state.sync_with_merge,
                    140.0,
                    cx,
                    |this, value, _, cx| this.set_sync_with_merge(value, cx),
                ),
                theme,
                search,
            ),
            setting_card(
                "icons/download.svg",
                tr!("settings.auto_fetch_remotes"),
                tr!("settings.auto_fetch_remotes_description"),
                toggle_switch(
                    "auto-fetch-remotes-toggle",
                    self.state.auto_fetch_remotes,
                    false,
                    theme,
                    cx,
                    {
                        let enabled = self.state.auto_fetch_remotes;
                        move |this, _, cx| this.set_auto_fetch_remotes(!enabled, cx)
                    },
                ),
                theme,
                search,
            ),
            setting_card(
                "icons/alert.svg",
                tr!("settings.sync_conflict_handling"),
                tr!("settings.sync_conflict_handling_description"),
                self.setting_selector(
                    "sync-conflict-handling-selector",
                    vec![
                        (false, tr!("settings.conflict_handling_dialog")),
                        (true, tr!("settings.conflict_handling_new_chat")),
                    ],
                    self.state.auto_resolve_in_chat,
                    200.0,
                    cx,
                    |this, value, _, cx| this.set_auto_resolve_in_chat(value, cx),
                ),
                theme,
                search,
            ),
            setting_card(
                "icons/octagon-alert.svg",
                tr!("settings.land_conflict_handling"),
                tr!("settings.land_conflict_handling_description"),
                self.setting_selector(
                    "land-conflict-handling-selector",
                    vec![
                        (false, tr!("settings.conflict_handling_dialog")),
                        (true, tr!("settings.conflict_handling_task_chat")),
                    ],
                    self.state.auto_resolve_land_conflicts,
                    200.0,
                    cx,
                    |this, value, _, cx| this.set_auto_resolve_land_conflicts(value, cx),
                ),
                theme,
                search,
            ),
            setting_card(
                "icons/git-commit-horizontal.svg",
                tr!("settings.land_commit_reminder"),
                tr!("settings.land_commit_reminder_description"),
                toggle_switch(
                    "land-commit-reminder-toggle",
                    self.state.auto_commit_reminder_on_land,
                    false,
                    theme,
                    cx,
                    {
                        let enabled = self.state.auto_commit_reminder_on_land;
                        move |this, _, cx| this.set_auto_commit_reminder_on_land(!enabled, cx)
                    },
                ),
                theme,
                search,
            ),
        ]
        .into_iter()
        .flatten()
        .collect();

        let notification_cards: Vec<AnyElement> = [
            settings_row_card(
                vec![
                    {
                        let enabled = self.state.notify_turn_finished;
                        settings_row(
                            "icons/bell.svg",
                            tr!("settings.finished_turn_notification"),
                            tr!("settings.finished_turn_notification_description"),
                            toggle_switch(
                                "finished-turn-notification-toggle",
                                enabled,
                                false,
                                theme,
                                cx,
                                move |this, _, cx| this.set_notify_turn_finished(!enabled, cx),
                            ),
                            theme,
                            search,
                        )
                    },
                    {
                        let enabled = self.state.notify_waiting_input;
                        settings_row(
                            "icons/inbox.svg",
                            tr!("settings.waiting_input_notification"),
                            tr!("settings.waiting_input_notification_description"),
                            toggle_switch(
                                "waiting-input-notification-toggle",
                                enabled,
                                false,
                                theme,
                                cx,
                                move |this, _, cx| this.set_notify_waiting_input(!enabled, cx),
                            ),
                            theme,
                            search,
                        )
                    },
                ],
                theme,
            )
            .map(|card| card.into_any_element()),
            {
                let enabled = self.state.completion_sound_enabled;
                let selected_sound = self.state.completion_sound;
                let volume = self.state.completion_sound_volume;
                let volume_shown = self.completion_volume_slider.shown(volume);
                let volume_slider = slider::slider(
                    "completion-volume-slider",
                    &self.completion_volume_slider,
                    crate::persistence::MAX_COMPLETION_SOUND_VOLUME,
                    volume,
                    cx,
                    |this, volume, _, cx| this.set_completion_sound_volume(volume, cx),
                );
                let weak = cx.entity().downgrade();
                let sound_handle = self.menu_handle("completion-sound-selector", cx);
                let sound_selector = dropdown_menu(
                    MenuChip::new("completion-sound-selector")
                        .label(selected_sound.label())
                        .outlined()
                        .selected(sound_handle.is_open())
                        .w(px(116.0))
                        .justify_between(),
                    "completion-sound-selector-menu",
                    &sound_handle,
                    MenuAlign::BelowRight,
                    move |_| {
                        CompletionSound::ALL
                            .into_iter()
                            .map(|sound| {
                                let weak = weak.clone();
                                MenuItem::new(sound.label(), move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_completion_sound(sound, cx);
                                    });
                                })
                                .selected(sound == selected_sound)
                                .on_highlight(move |_, _| {
                                    crate::platform::play_completion_sound(sound, volume);
                                })
                            })
                            .collect()
                    },
                );
                let toggle_row = settings_row(
                    "icons/bell.svg",
                    tr!("settings.completion_sound"),
                    tr!("settings.completion_sound_description"),
                    toggle_switch(
                        "completion-sound-toggle",
                        enabled,
                        false,
                        theme,
                        cx,
                        move |this, _, cx| this.set_completion_sound_enabled(!enabled, cx),
                    ),
                    theme,
                    search,
                );
                let sound_row = if !enabled {
                    None
                } else {
                    let title = tr!("settings.completion_sound_name");
                    search.matched(&title, "").map(|matched| {
                        div()
                            .w_full()
                            .min_h(px(52.0))
                            .px(px(20.0))
                            .py(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(settings_row_icon("icons/volume-2.svg", theme))
                            .child(settings_title_jump(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(settings_search_text(
                                        title,
                                        matched.title_ranges.clone(),
                                        theme,
                                    )),
                                &matched,
                                theme,
                            ))
                            .child(sound_selector)
                    })
                };
                let volume_row = if !enabled {
                    None
                } else {
                    let title = tr!("settings.completion_sound_volume");
                    search.matched(&title, "").map(|matched| {
                        div()
                            .w_full()
                            .min_h(px(52.0))
                            .px(px(20.0))
                            .py(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(settings_row_icon("icons/gauge.svg", theme))
                            .child(settings_title_jump(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(settings_search_text(
                                        title,
                                        matched.title_ranges.clone(),
                                        theme,
                                    )),
                                &matched,
                                theme,
                            ))
                            .child(volume_slider.w(px(140.0)).flex_none())
                            .child(
                                div()
                                    .w(px(32.0))
                                    .flex_none()
                                    .flex()
                                    .justify_end()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_secondary)
                                    .child(format!("{}%", (volume_shown * 100.0).round() as i32)),
                            )
                    })
                };
                let starred_row = if !enabled {
                    None
                } else {
                    let starred_sound = self.state.starred_completion_sound;
                    settings_row(
                        "icons/star.svg",
                        tr!("settings.completion_sound_starred"),
                        tr!("settings.completion_sound_starred_description"),
                        toggle_switch(
                            "completion-sound-starred-toggle",
                            starred_sound,
                            false,
                            theme,
                            cx,
                            move |this, _, cx| {
                                this.set_starred_completion_sound(!starred_sound, cx)
                            },
                        ),
                        theme,
                        search,
                    )
                };
                settings_row_card(
                    vec![
                        toggle_row,
                        sound_row.map(|row| row.into_any_element()),
                        volume_row.map(|row| row.into_any_element()),
                        starred_row,
                    ],
                    theme,
                )
                .map(|card| card.into_any_element())
            },
        ]
        .into_iter()
        .flatten()
        .collect();

        div()
            .mt(px(15.0))
            .flex()
            .flex_col()
            .gap(px(20.0))
            .children((!head_cards.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .children(head_cards)
                    .into_any_element()
            }))
            .children(
                [
                    settings_group(tr!("settings.group_sessions"), session_cards, theme),
                    settings_group(tr!("settings.group_git"), git_cards, theme),
                    settings_group(
                        tr!("settings.group_notifications"),
                        notification_cards,
                        theme,
                    ),
                ]
                .into_iter()
                .flatten(),
            )
            .into_any_element()
    }

    /// The Terminal page — the integrated terminal's preferences. The font
    /// size row duplicates the Appearance page's selector under different
    /// element ids: a search query can co-render both pages, and matching
    /// rows there need independent interactivity state.
    fn render_terminal_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);

        let selected_font_size = self.state.terminal_font_size();
        let weak = cx.entity().downgrade();
        let font_size_handle = self.menu_handle("terminal-settings-font-size-selector", cx);
        let font_size_selector = dropdown_menu(
            MenuChip::new("terminal-settings-font-size-selector")
                .label(font_size_label(selected_font_size))
                .outlined()
                .selected(font_size_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "terminal-settings-font-size-selector-menu",
            &font_size_handle,
            MenuAlign::BelowRight,
            move |_| {
                FONT_SIZES
                    .into_iter()
                    .map(|size| {
                        let weak = weak.clone();
                        MenuItem::new(font_size_label(size), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_terminal_font_size(size, cx);
                            });
                        })
                        .selected(size == selected_font_size)
                    })
                    .collect()
            },
        );

        let selected_modifier = self.state.terminal_link_modifier;
        let weak = cx.entity().downgrade();
        let modifier_handle = self.menu_handle("terminal-link-modifier-selector", cx);
        let modifier_selector = dropdown_menu(
            MenuChip::new("terminal-link-modifier-selector")
                .label(link_modifier_label(selected_modifier))
                .outlined()
                .selected(modifier_handle.is_open())
                .w(px(220.0))
                .justify_between(),
            "terminal-link-modifier-selector-menu",
            &modifier_handle,
            MenuAlign::BelowRight,
            move |_| {
                TerminalLinkModifier::ALL
                    .into_iter()
                    .map(|option| {
                        let weak = weak.clone();
                        MenuItem::new(link_modifier_label(option), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_terminal_link_modifier(option, cx);
                            });
                        })
                        .selected(option == selected_modifier)
                    })
                    .collect()
            },
        );

        div()
            .mt(px(15.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .children(
                [
                    setting_card(
                        "icons/terminal.svg",
                        tr!("settings.terminal_font_size"),
                        tr!("settings.terminal_font_size_description"),
                        font_size_selector,
                        theme,
                        search,
                    ),
                    setting_card(
                        "icons/external-link.svg",
                        tr!(
                            "settings.terminal_mouse_mode_click",
                            modifier = crate::platform::primary_shortcut("⌘", "Ctrl")
                        ),
                        tr!(
                            "settings.terminal_mouse_mode_click_description",
                            modifier = crate::platform::primary_shortcut("⌘", "Ctrl")
                        ),
                        self.setting_selector(
                            "terminal-mouse-mode-click-selector",
                            vec![
                                (true, tr!("settings.terminal_mouse_mode_click_links")),
                                (false, tr!("settings.terminal_mouse_mode_click_program")),
                            ],
                            self.state.terminal_open_links_in_mouse_mode,
                            200.0,
                            cx,
                            |this, value, _, cx| {
                                this.set_terminal_open_links_in_mouse_mode(value, cx)
                            },
                        ),
                        theme,
                        search,
                    ),
                    setting_card(
                        "icons/command.svg",
                        tr!("settings.terminal_link_modifier"),
                        tr!("settings.terminal_link_modifier_description"),
                        modifier_selector,
                        theme,
                        search,
                    ),
                    setting_card(
                        "icons/copy.svg",
                        tr!("settings.terminal_copy_on_select"),
                        tr!("settings.terminal_copy_on_select_description"),
                        toggle_switch(
                            "terminal-copy-on-select-toggle",
                            self.state.terminal_copy_on_select,
                            false,
                            theme,
                            cx,
                            {
                                let enabled = self.state.terminal_copy_on_select;
                                move |this, _, cx| this.set_terminal_copy_on_select(!enabled, cx)
                            },
                        ),
                        theme,
                        search,
                    ),
                ]
                .into_iter()
                .flatten(),
            )
            .into_any_element()
    }

    fn set_analytics_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.analytics_enabled = enabled;
        self.analytics.set_enabled(enabled);
        self.save();
        cx.notify();
    }

    fn set_completion_sound_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.completion_sound_enabled == enabled {
            return;
        }
        if !enabled {
            self.completion_volume_slider.cancel();
        }
        self.state.completion_sound_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_notify_turn_finished(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.notify_turn_finished == enabled {
            return;
        }
        self.state.notify_turn_finished = enabled;
        self.save();
        cx.notify();
    }

    fn set_notify_waiting_input(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.notify_waiting_input == enabled {
            return;
        }
        self.state.notify_waiting_input = enabled;
        self.save();
        cx.notify();
    }

    fn set_terminal_open_links_in_mouse_mode(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.terminal_open_links_in_mouse_mode == enabled {
            return;
        }
        self.state.terminal_open_links_in_mouse_mode = enabled;
        crate::terminal::install_open_links_in_mouse_mode(enabled, cx);
        self.save();
        cx.notify();
    }

    fn set_terminal_link_modifier(
        &mut self,
        modifier: TerminalLinkModifier,
        cx: &mut Context<Self>,
    ) {
        if self.state.terminal_link_modifier == modifier {
            return;
        }
        self.state.terminal_link_modifier = modifier;
        crate::terminal::install_link_modifier(modifier, cx);
        self.save();
        cx.notify();
    }

    fn set_terminal_copy_on_select(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.terminal_copy_on_select == enabled {
            return;
        }
        self.state.terminal_copy_on_select = enabled;
        crate::terminal::install_copy_on_select(enabled, cx);
        self.save();
        cx.notify();
    }

    fn set_completion_sound(&mut self, sound: CompletionSound, cx: &mut Context<Self>) {
        if self.state.completion_sound != sound {
            self.state.completion_sound = sound;
            self.save();
        }
        // Picking from the menu previews the sound.
        crate::platform::play_completion_sound(sound, self.state.completion_sound_volume);
        cx.notify();
    }

    fn set_starred_completion_sound(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.starred_completion_sound == enabled {
            return;
        }
        self.state.starred_completion_sound = enabled;
        self.save();
        cx.notify();
    }

    fn set_completion_sound_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = crate::persistence::sanitized_completion_sound_volume(volume);
        if self.state.completion_sound_volume == volume {
            return;
        }
        self.state.completion_sound_volume = volume;
        self.save();
        // Committing a new level previews the current sound at it.
        crate::platform::play_completion_sound(self.state.completion_sound, volume);
        cx.notify();
    }

    fn set_automatic_updates_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.automatic_updates_enabled = enabled;
        if let Some(updater) = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|updater| updater.0.as_ref())
        {
            updater.set_automatically_checks_for_updates(enabled);
        }
        cx.notify();
    }

    fn set_update_channel(&mut self, channel: UpdateChannel, cx: &mut Context<Self>) {
        if self.state.update_channel == channel {
            return;
        }
        self.state.update_channel = channel;
        self.save();
        if let Some(updater) = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|updater| updater.0.as_ref())
        {
            updater.set_update_channel(channel);
            // The new feed should answer now rather than at the next
            // scheduled check — silent, like the launch-time one.
            if updater.automatically_checks_for_updates() {
                updater.check_for_updates_in_background();
            }
        }
        cx.notify();
    }

    /// The Commands page's open editor form. Input entities live here rather
    /// than in `Waku::new` because they only exist while the form is open.
    pub(super) fn open_custom_command_editor(
        &mut self,
        command: Option<&CustomCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("commands.name"))
                .placeholder(tr!("commands.name_placeholder"));
            if let Some(name) = command.and_then(|command| command.name.as_deref()) {
                input.set_content(name, cx);
            }
            input
        });
        let shell = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("commands.shell"))
                .placeholder(tr!("commands.shell_placeholder"));
            if let Some(shell) = command.and_then(|command| command.shell.as_deref()) {
                input.set_content(shell, cx);
            }
            input
        });
        let script = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .multi_line()
                .auto_height()
                .max_lines(10)
                .syntax(Some("shell"))
                .accessibility_label(tr!("commands.script"))
                .placeholder(tr!("commands.script_placeholder"));
            if let Some(command) = command {
                input.set_content(command.script.clone(), cx);
            }
            input
        });
        self.custom_command_editor = Some(CustomCommandEditor {
            id: command.map(|command| command.id),
            name: name.clone(),
            icon: command.map(|command| command.icon).unwrap_or_default(),
            shell,
            script,
            close_on_success: command.is_some_and(|command| command.close_on_success),
            previous_script: command.map(|command| command.script.clone()),
            script_required: false,
            exit_settings_on_save: false,
        });
        let focus = name.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn save_custom_command_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.custom_command_editor else {
            return;
        };
        let script = editor.script.read(cx).content().trim_end().to_owned();
        if script.trim().is_empty() {
            self.custom_command_editor.as_mut().unwrap().script_required = true;
            cx.notify();
            return;
        }
        let name = editor.name.read(cx).content().trim().to_owned();
        let shell = editor.shell.read(cx).content().trim().to_owned();
        let previous_script = editor.previous_script.clone();
        let exit_settings_on_save = editor.exit_settings_on_save;
        let command = CustomCommand {
            id: editor.id.unwrap_or_else(Uuid::new_v4),
            name: (!name.is_empty()).then_some(name),
            icon: editor.icon,
            shell: (!shell.is_empty()).then_some(shell),
            script,
            close_on_success: editor.close_on_success,
            // A human editing an agent's command keeps its attribution.
            created_by_task: editor
                .id
                .and_then(|id| {
                    self.state
                        .custom_commands
                        .iter()
                        .find(|existing| existing.id == id)
                })
                .and_then(|existing| existing.created_by_task),
        };
        if let Some(index) = self
            .state
            .custom_commands
            .iter()
            .position(|existing| existing.id == command.id)
        {
            self.state.custom_commands[index] = command;
        } else {
            self.state.custom_commands.push(command);
        }
        self.custom_command_editor = None;
        // The old text's hashed script file is only safe to drop once the
        // list no longer carries it.
        if let Some(previous_script) = previous_script {
            crate::custom_commands::remove_script_if_unreferenced(
                &previous_script,
                &self.state.custom_commands,
            );
        }
        self.save();
        if exit_settings_on_save {
            self.settings_page = None;
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn delete_custom_command(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self
            .state
            .custom_commands
            .iter()
            .position(|command| command.id == id)
        else {
            return;
        };
        let removed = self.state.custom_commands.remove(index);
        if self
            .custom_command_editor
            .as_ref()
            .is_some_and(|editor| editor.id == Some(id))
        {
            self.custom_command_editor = None;
        }
        crate::custom_commands::remove_script_if_unreferenced(
            &removed.script,
            &self.state.custom_commands,
        );
        self.save();
        cx.notify();
    }

    pub(super) fn open_remote_host_editor(
        &mut self,
        host: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let record = host.and_then(|id| {
            self.state
                .remote_hosts
                .iter()
                .find(|record| record.id == id)
        });
        let name = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_name"))
                .placeholder(tr!("daemon.remote_host_name_placeholder"));
            if let Some(name) = record.map(|record| record.name.as_str()) {
                input.set_content(name, cx);
            }
            input
        });
        let address = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_address"))
                .placeholder(tr!("daemon.remote_host_address_placeholder"));
            if let Some(address) = record.map(|record| record.address.as_str()) {
                input.set_content(address, cx);
            }
            input
        });
        let token = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .masked()
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_token"))
                .placeholder(tr!("daemon.remote_host_token_placeholder"));
            if let Some(token) = record.map(|record| record.token.as_str()) {
                input.set_content(token, cx);
            }
            input
        });
        let destination = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_ssh"))
                .placeholder(tr!("daemon.remote_host_ssh_placeholder"));
            if let Some(ssh) = record.and_then(|record| record.ssh_destination.as_deref()) {
                input.set_content(ssh, cx);
            }
            input
        });
        self.remote_host_editor = Some(RemoteHostEditor {
            id: host,
            // SSH is the self-provisioning default on unix; a record with no
            // destination is a direct host. Other platforms only do direct.
            transport: if cfg!(unix) {
                match record {
                    Some(record) if record.ssh_destination.is_none() => RemoteHostTransport::Direct,
                    _ => RemoteHostTransport::Ssh,
                }
            } else {
                RemoteHostTransport::Direct
            },
            name: name.clone(),
            address,
            token,
            destination,
            token_revealed: false,
            ssh_hosts: Vec::new(),
            missing_fields: false,
        });
        #[cfg(unix)]
        cx.spawn(async move |waku, cx| {
            let hosts = cx
                .background_executor()
                .spawn(async move { crate::ssh::ssh_config_hosts() })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if let Some(editor) = &mut waku.remote_host_editor {
                    editor.ssh_hosts = hosts;
                    cx.notify();
                }
            });
        })
        .detach();
        let focus = name.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Flip the remote-host token field between bullets and plaintext —
    /// it starts masked, like every other secret field.
    fn toggle_remote_host_token(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.remote_host_editor {
            editor.token_revealed = !editor.token_revealed;
            let masked = !editor.token_revealed;
            editor.token.update(cx, |input, _| input.set_masked(masked));
            cx.notify();
        }
    }

    fn save_remote_host_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.remote_host_editor else {
            return;
        };
        let name = editor.name.read(cx).content().trim().to_owned();
        // Only the active transport's fields apply; the picker hid the rest.
        let (address, token, destination) = match editor.transport {
            RemoteHostTransport::Ssh => (
                String::new(),
                String::new(),
                editor.destination.read(cx).content().trim().to_owned(),
            ),
            RemoteHostTransport::Direct => (
                editor.address.read(cx).content().trim().to_owned(),
                editor.token.read(cx).content().trim().to_owned(),
                String::new(),
            ),
        };
        // An SSH destination is self-contained: the daemon and its token are
        // provisioned on the remote. A direct connection needs both fields.
        if destination.is_empty() && (address.is_empty() || token.is_empty()) {
            self.remote_host_editor.as_mut().unwrap().missing_fields = true;
            cx.notify();
            return;
        }
        let name = if name.is_empty() {
            if destination.is_empty() {
                address.clone()
            } else {
                destination.clone()
            }
        } else {
            name
        };
        let destination = (!destination.is_empty()).then_some(destination);
        if let Some(id) = editor.id {
            self.update_remote_host(id, name, address, token, destination, cx);
        } else {
            self.add_remote_host(name, address, token, destination, cx);
        }
        self.remote_host_editor = None;
        cx.notify();
    }

    /// Bind the LAN browser the first time the Daemon page opens. It then
    /// runs for the rest of the session — cheap (one dormant iroh endpoint)
    /// and it keeps the nearby rows' Found/Gone history warm.
    pub(super) fn ensure_daemon_discovery(&mut self) {
        if self.daemon_discovery.is_none() {
            self.daemon_discovery = Some(Arc::new(waku_client::DaemonDiscovery::start()));
        }
    }

    /// Send an encrypted pair request over the `waku-link` ALPN. On grant
    /// the returned token becomes a normal remote host record — the other
    /// machine's user already approved, so no editor round-trip is needed.
    fn pair_with_daemon(
        &mut self,
        daemon: waku_client::discover::DiscoveredDaemon,
        cx: &mut Context<Self>,
    ) {
        let Some(discovery) = self.daemon_discovery.clone() else {
            return;
        };
        let endpoint_id = daemon.endpoint_id.to_string();
        if !self.pair_requests_in_flight.insert(endpoint_id.clone()) {
            return;
        }
        let name = daemon.info.name.clone();
        let address = daemon.ws_url.clone();
        let device_name = self.daemon_hostname.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { discovery.request_pair(&daemon, &device_name) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.pair_requests_in_flight.remove(&endpoint_id);
                match outcome {
                    Ok(waku_client::discover::PairOutcome::Granted { token }) => {
                        if let Some(address) = address {
                            this.analytics
                                .track(crate::analytics::Event::PairingFinished {
                                    outcome: "granted",
                                });
                            this.add_remote_host(name.clone(), address, token, None, cx);
                            this.show_success_toast(tr!("daemon.pair_added", name = name));
                        } else {
                            this.analytics
                                .track(crate::analytics::Event::PairingFinished {
                                    outcome: "no_address",
                                });
                            this.show_toast(tr!("daemon.pair_no_address", name = name));
                        }
                    }
                    Ok(waku_client::discover::PairOutcome::Declined) => {
                        this.analytics
                            .track(crate::analytics::Event::PairingFinished {
                                outcome: "declined",
                            });
                        this.show_toast(tr!("daemon.pair_declined", name = name));
                    }
                    Err(error) => {
                        this.analytics
                            .track(crate::analytics::Event::PairingFinished { outcome: "failed" });
                        this.show_toast(tr!("daemon.pair_failed", error = format!("{error:#}")));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_commands_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let mut column = div().mt(px(15.0)).w_full().flex().flex_col().gap(px(12.0));
        let header = {
            let title = tr!("commands.title");
            let description = tr!("commands.description");
            search.matched(&title, &description).map(|matched| {
                div()
                    .w_full()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .child(settings_row_label(
                        "icons/slash.svg",
                        settings_row_text(title, description, matched, theme),
                        theme,
                    ))
                    .into_any_element()
            })
        };
        column = column.children(header);

        if !search.active() {
            if let Some(editor) = &self.custom_command_editor {
                return column
                    .child(self.render_custom_command_editor(editor, theme, cx))
                    .into_any_element();
            }

            column = column.child(
                div()
                    .id("new-custom-command")
                    .tab_index(0)
                    .w_full()
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_default()
                    .text_size(sp(13.0))
                    .text_color(theme.text_secondary)
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(icon("icons/plus.svg", 14.0, theme.text_tertiary))
                    .child(tr!("commands.new_command"))
                    .on_activation(cx, |this, window, cx| {
                        this.open_custom_command_editor(None, window, cx);
                    }),
            );

            if self.state.custom_commands.is_empty() {
                column = column.child(
                    div()
                        .w_full()
                        .px(px(20.0))
                        .py(px(24.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(sp(13.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("commands.empty")),
                        )
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(tr!("commands.empty_description")),
                        ),
                );
            }
        }

        for command in &self.state.custom_commands {
            let id = command.id;
            let label = command.display_name().to_owned();
            let script = command.script.clone();
            let edit_command = command.clone();
            let agent_added = command.created_by_task.is_some();
            let Some(matched) = search.matched(&label, "") else {
                continue;
            };
            column = column.child(
                div()
                    .w_full()
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(icon(
                        crate::custom_commands::icon_path(command.icon),
                        15.0,
                        theme.text_tertiary,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(settings_title_jump(
                                        div()
                                            .truncate()
                                            .text_size(sp(13.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(settings_search_text(
                                                label,
                                                matched.title_ranges.clone(),
                                                theme,
                                            )),
                                        &matched,
                                        theme,
                                    ))
                                    .when(agent_added, |row| {
                                        row.child(
                                            div()
                                                .flex_none()
                                                .px(px(6.0))
                                                .py(px(1.0))
                                                .rounded(px(5.0))
                                                .bg(theme.overlay)
                                                .text_size(sp(10.5))
                                                .text_color(theme.text_secondary)
                                                .child(tr!("commands.agent_badge")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .truncate()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(script),
                            ),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!("edit-custom-command-{id}")),
                            "icons/pencil.svg",
                            theme,
                        )
                        .tab_index(0)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .tooltip(|window, cx| Tooltip::new(tr!("commands.edit")).build(window, cx))
                        .on_activation(cx, move |this, window, cx| {
                            this.open_custom_command_editor(Some(&edit_command), window, cx);
                        }),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!("delete-custom-command-{id}")),
                            "icons/trash.svg",
                            theme,
                        )
                        .tab_index(0)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .tooltip(|window, cx| {
                            Tooltip::new(tr!("commands.delete")).build(window, cx)
                        })
                        .on_activation(cx, move |this, _, cx| {
                            this.delete_custom_command(id, cx);
                        }),
                    ),
            );
        }

        column.into_any_element()
    }

    fn render_custom_command_editor(
        &self,
        editor: &CustomCommandEditor,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let editing = editor.id.is_some();
        let close_on_success = editor.close_on_success;
        let field_label = |text: String| {
            div()
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(text)
        };
        let field_hint = |text: String| {
            div()
                .mt(px(3.0))
                .text_size(sp(12.0))
                .line_height(sp(15.0))
                .text_color(theme.text_tertiary)
                .child(text)
        };
        let ghost_button = |id: &'static str, label: String| {
            div()
                .id(id)
                .tab_index(0)
                .h(px(27.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .child(label)
        };
        let selected_icon = editor.icon;
        let weak = cx.entity().downgrade();
        let icon_handle = self.menu_handle("custom-command-icon-selector", cx);
        let icon_selector = dropdown_menu(
            MenuChip::new("custom-command-icon-selector")
                .label(selected_icon.label())
                .icon(
                    crate::custom_commands::icon_path(selected_icon),
                    theme.text_secondary,
                )
                .outlined()
                .selected(icon_handle.is_open())
                .w(px(150.0))
                .justify_between(),
            "custom-command-icon-selector-menu",
            &icon_handle,
            MenuAlign::BelowRight,
            move |_| {
                CustomCommandIcon::ALL
                    .into_iter()
                    .map(|icon| {
                        let weak = weak.clone();
                        MenuItem::new(icon.label(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(editor) = this.custom_command_editor.as_mut() {
                                    editor.icon = icon;
                                }
                                cx.notify();
                            });
                        })
                        .icon(crate::custom_commands::icon_path(icon))
                        .selected(icon == selected_icon)
                    })
                    .collect()
            },
        );

        div()
            .w_full()
            .px(px(20.0))
            .py(px(15.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(sp(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(if editing {
                        tr!("commands.edit_command")
                    } else {
                        tr!("commands.new_command")
                    }),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .child(field_label(tr!("commands.name")))
                    .child(field_hint(tr!("commands.name_description")))
                    .child(div().mt(px(6.0)).child(
                        TextField::new("custom-command-name", editor.name.clone()).w_full(),
                    )),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field_label(tr!("commands.icon")))
                            .child(field_hint(tr!("commands.icon_description"))),
                    )
                    .child(icon_selector),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .child(field_label(tr!("commands.shell")))
                    .child(field_hint(tr!("commands.shell_description")))
                    .child(div().mt(px(6.0)).child(
                        TextField::new("custom-command-shell", editor.shell.clone()).w_full(),
                    )),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .child(field_label(tr!("commands.script")))
                    .child(field_hint(tr!("commands.script_description")))
                    .child(
                        div()
                            .mt(px(6.0))
                            .w_full()
                            .px(px(8.0))
                            .py(px(6.0))
                            .rounded(px(8.0))
                            .border(hairline())
                            .border_color(theme.border_strong)
                            .bg(theme.inset)
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .child(editor.script.clone()),
                    )
                    .when(editor.script_required, |element| {
                        element.child(
                            div()
                                .mt(px(4.0))
                                .text_size(sp(12.0))
                                .text_color(theme.danger)
                                .child(tr!("commands.script_required")),
                        )
                    }),
            )
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field_label(tr!("commands.close_on_success")))
                            .child(field_hint(tr!("commands.close_on_success_description"))),
                    )
                    .child(toggle_switch(
                        "custom-command-close-on-success",
                        close_on_success,
                        false,
                        theme,
                        cx,
                        move |this, _, cx| {
                            if let Some(editor) = this.custom_command_editor.as_mut() {
                                editor.close_on_success = !editor.close_on_success;
                            }
                            cx.notify();
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        ghost_button("custom-command-cancel", tr!("commands.cancel"))
                            .on_activation(cx, |this, _, cx| {
                                this.custom_command_editor = None;
                                cx.notify();
                            }),
                    )
                    .child(
                        ghost_button("custom-command-save", tr!("commands.save"))
                            .on_activation(cx, |this, window, cx| {
                                this.save_custom_command_editor(window, cx)
                            }),
                    ),
            )
    }

    fn render_daemon_settings(&self, search: &SettingSearch, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let agent_tools_card = self.agent_tools_card(theme, search, cx);
        let keep_awake_card = self.keep_awake_card(theme, search, cx);
        let qa_branch_card = self.qa_branch_card(theme, search);
        let agent_settings_card = self.agent_settings_card(theme, search, cx);
        let sandbox_default_card = self.sandbox_default_card(theme, search, cx);
        let remote_hosts_card = self.render_remote_hosts_card(theme, search, cx);
        let build_card = self.render_build_card(theme, search, cx);
        if self.daemon.is_externally_managed() {
            let external_card = {
                let title = tr!("daemon.external_title");
                let description = tr!("daemon.external_description");
                search.matched(&title, &description).map(|matched| {
                    div()
                        .px(px(20.0))
                        .py(px(16.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .flex()
                        .items_center()
                        .child(settings_row_label(
                            "icons/unplug.svg",
                            settings_row_text(title, description, matched, theme),
                            theme,
                        ))
                        .into_any_element()
                })
            };
            return div()
                .mt(px(15.0))
                .w_full()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .children(remote_hosts_card)
                .children(external_card)
                .children(keep_awake_card)
                .children(qa_branch_card)
                .children(agent_tools_card)
                .children(agent_settings_card)
                .children(sandbox_default_card)
                .children(build_card)
                .into_any_element();
        }

        let enabled = self.state.daemon_exposure.enabled;
        let pending = self.daemon_reconfigure_pending;
        let fields_dirty = self.daemon_exposure_fields_dirty(cx);
        let port = self.state.daemon_exposure.port;
        let websocket_url = format!("ws://{}:{port}", self.daemon_hostname);
        let token = self.state.daemon_exposure.token.clone();

        let exposure_toggle = toggle_switch(
            "daemon-exposure-toggle",
            enabled,
            pending,
            theme,
            cx,
            move |this, _, cx| this.set_daemon_exposure_enabled(!enabled, cx),
        );

        let apply_disabled = pending || !fields_dirty;
        let apply_button = settings_button(
            "apply-daemon-settings",
            if pending {
                tr!("daemon.restarting")
            } else {
                tr!("daemon.apply")
            },
            !apply_disabled,
            false,
            false,
            theme,
            cx,
            |this, _, cx| this.apply_daemon_exposure_fields(cx),
        );

        let copy_url_feedback_id = "daemon-url";
        let url_copied = self.control_was_copied(copy_url_feedback_id);
        let copy_url = websocket_url.clone();
        let copy_url_button = div()
            .id("copy-daemon-url")
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .child(icon(
                if url_copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                theme.text_tertiary,
            ))
            .child(if url_copied {
                tr!("common.copied")
            } else {
                tr!("common.copy")
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                this.show_control_copied(copy_url_feedback_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(websocket_url.clone()));
                    this.show_control_copied(copy_url_feedback_id, cx);
                    cx.stop_propagation();
                }
            }));

        let copy_token_feedback_id = "daemon-token";
        let token_copied = self.control_was_copied(copy_token_feedback_id);
        let click_token = token.clone();
        let key_token = token.clone();
        let token_revealed = self.daemon_token_revealed;
        let reveal_token_button = div()
            .id("reveal-daemon-token")
            .tab_index(0)
            .size(px(27.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon(
                if token_revealed {
                    "icons/eye-off.svg"
                } else {
                    "icons/eye.svg"
                },
                12.0,
                theme.text_tertiary,
            ))
            .tooltip(Tooltip::text(if token_revealed {
                tr!("daemon.hide_token")
            } else {
                tr!("daemon.reveal_token")
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.daemon_token_revealed = !this.daemon_token_revealed;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.daemon_token_revealed = !this.daemon_token_revealed;
                    cx.stop_propagation();
                    cx.notify();
                }
            }));
        let copy_token_button = div()
            .id("copy-daemon-token")
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .child(icon(
                if token_copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                theme.text_tertiary,
            ))
            .child(if token_copied {
                tr!("common.copied")
            } else {
                tr!("common.copy")
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(click_token.clone()));
                this.show_control_copied(copy_token_feedback_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(key_token.clone()));
                    this.show_control_copied(copy_token_feedback_id, cx);
                    cx.stop_propagation();
                }
            }));

        let regenerate_button = div()
            .id("regenerate-daemon-token")
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .opacity(if pending { 0.55 } else { 1.0 })
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .when(!pending, |element| {
                element
                    .hover(|element| element.bg(theme.overlay))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.regenerate_daemon_token(cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            this.regenerate_daemon_token(cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(tr!("daemon.regenerate_token"));

        let qr_shown = self.daemon_qr.is_some();
        let qr_toggle = div()
            .id("toggle-daemon-qr")
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(if qr_shown {
                tr!("daemon.hide_qr")
            } else {
                tr!("daemon.show_qr")
            })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_daemon_qr(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.toggle_daemon_qr(cx);
                    cx.stop_propagation();
                }
            }));

        let expose_card = {
            let title = tr!("daemon.expose_title");
            let description = tr!("daemon.expose_description");
            search.matched(&title, &description).map(|matched| {
                div()
                    .min_h(px(66.0))
                    .px(px(20.0))
                    .py(px(13.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(settings_row_label(
                        "icons/wifi.svg",
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.0))
                                    .child(settings_title_jump(
                                        div()
                                            .text_size(sp(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(settings_search_text(
                                                title,
                                                matched.title_ranges.clone(),
                                                theme,
                                            )),
                                        &matched,
                                        theme,
                                    ))
                                    .child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(2.0))
                                            .rounded_full()
                                            .text_size(sp(12.5))
                                            .text_color(if enabled {
                                                theme.success
                                            } else {
                                                theme.text_tertiary
                                            })
                                            .bg(theme.overlay)
                                            .child(if pending {
                                                tr!("daemon.status_restarting")
                                            } else if enabled {
                                                tr!("daemon.status_exposed")
                                            } else {
                                                tr!("daemon.status_local")
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(settings_search_text(
                                        description,
                                        matched.description_ranges.clone(),
                                        theme,
                                    )),
                            ),
                        theme,
                    ))
                    .child(exposure_toggle)
                    .into_any_element()
            })
        };

        let connection_card = if !enabled {
            None
        } else {
            let before = search.hits();
            let header = {
                let title = tr!("daemon.connection_title");
                let description = tr!("daemon.connection_description");
                search.matched(&title, &description).map(|matched| {
                    settings_row_label(
                        "icons/link.svg",
                        div()
                            .child(settings_title_jump(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(settings_search_text(
                                        title,
                                        matched.title_ranges.clone(),
                                        theme,
                                    )),
                                &matched,
                                theme,
                            ))
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(sp(12.5))
                                    .line_height(sp(16.0))
                                    .text_color(theme.text_secondary)
                                    .child(settings_search_text(
                                        description,
                                        matched.description_ranges.clone(),
                                        theme,
                                    )),
                            ),
                        theme,
                    )
                })
            };
            let field_row = |icon_path: &'static str,
                             title: String,
                             description: String,
                             field: TextField|
             -> Option<Div> {
                search.matched(&title, &description).map(|matched| {
                    div()
                        .mt(px(14.0))
                        .flex()
                        .items_center()
                        .gap(px(24.0))
                        .child(settings_row_icon(icon_path, theme))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(settings_title_jump(
                                    div()
                                        .text_size(sp(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child(settings_search_text(
                                            title,
                                            matched.title_ranges.clone(),
                                            theme,
                                        )),
                                    &matched,
                                    theme,
                                ))
                                .child(
                                    div()
                                        .mt(px(3.0))
                                        .whitespace_normal()
                                        .text_size(sp(12.5))
                                        .line_height(sp(14.0))
                                        .text_color(theme.text_tertiary)
                                        .child(settings_search_text(
                                            description,
                                            matched.description_ranges.clone(),
                                            theme,
                                        )),
                                ),
                        )
                        .child(div().flex_1().min_w_0().flex().justify_end().child(field))
                })
            };
            let port_row = field_row(
                "icons/server.svg",
                tr!("daemon.port"),
                tr!("daemon.port_description"),
                TextField::new("daemon-port-field", self.daemon_port_input.clone()).w(px(150.0)),
            );
            let origins_row = field_row(
                "icons/globe.svg",
                tr!("daemon.allowed_origins"),
                tr!("daemon.allowed_origins_description"),
                TextField::new("daemon-origins-field", self.daemon_origins_input.clone())
                    .w_full()
                    .max_w(px(360.0)),
            );
            if search.active() && search.hits() == before {
                None
            } else {
                Some(
                    div()
                        .px(px(20.0))
                        .py(px(15.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .children(header)
                        .children(port_row)
                        .children(origins_row)
                        .when(!search.active(), |card| {
                            card.child(div().mt(px(13.0)).flex().justify_end().child(apply_button))
                        })
                        .into_any_element(),
                )
            }
        };

        let credentials_card = if !enabled {
            None
        } else {
            let before = search.hits();
            let header = {
                let title = tr!("daemon.credentials_title");
                let description = tr!("daemon.credentials_description");
                search.matched(&title, &description).map(|matched| {
                    settings_row_label(
                        "icons/key-round.svg",
                        div()
                            .child(settings_title_jump(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(settings_search_text(
                                        title,
                                        matched.title_ranges.clone(),
                                        theme,
                                    )),
                                &matched,
                                theme,
                            ))
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(sp(12.5))
                                    .line_height(sp(16.0))
                                    .text_color(theme.text_secondary)
                                    .child(settings_search_text(
                                        description,
                                        matched.description_ranges.clone(),
                                        theme,
                                    )),
                            ),
                        theme,
                    )
                })
            };
            let url_row = {
                let title = tr!("daemon.websocket_url");
                search.matched(&title, "").map(|matched| {
                    div()
                        .mt(px(13.0))
                        .py(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(settings_row_icon("icons/link.svg", theme))
                        .child(settings_title_jump(
                            div()
                                .w(px(80.0))
                                .flex_none()
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(settings_search_text(
                                    title,
                                    matched.title_ranges.clone(),
                                    theme,
                                )),
                            &matched,
                            theme,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(crate::fonts::current(cx).code)
                                .text_size(sp(12.5))
                                .text_color(theme.text)
                                .child(SharedString::from(format!(
                                    "ws://{}:{port}",
                                    self.daemon_hostname
                                ))),
                        )
                        .child(copy_url_button)
                })
            };
            let token_row = {
                let title = tr!("daemon.token");
                search.matched(&title, "").map(|matched| {
                    div()
                        .py(px(8.0))
                        .border_t(hairline())
                        .border_color(theme.separator)
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(settings_row_icon("icons/key-round.svg", theme))
                        .child(settings_title_jump(
                            div()
                                .w(px(80.0))
                                .flex_none()
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(settings_search_text(
                                    title,
                                    matched.title_ranges.clone(),
                                    theme,
                                )),
                            &matched,
                            theme,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(crate::fonts::current(cx).code)
                                .text_size(sp(12.5))
                                .text_color(theme.text)
                                .child(SharedString::from(if token_revealed {
                                    token.clone()
                                } else {
                                    "••••••••••••••••••••••••••••••••".to_owned()
                                })),
                        )
                        .child(reveal_token_button)
                        .child(copy_token_button)
                        .child(regenerate_button)
                })
            };
            let qr_row = {
                let title = tr!("daemon.qr_code");
                let code = self.daemon_qr.clone();
                search.matched(&title, "").map(|matched| {
                    div()
                        .py(px(8.0))
                        .border_t(hairline())
                        .border_color(theme.separator)
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(settings_row_icon("icons/qr-code.svg", theme))
                        .child(settings_title_jump(
                            div()
                                .w(px(80.0))
                                .flex_none()
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(settings_search_text(
                                    title,
                                    matched.title_ranges.clone(),
                                    theme,
                                )),
                            &matched,
                            theme,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .items_start()
                                .gap(px(6.0))
                                .when_some(code, |column, code| column.child(daemon_qr_view(code)))
                                .child(
                                    div()
                                        .whitespace_normal()
                                        .text_size(sp(12.0))
                                        .line_height(sp(15.0))
                                        .text_color(theme.text_tertiary)
                                        .child(tr!("daemon.qr_hint")),
                                ),
                        )
                        .child(qr_toggle)
                })
            };
            if search.active() && search.hits() == before {
                None
            } else {
                Some(
                    div()
                        .px(px(20.0))
                        .py(px(15.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .children(header)
                        .children(url_row)
                        .children(token_row)
                        .children(qr_row)
                        .when(!search.active(), |card| {
                            card.child(
                                div()
                                    .mt(px(7.0))
                                    .px(px(10.0))
                                    .py(px(8.0))
                                    .rounded(px(10.0))
                                    .bg(theme.inset)
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .gap(px(8.0))
                                    .child(icon("icons/alert.svg", 13.0, theme.warning))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .whitespace_normal()
                                            .text_size(sp(12.5))
                                            .line_height(sp(15.0))
                                            .text_color(theme.text_secondary)
                                            .child(tr!("daemon.security_warning")),
                                    ),
                            )
                        })
                        .into_any_element(),
                )
            }
        };

        div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .children(expose_card)
            .children(connection_card)
            .children(credentials_card)
            .children(remote_hosts_card)
            .children(keep_awake_card)
            .children(qa_branch_card)
            .children(agent_tools_card)
            .children(agent_settings_card)
            .children(sandbox_default_card)
            .children(build_card)
            .into_any_element()
    }

    /// Dev builds stamp each binary with the commit it was built from; the
    /// card puts the app's beside the connected daemon's so a stale daemon
    /// stands out. Release builds never render it.
    fn render_build_card(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let before = search.hits();
        let header = {
            let title = tr!("daemon.build_title");
            let description = tr!("daemon.build_description");
            search.matched(&title, &description).map(|matched| {
                settings_row_label(
                    "icons/hammer.svg",
                    div()
                        .child(settings_title_jump(
                            div()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(settings_search_text(
                                    title,
                                    matched.title_ranges.clone(),
                                    theme,
                                )),
                            &matched,
                            theme,
                        ))
                        .child(
                            div()
                                .mt(px(4.0))
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(sp(12.5))
                                .line_height(sp(16.0))
                                .text_color(theme.text_secondary)
                                .child(settings_search_text(
                                    description,
                                    matched.description_ranges,
                                    theme,
                                )),
                        ),
                    theme,
                )
            })
        };
        let copy_button = |id: &'static str, value: String, cx: &mut Context<Self>| {
            let copied = self.control_was_copied(id);
            div()
                .id(id)
                .tab_index(0)
                .h(px(27.0))
                .px(px(9.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .gap(px(5.0))
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .hover(|element| element.bg(theme.overlay))
                .child(icon(
                    if copied {
                        "icons/check.svg"
                    } else {
                        "icons/copy.svg"
                    },
                    11.0,
                    theme.text_tertiary,
                ))
                .child(if copied {
                    tr!("common.copied")
                } else {
                    tr!("common.copy")
                })
                .on_click(cx.listener({
                    let value = value.clone();
                    move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                        this.show_control_copied(id, cx);
                    }
                }))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                    if !event.keystroke.modifiers.modified()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                        this.show_control_copied(id, cx);
                        cx.stop_propagation();
                    }
                }))
        };
        let build_row = |icon_path: &'static str,
                         title: String,
                         value: String,
                         copy_id: &'static str,
                         top_border: bool,
                         cx: &mut Context<Self>|
         -> Option<Div> {
            search.matched(&title, "").map(|matched| {
                div()
                    .when(!top_border, |row| row.mt(px(13.0)))
                    .py(px(8.0))
                    .when(top_border, |row| {
                        row.border_t(hairline()).border_color(theme.separator)
                    })
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(settings_row_icon(icon_path, theme))
                    .child(settings_title_jump(
                        div()
                            .w(px(80.0))
                            .flex_none()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(settings_search_text(
                                title,
                                matched.title_ranges.clone(),
                                theme,
                            )),
                        &matched,
                        theme,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(crate::fonts::current(cx).code)
                            .text_size(sp(12.5))
                            .text_color(theme.text)
                            .child(SharedString::from(value.clone())),
                    )
                    .child(copy_button(copy_id, value, cx))
            })
        };
        let app_build = match option_env!("GODDARD_COMMIT_SHA") {
            Some(commit) => format!("{} · {commit}", env!("CARGO_PKG_VERSION")),
            None => env!("CARGO_PKG_VERSION").to_owned(),
        };
        let client = self.daemon.client();
        let daemon_build = match client.daemon_commit() {
            Some(commit) => format!("{} · {commit}", client.daemon_version()),
            None => client.daemon_version().to_owned(),
        };
        let app_row = build_row(
            "icons/laptop.svg",
            tr!("daemon.build_app"),
            app_build,
            "copy-app-build",
            false,
            cx,
        );
        let daemon_row = build_row(
            "icons/server.svg",
            tr!("daemon.build_daemon"),
            daemon_build,
            "copy-daemon-build",
            true,
            cx,
        );
        if search.active() && search.hits() == before {
            None
        } else {
            Some(
                div()
                    .px(px(20.0))
                    .py(px(15.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .children(header)
                    .children(app_row)
                    .children(daemon_row)
                    .into_any_element(),
            )
        }
    }

    /// Saved remote daemons merged into this window's catalog. Each row shows
    /// the record's live connection state; editing re-points the same id so
    /// its projects and sessions keep their owner.
    fn render_remote_hosts_card(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let title = tr!("daemon.remote_hosts_title");
        let description = tr!("daemon.remote_hosts_description");
        let matched = search.matched(&title, &description)?;
        let mut rows = div().flex().flex_col();
        for (index, host) in self.state.remote_hosts.iter().enumerate() {
            let host_id = host.id;
            let error = self.remote_errors.get(&host_id).cloned();
            let status = match self
                .daemons
                .supervisor(waku_client::DaemonKey::Remote(host_id))
            {
                Some(supervisor) => match supervisor.status() {
                    waku_client::DaemonStatus::Connected => {
                        (tr!("daemon.phase_connected"), theme.success)
                    }
                    waku_client::DaemonStatus::Recovering => {
                        (tr!("daemon.phase_connecting"), theme.warning)
                    }
                    waku_client::DaemonStatus::Unreachable => {
                        (tr!("daemon.phase_disconnected"), theme.danger)
                    }
                },
                None => match &error {
                    Some(_) => (tr!("daemon.phase_error"), theme.danger),
                    None => (tr!("daemon.phase_connecting"), theme.text_tertiary),
                },
            };
            let action_button = |id: SharedString, icon_path: &'static str, label: String| {
                div()
                    .id(id)
                    .tab_index(0)
                    .size(px(24.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .text_color(theme.text_secondary)
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .focus_visible(|element| element.bg(theme.focus_highlight()))
                    .tooltip(Tooltip::text(label))
                    .child(icon(icon_path, 13.0, theme.text_tertiary))
            };
            let edit_button = action_button(
                SharedString::from(format!("remote-host-edit-{host_id}")),
                "icons/pencil.svg",
                tr!("daemon.remote_host_edit"),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_remote_host_editor(Some(host_id), window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.open_remote_host_editor(Some(host_id), window, cx);
                    cx.stop_propagation();
                }
            }));
            let remove_button = action_button(
                SharedString::from(format!("remote-host-remove-{host_id}")),
                "icons/trash.svg",
                tr!("daemon.remote_host_remove"),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.remove_remote_host(host_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.remove_remote_host(host_id, cx);
                    cx.stop_propagation();
                }
            }));
            // An explicit connect affordance for offline hosts — the only
            // trigger a catalog-less host has, and the retry path for one
            // whose auth the user cancelled.
            let connect_button = (!self.remote_host_connected(host_id)).then(|| {
                action_button(
                    SharedString::from(format!("remote-host-connect-{host_id}")),
                    "icons/rotate-cw.svg",
                    tr!("daemon.reconnect"),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.use_remote_host(host_id, cx);
                }))
                .on_key_down(cx.listener(
                    move |this, event: &KeyDownEvent, _, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            this.use_remote_host(host_id, cx);
                            cx.stop_propagation();
                        }
                    },
                ))
            });
            rows = rows.child(
                div()
                    .when(index > 0, |element| {
                        element.border_t(hairline()).border_color(theme.separator)
                    })
                    .py(px(9.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(icon("icons/server.svg", 14.0, theme.text_secondary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(sp(13.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(SharedString::from(host.name.clone())),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded_full()
                                            .text_size(sp(11.5))
                                            .text_color(status.1)
                                            .bg(theme.overlay)
                                            .child(status.0),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(1.0))
                                    .truncate()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(12.0))
                                    .text_color(theme.text_tertiary)
                                    .child(SharedString::from(
                                        host.ssh_destination
                                            .clone()
                                            .unwrap_or_else(|| host.address.clone()),
                                    )),
                            )
                            .when_some(error, |element, error| {
                                element.child(
                                    div()
                                        .mt(px(1.0))
                                        .whitespace_normal()
                                        .line_height(sp(15.0))
                                        .text_size(sp(12.0))
                                        .text_color(theme.danger)
                                        .child(SharedString::from(error)),
                                )
                            }),
                    )
                    .when_some(connect_button, |element, button| element.child(button))
                    .child(edit_button)
                    .child(remove_button),
            );
        }

        let add_button = div()
            .id("remote-host-add")
            .tab_index(0)
            .h(px(27.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|element| element.bg(theme.focus_highlight()))
            .child(icon("icons/plus.svg", 12.0, theme.text_tertiary))
            .child(tr!("daemon.remote_host_add"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_remote_host_editor(None, window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.open_remote_host_editor(None, window, cx);
                    cx.stop_propagation();
                }
            }));

        let mut card = div()
            .px(px(20.0))
            .py(px(15.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .child(settings_row_label(
                "icons/server.svg",
                div()
                    .child(settings_title_jump(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(settings_search_text(
                                title,
                                matched.title_ranges.clone(),
                                theme,
                            )),
                        &matched,
                        theme,
                    ))
                    .child(
                        div()
                            .mt(px(4.0))
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(sp(12.5))
                            .line_height(sp(16.0))
                            .text_color(theme.text_secondary)
                            .child(settings_search_text(
                                description,
                                matched.description_ranges.clone(),
                                theme,
                            )),
                    ),
                theme,
            ));
        // The editor form and the add button are chrome, not matches — a
        // search renders the card's saved rows only.
        if !search.active() {
            if let Some(editor) = &self.remote_host_editor {
                card = card.child(self.render_remote_host_editor(editor, theme, cx));
            } else {
                card = card.child(div().mt(px(12.0)).flex().justify_end().child(add_button));
            }
        }
        if self.remote_host_editor.is_none() || search.active() {
            card = card.when(!self.state.remote_hosts.is_empty(), |card| {
                card.child(div().mt(px(6.0)).child(rows))
            });
        }
        // Daemons discovered on the LAN — skip ones already saved, and
        // ones with no exposed WebSocket since there'd be nothing to
        // connect the granted token to.
        let nearby: Vec<_> = self
            .nearby_daemons
            .values()
            .filter(|daemon| {
                daemon.ws_url.as_ref().is_some_and(|url| {
                    !self
                        .state
                        .remote_hosts
                        .iter()
                        .any(|host| host.address == *url)
                })
            })
            .collect();
        if !search.active() && self.remote_host_editor.is_none() && !nearby.is_empty() {
            let mut nearby_rows = div().flex().flex_col();
            for (index, daemon) in nearby.iter().enumerate() {
                let daemon = (*daemon).clone();
                let endpoint_id = daemon.endpoint_id.to_string();
                let action = if self.pair_requests_in_flight.contains(&endpoint_id) {
                    div()
                        .flex_none()
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded_full()
                        .text_size(sp(11.5))
                        .text_color(theme.warning)
                        .bg(theme.overlay)
                        .child(tr!("daemon.pair_waiting"))
                        .into_any_element()
                } else {
                    div()
                        .id(SharedString::from(format!("nearby-pair-{endpoint_id}")))
                        .tab_index(0)
                        .h(px(27.0))
                        .px(px(10.0))
                        .rounded(px(8.0))
                        .border(hairline())
                        .border_color(theme.border_strong)
                        .flex()
                        .items_center()
                        .cursor_default()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .hover(|element| element.bg(theme.overlay))
                        .focus_visible(|element| element.bg(theme.focus_highlight()))
                        .child(tr!("daemon.pair"))
                        .on_click(cx.listener({
                            let daemon = daemon.clone();
                            move |this, _, _, cx| this.pair_with_daemon(daemon.clone(), cx)
                        }))
                        .on_key_down(cx.listener({
                            let daemon = daemon.clone();
                            move |this, event: &KeyDownEvent, _, cx| {
                                if !event.keystroke.modifiers.modified()
                                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    this.pair_with_daemon(daemon.clone(), cx);
                                    cx.stop_propagation();
                                }
                            }
                        }))
                        .into_any_element()
                };
                nearby_rows = nearby_rows.child(
                    div()
                        .when(index > 0, |element| {
                            element.border_t(hairline()).border_color(theme.separator)
                        })
                        .py(px(9.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(icon("icons/server.svg", 14.0, theme.text_secondary))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(sp(13.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child(SharedString::from(daemon.info.name.clone())),
                                )
                                .child(
                                    div()
                                        .mt(px(1.0))
                                        .truncate()
                                        .font_family(crate::fonts::current(cx).code)
                                        .text_size(sp(12.0))
                                        .text_color(theme.text_tertiary)
                                        .child(SharedString::from(
                                            daemon.ws_url.clone().unwrap_or_default(),
                                        )),
                                ),
                        )
                        .child(action),
                );
            }
            card = card.child(
                div()
                    .mt(px(12.0))
                    .pt(px(10.0))
                    .border_t(hairline())
                    .border_color(theme.separator)
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child(tr!("daemon.nearby_title")),
                    )
                    .child(nearby_rows),
            );
        }
        Some(card.into_any_element())
    }

    fn render_remote_host_editor(
        &self,
        editor: &RemoteHostEditor,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let editing = editor.id.is_some();
        let ssh = editor.transport == RemoteHostTransport::Ssh;
        let ssh_button = self.remote_host_transport_button(
            RemoteHostTransport::Ssh,
            editor.transport,
            theme,
            cx,
        );
        let direct_button = self.remote_host_transport_button(
            RemoteHostTransport::Direct,
            editor.transport,
            theme,
            cx,
        );
        let field_label = |text: String| {
            div()
                .mt(px(12.0))
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(text)
        };
        let ghost_button = |id: &'static str, label: String| {
            div()
                .id(id)
                .tab_index(0)
                .h(px(27.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .child(label)
        };
        let reveal_token_button = div()
            .id("remote-host-token-reveal")
            .tab_index(0)
            .size(px(27.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.border_color(theme.accent))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon(
                if editor.token_revealed {
                    "icons/eye-off.svg"
                } else {
                    "icons/eye.svg"
                },
                12.0,
                theme.text_tertiary,
            ))
            .tooltip(Tooltip::text(if editor.token_revealed {
                tr!("daemon.hide_token")
            } else {
                tr!("daemon.reveal_token")
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_remote_host_token(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.toggle_remote_host_token(cx);
                    cx.stop_propagation();
                }
            }));
        div()
            .mt(px(12.0))
            .pt(px(4.0))
            .border_t(hairline())
            .border_color(theme.separator)
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(if editing {
                        tr!("daemon.remote_host_edit")
                    } else {
                        tr!("daemon.remote_host_new")
                    }),
            )
            // The transport picker is the field group's switch — off unix
            // there is only WebSocket, so the row is omitted entirely.
            .when(cfg!(unix), |element| {
                element
                    .child(field_label(tr!("daemon.remote_host_transport")))
                    .child(
                        div()
                            .mt(px(5.0))
                            .self_start()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .rounded(px(8.0))
                            .p(px(2.0))
                            .bg(theme.inset)
                            .child(ssh_button)
                            .child(direct_button),
                    )
            })
            .child(field_label(tr!("daemon.remote_host_name")))
            .child(
                div()
                    .mt(px(5.0))
                    .child(TextField::new("remote-host-name", editor.name.clone()).w_full()),
            )
            .when(ssh, |element| {
                element
                    .child(field_label(tr!("daemon.remote_host_ssh")))
                    .child(div().mt(px(5.0)).child(
                        TextField::new("remote-host-ssh", editor.destination.clone()).w_full(),
                    ))
                    .child(
                        div()
                            .mt(px(5.0))
                            .whitespace_normal()
                            .text_size(sp(12.0))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .child(tr!("daemon.remote_host_ssh_description")),
                    )
                    .when(!editor.ssh_hosts.is_empty(), |element| {
                        element.child(div().mt(px(7.0)).flex().flex_wrap().gap(px(4.0)).children(
                            editor.ssh_hosts.iter().map(|alias| {
                                let alias = alias.clone();
                                let destination = editor.destination.clone();
                                div()
                                    .id(SharedString::from(format!("ssh-host-{alias}")))
                                    .tab_index(0)
                                    .h(px(22.0))
                                    .px(px(7.0))
                                    .rounded(px(6.0))
                                    .border(hairline())
                                    .border_color(theme.border)
                                    .flex()
                                    .items_center()
                                    .cursor_default()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_secondary)
                                    .hover(|element| element.bg(theme.overlay))
                                    .focus_visible(|element| element.bg(theme.focus_highlight()))
                                    .child(alias.clone())
                                    .on_click(move |_, _, cx| {
                                        destination
                                            .update(cx, |input, cx| input.set_content(&alias, cx));
                                    })
                            }),
                        ))
                    })
            })
            .when(!ssh, |element| {
                element
                    .child(field_label(tr!("daemon.remote_host_address")))
                    .child(div().mt(px(5.0)).child(
                        TextField::new("remote-host-address", editor.address.clone()).w_full(),
                    ))
                    .child(field_label(tr!("daemon.remote_host_token")))
                    .child(
                        div()
                            .mt(px(5.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                TextField::new("remote-host-token", editor.token.clone()).flex_1(),
                            )
                            .child(reveal_token_button),
                    )
            })
            .when(editor.missing_fields, |element| {
                element.child(
                    div()
                        .mt(px(6.0))
                        .text_size(sp(12.0))
                        .text_color(theme.danger)
                        .child(if ssh {
                            tr!("daemon.remote_host_ssh_required")
                        } else {
                            tr!("daemon.remote_host_direct_required")
                        }),
                )
            })
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        ghost_button("remote-host-cancel", tr!("common.cancel")).on_activation(
                            cx,
                            |this, _, cx| {
                                this.remote_host_editor = None;
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        ghost_button("remote-host-save", tr!("daemon.remote_host_save"))
                            .on_activation(cx, |this, _, cx| this.save_remote_host_editor(cx)),
                    ),
            )
    }

    /// One segment of the remote-host editor's transport picker, styled like
    /// the Projects header tabs. Switching clears a stale missing-fields
    /// error since the required set changes with the mode.
    fn remote_host_transport_button(
        &self,
        transport: RemoteHostTransport,
        current: RemoteHostTransport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = transport == current;
        let (id, label) = match transport {
            RemoteHostTransport::Ssh => (
                "remote-host-transport-ssh",
                tr!("daemon.remote_host_transport_ssh"),
            ),
            RemoteHostTransport::Direct => (
                "remote-host-transport-direct",
                tr!("daemon.remote_host_transport_direct"),
            ),
        };
        div()
            .id(id)
            .tab_index(0)
            .h(px(22.0))
            .px(px(10.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.0))
            .when(selected, |element| {
                element.bg(theme.surface).text_color(theme.text)
            })
            .when(!selected, |element| {
                element
                    .text_color(theme.text_secondary)
                    .hover(|style| style.text_color(theme.text))
            })
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .child(label)
            .on_activation(cx, move |this, _, cx| {
                if let Some(editor) = &mut this.remote_host_editor {
                    editor.transport = transport;
                    editor.missing_fields = false;
                    cx.notify();
                }
            })
    }

    /// The daemon-scoped opt-in for agent-to-agent commands. Toggling it only
    /// affects sessions started afterwards — running sessions keep the launch
    /// environment they already have.
    fn agent_tools_card(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let enabled = self.state.agent_tools_enabled;
        let title = tr!("daemon.agent_tools_title");
        let description = tr!("daemon.agent_tools_description");
        let matched = search.matched(&title, &description)?;
        let toggle = toggle_switch(
            "agent-tools-toggle",
            enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_agent_tools_enabled(!enabled, cx),
        );
        Some(
            div()
                .min_h(px(66.0))
                .px(px(20.0))
                .py(px(13.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .gap(px(24.0))
                .child(settings_row_label(
                    "icons/bot.svg",
                    settings_row_text(title, description, matched, theme).whitespace_normal(),
                    theme,
                ))
                .child(toggle)
                .into_any_element(),
        )
    }

    fn set_agent_tools_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.agent_tools_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The daemon-scoped Caffeine switch: while on, the daemon holds the
    /// host's sleep assertions so the mobile and web apps can still connect.
    fn keep_awake_card(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let enabled = self.state.keep_awake;
        let title = tr!("daemon.keep_awake_title");
        let description = tr!("daemon.keep_awake_description");
        let matched = search.matched(&title, &description)?;
        let toggle = toggle_switch(
            "keep-awake-toggle",
            enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_keep_awake(!enabled, cx),
        );
        Some(
            div()
                .min_h(px(66.0))
                .px(px(20.0))
                .py(px(13.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .gap(px(24.0))
                .child(settings_row_label(
                    "icons/coffee.svg",
                    settings_row_text(title, description, matched, theme).whitespace_normal(),
                    theme,
                ))
                .child(toggle)
                .into_any_element(),
        )
    }

    fn set_keep_awake(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.keep_awake = enabled;
        self.save();
        cx.notify();
    }

    /// The daemon-scoped name of the review train's branch — the Review
    /// tab lists `origin/<name>` and rejections push reverts onto it.
    /// Edited live; an empty field resolves to `qa` daemon-side.
    fn qa_branch_card(&self, theme: Theme, search: &SettingSearch) -> Option<AnyElement> {
        let title = tr!("daemon.qa_branch_title");
        let description = tr!("daemon.qa_branch_description");
        let matched = search.matched(&title, &description)?;
        Some(
            div()
                .w_full()
                .min_h(px(66.0))
                .px(px(20.0))
                .py(px(13.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .child(settings_row_label(
                    "icons/git-branch.svg",
                    settings_row_text(title, description, matched, theme).child(
                        div().mt(px(9.0)).max_w(px(360.0)).child(
                            TextField::new("qa-branch-field", self.qa_branch_input.clone())
                                .w_full(),
                        ),
                    ),
                    theme,
                ))
                .into_any_element(),
        )
    }

    /// The daemon-scoped settings surface agents may write — custom commands
    /// today. On by default; turning it off makes the daemon reject the
    /// `goddard-agent command` calls outright while `create`/`prompt` stay gated
    /// by their own switch above.
    fn agent_settings_card(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let enabled = self.state.agent_settings_enabled;
        let title = tr!("daemon.agent_settings_title");
        let description = tr!("daemon.agent_settings_description");
        let matched = search.matched(&title, &description)?;
        let toggle = toggle_switch(
            "agent-settings-toggle",
            enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_agent_settings_enabled(!enabled, cx),
        );
        Some(
            div()
                .min_h(px(66.0))
                .px(px(20.0))
                .py(px(13.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .gap(px(24.0))
                .child(settings_row_label(
                    "icons/settings.svg",
                    settings_row_text(title, description, matched, theme).whitespace_normal(),
                    theme,
                ))
                .child(toggle)
                .into_any_element(),
        )
    }

    fn set_agent_settings_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.agent_settings_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The daemon-scoped "sandbox new tasks by default" pref. It only changes
    /// the environment a fresh task seeds — a task's own Environment pick
    /// still wins, and the card only exists while the sandbox experiment
    /// exposes the surface.
    fn sandbox_default_card(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.state.sandbox_experiment_enabled {
            return None;
        }
        let enabled = self.state.sandbox_default_enabled;
        let title = tr!("daemon.new_task_environment");
        let description = tr!("daemon.new_task_environment_description");
        let matched = search.matched(&title, &description)?;
        let selector = self.setting_selector(
            "sandbox-default-selector",
            vec![
                (false, tr!("sandbox.this_mac")),
                (true, tr!("sandbox.sandbox_vm")),
            ],
            enabled,
            160.0,
            cx,
            |this, value, _, cx| this.set_sandbox_default_enabled(value, cx),
        );
        Some(
            div()
                .min_h(px(66.0))
                .px(px(20.0))
                .py(px(13.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .gap(px(24.0))
                .child(settings_row_label(
                    "icons/container.svg",
                    settings_row_text(title, description, matched, theme).whitespace_normal(),
                    theme,
                ))
                .child(selector)
                .into_any_element(),
        )
    }

    fn set_sandbox_default_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.sandbox_default_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The Experiments page: one opt-in card per unfinished feature, each
    /// defaulting off, grouped under the surface it changes. Subagents and
    /// Computer Use are daemon-owned — their flags travel with the daemon
    /// settings `save()` already syncs.
    fn render_experiments_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        // The page stays behind a warning until the user accepts it once.
        // Under a query the section renders nothing at all, so a settings
        // search can't reach a toggle that skips the gate.
        if !self.state.experiments_warning_acknowledged {
            return if search.active() {
                div().into_any_element()
            } else {
                self.render_experiments_gate(theme, cx)
            };
        }
        let experiments = [
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "subagents-experiment-toggle",
                icon: "icons/bot.svg",
                title_key: "experiments.subagents_title",
                description_key: "experiments.subagents_description",
                enabled: self.state.subagents_enabled,
                set: Self::set_subagents_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "automations-experiment-toggle",
                icon: "icons/zap.svg",
                title_key: "experiments.automations_title",
                description_key: "experiments.automations_description",
                enabled: self.state.automations_enabled,
                set: Self::set_automations_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "memory-experiment-toggle",
                icon: "icons/brain.svg",
                title_key: "experiments.memory_title",
                description_key: "experiments.memory_description",
                enabled: self.state.memory_experiment_enabled,
                set: Self::set_memory_experiment_enabled,
                eval_backed: false,
                tuning: Some(Self::memory_tuning),
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "project-map-experiment-toggle",
                icon: "icons/map.svg",
                title_key: "experiments.project_map_title",
                description_key: "experiments.project_map_description",
                enabled: self.state.project_map_enabled,
                set: Self::set_project_map_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "model-router-experiment-toggle",
                icon: "icons/fork.svg",
                title_key: "experiments.model_router_title",
                description_key: "experiments.model_router_description",
                enabled: self.state.model_router_enabled,
                set: Self::set_model_router_enabled,
                eval_backed: true,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "status-markers-experiment-toggle",
                icon: "icons/circle-dot.svg",
                title_key: "experiments.status_markers_title",
                description_key: "experiments.status_markers_description",
                enabled: self.state.status_markers_enabled,
                set: Self::set_status_markers_enabled,
                eval_backed: true,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "action-predictions-experiment-toggle",
                icon: "icons/sparkle.svg",
                title_key: "experiments.action_predictions_title",
                description_key: "experiments.action_predictions_description",
                enabled: self.state.action_predictions_enabled,
                set: Self::set_action_predictions_enabled,
                eval_backed: true,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "phase-routing-experiment-toggle",
                icon: "icons/map.svg",
                title_key: "experiments.phase_routing_title",
                description_key: "experiments.phase_routing_description",
                enabled: self.state.phase_routing_enabled,
                set: Self::set_phase_routing_enabled,
                eval_backed: true,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "computer-use-experiment-toggle",
                icon: "icons/monitor.svg",
                title_key: "experiments.computer_use_title",
                description_key: "experiments.computer_use_description",
                enabled: self.state.computer_use_experiment_enabled,
                set: Self::set_computer_use_experiment_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "integrations-experiment-toggle",
                icon: "icons/package.svg",
                title_key: "experiments.integrations_title",
                description_key: "experiments.integrations_description",
                enabled: self.state.integrations_enabled,
                set: Self::set_integrations_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "sandbox-experiment-toggle",
                icon: "icons/container.svg",
                title_key: "experiments.sandbox_title",
                description_key: "experiments.sandbox_description",
                enabled: self.state.sandbox_experiment_enabled,
                set: Self::set_sandbox_experiment_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Sessions,
                id: "voice-briefing-experiment-toggle",
                icon: "icons/volume-2.svg",
                title_key: "experiments.voice_briefing_title",
                description_key: "experiments.voice_briefing_description",
                enabled: self.state.voice_briefing_enabled,
                set: Self::set_voice_briefing_enabled,
                eval_backed: false,
                tuning: Some(Self::voice_briefing_tuning),
            },
            ExperimentDef {
                group: ExperimentGroup::Git,
                id: "git-panel-experiment-toggle",
                icon: "icons/panel-right.svg",
                title_key: "experiments.git_panel_title",
                description_key: "experiments.git_panel_description",
                enabled: self.state.git_panel_enabled,
                set: Self::set_git_panel_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Git,
                id: "github-experiment-toggle",
                icon: "icons/github.svg",
                title_key: "experiments.github_title",
                description_key: "experiments.github_description",
                enabled: self.state.github_enabled,
                set: Self::set_github_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Git,
                id: "projects-page-experiment-toggle",
                icon: "icons/projects.svg",
                title_key: "experiments.projects_page_title",
                description_key: "experiments.projects_page_description",
                enabled: self.state.projects_page_enabled,
                set: Self::set_projects_page_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Git,
                id: "review-queue-experiment-toggle",
                icon: "icons/queue.svg",
                title_key: "experiments.review_queue_title",
                description_key: "experiments.review_queue_description",
                enabled: self.state.review_queue_enabled,
                set: Self::set_review_queue_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Surfaces,
                id: "big-picture-experiment-toggle",
                icon: "icons/map.svg",
                title_key: "experiments.big_picture_title",
                description_key: "experiments.big_picture_description",
                enabled: self.state.big_picture_enabled,
                set: Self::set_big_picture_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Surfaces,
                id: "friends-experiment-toggle",
                icon: "icons/friends.svg",
                title_key: "experiments.friends_title",
                description_key: "experiments.friends_description",
                enabled: self.state.friends_enabled,
                set: Self::set_friends_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Surfaces,
                id: "sidebar-dock-experiment-toggle",
                icon: "icons/panel-left.svg",
                title_key: "experiments.sidebar_dock_title",
                description_key: "experiments.sidebar_dock_description",
                enabled: self.state.sidebar_dock_enabled,
                set: Self::set_sidebar_dock_enabled,
                eval_backed: false,
                tuning: None,
            },
            ExperimentDef {
                group: ExperimentGroup::Surfaces,
                id: "guided-reading-experiment-toggle",
                icon: "icons/book-open.svg",
                title_key: "experiments.guided_reading_title",
                description_key: "experiments.guided_reading_description",
                enabled: self.state.guided_reading_enabled,
                set: Self::set_guided_reading_enabled,
                eval_backed: false,
                tuning: Some(Self::guided_reading_tuning),
            },
        ];
        let groups = ExperimentGroup::ALL.into_iter().filter_map(|group| {
            let cards: Vec<AnyElement> = experiments
                .iter()
                .filter(|experiment| experiment.group == group)
                .filter_map(|experiment| self.experiment_card(experiment, theme, search, cx))
                .collect();
            settings_group(tr!(group.title_key()), cards, theme)
        });
        div()
            .when(!search.active(), |element| {
                element.child(
                    div()
                        .mt(px(15.0))
                        .w_full()
                        .px(px(20.0))
                        .py(px(14.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .line_height(sp(18.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("settings.experiments_description")),
                        ),
                )
            })
            .child(
                div()
                    .mt(px(15.0))
                    .flex()
                    .flex_col()
                    .gap(px(20.0))
                    .children(groups),
            )
            .into_any_element()
    }

    /// The one-time interstitial standing between the Experiments page and
    /// its toggles: experiments can be buggy or corrupt data outright, so
    /// the page shows this warning until the user accepts it once.
    fn render_experiments_gate(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mt(px(15.0))
            .w_full()
            .px(px(20.0))
            .py(px(16.0))
            .rounded(px(16.0))
            .border(hairline())
            .border_color(theme.warning.opacity(0.5))
            .bg(theme.raised)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .child(icon("icons/alert.svg", 16.0, theme.warning))
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(tr!("experiments.gate_title")),
                    ),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(sp(12.5))
                    .line_height(sp(18.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("experiments.gate_body")),
            )
            .child(
                div().mt(px(12.0)).flex().child(
                    div()
                        .id("experiments-gate-confirm")
                        .tab_index(0)
                        .h(px(29.0))
                        .px(px(13.0))
                        .rounded(px(9.0))
                        .border(hairline())
                        .border_color(theme.inverse)
                        .flex()
                        .items_center()
                        .cursor_default()
                        .text_size(sp(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .bg(theme.inverse)
                        .text_color(theme.on_inverse)
                        .hover(|element| element.opacity(0.9))
                        .active(|element| element.opacity(0.8))
                        .focus_visible(|element| element.bg(theme.focus_highlight()))
                        .child(tr!("experiments.gate_confirm"))
                        .on_activation(cx, |this, _, cx| this.acknowledge_experiments_gate(cx)),
                ),
            )
            .into_any_element()
    }

    fn acknowledge_experiments_gate(&mut self, cx: &mut Context<Self>) {
        self.state.experiments_warning_acknowledged = true;
        self.save();
        cx.notify();
    }

    /// The Jev page — the eval backend every eval-backed feature shares:
    /// backend and credentials first, then the Auto routing experiment's
    /// class-level targets. The page only exists in navigation while at
    /// least one eval-backed experiment is on.
    fn render_jev_settings(&self, search: &SettingSearch, cx: &mut Context<Self>) -> AnyElement {
        div()
            .child(self.render_model_routing_settings(Theme::current(cx), search, cx))
            .child(self.render_auto_prompt_settings(search, cx))
            .into_any_element()
    }

    fn render_auto_prompt_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let mut rows = Vec::new();
        for rule in &self.state.auto_prompts {
            let id = rule.id;
            let enabled = rule.enabled;
            let controls = div()
                .flex()
                .items_center()
                .gap(px(7.0))
                .child(settings_button(
                    format!("auto-prompt-edit-{id}"),
                    tr!("auto_prompts.edit"),
                    true,
                    false,
                    true,
                    theme,
                    cx,
                    move |this, window, cx| this.open_auto_prompt_editor(Some(id), window, cx),
                ))
                .child(settings_button(
                    format!("auto-prompt-toggle-{id}"),
                    if enabled {
                        tr!("auto_prompts.disable")
                    } else {
                        tr!("auto_prompts.enable")
                    },
                    enabled || rule.valid_for_dispatch_without_enabled(),
                    false,
                    true,
                    theme,
                    cx,
                    move |this, _, cx| this.set_auto_prompt_enabled(id, !enabled, cx),
                ))
                .child(settings_button(
                    format!("auto-prompt-remove-{id}"),
                    tr!("auto_prompts.remove"),
                    true,
                    true,
                    true,
                    theme,
                    cx,
                    move |this, _, cx| this.remove_auto_prompt(id, cx),
                ));
            rows.push(settings_row(
                "icons/zap.svg",
                rule.name.clone(),
                rule.prompt.clone(),
                controls,
                theme,
                search,
            ));
        }
        if rows.is_empty() {
            rows.push(settings_row(
                "icons/zap.svg",
                tr!("auto_prompts.title"),
                tr!("auto_prompts.description"),
                div(),
                theme,
                search,
            ));
        }
        let list = settings_row_card(rows, theme).map(|card| {
            card.mt(px(15.0)).child(
                div()
                    .p(px(12.0))
                    .flex()
                    .justify_end()
                    .child(settings_button(
                        "auto-prompt-add",
                        tr!("auto_prompts.add"),
                        true,
                        false,
                        false,
                        theme,
                        cx,
                        |this, window, cx| this.open_auto_prompt_editor(None, window, cx),
                    )),
            )
        });
        div()
            .children(list)
            .when(!search.active(), |element| {
                element.children(self.render_auto_prompt_editor(cx))
            })
            .into_any_element()
    }

    fn render_auto_prompt_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.auto_prompt_editor.as_ref()?;
        let theme = Theme::current(cx);
        let mut rows = vec![
            settings_row(
                "icons/pencil.svg",
                tr!("auto_prompts.name"),
                tr!("auto_prompts.name_description"),
                TextField::new("auto-prompt-name", editor.name.clone()).w(px(320.0)),
                theme,
                &SettingSearch::new(""),
            ),
            settings_row(
                "icons/compose.svg",
                tr!("auto_prompts.prompt"),
                tr!("auto_prompts.prompt_description"),
                TextField::new("auto-prompt-prompt", editor.prompt.clone()).w(px(320.0)),
                theme,
                &SettingSearch::new(""),
            ),
        ];
        for (index, question) in editor.questions.iter().enumerate() {
            let id = question.id;
            rows.push(settings_row(
                "icons/circle-help.svg",
                tr!("auto_prompts.question", number = index + 1),
                tr!("auto_prompts.question_description"),
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .child(
                        TextField::new(
                            format!("auto-prompt-question-{id}"),
                            question.instructions.clone(),
                        )
                        .w(px(260.0)),
                    )
                    .when(editor.advanced, |row| {
                        row.child(
                            div()
                                .text_size(sp(12.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("auto_prompts.weight")),
                        )
                        .child(
                            TextField::new(
                                format!("auto-prompt-weight-{id}"),
                                question.weight.clone(),
                            )
                            .w(px(60.0)),
                        )
                    })
                    .child(settings_button(
                        format!("auto-prompt-question-remove-{id}"),
                        tr!("auto_prompts.remove"),
                        editor.questions.len() > 1,
                        true,
                        true,
                        theme,
                        cx,
                        move |this, _, cx| this.remove_auto_prompt_question(id, cx),
                    )),
                theme,
                &SettingSearch::new(""),
            ));
        }
        let threshold = editor
            .threshold
            .read(cx)
            .content()
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .unwrap_or(0.85)
            .clamp(0.0, 1.0);
        let shown_threshold = self.auto_prompt_sensitivity_slider.shown(threshold * 100.0);
        let sensitivity_slider = slider::slider(
            "auto-prompt-sensitivity-slider",
            &self.auto_prompt_sensitivity_slider,
            100.0,
            threshold * 100.0,
            cx,
            |this, value, window, cx| this.set_auto_prompt_sensitivity(value, window, cx),
        );
        rows.push(settings_row(
            "icons/gauge.svg",
            tr!("auto_prompts.sensitivity"),
            tr!("auto_prompts.sensitivity_description"),
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(sp(11.5))
                        .text_color(theme.text_secondary)
                        .child(tr!("auto_prompts.more_often")),
                )
                .child(sensitivity_slider.w(px(130.0)).flex_none())
                .child(
                    div()
                        .text_size(sp(11.5))
                        .text_color(theme.text_secondary)
                        .child(tr!("auto_prompts.less_often")),
                )
                .child(
                    div()
                        .w(px(36.0))
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(format!("{shown_threshold:.0}%")),
                ),
            theme,
            &SettingSearch::new(""),
        ));
        if editor.advanced {
            rows.push(settings_row(
                "icons/gauge.svg",
                tr!("auto_prompts.threshold"),
                tr!("auto_prompts.threshold_description"),
                TextField::new("auto-prompt-threshold", editor.threshold.clone()).w(px(100.0)),
                theme,
                &SettingSearch::new(""),
            ));
        }
        let status = editor.status.as_ref().map(|result| match result {
            Ok(message) => (message.clone(), theme.success),
            Err(message) => (message.clone(), theme.warning),
        });
        let preview = editor.preview.as_ref().map(|(answers, score, fires)| {
            let parts = answers
                .iter()
                .map(|(question, probability)| format!("{question}: {:.0}%", probability * 100.0))
                .collect::<Vec<_>>()
                .join(" · ");
            format!(
                "{} · {:.0}% · {}",
                parts,
                score * 100.0,
                if *fires {
                    tr!("auto_prompts.would_send")
                } else {
                    tr!("auto_prompts.would_skip")
                }
            )
        });
        settings_row_card(rows, theme).map(|card| {
            card.mt(px(15.0))
                .child(
                    div()
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .when_some(status, |column, (message, color)| {
                            column.child(div().text_size(sp(12.0)).text_color(color).child(message))
                        })
                        .when_some(preview, |column, message| {
                            column.child(
                                div()
                                    .text_size(sp(12.0))
                                    .text_color(theme.text_secondary)
                                    .child(message),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .justify_end()
                                .gap(px(7.0))
                                .child(settings_button(
                                    "auto-prompt-add-question",
                                    tr!("auto_prompts.add_question"),
                                    true,
                                    false,
                                    true,
                                    theme,
                                    cx,
                                    |this, window, cx| this.add_auto_prompt_question(window, cx),
                                ))
                                .child(settings_button(
                                    "auto-prompt-advanced",
                                    tr!("auto_prompts.advanced"),
                                    true,
                                    false,
                                    true,
                                    theme,
                                    cx,
                                    |this, _, cx| this.toggle_auto_prompt_advanced(cx),
                                ))
                                .child(settings_button(
                                    "auto-prompt-suggest",
                                    tr!("auto_prompts.suggest"),
                                    !editor.pending,
                                    false,
                                    true,
                                    theme,
                                    cx,
                                    |this, _, cx| this.suggest_auto_prompt_values(false, cx),
                                ))
                                .child(settings_button(
                                    "auto-prompt-try",
                                    tr!("auto_prompts.try_task"),
                                    !editor.pending,
                                    false,
                                    true,
                                    theme,
                                    cx,
                                    |this, _, cx| this.preview_auto_prompt(cx),
                                ))
                                .child(settings_button(
                                    "auto-prompt-save",
                                    if editor.pending {
                                        tr!("auto_prompts.working")
                                    } else {
                                        tr!("auto_prompts.save")
                                    },
                                    !editor.pending,
                                    false,
                                    false,
                                    theme,
                                    cx,
                                    |this, _, cx| this.save_auto_prompt_editor(cx),
                                )),
                        ),
                )
                .into_any_element()
        })
    }

    fn open_auto_prompt_editor(
        &mut self,
        id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.auto_prompt_sensitivity_slider.cancel();
        let existing = id.and_then(|id| {
            self.state
                .auto_prompts
                .iter()
                .find(|rule| rule.id == id)
                .cloned()
        });
        let name = cx.new(|cx| {
            TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("auto_prompts.name"))
                .placeholder(tr!("auto_prompts.name_placeholder"))
        });
        let prompt = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .auto_height()
                .max_lines(8)
                .tab_index(0)
                .accessibility_label(tr!("auto_prompts.prompt"))
                .placeholder(tr!("auto_prompts.prompt_placeholder"))
        });
        let threshold = cx.new(|cx| {
            TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("auto_prompts.threshold"))
                .placeholder("0.80")
        });
        if let Some(rule) = &existing {
            name.update(cx, |input, cx| input.set_content(rule.name.clone(), cx));
            prompt.update(cx, |input, cx| input.set_content(rule.prompt.clone(), cx));
            if let Some(value) = rule.threshold {
                threshold.update(cx, |input, cx| input.set_content(format!("{value:.2}"), cx));
            }
        }
        let questions = existing
            .as_ref()
            .map(|rule| {
                rule.questions
                    .iter()
                    .map(|question| auto_prompt_question_editor(window, cx, Some(question)))
                    .collect()
            })
            .unwrap_or_else(|| vec![auto_prompt_question_editor(window, cx, None)]);
        self.auto_prompt_editor = Some(AutoPromptEditor {
            id: existing
                .as_ref()
                .map(|rule| rule.id)
                .unwrap_or_else(Uuid::new_v4),
            name,
            prompt,
            questions,
            threshold,
            advanced: false,
            pending: false,
            status: None,
            preview: None,
        });
        cx.notify();
    }

    fn add_auto_prompt_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.auto_prompt_editor.as_mut() {
            editor
                .questions
                .push(auto_prompt_question_editor(window, cx, None));
            editor.preview = None;
            cx.notify();
        }
    }

    fn remove_auto_prompt_question(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(editor) = self.auto_prompt_editor.as_mut() {
            if editor.questions.len() > 1 {
                editor.questions.retain(|question| question.id != id);
                editor.preview = None;
                cx.notify();
            }
        }
    }

    fn toggle_auto_prompt_advanced(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.auto_prompt_editor.as_mut() {
            editor.advanced = !editor.advanced;
            cx.notify();
        }
    }

    fn set_auto_prompt_sensitivity(
        &mut self,
        value: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.auto_prompt_editor.as_mut() else {
            return;
        };
        let threshold = (value.round() as i32).clamp(0, 100) as f64 / 100.0;
        editor.threshold.update(cx, |input, cx| {
            input.set_content(format!("{threshold:.2}"), cx)
        });
        editor.preview = None;
        editor.status = None;
        cx.notify();
    }

    pub(super) fn set_auto_prompt_enabled(
        &mut self,
        id: Uuid,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(rule) = self
            .state
            .auto_prompts
            .iter_mut()
            .find(|rule| rule.id == id)
        {
            if !enabled || rule.valid_for_dispatch_without_enabled() {
                rule.enabled = enabled;
                self.save();
                cx.notify();
            }
        }
    }

    pub(super) fn enabled_auto_prompt_for_content(&self, content: &str) -> Option<(Uuid, String)> {
        let (name, prompt) = content
            .strip_prefix("[Auto prompt: ")?
            .split_once("]\n\n")?;
        let mut matches = self.state.auto_prompts.iter().filter(|rule| {
            rule.enabled && rule.name.replace(['\n', '\r'], " ") == name && rule.prompt == prompt
        });
        let rule = matches.next()?;
        matches
            .next()
            .is_none()
            .then(|| (rule.id, rule.name.clone()))
    }

    fn remove_auto_prompt(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.state.auto_prompts.retain(|rule| rule.id != id);
        if self
            .auto_prompt_editor
            .as_ref()
            .is_some_and(|editor| editor.id == id)
        {
            self.auto_prompt_editor = None;
            self.auto_prompt_sensitivity_slider.cancel();
        }
        self.save();
        cx.notify();
    }

    fn collect_auto_prompt_rule(&self, cx: &App) -> Result<AutoPromptRule, String> {
        let editor = self
            .auto_prompt_editor
            .as_ref()
            .ok_or_else(|| tr!("auto_prompts.no_editor"))?;
        let content = |field: &Entity<TextInput>| field.read(cx).content().trim().to_owned();
        let name = content(&editor.name).replace(['\n', '\r'], " ");
        let prompt = content(&editor.prompt);
        if name.is_empty() || prompt.is_empty() {
            return Err(tr!("auto_prompts.need_prompt"));
        }
        let questions = editor
            .questions
            .iter()
            .map(|question| {
                let instructions = content(&question.instructions);
                if instructions.is_empty() {
                    return Err(tr!("auto_prompts.need_questions"));
                }
                let raw_weight = content(&question.weight);
                let weight = if raw_weight.is_empty() {
                    None
                } else {
                    let value = raw_weight
                        .parse::<f64>()
                        .map_err(|_| tr!("auto_prompts.invalid_weight"))?;
                    if !value.is_finite() || value <= 0.0 {
                        return Err(tr!("auto_prompts.invalid_weight"));
                    }
                    Some(value)
                };
                Ok(AutoPromptQuestion {
                    id: question.id,
                    instructions,
                    weight,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let raw_threshold = content(&editor.threshold);
        let threshold = if raw_threshold.is_empty() {
            None
        } else {
            let value = raw_threshold
                .parse::<f64>()
                .map_err(|_| tr!("auto_prompts.invalid_threshold"))?;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(tr!("auto_prompts.invalid_threshold"));
            }
            Some(value)
        };
        Ok(AutoPromptRule {
            id: editor.id,
            name,
            prompt,
            questions,
            threshold,
            enabled: false,
        })
    }

    fn persist_auto_prompt_rule(&mut self, rule: AutoPromptRule, cx: &mut Context<Self>) {
        if let Some(existing) = self
            .state
            .auto_prompts
            .iter_mut()
            .find(|existing| existing.id == rule.id)
        {
            *existing = rule;
        } else {
            self.state.auto_prompts.push(rule);
        }
        self.save();
        if let Some(editor) = self.auto_prompt_editor.as_mut() {
            editor.status = Some(Ok(tr!("auto_prompts.saved_disabled")));
        }
        cx.notify();
    }

    fn save_auto_prompt_editor(&mut self, cx: &mut Context<Self>) {
        let rule = match self.collect_auto_prompt_rule(cx) {
            Ok(rule) => rule,
            Err(error) => {
                if let Some(editor) = self.auto_prompt_editor.as_mut() {
                    editor.status = Some(Err(error));
                }
                cx.notify();
                return;
            }
        };
        if rule.threshold.is_none()
            || rule
                .questions
                .iter()
                .any(|question| question.weight.is_none())
        {
            self.persist_auto_prompt_rule(rule, cx);
            self.suggest_auto_prompt_values(true, cx);
        } else {
            self.persist_auto_prompt_rule(rule, cx);
        }
    }

    fn suggest_auto_prompt_values(&mut self, save_after: bool, cx: &mut Context<Self>) {
        let rule = match self.collect_auto_prompt_rule(cx) {
            Ok(rule) => rule,
            Err(error) => {
                if let Some(editor) = self.auto_prompt_editor.as_mut() {
                    editor.status = Some(Err(error));
                }
                cx.notify();
                return;
            }
        };
        let Some(editor) = self.auto_prompt_editor.as_mut() else {
            return;
        };
        if editor.pending {
            return;
        }
        let mut questions = BTreeMap::new();
        for question in rule
            .questions
            .iter()
            .filter(|question| question.weight.is_none())
        {
            questions.insert(format!("weight:{}", question.id), waku_protocol::eval::EvalQuestion::Choice {
                instructions: format!("For the auto prompt in `prompt`, how important is this question relative to the other questions in `questions` for deciding whether to send it: {}", question.instructions),
                criteria: BTreeMap::from([
                    ("1".to_owned(), Some("Supporting evidence".to_owned())),
                    ("2".to_owned(), Some("Important evidence".to_owned())),
                    ("3".to_owned(), Some("Essential evidence".to_owned())),
                ]),
            });
        }
        if rule.threshold.is_none() {
            questions.insert("threshold".to_owned(), waku_protocol::eval::EvalQuestion::Choice {
                instructions: "Given `prompt` and every question in `questions`, how strong should the combined evidence be before this prompt is sent automatically?".to_owned(),
                criteria: BTreeMap::from([
                    ("0.65".to_owned(), Some("A missed opportunity costs more than an unnecessary follow-up".to_owned())),
                    ("0.80".to_owned(), Some("A balanced default for an automatic follow-up".to_owned())),
                    ("0.90".to_owned(), Some("An unnecessary follow-up is especially costly".to_owned())),
                ]),
            });
        }
        if questions.is_empty() {
            editor.status = Some(Ok(tr!("auto_prompts.values_complete")));
            cx.notify();
            return;
        }
        editor.pending = true;
        editor.status = None;
        let daemon = self.daemon.client();
        let state = serde_json::json!({ "prompt": rule.prompt, "questions": rule.questions.iter().map(|question| &question.instructions).collect::<Vec<_>>() });
        let suggest = cx.background_executor().spawn(async move {
            daemon
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::Evaluate {
                        state,
                        questions,
                        feature: Some("auto-prompt-suggest".to_owned()),
                        timeout_secs: None,
                    },
                )
                .map_err(|error| format!("{error:#}"))
        });
        cx.spawn(async move |this, cx| {
            let result = suggest.await;
            let _ = this.update(cx, |this, cx| {
                let unchanged = this.collect_auto_prompt_rule(cx).ok().as_ref() == Some(&rule);
                let Some(editor) = this
                    .auto_prompt_editor
                    .as_mut()
                    .filter(|editor| editor.id == rule.id)
                else {
                    return;
                };
                editor.pending = false;
                if !unchanged {
                    editor.status = Some(Err(tr!("auto_prompts.changed_during_suggestion")));
                    cx.notify();
                    return;
                }
                let outcome = result.and_then(|payload| match payload {
                    waku_client::ResponsePayload::Evaluation { evaluation } => Ok(evaluation),
                    _ => Err("the daemon returned an invalid evaluation response".to_owned()),
                });
                match outcome {
                    Ok(evaluation) => {
                        let mut completed = rule.clone();
                        for question in &mut completed.questions {
                            if question.weight.is_none() {
                                let key = format!("weight:{}", question.id);
                                question.weight =
                                    confident_auto_prompt_suggestion(evaluation.answers.get(&key));
                            }
                        }
                        if completed.threshold.is_none() {
                            completed.threshold = confident_auto_prompt_suggestion(
                                evaluation.answers.get("threshold"),
                            );
                        }
                        if !completed.valid_for_dispatch_without_enabled() {
                            editor.status = Some(Err(tr!("auto_prompts.suggest_incomplete")));
                        } else {
                            editor.advanced = true;
                            for question in &editor.questions {
                                if let Some(value) = completed
                                    .questions
                                    .iter()
                                    .find(|candidate| candidate.id == question.id)
                                    .and_then(|candidate| candidate.weight)
                                {
                                    question.weight.update(cx, |input, cx| {
                                        input.set_content(format!("{value:.0}"), cx)
                                    });
                                }
                            }
                            if let Some(value) = completed.threshold {
                                editor.threshold.update(cx, |input, cx| {
                                    input.set_content(format!("{value:.2}"), cx)
                                });
                            }
                            editor.status = Some(Ok(tr!("auto_prompts.suggested")));
                            if save_after {
                                this.persist_auto_prompt_rule(completed, cx);
                            }
                        }
                    }
                    Err(error) => editor.status = Some(Err(error)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn preview_auto_prompt(&mut self, cx: &mut Context<Self>) {
        let rule = match self.collect_auto_prompt_rule(cx) {
            Ok(rule) if rule.valid_for_dispatch_without_enabled() => rule,
            Ok(_) => {
                if let Some(editor) = self.auto_prompt_editor.as_mut() {
                    editor.status = Some(Err(tr!("auto_prompts.need_values")));
                }
                cx.notify();
                return;
            }
            Err(error) => {
                if let Some(editor) = self.auto_prompt_editor.as_mut() {
                    editor.status = Some(Err(error));
                }
                cx.notify();
                return;
            }
        };
        let Some(session) = self
            .state
            .selected_session
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
        else {
            if let Some(editor) = self.auto_prompt_editor.as_mut() {
                editor.status = Some(Err(tr!("auto_prompts.need_task")));
            }
            cx.notify();
            return;
        };
        let Some(turn) = session
            .turns
            .last()
            .filter(|turn| turn.status == TurnStatus::Completed)
        else {
            if let Some(editor) = self.auto_prompt_editor.as_mut() {
                editor.status = Some(Err(tr!("auto_prompts.need_completed_turn")));
            }
            cx.notify();
            return;
        };
        let state = waku_protocol::auto_prompts::turn_state(session, turn.id);
        let questions = rule
            .questions
            .iter()
            .map(|question| {
                (
                    question.id.to_string(),
                    waku_protocol::eval::EvalQuestion::Noul {
                        instructions: question.instructions.clone(),
                        criteria: None,
                    },
                )
            })
            .collect();
        let client = self.daemon.client();
        let session_id = session.id;
        if let Some(editor) = self.auto_prompt_editor.as_mut() {
            editor.pending = true;
            editor.status = None;
            editor.preview = None;
        }
        let preview = cx.background_executor().spawn(async move {
            client
                .request(
                    Uuid::nil(),
                    session_id,
                    waku_client::Command::Evaluate {
                        state,
                        questions,
                        feature: Some("auto-prompt-preview".to_owned()),
                        timeout_secs: None,
                    },
                )
                .map_err(|error| format!("{error:#}"))
        });
        cx.spawn(async move |this, cx| {
            let result = preview.await;
            let _ = this.update(cx, |this, cx| {
                let Some(editor) = this
                    .auto_prompt_editor
                    .as_mut()
                    .filter(|editor| editor.id == rule.id)
                else {
                    return;
                };
                editor.pending = false;
                match result {
                    Ok(waku_client::ResponsePayload::Evaluation { evaluation }) => {
                        let mut answers = Vec::new();
                        let mut weighted = 0.0;
                        let mut total = 0.0;
                        for question in &rule.questions {
                            let Some(waku_protocol::eval::EvalAnswer::Noul { noul }) =
                                evaluation.answers.get(&question.id.to_string())
                            else {
                                editor.status = Some(Err(tr!("auto_prompts.preview_incomplete")));
                                cx.notify();
                                return;
                            };
                            answers.push((question.instructions.clone(), *noul));
                            let weight = question.weight.unwrap_or(1.0);
                            weighted += weight * *noul;
                            total += weight;
                        }
                        let score = weighted / total;
                        editor.preview =
                            Some((answers, score, score >= rule.threshold.unwrap_or(1.0)));
                    }
                    Ok(_) => {
                        editor.status = Some(Err(
                            "the daemon returned an invalid evaluation response".to_owned(),
                        ))
                    }
                    Err(error) => editor.status = Some(Err(error)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_suggested_prompt_editor(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = action_predictions::suggested_prompt(
            id,
            &self.state.suggested_prompts,
            Some("{option}"),
        ) else {
            return;
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .multi_line()
                .auto_height()
                .max_lines(10)
                .accessibility_label(tr!("suggestions.prompt_text"));
            input.set_content(prompt, cx);
            input
        });
        let focus = input.read(cx).focus_handle(cx);
        self.suggested_prompt_editor = Some(SuggestedPromptEditor {
            id,
            input,
            error: None,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    fn save_suggested_prompt_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.suggested_prompt_editor else {
            return;
        };
        let id = editor.id;
        let prompt = editor.input.read(cx).content().trim().to_owned();
        if !action_predictions::valid_suggested_prompt(id, &prompt) {
            self.suggested_prompt_editor.as_mut().unwrap().error = Some(
                if id == action_predictions::CHOICE_PROMPT_ID
                    && prompt.matches("{option}").count() != 1
                {
                    tr!(
                        "suggestions.option_placeholder_required",
                        option = "{option}"
                    )
                } else {
                    tr!("suggestions.prompt_invalid")
                },
            );
            cx.notify();
            return;
        }
        if id != action_predictions::CHOICE_PROMPT_ID
            && action_predictions::CANNED_PROMPTS.iter().any(|(other, _)| {
                *other != id
                    && action_predictions::suggested_prompt(
                        other,
                        &self.state.suggested_prompts,
                        None,
                    )
                    .is_some_and(|value| value.trim().eq_ignore_ascii_case(&prompt))
            })
        {
            self.suggested_prompt_editor.as_mut().unwrap().error =
                Some(tr!("suggestions.prompt_duplicate"));
            cx.notify();
            return;
        }
        if action_predictions::default_suggested_prompt(id).as_deref() == Some(prompt.as_str()) {
            self.state.suggested_prompts.remove(id);
        } else {
            self.state.suggested_prompts.insert(id.to_owned(), prompt);
        }
        self.suggested_prompt_editor = None;
        self.save();
        cx.notify();
    }

    fn reset_suggested_prompt(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.state.suggested_prompts.remove(id);
        if self
            .suggested_prompt_editor
            .as_ref()
            .is_some_and(|editor| editor.id == id)
        {
            self.suggested_prompt_editor = None;
        }
        self.save();
        cx.notify();
    }

    fn render_suggested_prompts_settings(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let entries = action_predictions::CANNED_PROMPTS.iter().copied().chain([(
            action_predictions::CHOICE_PROMPT_ID,
            "suggestions.choice_template",
        )]);
        let rows = entries
            .map(|(id, label_key)| {
                let title = tr!(label_key);
                let prompt = action_predictions::suggested_prompt(
                    id,
                    &self.state.suggested_prompts,
                    Some("{option}"),
                )?;
                let preview: String = prompt.chars().take(120).collect();
                let description = if prompt.chars().count() > 120 {
                    format!("{preview}…")
                } else {
                    preview
                };
                let controls = div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .child(settings_button(
                        format!("edit-suggested-prompt-{id}"),
                        tr!("suggestions.edit_prompt"),
                        true,
                        false,
                        true,
                        theme,
                        cx,
                        move |this, window, cx| this.open_suggested_prompt_editor(id, window, cx),
                    ))
                    .child(settings_button(
                        format!("reset-suggested-prompt-{id}"),
                        tr!("suggestions.reset_prompt"),
                        self.state.suggested_prompts.contains_key(id),
                        false,
                        true,
                        theme,
                        cx,
                        move |this, _, cx| this.reset_suggested_prompt(id, cx),
                    ));
                settings_row(
                    "icons/sparkle.svg",
                    title,
                    description,
                    controls,
                    theme,
                    search,
                )
            })
            .collect::<Vec<_>>();
        let mut cards = Vec::new();
        if let Some(rows) = settings_row_card(rows, theme) {
            cards.push(rows.into_any_element());
        }
        if !search.active() {
            if let Some(editor) = &self.suggested_prompt_editor {
                let form = div()
                    .w_full()
                    .px(px(20.0))
                    .py(px(15.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .flex_col()
                    .gap(px(9.0))
                    .child(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!("suggestions.edit_prompt")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .text_color(theme.text_tertiary)
                            .child(if editor.id == action_predictions::CHOICE_PROMPT_ID {
                                tr!(
                                    "suggestions.choice_template_description",
                                    option = "{option}"
                                )
                            } else {
                                tr!("suggestions.prompt_editor_description")
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .px(px(8.0))
                            .py(px(6.0))
                            .rounded(px(8.0))
                            .border(hairline())
                            .border_color(theme.border_strong)
                            .bg(theme.inset)
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .child(editor.input.clone()),
                    )
                    .when_some(editor.error.as_ref(), |card, error| {
                        card.child(
                            div()
                                .text_size(sp(12.0))
                                .text_color(theme.danger)
                                .child(error.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(settings_button(
                                "cancel-suggested-prompt-editor",
                                tr!("commands.cancel"),
                                true,
                                false,
                                true,
                                theme,
                                cx,
                                |this, _, cx| {
                                    this.suggested_prompt_editor = None;
                                    cx.notify();
                                },
                            ))
                            .child(settings_button(
                                "save-suggested-prompt-editor",
                                tr!("commands.save"),
                                true,
                                false,
                                true,
                                theme,
                                cx,
                                |this, _, cx| this.save_suggested_prompt_editor(cx),
                            )),
                    );
                cards.push(form.into_any_element());
            }
        }
        settings_group(tr!("suggestions.settings_title"), cards, theme)
    }

    fn experiment_card(
        &self,
        experiment: &ExperimentDef,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let title = tr!(experiment.title_key);
        let description = tr!(experiment.description_key);
        let matched = search.matched(&title, &description)?;
        let enabled = experiment.enabled;
        let set = experiment.set;
        let toggle = toggle_switch(
            experiment.id,
            enabled,
            false,
            theme,
            cx,
            move |this, _, cx| set(this, !enabled, cx),
        );
        // The hint reads the local daemon's eval document — the one the Jev
        // page edits — so its fix button repairs exactly the state it shows.
        let eval_hint = experiment.eval_backed
            && enabled
            && self
                .state
                .eval
                .as_ref()
                .is_none_or(|eval| eval.credential_missing());
        Some(
            div()
                .min_h(px(66.0))
                .px(px(20.0))
                .py(px(13.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .flex_col()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(24.0))
                        .child(settings_row_label(
                            experiment.icon,
                            settings_row_text(title, description, matched, theme)
                                .whitespace_normal(),
                            theme,
                        ))
                        .child(toggle),
                )
                .when(eval_hint, |card| {
                    card.child(
                        div()
                            .mt(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("icons/alert.svg", 12.0, theme.warning))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(sp(12.0))
                                    .line_height(sp(16.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("experiments.needs_eval_backend")),
                            )
                            .child(settings_button(
                                format!("{}-open-jev-settings", experiment.id),
                                tr!("experiments.open_jev_settings"),
                                true,
                                false,
                                true,
                                theme,
                                cx,
                                |this, window, cx| {
                                    this.open_settings_page(SettingsPage::Jev, window, cx);
                                },
                            )),
                    )
                })
                .when_some(experiment.tuning.filter(|_| enabled), |card, tuning| {
                    card.child(tuning(self, theme, cx))
                })
                .into_any_element(),
        )
    }

    /// The Guided reading experiment's three parameters: fixation 1–5,
    /// saccade 10–50 in tens, opacity 0–100.
    /// Fixation and saccade change shaped widths, so their commits remeasure
    /// like a font-size change; opacity is paint-only.
    fn guided_reading_tuning(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let row = |label: String, slider: Stateful<Div>, shown: String| {
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(64.0))
                        .flex_none()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(label),
                )
                .child(slider.w(px(140.0)).flex_none())
                .child(
                    div()
                        .w(px(32.0))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(shown),
                )
        };

        let fixation = self.state.guided_reading_fixation;
        let fixation_shown = self
            .guided_reading_fixation_slider
            .shown((fixation - 1) as f32);
        let fixation_slider = slider::slider(
            "guided-reading-fixation-slider",
            &self.guided_reading_fixation_slider,
            4.0,
            (fixation - 1) as f32,
            cx,
            |this, value, window, cx| this.set_guided_reading_fixation(value, window, cx),
        );
        let saccade = self.state.guided_reading_saccade;
        let saccade_shown = self
            .guided_reading_saccade_slider
            .shown((saccade / 10 - 1) as f32);
        let saccade_slider = slider::slider(
            "guided-reading-saccade-slider",
            &self.guided_reading_saccade_slider,
            4.0,
            (saccade / 10 - 1) as f32,
            cx,
            |this, value, window, cx| this.set_guided_reading_saccade(value, window, cx),
        );
        let opacity = self.state.guided_reading_opacity;
        let opacity_shown = self.guided_reading_opacity_slider.shown(opacity as f32);
        let opacity_slider = slider::slider(
            "guided-reading-opacity-slider",
            &self.guided_reading_opacity_slider,
            100.0,
            opacity as f32,
            cx,
            |this, value, window, cx| this.set_guided_reading_opacity(value, window, cx),
        );

        div()
            .mt(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(row(
                tr!("experiments.guided_reading_fixation"),
                fixation_slider,
                format!("{}", fixation_shown.round() as i32 + 1),
            ))
            .child(row(
                tr!("experiments.guided_reading_saccade"),
                saccade_slider,
                format!("{}", (saccade_shown.round() as i32 + 1) * 10),
            ))
            .child(row(
                tr!("experiments.guided_reading_opacity"),
                opacity_slider,
                format!("{}%", opacity_shown.round() as i32),
            ))
            .into_any_element()
    }

    /// The renderer's guided-reading parameters while the experiment is on.
    pub(super) fn guided_reading(&self) -> Option<md::render::GuidedReading> {
        self.state
            .guided_reading_enabled
            .then_some(md::render::GuidedReading {
                fixation: self.state.guided_reading_fixation,
                saccade: self.state.guided_reading_saccade,
                opacity: self.state.guided_reading_opacity,
            })
    }

    fn set_guided_reading_fixation(
        &mut self,
        value: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let level = (value.round() as i32 + 1).clamp(1, 5) as u8;
        if self.state.guided_reading_fixation == level {
            return;
        }
        self.state.guided_reading_fixation = level;
        self.remeasure_font_sized_surfaces();
        self.save();
        cx.notify();
    }

    fn set_guided_reading_saccade(
        &mut self,
        value: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saccade = ((value.round() as i32 + 1) * 10).clamp(10, 50) as u8;
        if self.state.guided_reading_saccade == saccade {
            return;
        }
        self.state.guided_reading_saccade = saccade;
        self.remeasure_font_sized_surfaces();
        self.save();
        cx.notify();
    }

    fn set_guided_reading_opacity(
        &mut self,
        value: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opacity = (value.round() as i32).clamp(0, 100) as u8;
        if self.state.guided_reading_opacity == opacity {
            return;
        }
        self.state.guided_reading_opacity = opacity;
        self.save();
        cx.notify();
    }

    fn set_big_picture_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.dismiss_big_picture(cx);
        }
        self.state.big_picture_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_git_panel_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.close_git_panel_state();
        }
        self.state.git_panel_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_github_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.notifications.reset();
        }
        self.state.github_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_projects_page_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.close_projects_page(cx);
        }
        self.state.projects_page_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The Review tab's opt-in sits under the Projects page's: disabling
    /// folds any open Review tab back to Issues so the surface is never
    /// reachable-but-dead.
    fn set_review_queue_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            for state in self.projects_page_states.values_mut() {
                if state.tab == projects::ProjectsTab::Review {
                    state.tab = projects::ProjectsTab::Issues;
                }
            }
        }
        self.state.review_queue_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_subagents_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.subagents_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The project-map experiment opt-in is daemon-owned like subagents: the
    /// flag travels with the settings document `save()` already syncs, and
    /// only sessions started afterwards pick it up.
    fn set_project_map_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.project_map_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The Computer Use experiment opt-in also decides whether its dedicated
    /// settings page appears in navigation. Disabling the experiment turns the
    /// feature flag itself off so new sessions stop registering the Cua bridge.
    fn set_computer_use_experiment_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.computer_use_experiment_enabled = enabled;
        if !enabled {
            self.state.computer_use_enabled = false;
            self.settings_navigation.remove(SettingsPage::ComputerUse);
        }
        self.save();
        cx.notify();
    }

    fn set_automations_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.automations_enabled = enabled;
        // Disabling folds the page and any open editor so the surface is
        // never reachable-but-dead.
        self.apply_automations_enabled(cx);
        self.save();
        cx.notify();
    }

    fn set_friends_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled && self.settings_page == Some(SettingsPage::Friends) {
            self.settings_page = None;
        }
        if !enabled {
            self.settings_navigation.remove(SettingsPage::Friends);
        }
        self.state.friends_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// An open Jev page unmounts when the last eval-backed experiment goes
    /// off — its navigation row is gone too. Enabling anything leaves it.
    fn close_jev_page_if_unused(&mut self) {
        if !self.jev_in_use() && self.settings_page == Some(SettingsPage::Jev) {
            self.settings_page = None;
        }
        if !self.jev_in_use() {
            self.settings_navigation.remove(SettingsPage::Jev);
        }
    }

    fn set_model_router_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.model_router_enabled = enabled;
        self.close_jev_page_if_unused();
        if enabled {
            // The Jev page reads the eval mirror — warm it rather than
            // waiting for the first frame to discover it is missing.
            self.seed_eval_inputs(cx);
        }
        self.save();
        cx.notify();
    }

    fn set_status_markers_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.clear_status_markers();
        }
        self.state.status_markers_enabled = enabled;
        self.close_jev_page_if_unused();
        self.save();
        cx.notify();
    }

    fn set_action_predictions_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.clear_action_predictions();
        }
        self.state.action_predictions_enabled = enabled;
        self.close_jev_page_if_unused();
        self.save();
        cx.notify();
    }

    fn set_phase_routing_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.phase_routing_enabled = enabled;
        if !enabled {
            self.phase_eval_in_flight.clear();
        }
        self.close_jev_page_if_unused();
        if enabled {
            // The Jev page reads the eval mirror — warm it rather than
            // waiting for the first frame to discover it is missing.
            self.seed_eval_inputs(cx);
        }
        self.save();
        cx.notify();
    }

    fn set_sidebar_dock_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.sidebar_dock_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// Fixated text is wider than the prose it replaces, so toggling
    /// reflows wrapped rows — drop cached heights the way a font-size
    /// change does.
    fn set_guided_reading_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.guided_reading_enabled == enabled {
            return;
        }
        if !enabled {
            // The tuning sliders unmount with the toggle; drop any in-flight
            // drag so its release handler can't die mid-gesture.
            self.guided_reading_fixation_slider.cancel();
            self.guided_reading_saccade_slider.cancel();
            self.guided_reading_opacity_slider.cancel();
        }
        self.state.guided_reading_enabled = enabled;
        self.remeasure_font_sized_surfaces();
        self.save();
        cx.notify();
    }

    fn set_memory_experiment_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.memory_experiment_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The per-provider memory-distillation override: `None` restores the
    /// provider's advertised default model.
    fn set_memory_model(
        &mut self,
        provider: ProviderKind,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match model {
            Some(model) => {
                self.state.memory_models.insert(provider, model);
            }
            None => {
                self.state.memory_models.remove(&provider);
            }
        }
        self.save();
        cx.notify();
    }

    /// The voice briefing card's tuning block: the shared AI Gateway
    /// credential, the chat model that writes the transcript, and which
    /// Gemini TTS tier speaks it.
    fn voice_briefing_tuning(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let row = |label: String, control: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(110.0))
                        .flex_none()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(label),
                )
                .child(control)
        };

        let tts = self.state.voice_briefing_tts_model;
        let handle = self.menu_handle("voice-briefing-tts-model".to_owned(), cx);
        let weak = cx.entity().downgrade();
        let tts_selector = dropdown_menu(
            MenuChip::new("voice-briefing-tts-model")
                .label(tts.label())
                .outlined()
                .selected(handle.is_open())
                .w(px(200.0))
                .justify_between(),
            "voice-briefing-tts-model-menu",
            &handle,
            MenuAlign::BelowRight,
            move |_| {
                VoiceBriefingTtsModel::ALL
                    .into_iter()
                    .map(|option| {
                        let weak = weak.clone();
                        MenuItem::new(option.label(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_voice_briefing_tts_model(option, cx)
                            });
                        })
                        .selected(option == tts)
                    })
                    .collect()
            },
        );

        div()
            .mt(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(sp(11.5))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("experiments.voice_briefing_caption")),
            )
            .child(row(
                tr!("experiments.voice_briefing_key"),
                TextField::new("voice-briefing-key", self.voice_briefing_key_input.clone())
                    .w(px(280.0))
                    .into_any_element(),
            ))
            .child(row(
                tr!("experiments.voice_briefing_model"),
                TextField::new(
                    "voice-briefing-model",
                    self.voice_briefing_model_input.clone(),
                )
                .w(px(280.0))
                .into_any_element(),
            ))
            .child(row(
                tr!("experiments.voice_briefing_tts"),
                tts_selector.into_any_element(),
            ))
            .into_any_element()
    }

    fn set_voice_briefing_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.voice_briefing_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_voice_briefing_tts_model(
        &mut self,
        model: VoiceBriefingTtsModel,
        cx: &mut Context<Self>,
    ) {
        if self.state.voice_briefing_tts_model == model {
            return;
        }
        self.state.voice_briefing_tts_model = model;
        self.save();
        cx.notify();
    }

    /// The card's two text fields write straight into app state on each
    /// edit — a cleared model restores the default rather than storing a
    /// blank the pipeline's gate then reads as unconfigured.
    pub(super) fn save_voice_briefing_fields(&mut self, cx: &mut Context<Self>) {
        self.state.voice_briefing_gateway_key = self
            .voice_briefing_key_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        let model = self
            .voice_briefing_model_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        self.state.voice_briefing_summary_model = if model.is_empty() {
            crate::persistence::default_voice_briefing_summary_model()
        } else {
            model
        };
        self.save();
    }

    /// The project-memory card's tuning block: one model picker per enabled
    /// provider, governing only the daemon's background distillation runs.
    fn memory_tuning(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows = self
            .probes
            .iter()
            .filter(|probe| {
                probe.installed && !self.state.disabled_providers.contains(&probe.provider)
            })
            .map(|probe| self.memory_model_row(probe, theme, cx))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return div().into_any_element();
        }
        div()
            .mt(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(sp(11.5))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("experiments.memory_models_caption")),
            )
            .children(rows)
            .into_any_element()
    }

    /// One provider's distillation-model picker: provider mark and name on
    /// the left, a catalog dropdown on the right whose first entry clears
    /// the override back to the provider default.
    fn memory_model_row(&self, probe: &ProviderProbe, theme: Theme, cx: &mut Context<Self>) -> Div {
        let provider = probe.provider;
        let current = self.state.memory_models.get(&provider).cloned();
        let label = current
            .as_deref()
            .map(|id| {
                probe
                    .model(id)
                    .map(|model| model.name.clone())
                    .unwrap_or_else(|| id.to_owned())
            })
            .unwrap_or_else(|| tr!("experiments.memory_model_default"));
        let menu_id = format!("memory-model-{}", provider.id());
        let handle = self.menu_handle(menu_id.clone(), cx);
        let weak = cx.entity().downgrade();
        let models = probe.models.clone();
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(provider_mark(&theme, provider, 15.0, theme.text_secondary))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(provider.display_name()),
                    ),
            )
            .child(dropdown_menu(
                MenuChip::new(format!("memory-model-selector-{}", provider.id()))
                    .label(label)
                    .outlined()
                    .selected(handle.is_open())
                    .w(px(160.0))
                    .justify_between(),
                format!("memory-model-menu-{}", provider.id()),
                &handle,
                MenuAlign::BelowRight,
                move |_| {
                    let mut items = vec![{
                        let weak = weak.clone();
                        MenuItem::new(tr!("experiments.memory_model_default"), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_memory_model(provider, None, cx);
                            });
                        })
                        .selected(current.is_none())
                    }];
                    items.extend(models.iter().map(|model| {
                        let weak = weak.clone();
                        let id = model.id.clone();
                        let selected = current.as_deref() == Some(model.id.as_str());
                        MenuItem::new(model.name.clone(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_memory_model(provider, Some(id.clone()), cx);
                            });
                        })
                        .selected(selected)
                    }));
                    items
                },
            ))
    }

    /// The sandbox experiment opt-in is daemon-owned like subagents. Turning
    /// it off also clears remembered environment intent — a draft or
    /// `last_sandboxed` must not keep claiming the VM once the surface that
    /// chose it is hidden.
    fn set_sandbox_experiment_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.sandbox_experiment_enabled = enabled;
        if !enabled {
            self.state.last_environment = SessionEnvironment::Local;
            self.state.last_sandboxed = false;
            if let Some(session) = self.composer_session_mut()
                && !session.has_started()
            {
                session.environment = SessionEnvironment::Local;
            }
        }
        self.save();
        cx.notify();
    }

    /// Fill the evaluation credential fields from the daemon's settings
    /// mirror, once. After that the fields own their contents until Apply —
    /// re-seeding on every settings broadcast would eat in-progress edits.
    pub(super) fn seed_eval_inputs(&mut self, cx: &mut Context<Self>) {
        if self.eval_inputs_seeded {
            return;
        }
        self.eval_inputs_seeded = true;
        let eval = self.state.eval.clone().unwrap_or_default();
        for (input, value) in [
            (&self.eval_typesafe_key_input, eval.typesafe_api_key),
            (&self.eval_vercel_key_input, eval.vercel_api_key),
            (&self.eval_vercel_team_input, eval.vercel_team_id),
            (
                &self.eval_cloudflare_account_input,
                eval.cloudflare_account_id,
            ),
            (&self.eval_cloudflare_token_input, eval.cloudflare_api_token),
        ] {
            if let Some(value) = value {
                input.update(cx, |input, cx| input.set_content(value, cx));
            }
        }
    }

    fn set_eval_backend(
        &mut self,
        backend: waku_protocol::eval::EvalBackend,
        cx: &mut Context<Self>,
    ) {
        self.state.eval.get_or_insert_with(Default::default).backend = backend;
        self.save();
        cx.notify();
    }

    /// Whether any credential field differs from the mirror — the Apply
    /// button's enabled state.
    fn eval_credentials_dirty(&self, cx: &App) -> bool {
        let eval = self.state.eval.clone().unwrap_or_default();
        let content = |input: &Entity<TextInput>| {
            let content = input.read(cx).content().trim().to_owned();
            (!content.is_empty()).then_some(content)
        };
        content(&self.eval_typesafe_key_input) != eval.typesafe_api_key
            || content(&self.eval_vercel_key_input) != eval.vercel_api_key
            || content(&self.eval_vercel_team_input) != eval.vercel_team_id
            || content(&self.eval_cloudflare_account_input) != eval.cloudflare_account_id
            || content(&self.eval_cloudflare_token_input) != eval.cloudflare_api_token
    }

    /// Persist every credential field into the eval settings document — one
    /// write so the daemon sees a consistent set. An empty field clears the
    /// slot rather than storing whitespace.
    pub(super) fn save_eval_credentials(&mut self, cx: &mut Context<Self>) {
        let content = |input: &Entity<TextInput>| {
            let content = input.read(cx).content().trim().to_owned();
            (!content.is_empty()).then_some(content)
        };
        let eval = self.state.eval.get_or_insert_with(Default::default);
        eval.typesafe_api_key = content(&self.eval_typesafe_key_input);
        eval.vercel_api_key = content(&self.eval_vercel_key_input);
        eval.vercel_team_id = content(&self.eval_vercel_team_input);
        eval.cloudflare_account_id = content(&self.eval_cloudflare_account_input);
        eval.cloudflare_api_token = content(&self.eval_cloudflare_token_input);
        self.save();
        cx.notify();
    }

    /// The Jev page's "Test connection": one probe evaluation against the
    /// backend selected in the dropdown, using the field contents as staged —
    /// a configuration verifies before Apply persists it. The daemon owns the
    /// HTTP call; the answer lands on `eval_probe_result` and renders as the
    /// status text beside the button.
    fn verify_eval_connection(&mut self, cx: &mut Context<Self>) {
        if self.eval_probe_pending {
            return;
        }
        let content = |input: &Entity<TextInput>| {
            let content = input.read(cx).content().trim().to_owned();
            (!content.is_empty()).then_some(content)
        };
        let settings = waku_protocol::eval::EvalSettings {
            backend: self.state.eval.clone().unwrap_or_default().backend,
            typesafe_api_key: content(&self.eval_typesafe_key_input),
            vercel_api_key: content(&self.eval_vercel_key_input),
            vercel_team_id: content(&self.eval_vercel_team_input),
            cloudflare_account_id: content(&self.eval_cloudflare_account_input),
            cloudflare_api_token: content(&self.eval_cloudflare_token_input),
        };
        self.eval_probe_pending = true;
        self.eval_probe_result = None;
        let daemon = self.daemon.client();
        let probe = cx.background_executor().spawn(async move {
            daemon
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::TestEvalConnection { settings },
                )
                .map_err(|error| format!("{error:#}"))
                .and_then(|payload| match payload {
                    waku_client::ResponsePayload::Evaluation { evaluation } => {
                        Ok((evaluation.model, evaluation.latency_ms))
                    }
                    _ => Err("the daemon returned an invalid eval probe response".into()),
                })
        });
        cx.spawn(async move |this, cx| {
            let result = probe.await;
            let _ = this.update(cx, |this, cx| {
                this.eval_probe_pending = false;
                this.eval_probe_result = Some(result);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Refresh the Jev page's usage card: the daemon sums the decision log's
    /// recorded token usage and the answer lands on `eval_usage_stats`.
    /// Every page open re-scans, so a visit always sees the latest calls.
    pub(super) fn load_eval_usage_stats(&mut self, cx: &mut Context<Self>) {
        let daemon = self.daemon.client();
        let load = cx.background_executor().spawn(async move {
            daemon
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::LoadEvalUsage,
                )
                .ok()
                .and_then(|payload| match payload {
                    waku_client::ResponsePayload::EvalUsage { stats } => Some(stats),
                    _ => None,
                })
        });
        cx.spawn(async move |this, cx| {
            let stats = load.await;
            let _ = this.update(cx, |this, cx| {
                this.eval_usage_stats = stats;
                cx.notify();
            });
        })
        .detach();
    }

    /// The Jev page's routing configuration: the eval backend and its
    /// credentials, then the three class-level routes the class map resolves
    /// through. The daemon settings document stays authoritative — edits
    /// write through the settings mirror like every other field.
    fn render_model_routing_settings(
        &self,
        theme: Theme,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let eval = self.state.eval.clone().unwrap_or_default();
        let backend = eval.backend;

        let backend_handle = self.menu_handle("eval-backend-selector", cx);
        let backend_selector = dropdown_menu(
            MenuChip::new("eval-backend-selector")
                .label(eval_backend_label(backend))
                .outlined()
                .selected(backend_handle.is_open())
                .w(px(200.0))
                .justify_between(),
            "eval-backend-selector-menu",
            &backend_handle,
            MenuAlign::BelowRight,
            move |_| {
                [
                    waku_protocol::eval::EvalBackend::TypeSafe,
                    waku_protocol::eval::EvalBackend::VercelGateway,
                    waku_protocol::eval::EvalBackend::Cloudflare,
                ]
                .into_iter()
                .map(|option| {
                    let weak = weak.clone();
                    MenuItem::new(eval_backend_label(option), move |_, cx| {
                        let _ = weak.update(cx, |this, cx| this.set_eval_backend(option, cx));
                    })
                    .selected(option == backend)
                })
                .collect()
            },
        );

        let dirty = self.eval_credentials_dirty(cx);
        let apply_button = settings_button(
            "apply-eval-credentials",
            tr!("daemon.apply"),
            dirty,
            false,
            false,
            theme,
            cx,
            |this, _, cx| this.save_eval_credentials(cx),
        );

        let pending = self.eval_probe_pending;
        let test_button = settings_button(
            "test-eval-connection",
            tr!("routing.test_connection"),
            !pending,
            false,
            false,
            theme,
            cx,
            |this, _, cx| this.verify_eval_connection(cx),
        );

        let probe_status = if pending {
            Some((tr!("routing.testing"), theme.text_tertiary))
        } else {
            self.eval_probe_result.as_ref().map(|result| match result {
                Ok((model, latency)) => (
                    tr!("routing.connection_ok", model = model, latency = latency),
                    theme.success,
                ),
                Err(error) => (
                    tr!("routing.connection_failed", error = error),
                    theme.warning,
                ),
            })
        };
        let no_probe_status = probe_status.is_none();

        let mut credential_rows: Vec<Option<AnyElement>> = match backend {
            waku_protocol::eval::EvalBackend::TypeSafe => vec![settings_row(
                "icons/key-round.svg",
                tr!("routing.typesafe_key"),
                tr!("routing.typesafe_key_description"),
                TextField::new("eval-typesafe-key", self.eval_typesafe_key_input.clone())
                    .w(px(300.0)),
                theme,
                search,
            )],
            waku_protocol::eval::EvalBackend::VercelGateway => vec![
                settings_row(
                    "icons/key-round.svg",
                    tr!("routing.vercel_key"),
                    tr!("routing.vercel_key_description"),
                    TextField::new("eval-vercel-key", self.eval_vercel_key_input.clone())
                        .w(px(300.0)),
                    theme,
                    search,
                ),
                settings_row(
                    "icons/friends.svg",
                    tr!("routing.vercel_team"),
                    tr!("routing.vercel_team_description"),
                    TextField::new("eval-vercel-team", self.eval_vercel_team_input.clone())
                        .w(px(300.0)),
                    theme,
                    search,
                ),
            ],
            waku_protocol::eval::EvalBackend::Cloudflare => vec![
                settings_row(
                    "icons/globe.svg",
                    tr!("routing.cloudflare_account"),
                    tr!("routing.cloudflare_account_description"),
                    TextField::new(
                        "eval-cloudflare-account",
                        self.eval_cloudflare_account_input.clone(),
                    )
                    .w(px(300.0)),
                    theme,
                    search,
                ),
                settings_row(
                    "icons/key-round.svg",
                    tr!("routing.cloudflare_token"),
                    tr!("routing.cloudflare_token_description"),
                    TextField::new(
                        "eval-cloudflare-token",
                        self.eval_cloudflare_token_input.clone(),
                    )
                    .w(px(300.0)),
                    theme,
                    search,
                ),
            ],
        };

        let mut credential_rows_with_backend = vec![settings_row(
            "icons/server.svg",
            tr!("routing.backend"),
            tr!("routing.backend_description"),
            backend_selector,
            theme,
            search,
        )];
        credential_rows_with_backend.append(&mut credential_rows);
        let credentials = settings_row_card(credential_rows_with_backend, theme).map(|card| {
            card.mt(px(15.0)).when(!search.active(), |card| {
                card.child(
                    div()
                        .px(px(20.0))
                        .pb(px(13.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .when_some(probe_status, |row, (message, color)| {
                            row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(12.0))
                                    .text_color(color)
                                    .child(message),
                            )
                        })
                        .when(no_probe_status, |row| row.child(div().flex_1()))
                        .child(test_button)
                        .child(apply_button),
                )
            })
        });

        // The three class cards: each maps a task class to the provider/model
        // (and effort) Auto starts that kind of work on. An unmapped class
        // leaves the route on whatever was last used.
        let class_rows = [
            (
                TaskClass::Routine,
                tr!("routing.class_easy"),
                tr!("routing.class_easy_description"),
            ),
            (
                TaskClass::General,
                tr!("routing.class_medium"),
                tr!("routing.class_medium_description"),
            ),
            (
                TaskClass::Demanding,
                tr!("routing.class_hard"),
                tr!("routing.class_hard_description"),
            ),
        ];
        let class_row_elements: Vec<Option<AnyElement>> = class_rows
            .into_iter()
            .map(|(class, title, description)| {
                settings_row(
                    "icons/gauge.svg",
                    title,
                    description,
                    self.route_class_controls(class, cx),
                    theme,
                    search,
                )
            })
            .collect();
        let classes = settings_row_card(class_row_elements, theme).map(|card| {
            card.mt(px(15.0)).when(!search.active(), |card| {
                let has_status = self.route_suggest_result.is_some();
                let suggest_status =
                    self.route_suggest_result
                        .as_ref()
                        .map(|result| match result {
                            Ok(()) => (tr!("routing.suggest_done"), theme.success),
                            Err(error) => (error.clone(), theme.warning),
                        });
                card.child(
                    div()
                        .px(px(20.0))
                        .pb(px(13.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .when_some(suggest_status, |row, (message, color)| {
                            row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(12.0))
                                    .text_color(color)
                                    .child(message),
                            )
                        })
                        .when(!has_status, |row| row.child(div().flex_1()))
                        .child(settings_button(
                            "suggest-route-classes",
                            if self.route_suggest_pending {
                                tr!("routing.suggesting")
                            } else {
                                tr!("routing.suggest_defaults")
                            },
                            !self.route_suggest_pending,
                            false,
                            false,
                            theme,
                            cx,
                            |this, _, cx| this.suggest_route_class_defaults(cx),
                        )),
                )
            })
        });

        // Token usage summed from the daemon's decision log — the all-calls
        // total on top, then one row per feature that recorded calls. `None`
        // until the scan answers, and an empty log renders no card at all.
        let usage =
            self.eval_usage_stats
                .as_ref()
                .filter(|stats| stats.totals.calls > 0)
                .and_then(|stats| {
                    let untracked = stats.totals.calls - stats.totals.calls_with_usage;
                    let total_description = if untracked > 0 {
                        tr!(
                            "routing.usage_total_description_untracked",
                            calls = stats.totals.calls,
                            untracked = untracked
                        )
                    } else {
                        tr!(
                            "routing.usage_total_description",
                            calls = stats.totals.calls
                        )
                    };
                    let mut rows: Vec<Option<AnyElement>> = vec![settings_row(
                        "icons/chart-column.svg",
                        tr!("routing.usage_total"),
                        total_description,
                        eval_usage_label(&stats.totals, theme),
                        theme,
                        search,
                    )];
                    rows.extend(
                        stats.features.iter().map(|(feature, totals)| {
                            eval_feature_row(feature, totals, theme, search)
                        }),
                    );
                    settings_row_card(rows, theme).map(|card| card.mt(px(15.0)).into_any_element())
                });

        div()
            .children(credentials)
            .children(classes)
            .children(self.render_suggested_prompts_settings(theme, search, cx))
            .children(usage)
            .into_any_element()
    }

    /// The class row's control: the shared model picker — a searchable
    /// catalog of effort-granularity combos led by a "No override" stance
    /// row. The effort rides each combo now, so there is no separate
    /// effort dropdown.
    fn route_class_controls(&self, class: TaskClass, cx: &mut Context<Self>) -> AnyElement {
        let entry = self.state.route_classes.get(&class).cloned();
        self.route_class_selector(class, entry.as_ref(), cx)
    }

    /// The class picker's row list: the unmapped "No override" stance
    /// first, then favorites, recents, and provider blocks — each
    /// provider's own default heading its effort-granularity combos. The
    /// class map stores an effort but no service tier, so combos name
    /// model + effort only.
    pub(super) fn route_class_picker_rows(&self, normalized_query: &str) -> Vec<PickerRow> {
        picker_rows(
            &self.probes,
            &PickerRowSpec {
                leading: &[PolicyRowId::NoOverride],
                provider_defaults: true,
                granularity: PickerGranularity::Efforts,
                favorites: &self.state.favorite_models,
                pinned: &self.pinned_unfavorites,
                recents: &self.state.recent_model_uses,
                disabled_providers: &self.state.disabled_providers,
                locked_provider: None,
                normalized_query,
            },
        )
    }

    /// The row the class's mapping occupies — the seed the reveal and the
    /// first arrow press agree on. An unmapped class sits on the leading
    /// "No override" row.
    pub(super) fn route_class_selected_index(
        &self,
        class: TaskClass,
        rows: &[PickerRow],
    ) -> Option<usize> {
        let current = self.state.route_classes.get(&class);
        rows.iter()
            .position(|row| route_class_row_matches(row, current))
    }

    /// One class-level model picker: the shared picker panel — a "No
    /// override" stance leading favorites, recents, and provider blocks of
    /// effort-granularity combos, drawn through the same panel the
    /// composer uses. Selecting writes a `RouteClassTarget` into the class
    /// map.
    fn route_class_selector(
        &self,
        class: TaskClass,
        current: Option<&RouteClassTarget>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let menu_id = format!("route-class-{}", class.id());
        let search = self.route_class_picker.search.clone();
        let search_focus = search.read(cx).focus_handle(cx);
        let empty_focus = self.route_class_picker.empty_focus.clone();
        // The same set Auto may pick between — a class target naming an
        // uninstalled provider would resolve but could never start.
        let no_targets = self.route_class_probes().is_empty();
        let handle = {
            let reset_weak = weak.clone();
            let reset_search = search.clone();
            let picker_focus = search_focus.clone();
            let empty_picker_focus = empty_focus.clone();
            self.menu_handle_with(menu_id.clone(), cx, move |open, window, cx| {
                // The empty state draws no filter field, so the handle the
                // deferred focus below targets depends on which body opened.
                let mut empty = false;
                let _ = reset_weak.update(cx, |this, cx| {
                    if open {
                        this.route_class_open = Some(class);
                        empty = this.route_class_probes().is_empty();
                        // Opening re-runs catalog discovery for every provider
                        // the merged list can draw, so models authored since
                        // launch appear without a restart.
                        for kind in ProviderKind::ALL {
                            if picker_lists_provider(
                                &this.probes,
                                &this.state.disabled_providers,
                                None,
                                this.daemon.is_remote(),
                                kind,
                            ) {
                                this.refresh_provider_model_discovery(kind);
                            }
                        }
                        this.route_class_picker.highlight = None;
                        reset_search.update(cx, |search, cx| search.clear(cx));
                        this.reveal_route_class_target(class);
                    } else {
                        this.route_class_open = None;
                        let focus = this.settings_focus.clone();
                        window.focus(&focus, cx);
                    }
                    cx.notify();
                });
                if open {
                    // The panel is deferred, so its input joins the dispatch
                    // tree only after the deferred draw — the same two-frame
                    // wait the model picker needs before it can take focus.
                    // The reveal is re-issued here too: a parked scroll
                    // request resolves against the previous paint's bounds,
                    // so on the container's first-ever paint it lands wrong.
                    let picker_focus = if empty {
                        empty_picker_focus.clone()
                    } else {
                        picker_focus.clone()
                    };
                    let focus_weak = reset_weak.clone();
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| {
                            window.focus(&picker_focus, cx);
                            let _ = focus_weak.update(cx, |this, _cx| {
                                this.reveal_route_class_target(class);
                            });
                        });
                    });
                }
            })
        };

        let normalized_query = self
            .route_class_picker
            .search
            .read(cx)
            .content()
            .trim()
            .to_ascii_lowercase();
        let searching = !normalized_query.is_empty();
        let remote = self.daemon.is_remote();
        let probes = self.probes.clone();
        let disabled_providers = self.state.disabled_providers.clone();
        let current = current.cloned();
        let label = route_class_target_label(current.as_ref(), &self.probes);
        // One ordering shared by the rendered rows and `enter`'s handler —
        // the same rule the composer documents for its own list. Only built
        // while the panel is open: it clones every provider's model list.
        let available_rows = Rc::new(if handle.is_open() {
            self.route_class_picker_rows(&normalized_query)
        } else {
            Vec::new()
        });
        // Which sections the rail offers comes from the unfiltered list —
        // a rail button's jump clears the query before scrolling.
        let section_rows = if searching && handle.is_open() {
            Rc::new(self.route_class_picker_rows(""))
        } else {
            available_rows.clone()
        };
        let highlight = self
            .route_class_picker
            .highlight
            .filter(|index| *index < available_rows.len());
        let list_state = self.route_class_picker.list.clone();
        let scrollbar_state = self.route_class_picker.scrollbar.clone();
        let render_empty_focus = empty_focus.clone();

        popover(
            MenuChip::new(format!("route-class-selector-{}", class.id()))
                .label(label)
                .outlined()
                .selected(handle.is_open())
                .w(px(240.0))
                .justify_between(),
            &handle,
            MenuAlign::BelowRight,
            move |popover, _window, _cx| {
                let popover = popover.clone();
                if no_targets {
                    return model_picker_empty_state(
                        &theme,
                        &render_empty_focus,
                        popover,
                        weak.clone(),
                    );
                }

                // The composer's own rail: the leading stance gets the
                // first jump, favorites and recents jump when those
                // sections have rows, and provider buttons filter through a
                // `provider:<id>` token — the same contract the composer's
                // rail keeps.
                let mut rail_sections = vec![picker_section_rail_item(
                    "route-class-rail-unmapped",
                    icon("icons/sparkle.svg", 17.0, theme.text_tertiary).into_any_element(),
                    PickerSection::Policies,
                    move |this| this.route_class_picker_rows(""),
                    route_class_picker_state,
                )];
                if section_rows
                    .iter()
                    .any(|row| picker_row_section(row) == PickerSection::Favorites)
                {
                    rail_sections.push(picker_section_rail_item(
                        "route-class-rail-favorites",
                        icon("icons/star.svg", 17.0, theme.text_tertiary).into_any_element(),
                        PickerSection::Favorites,
                        move |this| this.route_class_picker_rows(""),
                        route_class_picker_state,
                    ));
                }
                if section_rows
                    .iter()
                    .any(|row| picker_row_section(row) == PickerSection::Recents)
                {
                    rail_sections.push(picker_section_rail_item(
                        "route-class-rail-recents",
                        icon("icons/hourglass.svg", 17.0, theme.text_tertiary).into_any_element(),
                        PickerSection::Recents,
                        move |this| this.route_class_picker_rows(""),
                        route_class_picker_state,
                    ));
                }
                let rail_providers = ProviderKind::ALL
                    .into_iter()
                    .filter(|kind| {
                        picker_lists_provider(&probes, &disabled_providers, None, remote, *kind)
                            && section_rows.iter().any(|row| {
                                picker_row_section(row) == PickerSection::Provider(*kind)
                            })
                    })
                    .map(|kind| {
                        let active = normalized_query.split_whitespace().any(|token| {
                            token
                                .strip_prefix("provider:")
                                .is_some_and(|value| value == kind.id())
                        });
                        picker_provider_rail_item(
                            kind,
                            provider_mark(&theme, kind, 18.0, theme.text_tertiary)
                                .into_any_element(),
                            active,
                            route_class_picker_state,
                        )
                    })
                    .collect();

                let render_row = Rc::new({
                    let weak = weak.clone();
                    let current = current.clone();
                    let render = move |row_index: usize,
                                       row: &PickerRow,
                                       is_highlighted: bool,
                                       popover: &ContextMenuHandle,
                                       _window: &mut Window,
                                       cx: &mut App|
                          -> AnyElement {
                        let theme = Theme::current(cx);
                        let is_selected = route_class_row_matches(row, current.as_ref());
                        let (mark, title, subtitle) = match row {
                            PickerRow::Policy(policy) => policy_row_parts(*policy, &theme),
                            PickerRow::ProviderDefault(provider) => (
                                provider_mark(&theme, *provider, 12.0, theme.text_tertiary)
                                    .into_any_element(),
                                provider.short_name().to_owned(),
                                tr!("routing.provider_default"),
                            ),
                            PickerRow::Combo(row) => (
                                provider_mark(&theme, row.provider, 12.0, theme.text_tertiary)
                                    .into_any_element(),
                                row.model
                                    .name_i18n
                                    .as_ref()
                                    .map(waku_client::WireTranslation::render)
                                    .unwrap_or_else(|| row.model.name.clone()),
                                model_picker_subtitle(
                                    row.provider,
                                    row.model.sub_provider.as_deref(),
                                ),
                            ),
                        };
                        let effort_label = match row {
                            PickerRow::Combo(row) => row.effort.as_deref().and_then(|effort| {
                                row.model
                                    .reasoning_efforts
                                    .iter()
                                    .find(|option| option.id == effort)
                                    .map(|option| {
                                        option
                                            .label_i18n
                                            .as_ref()
                                            .map(waku_client::WireTranslation::render)
                                            .unwrap_or_else(|| option.label.clone())
                                    })
                            }),
                            _ => None,
                        };
                        let row = row.clone();
                        let select_weak = weak.clone();
                        let select_popover = popover.clone();
                        model_picker_row_shell(
                            SharedString::from(format!("route-class-row-{row_index}")),
                            is_selected,
                            is_highlighted,
                            &theme,
                        )
                        .child(model_picker_row_body(
                            title,
                            effort_label.map(|label| {
                                div()
                                    .flex_none()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .child(SharedString::from(label))
                                    .into_any_element()
                            }),
                            mark,
                            subtitle,
                            &theme,
                        ))
                        .when(is_selected, |element| {
                            element.child(icon("icons/check.svg", 13.0, theme.accent))
                        })
                        .on_click(move |_, window, cx| {
                            let _ = select_weak.update(cx, |this, cx| {
                                this.apply_route_class_row(class, &row, cx);
                            });
                            select_popover.close(window, cx);
                        })
                        .into_any_element()
                    };
                    render
                });

                model_picker_panel(
                    ModelPickerPanel {
                        rows: available_rows.clone(),
                        search: search.clone(),
                        list_state: list_state.clone(),
                        scrollbar_state: scrollbar_state.clone(),
                        highlight,
                        empty_label: tr!("models.none_found").into(),
                        rail_sections,
                        rail_providers,
                        render_row,
                        on_move: Rc::new(move |this, key, rows, cx| {
                            // The first arrow walks from the class's current
                            // target row, the way the reveal landed.
                            let seed = this.route_class_selected_index(class, rows);
                            if this
                                .route_class_picker
                                .move_highlight(seed, rows.len(), key)
                                .is_some()
                            {
                                cx.notify();
                            }
                        }),
                        on_confirm: Rc::new(move |this, rows, cx| {
                            if let Some(row) = rows
                                .get(this.route_class_picker.highlight.unwrap_or(0))
                                .cloned()
                            {
                                this.apply_route_class_row(class, &row, cx);
                            }
                        }),
                        on_cycle_section: Some(Rc::new(|this, key, cx| {
                            this.cycle_route_class_section(key, cx);
                        })),
                    },
                    &popover,
                    &theme,
                    &weak,
                )
            },
        )
        .into_any_element()
    }

    /// The providers the class pickers can offer — the same set Auto may
    /// pick between, since a class target naming an uninstalled provider
    /// would resolve but could never start.
    fn route_class_probes(&self) -> Vec<ProviderProbe> {
        self.probes
            .iter()
            .filter(|probe| {
                probe.installed && !self.state.disabled_providers.contains(&probe.provider)
            })
            .cloned()
            .collect()
    }

    /// Apply a picked row: "No override" clears the class's mapping, a
    /// provider default stores the bare provider, and a combo stores its
    /// exact model + effort — the rows name the whole target now.
    fn apply_route_class_row(&mut self, class: TaskClass, row: &PickerRow, cx: &mut Context<Self>) {
        match row {
            PickerRow::Policy(PolicyRowId::NoOverride) => {
                self.state.route_classes.remove(&class);
            }
            PickerRow::ProviderDefault(provider) => {
                self.state.route_classes.insert(
                    class,
                    RouteClassTarget {
                        provider: *provider,
                        model: None,
                        effort: None,
                    },
                );
            }
            PickerRow::Combo(row) => {
                self.state.route_classes.insert(
                    class,
                    RouteClassTarget {
                        provider: row.provider,
                        model: Some(row.model.id.clone()),
                        effort: row.effort.clone(),
                    },
                );
            }
            // The class spec never lists the route row.
            PickerRow::Policy(PolicyRowId::Auto) => {}
        }
        self.save();
        cx.notify();
    }

    /// Bring the class's current target row into view — the same reveal the
    /// model picker does for the session's combo.
    pub(super) fn reveal_route_class_target(&self, class: TaskClass) {
        let rows = self.route_class_picker_rows("");
        let index = self.route_class_selected_index(class, &rows).unwrap_or(0);
        self.route_class_picker.reveal(index, rows.len());
    }

    /// Step the rail to the adjacent section, wrapping at both ends.
    /// `tab`/`shift-tab` land here from under the focused filter field, the
    /// same route the arrows take. A live query filters across all
    /// sections, so cycling waits until the field is cleared.
    fn cycle_route_class_section(&mut self, key: &str, cx: &mut Context<Self>) {
        let rows = self.route_class_picker_rows("");
        // The section the keyboard cursor sits in — seeded from the class's
        // target row the way the reveal lands, so the first tab steps
        // relative to the selection rather than an end.
        let seed = self
            .route_class_open
            .and_then(|class| self.route_class_selected_index(class, &rows))
            .unwrap_or(0);
        self.route_class_picker.cycle_section(&rows, seed, key, cx);
    }

    /// Ask Jev to guess the three class mappings from the user's recent
    /// model+effort usage — the recency-ordered list the model picker
    /// already keeps. The answers land as ordinary class entries, editable
    /// the same as a manual pick.
    fn suggest_route_class_defaults(&mut self, cx: &mut Context<Self>) {
        if self.route_suggest_pending {
            return;
        }
        let combos: Vec<RouteClassTarget> = self
            .state
            .recent_model_uses
            .iter()
            .take(ROUTE_SUGGEST_COMBOS)
            .map(|use_| RouteClassTarget {
                provider: use_.provider,
                model: Some(use_.model.clone()),
                effort: use_.effort.clone(),
            })
            .collect();
        if combos.len() < ALL_TASK_CLASSES.len() {
            self.route_suggest_result = Some(Err(tr!("routing.suggest_no_history")));
            cx.notify();
            return;
        }
        let labels: Vec<String> = combos
            .iter()
            .map(|target| route_class_target_label(Some(target), &self.probes))
            .collect();
        self.route_suggest_pending = true;
        self.route_suggest_result = None;
        let criteria: BTreeMap<String, Option<String>> = labels
            .iter()
            .enumerate()
            .map(|(index, label)| (index.to_string(), Some(label.clone())))
            .collect();
        let questions = ALL_TASK_CLASSES
            .iter()
            .map(|class| {
                (
                    class.id().to_owned(),
                    waku_protocol::eval::EvalQuestion::Choice {
                        instructions: format!(
                            "These are the model+effort combos the user ran most recently, \
                             most recent first. Pick the combo that best fits {} work: {}",
                            class.id(),
                            match class {
                                TaskClass::Routine =>
                                    "mechanical, low-risk, or single-step work — cheap and fast",
                                TaskClass::General =>
                                    "ordinary tasks — the solid default",
                                TaskClass::Demanding =>
                                    "subtle, high-stakes, or long-horizon work — the strongest combo",
                            }
                        ),
                        criteria: criteria.clone(),
                    },
                )
            })
            .collect();
        let state = serde_json::json!({ "recentCombos": labels });
        let daemon = self.daemon.client();
        let suggest = cx.background_executor().spawn(async move {
            daemon
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::Evaluate {
                        state,
                        questions,
                        feature: Some("route-class-suggest".to_owned()),
                        timeout_secs: None,
                    },
                )
                .map_err(|error| format!("{error:#}"))
                .and_then(|payload| match payload {
                    waku_client::ResponsePayload::Evaluation { evaluation } => Ok(evaluation),
                    _ => Err("the daemon returned an invalid evaluation response".into()),
                })
        });
        cx.spawn(async move |this, cx| {
            let result = suggest.await;
            let _ = this.update(cx, |this, cx| {
                this.route_suggest_pending = false;
                match result {
                    Ok(evaluation) => {
                        let mut applied = 0;
                        for class in ALL_TASK_CLASSES {
                            if let Some(waku_protocol::eval::EvalAnswer::Choice { choice, .. }) =
                                evaluation.answers.get(class.id())
                                && let Ok(index) = choice.parse::<usize>()
                                && let Some(target) = combos.get(index)
                            {
                                this.state.route_classes.insert(class, target.clone());
                                applied += 1;
                            }
                        }
                        this.route_suggest_result = Some(if applied == 0 {
                            Err(tr!("routing.suggest_empty"))
                        } else {
                            this.save();
                            Ok(())
                        });
                    }
                    Err(error) => {
                        this.route_suggest_result = Some(Err(error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn daemon_exposure_from_fields(
        &self,
        cx: &App,
    ) -> Result<waku_client::DaemonExposureSettings, String> {
        let port = self
            .daemon_port_input
            .read(cx)
            .content()
            .trim()
            .parse::<u16>()
            .map_err(|_| tr!("daemon.invalid_port"))?;
        if port == 0 {
            return Err(tr!("daemon.invalid_port"));
        }
        let origins = self.daemon_origins_input.read(cx).content().to_owned();
        let mut settings = self.state.daemon_exposure.clone();
        settings.port = port;
        settings
            .with_allowed_origins_text(&origins)
            .and_then(waku_client::DaemonExposureSettings::validate)
            .map_err(|error| error.to_string())
    }

    fn daemon_exposure_fields_dirty(&self, cx: &App) -> bool {
        self.daemon_exposure_from_fields(cx)
            .map(|settings| {
                settings.port != self.state.daemon_exposure.port
                    || settings.allowed_origins != self.state.daemon_exposure.allowed_origins
            })
            .unwrap_or(true)
    }

    /// The `goddard://connect` link the QR encodes — the LAN IPv4 a phone
    /// can dial (this machine's hostname does not resolve for it), the
    /// exposed port, the mnemonic token, and the hostname as a label hint.
    fn daemon_connect_url(&self) -> String {
        let host = self
            .daemon_lan_ip
            .as_deref()
            .unwrap_or(&self.daemon_hostname);
        let mut url = url::Url::parse("goddard://connect").expect("the connect link base is valid");
        url.query_pairs_mut()
            .append_pair(
                "address",
                &format!("ws://{host}:{}", self.state.daemon_exposure.port),
            )
            .append_pair("token", &self.state.daemon_exposure.token)
            .append_pair("name", &self.daemon_hostname);
        url.into()
    }

    /// The QR carries the token in scannable form, so it hides behind its
    /// own toggle like the token's eye. Encoding once at reveal keeps the
    /// paint pass to a read of the cached matrix.
    fn toggle_daemon_qr(&mut self, cx: &mut Context<Self>) {
        self.daemon_qr = if self.daemon_qr.is_some() {
            None
        } else {
            qrcode::QrCode::with_error_correction_level(
                self.daemon_connect_url(),
                qrcode::EcLevel::M,
            )
            .ok()
            .map(|code| {
                std::sync::Arc::new(DaemonQrCode {
                    width: code.width(),
                    dark: code
                        .into_colors()
                        .into_iter()
                        .map(|color| color == qrcode::types::Color::Dark)
                        .collect(),
                })
            })
        };
        cx.notify();
    }

    fn set_daemon_exposure_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.daemon_token_revealed = false;
            self.daemon_qr = None;
        }
        let settings = if enabled {
            match self.daemon_exposure_from_fields(cx) {
                Ok(mut settings) => {
                    settings.enabled = true;
                    settings
                }
                Err(error) => {
                    self.show_toast(tr!("daemon.invalid_settings", error = error));
                    return;
                }
            }
        } else {
            let mut settings = self.state.daemon_exposure.clone();
            settings.enabled = false;
            settings
        };
        self.analytics
            .track(crate::analytics::Event::DaemonExposureChanged { enabled });
        self.apply_daemon_exposure(settings, cx);
    }

    pub(super) fn apply_daemon_exposure_fields(&mut self, cx: &mut Context<Self>) {
        let settings = match self.daemon_exposure_from_fields(cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.show_toast(tr!("daemon.invalid_settings", error = error));
                return;
            }
        };
        self.apply_daemon_exposure(settings, cx);
    }

    fn regenerate_daemon_token(&mut self, cx: &mut Context<Self>) {
        let mut settings = match self.daemon_exposure_from_fields(cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.show_toast(tr!("daemon.invalid_settings", error = error));
                return;
            }
        };
        settings.token = waku_client::DaemonExposureSettings::new_token();
        self.daemon_token_revealed = false;
        self.daemon_qr = None;
        self.apply_daemon_exposure(settings, cx);
    }

    fn apply_daemon_exposure(
        &mut self,
        settings: waku_client::DaemonExposureSettings,
        cx: &mut Context<Self>,
    ) {
        if self.daemon_reconfigure_pending || settings == self.state.daemon_exposure {
            return;
        }
        // A changed port or token stales the encoded link — hide it rather
        // than show a code that no longer connects.
        self.daemon_qr = None;
        if self.daemon.is_externally_managed() {
            self.show_toast(tr!("daemon.external_description"));
            return;
        }

        // Exposure changes apply over the control socket — the daemon opens
        // or closes its exposed listener without restarting, so sessions
        // keep running either way. Only a daemon round trip is needed when
        // exposure is on at either end of the change.
        let needs_daemon = self.state.daemon_exposure.enabled || settings.enabled;
        if !needs_daemon {
            self.state.daemon_exposure = settings;
            self.save();
            cx.notify();
            return;
        }

        self.daemon_reconfigure_pending = true;
        let daemon = self.daemon.clone();
        let applied = settings.clone();
        let restart = cx
            .background_executor()
            .spawn(async move { daemon.reconfigure(settings) });
        cx.spawn(async move |this, cx| {
            let result = restart.await;
            let _ = this.update(cx, |this, cx| {
                this.daemon_reconfigure_pending = false;
                match result {
                    Ok(()) => {
                        this.state.daemon_exposure = applied.clone();
                        this.daemon_port_input.update(cx, |input, cx| {
                            input.set_content(applied.port.to_string(), cx)
                        });
                        this.daemon_origins_input.update(cx, |input, cx| {
                            input.set_content(applied.allowed_origins_text(), cx)
                        });
                        this.save();
                        this.show_success_toast(tr!("daemon.settings_applied"));
                    }
                    Err(error) => {
                        this.show_toast(tr!("daemon.restart_failed", error = error.to_string()))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_archived_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let query = self
            .archived_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();

        let mut archived = self
            .state
            .sessions
            .iter()
            .filter(|session| session.archived_at.is_some())
            .collect::<Vec<_>>();
        archived.sort_by_key(|session| std::cmp::Reverse(session.archived_at));
        let any_archived = !archived.is_empty();

        // The selector offers the projects the archive actually spans, in
        // name order, and earns its slot once a choice would narrow
        // anything — or while a set filter needs clearing.
        let mut project_options: Vec<(Uuid, String)> = Vec::new();
        for session in &archived {
            if project_options
                .iter()
                .any(|(id, _)| *id == session.project_id)
            {
                continue;
            }
            project_options.push((
                session.project_id,
                self.project_display_name(session.project_id),
            ));
        }
        project_options.sort_by_cached_key(|(_, name)| name.to_lowercase());
        let project_names: HashMap<Uuid, String> = project_options
            .iter()
            .map(|(id, name)| (*id, name.to_lowercase()))
            .collect();
        // A filter whose project left the archive matches nothing; treat it
        // as cleared so it cannot silently hide every row.
        let project_filter = self
            .archived_project_filter
            .filter(|id| project_names.contains_key(id));

        // Transcript hits only count while the stored map belongs to this
        // exact query — a stale map would show the wrong sessions' snippets.
        let content_matches = (self.archived_message_matches_query.as_deref()
            == Some(query.as_str()))
        .then_some(&self.archived_message_matches);
        let visible = filter_archived_sessions(
            &archived,
            &query,
            project_filter,
            &project_names,
            content_matches,
        );
        self.sync_archived_session_rows(&visible);

        let mut page = div()
            .mt(px(15.0))
            .w_full()
            .flex_1()
            .min_h_0()
            .pb(px(32.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .text_size(sp(12.5))
                    .line_height(sp(18.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("settings.archived_description")),
            );
        if !any_archived {
            return page
                .child(
                    div()
                        .mt(px(12.0))
                        .text_size(sp(13.0))
                        .text_color(theme.text_tertiary)
                        .child(tr!("settings.archived_empty")),
                )
                .into_any_element();
        }

        let mut toolbar = div()
            .mt(px(14.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                TextField::new("archived-search-field", self.archived_search.clone())
                    .icon("icons/search.svg", 13.0)
                    .flex_1()
                    .min_w_0(),
            );
        if project_options.len() > 1 || project_filter.is_some() {
            let weak = cx.entity().downgrade();
            let selected_label = project_filter
                .and_then(|filter| {
                    project_options
                        .iter()
                        .find(|(id, _)| *id == filter)
                        .map(|(_, name)| name.clone())
                })
                .unwrap_or_else(|| tr!("settings.archived_all_projects"));
            let menu_handle = self.menu_handle("archived-project-selector", cx);
            toolbar = toolbar.child(dropdown_menu(
                MenuChip::new("archived-project-selector")
                    .icon("icons/folder.svg", theme.text_tertiary)
                    .label(selected_label)
                    .outlined()
                    .selected(menu_handle.is_open())
                    .max_w(px(220.0))
                    .flex_none(),
                "archived-project-selector-menu",
                &menu_handle,
                MenuAlign::BelowRight,
                move |_| {
                    let mut items = Vec::with_capacity(project_options.len() + 1);
                    items.push(
                        MenuItem::new(tr!("settings.archived_all_projects"), {
                            let weak = weak.clone();
                            move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.archived_project_filter = None;
                                    cx.notify();
                                });
                            }
                        })
                        .selected(project_filter.is_none()),
                    );
                    items.extend(project_options.iter().map(|(project_id, name)| {
                        let project_id = *project_id;
                        let weak = weak.clone();
                        MenuItem::new(name.clone(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.archived_project_filter = Some(project_id);
                                cx.notify();
                            });
                        })
                        .selected(project_filter == Some(project_id))
                    }));
                    items
                },
            ));
        }
        page = page.child(toolbar);

        if visible.is_empty() {
            page = page.child(
                div()
                    .mt(px(12.0))
                    .text_size(sp(13.0))
                    .text_color(theme.text_tertiary)
                    .child(
                        if self.archived_message_search_pending && !query.is_empty() {
                            tr!("settings.archived_searching")
                        } else {
                            tr!("settings.archived_no_match")
                        },
                    ),
            );
        } else {
            let entity = cx.entity().downgrade();
            page = page.child(
                div()
                    .mt(px(10.0))
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        list(
                            self.archived_sessions_list.clone(),
                            move |index, window, cx| {
                                entity
                                    .upgrade()
                                    .map(|entity| {
                                        entity.update(cx, |this, cx| {
                                            this.archived_session_row(index, window, cx)
                                        })
                                    })
                                    .unwrap_or_else(|| div().into_any_element())
                            },
                        )
                        .size_full(),
                    )
                    .child(scrollbar::vertical(
                        &self.archived_sessions_list,
                        &self.archived_sessions_scrollbar,
                    )),
            );
        }
        page.into_any_element()
    }

    /// A project's display name, falling back to the "No project" label when
    /// the archived session's project is gone.
    fn project_display_name(&self, project_id: Uuid) -> String {
        self.state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(Project::display_name)
            .unwrap_or_else(|| tr!("project.no_project_name"))
    }

    /// Debounce the archived filter's transcript scan onto the background
    /// executor, mirroring the command palette's message search: the SQLite
    /// read runs off-thread, results land on `archived_message_matches`, and
    /// rows pick up their snippets on the next frame.
    pub(super) fn schedule_archived_message_search(&mut self, cx: &mut Context<Self>) {
        let query = self
            .archived_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();
        let fetch = if query.is_empty() {
            self.archived_message_matches_query = None;
            self.archived_message_matches.clear();
            self.archived_message_search_pending = false;
            None
        } else {
            match self.archived_message_searches.read(&query) {
                Query::Ready(matches) => {
                    self.archived_message_matches_query = Some(query.clone());
                    self.archived_message_matches = matches
                        .iter()
                        .cloned()
                        .map(|matched| (matched.session_id, matched))
                        .collect();
                    self.archived_message_search_pending = false;
                    None
                }
                Query::Pending => {
                    self.archived_message_search_pending = true;
                    None
                }
                Query::Missing(token) => {
                    self.archived_message_search_pending = true;
                    Some(token)
                }
            }
        };
        cx.notify();

        let Some(token) = fetch else { return };
        let search = self.store.session_message_search(
            query.clone(),
            ARCHIVED_MESSAGE_SEARCH_LIMIT,
            crate::persistence::SessionMessageSearchScope::Archived,
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(ARCHIVED_MESSAGE_SEARCH_DEBOUNCE)
                .await;
            let current = this
                .update(cx, |this, cx| {
                    this.archived_search
                        .read(cx)
                        .content()
                        .trim()
                        .to_lowercase()
                        == query
                })
                .unwrap_or(false);
            if !current {
                let _ = this.update(cx, |this, _| this.archived_message_searches.abandon(token));
                return;
            }
            let matches = cx
                .background_executor()
                .spawn(async move { search().unwrap_or_default() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.archived_message_searches.fulfill(token, matches) {
                    return;
                }
                if this
                    .archived_search
                    .read(cx)
                    .content()
                    .trim()
                    .to_lowercase()
                    != query
                {
                    return;
                }
                let Query::Ready(matches) = this.archived_message_searches.read(&query) else {
                    return;
                };
                this.archived_message_matches_query = Some(query.clone());
                this.archived_message_matches = matches
                    .iter()
                    .cloned()
                    .map(|matched| (matched.session_id, matched))
                    .collect();
                this.archived_message_search_pending = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Keep the virtualized archived list in sync with the filtered session
    /// ids. Filtering preserves order, so an unchanged prefix splices only
    /// the tail and the scroll position survives typing in the filter.
    fn sync_archived_session_rows(&self, sessions: &[Uuid]) {
        let mut cached = self.archived_session_rows.borrow_mut();
        if cached.as_slice() == sessions {
            return;
        }
        let prefix = cached
            .iter()
            .zip(sessions.iter())
            .take_while(|(cached, fresh)| cached == fresh)
            .count();
        let old_count = cached.len();
        *cached = sessions.to_vec();
        if old_count == 0 {
            self.archived_sessions_list
                .reset_with_uniform_height(sessions.len(), px(ARCHIVED_SESSION_ROW_HEIGHT));
        } else {
            self.archived_sessions_list
                .splice(prefix..old_count, sessions.len() - prefix);
            // Newly inserted rows have no measured height yet; the uniform
            // hint keeps the scrollbar's total height honest.
            self.archived_sessions_list
                .clone()
                .with_uniform_item_height(px(ARCHIVED_SESSION_ROW_HEIGHT));
        }
    }

    /// One archived-chat row, built only while visible. Reads the per-frame
    /// id cache; a stale index from a frame racing a state change renders as
    /// an empty row for that frame rather than panicking.
    fn archived_session_row(
        &self,
        row: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let rows = self.archived_session_rows.borrow();
        let Some(session_id) = rows.get(row).copied() else {
            return div().into_any_element();
        };
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return div().into_any_element();
        };
        let group = SharedString::from(format!("archived-chat-{session_id}"));
        let project_name = self.project_display_name(session.project_id);
        let updated =
            super::sidebar::format_time_ago(unix_time().saturating_sub(session.updated_at));
        let detail = if session.landed_at.is_some() {
            format!(
                "{project_name} · {updated} · {}",
                tr!("settings.archived_landed")
            )
        } else {
            format!("{project_name} · {updated}")
        };
        // The transcript hit that surfaced this row, when the field's query
        // still owns the stored map — shown like the palette's match line.
        let query = self
            .archived_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();
        let content_match = (self.archived_message_matches_query.as_deref()
            == Some(query.as_str()))
        .then(|| self.archived_message_matches.get(&session_id))
        .flatten()
        .cloned();

        let unarchive_button = div()
            .id(SharedString::from(format!(
                "archived-chat-unarchive-{session_id}"
            )))
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            // Hover reveals the actions; keyboard focus has to reach the
            // same buttons, so focus makes them visible too.
            .opacity(0.0)
            .group_hover(group.clone(), |element| element.opacity(1.0))
            .focus_visible(|element| element.opacity(1.0).bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(tr!("common.unarchive"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.unarchive_session(session_id, true, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.unarchive_session(session_id, true, cx);
                    cx.stop_propagation();
                }
            }));

        let remove_button = div()
            .id(SharedString::from(format!(
                "archived-chat-remove-{session_id}"
            )))
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.danger)
            .opacity(0.0)
            .group_hover(group.clone(), |element| element.opacity(1.0))
            .focus_visible(|element| element.opacity(1.0).bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.danger.opacity(0.12)))
            .active(|element| element.bg(theme.danger.opacity(0.18)))
            .child(tr!("common.remove"))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.remove_session(session_id, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.remove_session(session_id, window, cx);
                    cx.stop_propagation();
                }
            }));

        div()
            .group(group)
            .w_full()
            .px(px(12.0))
            .py(px(7.0))
            .rounded(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .hover(|element| element.bg(theme.sidebar_item_background))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(session.display_title().to_owned()),
                    )
                    .child(
                        div()
                            .mt(px(1.0))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(sp(11.5))
                            .text_color(theme.text_tertiary)
                            .when(session.landed_at.is_some(), |element| {
                                element.child(icon("icons/check.svg", 11.0, theme.success))
                            })
                            .child(detail),
                    )
                    .when_some(content_match, |column, matched| {
                        column.child(
                            div()
                                .mt(px(1.0))
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(sp(11.5))
                                .child(super::command_palette::palette_content_match_text(
                                    &matched, &query, window, theme,
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(unarchive_button)
                    .child(remove_button),
            )
            .into_any_element()
    }

    fn render_appearance_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let theme_settings = self.state.theme;
        let selected_language = self.state.language;

        let weak = cx.entity().downgrade();
        let restore_preview = Self::theme_preview_restore(cx);
        let mode_handle = self.menu_handle_with("appearance-mode-selector", cx, restore_preview);
        let mode_selector = dropdown_menu(
            MenuChip::new("appearance-mode-selector")
                .label(theme_settings.mode.label())
                .outlined()
                .selected(mode_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "appearance-mode-selector-menu",
            &mode_handle,
            MenuAlign::BelowRight,
            move |_| {
                ThemeMode::ALL
                    .into_iter()
                    .map(|mode| {
                        let weak = weak.clone();
                        MenuItem::new(mode.label(), {
                            let weak = weak.clone();
                            move |window, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.update_theme_settings(
                                        |settings| settings.mode = mode,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        })
                        .selected(mode == theme_settings.mode)
                        .on_highlight(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.preview_theme_settings(
                                    |settings| settings.mode = mode,
                                    window,
                                    cx,
                                );
                            });
                        })
                    })
                    .collect()
            },
        );

        let weak = cx.entity().downgrade();
        let slot_preview = Self::theme_slot_preview(ThemeMode::Light, cx);
        let light_handle = self.menu_handle_with("light-theme-selector", cx, slot_preview);
        let light_theme_selector = dropdown_menu(
            MenuChip::new("light-theme-selector")
                .label(theme_settings.light.label())
                .outlined()
                .selected(light_handle.is_open())
                .w(px(160.0))
                .justify_between(),
            "light-theme-selector-menu",
            &light_handle,
            MenuAlign::BelowRight,
            move |_| {
                ThemeName::LIGHT
                    .into_iter()
                    .map(|name| {
                        let weak = weak.clone();
                        MenuItem::new(name.label(), {
                            let weak = weak.clone();
                            move |window, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.update_theme_settings(
                                        |settings| settings.light = name,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        })
                        .selected(name == theme_settings.light)
                        .on_highlight(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.preview_theme_settings(
                                    |settings| {
                                        // A palette only shows while its slot
                                        // is live, so the preview claims the
                                        // slot too — otherwise light options
                                        // would stay invisible in dark mode.
                                        settings.light = name;
                                        settings.mode = ThemeMode::Light;
                                    },
                                    window,
                                    cx,
                                );
                            });
                        })
                    })
                    .collect()
            },
        );

        let weak = cx.entity().downgrade();
        let slot_preview = Self::theme_slot_preview(ThemeMode::Dark, cx);
        let dark_handle = self.menu_handle_with("dark-theme-selector", cx, slot_preview);
        let dark_theme_selector = dropdown_menu(
            MenuChip::new("dark-theme-selector")
                .label(theme_settings.dark.label())
                .outlined()
                .selected(dark_handle.is_open())
                .w(px(160.0))
                .justify_between(),
            "dark-theme-selector-menu",
            &dark_handle,
            MenuAlign::BelowRight,
            move |_| {
                ThemeName::DARK
                    .into_iter()
                    .map(|name| {
                        let weak = weak.clone();
                        MenuItem::new(name.label(), {
                            let weak = weak.clone();
                            move |window, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.update_theme_settings(
                                        |settings| settings.dark = name,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        })
                        .selected(name == theme_settings.dark)
                        .on_highlight(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.preview_theme_settings(
                                    |settings| {
                                        // A palette only shows while its slot
                                        // is live, so the preview claims the
                                        // slot too — otherwise dark options
                                        // would stay invisible in light mode.
                                        settings.dark = name;
                                        settings.mode = ThemeMode::Dark;
                                    },
                                    window,
                                    cx,
                                );
                            });
                        })
                    })
                    .collect()
            },
        );

        let ui_font_selector = self.font_family_selector(FontTarget::Ui, cx);
        let code_font_selector = self.font_family_selector(FontTarget::Code, cx);

        let selected_ui_font_size = self.state.ui_font_size;
        let weak = cx.entity().downgrade();
        let ui_font_size_handle = self.menu_handle("ui-font-size-selector", cx);
        let ui_font_size_selector = dropdown_menu(
            MenuChip::new("ui-font-size-selector")
                .label(font_size_label(selected_ui_font_size))
                .outlined()
                .selected(ui_font_size_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "ui-font-size-selector-menu",
            &ui_font_size_handle,
            MenuAlign::BelowRight,
            move |_| {
                FONT_SIZES
                    .into_iter()
                    .map(|size| {
                        let weak = weak.clone();
                        MenuItem::new(font_size_label(size), move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_ui_font_size(size, window, cx);
                            });
                        })
                        .selected(size == selected_ui_font_size)
                    })
                    .collect()
            },
        );

        let selected_code_font_size = self.state.code_font_size;
        let weak = cx.entity().downgrade();
        let code_font_size_handle = self.menu_handle("code-font-size-selector", cx);
        let code_font_size_selector = dropdown_menu(
            MenuChip::new("code-font-size-selector")
                .label(font_size_label(selected_code_font_size))
                .outlined()
                .selected(code_font_size_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "code-font-size-selector-menu",
            &code_font_size_handle,
            MenuAlign::BelowRight,
            move |_| {
                FONT_SIZES
                    .into_iter()
                    .map(|size| {
                        let weak = weak.clone();
                        MenuItem::new(font_size_label(size), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_code_font_size(size, cx);
                            });
                        })
                        .selected(size == selected_code_font_size)
                    })
                    .collect()
            },
        );

        let selected_terminal_font_size = self.state.terminal_font_size();
        let weak = cx.entity().downgrade();
        let terminal_font_size_handle = self.menu_handle("terminal-font-size-selector", cx);
        let terminal_font_size_selector = dropdown_menu(
            MenuChip::new("terminal-font-size-selector")
                .label(font_size_label(selected_terminal_font_size))
                .outlined()
                .selected(terminal_font_size_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "terminal-font-size-selector-menu",
            &terminal_font_size_handle,
            MenuAlign::BelowRight,
            move |_| {
                FONT_SIZES
                    .into_iter()
                    .map(|size| {
                        let weak = weak.clone();
                        MenuItem::new(font_size_label(size), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_terminal_font_size(size, cx);
                            });
                        })
                        .selected(size == selected_terminal_font_size)
                    })
                    .collect()
            },
        );

        let weak = cx.entity().downgrade();
        let language_handle = self.menu_handle("language-selector", cx);
        let language_selector = dropdown_menu(
            MenuChip::new("language-selector")
                .label(selected_language.label())
                .outlined()
                .selected(language_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "language-selector-menu",
            &language_handle,
            MenuAlign::BelowRight,
            move |_| {
                crate::i18n::AppLanguage::ALL
                    .into_iter()
                    .map(|language| {
                        let weak = weak.clone();
                        MenuItem::new(language.label(), move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_language(language, window, cx);
                            });
                        })
                        .selected(language == selected_language)
                    })
                    .collect()
            },
        );

        let preview_open = self.theme_preview_expanded
            || mode_handle.is_open()
            || light_handle.is_open()
            || dark_handle.is_open();
        let preview_row = {
            let title = tr!("settings.preview");
            let description = tr!("settings.preview_description");
            search
                .matched(&title, &description)
                .map(|matched| self.render_theme_preview(preview_open, matched, cx))
        };

        // Vibrancy is a macOS-only effect; on other platforms the sidebar is
        // already a solid fill and there is nothing to switch.
        let transparency_rows = if cfg!(target_os = "macos") {
            let transparent = self.state.sidebar_transparency;
            let amount = self.state.sidebar_transparency_amount;
            let amount_shown = self.sidebar_transparency_slider.shown(amount);
            let amount_slider = slider::slider(
                "sidebar-transparency-slider",
                &self.sidebar_transparency_slider,
                crate::persistence::MAX_SIDEBAR_TRANSPARENCY,
                amount,
                cx,
                |this, amount, window, cx| this.set_sidebar_transparency_amount(amount, window, cx),
            );
            let transparency_row = settings_row(
                "icons/panel-left.svg",
                tr!("settings.sidebar_transparency"),
                tr!("settings.sidebar_transparency_description"),
                self.setting_selector(
                    "sidebar-transparency-selector",
                    vec![
                        (true, tr!("settings.sidebar_transparency_translucent")),
                        (false, tr!("settings.sidebar_transparency_solid")),
                    ],
                    transparent,
                    160.0,
                    cx,
                    Self::set_sidebar_transparency,
                ),
                theme,
                search,
            );
            // Same shape as the completion-volume row: the slider only exists
            // while Translucent is picked, so an in-flight drag cannot outlive
            // it — `set_sidebar_transparency` cancels the drag state on the
            // way off.
            let amount_row = if !transparent {
                None
            } else {
                let title = tr!("settings.sidebar_transparency_amount");
                search.matched(&title, "").map(|matched| {
                    div()
                        .w_full()
                        .min_h(px(52.0))
                        .px(px(20.0))
                        .py(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .child(settings_row_icon("icons/gauge.svg", theme))
                        .child(settings_title_jump(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(settings_search_text(
                                    title,
                                    matched.title_ranges.clone(),
                                    theme,
                                )),
                            &matched,
                            theme,
                        ))
                        .child(amount_slider.w(px(140.0)).flex_none())
                        .child(
                            div()
                                .w(px(32.0))
                                .flex_none()
                                .flex()
                                .justify_end()
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(format!("{}%", (amount_shown * 100.0).round() as i32)),
                        )
                        .into_any_element()
                })
            };
            vec![transparency_row, amount_row]
        } else {
            Vec::new()
        };

        let mut rows: Vec<Option<AnyElement>> = vec![
            settings_row(
                "icons/appearance.svg",
                tr!("settings.appearance"),
                tr!("settings.appearance_mode_description"),
                mode_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/sun.svg",
                tr!("settings.light_theme"),
                tr!("settings.light_theme_description"),
                light_theme_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/moon.svg",
                tr!("settings.dark_theme"),
                tr!("settings.dark_theme_description"),
                dark_theme_selector,
                theme,
                search,
            ),
            preview_row,
        ];
        rows.extend(transparency_rows);
        rows.extend([
            settings_row(
                "icons/languages.svg",
                tr!("language.title"),
                tr!("language.description"),
                language_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/type.svg",
                tr!("settings.ui_font"),
                tr!("settings.ui_font_description"),
                ui_font_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/type.svg",
                tr!("settings.code_font"),
                tr!("settings.code_font_description"),
                code_font_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/case-sensitive.svg",
                tr!("settings.ui_font_size"),
                tr!("settings.ui_font_size_description"),
                ui_font_size_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/case-sensitive.svg",
                tr!("settings.code_font_size"),
                tr!("settings.code_font_size_description"),
                code_font_size_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/terminal.svg",
                tr!("settings.terminal_font_size"),
                tr!("settings.terminal_font_size_description"),
                terminal_font_size_selector,
                theme,
                search,
            ),
            settings_row(
                "icons/square.svg",
                tr!("settings.border_weight"),
                tr!("settings.border_weight_description"),
                self.setting_selector(
                    "border-weight-selector",
                    vec![
                        (false, tr!("settings.border_weight_hairline")),
                        (true, tr!("settings.border_weight_pixel")),
                    ],
                    self.state.thick_borders,
                    140.0,
                    cx,
                    |this, value, _, cx| this.set_thick_borders(value, cx),
                ),
                theme,
                search,
            ),
            settings_row(
                "icons/gauge.svg",
                tr!("settings.border_intensity"),
                tr!("settings.border_intensity_description"),
                {
                    let intensity = self.state.border_intensity;
                    let shown = self.border_intensity_slider.shown(intensity);
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            slider::slider(
                                "border-intensity-slider",
                                &self.border_intensity_slider,
                                crate::persistence::MAX_BORDER_INTENSITY,
                                intensity,
                                cx,
                                |this, intensity, window, cx| {
                                    this.set_border_intensity(intensity, window, cx)
                                },
                            )
                            .w(px(140.0))
                            .flex_none(),
                        )
                        .child(
                            div()
                                .w(px(32.0))
                                .flex_none()
                                .flex()
                                .justify_end()
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(format!("{}%", (shown * 100.0).round() as i32)),
                        )
                },
                theme,
                search,
            ),
            settings_row(
                "icons/contrast.svg",
                tr!("settings.high_contrast"),
                tr!("settings.high_contrast_description"),
                toggle_switch(
                    "high-contrast-toggle",
                    self.state.high_contrast || crate::platform::increase_contrast(),
                    crate::platform::increase_contrast(),
                    theme,
                    cx,
                    {
                        let enabled = self.state.high_contrast;
                        move |this, window, cx| this.set_high_contrast(!enabled, window, cx)
                    },
                ),
                theme,
                search,
            ),
        ]);

        // Rendered-content prefs get their own labeled card under the theme
        // card rather than joining its hairline-separated rows.
        let transcript_card = settings_row_card(
            vec![
                settings_row(
                    "icons/sigma.svg",
                    tr!("settings.math_rendering"),
                    tr!("settings.math_rendering_description"),
                    self.setting_selector(
                        "math-rendering-selector",
                        vec![
                            (true, tr!("settings.math_rendering_formatted")),
                            (false, tr!("settings.math_rendering_latex")),
                        ],
                        self.state.render_math,
                        160.0,
                        cx,
                        |this, value, _, cx| this.set_render_math(value, cx),
                    ),
                    theme,
                    search,
                ),
                settings_row(
                    "icons/gauge.svg",
                    tr!("settings.show_response_token_speed"),
                    tr!("settings.show_response_token_speed_description"),
                    toggle_switch(
                        "response-token-speed-toggle",
                        self.state.show_response_token_speed,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.show_response_token_speed;
                            move |this, _, cx| this.set_show_response_token_speed(!enabled, cx)
                        },
                    ),
                    theme,
                    search,
                ),
                settings_row(
                    "icons/file.svg",
                    tr!("settings.markdown_files"),
                    tr!("settings.markdown_files_description"),
                    self.setting_selector(
                        "markdown-files-selector",
                        vec![
                            (true, tr!("settings.markdown_files_preview")),
                            (false, tr!("settings.markdown_files_source")),
                        ],
                        self.state.markdown_preview,
                        160.0,
                        cx,
                        |this, value, _, cx| this.set_markdown_preview(value, cx),
                    ),
                    theme,
                    search,
                ),
            ],
            theme,
        );

        div()
            .mt(px(15.0))
            .flex()
            .flex_col()
            .gap(px(20.0))
            .children(settings_row_card(rows, theme))
            .children(settings_group(
                tr!("settings.group_transcript"),
                transcript_card
                    .map(|card| card.into_any_element())
                    .into_iter()
                    .collect(),
                theme,
            ))
            .into_any_element()
    }

    /// The Appearance page's collapsible sample: a miniature transcript —
    /// user bubble, assistant reply, code block — painted on the transcript's
    /// `surface` so a previewed palette reads exactly as it will in chat.
    /// `open` also comes in held by an open theme selector, so the sample
    /// appears for the duration of a pick.
    fn render_theme_preview(
        &self,
        open: bool,
        matched: SettingRowMatch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let metrics = self.scaled_markdown_metrics(MarkdownMetrics::BODY);
        let syntax = theme.syntax;

        let disclosure = div()
            .id("theme-preview-disclosure")
            .tab_index(0)
            .w_full()
            .min_h(px(60.0))
            .px(px(20.0))
            .py(px(12.0))
            .flex()
            .items_center()
            .gap(px(24.0))
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .child(settings_row_label(
                "icons/eye.svg",
                settings_row_text(
                    tr!("settings.preview"),
                    tr!("settings.preview_description"),
                    matched,
                    theme,
                ),
                theme,
            ))
            .child(icon(
                if open {
                    "icons/chevron-down.svg"
                } else {
                    "icons/chevron-right.svg"
                },
                10.5,
                theme.affordance_icon(),
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.theme_preview_expanded = !this.theme_preview_expanded;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.theme_preview_expanded = !this.theme_preview_expanded;
                    cx.notify();
                    cx.stop_propagation();
                }
            }));

        if !open {
            return disclosure.into_any_element();
        }

        let code_line =
            |spans: &[(&'static str, Hsla)]| {
                div()
                    .flex()
                    .children(spans.iter().map(|&(text, color)| {
                        div().text_color(color).child(text).into_any_element()
                    }))
                    .into_any_element()
            };
        let plain = theme.text_secondary;
        let code_block = div()
            .w_full()
            .min_w_0()
            .rounded(px(10.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.inset)
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .h(px(28.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .border_b(hairline())
                    .border_color(theme.separator)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.5))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_ghost)
                            .child("rust"),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px(px(10.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .font_family(crate::fonts::current(cx).code)
                    .text_size(px(metrics.code_text_size))
                    .line_height(px(metrics.code_line_height))
                    .whitespace_nowrap()
                    .child(code_line(&[
                        ("fn ", syntax.keyword),
                        ("greet", syntax.function),
                        ("(name: ", plain),
                        ("&str", syntax.ty),
                        (") -> ", plain),
                        ("String", syntax.ty),
                        (" {", plain),
                    ]))
                    .child(code_line(&[(
                        "    // Return a greeting for the given name.",
                        syntax.comment,
                    )]))
                    .child(code_line(&[
                        ("    ", plain),
                        ("format!", syntax.meta),
                        ("(", plain),
                        ("\"Hello, {name}!\"", syntax.string),
                        (");", plain),
                    ]))
                    .child(code_line(&[("}", plain)])),
            );

        div()
            .w_full()
            .flex()
            .flex_col()
            .child(disclosure)
            .child(
                div().px(px(20.0)).pb(px(16.0)).child(
                    div()
                        .w_full()
                        .rounded(px(12.0))
                        .border(hairline())
                        .border_color(theme.border_subtle)
                        .bg(theme.surface)
                        .px(px(16.0))
                        .py(px(14.0))
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .child(
                            div().w_full().flex().justify_end().child(
                                div()
                                    .rounded(px(15.0))
                                    .border(hairline())
                                    .border_color(theme.raised)
                                    .bg(theme.raised)
                                    .px(px(11.0))
                                    .py(px(7.0))
                                    .text_size(px(metrics.text_size))
                                    .line_height(px(metrics.line_height))
                                    .text_color(theme.text)
                                    .child(tr!("settings.preview_user_message")),
                            ),
                        )
                        .child(
                            div()
                                .text_size(px(metrics.text_size))
                                .line_height(px(metrics.line_height))
                                .text_color(theme.text)
                                .child(tr!("settings.preview_assistant_reply")),
                        )
                        .child(code_block),
                ),
            )
            .into_any_element()
    }

    /// The Appearance page's family dropdown: a filter field pinned above a
    /// virtualized list of installed families. The field holds real focus
    /// while arrows move a drawn cursor — `up`/`down`/`enter` reach this card
    /// as actions under the `Menu > TextInput` bindings.
    fn font_family_selector(&self, target: FontTarget, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let selector = self.font_selector(target);
        let search = selector.search.clone();
        let list_state = selector.list.clone();
        let scrollbar_state = selector.scrollbar.clone();
        let highlight = selector.highlight;
        let selected = self.font_family_setting(target);
        let weak = cx.entity().downgrade();

        let search_focus = search.read(cx).focus_handle(cx);
        let handle = self.menu_handle_with(target.menu_id(), cx, {
            let weak = weak.clone();
            move |open, window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if open {
                        this.open_font_selector(target, window, cx);
                    } else {
                        let focus = this.settings_focus.clone();
                        window.focus(&focus, cx);
                    }
                });
                if open {
                    let search_focus = search_focus.clone();
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| window.focus(&search_focus, cx));
                    });
                }
            }
        });

        let families = Rc::new(if handle.is_open() {
            let families = self.visible_font_families(target, cx);
            self.sync_font_selector_rows(target, &families);
            // `open_font_selector` asks the first frame — or the frame the
            // background enumeration lands on — to park on the current face.
            if crate::fonts::installed(cx).is_some()
                && selector.pending_reveal.replace(false)
                && let Some(index) = families.iter().position(|name| *name == selected)
            {
                selector.list.scroll_to_reveal_item(index);
            }
            families
        } else {
            Vec::new()
        });
        let highlight = highlight.filter(|index| *index < families.len());
        let default_family = SharedString::from(target.default_family());

        let trigger = MenuChip::new(target.menu_id())
            .label(font_row_label(&selected))
            .outlined()
            .selected(handle.is_open())
            .w(px(180.0))
            .justify_between();

        popover(
            trigger,
            &handle,
            MenuAlign::BelowRight,
            move |popover, _window, _cx| {
                let popover = popover.clone();
                let next_families = families.clone();
                let previous_families = families.clone();
                let confirm_families = families.clone();
                let next_weak = weak.clone();
                let previous_weak = weak.clone();
                let confirm_weak = weak.clone();
                let confirm_popover = popover.clone();

                let rows = if families.is_empty() {
                    div()
                        .h(px(64.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(sp(12.5))
                        .text_color(theme.text_ghost)
                        .child(tr!("settings.no_fonts"))
                        .into_any_element()
                } else {
                    let row_families = families.clone();
                    let row_default = default_family.clone();
                    let row_selected = selected.clone();
                    let row_weak = weak.clone();
                    let row_popover = popover.clone();
                    let height = (families.len() as f32 * FONT_PICKER_ROW_HEIGHT)
                        .min(FONT_PICKER_LIST_MAX_HEIGHT);
                    div()
                        .w_full()
                        .h(px(height))
                        .flex_none()
                        .relative()
                        .px(px(4.0))
                        .child(
                            list(list_state.clone(), move |index, _window, _cx| {
                                let Some(name) = row_families.get(index).cloned() else {
                                    return div().into_any_element();
                                };
                                let is_selected = name == row_selected;
                                let is_default = name == row_default;
                                let highlighted = highlight == Some(index);
                                div()
                                    .id(SharedString::from(format!("font-row-{name}")))
                                    .w_full()
                                    .h(px(FONT_PICKER_ROW_HEIGHT))
                                    .px(px(8.0))
                                    .rounded(px(8.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .cursor_default()
                                    .when(highlighted, |element| element.bg(theme.overlay_strong))
                                    .hover(|element| element.bg(theme.overlay))
                                    .active(|element| element.opacity(0.85))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .font_family(name.clone())
                                            .text_size(sp(12.5))
                                            .line_height(sp(15.0))
                                            .text_color(if is_selected {
                                                theme.text
                                            } else {
                                                theme.text_secondary
                                            })
                                            .child(font_row_label(&name)),
                                    )
                                    .when(is_default, |element| {
                                        element.child(
                                            div()
                                                .flex_none()
                                                .text_size(sp(11.5))
                                                .text_color(theme.text_ghost)
                                                .child(tr!("settings.default_font")),
                                        )
                                    })
                                    .when(is_selected, |element| {
                                        element.child(icon(
                                            "icons/check.svg",
                                            11.0,
                                            theme.text_secondary,
                                        ))
                                    })
                                    .on_click({
                                        let weak = row_weak.clone();
                                        let popover = row_popover.clone();
                                        move |_, window, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.choose_font_family(
                                                    target,
                                                    name.clone(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                            popover.close(window, cx);
                                            window.refresh();
                                        }
                                    })
                                    .into_any_element()
                            })
                            .size_full(),
                        )
                        .child(scrollbar::vertical(&list_state, &scrollbar_state))
                        .into_any_element()
                };

                div()
                    .w(px(280.0))
                    .rounded(px(16.0))
                    .overflow_hidden()
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.raised)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .on_action(move |_: &SelectNextEntry, _, cx| {
                        let _ = next_weak.update(cx, |this, cx| {
                            this.move_font_selector_highlight(target, "down", &next_families, cx);
                        });
                    })
                    .on_action(move |_: &SelectPreviousEntry, _, cx| {
                        let _ = previous_weak.update(cx, |this, cx| {
                            this.move_font_selector_highlight(target, "up", &previous_families, cx);
                        });
                    })
                    .on_action(move |_: &ConfirmEntry, window, cx| {
                        let should_close = confirm_weak
                            .update(cx, |this, cx| {
                                this.confirm_font_selector(target, &confirm_families, window, cx)
                            })
                            .unwrap_or(false);
                        if should_close {
                            confirm_popover.close(window, cx);
                            window.refresh();
                        }
                    })
                    .child(
                        div().flex_none().p(px(8.0)).child(
                            TextField::new(
                                SharedString::from(format!("{}-filter", target.menu_id())),
                                search.clone(),
                            )
                            .icon("icons/search.svg", 13.0),
                        ),
                    )
                    .child(
                        div()
                            .mx(px(8.0))
                            .h(hairline())
                            .flex_none()
                            .bg(theme.separator),
                    )
                    .child(rows)
                    .child(div().h(px(4.0)))
                    .into_any_element()
            },
        )
    }

    fn font_selector(&self, target: FontTarget) -> &FontSelector {
        match target {
            FontTarget::Ui => &self.ui_font_selector,
            FontTarget::Code => &self.code_font_selector,
        }
    }

    fn font_selector_mut(&mut self, target: FontTarget) -> &mut FontSelector {
        match target {
            FontTarget::Ui => &mut self.ui_font_selector,
            FontTarget::Code => &mut self.code_font_selector,
        }
    }

    /// The name the picker checks: the stored choice, or the built-in default
    /// a `None` resolves to.
    fn font_family_setting(&self, target: FontTarget) -> SharedString {
        let stored = match target {
            FontTarget::Ui => &self.state.ui_font_family,
            FontTarget::Code => &self.state.code_font_family,
        };
        stored
            .as_deref()
            .map(SharedString::from)
            .unwrap_or_else(|| SharedString::from(target.default_family()))
    }

    /// The installed families filtered by the picker's query. Before the
    /// background enumeration lands the list holds only the current face, so
    /// the card still has a row to point at.
    fn visible_font_families(&self, target: FontTarget, cx: &App) -> Vec<SharedString> {
        let query = self
            .font_selector(target)
            .search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();
        crate::fonts::installed(cx)
            .unwrap_or_else(|| vec![self.font_family_setting(target)])
            .into_iter()
            .filter(|name| query.is_empty() || name.to_lowercase().contains(query.as_str()))
            .collect()
    }

    /// Keep the row list in sync with the filtered names — same discipline as
    /// the branch picker so an unchanged frame never resets the scroll.
    fn sync_font_selector_rows(&self, target: FontTarget, rows: &[SharedString]) {
        let selector = self.font_selector(target);
        let mut cached = selector.rows.borrow_mut();
        if cached.as_slice() == rows {
            return;
        }
        *cached = rows.to_vec();
        drop(cached);
        selector
            .list
            .reset_with_uniform_height(rows.len(), px(FONT_PICKER_ROW_HEIGHT));
    }

    /// Reset the picker for a fresh open: clear the filter, drop the cursor,
    /// and flag the reveal so the current family is scrolled into view once
    /// the deferred card has mounted. The family list is warmed at startup,
    /// but a very early open waits on the background enumeration and asks
    /// for a repaint when it lands.
    fn open_font_selector(
        &mut self,
        target: FontTarget,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selector = self.font_selector_mut(target);
        selector.pending_reveal.set(true);
        selector.highlight = None;
        let search = selector.search.clone();
        search.update(cx, |input, cx| input.clear(cx));
        if crate::fonts::installed(cx).is_none() {
            crate::fonts::prefetch(cx);
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(30))
                        .await;
                    let loaded = this
                        .update(cx, |_, cx| crate::fonts::installed(cx).is_some())
                        .unwrap_or(true);
                    if loaded {
                        break;
                    }
                }
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn font_selector_query_edited(
        &mut self,
        target: FontTarget,
        cx: &mut Context<Self>,
    ) {
        if self
            .font_selector(target)
            .search
            .read(cx)
            .content()
            .trim()
            .is_empty()
        {
            self.font_selector_mut(target).highlight = None;
            // Re-center on the applied family, same as the branch picker.
            let families = self.visible_font_families(target, cx);
            let selected = self.font_family_setting(target);
            if let Some(index) = families.iter().position(|name| *name == selected) {
                self.font_selector(target).list.scroll_to_reveal_item(index);
            }
        } else {
            let selector = self.font_selector_mut(target);
            selector.highlight = Some(0);
            selector.list.scroll_to_reveal_item(0);
        }
        cx.notify();
    }

    fn move_font_selector_highlight(
        &mut self,
        target: FontTarget,
        key: &str,
        families: &[SharedString],
        cx: &mut Context<Self>,
    ) {
        let selector = self.font_selector_mut(target);
        let Some(next) = next_picker_highlight(selector.highlight, families.len(), key) else {
            return;
        };
        selector.highlight = Some(next);
        selector.list.scroll_to_reveal_item(next);
        cx.notify();
    }

    /// Apply the keyboard-selected family, returning whether the picker
    /// should dismiss afterward.
    fn confirm_font_selector(
        &mut self,
        target: FontTarget,
        families: &[SharedString],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(name) = families
            .get(self.font_selector(target).highlight.unwrap_or(0))
            .cloned()
        else {
            return false;
        };
        self.choose_font_family(target, name, window, cx);
        true
    }

    /// Store `family` for `target` — `None` when it is the built-in default —
    /// then repaint every surface that shapes with it.
    fn choose_font_family(
        &mut self,
        target: FontTarget,
        family: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = waku_client::persistence::sanitized_font_family(
            (family.as_ref() != target.default_family()).then(|| family.to_string()),
        );
        let stored = match target {
            FontTarget::Ui => &mut self.state.ui_font_family,
            FontTarget::Code => &mut self.state.code_font_family,
        };
        if *stored == value {
            return;
        }
        *stored = value;
        crate::fonts::install(
            self.state.ui_font_family.as_deref(),
            self.state.code_font_family.as_deref(),
            cx,
        );
        // Prose, code, and terminal cells all shape against these faces;
        // markdown flats are dropped by `MarkdownView::sync_style`'s family
        // key and the rest of the measured surfaces by this reset.
        self.remeasure_font_sized_surfaces();
        self.save();
        window.refresh();
        cx.notify();
    }

    fn set_render_math(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.render_math == enabled {
            return;
        }
        self.state.render_math = enabled;
        self.remeasure_font_sized_surfaces();
        self.save();
        cx.notify();
    }

    fn set_show_response_token_speed(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.show_response_token_speed == enabled {
            return;
        }
        self.state.show_response_token_speed = enabled;
        self.save();
        cx.notify();
    }

    pub(super) fn set_markdown_preview(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.markdown_preview == enabled {
            return;
        }
        self.state.markdown_preview = enabled;
        self.save();
        cx.notify();
    }

    fn set_archive_navigation(&mut self, navigation: ArchiveNavigation, cx: &mut Context<Self>) {
        if self.state.archive_navigation == navigation {
            return;
        }
        self.state.archive_navigation = navigation;
        self.save();
        cx.notify();
    }

    fn set_archive_continues_unread_sweep(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.archive_continues_unread_sweep == enabled {
            return;
        }
        self.state.archive_continues_unread_sweep = enabled;
        self.save();
        cx.notify();
    }

    fn set_auto_resolve_land_conflicts(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.auto_resolve_land_conflicts == enabled {
            return;
        }
        self.state.auto_resolve_land_conflicts = enabled;
        self.save();
        cx.notify();
    }

    fn set_auto_commit_reminder_on_land(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.auto_commit_reminder_on_land == enabled {
            return;
        }
        self.state.auto_commit_reminder_on_land = enabled;
        self.save();
        cx.notify();
    }

    fn set_dormant_after_days(&mut self, days: Option<u32>, cx: &mut Context<Self>) {
        if self.state.dormant_after_days == days {
            return;
        }
        self.state.dormant_after_days = days;
        self.save();
        cx.notify();
    }

    fn set_sync_with_merge(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.sync_with_merge == enabled {
            return;
        }
        self.state.sync_with_merge = enabled;
        self.save();
        cx.notify();
    }

    fn set_auto_fetch_remotes(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.auto_fetch_remotes == enabled {
            return;
        }
        self.state.auto_fetch_remotes = enabled;
        self.save();
        cx.notify();
    }

    fn set_auto_resolve_in_chat(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.auto_resolve_in_chat == enabled {
            return;
        }
        self.state.auto_resolve_in_chat = enabled;
        self.save();
        cx.notify();
    }

    fn set_sidebar_shortcut_tags(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.sidebar_shortcut_tags == enabled {
            return;
        }
        self.state.sidebar_shortcut_tags = enabled;
        if !enabled {
            // Take down chips already up and cancel a pending reveal.
            self.sidebar_shortcut_hint_generation =
                self.sidebar_shortcut_hint_generation.wrapping_add(1);
            self.sidebar_shortcut_hints = false;
        }
        self.save();
        cx.notify();
    }

    fn set_sidebar_composer_drafts(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.sidebar_composer_drafts == enabled {
            return;
        }
        self.state.sidebar_composer_drafts = enabled;
        self.save();
        cx.notify();
    }

    fn set_sidebar_draft_preview_color(
        &mut self,
        color: SidebarDraftPreviewColor,
        cx: &mut Context<Self>,
    ) {
        if self.state.sidebar_draft_preview_color == color {
            return;
        }
        self.state.sidebar_draft_preview_color = color;
        self.save();
        cx.notify();
    }

    fn set_default_workspace(&mut self, workspace: DefaultWorkspace, cx: &mut Context<Self>) {
        if self.state.default_workspace == workspace {
            return;
        }
        self.state.default_workspace = workspace;
        self.save();
        cx.notify();
    }

    fn set_new_worktree_default_branch(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.new_worktree_default_branch == enabled {
            return;
        }
        self.state.new_worktree_default_branch = enabled;
        self.save();
        cx.notify();
    }

    fn set_local_workspace_accent(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.local_workspace_accent == enabled {
            return;
        }
        self.state.local_workspace_accent = enabled;
        self.save();
        cx.notify();
    }

    fn set_new_worktree_sync_default_branch(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.new_worktree_sync_default_branch == enabled {
            return;
        }
        self.state.new_worktree_sync_default_branch = enabled;
        self.save();
        cx.notify();
    }

    /// Persist the QA branch field's current name as the user types; the
    /// next review-queue read uses it. An empty field resolves to `qa`
    /// daemon-side.
    pub(super) fn apply_qa_branch(&mut self, cx: &mut Context<Self>) {
        let branch = self.qa_branch_input.read(cx).content().trim().to_owned();
        if branch != self.state.qa_branch {
            self.state.qa_branch = branch;
            self.save();
        }
        cx.notify();
    }

    /// Persist the whitelist field's current names as the user types; the
    /// list applies to the next worktree created.
    pub(super) fn apply_worktree_sync_branches(&mut self, cx: &mut Context<Self>) {
        let branches =
            sync_branches_from_text(&self.worktree_sync_branches_input.read(cx).content());
        if branches != self.state.new_worktree_sync_branches {
            self.state.new_worktree_sync_branches = branches;
            self.save();
        }
        cx.notify();
    }

    fn set_ui_font_size(&mut self, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        let size = waku_client::persistence::sanitized_ui_font_size(size);
        if self.state.ui_font_size == size {
            return;
        }
        self.state.ui_font_size = size;
        // Chrome is authored in `sp` rems; the rem size is the setting.
        window.set_rem_size(px(size));
        self.remeasure_font_sized_surfaces();
        self.save();
        window.refresh();
        cx.notify();
    }

    fn set_code_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = waku_client::persistence::sanitized_code_font_size(size);
        if self.state.code_font_size == size {
            return;
        }
        // The terminal follows the code size only while the two match; `None`
        // keeps it resolving to whatever the code size becomes.
        let terminal_follows = self.state.terminal_font_size() == self.state.code_font_size;
        self.state.code_font_size = size;
        if terminal_follows {
            self.state.terminal_font_size = None;
            crate::terminal::install_font_size(size, cx);
        }
        self.remeasure_font_sized_surfaces();
        self.save();
        cx.notify();
    }

    fn set_terminal_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let Some(size) = waku_client::persistence::sanitized_terminal_font_size(Some(size)) else {
            return;
        };
        // Picking the code size re-links the two: `None` resolves to the code
        // size, so the terminal keeps following future code-size changes.
        let stored = (size != self.state.code_font_size).then_some(size);
        if self.state.terminal_font_size == stored {
            return;
        }
        self.state.terminal_font_size = stored;
        crate::terminal::install_font_size(size, cx);
        self.save();
        cx.notify();
    }

    /// `secondary-=` / `secondary--`: step one `FONT_SIZES` preset in the
    /// direction given, against whichever surface the binding's key context
    /// routed the chord to.
    pub(super) fn adjust_font_size_action(
        &mut self,
        action: &crate::AdjustFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = match action.target {
            crate::FontSizeTarget::Ui => self.state.ui_font_size,
            crate::FontSizeTarget::Code => self.state.code_font_size,
            crate::FontSizeTarget::Terminal => self.state.terminal_font_size(),
        };
        let size = stepped_font_size(current, action.direction);
        match action.target {
            crate::FontSizeTarget::Ui => self.set_ui_font_size(size, window, cx),
            crate::FontSizeTarget::Code => self.set_code_font_size(size, cx),
            crate::FontSizeTarget::Terminal => self.set_terminal_font_size(size, cx),
        }
    }

    /// Drop every cached row height that a font size participates in. The
    /// virtualized lists remember measured heights, so a stale entry would
    /// misplace scroll anchors until the row happened to remeasure. The
    /// sidebar list keeps its uniform row height and needs no reset.
    fn remeasure_font_sized_surfaces(&self) {
        self.reset_transcript_rows(self.transcript_row_count());
        let line_count = self
            .right_panel_diff_snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.lines.len());
        self.right_panel_diff_list_state.reset(line_count);
        self.right_panel_diff_tree_list_state
            .reset(self.right_panel_diff_tree_rows.borrow().len());
        self.skills_list_state
            .reset(self.skills_rows.borrow().len());
    }

    fn render_providers_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let checking = self.provider_detection_remaining > 0;
        let detection_pending = self.provider_detection_checked_at.is_none();
        let checked_label = self
            .provider_detection_checked_at
            .filter(|_| !checking)
            .map(|checked_at| detection_checked_label(checked_at.elapsed()));

        let refresh = div()
            .id("refresh-providers")
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .h(px(28.0))
            .px(px(11.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .opacity(if checking { 0.6 } else { 1.0 })
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/rotate-cw.svg", 11.0, theme.text_tertiary))
            .child(if checking {
                tr!("common.checking")
            } else {
                tr!("common.refresh")
            })
            .on_activation(cx, |this, _, cx| {
                this.refresh_provider_detection(None);
                cx.notify();
            });

        let mut provider_rows: Vec<AnyElement> = Vec::new();
        for kind in ProviderKind::ALL {
            let probe = self.provider_probe(kind);
            let installed = probe.is_some_and(|probe| probe.installed);
            let binary_path = probe
                .filter(|probe| probe.installed)
                .and_then(|probe| probe.path.as_deref())
                .map(|path| abbreviate_home_path(path, self.home_directory.as_deref()));
            let model_count = probe.map(|probe| probe.models.len()).unwrap_or(0);
            let version = (!detection_pending)
                .then(|| {
                    self.provider_versions
                        .get(&kind)
                        .and_then(|version| version.clone())
                })
                .flatten();
            let disabled = self.state.disabled_providers.contains(&kind);

            let dot_color = if !installed {
                theme.text_ghost
            } else if disabled {
                theme.warning
            } else {
                theme.success
            };

            // Until the first detection completes the probe state is
            // unknowable — a "Checking…" placeholder keeps a real install
            // from flashing "Not detected" plus a Set up button.
            let detail_text: String = if detection_pending {
                tr!("common.checking")
            } else if installed {
                let mut parts = Vec::new();
                if let Some(path) = binary_path {
                    parts.push(path);
                }
                if disabled {
                    parts.push(tr!("providers.disabled_for_new_tasks"));
                } else if model_count > 0 {
                    parts.push(if model_count == 1 {
                        tr!("providers.model_count_one", count = model_count)
                    } else {
                        tr!("providers.model_count_many", count = model_count)
                    });
                }
                parts.join("  ·  ")
            } else {
                tr!("providers.not_detected_as", command = kind.command())
            };

            // The provider row is searchable on its name and the detail line
            // under it — path, model count, or the not-detected hint.
            let Some(matched) = search.matched(kind.display_name(), &detail_text) else {
                continue;
            };
            let detail: AnyElement = if installed {
                div()
                    .truncate()
                    .child(settings_search_text(
                        detail_text,
                        matched.description_ranges.clone(),
                        theme,
                    ))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .items_baseline()
                    .child(settings_search_text(
                        detail_text,
                        matched.description_ranges.clone(),
                        theme,
                    ))
                    .into_any_element()
            };

            let toggle_on = !disabled;
            let toggle = toggle_switch(
                SharedString::from(format!("provider-enabled-{}", kind.id())),
                toggle_on,
                false,
                theme,
                cx,
                move |this, _, cx| this.set_provider_enabled(kind, disabled, cx),
            );

            let expanded = self.expanded_provider_settings == Some(kind);
            let expand_button = icon_button_tinted(
                SharedString::from(format!("provider-expand-{}", kind.id())),
                if expanded {
                    "icons/chevron-down.svg"
                } else {
                    "icons/chevron-right.svg"
                },
                theme,
                theme.affordance_icon(),
            )
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                this.toggle_provider_expanded(kind, cx);
            });

            let setup_button = (!installed && !detection_pending).then(|| {
                div()
                    .id(SharedString::from(format!("provider-setup-{}", kind.id())))
                    .tab_index(0)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .h(px(28.0))
                    .px(px(11.0))
                    .rounded(px(9.0))
                    .border(hairline())
                    .border_color(theme.border_strong)
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_default()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .hover(|element| element.bg(theme.overlay))
                    .child(icon("icons/download.svg", 11.0, theme.text_tertiary))
                    .child(tr!("providers.set_up"))
                    .on_activation(cx, move |this, _, cx| {
                        this.provider_setup_clicked(kind, cx);
                    })
            });

            // Updates run where the CLI lives, so like Run in terminal the
            // button stays hidden against a remote daemon — the expanded
            // row's copyable command is the remote path.
            let updating = self.provider_update_runs.contains(&kind)
                && self.provider_setup_terminals.contains_key(&kind);
            let update_button =
                (installed && !detection_pending && !self.daemon.is_remote()).then(|| {
                    div()
                        .id(SharedString::from(format!("provider-update-{}", kind.id())))
                        .tab_index(0)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .h(px(28.0))
                        .px(px(11.0))
                        .rounded(px(9.0))
                        .border(hairline())
                        .border_color(theme.border_strong)
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.0))
                        .cursor_default()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .hover(|element| element.bg(theme.overlay))
                        .child(icon("icons/download.svg", 11.0, theme.text_tertiary))
                        .child(if updating {
                            tr!("providers.updating")
                        } else {
                            tr!("providers.update")
                        })
                        .tooltip(Tooltip::text(kind.setup().update_command()))
                        .on_activation(cx, move |this, window, cx| {
                            this.provider_update_clicked(kind, window, cx);
                        })
                });

            let header = div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .relative()
                        .w(px(30.0))
                        .h(px(30.0))
                        .flex_none()
                        .rounded(px(9.0))
                        .bg(theme.overlay)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(provider_mark(
                            &theme,
                            kind,
                            16.0,
                            theme
                                .text_secondary
                                .opacity(if installed { 1.0 } else { 0.5 }),
                        ))
                        .child(
                            div()
                                .absolute()
                                .bottom(px(-2.0))
                                .right(px(-2.0))
                                .w(px(10.0))
                                .h(px(10.0))
                                .rounded_full()
                                .border_2()
                                .border_color(theme.raised)
                                .bg(dot_color),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(7.0))
                                .child(settings_title_jump(
                                    div()
                                        .text_size(sp(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if installed {
                                            theme.text
                                        } else {
                                            theme.text_secondary
                                        })
                                        .child(settings_search_text(
                                            kind.display_name(),
                                            matched.title_ranges.clone(),
                                            theme,
                                        )),
                                    &matched,
                                    theme,
                                ))
                                .when_some(version, |element, version| {
                                    element.child(
                                        div()
                                            .font_family(crate::fonts::current(cx).code)
                                            .text_size(sp(12.5))
                                            .text_color(theme.text_tertiary)
                                            .child(SharedString::from(format!("v{version}"))),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .mt(px(3.0))
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(detail),
                        ),
                )
                .when_some(setup_button, |element, button| element.child(button))
                .when_some(update_button, |element, button| element.child(button))
                .child(expand_button)
                .when(installed && !detection_pending, |element| {
                    element.child(toggle)
                });

            provider_rows.push(
                div()
                    .py(px(11.0))
                    .flex()
                    .flex_col()
                    .child(header)
                    .when(expanded && !search.active(), |element| {
                        element.child(self.render_provider_expanded_settings(kind, theme, cx))
                    })
                    .into_any_element(),
            );
        }
        let any_rows = !provider_rows.is_empty();
        let last_row = provider_rows.len().saturating_sub(1);
        let mut rows = div().mt(px(4.0)).flex().flex_col();
        for (index, row) in provider_rows.into_iter().enumerate() {
            rows = rows.child(
                div()
                    .when(index != last_row, |element| {
                        element.border_b(hairline()).border_color(theme.separator)
                    })
                    .child(row),
            );
        }

        let header = {
            let title = tr!("providers.coding_agents");
            let description = tr!("providers.description");
            search.matched(&title, &description).map(|matched| {
                settings_row_label(
                    "icons/bot.svg",
                    settings_row_text(title, description, matched, theme),
                    theme,
                )
            })
        };
        // The card drops out of the results entirely when neither its header
        // nor a provider row matched.
        if search.active() && header.is_none() && !any_rows {
            return div().into_any_element();
        }

        div()
            .mt(px(15.0))
            .w_full()
            .px(px(20.0))
            .py(px(14.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(20.0))
                    .children(header)
                    .when(!search.active(), |element| {
                        element.child(
                            div()
                                .flex_none()
                                .flex()
                                .flex_col()
                                .items_end()
                                .gap(px(6.0))
                                .child(refresh)
                                // The label line is always present — an nbsp
                                // reserves its height so a label arriving or
                                // leaving can't shift the rows below.
                                .child(
                                    div()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_ghost)
                                        .child(SharedString::from(
                                            checked_label.unwrap_or_else(|| "\u{00a0}".to_owned()),
                                        )),
                                ),
                        )
                    }),
            )
            .child(rows)
            .into_any_element()
    }

    /// The expanded row's settings body: the binary override for this
    /// provider, with the detection result as its caption.
    fn render_provider_expanded_settings(
        &self,
        kind: ProviderKind,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let override_value = self.state.provider_binary_overrides.get(&kind).cloned();
        let full_path = self
            .provider_probe(kind)
            .filter(|probe| probe.installed)
            .and_then(|probe| probe.path.as_ref())
            .map(|path| path.display().to_string());

        let caption = match (&override_value, full_path) {
            (Some(_), Some(path)) => tr!("providers.using_override", path = path),
            (Some(_), None) => tr!("providers.invalid_override"),
            (None, Some(path)) => tr!("providers.detected_at", path = path),
            (None, None) => tr!("providers.searches_path", command = kind.command()),
        };

        let reset = div()
            .id(SharedString::from(format!(
                "provider-path-reset-{}",
                kind.id()
            )))
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .h(px(29.0))
            .px(px(10.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .flex_none()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .child(tr!("common.reset"))
            .on_activation(cx, |this, _, cx| {
                this.provider_path_input
                    .update(cx, |input, cx| input.clear(cx));
                this.apply_provider_path_override(cx);
            });

        div()
            .mt(px(10.0))
            .pl(px(42.0))
            .flex()
            .flex_col()
            .gap(px(5.0))
            .child(self.render_provider_setup_section(kind, theme, cx))
            .child(
                div()
                    .mt(px(6.0))
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(tr!("providers.binary_path")),
            )
            .child(
                div()
                    .text_size(sp(12.5))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(tr!(
                        "providers.binary_path_description",
                        provider = kind.short_name()
                    ))),
            )
            .child(
                div()
                    .mt(px(3.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        TextField::new(
                            SharedString::from(format!("provider-path-field-{}", kind.id())),
                            self.provider_path_input.clone(),
                        )
                        .flex_1()
                        .max_w(px(430.0)),
                    )
                    .when(override_value.is_some(), |element| element.child(reset)),
            )
            .child(
                div()
                    .text_size(sp(12.5))
                    .text_color(theme.text_ghost)
                    .child(SharedString::from(caption)),
            )
            .when(
                !matches!(kind, ProviderKind::DeepSeek | ProviderKind::Fx),
                |element| element.child(self.title_model_row(kind, theme, cx)),
            )
    }

    fn set_title_model(
        &mut self,
        provider: ProviderKind,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match model {
            Some(model) => {
                self.state.title_models.insert(provider, model);
            }
            None => {
                self.state.title_models.remove(&provider);
            }
        }
        self.save();
        cx.notify();
    }

    fn title_model_row(&self, provider: ProviderKind, theme: Theme, cx: &mut Context<Self>) -> Div {
        let models = self
            .provider_probe(provider)
            .map(|probe| probe.models.clone())
            .unwrap_or_default();
        let selected = self.state.title_models.get(&provider).cloned();
        let default = match provider {
            ProviderKind::Claude => Some(waku_protocol::git::CLAUDE_COMMIT_MODEL),
            ProviderKind::Codex => Some(waku_protocol::git::CODEX_COMMIT_MODEL),
            _ => None,
        };
        let label = selected
            .as_deref()
            .or(default)
            .map(|id| {
                models
                    .iter()
                    .find(|model| model.id == id)
                    .map(|model| model.name.as_str())
                    .unwrap_or(id)
            })
            .map(str::to_owned)
            .unwrap_or_else(|| tr!("providers.title_model_choose"));
        let handle = self.menu_handle(format!("title-model-{}", provider.id()), cx);
        let weak = cx.entity().downgrade();
        div()
            .mt(px(12.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .text_color(theme.text)
                            .child(tr!("providers.title_model")),
                    )
                    .child(
                        div()
                            .text_size(sp(11.5))
                            .text_color(theme.text_tertiary)
                            .child(tr!("providers.title_model_description")),
                    ),
            )
            .child(dropdown_menu(
                MenuChip::new(format!("title-model-selector-{}", provider.id()))
                    .label(label)
                    .outlined()
                    .selected(handle.is_open())
                    .w(px(180.0))
                    .justify_between(),
                format!("title-model-menu-{}", provider.id()),
                &handle,
                MenuAlign::BelowRight,
                move |_| {
                    let mut items = vec![{
                        let weak = weak.clone();
                        MenuItem::new(
                            if default.is_some() {
                                tr!("providers.title_model_default")
                            } else {
                                tr!("providers.title_model_disabled")
                            },
                            move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.set_title_model(provider, None, cx)
                                });
                            },
                        )
                        .selected(selected.is_none())
                    }];
                    items.extend(models.iter().map(|model| {
                        let weak = weak.clone();
                        let id = model.id.clone();
                        let chosen = selected.as_deref() == Some(id.as_str());
                        MenuItem::new(model.name.clone(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_title_model(provider, Some(id.clone()), cx)
                            });
                        })
                        .selected(chosen)
                    }));
                    items
                },
            ))
    }

    /// The expanded row's setup block: the provider's documented install and
    /// sign-in commands verbatim, each with a copy button, plus a Docs link
    /// and a Run button that executes them in a terminal embedded in the row.
    /// A remote daemon can't use a local PTY, so there Run stays hidden.
    fn render_provider_setup_section(
        &self,
        kind: ProviderKind,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let setup = kind.setup();
        let installed = self
            .provider_probe(kind)
            .is_some_and(|probe| probe.installed);
        let terminal = self.provider_setup_terminals.get(&kind).cloned();

        let mut steps = div().mt(px(2.0)).flex().flex_col().gap(px(6.0));
        if installed {
            steps = steps.child(self.provider_setup_step_row(
                kind,
                "update",
                tr!("providers.update_label"),
                setup.update_command(),
                theme,
                cx,
            ));
        } else {
            steps = steps.child(self.provider_setup_step_row(
                kind,
                "install",
                tr!("providers.install_label"),
                setup.install,
                theme,
                cx,
            ));
        }
        if let Some(sign_in) = setup.sign_in {
            steps = steps.child(self.provider_setup_step_row(
                kind,
                "sign-in",
                tr!("providers.sign_in_label"),
                sign_in,
                theme,
                cx,
            ));
        }

        let mut actions = div().mt(px(4.0)).flex().items_center().gap(px(8.0));
        if !self.daemon.is_remote()
            && let Some(script) = self.provider_setup_script(kind)
        {
            let run = div()
                .id(SharedString::from(format!(
                    "provider-run-setup-{}",
                    kind.id()
                )))
                .tab_index(0)
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .h(px(29.0))
                .px(px(10.0))
                .rounded(px(9.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .child(icon("icons/terminal.svg", 11.0, theme.text_tertiary))
                .child(tr!("providers.run_in_terminal"))
                .tooltip(Tooltip::text(script))
                .on_activation(cx, move |this, window, cx| {
                    this.run_provider_setup(kind, window, cx);
                });
            actions = actions.child(run);
        }
        let docs_url = setup.docs_url;
        let docs = div()
            .id(SharedString::from(format!("provider-docs-{}", kind.id())))
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .h(px(29.0))
            .px(px(10.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/external-link.svg", 11.0, theme.text_tertiary))
            .child(tr!("providers.docs"))
            .on_activation(cx, move |_, _, cx| cx.open_url(docs_url));
        actions = actions.child(docs);
        if let Some(env) = setup.api_key_env {
            actions = actions.child(
                div()
                    .text_size(sp(12.5))
                    .text_color(theme.text_ghost)
                    .child(SharedString::from(tr!("providers.api_key_hint", var = env))),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(5.0))
            .child(
                div()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(tr!("providers.setup")),
            )
            .child(steps)
            .child(actions)
            .when_some(terminal, |element, view| {
                element.child(
                    div()
                        .mt(px(4.0))
                        .h(px(280.0))
                        .border(hairline())
                        .border_color(theme.border)
                        .relative()
                        .child(view)
                        .child(
                            div().absolute().top(px(6.0)).right(px(6.0)).child(
                                icon_button(
                                    SharedString::from(format!(
                                        "provider-setup-close-{}",
                                        kind.id()
                                    )),
                                    "icons/x.svg",
                                    theme,
                                )
                                .tab_index(0)
                                .focus_visible(|style| style.bg(theme.focus_highlight()))
                                .on_activation(
                                    cx,
                                    move |this, _, cx| {
                                        this.dismiss_provider_setup_terminal(kind, cx);
                                    },
                                ),
                            ),
                        ),
                )
            })
    }

    /// One setup step: its label, the exact command in code, and a copy
    /// button.
    fn provider_setup_step_row(
        &self,
        kind: ProviderKind,
        step: &'static str,
        label: String,
        command: &'static str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let copy_id = format!("provider-setup-copy-{}-{}", kind.id(), step);
        let copied = self.control_was_copied(&copy_id);
        let copy = div()
            .id(SharedString::from(copy_id.clone()))
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .size(px(22.0))
            .rounded(px(7.0))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .child(icon(
                if copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                theme.text_tertiary,
            ))
            .on_activation(cx, move |this, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(command.to_owned()));
                this.show_control_copied(copy_id.clone(), cx);
            });
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(56.0))
                    .flex_none()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "provider-setup-command-{}-{}",
                        kind.id(),
                        step
                    )))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(crate::fonts::current(cx).code)
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .tooltip(Tooltip::text(command))
                    .child(SharedString::from(command)),
            )
            .child(copy)
    }

    /// The script a provider's Run button executes: install when the CLI is
    /// undetected — chained into sign-in when the provider has one — or
    /// sign-in alone for an installed CLI. `None` when nothing is runnable.
    fn provider_setup_script(&self, provider: ProviderKind) -> Option<String> {
        let setup = provider.setup();
        let installed = self
            .provider_probe(provider)
            .is_some_and(|probe| probe.installed);
        match (installed, setup.sign_in) {
            (false, Some(sign_in)) => Some(format!("{} && {}", setup.install, sign_in)),
            (false, None) => Some(setup.install.to_owned()),
            (true, Some(sign_in)) => Some(sign_in.to_owned()),
            (true, None) => None,
        }
    }

    /// The row's Set up button: expand the provider's settings so its
    /// documented commands, copy buttons, and Run button are visible — the
    /// expanded row's Run stays the explicit execution step, since these are
    /// `curl | bash` installers the user should see before they run. A click
    /// on an already-expanded row is a no-op: collapsing it could hide a
    /// running setup terminal.
    fn provider_setup_clicked(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        if self.expanded_provider_settings != Some(provider) {
            self.toggle_provider_expanded(provider, cx);
        }
    }

    /// The row's Update button: expand the provider's settings so the update
    /// command and its terminal are visible, then run it. A click on an
    /// already-expanded row is still just the run — collapsing would hide a
    /// live update terminal.
    fn provider_update_clicked(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.expanded_provider_settings != Some(provider) {
            self.toggle_provider_expanded(provider, cx);
        }
        self.run_provider_update(provider, window, cx);
    }

    /// The command a provider's Update button runs: the CLI's own updater
    /// when it has one, else its install one-liner — every documented
    /// installer fetches the latest release. `None` for a CLI that isn't
    /// installed.
    fn provider_update_script(&self, provider: ProviderKind) -> Option<String> {
        self.provider_probe(provider)
            .is_some_and(|probe| probe.installed)
            .then(|| provider.setup().update_command().to_owned())
    }

    /// Run the setup script in a terminal embedded in the expanded row. The
    /// command closes its own shell on success; the exit event drops the
    /// embed and re-detects the provider.
    fn run_provider_setup(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(script) = self.provider_setup_script(provider) else {
            return;
        };
        self.run_provider_script(
            provider,
            script,
            tr!(
                "providers.setup_terminal_title",
                provider = provider.display_name()
            ),
            false,
            window,
            cx,
        );
    }

    /// Run the provider's update command in the same embedded terminal the
    /// setup flow uses, flagged so exit reporting routes to
    /// `provider.update.finished` rather than `provider.setup.finished`.
    fn run_provider_update(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(script) = self.provider_update_script(provider) else {
            return;
        };
        self.run_provider_script(
            provider,
            script,
            tr!(
                "providers.update_terminal_title",
                provider = provider.display_name()
            ),
            true,
            window,
            cx,
        );
    }

    /// Run a provider maintenance script — setup or update — in a terminal
    /// embedded in the expanded row. The command closes its own shell on
    /// success; the exit event drops the embed and re-detects the provider.
    fn run_provider_script(
        &mut self,
        provider: ProviderKind,
        script: String,
        title: String,
        update: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.daemon.is_remote() {
            return;
        }
        // A live embed owns the slot — refocus it rather than killing the
        // in-flight install by replacing the map entry.
        if let Some(view) = self.provider_setup_terminals.get(&provider) {
            let focus = view.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
            return;
        }
        if update {
            self.provider_update_runs.insert(provider);
        }
        let mut command = CustomCommand::new(script);
        command.name = Some(title);
        command.icon = CustomCommandIcon::Download;
        command.close_on_success = true;
        let cwd = self
            .home_directory
            .clone()
            .unwrap_or_else(|| PathBuf::from("/"));
        let view =
            cx.new(|cx| TerminalView::embedded(cwd, TerminalLaunch::CustomCommand(command), cx));
        cx.subscribe(&view, move |this, _, event: &TerminalViewEvent, cx| {
            match event {
                // The shell is gone — drop the embed and re-detect so the row
                // reflects whatever the script changed.
                TerminalViewEvent::Exited => {
                    this.provider_setup_terminal_exited(provider, cx);
                }
                // A failed script leaves its shell open with the error
                // visible — never tear the embed down on the finish report.
                // An install may still have succeeded mid-script (a sign-in
                // that bailed after the CLI landed), so re-detect on a
                // non-clean finish too. A clean finish from a typed-input
                // launch leaves its shell running, so the report itself is
                // the close signal the sourced line's `exit` provided.
                TerminalViewEvent::CommandFinished(code) => {
                    if *code == Some(0) {
                        this.provider_setup_terminal_exited(provider, cx);
                    } else {
                        this.refresh_provider_detection(Some(provider));
                    }
                }
                TerminalViewEvent::ActivityChanged
                | TerminalViewEvent::LocalhostUrl(_)
                | TerminalViewEvent::GenerateCommand { .. } => {}
            }
        })
        .detach();
        self.provider_setup_terminals.insert(provider, view.clone());
        let focus = view.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The setup terminal's run ended — the shell exited, or a typed-input
    /// launch reported success. Drop the embed and re-detect so the row
    /// reflects whatever the script changed. A sourced-file launch reports
    /// success and then exits its shell, so both signals arrive; only the
    /// first acts.
    fn provider_setup_terminal_exited(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        if self.provider_setup_terminals.remove(&provider).is_none() {
            return;
        }
        if self.provider_update_runs.remove(&provider) {
            self.provider_update_outcomes.insert(provider);
        } else {
            self.provider_setup_outcomes.insert(provider);
        }
        self.refresh_provider_detection(Some(provider));
        cx.notify();
    }

    fn dismiss_provider_setup_terminal(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        self.provider_setup_terminals.remove(&provider);
        self.provider_update_runs.remove(&provider);
        cx.notify();
    }

    fn toggle_provider_expanded(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        // Commit any pending edit for the previously expanded provider before
        // the input is handed to another row.
        self.apply_provider_path_override(cx);
        if self.expanded_provider_settings == Some(provider) {
            self.expanded_provider_settings = None;
        } else {
            self.expanded_provider_settings = Some(provider);
            let override_value = self
                .state
                .provider_binary_overrides
                .get(&provider)
                .cloned()
                .unwrap_or_default();
            self.provider_path_input
                .update(cx, |input, cx| input.set_content(override_value, cx));
        }
        cx.notify();
    }

    /// Commit the binary override edit for the expanded provider: empty means
    /// detect from PATH. Re-detects that provider and refreshes every catalog
    /// keyed by the executable path.
    pub(super) fn apply_provider_path_override(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.expanded_provider_settings else {
            return;
        };
        let text = self
            .provider_path_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        let current = self
            .state
            .provider_binary_overrides
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        if text == current {
            return;
        }
        if text.is_empty() {
            self.state.provider_binary_overrides.remove(&provider);
        } else {
            self.state.provider_binary_overrides.insert(provider, text);
        }
        self.save();
        self.refresh_provider_detection(Some(provider));
        self.refresh_composer_sources(cx);
        cx.notify();
    }

    /// Providers switched off here stop offering models to new sessions;
    /// sessions already locked to them keep working.
    fn set_provider_enabled(
        &mut self,
        provider: ProviderKind,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if enabled {
            self.state
                .disabled_providers
                .retain(|kind| *kind != provider);
        } else if !self.state.disabled_providers.contains(&provider) {
            self.state.disabled_providers.push(provider);
        }
        if !enabled
            && let Some(fallback) = ProviderKind::ALL
                .into_iter()
                .find(|kind| self.provider_enabled(*kind))
        {
            // New work must land somewhere usable: move the new-session
            // default and any unstarted drafts off the switched-off provider.
            // The remembered model belongs to the old provider, so it resets
            // with it.
            if self.state.last_provider == provider {
                self.state.last_provider = fallback;
                self.state.last_model = None;
                self.state.last_reasoning_effort = None;
                self.state.last_service_tier = None;
                self.state.last_context_window = None;
            }
            let draft_ids = self
                .state
                .sessions
                .iter()
                .filter(|session| session.provider == provider && !session.has_started())
                .map(|session| session.id)
                .collect::<Vec<_>>();
            for id in draft_ids {
                if let Some(session) = self.state.session_mut(id) {
                    session.provider = fallback;
                    session.model = None;
                    session.reasoning_effort = None;
                    session.service_tier = None;
                    session.context_window = None;
                }
            }
        }
        self.save();
        cx.notify();
    }

    fn render_computer_use_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let enabled = self.state.computer_use_enabled;
        let permissions = self.computer_permissions.clone();
        let pending = self.computer_permission_request_pending;
        let helper_name = crate::computer_use::helper_display_name();
        let mut allowed_apps = div().flex().flex_col().gap(px(1.0));
        let verified_apps = self
            .state
            .computer_use_allowed_apps
            .iter()
            .filter(|grant| grant.verified)
            .collect::<Vec<_>>();
        if verified_apps.is_empty() {
            allowed_apps = allowed_apps.child(
                div()
                    .py(px(12.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("computer_use.no_always_allowed_apps")),
            );
        } else {
            for (index, grant) in verified_apps.iter().enumerate() {
                let key = grant.key();
                let revoke_name = grant.app_name.clone();
                let is_last = index + 1 == verified_apps.len();
                let app_icon = self.computer_use_app_icon(&grant.bundle_id, cx);
                allowed_apps = allowed_apps.child(
                    div()
                        .py(px(9.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .when(!is_last, |element| {
                            element.border_b(hairline()).border_color(theme.separator)
                        })
                        .child(
                            div()
                                .w(px(32.0))
                                .h(px(32.0))
                                .flex_none()
                                .rounded(px(9.0))
                                .when_some(app_icon, |element, app_icon| {
                                    element.child(img(app_icon).size_full().rounded(px(9.0)))
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(sp(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child(SharedString::from(grant.app_name.clone())),
                                )
                                .child(
                                    div()
                                        .mt(px(2.0))
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_tertiary)
                                        .truncate()
                                        .child(SharedString::from(grant.bundle_id.clone())),
                                ),
                        )
                        .child(settings_button(
                            SharedString::from(format!("revoke-computer-app-{key}")),
                            tr!("common.revoke"),
                            true,
                            true,
                            true,
                            theme,
                            cx,
                            move |_this, window, cx| {
                                let answer = window.prompt(
                                    gpui::PromptLevel::Warning,
                                    &tr!("computer_use.confirm_revoke", name = revoke_name.clone()),
                                    Some(&tr!("computer_use.confirm_revoke_detail")),
                                    &[
                                        gpui::PromptButton::cancel(tr!("common.cancel")),
                                        gpui::PromptButton::ok(tr!("common.revoke")),
                                    ],
                                    cx,
                                );
                                let key = key.clone();
                                cx.spawn(async move |this, cx| {
                                    if answer.await.ok() != Some(1) {
                                        return;
                                    }
                                    let _ = this.update(cx, |this, cx| {
                                        this.revoke_computer_app(&key, cx);
                                    });
                                })
                                .detach();
                            },
                        )),
                );
            }
        }

        let allow_card = {
            let title = tr!("computer_use.allow_apps");
            let description = tr!("computer_use.availability");
            search.matched(&title, &description).map(|matched| {
                div()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(20.0))
                    .child(settings_row_label(
                        "icons/laptop.svg",
                        settings_row_text(title, description, matched, theme),
                        theme,
                    ))
                    .child(toggle_switch(
                        "computer-use-enabled",
                        enabled,
                        false,
                        theme,
                        cx,
                        move |this, _, cx| this.set_computer_use_enabled(!enabled, cx),
                    ))
                    .into_any_element()
            })
        };

        let macos_card = if !cfg!(target_os = "macos") {
            None
        } else {
            let before = search.hits();
            let header = {
                let title = tr!("computer_use.macos_access");
                let description = tr!("computer_use.helper_access", helper = helper_name);
                search.matched(&title, &description).map(|matched| {
                    settings_row_label(
                        "icons/lock-open.svg",
                        div()
                            .child(settings_title_jump(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(settings_search_text(
                                        title,
                                        matched.title_ranges.clone(),
                                        theme,
                                    )),
                                &matched,
                                theme,
                            ))
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_secondary)
                                    .child(settings_search_text(
                                        description,
                                        matched.description_ranges.clone(),
                                        theme,
                                    )),
                            ),
                        theme,
                    )
                })
            };
            let screen_row = permission_status_row(
                "icons/monitor.svg",
                tr!("computer_use.screen_recording"),
                tr!("computer_use.screen_recording_description"),
                permissions.screen_recording,
                "screen-recording-settings",
                theme,
                search,
                cx,
            );
            let accessibility_row = permission_status_row(
                "icons/hand.svg",
                tr!("computer_use.accessibility"),
                tr!("computer_use.accessibility_description"),
                permissions.accessibility,
                "accessibility-settings",
                theme,
                search,
                cx,
            );
            if search.active() && search.hits() == before {
                None
            } else {
                Some(
                    div()
                        .px(px(20.0))
                        .py(px(14.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .children(header)
                        .children(screen_row)
                        .children(accessibility_row)
                        .when(!search.active(), |card| {
                            card.child(
                                div().mt(px(11.0)).flex().items_center().gap(px(8.0)).child(
                                    div()
                                        .id("recheck-computer-permissions")
                                        .tab_index(0)
                                        .h(px(28.0))
                                        .px(px(11.0))
                                        .rounded(px(9.0))
                                        .border(hairline())
                                        .border_color(theme.border_strong)
                                        .text_color(theme.text_secondary)
                                        .flex()
                                        .items_center()
                                        .cursor_default()
                                        .text_size(sp(12.5))
                                        .opacity(if pending { 0.6 } else { 1.0 })
                                        .focus_visible(|element| element.border_color(theme.accent))
                                        .child(if pending {
                                            tr!("common.checking")
                                        } else {
                                            tr!("common.recheck")
                                        })
                                        .on_activation(cx, |this, _, cx| {
                                            this.request_computer_permissions(false, cx);
                                        }),
                                ),
                            )
                        })
                        .into_any_element(),
                )
            }
        };

        let desktop_card = if cfg!(target_os = "macos") {
            None
        } else {
            let title = tr!("computer_use.desktop_access");
            let description = if cfg!(target_os = "windows") {
                tr!("computer_use.windows_access_description")
            } else {
                tr!("computer_use.linux_access_description")
            };
            search.matched(&title, &description).map(|matched| {
                div()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .child(settings_row_label(
                        "icons/laptop.svg",
                        settings_row_text(title, description, matched, theme),
                        theme,
                    ))
                    .into_any_element()
            })
        };

        let apps_card = {
            let title = tr!("computer_use.always_allowed_apps");
            let description = tr!("computer_use.always_allowed_apps_description");
            search.matched(&title, &description).map(|matched| {
                div()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .child(settings_row_label(
                        "icons/package.svg",
                        div()
                            .child(settings_title_jump(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(settings_search_text(
                                        title,
                                        matched.title_ranges.clone(),
                                        theme,
                                    )),
                                &matched,
                                theme,
                            ))
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_secondary)
                                    .child(settings_search_text(
                                        description,
                                        matched.description_ranges.clone(),
                                        theme,
                                    )),
                            ),
                        theme,
                    ))
                    .when(!search.active(), |card| card.child(allowed_apps))
                    .into_any_element()
            })
        };

        div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .children(allow_card)
            .children(macos_card)
            .children(desktop_card)
            .children(apps_card)
            .into_any_element()
    }

    fn set_computer_use_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.computer_use_enabled = enabled;
        self.analytics
            .track(crate::analytics::Event::ComputerUseToggled { enabled });
        self.save();
        if enabled {
            self.request_computer_permissions(true, cx);
        }
        cx.notify();
    }

    pub(super) fn request_computer_permissions(&mut self, prompt: bool, cx: &mut Context<Self>) {
        if !cfg!(target_os = "macos")
            || !self.state.computer_use_experiment_enabled
            || self.computer_permission_request_pending
        {
            return;
        }
        self.computer_permission_request_pending = true;
        let tx = self.computer_permission_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let daemon = self.daemon.client();
        std::thread::Builder::new()
            .name("waku-computer-permission-request".into())
            .spawn(move || {
                let result = match daemon.request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::ProbeComputerPermissions { prompt },
                ) {
                    Ok(waku_client::ResponsePayload::ComputerPermissions { permissions }) => {
                        Ok(permissions)
                    }
                    Ok(_) => Err("the daemon returned an invalid permission response".into()),
                    Err(error) => Err(error.to_string()),
                };
                if tx.send(result).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .ok();
        cx.notify();
    }

    fn revoke_computer_app(&mut self, key: &str, cx: &mut Context<Self>) {
        self.state
            .computer_use_allowed_apps
            .retain(|grant| grant.key() != key);
        self.save();
        cx.notify();
    }

    fn computer_use_app_icon(
        &self,
        bundle_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<std::sync::Arc<gpui::Image>> {
        if let Some(icon) = self.computer_use_app_icons.borrow().get(bundle_id) {
            return icon.clone();
        }

        let bundle_id = bundle_id.to_owned();
        if self
            .computer_use_app_icon_loads
            .borrow_mut()
            .insert(bundle_id.clone())
        {
            cx.spawn(async move |this, cx| {
                let load_bundle_id = bundle_id.clone();
                let icon =
                    cx.background_executor()
                        .spawn(async move {
                            crate::platform::load_app_icon_for_bundle_id(&load_bundle_id)
                        })
                        .await;
                let _ = this.update(cx, |this, cx| {
                    this.computer_use_app_icon_loads
                        .borrow_mut()
                        .remove(&bundle_id);
                    this.computer_use_app_icons
                        .borrow_mut()
                        .insert(bundle_id, icon);
                    cx.notify();
                });
            })
            .detach();
        }
        None
    }

    fn render_settings_drag_region(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let region = div().id(id);
        // Windows drags from the hit test rather than a mouse-move handler.
        #[cfg(target_os = "windows")]
        let region = region.window_control_area(gpui::WindowControlArea::Drag);

        region
            .h(px(48.0))
            .flex_none()
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    crate::platform::titlebar_double_click(window);
                }
            })
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                this.header_drag_armed = false;
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = false;
                }),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.header_drag_armed {
                    this.header_drag_armed = false;
                    crate::platform::start_window_move(window);
                }
            }))
    }

    fn update_theme_settings(
        &mut self,
        update: impl FnOnce(&mut ThemeSettings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.state.theme;
        update(&mut settings);
        if self.state.theme == settings {
            return;
        }
        self.state.theme = settings;
        crate::theme::apply_theme_preference(
            settings,
            self.state.sidebar_transparency,
            self.state.sidebar_transparency_amount,
            window,
            cx,
        );
        self.save();
        cx.notify();
    }

    /// Apply a would-be theme choice on screen without persisting it, so a
    /// hovered menu option shows itself. [`Self::restore_theme_preview`] puts
    /// the persisted theme back when the menu dismisses.
    fn preview_theme_settings(
        &mut self,
        update: impl FnOnce(&mut ThemeSettings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.state.theme;
        update(&mut settings);
        if self.state.theme == settings {
            return;
        }
        self.theme_preview_active = true;
        crate::theme::apply_theme_preference(
            settings,
            self.state.sidebar_transparency,
            self.state.sidebar_transparency_amount,
            window,
            cx,
        );
    }

    /// Re-apply the persisted theme after a previewed choice is dismissed
    /// without being picked. A pick commits through
    /// [`Self::update_theme_settings`] right after close, so this stays a
    /// no-op then — the persisted theme is the picked one.
    fn restore_theme_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.theme_preview_active {
            return;
        }
        self.theme_preview_active = false;
        crate::theme::apply_theme_preference(
            self.state.theme,
            self.state.sidebar_transparency,
            self.state.sidebar_transparency_amount,
            window,
            cx,
        );
    }

    /// The menu toggle observer that calls [`Self::restore_theme_preview`] on
    /// close. Pass to [`Self::menu_handle_with`] for selectors whose options
    /// preview through [`Self::preview_theme_settings`].
    fn theme_preview_restore(
        cx: &mut Context<Self>,
    ) -> impl Fn(bool, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |open, window, cx| {
            if !open {
                let _ = weak.update(cx, |this, cx| {
                    this.restore_theme_preview(window, cx);
                });
            }
        }
    }

    /// [`Self::theme_preview_restore`] plus an open-time claim on the selector's
    /// mode slot, so its menu and options are browsable while the other slot
    /// owns the window — light themes under a dark appearance and vice versa —
    /// even while Appearance is System. The claim is a preview: it never
    /// persists, and a pick only commits the palette, not the slot.
    fn theme_slot_preview(
        mode: ThemeMode,
        cx: &mut Context<Self>,
    ) -> impl Fn(bool, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |open, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                if open {
                    this.preview_theme_settings(|settings| settings.mode = mode, window, cx);
                } else {
                    this.restore_theme_preview(window, cx);
                }
            });
        }
    }

    pub(crate) fn sidebar_transparency(&self) -> bool {
        self.state.sidebar_transparency
    }

    fn set_thick_borders(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.thick_borders == enabled {
            return;
        }
        self.state.thick_borders = enabled;
        crate::theme::set_thick_borders(enabled);
        self.save();
        cx.notify();
    }

    /// The intensity slider commits once per gesture; each commit rebuilds
    /// the border tiers off the rescaled floors, like a contrast-mode flip.
    fn set_border_intensity(
        &mut self,
        intensity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let intensity = crate::persistence::sanitized_border_intensity(intensity);
        if self.state.border_intensity == intensity {
            return;
        }
        self.state.border_intensity = intensity;
        crate::theme::set_border_intensity(intensity);
        crate::theme::apply_theme_preference(
            self.state.theme,
            self.state.sidebar_transparency,
            self.state.sidebar_transparency_amount,
            window,
            cx,
        );
        self.save();
        cx.notify();
    }

    /// The "High contrast" preference widens the border-tier floors, so it
    /// rebuilds the theme rather than only flagging a static. The OS's own
    /// Increase Contrast setting forces it on — the toggle renders disabled
    /// in that case.
    fn set_high_contrast(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.high_contrast == enabled {
            return;
        }
        self.state.high_contrast = enabled;
        crate::theme::set_high_contrast(enabled);
        crate::theme::apply_theme_preference(
            self.state.theme,
            self.state.sidebar_transparency,
            self.state.sidebar_transparency_amount,
            window,
            cx,
        );
        self.save();
        cx.notify();
    }

    fn set_sidebar_transparency(
        &mut self,
        transparent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.sidebar_transparency == transparent {
            return;
        }
        if !transparent {
            self.sidebar_transparency_slider.cancel();
        }
        self.state.sidebar_transparency = transparent;
        crate::theme::apply_theme_preference(
            self.state.theme,
            transparent,
            self.state.sidebar_transparency_amount,
            window,
            cx,
        );
        self.save();
        cx.notify();
    }

    /// The transparency slider commits once per gesture; each commit retints
    /// the native vibrancy stack and persists like the toggle does.
    fn set_sidebar_transparency_amount(
        &mut self,
        amount: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let amount = crate::persistence::sanitized_sidebar_transparency_amount(amount);
        if self.state.sidebar_transparency_amount == amount {
            return;
        }
        self.state.sidebar_transparency_amount = amount;
        crate::theme::apply_theme_preference(
            self.state.theme,
            self.state.sidebar_transparency,
            amount,
            window,
            cx,
        );
        self.save();
        cx.notify();
    }

    fn set_three_finger_swipe_navigation(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.three_finger_swipe_navigation == enabled {
            return;
        }
        self.state.three_finger_swipe_navigation = enabled;
        crate::platform::set_trackpad_navigation_swipe_enabled(window, enabled);
        self.save();
        cx.notify();
    }

    fn set_language(
        &mut self,
        language: crate::i18n::AppLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.language == language {
            return;
        }

        self.state.language = language;
        crate::i18n::set_language(language);

        self.composer.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.composer"), cx);
            input.set_placeholder(tr!("input.do_anything"), cx)
        });
        self.model_picker.search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.search_models"), cx);
            input.set_placeholder(tr!("input.search_models"), cx)
        });
        self.route_class_picker.search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.search_models"), cx);
            input.set_placeholder(tr!("input.search_models"), cx)
        });
        self.branch_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.search_branches"), cx);
            input.set_placeholder(tr!("input.search_branches"), cx)
        });
        self.branch_create_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.new_branch_name"), cx);
            input.set_placeholder(tr!("input.new_branch_name"), cx)
        });
        self.worktree_name_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.worktree_name"), cx);
            input.set_placeholder(tr!("input.worktree_name"), cx)
        });
        self.settings_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("settings.search"), cx);
            input.set_placeholder(tr!("settings.search"), cx)
        });
        self.archived_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("settings.archived_search"), cx);
            input.set_placeholder(tr!("settings.archived_search"), cx)
        });
        for selector in [&self.ui_font_selector, &self.code_font_selector] {
            selector.search.update(cx, |input, cx| {
                input.set_accessibility_label(tr!("input.search_fonts"), cx);
                input.set_placeholder(tr!("input.search_fonts"), cx)
            });
        }
        self.skills_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("skills.search"), cx);
            input.set_placeholder(tr!("skills.search"), cx)
        });
        self.provider_path_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("providers.binary_path"), cx);
            input.set_placeholder(tr!("input.detected_automatically"), cx)
        });
        self.usage_project_filter.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.filter_projects"), cx);
            input.set_placeholder(tr!("input.filter_projects"), cx)
        });
        self.user_input_answer.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.answer"), cx);
            input.set_placeholder(tr!("user_input.other_placeholder"), cx)
        });
        self.annotation_comment_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.comment"), cx);
            input.set_placeholder(tr!("annotations.comment_placeholder"), cx)
        });
        self.session_rename_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.task_name"), cx)
        });
        self.daemon_port_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("daemon.port"), cx)
        });
        self.daemon_origins_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("daemon.allowed_origins"), cx)
        });
        self.right_panel_diff_filter.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("diff.filter_files"), cx);
            input.set_placeholder(tr!("diff.filter_files"), cx)
        });
        self.refresh_command_palette_localized_text(cx);
        self.refresh_file_finder_localized_text(cx);
        self.refresh_sync_branch_localized_text(cx);
        self.refresh_file_search_localized_text(cx);
        self.refresh_transcript_search_localized_text(cx);
        for browser in self.right_panel_browsers.values() {
            browser.update(cx, |browser, cx| browser.refresh_localized_text(cx));
        }
        for terminal in self.right_panel_terminals.values() {
            terminal.update(cx, |terminal, cx| terminal.refresh_localized_text(cx));
        }
        for probe in &mut self.probes {
            probe.models = crate::model_catalog::fallback_models(probe.provider);
        }
        self.refresh_provider_detection(None);
        self.invalidate_composer_sources(cx);

        let updater_available = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|updater| updater.0.as_ref())
            .is_some();
        crate::set_app_menus(cx, updater_available);
        self.save();
        window.refresh();
        cx.notify();
    }

    // ── Integrations ────────────────────────────────────────────────────
    // The pane renders the daemon's catalog joined with the settings mirror:
    // `integration_snapshots` is fetched once per page visit, and auth-state
    // flips arrive through the daemon's SettingsChanged broadcast.

    /// Fetch the catalog + configuration from the daemon. Runs off the UI
    /// thread; the answer lands through `integration_snapshots_events`.
    pub(super) fn load_integrations(&mut self) {
        let tx = self.integration_snapshots_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let daemon = self.daemon.client();
        std::thread::Builder::new()
            .name("waku-integrations-load".into())
            .spawn(move || {
                let result = match daemon.request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::ListIntegrations,
                ) {
                    Ok(waku_client::ResponsePayload::Integrations { snapshots }) => Ok(snapshots),
                    Ok(_) => Err("the daemon returned an invalid integrations response".into()),
                    Err(error) => Err(error.to_string()),
                };
                if tx.send(result).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .ok();
    }

    /// Send an integration command, then refresh the catalog snapshot so the
    /// pane re-renders with the daemon's truth. The result channel carries
    /// the refreshed list; a command failure sends the error instead.
    fn run_integration_command(&mut self, id: &str, command: waku_client::Command) {
        self.integration_commands_pending.insert(id.to_owned());
        let tx = self.integration_snapshots_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let daemon = self.daemon.client();
        std::thread::Builder::new()
            .name("waku-integration-command".into())
            .spawn(move || {
                let result = daemon
                    .request(Uuid::nil(), Uuid::nil(), command)
                    .map(|_| ())
                    .and_then(|_| {
                        match daemon.request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::ListIntegrations,
                        ) {
                            Ok(waku_client::ResponsePayload::Integrations { snapshots }) => {
                                Ok(snapshots)
                            }
                            Ok(_) => Err(anyhow::anyhow!(
                                "the daemon returned an invalid integrations response"
                            )),
                            Err(error) => Err(error),
                        }
                    })
                    .map_err(|error| error.to_string());
                if tx.send(result).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .ok();
    }

    /// Providers the picker may offer: installed, enabled, and able to carry
    /// MCP at all. Pi has no native MCP, so it is never a candidate.
    fn integration_providers(&self) -> Vec<ProviderKind> {
        ProviderKind::ALL
            .into_iter()
            .filter(|kind| {
                *kind != ProviderKind::Pi
                    && !self.state.disabled_providers.contains(kind)
                    && self
                        .provider_probe(*kind)
                        .is_some_and(|probe| probe.installed)
            })
            .collect()
    }

    /// The provider a new connection defaults to: the session the user is
    /// looking at when it is one of the eligible providers.
    fn default_integration_providers(&self) -> HashSet<ProviderKind> {
        let eligible = self.integration_providers();
        let current = self
            .selected_session()
            .map(|session| session.provider)
            .filter(|provider| eligible.contains(provider));
        current.into_iter().collect()
    }

    fn open_integration_editor(
        &mut self,
        snapshot: &waku_protocol::integrations::IntegrationSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let configured = self
            .state
            .integrations
            .iter()
            .find(|setting| setting.id == snapshot.info.id);
        let api_key = matches!(
            snapshot.info.auth,
            IntegrationAuthKind::ApiKey | IntegrationAuthKind::OauthOrApiKey
        )
        .then(|| {
            cx.new(|cx| {
                TextInput::new(window, cx)
                    .tab_index(0)
                    .accessibility_label(tr!("integrations.api_key"))
                    .placeholder(tr!("integrations.api_key_placeholder"))
            })
        });
        self.integration_editor = Some(IntegrationEditor {
            id: snapshot.info.id.clone(),
            variant_id: configured
                .map(|setting| setting.variant_id.clone())
                .unwrap_or_else(|| snapshot.info.variants[0].id.clone()),
            providers: configured
                .map(|setting| setting.providers.iter().copied().collect())
                .unwrap_or_else(|| self.default_integration_providers()),
            api_key,
        });
        cx.notify();
    }

    fn connect_integration(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.integration_editor.take() else {
            return;
        };
        let api_key = editor
            .api_key
            .as_ref()
            .map(|input| input.read(cx).content().trim().to_owned())
            .filter(|key| !key.is_empty());
        let id = editor.id.clone();
        self.run_integration_command(
            &id,
            waku_client::Command::ConnectIntegration {
                id: id.clone(),
                variant_id: editor.variant_id,
                providers: editor.providers.into_iter().collect(),
                api_key,
            },
        );
        cx.notify();
    }

    fn disconnect_integration(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.integration_editor.as_ref().is_some_and(|e| e.id == id) {
            self.integration_editor = None;
        }
        self.run_integration_command(
            id,
            waku_client::Command::DisconnectIntegration { id: id.to_owned() },
        );
        cx.notify();
    }

    /// Toggle one provider's assignment on an already-connected integration.
    fn toggle_integration_provider(
        &mut self,
        id: &str,
        provider: ProviderKind,
        cx: &mut Context<Self>,
    ) {
        let Some(setting) = self
            .state
            .integrations
            .iter()
            .find(|setting| setting.id == id)
        else {
            return;
        };
        let mut providers = setting.providers.clone();
        if providers.contains(&provider) {
            providers.retain(|existing| *existing != provider);
        } else {
            providers.push(provider);
        }
        self.run_integration_command(
            id,
            waku_client::Command::SetIntegrationProviders {
                id: id.to_owned(),
                providers,
            },
        );
        cx.notify();
    }

    fn toggle_editor_provider(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.integration_editor {
            if !editor.providers.insert(provider) {
                editor.providers.remove(&provider);
            }
        }
        cx.notify();
    }

    fn set_editor_variant(&mut self, variant_id: &str, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.integration_editor {
            editor.variant_id = variant_id.to_owned();
        }
        cx.notify();
    }

    fn retry_integration_auth(&mut self, id: &str, cx: &mut Context<Self>) {
        self.run_integration_command(
            id,
            waku_client::Command::StartIntegrationAuth { id: id.to_owned() },
        );
        cx.notify();
    }

    /// The Integrations experiment opt-in also decides whether its settings
    /// page appears in navigation.
    fn set_integrations_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled && self.settings_page == Some(SettingsPage::Integrations) {
            self.settings_page = None;
        }
        if !enabled {
            self.settings_navigation.remove(SettingsPage::Integrations);
        }
        self.state.integrations_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn render_integrations_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let providers = self.integration_providers();
        let mut cards = div().flex().flex_col().gap(px(10.0));
        match &self.integration_snapshots {
            None => {
                return div()
                    .mt(px(15.0))
                    .py(px(24.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("integrations.loading"))
                    .into_any_element();
            }
            Some(snapshots) => {
                for snapshot in snapshots {
                    cards =
                        cards.child(self.render_integration_card(snapshot, &providers, theme, cx));
                }
            }
        }
        div()
            .when(true, |element| {
                element.child(
                    div()
                        .mt(px(15.0))
                        .w_full()
                        .px(px(20.0))
                        .py(px(14.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .line_height(sp(18.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("integrations.description")),
                        ),
                )
            })
            .child(div().mt(px(15.0)).child(cards))
            .into_any_element()
    }

    fn render_integration_card(
        &self,
        snapshot: &waku_protocol::integrations::IntegrationSnapshot,
        providers: &[ProviderKind],
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = snapshot.info.id.clone();
        let configured = self
            .state
            .integrations
            .iter()
            .find(|setting| setting.id == id);
        let pending = self.integration_commands_pending.contains(&id);
        let editing = self
            .integration_editor
            .as_ref()
            .is_some_and(|editor| editor.id == id);

        // Right side: status + primary action. A connected integration offers
        // Disconnect; one waiting on OAuth offers Sign in; anything else
        // opens the connect form.
        let (status_label, status_color) = match configured.map(|setting| setting.auth) {
            Some(IntegrationAuthState::Connected) => (tr!("integrations.connected"), theme.success),
            Some(IntegrationAuthState::NeedsAuth) => {
                (tr!("integrations.sign_in_required"), theme.warning)
            }
            _ => (tr!("integrations.not_connected"), theme.text_ghost),
        };
        let action = if pending {
            div()
                .h(px(25.0))
                .px(px(9.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary)
                .child(tr!("common.checking"))
                .into_any_element()
        } else if configured.is_some_and(|s| s.auth == IntegrationAuthState::NeedsAuth) {
            integration_button(
                format!("integration-signin-{id}"),
                tr!("integrations.sign_in"),
                theme,
            )
            .on_activation(cx, {
                let id = id.clone();
                move |this, _, cx| {
                    this.retry_integration_auth(&id, cx);
                }
            })
            .into_any_element()
        } else if configured.is_some() {
            integration_button(
                format!("integration-disconnect-{id}"),
                tr!("integrations.disconnect"),
                theme,
            )
            .on_activation(cx, {
                let id = id.clone();
                let disconnect_name = snapshot.info.name.clone();
                move |_this, window, cx| {
                    let answer = window.prompt(
                        gpui::PromptLevel::Warning,
                        &tr!(
                            "integrations.confirm_disconnect",
                            name = disconnect_name.clone()
                        ),
                        Some(&tr!("integrations.confirm_disconnect_detail")),
                        &[
                            gpui::PromptButton::cancel(tr!("common.cancel")),
                            gpui::PromptButton::ok(tr!("integrations.disconnect")),
                        ],
                        cx,
                    );
                    let id = id.clone();
                    cx.spawn(async move |this, cx| {
                        if answer.await.ok() != Some(1) {
                            return;
                        }
                        let _ = this.update(cx, |this, cx| {
                            this.disconnect_integration(&id, cx);
                        });
                    })
                    .detach();
                }
            })
            .into_any_element()
        } else {
            integration_button(
                format!("integration-connect-{id}"),
                tr!("integrations.connect"),
                theme,
            )
            .on_activation(cx, {
                let snapshot = snapshot.clone();
                move |this, window, cx| {
                    this.open_integration_editor(&snapshot, window, cx);
                }
            })
            .into_any_element()
        };

        let mut body = div().flex().flex_col().gap(px(10.0));
        if let Some(setting) = configured {
            if editing {
                body = body.child(self.render_integration_editor(
                    snapshot,
                    Some(setting),
                    providers,
                    theme,
                    cx,
                ));
            } else {
                body = body.child(self.render_integration_providers(
                    &id,
                    &setting.providers,
                    providers,
                    theme,
                    cx,
                ));
            }
        } else if editing {
            body = body.child(self.render_integration_editor(snapshot, None, providers, theme, cx));
        }

        div()
            .w_full()
            .px(px(20.0))
            .py(px(14.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(integration_logo(&snapshot.info.id, theme))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .text_size(sp(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(snapshot.info.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(sp(11.5))
                                            .text_color(status_color)
                                            .child(status_label),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .truncate()
                                    .child(snapshot.info.summary.clone()),
                            ),
                    )
                    .child(action),
            )
            .child(body)
            .into_any_element()
    }

    /// Provider chips on a connected integration: click toggles assignment.
    fn render_integration_providers(
        &self,
        id: &str,
        selected: &[ProviderKind],
        providers: &[ProviderKind],
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.to_owned();
        let chips = providers.iter().map(|provider| {
            let provider = *provider;
            let on = selected.contains(&provider);
            integration_chip(
                format!("integration-{id}-provider-{}", provider.id()),
                provider.display_name().to_string(),
                on,
                theme,
            )
            .on_activation(cx, {
                let id = id.clone();
                move |this, _, cx| this.toggle_integration_provider(&id, provider, cx)
            })
            .into_any_element()
        });
        div()
            .pt(px(10.0))
            .border_t(hairline())
            .border_color(theme.separator)
            .flex()
            .items_center()
            .gap(px(8.0))
            .flex_wrap()
            .child(
                div()
                    .text_size(sp(11.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("integrations.agents")),
            )
            .children(chips)
            .into_any_element()
    }

    /// The connect form: variant chips when the service ships more than one,
    /// provider chips, an API-key field for services that take one, and
    /// Connect/Cancel.
    fn render_integration_editor(
        &self,
        snapshot: &waku_protocol::integrations::IntegrationSnapshot,
        _configured: Option<&waku_protocol::integrations::IntegrationSetting>,
        providers: &[ProviderKind],
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(editor) = &self.integration_editor else {
            return div().into_any_element();
        };
        let id = editor.id.clone();

        let mut form = div().flex().flex_col().gap(px(10.0));

        if snapshot.info.variants.len() > 1 {
            let chips = snapshot.info.variants.iter().map(|variant| {
                let variant_id = variant.id.clone();
                let on = editor.variant_id == variant.id;
                integration_chip(
                    format!("integration-{id}-variant-{}", variant.id),
                    variant.label.clone(),
                    on,
                    theme,
                )
                .on_activation(cx, move |this, _, cx| {
                    this.set_editor_variant(&variant_id, cx);
                })
                .into_any_element()
            });
            form = form.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .flex_wrap()
                    .children(chips),
            );
        }

        let provider_chips = providers.iter().map(|provider| {
            let provider = *provider;
            let on = editor.providers.contains(&provider);
            integration_chip(
                format!("integration-{id}-edit-provider-{}", provider.id()),
                provider.display_name().to_string(),
                on,
                theme,
            )
            .on_activation(cx, move |this, _, cx| {
                this.toggle_editor_provider(provider, cx);
            })
            .into_any_element()
        });
        form = form.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .flex_wrap()
                .child(
                    div()
                        .text_size(sp(11.5))
                        .text_color(theme.text_tertiary)
                        .child(tr!("integrations.agents")),
                )
                .children(provider_chips),
        );

        if let Some(api_key) = &editor.api_key {
            form = form.child(
                div()
                    .text_size(sp(11.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("integrations.api_key_hint")),
            );
            form = form.child(TextField::new("integration-api-key", api_key.clone()).w_full());
        }

        form = form.child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .child(
                    integration_button("integration-editor-cancel", tr!("common.cancel"), theme)
                        .on_activation(cx, |this, _, cx| {
                            this.integration_editor = None;
                            cx.notify();
                        }),
                )
                .child(
                    integration_button(
                        format!("integration-editor-connect-{id}"),
                        tr!("integrations.connect"),
                        theme,
                    )
                    .on_activation(cx, move |this, _, cx| {
                        this.connect_integration(cx);
                    }),
                ),
        );

        div()
            .pt(px(10.0))
            .border_t(hairline())
            .border_color(theme.separator)
            .child(form)
            .into_any_element()
    }
}

/// Sizes offered by the font-size dropdowns. A hand-edited `app.json` may
/// hold values outside this list; they render as-is and simply select
/// nothing here.
const FONT_SIZES: [f32; 8] = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0];

/// Next `FONT_SIZES` preset in the direction given — a between-presets value
/// lands on the first step past it, and the list ends hold their ground.
fn stepped_font_size(current: f32, direction: crate::FontSizeDirection) -> f32 {
    match direction {
        crate::FontSizeDirection::Increase => FONT_SIZES
            .into_iter()
            .find(|size| *size > current)
            .unwrap_or(current),
        crate::FontSizeDirection::Decrease => FONT_SIZES
            .into_iter()
            .rev()
            .find(|size| *size < current)
            .unwrap_or(current),
    }
}

fn font_size_label(size: f32) -> String {
    if size.fract() == 0.0 {
        format!("{size:.0} px")
    } else {
        format!("{size} px")
    }
}

const FONT_PICKER_ROW_HEIGHT: f32 = 30.0;
const FONT_PICKER_LIST_MAX_HEIGHT: f32 = 300.0;

/// Which configurable face a font picker edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FontTarget {
    Ui,
    Code,
}

impl FontTarget {
    fn menu_id(self) -> &'static str {
        match self {
            Self::Ui => "ui-font-selector",
            Self::Code => "code-font-selector",
        }
    }

    /// The family a `None` setting resolves to — also the row that resets
    /// the field to default when chosen.
    fn default_family(self) -> &'static str {
        match self {
            Self::Ui => crate::fonts::DEFAULT_UI_FAMILY,
            Self::Code => crate::fonts::DEFAULT_CODE_FAMILY,
        }
    }
}

/// One Appearance font picker's state: its filter field, the virtualized row
/// list, and the keyboard cursor. `rows` mirrors the names the list was last
/// built for so an unchanged frame never resets the scroll position;
/// `pending_reveal` asks the next rendered frame to park on the selected row.
pub(super) struct FontSelector {
    pub search: Entity<TextInput>,
    pub list: ListState,
    pub scrollbar: Rc<ScrollbarState>,
    pub highlight: Option<usize>,
    pub pending_reveal: Cell<bool>,
    pub rows: RefCell<Vec<SharedString>>,
}

impl FontSelector {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            search: cx.new(|cx| {
                TextInput::new(window, cx)
                    .clear_on_escape()
                    .accessibility_label(tr!("input.search_fonts"))
                    .placeholder(tr!("input.search_fonts"))
            }),
            list: ListState::new(0, ListAlignment::Top, px(64.0)),
            scrollbar: ScrollbarState::new(),
            highlight: None,
            pending_reveal: Cell::new(false),
            rows: RefCell::new(Vec::new()),
        }
    }
}

/// What a family row shows: the platform alias reads better as "System
/// font" than ".SystemUIFont".
fn font_row_label(family: &SharedString) -> SharedString {
    if family.as_ref() == crate::fonts::DEFAULT_UI_FAMILY {
        SharedString::from(tr!("settings.system_font"))
    } else {
        family.clone()
    }
}

/// A settings card row: icon, title, and description on the left, control
/// on the right — the card's divider lines are drawn by the caller.
#[track_caller]
fn settings_row(
    icon_path: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    theme: Theme,
    search: &SettingSearch,
) -> Option<AnyElement> {
    let title = title.into();
    let description = description.into();
    let matched = search.matched(&title, &description)?;
    Some(
        div()
            .w_full()
            .min_h(px(60.0))
            .px(px(20.0))
            .py(px(12.0))
            .flex()
            .items_center()
            .gap(px(24.0))
            .child(settings_row_label(
                icon_path,
                settings_row_text(title, description, matched, theme),
                theme,
            ))
            .child(control)
            .into_any_element(),
    )
}

/// Display label for a decision-log feature name on the Jev usage card.
/// Unknown tags render raw — a new call site labels itself on the next line
/// it logs, and a missing entry here degrades rather than hides.
fn eval_feature_label(feature: &str) -> String {
    match feature {
        "evaluate" => tr!("routing.feature_evaluate"),
        "turn-status" => tr!("routing.feature_turn_status"),
        "title-quality" => tr!("routing.feature_title_quality"),
        "route" => tr!("routing.feature_route"),
        "route-effort" => tr!("routing.feature_route_effort"),
        "route-phase" => tr!("routing.feature_route_phase"),
        "route-class-suggest" => tr!("routing.feature_route_class_suggest"),
        "next-action" => tr!("experiments.action_predictions_title"),
        "paste-classification" => tr!("routing.feature_paste_classification"),
        "provider-switch" => tr!("routing.feature_provider_switch"),
        "memory-rank" => tr!("routing.feature_memory_rank"),
        "memory-triage" => tr!("routing.feature_memory_triage"),
        "permission-review" => tr!("routing.feature_permission_review"),
        "auto-prompt" => tr!("auto_prompts.title"),
        "auto-prompt-suggest" => tr!("auto_prompts.suggest"),
        "auto-prompt-preview" => tr!("auto_prompts.try_task"),
        _ => return feature.to_owned(),
    }
}

/// What one usage-card spend category judges, in a line — rendered under
/// the feature's label ahead of its call count. `None` for unknown tags:
/// the row keeps the bare call count rather than inventing a meaning.
fn eval_feature_description(feature: &str) -> Option<String> {
    Some(match feature {
        "evaluate" => tr!("routing.feature_evaluate_description"),
        "turn-status" => tr!("routing.feature_turn_status_description"),
        "title-quality" => tr!("routing.feature_title_quality_description"),
        "route" => tr!("routing.feature_route_description"),
        "route-effort" => tr!("routing.feature_route_effort_description"),
        "route-phase" => tr!("routing.feature_route_phase_description"),
        "route-class-suggest" => tr!("routing.feature_route_class_suggest_description"),
        "next-action" => tr!("routing.feature_next_action_description"),
        "paste-classification" => tr!("routing.feature_paste_classification_description"),
        "provider-switch" => tr!("routing.feature_provider_switch_description"),
        "memory-rank" => tr!("routing.feature_memory_rank_description"),
        "memory-triage" => tr!("routing.feature_memory_triage_description"),
        "permission-review" => tr!("routing.feature_permission_review_description"),
        "auto-prompt" => tr!("routing.feature_auto_prompt_description"),
        "auto-prompt-suggest" => tr!("routing.feature_auto_prompt_suggest_description"),
        "auto-prompt-preview" => tr!("routing.feature_auto_prompt_preview_description"),
        _ => return None,
    })
}

/// One feature row in the Jev usage card — `settings_row`'s shell with the
/// call count stacked on its own line under the category's description.
/// Unknown tags get no description and fall back to the plain one-line row.
#[track_caller]
fn eval_feature_row(
    feature: &str,
    totals: &waku_protocol::eval::EvalUsageTotals,
    theme: Theme,
    search: &SettingSearch,
) -> Option<AnyElement> {
    let title = eval_feature_label(feature);
    let calls = tr!("routing.usage_feature_calls", calls = totals.calls);
    let Some(detail) = eval_feature_description(feature) else {
        return settings_row(
            "icons/chart-column.svg",
            title,
            calls,
            eval_usage_label(totals, theme),
            theme,
            search,
        );
    };
    let matched = search.matched(&title, &detail)?;
    Some(
        div()
            .w_full()
            .min_h(px(60.0))
            .px(px(20.0))
            .py(px(12.0))
            .flex()
            .items_center()
            .gap(px(24.0))
            .child(settings_row_label(
                "icons/chart-column.svg",
                settings_row_text(title, detail, matched, theme).child(
                    div()
                        .mt(px(2.0))
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(calls),
                ),
                theme,
            ))
            .child(eval_usage_label(totals, theme))
            .into_any_element(),
    )
}

/// The right-side readout on a Jev usage row: compact "in · out" token
/// counts in the same compact format the Usage page's columns use.
fn eval_usage_label(totals: &waku_protocol::eval::EvalUsageTotals, theme: Theme) -> Div {
    div()
        .text_size(sp(12.5))
        .text_color(theme.text_secondary)
        .child(tr!(
            "routing.usage_tokens",
            input = format_tokens_compact(totals.input_tokens as f64),
            output = format_tokens_compact(totals.output_tokens as f64)
        ))
}

/// Display label for an eval backend in the routing section's selector.
fn eval_backend_label(backend: waku_protocol::eval::EvalBackend) -> &'static str {
    match backend {
        waku_protocol::eval::EvalBackend::TypeSafe => "TypeSafe",
        waku_protocol::eval::EvalBackend::VercelGateway => "Vercel AI Gateway",
        waku_protocol::eval::EvalBackend::Cloudflare => "Cloudflare Workers AI",
    }
}

fn auto_prompt_question_editor(
    window: &mut Window,
    cx: &mut Context<Waku>,
    existing: Option<&AutoPromptQuestion>,
) -> AutoPromptQuestionEditor {
    let instructions = cx.new(|cx| {
        TextInput::new(window, cx)
            .multi_line()
            .auto_height()
            .max_lines(4)
            .tab_index(0)
            .accessibility_label(tr!("auto_prompts.question_label"))
            .placeholder(tr!("auto_prompts.question_placeholder"))
    });
    let weight = cx.new(|cx| {
        TextInput::new(window, cx)
            .tab_index(0)
            .accessibility_label(tr!("auto_prompts.weight"))
            .placeholder("1")
    });
    if let Some(question) = existing {
        instructions.update(cx, |input, cx| {
            input.set_content(question.instructions.clone(), cx)
        });
        if let Some(value) = question.weight {
            weight.update(cx, |input, cx| input.set_content(format!("{value}"), cx));
        }
    }
    AutoPromptQuestionEditor {
        id: existing
            .map(|question| question.id)
            .unwrap_or_else(Uuid::new_v4),
        instructions,
        weight,
    }
}

fn confident_auto_prompt_suggestion(
    answer: Option<&waku_protocol::eval::EvalAnswer>,
) -> Option<f64> {
    let waku_protocol::eval::EvalAnswer::Choice {
        choice,
        confidence: Some(confidence),
        probabilities,
    } = answer?
    else {
        return None;
    };
    if !confidence.is_finite() || *confidence < 0.5 {
        return None;
    }
    let selected = *probabilities.get(choice)?;
    if !selected.is_finite() {
        return None;
    }
    let highest = probabilities
        .values()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let runner_up = probabilities
        .iter()
        .filter(|(candidate, _)| candidate.as_str() != choice)
        .map(|(_, probability)| *probability)
        .fold(0.0, f64::max);
    if selected < highest || selected - runner_up < 0.15 {
        return None;
    }
    choice.parse::<f64>().ok()
}

/// How many recent combos Jev sees when suggesting class defaults.
const ROUTE_SUGGEST_COMBOS: usize = 10;

/// A friendly label for a class's mapped target, resolving provider and
/// model ids against the probe catalog — the effort's label trailing the
/// name when one is pinned. `None` is the unmapped route: no override, the
/// class keeps whatever provider/model was last used.
fn route_class_target_label(target: Option<&RouteClassTarget>, probes: &[ProviderProbe]) -> String {
    let Some(target) = target else {
        return tr!("routing.no_override");
    };
    match &target.model {
        Some(model_id) => {
            let model = probes
                .iter()
                .find(|probe| probe.provider == target.provider)
                .and_then(|probe| probe.model(model_id));
            let name = model
                .map(|model| {
                    model
                        .name_i18n
                        .as_ref()
                        .map(waku_client::WireTranslation::render)
                        .unwrap_or_else(|| model.name.clone())
                })
                .unwrap_or_else(|| model_id.clone());
            let effort_label = target.effort.as_deref().and_then(|effort| {
                model
                    .and_then(|model| {
                        model
                            .reasoning_efforts
                            .iter()
                            .find(|option| option.id == effort)
                    })
                    .map(|option| {
                        option
                            .label_i18n
                            .as_ref()
                            .map(waku_client::WireTranslation::render)
                            .unwrap_or_else(|| option.label.clone())
                    })
            });
            match effort_label {
                Some(effort) => format!("{name} {effort} · {}", target.provider.short_name()),
                None => format!("{name} · {}", target.provider.short_name()),
            }
        }
        None => format!(
            "{} ({})",
            target.provider.short_name(),
            tr!("routing.provider_default")
        ),
    }
}

/// Whether a picker row describes the class's current mapping: the leading
/// stance for an unmapped class, the provider-default row for a bare
/// provider target, or the combo naming the mapped model's exact effort.
/// A target stored before rows carried effort lands on its model's
/// default-effort row — that is the rung it resolves to.
fn route_class_row_matches(row: &PickerRow, target: Option<&RouteClassTarget>) -> bool {
    match (row, target) {
        (PickerRow::Policy(PolicyRowId::NoOverride), None) => true,
        (PickerRow::ProviderDefault(provider), Some(target)) => {
            *provider == target.provider && target.model.is_none()
        }
        (PickerRow::Combo(row), Some(target)) => {
            if row.provider != target.provider
                || target.model.as_deref() != Some(row.model.id.as_str())
            {
                return false;
            }
            match target.effort.as_deref() {
                Some(effort) => Some(effort) == row.effort.as_deref(),
                None => row.effort == model_default_effort(&row.model),
            }
        }
        _ => false,
    }
}

/// The display bucket the "Checked …" caption renders for an elapsed time:
/// 0 is "just now", then the minute count, then the hour count offset so the
/// two ranges never collide. A bucket change is the only repaint the caption
/// needs — the maintenance tick watches it.
pub(super) fn detection_checked_bucket(elapsed: Duration) -> u64 {
    let seconds = elapsed.as_secs();
    if seconds < 90 {
        0
    } else if seconds < 3600 {
        seconds / 60
    } else {
        seconds / 3600 + 1000
    }
}

/// "Checked …" caption for the Providers page. Recomputed whenever the page
/// redraws; precision beyond the minute is noise here.
fn detection_checked_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 90 {
        tr!("providers.checked_just_now")
    } else if seconds < 3600 {
        tr!("providers.checked_minutes_ago", count = seconds / 60)
    } else {
        tr!("providers.checked_hours_ago", count = seconds / 3600)
    }
}

/// Split the whitelist field's comma- or whitespace-separated text into
/// branch names — Git names admit neither separator — deduplicated in
/// first-seen order.
fn sync_branches_from_text(text: &str) -> Vec<String> {
    let mut branches = Vec::new();
    for name in text.split([',', ' ', '\t', '\n']) {
        let name = name.trim();
        if !name.is_empty() && !branches.iter().any(|seen| seen == name) {
            branches.push(name.to_owned());
        }
    }
    branches
}

/// Keep the full binary path, abbreviating only the user's home directory.
pub(super) fn abbreviate_home_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// The mobile connect code — black modules on a white card regardless of
/// theme, since a scanner needs the classic contrast. Painted as quads so
/// it stays sharp at any scale rather than rasterizing to an image.
#[track_caller]
fn daemon_qr_view(code: std::sync::Arc<DaemonQrCode>) -> Div {
    const QUIET_ZONE: f32 = 10.0;
    div()
        .p(px(QUIET_ZONE))
        .rounded(px(8.0))
        .bg(gpui::white())
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let module = f32::from(bounds.size.width) / code.width as f32;
                    // A hair of overlap keeps rasterization from leaving
                    // bright seams between adjacent dark modules.
                    let side = px(module + 0.5);
                    for y in 0..code.width {
                        for x in 0..code.width {
                            if !code.dark[y * code.width + x] {
                                continue;
                            }
                            window.paint_quad(fill(
                                gpui::Bounds::new(
                                    point(
                                        bounds.origin.x + px(module * x as f32),
                                        bounds.origin.y + px(module * y as f32),
                                    ),
                                    gpui::size(side, side),
                                ),
                                gpui::black(),
                            ));
                        }
                    }
                },
            )
            .size(px(120.0)),
        )
}

/// The bordered action button the settings cards share — the Apply/Test/
/// Suggest/Recheck/Revoke family. `enabled: false` dims the button and drops
/// its activation but keeps the tab stop, so a pending operation cannot move
/// focus. `danger` tints the label on hover for destructive actions;
/// `compact` shrinks to `integration_button`'s footprint for buttons living
/// inside list rows.
#[track_caller]
pub(super) fn settings_button<E>(
    id: impl Into<ElementId>,
    label: String,
    enabled: bool,
    danger: bool,
    compact: bool,
    theme: Theme,
    cx: &mut Context<E>,
    activate: impl Fn(&mut E, &mut Window, &mut Context<E>) + 'static,
) -> Stateful<Div>
where
    E: 'static,
{
    let button = div()
        .id(id)
        .tab_index(0)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .h(if compact { px(25.0) } else { px(29.0) })
        .px(if compact { px(9.0) } else { px(11.0) })
        .rounded(if compact { px(8.0) } else { px(9.0) })
        .border(hairline())
        .border_color(theme.border_strong)
        .flex()
        .items_center()
        .justify_center()
        .cursor_default()
        .text_size(sp(12.5))
        .text_color(theme.text_secondary)
        .when(!enabled, |element| element.opacity(0.55))
        .when(enabled, |element| {
            element.hover(move |style| {
                let style = style.bg(theme.overlay);
                if danger {
                    style.text_color(theme.danger)
                } else {
                    style
                }
            })
        })
        .child(label);
    if enabled {
        button.on_activation(cx, activate)
    } else {
        button
    }
}

/// Small bordered action button for the Integrations cards.
#[track_caller]
fn integration_button(id: impl Into<ElementId>, label: String, theme: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .tab_index(0)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .h(px(25.0))
        .px(px(9.0))
        .rounded(px(8.0))
        .border(hairline())
        .border_color(theme.border_strong)
        .flex()
        .items_center()
        .cursor_default()
        .text_size(sp(12.5))
        .text_color(theme.text_secondary)
        .hover(|element| element.bg(theme.overlay).text_color(theme.text))
        .child(label)
}

/// One selectable chip — a provider or variant choice on the Integrations
/// cards. `on` fills it with the accent tint.
#[track_caller]
fn integration_chip(
    id: impl Into<ElementId>,
    label: String,
    on: bool,
    theme: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .tab_index(0)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .h(px(22.0))
        .px(px(8.0))
        .rounded_full()
        .border(hairline())
        .border_color(if on {
            theme.accent
        } else {
            theme.border_strong
        })
        .flex()
        .items_center()
        .cursor_default()
        .text_size(sp(11.5))
        .text_color(if on {
            theme.accent
        } else {
            theme.text_secondary
        })
        .when(on, |element| element.bg(theme.accent.opacity(0.12)))
        .hover(|element| element.bg(theme.overlay))
        .child(label)
}

/// A service's square brand mark as a tinted `svg()` alpha mask. Each takes
/// one color: the brand's own where it reads on either theme, `theme.text`
/// where the authored black would vanish on the dark card, and a lighter
/// brand-family color where the official one is too dark (Sentry, Atlassian).
#[track_caller]
fn integration_logo(id: &str, theme: Theme) -> Svg {
    let (path, color): (&'static str, Hsla) = match id {
        "atlassian" => ("icons/integration-atlassian.svg", rgb(0x2684FF).into()),
        "figma" => ("icons/integration-figma.svg", rgb(0xF24E1E).into()),
        "github" => ("icons/integration-github.svg", theme.text),
        "linear" => ("icons/integration-linear.svg", rgb(0x5E6AD2).into()),
        "monday" => ("icons/integration-monday.svg", rgb(0xFF3D57).into()),
        "notion" => ("icons/integration-notion.svg", theme.text),
        "sentry" => ("icons/integration-sentry.svg", rgb(0x6C5FC7).into()),
        "stripe" => ("icons/integration-stripe.svg", rgb(0x635BFF).into()),
        "supabase" => ("icons/integration-supabase.svg", rgb(0x3ECF8E).into()),
        "vercel" => ("icons/integration-vercel.svg", theme.text),
        _ => ("icons/server.svg", theme.text_tertiary),
    };
    icon(path, 15.0, color)
}

#[track_caller]
fn permission_status_row(
    icon_path: &'static str,
    name: String,
    description: String,
    granted: bool,
    id: &'static str,
    theme: Theme,
    search: &SettingSearch,
    cx: &mut Context<Waku>,
) -> Option<Div> {
    let matched = search.matched(&name, &description)?;
    let status = if granted {
        div()
            .id(id)
            .h(px(25.0))
            .px(px(4.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.success)
            .child(icon("icons/check.svg", 12.0, theme.success))
            .child(tr!("computer_use.access_granted"))
    } else {
        div()
            .id(id)
            .tab_index(0)
            .h(px(25.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay).text_color(theme.text))
            .focus_visible(|element| element.border_color(theme.accent))
            .child(tr!("computer_use.grant_access"))
            .on_activation(cx, move |this, _, cx| {
                this.request_computer_permissions(true, cx);
            })
    };

    Some(
        div()
            .mt(px(10.0))
            .pt(px(10.0))
            .border_t(hairline())
            .border_color(theme.separator)
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(settings_row_icon(icon_path, theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(settings_title_jump(
                        div()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(settings_search_text(
                                name,
                                matched.title_ranges.clone(),
                                theme,
                            )),
                        &matched,
                        theme,
                    ))
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(settings_search_text(
                                description,
                                matched.description_ranges.clone(),
                                theme,
                            )),
                    ),
            )
            .child(status),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        FocusNext, FocusPrevious, SETTINGS_CONTEXT, SETTINGS_PAGES, abbreviate_home_path,
        sync_branches_from_text,
    };
    use crate::input::TextInput;
    use gpui::{Context, Entity, Render, TestAppContext, Window, div, prelude::*};
    use std::path::Path;

    struct TabHarness {
        name: Entity<TextInput>,
        shell: Entity<TextInput>,
        script: Entity<TextInput>,
    }

    impl Render for TabHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context(SETTINGS_CONTEXT)
                .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
                .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
                .child(self.name.clone())
                .child(div().id("icon-selector").tab_index(0))
                .child(self.shell.clone())
                .child(self.script.clone())
                .child(div().id("toggle").tab_index(0))
                .child(div().id("cancel").tab_index(0))
                .child(div().id("save").tab_index(0))
        }
    }

    #[gpui::test]
    fn tab_moves_through_settings_form_controls(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::input::init(cx);
            super::init(cx);
        });
        let (harness, cx) = cx.add_window_view(|window, cx| {
            let name = cx.new(|cx| TextInput::new(window, cx).tab_index(0));
            let shell = cx.new(|cx| TextInput::new(window, cx).tab_index(0));
            let script = cx.new(|cx| TextInput::new(window, cx).tab_index(0));
            TabHarness {
                name,
                shell,
                script,
            }
        });
        let name = cx.read_entity(&harness, |harness, _| harness.name.clone());
        let shell = cx.read_entity(&harness, |harness, _| harness.shell.clone());
        let script = cx.read_entity(&harness, |harness, _| harness.script.clone());
        let name_focus = cx.read_entity(&name, |input, _| input.focus());
        let shell_focus = cx.read_entity(&shell, |input, _| input.focus());
        let script_focus = cx.read_entity(&script, |input, _| input.focus());
        cx.update(|window, cx| window.focus(&name_focus, cx));

        let mut forward = Vec::new();
        for _ in 0..7 {
            cx.simulate_keystrokes("tab");
            cx.update(|window, cx| {
                forward.push(window.focused(cx).expect("a tab stop should be focused"));
            });
        }

        assert_eq!(forward[1], shell_focus);
        assert_eq!(forward[2], script_focus);
        assert_eq!(forward[6], name_focus);
        for (index, focus) in forward[..6].iter().enumerate() {
            assert!(!forward[..index].contains(focus));
        }

        for expected in forward[..6].iter().rev() {
            cx.simulate_keystrokes("shift-tab");
            cx.update(|window, cx| {
                assert_eq!(window.focused(cx).as_ref(), Some(expected));
            });
        }
    }

    #[test]
    fn every_settings_page_icon_is_embedded() {
        use crate::assets::Assets;
        use gpui::AssetSource;

        for (.., icon, _) in SETTINGS_PAGES {
            assert!(
                Assets.load(icon).unwrap().is_some(),
                "missing embedded icon: {icon}"
            );
        }
    }

    #[test]
    fn sync_branches_parse_separators_and_deduplicate() {
        assert_eq!(
            sync_branches_from_text("develop, release/1.2\nnext develop"),
            ["develop", "release/1.2", "next"]
        );
        assert!(sync_branches_from_text(" , ").is_empty());
    }

    #[test]
    fn provider_paths_abbreviate_only_the_home_prefix() {
        let home = Path::new("/Users/example");

        assert_eq!(
            abbreviate_home_path(Path::new("/Users/example/.local/bin/amp"), Some(home)),
            "~/.local/bin/amp"
        );
        assert_eq!(
            abbreviate_home_path(Path::new("/opt/homebrew/bin/codex"), Some(home)),
            "/opt/homebrew/bin/codex"
        );
    }

    #[test]
    fn settings_match_ranges_finds_every_case_insensitive_hit() {
        assert_eq!(
            super::settings_match_ranges("Font size and FONT family", "font"),
            vec![0..4, 14..18]
        );
        assert!(super::settings_match_ranges("no hit here", "font").is_empty());
        assert!(super::settings_match_ranges("anything", "").is_empty());
        // A query that occurs past the end of a shorter text reports nothing.
        assert!(super::settings_match_ranges("ab", "abc").is_empty());
    }

    #[test]
    fn setting_search_keeps_title_or_description_hits_only() {
        let search = super::SettingSearch::new("font");
        assert!(search.matched("UI Font", "ignored").is_some());
        // A description-only match still keeps the row.
        assert!(search.matched("ignored", "the font used").is_some());
        assert!(search.matched("ignored", "also ignored").is_none());
        assert_eq!(search.hits(), 2);
    }

    #[test]
    fn setting_search_forced_keeps_every_row() {
        let search = super::SettingSearch::forced("appearance");
        assert!(search.matched("Theme", "ignored").is_some());
        // Rows with no hit stay too — the match was on the section title.
        assert!(search.matched("ignored", "also ignored").is_some());
        assert_eq!(search.hits(), 2);
    }

    #[test]
    fn setting_search_inactive_matches_everything_without_counting() {
        let search = super::SettingSearch::inactive();
        let matched = search
            .matched("anything", "goes")
            .expect("inactive search keeps rows");
        assert!(matched.title_ranges.is_empty() && matched.description_ranges.is_empty());
        assert_eq!(search.hits(), 0);
    }
}
