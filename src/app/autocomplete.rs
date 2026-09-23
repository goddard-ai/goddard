//! The composer's autocompletion popup: slash commands, `@` file mentions,
//! and `#` GitHub issue/pull-request references.
//!
//! The popup is a pure view over prefetched data. Command and file indexes are
//! discovered on the background executor into `QueryCache`s and mirrored into
//! plain fields the frame reads; the filter over them is memoized per
//! keystroke, so a caret blink re-renders the popup without re-fuzzy-matching
//! the workspace.
//!
//! Keys follow the model picker's split: the composer keeps real focus the
//! whole time and the popup's selection is only drawn. While the popup is
//! open the composer card declares the `ComposerAutocomplete` key context, and
//! `up`/`down`/`enter`/`tab`/`escape` reach it as actions that outrank the
//! field's own bindings; when it closes the context disappears and `enter`
//! submits again.

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use gpui::{
    Anchor, App, Bounds, Font, KeyBinding, Pixels, StyledText, TextRun, anchored, deferred,
};
use nucleo_matcher::Matcher;

use waku_client::{GitHubAvailability, GitHubRepoRef, WorkItemKind, WorkItemQueryState};

use crate::composer_complete::{
    self, ComposerWorkItem, ComposerWorkItemState, FILE_INDEX_CAP, FileEntry, Scored, SlashCommand,
    Trigger, TriggerKind, highlight_byte_ranges,
};
use crate::ui::menu::{ConfirmEntry, DismissMenu, SelectNextEntry, SelectPreviousEntry};

use super::model_picker::next_picker_highlight;
use super::*;

/// Key context the composer card declares while the popup is open.
const AUTOCOMPLETE_CONTEXT: &str = "ComposerAutocomplete > TextInput";
const AUTOCOMPLETE_LOADING_CONTEXT: &str = "ComposerAutocompleteLoading > TextInput";

/// Bind the popup's keys. Must run after [`crate::input::init`]: `enter` and
/// the arrows tie with the field's own bindings at the `ComposerInput` depth,
/// and the tie goes to whichever was registered last.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNextEntry, Some(AUTOCOMPLETE_CONTEXT)),
        KeyBinding::new("up", SelectPreviousEntry, Some(AUTOCOMPLETE_CONTEXT)),
        // The field binds the emacs spelling of the arrows too; while the
        // popup owns them they must move the highlight, not the caret.
        KeyBinding::new("ctrl-n", SelectNextEntry, Some(AUTOCOMPLETE_CONTEXT)),
        KeyBinding::new("ctrl-p", SelectPreviousEntry, Some(AUTOCOMPLETE_CONTEXT)),
        KeyBinding::new("enter", ConfirmEntry, Some(AUTOCOMPLETE_CONTEXT)),
        KeyBinding::new("tab", ConfirmEntry, Some(AUTOCOMPLETE_CONTEXT)),
        KeyBinding::new("escape", DismissMenu, Some(AUTOCOMPLETE_CONTEXT)),
        KeyBinding::new("escape", DismissMenu, Some(AUTOCOMPLETE_LOADING_CONTEXT)),
    ]);
}

pub(super) enum AutocompleteRow {
    Command(Scored<SlashCommand>),
    /// A task mention under `@` — accepting splices an inline session atom.
    Session(Scored<ComposerSessionRef>),
    File(Scored<FileEntry>),
    WorkItem(Scored<ComposerWorkItem>),
}

/// What a session `@` row carries: the id the atom records, the title the
/// popup matches and the mention paints, and the project for disambiguation.
#[derive(Clone)]
pub(super) struct ComposerSessionRef {
    pub id: Uuid,
    pub title: SharedString,
    pub project: SharedString,
}

/// Session rows cap under `@` — they lead the list so a title match is
/// never buried under the file index, but a broad query still leaves files
/// reachable.
const SESSION_MENTION_CAP: usize = 8;

/// The `mentionable_sessions` pool minus the staged/target exclusions:
/// started, unarchived, not a side chat, and in the composer's own project
/// when one is known — a cross-project reference is a sidebar drag away,
/// not worth the autocomplete noise. Shared with the fingerprint so both
/// watch the same pool.
pub(super) fn session_mention_candidate(session: &AgentSession, project: Option<Uuid>) -> bool {
    session.has_started()
        && session.archived_at.is_none()
        && !session.is_side_chat()
        && project.is_none_or(|project| session.project_id == project)
}

/// Keystroke pause before a `#` query goes to the daemon — each search is two
/// `gh` subprocesses, so typing waits for a settle the way the transcript
/// search does.
const WORK_ITEM_SEARCH_DEBOUNCE: Duration = Duration::from_millis(220);

/// One workspace's `#` mention state: the resolved repo, the newest landed
/// search, and every item ever seen (the `known` map feeds both the
/// provider-prompt expansion and the transcript's `#N` chips, which must keep
/// working after the query that found them is gone).
#[derive(Default)]
pub(super) struct WorkItemMentions {
    /// `None` until `ResolveGitHubRepo` answers; `Some((None, availability))`
    /// explains why `#` cannot be answered here.
    pub repo: Option<(Option<GitHubRepoRef>, GitHubAvailability)>,
    /// Merged issue/PR rows for `items_query`.
    pub items: Rc<Vec<ComposerWorkItem>>,
    pub items_query: String,
    /// Number → item for every landed row.
    pub known: HashMap<u64, ComposerWorkItem>,
    /// A remote search is in flight.
    pub loading: bool,
    /// The query the in-flight (or last) fetch was started for.
    pub requested_query: Option<String>,
    /// Bumped per fetch; replies from older generations drop.
    pub generation: u64,
}

/// Filter results for one (kind, query, source index) — the popup's rows are
/// recomputed on a keystroke, not on every frame the caret blinks.
struct ResultsMemo {
    kind: TriggerKind,
    query: String,
    /// `Rc::as_ptr` identity of the source index the rows were filtered from.
    source: usize,
    /// Whether `/incognito` was offered — the draft-only command is filtered
    /// out once the composer session has started, so it keys the memo too.
    incognito_offered: bool,
    rows: Rc<Vec<AutocompleteRow>>,
}

/// Cross-frame state for the popup. All interior-mutable: the render path
/// reconciles it from `&self`, the same way the transcript anchors do.
pub(super) struct AutocompleteUi {
    /// The trigger as of the last frame, for detecting query/site changes.
    token: RefCell<Option<Trigger>>,
    /// Keyboard cursor over the filtered rows: the popup opens with the first
    /// row selected, and every token change snaps back to it so the best
    /// match is always the one `enter` takes. Clamped to the list at use.
    highlight: Cell<usize>,
    /// Escape pressed on the current token; cleared the moment it changes.
    dismissed: Cell<bool>,
    scroll: ScrollHandle,
    /// The composer card's bounds as of the last frame, recorded by a probe,
    /// so the popup can anchor above the card at the card's own width.
    card_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    results: RefCell<Option<ResultsMemo>>,
    matcher: RefCell<Matcher>,
    /// The `#` query waiting out the debounce, and the generation of the
    /// newest scheduled fetch — a stale timer must not fire it.
    work_item_scheduled: RefCell<Option<(PathBuf, String)>>,
    work_item_debounce: Cell<u64>,
}

impl AutocompleteUi {
    pub(super) fn new() -> Self {
        Self {
            token: RefCell::new(None),
            highlight: Cell::new(0),
            dismissed: Cell::new(false),
            scroll: ScrollHandle::new(),
            card_bounds: Rc::new(Cell::new(None)),
            results: RefCell::new(None),
            matcher: RefCell::new(composer_complete::matcher()),
            work_item_scheduled: RefCell::new(None),
            work_item_debounce: Cell::new(0),
        }
    }

    /// The cell the composer card's bounds probe writes into.
    pub(super) fn card_bounds_cell(&self) -> Rc<Cell<Option<Bounds<Pixels>>>> {
        self.card_bounds.clone()
    }
}

impl Waku {
    /// Refresh the drawn command and file indexes for the selected session.
    ///
    /// A cache hit lands immediately; a miss starts discovery on the
    /// background executor and re-runs this when it arrives. Nothing here may
    /// touch the filesystem directly.
    pub(super) fn refresh_composer_sources(&mut self, cx: &mut Context<Self>) {
        let Some(project_path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            self.slash_command_index = Rc::new(Vec::new());
            self.slash_command_index_key = None;
            self.slash_command_index_loading = false;
            self.mention_file_index = Rc::new(Vec::new());
            self.mention_file_index_path = None;
            self.mention_file_index_loading = false;
            return;
        };
        let provider = self
            .selected_session()
            .map(|session| session.provider)
            .unwrap_or(self.state.last_provider);
        let reported = self
            .selected_session()
            .map(|session| session.available_commands.clone())
            .unwrap_or_default();
        let binary_override = self.state.provider_binary_overrides.get(&provider).cloned();

        let command_key = (provider, project_path.clone(), binary_override.clone());
        match self.slash_commands.read(&command_key) {
            Query::Ready(commands) => {
                let mut merged = composer_complete::merge_reported_commands(&commands, &reported);
                if !merged.iter().any(|command| command.name == "goal") {
                    merged.push(SlashCommand {
                        name: "goal".to_owned(),
                        description: tr!("goal.title"),
                        scope: composer_complete::CommandScope::Waku,
                        argument_hint: Some("<description>".to_owned()),
                        template: None,
                    });
                }
                self.slash_command_index = Rc::new(merged);
                self.slash_command_index_key = Some(command_key);
                self.slash_command_index_loading = false;
            }
            Query::Pending => {
                self.slash_command_index_loading = true;
                // A scan for this exact key is in flight; anything drawn
                // meanwhile must not be another provider's list.
                if self.slash_command_index_key.as_ref() != Some(&command_key) {
                    self.slash_command_index = Rc::new(Vec::new());
                    self.slash_command_index_key = None;
                }
            }
            Query::Missing(token) => {
                self.slash_command_index_loading = true;
                if self.slash_command_index_key.as_ref() != Some(&command_key) {
                    self.slash_command_index = Rc::new(Vec::new());
                    self.slash_command_index_key = None;
                }
                let path = project_path.clone();
                let Some(workspace) = self.workspace_client_for_path(&path) else {
                    self.slash_command_index_loading = false;
                    return;
                };
                cx.spawn(async move |waku, cx| {
                    let commands = cx
                        .background_executor()
                        .spawn(async move {
                            match workspace.request(
                                waku_client::WorkspaceOperation::DiscoverSlashCommands {
                                    provider,
                                    project_root: path,
                                    binary_override,
                                },
                            ) {
                                Ok(waku_client::WorkspaceResult::SlashCommands { commands }) => {
                                    commands
                                }
                                Ok(_) | Err(_) => Vec::new(),
                            }
                        })
                        .await;
                    waku.update(cx, |waku, cx| {
                        if waku.slash_commands.fulfill(token, commands) {
                            waku.refresh_composer_sources(cx);
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }

        match self.mention_files.read(&project_path) {
            Query::Ready(files) => {
                self.mention_file_index = files.as_ref().clone().into();
                self.mention_file_index_path = Some(project_path);
                self.mention_file_index_loading = false;
            }
            Query::Pending => {
                self.mention_file_index_loading = true;
                if self.mention_file_index_path.as_ref() != Some(&project_path) {
                    self.mention_file_index = Rc::new(Vec::new());
                    self.mention_file_index_path = None;
                }
            }
            Query::Missing(token) => {
                self.mention_file_index_loading = true;
                if self.mention_file_index_path.as_ref() != Some(&project_path) {
                    self.mention_file_index = Rc::new(Vec::new());
                    self.mention_file_index_path = None;
                }
                if self.workspace_client_for_path(&project_path).is_none() {
                    self.mention_file_index_loading = false;
                    self.mention_files.abandon(token);
                    return;
                }
                self.fetch_mention_files(token, project_path.clone(), cx);
            }
        }

        // The `Cmd+P` finder reads the same mirrored index; refresh its rows
        // when the index lands or is swapped out from under an open modal.
        if self.file_finder.is_open() {
            self.refresh_file_finder_results(cx);
        }
        // So does the palette's Prompts section — keep an open palette's
        // results in step with late-arriving discovery.
        self.refresh_open_command_palette(cx);
    }

    /// Run the daemon's file listing for `root`, claiming `token`'s fetch.
    /// The composer reads the workspace key; the `Cmd+P` finder reads
    /// whatever files root it resolved — both land in the same cache. The
    /// fulfill re-runs the composer mirror and an open finder's refresh so
    /// either consumer picks up the arrival.
    pub(super) fn fetch_mention_files(
        &mut self,
        token: crate::query::FetchToken<PathBuf>,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace_client_for_path(&root) else {
            self.mention_files.abandon(token);
            return;
        };
        cx.spawn(async move |waku, cx| {
            let files = cx
                .background_executor()
                .spawn(async move {
                    match workspace.request(waku_client::WorkspaceOperation::ListProjectFiles {
                        root,
                        cap: FILE_INDEX_CAP,
                    }) {
                        Ok(waku_client::WorkspaceResult::ProjectFiles { entries }) => entries,
                        Ok(_) | Err(_) => Vec::new(),
                    }
                })
                .await;
            waku.update(cx, |waku, cx| {
                if waku.mention_files.fulfill(token, files) {
                    waku.refresh_composer_sources(cx);
                    waku.refresh_file_finder_results(cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Invalidate and re-request both indexes for the selected workspace.
    pub(super) fn invalidate_composer_sources(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        {
            let provider = self
                .selected_session()
                .map(|session| session.provider)
                .unwrap_or(self.state.last_provider);
            let binary_override = self.state.provider_binary_overrides.get(&provider).cloned();
            self.slash_commands
                .invalidate(&(provider, path.clone(), binary_override));
            self.mention_files.invalidate(&path);
        }
        self.refresh_composer_sources(cx);
    }

    /// The trigger under the composer's caret, reconciled with the popup's
    /// cross-frame state. `None` while the composer is unfocused, the token is
    /// dismissed, or there is nothing to complete.
    fn composer_trigger(&self, window: &Window, cx: &App) -> Option<Trigger> {
        let input = self.composer.read(cx);
        let trigger = if input.focus().is_focused(window) {
            composer_complete::detect_trigger(input.content(cx), input.cursor(cx))
        } else {
            None
        };
        let trigger = trigger.filter(|trigger| {
            !matches!(trigger.kind, TriggerKind::WorkItem) || self.state.github_enabled
        });
        let ui = &self.composer_autocomplete;
        if *ui.token.borrow() != trigger {
            *ui.token.borrow_mut() = trigger.clone();
            // A different token renumbers the rows: the keyboard cursor and a
            // standing dismissal both describe the previous list.
            ui.highlight.set(0);
            ui.dismissed.set(false);
            ui.scroll.scroll_to_item(0);
        }
        if ui.dismissed.get() {
            return None;
        }
        trigger
    }

    /// The project the composer addresses: the target session's own for a
    /// live task, or the new-task draft's destination — which is also where
    /// Big Picture's unarmed composer lands. `None` when no draft slot is
    /// live leaves the pool unscoped.
    fn composer_project_id(&self) -> Option<Uuid> {
        match self.composer_draft_key()? {
            crate::persistence::ComposerDraftKey::NewSession(project_id) => Some(project_id),
            crate::persistence::ComposerDraftKey::Session(session_id) => self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.project_id),
        }
    }

    /// Sessions the `@` popup can offer — the same set the sidebar drags:
    /// started, unarchived, not side chats, in the composer's own project,
    /// minus the session the composer addresses and any already staged.
    /// Recent activity first.
    fn mentionable_sessions(&self) -> Vec<ComposerSessionRef> {
        let project = self.composer_project_id();
        let project_name = |session: &AgentSession| {
            self.state
                .projects
                .iter()
                .find(|project| project.id == session.project_id)
                .map_or(SharedString::default(), |project| {
                    project.name.clone().into()
                })
        };
        let mut sessions: Vec<&AgentSession> = self
            .state
            .sessions
            .iter()
            .filter(|session| {
                session_mention_candidate(session, project)
                    && self.session_atom_allowed(session.id)
            })
            .collect();
        sessions.sort_by_key(|session| {
            std::cmp::Reverse(session.last_reply_at.unwrap_or(session.updated_at))
        });
        sessions
            .into_iter()
            .map(|session| ComposerSessionRef {
                id: session.id,
                title: SharedString::from(session.display_title().to_owned()),
                project: project_name(session),
            })
            .collect()
    }

    /// A cheap fingerprint of what the session rows derive from — the
    /// offerable set and the exclusions — so a session title edit or a
    /// freshly staged atom invalidates the memo the way a new file index
    /// does.
    fn session_mention_fingerprint(&self) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.composer_target_session().hash(&mut hasher);
        let project = self.composer_project_id();
        project.hash(&mut hasher);
        for atom in &self.composer_inline_atoms {
            atom.session_id().hash(&mut hasher);
        }
        for attachment in &self.composer_attachments {
            attachment.session_id.hash(&mut hasher);
        }
        for session in &self.state.sessions {
            if session_mention_candidate(session, project) {
                session.id.hash(&mut hasher);
                session.display_title().hash(&mut hasher);
            }
        }
        hasher.finish() as usize
    }

    /// The filtered rows for `trigger`, shared by the popup body, the keyboard
    /// cursor and `enter` so an index always means the same row everywhere.
    fn autocomplete_rows(&self, trigger: &Trigger) -> Rc<Vec<AutocompleteRow>> {
        let work_item_items = match trigger.kind {
            TriggerKind::WorkItem => self
                .selected_workspace_path()
                .and_then(|path| self.work_item_mentions.get(path))
                .map(|state| state.items.clone()),
            _ => None,
        };
        let source = match trigger.kind {
            TriggerKind::Command => Rc::as_ptr(&self.slash_command_index) as usize,
            TriggerKind::File => {
                Rc::as_ptr(&self.mention_file_index) as usize ^ self.session_mention_fingerprint()
            }
            // `0` while no fetch has landed — a fresh `Rc` per call would
            // defeat the memo.
            TriggerKind::WorkItem => work_item_items
                .as_ref()
                .map(|items| Rc::as_ptr(items) as usize)
                .unwrap_or(0),
        };
        // `/incognito` only exists on drafts — a started session's boundary
        // is fixed at creation.
        let incognito_offered = self
            .composer_session()
            .is_none_or(|session| !session.has_started());
        {
            let memo = self.composer_autocomplete.results.borrow();
            if let Some(memo) = memo.as_ref().filter(|memo| {
                memo.kind == trigger.kind
                    && memo.query == trigger.query
                    && memo.source == source
                    && memo.incognito_offered == incognito_offered
            }) {
                return memo.rows.clone();
            }
        }
        let mut matcher = self.composer_autocomplete.matcher.borrow_mut();
        let rows = match trigger.kind {
            TriggerKind::Command => composer_complete::filter_commands(
                &self.slash_command_index,
                &trigger.query,
                &mut matcher,
            )
            .into_iter()
            .filter(|scored| incognito_offered || scored.item.name != "incognito")
            .map(AutocompleteRow::Command)
            .collect::<Vec<_>>(),
            TriggerKind::File => {
                // Session mentions lead: a title match names a task the user
                // is thinking about, while a broad query still leaves files
                // reachable below the cap.
                let sessions = self.mentionable_sessions();
                let titles = sessions
                    .iter()
                    .map(|session| session.title.as_str())
                    .collect::<Vec<_>>();
                composer_complete::filter_scored(
                    &titles,
                    &trigger.query,
                    &mut matcher,
                    SESSION_MENTION_CAP,
                )
                .into_iter()
                .map(|(index, positions)| {
                    AutocompleteRow::Session(Scored {
                        item: sessions[index].clone(),
                        positions,
                    })
                })
                .chain(
                    composer_complete::filter_files(
                        &self.mention_file_index,
                        &trigger.query,
                        &mut matcher,
                    )
                    .into_iter()
                    .map(AutocompleteRow::File),
                )
                .collect()
            }
            // The remote search already narrowed `items` to the query; the
            // local pass only re-ranks so digits hit the number and text hits
            // the title, and so a still-in-flight query keeps the stale list
            // useful while it types ahead of `gh`.
            TriggerKind::WorkItem => composer_complete::filter_work_items(
                work_item_items.as_deref().map_or(&[][..], Vec::as_slice),
                &trigger.query,
                &mut matcher,
            )
            .into_iter()
            .map(AutocompleteRow::WorkItem)
            .collect(),
        };
        let rows = Rc::new(rows);
        *self.composer_autocomplete.results.borrow_mut() = Some(ResultsMemo {
            kind: trigger.kind,
            query: trigger.query.clone(),
            source,
            incognito_offered,
            rows: rows.clone(),
        });
        rows
    }

    pub(super) fn move_autocomplete_highlight(
        &mut self,
        key: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(trigger) = self.composer_trigger(window, cx) else {
            return;
        };
        let rows = self.autocomplete_rows(&trigger);
        let ui = &self.composer_autocomplete;
        let current = ui.highlight.get().min(rows.len().saturating_sub(1));
        let Some(next) = next_picker_highlight(Some(current), rows.len(), key) else {
            return;
        };
        ui.highlight.set(next);
        ui.scroll.scroll_to_item(next);
        cx.notify();
    }

    /// Insert the chosen row over the trigger token. `index` comes from a
    /// click; `None` is the keyboard path, which takes the drawn cursor and
    /// defaults to the first row so `enter` works the moment the popup opens.
    pub(super) fn accept_autocomplete(
        &mut self,
        index: Option<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(trigger) = self.composer_trigger(window, cx) else {
            return;
        };
        let rows = self.autocomplete_rows(&trigger);
        let index = index.unwrap_or_else(|| {
            self.composer_autocomplete
                .highlight
                .get()
                .min(rows.len().saturating_sub(1))
        });
        let Some(row) = rows.get(index) else {
            return;
        };
        // A session row splices the marker over the trigger token directly —
        // one splice, so the atom's recorded seat is the post-splice position
        // the remap keeps.
        if let AutocompleteRow::Session(scored) = row {
            let session = scored.item.clone();
            if self.session_atom_allowed(session.id) {
                let marker = self.composer.update(cx, |input, cx| {
                    input.insert_inline_marker_at(trigger.range.clone(), cx)
                });
                self.record_session_atom(session.id, &session.title, marker, cx);
            }
            cx.notify();
            return;
        }
        let insert = match row {
            AutocompleteRow::Command(scored) => {
                let composer_text = composer_complete::command_composer_text(&scored.item);
                format!("{composer_text} ")
            }
            AutocompleteRow::File(scored) => format!("@{} ", scored.item.path),
            // The plain-text token is the durable mention — expansion to a
            // titled link happens at the transport boundary, like a command
            // template.
            AutocompleteRow::WorkItem(scored) => format!("#{} ", scored.item.number),
            AutocompleteRow::Session(_) => unreachable!(),
        };
        if matches!(row, AutocompleteRow::Command(_)) {
            let mut submission = self.composer.read(cx).content(cx).to_owned();
            submission.replace_range(trigger.range.clone(), &insert);
            if self.execute_local_composer_command(&submission, cx) {
                return;
            }
        }
        self.composer.update(cx, |input, cx| {
            input.replace_range(trigger.range.clone(), &insert, cx);
        });
        cx.notify();
    }

    pub(super) fn dismiss_autocomplete(&mut self, cx: &mut Context<Self>) {
        self.composer_autocomplete.dismissed.set(true);
        cx.notify();
    }

    /// Schedule the remote `gh` search behind a `#` trigger, debounced so a
    /// keystroke burst is one search rather than one per character. Safe to
    /// call from the render path: the timer only arms work that lands through
    /// `start_work_item_search` on the background executor.
    fn schedule_work_item_search(&self, trigger: &Trigger, cx: &mut Context<Self>) {
        if trigger.kind != TriggerKind::WorkItem {
            return;
        }
        let Some(path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        if self.work_item_mentions.get(&path).is_some_and(|state| {
            state.items_query == trigger.query
                || state.requested_query.as_deref() == Some(trigger.query.as_str())
        }) {
            return;
        }
        let ui = &self.composer_autocomplete;
        let pending = (path.clone(), trigger.query.clone());
        if ui.work_item_scheduled.borrow().as_ref() == Some(&pending) {
            return;
        }
        *ui.work_item_scheduled.borrow_mut() = Some(pending);
        let debounce = ui.work_item_debounce.get().wrapping_add(1);
        ui.work_item_debounce.set(debounce);
        let query = trigger.query.clone();
        cx.spawn(async move |waku, cx| {
            cx.background_executor()
                .timer(WORK_ITEM_SEARCH_DEBOUNCE)
                .await;
            waku.update(cx, |waku, cx| {
                if waku.composer_autocomplete.work_item_debounce.get() != debounce {
                    return;
                }
                *waku.composer_autocomplete.work_item_scheduled.borrow_mut() = None;
                waku.start_work_item_search(path, query, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Fire one `#` search: resolve the repo when unknown, query issues and
    /// pull requests in parallel, and — for an all-digit query — fill in the
    /// exact item `list --search` can miss. Everything runs on the background
    /// executor through the daemon; only the landing touches `self`.
    fn start_work_item_search(&mut self, path: PathBuf, query: String, cx: &mut Context<Self>) {
        let state = self.work_item_mentions.entry(path.clone()).or_default();
        let need_repo = state.repo.is_none();
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        state.requested_query = Some(query.clone());
        state.loading = true;
        cx.notify();
        let search = Some(query.trim().to_owned()).filter(|query| !query.is_empty());
        let number = query.trim().parse::<u64>().ok();
        let issues_client = waku_client::WorkspaceClient::new(self.daemon.client());
        let pulls_client = waku_client::WorkspaceClient::new(self.daemon.client());
        let repo_client = waku_client::WorkspaceClient::new(self.daemon.client());
        let issue_detail_client = waku_client::WorkspaceClient::new(self.daemon.client());
        let pr_detail_client = waku_client::WorkspaceClient::new(self.daemon.client());
        let cwd = path.clone();
        cx.spawn(async move |waku, cx| {
            let issues = {
                let cwd = cwd.clone();
                let search = search.clone();
                cx.background_executor().spawn(async move {
                    match issues_client.request(waku_client::WorkspaceOperation::ListIssues {
                        cwd,
                        state: WorkItemQueryState::All,
                        query: search,
                    }) {
                        Ok(waku_client::WorkspaceResult::Issues { entries }) => entries,
                        _ => None,
                    }
                })
            };
            let pull_requests = {
                let cwd = cwd.clone();
                let search = search.clone();
                cx.background_executor().spawn(async move {
                    match pulls_client.request(
                        waku_client::WorkspaceOperation::ListRepoPullRequests {
                            cwd,
                            state: WorkItemQueryState::All,
                            query: search,
                        },
                    ) {
                        Ok(waku_client::WorkspaceResult::PullRequests { entries }) => entries,
                        _ => None,
                    }
                })
            };
            let repo = need_repo.then(|| {
                let cwd = cwd.clone();
                cx.background_executor().spawn(async move {
                    match repo_client
                        .request(waku_client::WorkspaceOperation::ResolveGitHubRepo { cwd })
                    {
                        Ok(waku_client::WorkspaceResult::GitHubRepo { repo, availability }) => {
                            (repo, availability)
                        }
                        _ => (None, GitHubAvailability::Ready),
                    }
                })
            });
            let issues = issues.await;
            let pull_requests = pull_requests.await;
            let repo = match repo {
                Some(task) => Some(task.await),
                None => None,
            };
            // A digit query names a number, not a search term: `gh list
            // --search` does not promise a number match, so read the item
            // directly when neither list surfaced it.
            let mut exact = Vec::new();
            if let Some(number) = number {
                let listed = issues.iter().flatten().any(|issue| issue.number == number)
                    || pull_requests.iter().flatten().any(|pr| pr.number == number);
                if !listed {
                    let issue_task = {
                        let cwd = cwd.clone();
                        cx.background_executor().spawn(async move {
                            match issue_detail_client
                                .request(waku_client::WorkspaceOperation::GetIssue { cwd, number })
                            {
                                Ok(waku_client::WorkspaceResult::Issue {
                                    detail: Some(detail),
                                }) => Some(ComposerWorkItem::from_issue(detail.summary)),
                                _ => None,
                            }
                        })
                    };
                    let pr_task = {
                        let cwd = cwd.clone();
                        cx.background_executor().spawn(async move {
                            match pr_detail_client.request(
                                waku_client::WorkspaceOperation::GetPullRequest { cwd, number },
                            ) {
                                Ok(waku_client::WorkspaceResult::PullRequest {
                                    detail: Some(detail),
                                }) => Some(ComposerWorkItem::from_pull_request(detail.summary)),
                                _ => None,
                            }
                        })
                    };
                    exact.extend(issue_task.await);
                    exact.extend(pr_task.await);
                }
            }
            let merged = composer_complete::merge_work_items(
                issues.unwrap_or_default(),
                pull_requests.unwrap_or_default(),
                exact,
            );
            waku.update(cx, |waku, cx| {
                let Some(state) = waku.work_item_mentions.get_mut(&path) else {
                    return;
                };
                if state.generation != generation {
                    return;
                }
                if let Some(repo) = repo {
                    state.repo = Some(repo);
                }
                // A resolved non-GitHub repo keeps nothing — the rows would
                // be another host's items.
                let merged = match &state.repo {
                    Some((None, _)) => Vec::new(),
                    _ => merged,
                };
                for item in &merged {
                    state.known.insert(item.number, item.clone());
                }
                state.items = Rc::new(merged);
                state.items_query = query;
                state.loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The `#N` mentions in `content` resolved against the workspace's store —
    /// the chips a sent user bubble draws. Numbers the store never saw stay
    /// plain text; a chip promises a title and URL it cannot invent.
    pub(super) fn work_item_refs_for_content(
        &self,
        workspace: Option<&Path>,
        content: &str,
    ) -> Vec<ComposerWorkItem> {
        if !self.state.github_enabled || !content.contains('#') {
            return Vec::new();
        }
        let Some(state) = workspace.and_then(|path| self.work_item_mentions.get(path)) else {
            return Vec::new();
        };
        composer_complete::work_item_reference_numbers(content)
            .into_iter()
            .filter_map(|number| state.known.get(&number).cloned())
            .collect()
    }

    /// The popup, anchored above the composer card, or `None` when idle.
    ///
    /// Reads only the prefetched indexes — discovery never runs on a frame.
    pub(super) fn render_composer_autocomplete(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, bool)> {
        let trigger = self.composer_trigger(window, cx)?;
        self.schedule_work_item_search(&trigger, cx);
        let rows = self.autocomplete_rows(&trigger);
        let (loading, hint) = match trigger.kind {
            TriggerKind::Command => (self.slash_command_index_loading, None),
            TriggerKind::File => (self.mention_file_index_loading, None),
            TriggerKind::WorkItem => {
                let workspace = self.selected_workspace_path();
                let state = workspace.and_then(|path| self.work_item_mentions.get(path));
                let hint = if workspace.is_none() {
                    // A projectless session has no remote to search.
                    Some(tr!("github.not_a_repo"))
                } else {
                    state.and_then(|state| match &state.repo {
                        Some((None, availability)) => Some(match availability {
                            GitHubAvailability::MissingCli => tr!("github.install_gh"),
                            GitHubAvailability::Unauthenticated => tr!("github.auth_gh"),
                            GitHubAvailability::Ready => tr!("github.not_a_repo"),
                        }),
                        _ => None,
                    })
                };
                // No state yet means the first search has not landed; the
                // popup reads as loading until it (or the hint) does.
                (
                    hint.is_none() && state.map_or(true, |state| state.loading),
                    hint,
                )
            }
        };
        if rows.is_empty() && !loading && hint.is_none() {
            return None;
        }
        // The probe records during paint, so the first frame a composer ever
        // draws has no bounds yet; the popup appears one frame later.
        let card_bounds = self.composer_autocomplete.card_bounds.get()?;
        let theme = Theme::current(cx);
        let highlight = self
            .composer_autocomplete
            .highlight
            .get()
            .min(rows.len().saturating_sub(1));

        let mut list = div()
            .id("composer-autocomplete-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.composer_autocomplete.scroll)
            .p(px(4.0));
        if rows.is_empty() {
            let row = div()
                .h(px(30.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary);
            list = list.child(match hint {
                // Not a GitHub repo (or no working `gh`): say why instead of
                // spinning forever.
                Some(hint) => row
                    .child(icon("icons/github.svg", 12.0, theme.text_tertiary))
                    .child(hint),
                None => row
                    .child(crate::ui::motion::spin(icon(
                        "icons/loader-circle.svg",
                        12.0,
                        theme.text_tertiary,
                    )))
                    .child(tr!("composer.loading_suggestions")),
            });
        } else {
            for (index, row) in rows.iter().enumerate() {
                list = list
                    .child(self.render_autocomplete_row(index, row, highlight, &theme, window, cx));
            }
        }

        let anchor = point(card_bounds.origin.x, card_bounds.origin.y - px(6.0));
        Some((
            deferred(
                anchored()
                    .position(anchor)
                    .anchor(Anchor::BottomLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(motion::surface_enter(
                        "composer-autocomplete-enter",
                        div()
                            .occlude()
                            .w(card_bounds.size.width)
                            .max_h(px(302.0))
                            .rounded(px(13.0))
                            .border(hairline())
                            .border_color(theme.border_subtle)
                            .bg(theme.raised)
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.dismiss_autocomplete(cx);
                            }))
                            .child(list),
                    )),
            )
            .with_priority(crate::ui::menu::MENU_PAINT_PRIORITY)
            .into_any_element(),
            !rows.is_empty(),
        ))
    }

    fn render_autocomplete_row(
        &self,
        index: usize,
        row: &AutocompleteRow,
        highlight: usize,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let highlighted = highlight == index;
        let font = window.text_style().font();
        let base = div()
            .id(index)
            .h(px(30.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .when(highlighted, |element| element.bg(theme.overlay_strong))
            .hover(|element| element.bg(theme.overlay))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.accept_autocomplete(Some(index), window, cx);
                }),
            );
        match row {
            AutocompleteRow::Command(scored) => {
                let command = &scored.item;
                let composer_text = composer_complete::command_composer_text(command);
                let icon_path = if command.scope == composer_complete::CommandScope::Skill {
                    "icons/sparkle.svg"
                } else {
                    "icons/command.svg"
                };
                // Positions index the bare name; the drawn sigil shifts every
                // byte range right by one.
                let name_ranges = highlight_byte_ranges(&command.name, &scored.positions, 0)
                    .into_iter()
                    .map(|range| range.start + 1..range.end + 1)
                    .collect();
                let mut name_font = font.clone();
                name_font.weight = FontWeight::MEDIUM;
                base.child(icon(icon_path, 12.0, theme.text_tertiary))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(260.0))
                            .truncate()
                            .text_size(sp(12.5))
                            .child(matched_text(
                                composer_text,
                                name_ranges,
                                theme.text,
                                theme.accent,
                                name_font,
                            )),
                    )
                    .when_some(command.argument_hint.clone(), |element, hint| {
                        element.child(
                            div()
                                .flex_none()
                                .text_size(sp(12.5))
                                .text_color(theme.text_ghost)
                                .child(hint),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(command.description.clone())),
                    )
                    .child(
                        div()
                            .h(px(18.0))
                            .px(px(5.0))
                            .flex_none()
                            .rounded(px(4.0))
                            .border(hairline())
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_tertiary)
                            .child(command.scope.label()),
                    )
                    .into_any_element()
            }
            AutocompleteRow::Session(scored) => {
                let session = &scored.item;
                base.child(icon("icons/chat.svg", 12.0, theme.text_tertiary))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(300.0))
                            .truncate()
                            .text_size(sp(12.5))
                            .child(matched_text(
                                session.title.to_string(),
                                highlight_byte_ranges(&session.title, &scored.positions, 0),
                                theme.text,
                                theme.accent,
                                font.clone(),
                            )),
                    )
                    .when(!session.project.is_empty(), |element| {
                        element.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(sp(12.5))
                                .text_color(theme.text_ghost)
                                .child(session.project.clone()),
                        )
                    })
                    .into_any_element()
            }
            AutocompleteRow::File(scored) => {
                let file = &scored.item;
                // The row draws basename then directory, but the positions
                // index the full path — each segment recovers its own ranges.
                // A directory's trailing slash stays with the basename, so a
                // match on it still paints.
                let trimmed_len = file.path.trim_end_matches('/').len();
                let name_start = file.path[..trimmed_len]
                    .rfind('/')
                    .map_or(0, |index| index + 1);
                let name = &file.path[name_start..];
                let parent = &file.path[..name_start.saturating_sub(1)];
                let name_char_offset = file.path[..name_start].chars().count();
                let icon_path = if file.is_dir {
                    "icons/folder.svg"
                } else {
                    super::right_panel::file_icon_for_path(&file.path)
                };
                base.child(icon(icon_path, 13.0, theme.text_tertiary))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(300.0))
                            .truncate()
                            .text_size(sp(12.5))
                            .child(matched_text(
                                name.to_owned(),
                                highlight_byte_ranges(name, &scored.positions, name_char_offset),
                                theme.text,
                                theme.accent,
                                font.clone(),
                            )),
                    )
                    .when(!parent.is_empty(), |element| {
                        element.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(sp(12.5))
                                .child(matched_text(
                                    parent.to_owned(),
                                    highlight_byte_ranges(parent, &scored.positions, 0),
                                    theme.text_ghost,
                                    theme.accent,
                                    font,
                                )),
                        )
                    })
                    .into_any_element()
            }
            AutocompleteRow::WorkItem(scored) => {
                let item = &scored.item;
                // The glyph pair distinguishes issue from pull request and
                // state — color only reinforces it, matching the sidebar
                // badge's accessibility rule.
                let (icon_path, icon_color, state_label) = match item.kind {
                    WorkItemKind::Issue => match item.state {
                        ComposerWorkItemState::Open => {
                            ("icons/info.svg", theme.success, tr!("github.issue_open"))
                        }
                        _ => ("icons/check.svg", theme.info, tr!("github.issue_closed")),
                    },
                    WorkItemKind::PullRequest => {
                        let state = match item.state {
                            ComposerWorkItemState::Open => sidebar::SidebarPullRequestState::Open,
                            ComposerWorkItemState::Draft => sidebar::SidebarPullRequestState::Draft,
                            ComposerWorkItemState::Merged => {
                                sidebar::SidebarPullRequestState::Merged
                            }
                            ComposerWorkItemState::Closed => {
                                sidebar::SidebarPullRequestState::Closed
                            }
                        };
                        (
                            sidebar::sidebar_pull_request_icon(state),
                            sidebar::sidebar_pull_request_color(theme, state),
                            sidebar::sidebar_pull_request_state_label(state),
                        )
                    }
                };
                let number_text = format!("#{}", item.number);
                let title_char_offset = item.candidate_title_offset();
                let mut number_font = font.clone();
                number_font.weight = FontWeight::MEDIUM;
                base.child(icon(icon_path, 12.0, icon_color))
                    .child(
                        div().flex_none().text_size(sp(12.5)).child(matched_text(
                            number_text,
                            highlight_byte_ranges(&item.candidate(), &scored.positions, 0)
                                .into_iter()
                                .filter(|range| range.start <= item.candidate_title_offset())
                                .collect(),
                            theme.text,
                            theme.accent,
                            number_font,
                        )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(matched_text(
                                item.title.clone(),
                                highlight_byte_ranges(
                                    &item.title,
                                    &scored.positions,
                                    title_char_offset,
                                ),
                                theme.text_secondary,
                                theme.accent,
                                font.clone(),
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.5))
                            .text_color(theme.text_ghost)
                            .child(item.author.clone().unwrap_or_else(|| state_label)),
                    )
                    .into_any_element()
            }
        }
    }
}

/// Text with the fuzzy-matched byte ranges lifted to the accent colour and a
/// semibold weight, the runs tiling the string exactly. `ranges` are sorted
/// and non-overlapping, as [`highlight_byte_ranges`] returns them.
pub(super) fn matched_text(
    text: String,
    ranges: Vec<std::ops::Range<usize>>,
    base_color: Hsla,
    accent: Hsla,
    font: Font,
) -> StyledText {
    // A step above either base weight in these rows — regular file paths and
    // medium command names both read as "a bit bolder", not shouting.
    let mut accent_font = font.clone();
    accent_font.weight = FontWeight::SEMIBOLD;
    let run = |len: usize, font: Font, color: Hsla| TextRun {
        len,
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::new();
    let mut cursor = 0;
    for range in ranges {
        if range.start > cursor {
            runs.push(run(range.start - cursor, font.clone(), base_color));
        }
        runs.push(run(range.len(), accent_font.clone(), accent));
        cursor = range.end;
    }
    if cursor < text.len() {
        runs.push(run(text.len() - cursor, font.clone(), base_color));
    }
    StyledText::new(text).with_runs(runs)
}

/// The probe recording the composer card's bounds for the popup's anchor.
/// `inset_0` so it reports the border box, not the padded content box.
pub(super) fn composer_card_bounds_probe(
    cell: Rc<Cell<Option<Bounds<Pixels>>>>,
) -> impl IntoElement {
    canvas(
        move |bounds: Bounds<Pixels>, _, _| cell.set(Some(bounds)),
        |_, _, _, _| (),
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
mod tests {
    /// The popup renders on every keystroke frame; discovery walks the
    /// filesystem and forks subprocesses. The two must never meet: everything
    /// the render path shows comes from the prefetched indexes.
    #[test]
    fn the_autocomplete_render_path_does_no_filesystem_work() {
        let source = include_str!("./autocomplete.rs");
        let start = source
            .find("\n    fn composer_trigger(")
            .expect("composer_trigger must exist");
        let end = source
            .find("\n/// The probe recording")
            .expect("probe marker must exist");
        let render_paths = &source[start..end];
        for forbidden in [
            "discover_slash_commands(",
            "list_project_files(",
            "std::fs",
            "Command::new",
            "read_dir",
        ] {
            assert!(
                !render_paths.contains(forbidden),
                "the render path must not call `{forbidden}`; \
                 discovery belongs in refresh_composer_sources"
            );
        }
    }
}
