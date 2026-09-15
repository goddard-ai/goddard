//! The Git panel: a working-tree surface that shares the right panel's slot.
//!
//! The panels are alternatives, not tabs — opening one dismisses the other,
//! and the Git panel reuses the right panel's width, slide, and resize
//! affordances. Every Git read and mutation runs on the daemon through
//! workspace requests on the background executor; render only ever reads the
//! state those requests landed.

use std::rc::Rc;

use gpui::{KeyBinding, actions};

use waku_client::git::{
    CommitEntry, GitFileChange, GitPanelSnapshot, PullOutcome, PullStrategy, SyncInProgress,
};
use waku_client::workspace::{WorkspaceOperation, WorkspaceResult};

use super::*;

actions!(
    waku_git_panel,
    [GitPanelPrimaryAction, DismissGitPanelModal]
);

const PANEL_CONTEXT: &str = "GitPanel";
const PANEL_INPUT_CONTEXT: &str = "GitPanel > TextInput";
const MODAL_CONTEXT: &str = "GitPanelModal";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(
            "secondary-enter",
            GitPanelPrimaryAction,
            Some(PANEL_INPUT_CONTEXT),
        ),
        KeyBinding::new(
            "secondary-enter",
            GitPanelPrimaryAction,
            Some(PANEL_CONTEXT),
        ),
        // Dismissal works from the modal itself and from anywhere in the
        // panel, so Escape still reaches it while the input holds focus.
        KeyBinding::new("escape", DismissGitPanelModal, Some(PANEL_CONTEXT)),
        KeyBinding::new("escape", DismissGitPanelModal, Some(MODAL_CONTEXT)),
    ]);
}

/// `git log` page size for the commits section; more arrive as the list
/// scrolls to its end.
const GIT_PANEL_COMMIT_PAGE: usize = 100;
const GIT_PANEL_FILE_ROW_HEIGHT: f32 = 26.0;
const GIT_PANEL_COMMIT_ROW_HEIGHT: f32 = 26.0;
/// How far a file row's diff preview crosses the panel's left edge — the
/// card reads as peeking out from behind it rather than covering the row.
const GIT_PANEL_DIFF_OVERLAP: f32 = 2.0;
const GIT_PANEL_DIFF_WIDTH: f32 = 560.0;
const GIT_PANEL_DIFF_MAX_HEIGHT: f32 = 360.0;
const GIT_PANEL_HOVER_OPEN_DELAY: Duration = Duration::from_millis(350);
const GIT_PANEL_HOVER_CLOSE_DELAY: Duration = Duration::from_millis(150);
const GIT_PANEL_MODAL_WIDTH: f32 = 720.0;
const GIT_PANEL_MODAL_HEIGHT: f32 = 520.0;

/// What the panel's background task is doing. The action button renders the
/// pending label while one is in flight and refuses to start a second.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GitPanelPending {
    Generating { include_unstaged: bool },
    Committing,
    Pushing,
    Syncing(PullStrategy),
    AbortingSync,
}

impl GitPanelPending {
    fn label(&self) -> String {
        match self {
            GitPanelPending::Generating { .. } => tr!("commit.generating_message"),
            GitPanelPending::Committing => tr!("commit.committing"),
            GitPanelPending::Pushing => tr!("commit.pushing"),
            GitPanelPending::Syncing(PullStrategy::Rebase) => tr!("git_panel.syncing_rebase"),
            GitPanelPending::Syncing(PullStrategy::Merge) => tr!("git_panel.syncing_merge"),
            GitPanelPending::AbortingSync => tr!("git_panel.aborting"),
        }
    }
}

pub(super) struct GitPanelOperation {
    pub id: Uuid,
    pub workspace: PathBuf,
    pub pending: GitPanelPending,
}

/// Everything the panel needs, created when the panel opens and rebuilt when
/// the selected session moves to a different checkout.
pub(super) struct GitPanelState {
    pub id: Uuid,
    pub workspace: PathBuf,
    pub invocation: Option<crate::git_commit::AgentInvocation>,
    pub message: Entity<TextInput>,
    pub snapshot: Option<GitPanelSnapshot>,
    pub snapshot_loading: bool,
    /// Load or operation failure, shown under the commit box.
    pub error: Option<String>,
    pub commits: Vec<CommitEntry>,
    pub commits_loading: bool,
    /// `git log` returned a short page — the history is exhausted.
    pub commits_exhausted: bool,
    pub changes_scroll: ScrollHandle,
    pub changes_scrollbar: Rc<ScrollbarState>,
    pub commits_scroll: ScrollHandle,
    pub commits_scrollbar: Rc<ScrollbarState>,
    pub action_focus: FocusHandle,
}

/// A file row's hover preview state, tracked per `(path, staged)` pair.
pub(super) enum GitPanelFileDiff {
    Loading,
    Ready { snapshot: Arc<ReviewDiffSnapshot> },
    Failed(SharedString),
}

pub(super) struct GitPanelDiffHover {
    pub path: String,
    pub staged: bool,
    pub row_hovered: bool,
    pub card_hovered: bool,
    pub open: bool,
    pub scroll_handle: ScrollHandle,
    pub scrollbar: Rc<ScrollbarState>,
}

impl GitPanelDiffHover {
    fn targets(&self, path: &str, staged: bool) -> bool {
        self.path == path && self.staged == staged
    }
}

/// A commit row's tooltip; the message and stats already rode in with the
/// list, so hover only schedules the card.
pub(super) struct GitPanelCommitHover {
    pub sha: String,
    pub open: bool,
}

/// The commit-diff modal's state.
pub(super) struct GitPanelCommitDiff {
    pub sha: String,
    pub subject: String,
    pub body: String,
    pub state: GitPanelCommitDiffState,
    pub list_state: ListState,
    pub scrollbar: Rc<ScrollbarState>,
    pub body_scroll: ScrollHandle,
    pub body_scrollbar: Rc<ScrollbarState>,
}

pub(super) enum GitPanelCommitDiffState {
    Loading,
    Ready(Arc<ReviewDiffSnapshot>),
    Failed(String),
}

impl Waku {
    /// The Git icon button. It sits left of the right-panel toggle wherever
    /// that toggle renders: the main top bar, the right panel's header, and
    /// the Git panel's own.
    pub(super) fn render_git_panel_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id("toggle-git-panel")
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon("icons/git-branch.svg", 14.0, theme.text_tertiary))
            .tooltip(|window, cx| {
                Tooltip::new(tr!("git_panel.toggle"))
                    .action(&ToggleGitPanel)
                    .build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.set_git_panel_visible(!this.git_panel_visible, window, cx);
            }))
    }

    pub(super) fn toggle_git_panel_action(
        &mut self,
        _: &ToggleGitPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_git_panel_visible(!self.git_panel_visible, window, cx);
    }

    /// Show or hide the Git panel. It shares the right panel's slot, so the
    /// two are never visible at once and the slot's width and slide carry
    /// straight across a swap.
    pub(super) fn set_git_panel_visible(
        &mut self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_panel_visible == visible {
            return;
        }
        if visible {
            // The slot is exclusive — dismiss the right panel without its
            // own close semantics so the swap does not slide out and back.
            self.right_panel_visible = false;
            self.right_panel_pending_terminal_focus = None;
            self.git_panel_visible = true;
            self.open_git_panel(window, cx);
        } else {
            self.close_git_panel_state();
        }
        self.right_panel_slide = self.begin_panel_slide(self.right_panel_rendered_width, cx);
        self.persist_panel_layout();
        cx.notify();
    }

    /// Dismiss the Git panel wherever the right panel takes the slot — its
    /// toggle, a surface reveal, or a session restore — so the two never
    /// share it.
    pub(super) fn close_git_panel_state(&mut self) {
        if !self.git_panel_visible && self.git_panel.is_none() {
            return;
        }
        self.git_panel_visible = false;
        self.git_panel = None;
        self.git_panel_file_diffs.clear();
        self.git_panel_hover = None;
        self.git_panel_commit_hover = None;
        self.git_panel_sync_conflict = None;
        self.git_panel_unstaged_prompt = false;
        self.git_panel_commit_diff = None;
    }

    /// Whether the right-side slot is occupied by either panel — used by the
    /// header's traffic-light controls and the shared width math.
    pub(super) fn right_panel_slot_visible(&self) -> bool {
        self.right_panel_visible || self.git_panel_visible
    }

    /// Build the panel state for the selected session's workspace and start
    /// the first snapshot and commit page.
    fn open_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let invocation = self.git_panel_invocation();
        let message = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .placeholder(tr!("commit.message_placeholder"))
        });
        self.git_panel = Some(GitPanelState {
            id: Uuid::new_v4(),
            workspace,
            invocation,
            message: message.clone(),
            snapshot: None,
            snapshot_loading: false,
            error: None,
            commits: Vec::new(),
            commits_loading: false,
            commits_exhausted: false,
            changes_scroll: ScrollHandle::new(),
            changes_scrollbar: ScrollbarState::new(),
            commits_scroll: ScrollHandle::new(),
            commits_scrollbar: ScrollbarState::new(),
            action_focus: cx.focus_handle(),
        });
        self.git_panel_generation = self.git_panel_generation.wrapping_add(1);
        self.refresh_git_panel(cx);
        self.refresh_git_panel_commits(cx);
        let focus = message.read(cx).focus();
        // Two frames out: the panel needs a paint before its input can take
        // focus (the same deferral the commit dialog uses).
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
    }

    /// The agent invocation for generated commit messages — the same
    /// probe-derived binary the commit dialog uses.
    fn git_panel_invocation(&self) -> Option<crate::git_commit::AgentInvocation> {
        let session = self.selected_session()?;
        Some(crate::git_commit::AgentInvocation {
            provider: session.provider,
            binary: self.provider_probe(session.provider)?.path.clone()?,
            model: self.model_for_session(session).map(str::to_owned),
            reasoning_effort: session.reasoning_effort.clone(),
        })
    }

    /// The selected session moved to another checkout: point the open panel
    /// at it, dropping state that named the old working tree.
    pub(super) fn sync_git_panel_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        if self
            .git_panel
            .as_ref()
            .is_none_or(|panel| panel.workspace == workspace)
        {
            return;
        }
        let invocation = self.git_panel_invocation();
        let Some(panel) = self.git_panel.as_mut() else {
            return;
        };
        panel.id = Uuid::new_v4();
        panel.workspace = workspace;
        panel.invocation = invocation;
        panel.snapshot = None;
        panel.error = None;
        // Fetches in flight for the old checkout are discarded by the id
        // check on landing — clear the flags they would have left set.
        panel.snapshot_loading = false;
        panel.commits_loading = false;
        panel.commits.clear();
        panel.commits_exhausted = false;
        self.git_panel_file_diffs.clear();
        self.git_panel_hover = None;
        self.git_panel_commit_hover = None;
        self.git_panel_commit_diff = None;
        self.git_panel_generation = self.git_panel_generation.wrapping_add(1);
        self.refresh_git_panel(cx);
        self.refresh_git_panel_commits(cx);
    }

    /// Re-read the working tree. Superseded fetches cannot land over a newer
    /// panel state because the landing checks the generation arm.
    pub(super) fn refresh_git_panel(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.git_panel.as_mut() else {
            return;
        };
        if panel.snapshot_loading {
            return;
        }
        panel.snapshot_loading = true;
        let panel_id = panel.id;
        let workspace = panel.workspace.clone();
        self.git_panel_generation = self.git_panel_generation.wrapping_add(1);
        let generation = self.git_panel_generation;
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(WorkspaceOperation::InspectGitPanel { cwd: workspace })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                let Some(panel) = waku.git_panel.as_mut() else {
                    return;
                };
                if panel.id != panel_id || waku.git_panel_generation != generation {
                    return;
                }
                panel.snapshot_loading = false;
                match result {
                    Ok(WorkspaceResult::GitPanel { snapshot }) => {
                        panel.snapshot = snapshot;
                        panel.error = None;
                        // File previews name the pre-refresh working tree.
                        waku.git_panel_file_diffs.clear();
                        waku.git_panel_hover = None;
                    }
                    Ok(_) => {
                        panel.error = Some(tr!("git_panel.unexpected_result"));
                    }
                    Err(error) => {
                        panel.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Refetch the commits the list already shows — same window, so the
    /// scroll position survives a new commit landing at the top.
    fn refresh_git_panel_commits(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.git_panel.as_ref() else {
            return;
        };
        let limit = panel.commits.len().max(GIT_PANEL_COMMIT_PAGE);
        self.fetch_git_panel_commits(0, limit, false, cx);
    }

    /// The commits list scrolled to its end: pull the next page.
    fn ensure_more_git_panel_commits(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.git_panel.as_ref() else {
            return;
        };
        if panel.commits_loading || panel.commits_exhausted {
            return;
        }
        let skip = panel.commits.len();
        self.fetch_git_panel_commits(skip, GIT_PANEL_COMMIT_PAGE, true, cx);
    }

    fn fetch_git_panel_commits(
        &mut self,
        skip: usize,
        limit: usize,
        append: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.git_panel.as_mut() else {
            return;
        };
        if panel.commits_loading {
            return;
        }
        panel.commits_loading = true;
        let panel_id = panel.id;
        let workspace = panel.workspace.clone();
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(WorkspaceOperation::ListCommits {
                        cwd: workspace,
                        skip,
                        limit,
                    })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                let Some(panel) = waku.git_panel.as_mut() else {
                    return;
                };
                if panel.id != panel_id {
                    return;
                }
                panel.commits_loading = false;
                match result {
                    Ok(WorkspaceResult::Commits { entries }) => {
                        panel.commits_exhausted = entries.len() < limit;
                        if append {
                            panel.commits.extend(entries);
                        } else {
                            panel.commits = entries;
                        }
                    }
                    Ok(_) => {
                        panel.error = Some(tr!("git_panel.unexpected_result"));
                    }
                    Err(error) => {
                        panel.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Fetch a file row's hover diff once; a Ready/Failed entry is cached
    /// until the next working-tree refresh clears it.
    fn ensure_git_panel_file_diff(&mut self, path: &str, staged: bool, cx: &mut Context<Self>) {
        let key = (path.to_owned(), staged);
        if self.git_panel_file_diffs.contains_key(&key) {
            return;
        }
        let Some(panel) = self.git_panel.as_ref() else {
            return;
        };
        let panel_id = panel.id;
        let workspace = panel.workspace.clone();
        self.git_panel_file_diffs
            .insert(key.clone(), GitPanelFileDiff::Loading);
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        let path = path.to_owned();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client
                        .request(WorkspaceOperation::FileDiff {
                            cwd: workspace,
                            path,
                            staged,
                        })
                        .map(|result| match result {
                            WorkspaceResult::ReviewDiff { data } => {
                                crate::review_diff::parse_collected(
                                    ReviewDiffSource::Unstaged,
                                    &data.numstat,
                                    &data.patch,
                                    data.complete_context,
                                )
                            }
                            _ => crate::review_diff::parse_collected(
                                ReviewDiffSource::Unstaged,
                                "",
                                "",
                                true,
                            ),
                        })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                let Some(panel) = waku.git_panel.as_ref() else {
                    return;
                };
                if panel.id != panel_id {
                    return;
                }
                waku.git_panel_file_diffs.insert(
                    key,
                    match result {
                        Ok(snapshot) => GitPanelFileDiff::Ready {
                            snapshot: Arc::new(snapshot),
                        },
                        Err(error) => {
                            GitPanelFileDiff::Failed(SharedString::from(error.to_string()))
                        }
                    },
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Send a changed-file row to the Review surface, staged or unstaged to
    /// match the section the row lives in.
    fn open_git_panel_file_in_review(
        &mut self,
        path: String,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git_panel_hover = None;
        self.right_panel_pending_diff_file = Some(path);
        let source = if staged {
            ReviewDiffSource::Staged
        } else {
            ReviewDiffSource::Unstaged
        };
        self.set_right_panel_diff_source(source, cx);
        self.apply_pending_right_panel_diff_file(cx);
        let _ = window;
    }

    /// Point the Review surface's selection at the path the Git panel sent
    /// it. Runs when a snapshot is already loaded, and again from the
    /// snapshot landing when a refresh was needed first.
    pub(super) fn apply_pending_right_panel_diff_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.right_panel_pending_diff_file.clone() else {
            return;
        };
        let Some(index) = self
            .right_panel_diff_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.files.iter().position(|file| file.path == path))
        else {
            return;
        };
        self.right_panel_pending_diff_file = None;
        self.select_right_panel_diff_file(index, cx);
    }

    /// The action button's current job. Sync wins while upstream has commits
    /// the checkout lacks, then commit for a dirty tree, then push.
    fn git_panel_primary(&self) -> GitPanelPrimary {
        let Some(panel) = self.git_panel.as_ref() else {
            return GitPanelPrimary::Unavailable;
        };
        if self.git_panel_operation.is_some() {
            return GitPanelPrimary::Busy;
        }
        let Some(snapshot) = panel.snapshot.as_ref() else {
            return GitPanelPrimary::Unavailable;
        };
        if snapshot
            .upstream
            .as_ref()
            .is_some_and(|upstream| upstream.behind > 0)
        {
            return GitPanelPrimary::Sync;
        }
        if !snapshot.staged.is_empty() {
            return GitPanelPrimary::Commit;
        }
        if !snapshot.unstaged.is_empty() {
            return GitPanelPrimary::CommitUnstagedPrompt;
        }
        if snapshot.can_push {
            return GitPanelPrimary::Push;
        }
        GitPanelPrimary::Unavailable
    }

    /// The button and Cmd+Enter share this: commit the staged changes, or —
    /// with nothing staged — ask before sweeping the whole worktree in.
    fn run_git_panel_primary(&mut self, cx: &mut Context<Self>) {
        match self.git_panel_primary() {
            GitPanelPrimary::Sync => self.start_git_panel_sync(PullStrategy::Rebase, cx),
            GitPanelPrimary::Commit => self.run_git_panel_commit(false, cx),
            GitPanelPrimary::CommitUnstagedPrompt => {
                self.git_panel_unstaged_prompt = true;
                cx.notify();
            }
            GitPanelPrimary::Push => self.start_git_panel_push(cx),
            GitPanelPrimary::Busy | GitPanelPrimary::Unavailable => {}
        }
    }

    /// Commit staged changes — or, once the nothing-staged prompt confirms,
    /// sweep the whole worktree (`include_unstaged`). A blank message runs
    /// the message generator first and commits with its answer.
    fn run_git_panel_commit(&mut self, include_unstaged: bool, cx: &mut Context<Self>) {
        let Some(panel) = self.git_panel.as_ref() else {
            return;
        };
        let message = panel.message.read(cx).content().trim().to_owned();
        if message.is_empty() {
            self.start_git_panel_generate(include_unstaged, cx);
        } else {
            self.start_git_panel_commit(message, include_unstaged, cx);
        }
    }

    fn begin_git_panel_op(
        &mut self,
        pending: GitPanelPending,
        cx: &mut Context<Self>,
    ) -> Option<(Uuid, PathBuf)> {
        if self.git_panel_operation.is_some() {
            return None;
        }
        let panel = self.git_panel.as_ref()?;
        let id = Uuid::new_v4();
        let operation = GitPanelOperation {
            id,
            workspace: panel.workspace.clone(),
            pending,
        };
        self.git_panel_operation = Some(operation);
        cx.notify();
        Some((id, panel.workspace.clone()))
    }

    /// Generate a message for the pending commit, then commit with it.
    fn start_git_panel_generate(&mut self, include_unstaged: bool, cx: &mut Context<Self>) {
        let Some(invocation) = self
            .git_panel
            .as_ref()
            .and_then(|panel| panel.invocation.clone())
        else {
            if let Some(panel) = self.git_panel.as_mut() {
                panel.error = Some(tr!("commit.agent_unavailable"));
            }
            cx.notify();
            return;
        };
        let Some((op_id, workspace)) =
            self.begin_git_panel_op(GitPanelPending::Generating { include_unstaged }, cx)
        else {
            return;
        };
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(WorkspaceOperation::GenerateCommitMessage {
                        cwd: workspace,
                        include_unstaged,
                        invocation,
                    })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_git_panel_op(op_id, result, cx);
            });
        })
        .detach();
    }

    fn start_git_panel_commit(
        &mut self,
        message: String,
        include_unstaged: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((op_id, workspace)) = self.begin_git_panel_op(GitPanelPending::Committing, cx)
        else {
            return;
        };
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(WorkspaceOperation::Commit {
                        cwd: workspace,
                        message,
                        include_unstaged,
                        push: false,
                    })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_git_panel_op(op_id, result, cx);
            });
        })
        .detach();
    }

    fn start_git_panel_push(&mut self, cx: &mut Context<Self>) {
        let Some((op_id, workspace)) = self.begin_git_panel_op(GitPanelPending::Pushing, cx) else {
            return;
        };
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { client.request(WorkspaceOperation::Push { cwd: workspace }) })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_git_panel_op(op_id, result, cx);
            });
        })
        .detach();
    }

    /// `git pull --rebase` — or `--no-rebase` when the conflict modal chose
    /// Merge instead. A clean pull just refreshes; a conflicted one leaves
    /// the integration in progress and opens the modal.
    fn start_git_panel_sync(&mut self, strategy: PullStrategy, cx: &mut Context<Self>) {
        let Some((op_id, workspace)) =
            self.begin_git_panel_op(GitPanelPending::Syncing(strategy), cx)
        else {
            return;
        };
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(WorkspaceOperation::PullUpstream {
                        cwd: workspace,
                        strategy,
                    })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_git_panel_op(op_id, result, cx);
            });
        })
        .detach();
    }

    /// Merge instead: leave the stopped rebase, then pull as a merge.
    fn git_panel_merge_instead(&mut self, cx: &mut Context<Self>) {
        self.git_panel_sync_conflict = None;
        let Some((op_id, workspace)) = self.begin_git_panel_op(GitPanelPending::AbortingSync, cx)
        else {
            return;
        };
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client
                        .request(WorkspaceOperation::AbortSync {
                            cwd: workspace.clone(),
                        })
                        .and_then(|_| {
                            client.request(WorkspaceOperation::PullUpstream {
                                cwd: workspace,
                                strategy: PullStrategy::Merge,
                            })
                        })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_git_panel_op(op_id, result, cx);
            });
        })
        .detach();
    }

    fn git_panel_abort_sync(&mut self, cx: &mut Context<Self>) {
        self.git_panel_sync_conflict = None;
        let Some((op_id, workspace)) = self.begin_git_panel_op(GitPanelPending::AbortingSync, cx)
        else {
            return;
        };
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { client.request(WorkspaceOperation::AbortSync { cwd: workspace }) },
                )
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_git_panel_op(op_id, result, cx);
            });
        })
        .detach();
    }

    /// The conflict modal's Resolve in chat: hand the agent the conflicted
    /// pull and let it finish the integration.
    fn git_panel_resolve_in_chat(&mut self, cx: &mut Context<Self>) {
        let in_progress = self.git_panel_sync_conflict.take();
        let prompt = match in_progress {
            Some(SyncInProgress::Rebase) => tr!("git_panel.resolve_rebase_prompt"),
            Some(SyncInProgress::Merge) => tr!("git_panel.resolve_merge_prompt"),
            None => return,
        };
        self.submit_composer_submission(ComposerSubmission::plain(prompt), cx);
        cx.notify();
    }

    /// Every panel operation lands here: drop the pending marker, apply the
    /// result to the panel that asked for it, and refresh what moved.
    fn finish_git_panel_op(
        &mut self,
        op_id: Uuid,
        result: Result<WorkspaceResult, anyhow::Error>,
        cx: &mut Context<Self>,
    ) {
        let Some(op) = self
            .git_panel_operation
            .take_if(|operation| operation.id == op_id)
        else {
            return;
        };
        let same_panel = self
            .git_panel
            .as_ref()
            .is_some_and(|panel| panel.workspace == op.workspace);
        match result {
            Ok(WorkspaceResult::CommitMessage { message }) => {
                if let GitPanelPending::Generating { include_unstaged } = op.pending
                    && same_panel
                    && let Some(panel) = self.git_panel.as_mut()
                {
                    panel.error = None;
                    let message = message.clone();
                    panel
                        .message
                        .update(cx, |input, cx| input.set_content(message.clone(), cx));
                    self.start_git_panel_commit(message, include_unstaged, cx);
                }
                cx.notify();
            }
            Ok(WorkspaceResult::Pull {
                outcome: PullOutcome::Conflict { in_progress },
            }) => {
                self.git_panel_sync_conflict = Some(in_progress);
                self.invalidate_workspace_queries(cx);
                cx.notify();
            }
            Ok(_) => {
                if same_panel && let Some(panel) = self.git_panel.as_mut() {
                    panel.error = None;
                    if matches!(op.pending, GitPanelPending::Committing) {
                        panel
                            .message
                            .update(cx, |input, cx| input.set_content("", cx));
                    }
                }
                self.invalidate_workspace_queries(cx);
                self.refresh_git_panel(cx);
                self.refresh_git_panel_commits(cx);
            }
            Err(error) => {
                if same_panel && let Some(panel) = self.git_panel.as_mut() {
                    panel.error = Some(error.to_string());
                }
                self.invalidate_workspace_queries(cx);
                cx.notify();
            }
        }
    }

    fn stage_git_panel_file(&mut self, path: String, staged: bool, cx: &mut Context<Self>) {
        let Some(panel) = self.git_panel.as_ref() else {
            return;
        };
        let panel_id = panel.id;
        let workspace = panel.workspace.clone();
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if staged {
                        client.request(WorkspaceOperation::UnstageFile {
                            cwd: workspace,
                            path,
                        })
                    } else {
                        client.request(WorkspaceOperation::StageFile {
                            cwd: workspace,
                            path,
                        })
                    }
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                let Some(panel) = waku.git_panel.as_mut() else {
                    return;
                };
                if panel.id != panel_id {
                    return;
                }
                if let Err(error) = result {
                    panel.error = Some(error.to_string());
                }
                // Any inspect already in flight read the pre-stage index —
                // bump the generation so its landing is discarded, then
                // re-read.
                waku.git_panel_generation = waku.git_panel_generation.wrapping_add(1);
                panel.snapshot_loading = false;
                waku.invalidate_workspace_queries(cx);
                waku.refresh_git_panel(cx);
            });
        })
        .detach();
    }

    /// Track the pointer over a changed-file row; a dwell opens the diff
    /// preview, and once open gliding to a sibling retargets it immediately.
    fn git_panel_file_row_hovered(
        &mut self,
        path: String,
        staged: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if !hovered {
            let leaving = self
                .git_panel_hover
                .as_ref()
                .is_some_and(|hover| hover.targets(&path, staged) && hover.row_hovered);
            if !leaving {
                return;
            }
            if let Some(hover) = self.git_panel_hover.as_mut() {
                hover.row_hovered = false;
            }
            self.schedule_git_panel_hover_close(cx);
            return;
        }

        self.git_panel_hover_generation = self.git_panel_hover_generation.wrapping_add(1);
        let generation = self.git_panel_hover_generation;
        let mut retarget = false;
        match self.git_panel_hover.as_mut() {
            Some(hover) if hover.open => {
                if !hover.targets(&path, staged) {
                    hover.path = path.clone();
                    hover.staged = staged;
                    hover.scroll_handle = ScrollHandle::new();
                    retarget = true;
                }
                hover.row_hovered = true;
                hover.card_hovered = false;
            }
            _ => {
                self.git_panel_hover = Some(GitPanelDiffHover {
                    path: path.clone(),
                    staged,
                    row_hovered: true,
                    card_hovered: false,
                    open: false,
                    scroll_handle: ScrollHandle::new(),
                    scrollbar: ScrollbarState::new(),
                });
                retarget = true;
            }
        }
        if retarget {
            self.ensure_git_panel_file_diff(&path, staged, cx);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(GIT_PANEL_HOVER_OPEN_DELAY)
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.git_panel_hover_generation != generation {
                    return;
                }
                let Some(hover) = this.git_panel_hover.as_mut() else {
                    return;
                };
                if !hover.row_hovered || hover.open {
                    return;
                }
                hover.open = true;
                let (path, staged) = (hover.path.clone(), hover.staged);
                this.ensure_git_panel_file_diff(&path, staged, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn git_panel_file_card_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let Some(hover) = self.git_panel_hover.as_mut() else {
            return;
        };
        if hover.card_hovered == hovered {
            return;
        }
        hover.card_hovered = hovered;
        self.git_panel_hover_generation = self.git_panel_hover_generation.wrapping_add(1);
        if !hovered {
            self.schedule_git_panel_hover_close(cx);
        }
    }

    fn schedule_git_panel_hover_close(&mut self, cx: &mut Context<Self>) {
        self.git_panel_hover_generation = self.git_panel_hover_generation.wrapping_add(1);
        let generation = self.git_panel_hover_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(GIT_PANEL_HOVER_CLOSE_DELAY)
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.git_panel_hover_generation != generation {
                    return;
                }
                let close = this
                    .git_panel_hover
                    .as_ref()
                    .is_some_and(|hover| !hover.row_hovered && !hover.card_hovered);
                if close {
                    this.git_panel_hover = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The commit tooltip dwells before it opens but closes the moment the
    /// row loses the pointer — it sits under the row, so there is no card to
    /// keep alive for.
    fn git_panel_commit_row_hovered(&mut self, sha: String, hovered: bool, cx: &mut Context<Self>) {
        if !hovered {
            let leaving = self
                .git_panel_commit_hover
                .as_ref()
                .is_some_and(|hover| hover.sha == sha);
            if !leaving {
                return;
            }
            self.git_panel_commit_hover = None;
            self.git_panel_commit_hover_generation =
                self.git_panel_commit_hover_generation.wrapping_add(1);
            cx.notify();
            return;
        }

        self.git_panel_commit_hover = Some(GitPanelCommitHover { sha, open: false });
        self.git_panel_commit_hover_generation =
            self.git_panel_commit_hover_generation.wrapping_add(1);
        let generation = self.git_panel_commit_hover_generation;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(GIT_PANEL_HOVER_OPEN_DELAY)
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.git_panel_commit_hover_generation != generation {
                    return;
                }
                let Some(hover) = this.git_panel_commit_hover.as_mut() else {
                    return;
                };
                if hover.open {
                    return;
                }
                hover.open = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// A commit row's click: fetch its diff and open the modal.
    fn open_git_panel_commit_diff(&mut self, entry: &CommitEntry, cx: &mut Context<Self>) {
        self.git_panel_commit_hover = None;
        let Some(panel) = self.git_panel.as_ref() else {
            return;
        };
        let workspace = panel.workspace.clone();
        let sha = entry.sha.clone();
        let requested_sha = sha.clone();
        self.git_panel_commit_diff = Some(GitPanelCommitDiff {
            sha: sha.clone(),
            subject: entry.subject.clone(),
            body: entry.body.clone(),
            state: GitPanelCommitDiffState::Loading,
            list_state: ListState::new(0, ListAlignment::Top, px(GIT_PANEL_MODAL_HEIGHT)),
            scrollbar: ScrollbarState::new(),
            body_scroll: ScrollHandle::new(),
            body_scrollbar: ScrollbarState::new(),
        });
        cx.notify();
        let client = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client
                        .request(WorkspaceOperation::CommitDiff {
                            cwd: workspace,
                            sha,
                        })
                        .map(|result| match result {
                            WorkspaceResult::ReviewDiff { data } => {
                                crate::review_diff::parse_collected(
                                    ReviewDiffSource::Commit,
                                    &data.numstat,
                                    &data.patch,
                                    data.complete_context,
                                )
                            }
                            _ => crate::review_diff::parse_collected(
                                ReviewDiffSource::Commit,
                                "",
                                "",
                                true,
                            ),
                        })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                // A result only lands on the modal still showing the sha it
                // was requested for — a retargeted modal keeps its own fetch.
                let Some(modal) = waku.git_panel_commit_diff.as_mut() else {
                    return;
                };
                if modal.sha != requested_sha {
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        let count = snapshot.lines.len();
                        modal.state = GitPanelCommitDiffState::Ready(Arc::new(snapshot));
                        modal.list_state.reset(count);
                    }
                    Err(error) => {
                        modal.state = GitPanelCommitDiffState::Failed(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The app a commit file opens in: the persisted "open in" choice while
    /// it is an editor, otherwise the first installed editor — the catalog
    /// also lists the file manager and terminals, which cannot take a file.
    /// A deliberate non-editor pick is still honored last.
    fn preferred_file_app(&self) -> Option<&crate::platform::ExternalApp> {
        let persisted = self
            .state
            .open_in_app
            .as_deref()
            .and_then(|id| self.open_in_apps.iter().find(|app| app.id == id));
        persisted
            .filter(|app| app.is_editor())
            .or_else(|| self.open_in_apps.iter().find(|app| app.is_editor()))
            .or(persisted)
    }

    /// A file's "open" affordance in the commit modal — `None` opens the
    /// file, `Some` lands the editor on that line when the app takes a line
    /// deep link.
    fn open_commit_file(&self, relative_path: &str, line: Option<u32>, cx: &mut Context<Self>) {
        let Some(workspace) = self.git_panel.as_ref().map(|panel| panel.workspace.clone()) else {
            return;
        };
        let path = workspace.join(relative_path);
        match self.preferred_file_app() {
            Some(app) => crate::platform::open_file_in_app(&path, line, app, cx),
            None => crate::platform::open_with_default_app(&path, cx),
        }
    }

    /// "Open in <app>" for the file header's button, naming the resolved
    /// target when one is installed.
    fn open_commit_file_tooltip(&self) -> String {
        match self.preferred_file_app() {
            Some(app) => tr!("git_panel.open_file_in", app = app.label),
            None => tr!("git_panel.open_file"),
        }
    }
}

enum GitPanelPrimary {
    Sync,
    Commit,
    CommitUnstagedPrompt,
    Push,
    Busy,
    Unavailable,
}

impl Waku {
    /// [`WakuPane`] delegate for the Git-panel island — same slot, width, and
    /// slide as the right panel.
    pub(super) fn git_panel_pane_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = self.effective_panel_widths(window).1;
        self.render_git_panel(width, window, cx).into_any_element()
    }

    fn render_git_panel(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id("git-panel")
            .key_context(PANEL_CONTEXT)
            .on_action(cx.listener(|this, _: &GitPanelPrimaryAction, _, cx| {
                this.run_git_panel_primary(cx);
            }))
            .on_action(cx.listener(|this, _: &DismissGitPanelModal, _, cx| {
                this.dismiss_git_panel_modal(cx);
            }))
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .min_w_0()
            .border_l(hairline())
            .border_color(theme.border_strong)
            .bg(theme.surface)
            .relative()
            .child(self.render_git_panel_header(window, cx))
            .child(self.render_git_panel_commit_area(cx))
            .child(self.render_git_panel_body(width, window, cx))
            .child(self.render_panel_resize_handle(
                "git-panel-resize-handle",
                PanelResizeTarget::RightPanel,
                cx,
            ))
    }

    fn render_git_panel_header(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let header = div()
            .id("git-panel-header")
            .h(px(44.0))
            .flex_none()
            .pl(px(12.0))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .border_b(hairline())
            .border_color(theme.border)
            .child(icon("icons/git-branch.svg", 13.0, theme.text_tertiary))
            .child(
                div()
                    .text_size(sp(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(tr!("git_panel.title")),
            )
            .child(div().flex_1());
        self.window_drag_region(
            header
                .child(self.render_git_panel_toggle(cx))
                .child(self.render_right_panel_toggle(cx))
                .children(self.render_client_window_controls(
                    super::window_chrome::WindowControlSide::Right,
                    window,
                    cx,
                )),
            cx,
        )
        .into_any_element()
    }

    /// The commit box: the message input and the state-aware action button.
    /// The branch the box commits onto heads the commits section instead.
    fn render_git_panel_commit_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(panel) = self.git_panel.as_ref() else {
            return div().into_any_element();
        };

        let message_box = div()
            .w_full()
            .min_h(px(56.0))
            .max_h(px(112.0))
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(10.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.inset)
            .text_size(sp(13.0))
            .line_height(sp(18.0))
            .text_color(theme.text)
            .child(panel.message.clone());

        div()
            .flex_none()
            .px(px(10.0))
            .pt(px(10.0))
            .pb(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .border_b(hairline())
            .border_color(theme.border)
            .child(message_box)
            .child(self.render_git_panel_action_button(cx))
            .when_some(panel.error.clone(), |area, error| {
                area.child(
                    div()
                        .text_size(sp(12.0))
                        .line_height(sp(16.0))
                        .text_color(theme.danger)
                        .child(error),
                )
            })
            .into_any_element()
    }

    /// The one button: Sync while behind upstream, Commit on a dirty tree,
    /// Push on a clean ahead one — and the pending label while any of them
    /// runs. Focusable, and ⌘↩ works from the message input.
    fn render_git_panel_action_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let primary = self.git_panel_primary();
        let pending_label = self
            .git_panel_operation
            .as_ref()
            .filter(|operation| {
                self.git_panel
                    .as_ref()
                    .is_some_and(|panel| panel.workspace == operation.workspace)
            })
            .map(|operation| operation.pending.label());
        let behind = self
            .git_panel
            .as_ref()
            .and_then(|panel| panel.snapshot.as_ref())
            .and_then(|snapshot| snapshot.upstream.as_ref())
            .map(|upstream| upstream.behind)
            .unwrap_or(0);
        let enabled = !matches!(
            primary,
            GitPanelPrimary::Busy | GitPanelPrimary::Unavailable
        );

        let (icon_path, label): (AnyElement, String) = if let Some(ref label) = pending_label {
            (
                motion::spin(icon("icons/loader-circle.svg", 12.0, theme.text_secondary))
                    .into_any_element(),
                label.clone(),
            )
        } else {
            match primary {
                GitPanelPrimary::Sync => (
                    icon("icons/rotate-cw.svg", 12.0, theme.text).into_any_element(),
                    tr!("git_panel.sync_changes"),
                ),
                GitPanelPrimary::Commit | GitPanelPrimary::CommitUnstagedPrompt => (
                    icon("icons/git-commit-horizontal.svg", 12.0, theme.text).into_any_element(),
                    tr!("git_panel.commit"),
                ),
                GitPanelPrimary::Push => (
                    icon("icons/cloud-upload.svg", 12.0, theme.text).into_any_element(),
                    tr!("git_panel.push_changes"),
                ),
                GitPanelPrimary::Unavailable => (
                    icon("icons/git-commit-horizontal.svg", 12.0, theme.text_ghost)
                        .into_any_element(),
                    tr!("git_panel.commit"),
                ),
                GitPanelPrimary::Busy => unreachable!(),
            }
        };

        let mut button = div()
            .id("git-panel-primary")
            .track_focus(
                &self
                    .git_panel
                    .as_ref()
                    .map(|panel| panel.action_focus.clone())
                    .unwrap_or_else(|| cx.focus_handle()),
            )
            .when(enabled, |button| button.tab_index(0))
            .h(px(30.0))
            .w_full()
            .rounded(px(8.0))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .px(px(10.0))
            .cursor_default()
            .text_size(sp(12.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if enabled {
                theme.text
            } else {
                theme.text_ghost
            })
            .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
            .child(icon_path)
            .child(label);
        if enabled {
            button = button
                .bg(theme.overlay)
                .hover(|style| style.bg(theme.overlay_strong))
                .active(|style| style.opacity(0.8))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.run_git_panel_primary(cx);
                }))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.run_git_panel_primary(cx);
                        cx.stop_propagation();
                    }
                }));
        } else {
            button = button.bg(theme.inset);
        }
        // The ⌘↩ chip — the binding fires whichever primary the button is
        // showing — plus Sync's trailing count and down arrow, pinned right
        // so the icon and label stay centered.
        if enabled && pending_label.is_none() {
            let mut accessory = div()
                .absolute()
                .right(px(6.0))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .h(px(18.0))
                        .px(px(6.0))
                        .rounded(px(9.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .border(hairline())
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .text_size(sp(11.0))
                        .text_color(theme.text_secondary)
                        .child(crate::platform::primary_shortcut("⌘↩", "Ctrl+Enter")),
                );
            if matches!(primary, GitPanelPrimary::Sync) {
                accessory = accessory
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .text_color(theme.text_tertiary)
                            .child(behind.to_string()),
                    )
                    .child(icon("icons/arrow-down.svg", 11.0, theme.text_tertiary));
            }
            button = button.child(accessory);
        }
        button.into_any_element()
    }

    /// Working-tree sections above the paged commit list. Each scrolls inside
    /// its own region so neither crowds the other off the panel.
    fn render_git_panel_body(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(panel) = self.git_panel.as_ref() else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .px(px(24.0))
                .text_center()
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary)
                .child(tr!("git_panel.no_task"))
                .into_any_element();
        };
        // The commits list fetches as it fills: an end-reaching viewport —
        // including one whose content has not overflowed yet — pulls the next
        // page until `git log` comes back short.
        let commits_near_end = {
            let scrolled = -panel.commits_scroll.offset().y;
            panel.commits_scroll.max_offset().y - scrolled <= px(160.0)
        };
        if commits_near_end {
            self.ensure_more_git_panel_commits(cx);
        }
        let Some(panel) = self.git_panel.as_ref() else {
            return div().flex_1().into_any_element();
        };

        let mut body = div().flex_1().min_h_0().flex().flex_col();
        match panel.snapshot.as_ref() {
            None if panel.snapshot_loading && panel.error.is_none() => {
                body = body.child(div().flex_1().flex().items_center().justify_center().child(
                    motion::spin(icon("icons/loader-circle.svg", 14.0, theme.text_tertiary)),
                ));
            }
            None => {
                body = body.child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(px(24.0))
                        .text_center()
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(tr!("git_panel.not_a_repository")),
                );
            }
            Some(snapshot) => {
                if !snapshot.staged.is_empty() || !snapshot.unstaged.is_empty() {
                    body = body.child(self.render_git_panel_changes(snapshot, width, window, cx));
                }
            }
        }
        body.child(self.render_git_panel_commits(width, cx))
            .into_any_element()
    }

    fn git_panel_section_label(label: String, theme: &Theme) -> AnyElement {
        div()
            .h(px(24.0))
            .flex_none()
            .px(px(10.0))
            .flex()
            .items_center()
            .text_size(sp(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text_tertiary)
            .child(label)
            .into_any_element()
    }

    /// The two change groups: unstaged files (untracked included) first, then
    /// a labeled Staged section whenever anything is staged.
    fn render_git_panel_changes(
        &self,
        snapshot: &GitPanelSnapshot,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let both_sections = !snapshot.staged.is_empty();
        let mut list = div().w_full().min_h_0().flex().flex_col();
        if !snapshot.unstaged.is_empty() {
            if both_sections {
                list = list.child(Self::git_panel_section_label(
                    tr!("git_panel.changes"),
                    &theme,
                ));
            }
            for file in &snapshot.unstaged {
                list = list.child(self.render_git_panel_file_row(file, false, width, window, cx));
            }
        }
        if both_sections {
            list = list.child(Self::git_panel_section_label(
                tr!("git_panel.staged"),
                &theme,
            ));
            for file in &snapshot.staged {
                list = list.child(self.render_git_panel_file_row(file, true, width, window, cx));
            }
        }
        let scroll = self
            .git_panel
            .as_ref()
            .map(|panel| panel.changes_scroll.clone())
            .unwrap_or_default();
        let scrollbar_state = self
            .git_panel
            .as_ref()
            .map(|panel| panel.changes_scrollbar.clone())
            .unwrap_or_else(|| ScrollbarState::new());
        let scroll_track = scroll.clone();
        div()
            .flex_none()
            .max_h(gpui::relative(0.5))
            .min_h_0()
            .flex()
            .flex_col()
            .border_b(hairline())
            .border_color(theme.border)
            .relative()
            .child(
                div()
                    .id("git-panel-changes")
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll_track)
                    .on_scroll_wheel(move |_, _, cx| contain_scroll(&scroll, cx))
                    .child(list),
            )
            .child(scrollbar::edge_fade(
                scroll_track.clone(),
                scrollbar::FadeEdge::Top,
                theme.surface,
            ))
            .child(scrollbar::edge_fade(
                scroll_track.clone(),
                scrollbar::FadeEdge::Bottom,
                theme.surface,
            ))
            .child(scrollbar::vertical(&scroll_track, &scrollbar_state))
            .into_any_element()
    }

    /// A changed-file row: status letter, icon, path, counts, the stage or
    /// unstage button — and, once the hover dwell opens it, the diff preview
    /// floating just past the panel's left edge.
    fn render_git_panel_file_row(
        &self,
        file: &GitFileChange,
        staged: bool,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let path = file.path.clone();
        let status_color = match file.status.chars().next().unwrap_or('M') {
            'A' | '?' => theme.success,
            'D' => theme.danger,
            'R' | 'C' => theme.accent,
            _ => theme.warning,
        };
        let focus = self
            .transcript_control_focus(format!("git-panel-file-{}-{}", staged as u8, file.path), cx);
        let action_icon = if staged {
            "icons/minus.svg"
        } else {
            "icons/plus.svg"
        };
        let action_tooltip = if staged {
            tr!("git_panel.unstage")
        } else {
            tr!("git_panel.stage")
        };
        let action_focus = self.transcript_control_focus(
            format!("git-panel-file-action-{}-{}", staged as u8, file.path),
            cx,
        );
        let row_hovered = self
            .git_panel_hover
            .as_ref()
            .is_some_and(|hover| hover.targets(&file.path, staged));
        let popover = (row_hovered
            && self
                .git_panel_hover
                .as_ref()
                .is_some_and(|hover| hover.open))
        .then(|| self.render_git_panel_file_popover(width, window, cx))
        .flatten();

        let path_for_action = path.clone();
        let path_for_action_key = path.clone();
        let path_for_hover = path.clone();
        let path_for_click = path.clone();
        div()
            .id(SharedString::from(format!(
                "git-panel-file-{}-{}",
                staged as u8, file.path
            )))
            .relative()
            .track_focus(&focus)
            .tab_index(0)
            .h(px(GIT_PANEL_FILE_ROW_HEIGHT))
            .pl(px(10.0))
            .pr(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_default()
            .text_size(sp(12.5))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.overlay))
            .child(
                div()
                    .w(px(14.0))
                    .flex_none()
                    .text_size(sp(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(status_color)
                    .child(file.status.clone()),
            )
            .child(file_icon(right_panel::file_icon_for_path(&file.path), 13.0))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "git-panel-file-path-{}-{}",
                        staged as u8, file.path
                    )))
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(theme.text_secondary)
                    .tooltip(Tooltip::text(file.path.clone()))
                    .child(file.path.clone()),
            )
            .when(!file.untracked, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(sp(11.0))
                        .text_color(theme.success)
                        .child(format!("+{}", file.additions)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(sp(11.0))
                        .text_color(theme.danger)
                        .child(format!("-{}", file.deletions)),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!(
                        "git-panel-file-action-{}-{}",
                        staged as u8, file.path
                    )))
                    .track_focus(&action_focus)
                    .tab_index(0)
                    .size(px(20.0))
                    .flex_none()
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .hover(|style| style.bg(theme.overlay_strong))
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .tooltip(Tooltip::text(action_tooltip))
                    .child(icon(action_icon, 11.0, theme.text_tertiary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.stage_git_panel_file(path_for_action.clone(), staged, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.stage_git_panel_file(path_for_action_key.clone(), staged, cx);
                            cx.stop_propagation();
                        }
                    })),
            )
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                this.git_panel_file_row_hovered(path_for_hover.clone(), staged, *hovered, cx);
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_git_panel_file_in_review(path_for_click.clone(), staged, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.open_git_panel_file_in_review(path.clone(), staged, window, cx);
                    cx.stop_propagation();
                }
            }))
            .children(popover)
    }

    /// The floating diff preview for a hovered file row. Its surface measures
    /// wider than the card — the trailing spacer sits inside the panel — so
    /// right alignment parks the card just past the panel's left edge.
    fn render_git_panel_file_popover(
        &self,
        panel_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let hover = self.git_panel_hover.as_ref()?;
        if !hover.open {
            return None;
        }
        let theme = Theme::current(cx);
        let path = hover.path.clone();
        let staged = hover.staged;
        let body = match self.git_panel_file_diffs.get(&(path.clone(), staged)) {
            None | Some(GitPanelFileDiff::Loading) => {
                transcript_view::changed_files_diff_message(tr!("diff.loading"), &theme)
            }
            Some(GitPanelFileDiff::Failed(error)) => {
                transcript_view::changed_files_diff_message(error.to_string(), &theme)
            }
            Some(GitPanelFileDiff::Ready { snapshot }) => {
                if snapshot
                    .lines
                    .iter()
                    .all(|line| matches!(line.kind, crate::review_diff::LineKind::FileHeader))
                {
                    transcript_view::changed_files_diff_message(tr!("diff.no_changes"), &theme)
                } else {
                    let code_family = crate::fonts::current(cx).code;
                    let style = right_panel::DiffRowStyle::activity(
                        self.state.code_font_size,
                        code_family.clone(),
                    );
                    let scroll = hover.scroll_handle.clone();
                    let mut rows = div().w_full().min_w_0().flex().flex_col();
                    for (index, line) in snapshot.lines.iter().enumerate() {
                        let row = match &line.kind {
                            crate::review_diff::LineKind::FileHeader => continue,
                            crate::review_diff::LineKind::Gap(gap) => {
                                transcript_view::activity_diff_break_row(
                                    Some(tr!("diff.unmodified_lines", count = gap.count())),
                                    code_family.clone(),
                                    &theme,
                                )
                            }
                            crate::review_diff::LineKind::HunkHeader
                            | crate::review_diff::LineKind::Meta => {
                                transcript_view::activity_diff_break_row(
                                    (!line.content.is_empty()).then(|| line.content.clone()),
                                    code_family.clone(),
                                    &theme,
                                )
                            }
                            crate::review_diff::LineKind::Context
                            | crate::review_diff::LineKind::Addition
                            | crate::review_diff::LineKind::Deletion => {
                                right_panel::render_diff_code_row(
                                    line,
                                    index,
                                    &format!("git-panel-diff-{staged}-{path}"),
                                    &self.transcript_selection,
                                    style.clone(),
                                    &theme,
                                )
                            }
                        };
                        rows = rows.child(row);
                    }
                    let wheel = scroll.clone();
                    div()
                        .w_full()
                        .min_h_0()
                        .relative()
                        .max_h(px(GIT_PANEL_DIFF_MAX_HEIGHT))
                        .child(
                            div()
                                .id("git-panel-file-diff-scroll")
                                .w_full()
                                .min_h_0()
                                .max_h(px(GIT_PANEL_DIFF_MAX_HEIGHT))
                                .overflow_y_scroll()
                                .track_scroll(&scroll)
                                .on_scroll_wheel(move |_, _, cx| contain_scroll(&wheel, cx))
                                .child(rows),
                        )
                        .child(scrollbar::vertical(&scroll, &hover.scrollbar))
                        .into_any_element()
                }
            }
        };
        let file = self
            .git_panel
            .as_ref()
            .and_then(|panel| panel.snapshot.as_ref())
            .and_then(|snapshot| {
                let files = if staged {
                    &snapshot.staged
                } else {
                    &snapshot.unstaged
                };
                files.iter().find(|file| file.path == path)
            })
            .cloned();
        let card = div()
            .id("git-panel-file-diff-card")
            .on_hover(cx.listener(|this, hovering, _, cx| {
                this.git_panel_file_card_hovered(*hovering, cx);
            }))
            .overflow_hidden()
            .rounded(px(12.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_lg()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(30.0))
                    .flex_none()
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .border_b(hairline())
                    .border_color(theme.border)
                    .child(file_icon(right_panel::file_icon_for_path(&path), 13.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child(path.clone()),
                    )
                    .when_some(file, |header, file| {
                        header
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(sp(11.0))
                                    .text_color(theme.success)
                                    .child(format!("+{}", file.additions)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(sp(11.0))
                                    .text_color(theme.danger)
                                    .child(format!("-{}", file.deletions)),
                            )
                    }),
            )
            .child(body);
        Some(git_panel_hover_card(card, panel_width, window))
    }

    /// The branch the commit box writes to, with its upstream counts — moved
    /// out of the commit area to head the list it describes.
    fn git_panel_branch_row(snapshot: &GitPanelSnapshot, theme: &Theme) -> AnyElement {
        let mut row = div()
            .h(px(24.0))
            .flex_none()
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(icon("icons/git-branch.svg", 11.0, theme.text_tertiary))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child(snapshot.branch.clone()),
            );
        if let Some(upstream) = snapshot.upstream.as_ref() {
            row = row.child(div().flex_1()).children(
                [
                    (upstream.behind, "icons/arrow-down.svg"),
                    (upstream.ahead, "icons/arrow-up.svg"),
                ]
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, icon_path)| {
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(icon(icon_path, 10.0, theme.text_tertiary))
                        .child(count.to_string())
                        .into_any_element()
                }),
            );
        } else {
            row = row.child(div().flex_1());
        }
        row.into_any_element()
    }

    /// The paged commit list. Rows mark commits the upstream lacks and dwell
    /// for their full message; a click opens the commit-diff modal.
    fn render_git_panel_commits(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(panel) = self.git_panel.as_ref() else {
            return div().into_any_element();
        };
        let ahead = panel
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.upstream.as_ref())
            .map(|upstream| upstream.ahead)
            .unwrap_or(0);
        let mut rows = div().w_full().flex().flex_col().py(px(4.0));
        for (index, entry) in panel.commits.iter().enumerate() {
            // `git log` starts at HEAD and the first `ahead` entries are the
            // ones the upstream has not seen.
            let unpushed = index < ahead as usize;
            rows = rows.child(self.render_git_panel_commit_row(entry, index, unpushed, width, cx));
        }
        if panel.commits_loading {
            rows = rows.child(
                div()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(motion::spin(icon(
                        "icons/loader-circle.svg",
                        12.0,
                        theme.text_tertiary,
                    ))),
            );
        } else if panel.commits.is_empty() {
            rows = rows.child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("git_panel.no_commits")),
            );
        }
        let scroll = panel.commits_scroll.clone();
        let scrollbar_state = panel.commits_scrollbar.clone();
        let wheel = scroll.clone();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .relative()
            .child(Self::git_panel_section_label(
                tr!("git_panel.commits"),
                &theme,
            ))
            .when_some(panel.snapshot.as_ref(), |section, snapshot| {
                section.child(Self::git_panel_branch_row(snapshot, &theme))
            })
            .child(
                div()
                    .id("git-panel-commits")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .on_scroll_wheel(move |_, _, cx| contain_scroll(&wheel, cx))
                    .child(rows),
            )
            .child(scrollbar::edge_fade(
                scroll.clone(),
                scrollbar::FadeEdge::Bottom,
                theme.surface,
            ))
            .child(scrollbar::vertical(&scroll, &scrollbar_state))
            .into_any_element()
    }

    fn render_git_panel_commit_row(
        &self,
        entry: &CommitEntry,
        index: usize,
        unpushed: bool,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let focus = self.transcript_control_focus(format!("git-panel-commit-{}", entry.sha), cx);
        let popover = self
            .git_panel_commit_hover
            .as_ref()
            .is_some_and(|hover| hover.sha == entry.sha && hover.open)
            .then(|| self.render_git_panel_commit_popover(width, cx))
            .flatten();
        let sha = entry.sha.clone();
        let entry_for_click = entry.clone();
        let entry_for_key = entry.clone();
        div()
            .id(SharedString::from(format!("git-panel-commit-{index}")))
            .relative()
            .track_focus(&focus)
            .tab_index(0)
            .h(px(GIT_PANEL_COMMIT_ROW_HEIGHT))
            .pl(px(10.0))
            .pr(px(10.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_default()
            .text_size(sp(12.5))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.overlay))
            .child(
                div()
                    .flex_none()
                    .font_family(crate::fonts::current(cx).code)
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(entry.short_sha.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(theme.text_secondary)
                    .child(entry.subject.clone()),
            )
            .when(unpushed, |row| {
                row.child(
                    div()
                        .id(SharedString::from(format!(
                            "git-panel-commit-unpushed-{index}"
                        )))
                        .flex_none()
                        .tooltip(Tooltip::text(tr!("git_panel.unpushed")))
                        .child(icon("icons/arrow-up.svg", 10.0, theme.accent)),
                )
            })
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                this.git_panel_commit_row_hovered(sha.clone(), *hovered, cx);
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_git_panel_commit_diff(&entry_for_click, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.open_git_panel_commit_diff(&entry_for_key, cx);
                    cx.stop_propagation();
                }
            }))
            .children(popover)
    }

    /// A commit row's tooltip, parked directly under the row: the full
    /// subject, a clamped body preview, then who authored it and its `+/-`
    /// totals.
    fn render_git_panel_commit_popover(
        &self,
        panel_width: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let hover = self.git_panel_commit_hover.as_ref()?;
        if !hover.open {
            return None;
        }
        let theme = Theme::current(cx);
        let entry = self
            .git_panel
            .as_ref()
            .and_then(|panel| panel.commits.iter().find(|entry| entry.sha == hover.sha))
            .cloned()?;
        let body = commit_body_text(&entry.body);
        let ago = format_time_ago(unix_time().saturating_sub(entry.authored_at));
        let byline = if entry.author.is_empty() {
            ago
        } else {
            format!("{} · {}", entry.author, ago)
        };
        let mut meta = div()
            .flex_none()
            .px(px(12.0))
            .py(px(5.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .border_t(hairline())
            .border_color(theme.border)
            .text_size(sp(11.0))
            .text_color(theme.text_tertiary)
            .when_some(commit_author_avatar(&entry.author_email), |meta, source| {
                meta.child(
                    img(source)
                        .w(px(14.0))
                        .h(px(14.0))
                        .flex_none()
                        .rounded(px(4.0)),
                )
            })
            .child(div().min_w_0().flex_1().truncate().child(byline));
        if entry.additions + entry.deletions > 0 {
            meta = meta
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.success)
                        .child(format!("+{}", entry.additions)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.danger)
                        .child(format!("-{}", entry.deletions)),
                );
        }
        let card = div()
            .id("git-panel-commit-message-card")
            // The card floats over neighboring rows; don't let clicks land on
            // the commit underneath.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .overflow_hidden()
            .rounded(px(10.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.raised)
            .shadow_lg()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(px(12.0))
                    .pt(px(8.0))
                    .pb(px(6.0))
                    .flex()
                    // Baseline so the hash sits on the subject's first line
                    // when the subject wraps rather than truncating.
                    .items_baseline()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_none()
                            .font_family(crate::fonts::current(cx).code)
                            .text_size(sp(11.0))
                            .text_color(theme.text_tertiary)
                            .child(entry.short_sha.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(sp(12.5))
                            .line_height(sp(16.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(entry.subject.clone()),
                    ),
            )
            .when(!body.is_empty(), |card| {
                card.child(
                    div()
                        .flex_none()
                        .px(px(12.0))
                        .pb(px(8.0))
                        .text_size(sp(12.0))
                        .line_height(sp(16.0))
                        .text_color(theme.text_secondary)
                        .line_clamp(4)
                        .child(body),
                )
            })
            .child(meta);
        Some(
            deferred(FloatingSurface::anchored_to_parent(
                div()
                    .w(px(panel_width))
                    .px(px(8.0))
                    .child(card)
                    .into_any_element(),
                MenuAlign::BelowLeft,
                px(4.0),
                px(8.0),
            ))
            .into_any_element(),
        )
    }

    /// The panel's three modals, drawn above the workspace.
    pub(super) fn render_git_panel_overlays(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // An open modal holds focus so Escape reaches its context no matter
        // where the pointer was last; the scrim keeps the panel unclickable
        // in the meantime.
        if self.git_panel_commit_diff.is_some()
            || self.git_panel_sync_conflict.is_some()
            || self.git_panel_unstaged_prompt
        {
            window.focus(&self.git_panel_modal_focus, cx);
        }
        let mut overlays = Vec::new();
        overlays.extend(self.render_git_panel_unstaged_modal(window, cx));
        overlays.extend(self.render_git_panel_conflict_modal(window, cx));
        overlays.extend(self.render_git_panel_commit_modal(window, cx));
        overlays
    }

    fn git_panel_modal_layer(
        &self,
        id: &'static str,
        card: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        deferred(motion::fade_in(
            SharedString::from(format!("{id}-scrim-enter")),
            div()
                .id(id)
                .absolute()
                .inset_0()
                .occlude()
                .bg(scrim)
                .p(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.dismiss_git_panel_modal(cx)),
                )
                .child(motion::modal_enter(
                    SharedString::from(format!("{id}-card-enter")),
                    card,
                )),
        ))
        .with_priority(4)
        .into_any_element()
    }

    fn dismiss_git_panel_modal(&mut self, cx: &mut Context<Self>) {
        if self.git_panel_commit_diff.is_some() {
            self.git_panel_commit_diff = None;
        } else if self.git_panel_sync_conflict.is_some() {
            self.git_panel_sync_conflict = None;
        } else if self.git_panel_unstaged_prompt {
            self.git_panel_unstaged_prompt = false;
        } else {
            return;
        }
        cx.notify();
    }

    /// "Nothing staged": Commit staged-only by default means a clean index
    /// needs an explicit choice before sweeping the whole worktree in.
    fn render_git_panel_unstaged_modal(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.git_panel_unstaged_prompt {
            return None;
        }
        let theme = Theme::current(cx);
        let unstaged = self
            .git_panel
            .as_ref()
            .and_then(|panel| panel.snapshot.as_ref())
            .map(|snapshot| snapshot.unstaged.len())
            .unwrap_or(0);
        let confirm_label = if unstaged == 1 {
            tr!("git_panel.commit_all_one")
        } else {
            tr!("git_panel.commit_all", count = unstaged)
        };
        let confirm = modal_button(
            "git-panel-commit-all",
            confirm_label,
            true,
            theme,
            cx.listener(|this, _, _, cx| {
                this.git_panel_unstaged_prompt = false;
                this.run_git_panel_commit(true, cx);
            }),
        );
        let cancel = modal_button(
            "git-panel-commit-all-cancel",
            tr!("common.cancel"),
            false,
            theme,
            cx.listener(|this, _, _, cx| {
                this.dismiss_git_panel_modal(cx);
            }),
        );
        let card = self.git_panel_modal_card(cx).child(
            div()
                .px(px(16.0))
                .py(px(14.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(sp(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(tr!("git_panel.nothing_staged")),
                )
                .child(
                    div()
                        .text_size(sp(12.5))
                        .line_height(sp(17.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("git_panel.nothing_staged_description")),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.0))
                        .child(cancel)
                        .child(confirm),
                ),
        );
        Some(self.git_panel_modal_layer("git-panel-unstaged-modal", card, cx))
    }

    /// The sync conflicted: Resolve in chat, Merge instead (rebase only), or
    /// abort back to where the checkout was.
    fn render_git_panel_conflict_modal(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let in_progress = self.git_panel_sync_conflict?;
        let theme = Theme::current(cx);
        let rebase = matches!(in_progress, SyncInProgress::Rebase);
        let title = if rebase {
            tr!("git_panel.rebase_conflict")
        } else {
            tr!("git_panel.merge_conflict")
        };
        let abort_label = if rebase {
            tr!("git_panel.abort_rebase")
        } else {
            tr!("git_panel.abort_merge")
        };
        let resolve = modal_button(
            "git-panel-resolve-in-chat",
            tr!("git_panel.resolve_in_chat"),
            true,
            theme,
            cx.listener(|this, _, _, cx| this.git_panel_resolve_in_chat(cx)),
        );
        let abort = modal_button(
            "git-panel-abort-sync",
            abort_label,
            false,
            theme,
            cx.listener(|this, _, _, cx| this.git_panel_abort_sync(cx)),
        );
        let mut buttons = div().flex().items_center().gap(px(6.0)).child(abort);
        if rebase {
            buttons = buttons.child(div().flex_1()).child(modal_button(
                "git-panel-merge-instead",
                tr!("git_panel.merge_instead"),
                false,
                theme,
                cx.listener(|this, _, _, cx| this.git_panel_merge_instead(cx)),
            ));
        } else {
            buttons = buttons.child(div().flex_1());
        }
        buttons = buttons.child(resolve);
        let card = self.git_panel_modal_card(cx).child(
            div()
                .px(px(16.0))
                .py(px(14.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(sp(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(sp(12.5))
                        .line_height(sp(17.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("git_panel.conflict_description")),
                )
                .child(buttons),
        );
        Some(self.git_panel_modal_layer("git-panel-conflict-modal", card, cx))
    }

    fn git_panel_modal_card(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id("git-panel-modal-card")
            .track_focus(&self.git_panel_modal_focus)
            .tab_index(0)
            .key_context(MODAL_CONTEXT)
            .on_action(cx.listener(|this, _: &DismissGitPanelModal, _, cx| {
                this.dismiss_git_panel_modal(cx);
            }))
            .w(px(420.0))
            .overflow_hidden()
            .rounded(px(16.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_xl()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    /// One commit's diff, opened from its row.
    fn render_git_panel_commit_modal(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.git_panel_commit_diff.as_ref()?;
        let theme = Theme::current(cx);
        let body = match &modal.state {
            GitPanelCommitDiffState::Loading => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(motion::spin(icon(
                    "icons/loader-circle.svg",
                    14.0,
                    theme.text_tertiary,
                )))
                .into_any_element(),
            GitPanelCommitDiffState::Failed(error) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .px(px(24.0))
                .text_size(sp(12.5))
                .text_color(theme.danger)
                .child(error.clone())
                .into_any_element(),
            GitPanelCommitDiffState::Ready(snapshot) => {
                if snapshot.files.is_empty() {
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(tr!("diff.no_changes"))
                        .into_any_element()
                } else {
                    let entity = cx.entity().downgrade();
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(
                            list(modal.list_state.clone(), move |index, _window, cx| {
                                entity
                                    .upgrade()
                                    .map(|entity| {
                                        entity.update(cx, |this, cx| {
                                            this.render_git_panel_commit_diff_line(index, cx)
                                        })
                                    })
                                    .unwrap_or_else(|| div().into_any_element())
                            })
                            .size_full(),
                        )
                        .child(scrollbar::vertical(&modal.list_state, &modal.scrollbar))
                        .into_any_element()
                }
            }
        };
        let card = div()
            .id("git-panel-commit-modal-card")
            .track_focus(&self.git_panel_modal_focus)
            .tab_index(0)
            .key_context(MODAL_CONTEXT)
            .on_action(cx.listener(|this, _: &DismissGitPanelModal, _, cx| {
                this.dismiss_git_panel_modal(cx);
            }))
            .w(px(GIT_PANEL_MODAL_WIDTH))
            .h(px(GIT_PANEL_MODAL_HEIGHT))
            .overflow_hidden()
            .rounded(px(16.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_xl()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(40.0))
                    .flex_none()
                    .px(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b(hairline())
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex_none()
                            .font_family(crate::fonts::current(cx).code)
                            .text_size(sp(12.0))
                            .text_color(theme.text_tertiary)
                            .child(modal.sha.chars().take(8).collect::<String>()),
                    )
                    .child(
                        div()
                            .id("git-panel-commit-modal-subject")
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .tooltip(Tooltip::text(modal.subject.clone()))
                            .child(modal.subject.clone()),
                    )
                    .child(
                        icon_button("git-panel-commit-modal-close", "icons/x.svg", theme)
                            .tooltip(Tooltip::text(tr!("common.close")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismiss_git_panel_modal(cx);
                            })),
                    ),
            )
            .when(!modal.body.is_empty(), |card| {
                let scroll = modal.body_scroll.clone();
                let wheel = scroll.clone();
                card.child(
                    div()
                        .flex_none()
                        .relative()
                        .max_h(px(84.0))
                        .border_b(hairline())
                        .border_color(theme.border)
                        .child(
                            div()
                                .id("git-panel-commit-modal-body")
                                .w_full()
                                .max_h(px(84.0))
                                .overflow_y_scroll()
                                .track_scroll(&scroll)
                                .on_scroll_wheel(move |_, _, cx| contain_scroll(&wheel, cx))
                                .child(
                                    div()
                                        .px(px(14.0))
                                        .py(px(8.0))
                                        .text_size(sp(12.5))
                                        .line_height(sp(17.0))
                                        .text_color(theme.text_secondary)
                                        .child(commit_body_text(&modal.body)),
                                ),
                        )
                        .child(scrollbar::vertical(&scroll, &modal.body_scrollbar)),
                )
            })
            .child(body);
        Some(self.git_panel_modal_layer("git-panel-commit-modal", card, cx))
    }

    /// A row in the commit-diff modal — the Review panel's own row shapes,
    /// minus the expand affordances.
    fn render_git_panel_commit_diff_line(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(modal) = self.git_panel_commit_diff.as_ref() else {
            return div().into_any_element();
        };
        let GitPanelCommitDiffState::Ready(snapshot) = &modal.state else {
            return div().into_any_element();
        };
        let Some(line) = snapshot.lines.get(index) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let code_family = crate::fonts::current(cx).code;
        match &line.kind {
            crate::review_diff::LineKind::FileHeader => {
                let Some(file) = snapshot.files.get(line.file_index) else {
                    return div().into_any_element();
                };
                let open_path = file.path.clone();
                div()
                    .w_full()
                    .min_w_0()
                    .h(px(26.0))
                    .pl(px(12.0))
                    .pr(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .bg(theme.overlay)
                    .border_b(hairline())
                    .border_color(theme.border)
                    .child(file_icon(right_panel::file_icon_for_path(&file.path), 13.0))
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "git-panel-commit-modal-file-{}",
                                line.file_index
                            )))
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .tooltip(Tooltip::text(file.path.clone()))
                            .child(file.path.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.0))
                            .text_color(theme.success)
                            .child(format!("+{}", file.additions)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.0))
                            .text_color(theme.danger)
                            .child(format!("-{}", file.deletions)),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!(
                                "git-panel-commit-modal-open-{}",
                                line.file_index
                            )),
                            "icons/external-link.svg",
                            theme,
                        )
                        .tooltip(Tooltip::text(self.open_commit_file_tooltip()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_commit_file(&open_path, None, cx);
                            cx.stop_propagation();
                        })),
                    )
                    .into_any_element()
            }
            crate::review_diff::LineKind::Gap(gap) => transcript_view::activity_diff_break_row(
                Some(tr!("diff.unmodified_lines", count = gap.count())),
                code_family,
                &theme,
            ),
            crate::review_diff::LineKind::HunkHeader | crate::review_diff::LineKind::Meta => {
                transcript_view::activity_diff_break_row(
                    (!line.content.is_empty()).then(|| line.content.clone()),
                    code_family,
                    &theme,
                )
            }
            crate::review_diff::LineKind::Context
            | crate::review_diff::LineKind::Addition
            | crate::review_diff::LineKind::Deletion => {
                let row = right_panel::render_diff_code_row(
                    line,
                    index,
                    "git-panel-commit-diff",
                    &self.transcript_selection,
                    right_panel::DiffRowStyle::activity(self.state.code_font_size, code_family),
                    &theme,
                );
                let Some(file) = snapshot.files.get(line.file_index) else {
                    return row;
                };
                // A double-click opens the file at the row's shown line —
                // the postimage number, falling back to the preimage for
                // deletions.
                let path = file.path.clone();
                let target_line = line.new_line.or(line.old_line);
                div()
                    .id(SharedString::from(format!(
                        "git-panel-commit-diff-open-{index}"
                    )))
                    .w_full()
                    .child(row)
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 {
                            this.open_commit_file(&path, target_line, cx);
                        }
                    }))
                    .into_any_element()
            }
        }
    }
}

/// The author's avatar image: their GitHub picture when the commit used
/// GitHub's `noreply` alias (`<id>+<user>@users.noreply.github.com` or the
/// bare `<user>@` form), the Gravatar for the address otherwise. `None` for
/// a missing email — the byline then reads as before.
fn commit_author_avatar(email: &str) -> Option<SharedString> {
    let email = email.trim();
    if email.is_empty() {
        return None;
    }
    let lower = email.to_ascii_lowercase();
    if let Some(local) = lower.strip_suffix("@users.noreply.github.com") {
        // The id-prefixed form carries the login after the `+`.
        let username = local.rsplit('+').next().unwrap_or_default();
        if !username.is_empty() {
            return Some(format!("https://github.com/{username}.png?size=64").into());
        }
    }
    let digest = format!("{:x}", <md5::Md5 as md5::Digest>::digest(lower));
    Some(format!("https://www.gravatar.com/avatar/{digest}?s=64&d=identicon").into())
}

/// A commit body reads like Markdown source: single newlines are soft breaks
/// and collapse to spaces, while blank lines still separate paragraphs.
fn commit_body_text(body: &str) -> String {
    body.split("\n\n")
        .map(|paragraph| paragraph.lines().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The file-diff card's floating surface. The trailing spacer makes the
/// surface measure `panel - overlap` wider than the
/// card; right-aligning then parks the card's right edge just inside the
/// panel's left edge — the "barely overlaps" placement.
fn git_panel_hover_card(card: Stateful<Div>, panel_width: f32, window: &Window) -> AnyElement {
    let card_width = GIT_PANEL_DIFF_WIDTH
        .min((f32::from(window.viewport_size().width) - panel_width - 24.0).max(240.0));
    let spacer = (panel_width - GIT_PANEL_DIFF_OVERLAP).max(0.0);
    deferred(FloatingSurface::anchored_to_parent(
        div()
            .w(px(card_width + spacer))
            .h_auto()
            .flex()
            .child(card.w(px(card_width)).flex_none())
            .child(div().flex_1())
            .into_any_element(),
        MenuAlign::AboveRight,
        px(-GIT_PANEL_DIFF_OVERLAP),
        px(12.0),
    ))
    .into_any_element()
}

fn modal_button(
    id: &'static str,
    label: String,
    primary: bool,
    theme: Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_default()
        .text_size(sp(12.5))
        .font_weight(if primary {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(if primary {
            theme.text
        } else {
            theme.text_secondary
        })
        .bg(if primary {
            theme.overlay_strong
        } else {
            theme.overlay
        })
        .hover(|style| style.bg(theme.selection))
        .active(|style| style.opacity(0.8))
        .child(label)
        .on_click(on_click)
}
