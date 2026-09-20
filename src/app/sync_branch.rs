//! The `⌘S` "Sync branch…" modal: pick a tracked branch's checkout and pull
//! it up to its upstream.
//!
//! The chord still belongs to SaveFile — the save handler falls through to
//! `open_sync_branch` whenever no file-editor surface is active, so saving an
//! edited file keeps precedence. The list is the repository's own checkouts:
//! `ListRepoBranches` reports each local branch's upstream and the worktree
//! holding it, and only rows with both can take a `git pull`. The session's
//! checkout leads the list as "Current branch". Confirming runs
//! `PullUpstream` through the shared panel-operation slot — `pull --rebase`,
//! or `--no-rebase` when the merge setting is on — while the card stays up on
//! a pending row. A conflict hands off to the usual conflict modal, whose
//! Resolve in chat then opens a fresh chat rooted at the syncing folder.

use gpui::{KeyBinding, deferred};

use waku_client::RepoBranch;
use waku_client::git::PullStrategy;
use waku_client::workspace::{WorkspaceOperation, WorkspaceResult};

use super::command_palette::{
    Confirm, Dismiss, SelectFirst, SelectLast, SelectNext, SelectPageDown, SelectPageUp,
    SelectPrevious, next_selection_index,
};
use super::git_panel::GitPanelPending;
use super::*;

/// List navigation lives beneath the focused one-line input; escape lives on
/// the card so the field's own clear-on-escape outranks it while it has text.
const SEARCH_CONTEXT: &str = "SyncBranch > TextInput";
const CARD_CONTEXT: &str = "SyncBranch";

const SEARCH_ROW_HEIGHT: f32 = 52.0;
const RESULT_ROW_HEIGHT: f32 = 30.0;
const EMPTY_RESULTS_HEIGHT: f32 = 120.0;
const RESULTS_BOTTOM_PADDING: f32 = 8.0;
const MAX_CARD_HEIGHT: f32 = 400.0;
const PAGE_STEP: isize = 7;

/// Bind the picker's keys, mirroring the file finder's set.
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
        KeyBinding::new("escape", Dismiss, Some(CARD_CONTEXT)),
    ]);
}

/// One syncable checkout: a local branch with a tracking branch that is
/// checked out in a working tree — the folder `PullUpstream` runs in. A
/// tracked branch with no checkout is not listed: a pull has no working
/// tree to integrate into there.
pub(super) struct SyncBranchTarget {
    branch: String,
    upstream: String,
    cwd: PathBuf,
    /// The session checkout's own row — labeled "Current branch".
    current: bool,
}

/// What the body shows when it is not drawing rows.
#[derive(Clone, Debug, Eq, PartialEq)]
enum SyncBranchFetch {
    Loading,
    Ready,
    /// The daemon answered `None` — the folder is not a Git repository.
    Unavailable,
    Failed(String),
}

/// Cross-frame state for the picker. `results` indexes into `targets` and is
/// rebuilt on a keystroke or when the branch fetch lands — never in render.
pub(super) struct SyncBranchUi {
    search: Entity<TextInput>,
    open: bool,
    focus_generation: u64,
    previous_focus: Option<FocusHandle>,
    fetch_generation: u64,
    fetch: SyncBranchFetch,
    targets: Vec<SyncBranchTarget>,
    results: Vec<usize>,
    selected: usize,
    scroll: ScrollHandle,
    /// The panel operation this modal started — the card stays up on a
    /// pending row until `finish_git_panel_op` lands it.
    syncing: Option<(Uuid, String)>,
}

impl SyncBranchUi {
    pub(super) fn new(search: Entity<TextInput>) -> Self {
        Self {
            search,
            open: false,
            focus_generation: 0,
            previous_focus: None,
            fetch_generation: 0,
            fetch: SyncBranchFetch::Loading,
            targets: Vec::new(),
            results: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            syncing: None,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// The branch the pending row shows — `Some` while `op_id` is the
    /// operation this modal started. `finish_git_panel_op` takes it for the
    /// success toast.
    pub(super) fn take_syncing(&mut self, op_id: Uuid) -> Option<String> {
        self.syncing
            .take_if(|(id, _)| *id == op_id)
            .map(|(_, branch)| branch)
    }
}

/// The pickable rows: local branches that both track an upstream and are
/// checked out somewhere. The checkout containing `session_workspace` leads
/// as "Current branch"; every other syncable checkout follows.
fn sync_branch_targets(
    entries: &[RepoBranch],
    session_workspace: Option<&Path>,
) -> Vec<SyncBranchTarget> {
    let mut current = Vec::new();
    let mut rest = Vec::new();
    for entry in entries {
        if entry.remote.is_some() {
            continue;
        }
        let (Some(upstream), Some(checked_out_in)) =
            (entry.upstream.clone(), entry.checked_out_in.clone())
        else {
            continue;
        };
        let is_current = session_workspace.is_some_and(|workspace| {
            workspace == checked_out_in || workspace.starts_with(&checked_out_in)
        });
        let target = SyncBranchTarget {
            branch: entry.name.clone(),
            upstream,
            cwd: checked_out_in,
            current: is_current,
        };
        if is_current {
            current.push(target);
        } else {
            rest.push(target);
        }
    }
    current.extend(rest);
    current
}

fn sync_branch_results_height(result_count: usize, show_placeholder: bool) -> f32 {
    let content_height = if show_placeholder {
        EMPTY_RESULTS_HEIGHT
    } else {
        result_count as f32 * RESULT_ROW_HEIGHT
    };
    content_height + RESULTS_BOTTOM_PADDING
}

impl Waku {
    pub(super) fn sync_branch_action(
        &mut self,
        _: &SyncBranch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sync_branch.is_open() {
            self.close_sync_branch(cx);
        } else {
            self.open_sync_branch(window, cx);
        }
    }

    /// Open the picker over the session's repository — or the selected
    /// project's when no task is selected. The search field takes real focus;
    /// closing restores whatever held it before, on the next frame if the
    /// close came from a background completion.
    pub(super) fn open_sync_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_page.is_some() || self.sync_branch.open {
            return;
        }
        let session_workspace = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf);
        let Some(cwd) = session_workspace
            .clone()
            .or_else(|| self.selected_project().map(|project| project.path.clone()))
        else {
            return;
        };
        // One picker at a time; closing first restores the palette's recorded
        // focus so this picker captures the element that was really focused.
        if self.command_palette.is_open() {
            self.close_command_palette(window, cx);
        }
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
        self.sync_branch.previous_focus = if open_menus.is_empty() {
            window.focused(cx)
        } else if self.settings_page.is_some() {
            Some(self.settings_focus.clone())
        } else {
            Some(self.composer_focus(cx))
        };

        self.sync_branch.open = true;
        self.sync_branch.focus_generation = self.sync_branch.focus_generation.wrapping_add(1);
        self.sync_branch.selected = 0;
        self.sync_branch.scroll.scroll_to_item(0);
        self.sync_branch
            .search
            .update(cx, |input, cx| input.clear(cx));
        self.sync_branch.targets.clear();
        self.sync_branch.results.clear();
        self.sync_branch.fetch = SyncBranchFetch::Loading;
        self.sync_branch.syncing = None;

        // Closing an open GPUI menu can call its toggle observers back into
        // this entity, so release this action listener's mutable borrow first.
        if !open_menus.is_empty() {
            window.defer(cx, move |window, cx| {
                for menu in open_menus {
                    menu.close(window, cx);
                }
            });
        }

        // The picker is deferred onto GPUI's overlay plane. Wait for that
        // subtree to join the dispatch tree before handing focus to its input.
        let focus = self.sync_branch.search.read(cx).focus();
        let weak = cx.entity().downgrade();
        let focus_generation = self.sync_branch.focus_generation;
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let mut should_focus = false;
                let _ = weak.update(cx, |this, _| {
                    should_focus = this.sync_branch.open
                        && this.sync_branch.focus_generation == focus_generation;
                });
                if should_focus {
                    window.focus(&focus, cx);
                }
            });
        });

        let fetch_generation = self.sync_branch.fetch_generation.wrapping_add(1);
        self.sync_branch.fetch_generation = fetch_generation;
        let Some(client) = self.workspace_client_for_path(&cwd) else {
            self.sync_branch.fetch = SyncBranchFetch::Failed(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match client.request(WorkspaceOperation::ListRepoBranches { cwd }) {
                        Ok(WorkspaceResult::RepoBranches { entries }) => Ok(entries),
                        Ok(_) => Err(tr!("git_panel.unexpected_result")),
                        Err(error) => Err(error.to_string()),
                    }
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.sync_branch.fetch_generation != fetch_generation || !waku.sync_branch.open {
                    return;
                }
                match result {
                    Ok(Some(entries)) => {
                        waku.sync_branch.targets =
                            sync_branch_targets(&entries, session_workspace.as_deref());
                        waku.sync_branch.fetch = SyncBranchFetch::Ready;
                        waku.refresh_sync_branch_results(cx);
                    }
                    Ok(None) => waku.sync_branch.fetch = SyncBranchFetch::Unavailable,
                    Err(error) => waku.sync_branch.fetch = SyncBranchFetch::Failed(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Close the picker. Restoring the previous focus needs a window the
    /// background-completion paths do not have, so it is parked on
    /// `previous_focus` and `render_sync_branch` hands it back next frame.
    /// `syncing` survives the close — a dismissed card's operation still
    /// reports its outcome when it lands.
    pub(super) fn close_sync_branch(&mut self, cx: &mut Context<Self>) {
        if !self.sync_branch.open {
            return;
        }
        self.sync_branch.open = false;
        self.sync_branch.focus_generation = self.sync_branch.focus_generation.wrapping_add(1);
        cx.notify();
    }

    pub(super) fn refresh_sync_branch_localized_text(&mut self, cx: &mut Context<Self>) {
        self.sync_branch.search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.sync_branch"), cx);
            input.set_placeholder(tr!("input.search_branches"), cx)
        });
    }

    pub(super) fn sync_branch_query_edited(&mut self, cx: &mut Context<Self>) {
        if !self.sync_branch.open {
            return;
        }
        self.sync_branch.selected = 0;
        self.sync_branch.scroll.scroll_to_item(0);
        self.refresh_sync_branch_results(cx);
        cx.notify();
    }

    /// Recompute the drawn rows from the fetched targets — on a keystroke and
    /// when the fetch lands, never from `render`.
    fn refresh_sync_branch_results(&mut self, cx: &mut Context<Self>) {
        if !self.sync_branch.open {
            return;
        }
        let query = self
            .sync_branch
            .search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();
        self.sync_branch.results = self
            .sync_branch
            .targets
            .iter()
            .enumerate()
            .filter(|(_, target)| {
                query.is_empty()
                    || target.branch.to_lowercase().contains(&query)
                    || target.upstream.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();
        self.sync_branch.selected = self
            .sync_branch
            .selected
            .min(self.sync_branch.results.len().saturating_sub(1));
        cx.notify();
    }

    fn move_sync_branch_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.sync_branch.results.len();
        let Some(next) = next_selection_index(self.sync_branch.selected, len, delta) else {
            return;
        };
        self.sync_branch.selected = next;
        self.sync_branch.scroll.scroll_to_item(next);
        cx.notify();
    }

    fn set_sync_branch_selection(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.sync_branch.results.len() && self.sync_branch.selected != index {
            self.sync_branch.selected = index;
            cx.notify();
        }
    }

    /// Sync the highlighted row (`None`, the keyboard path) or a clicked one:
    /// `pull --rebase` in the branch's checkout — `git merge` when the
    /// setting says so. The card stays up on a pending row until the
    /// operation lands.
    fn execute_sync_branch(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if self.sync_branch.syncing.is_some() {
            return;
        }
        if self.git_panel_operation.is_some() {
            self.show_toast(tr!("sync_branch.busy"));
            cx.notify();
            return;
        }
        let index = index.unwrap_or_else(|| {
            self.sync_branch
                .selected
                .min(self.sync_branch.results.len().saturating_sub(1))
        });
        let Some(&target_index) = self.sync_branch.results.get(index) else {
            return;
        };
        let target = &self.sync_branch.targets[target_index];
        let workspace = target.cwd.clone();
        let branch = target.branch.clone();
        let strategy = if self.state.sync_with_merge {
            PullStrategy::Merge
        } else {
            PullStrategy::Rebase
        };
        let Some((op_id, workspace)) =
            self.begin_workspace_op(GitPanelPending::Syncing(strategy), workspace, cx)
        else {
            return;
        };
        if let Some(operation) = self.git_panel_operation.as_mut() {
            operation.sync_branch = true;
        }
        let Some(client) = self.workspace_client_for_path(&workspace) else {
            self.finish_git_panel_op(
                op_id,
                Err(anyhow::anyhow!(tr!("errors.daemon_disconnected"))),
                cx,
            );
            return;
        };
        self.sync_branch.syncing = Some((op_id, branch));
        cx.notify();
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

    pub(super) fn render_sync_branch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.sync_branch.open {
            // A background close — a finished sync — still owes the previous
            // focus its restoration; interactive closes take the same path.
            if let Some(previous_focus) = self.sync_branch.previous_focus.take() {
                window.focus(&previous_focus, cx);
            }
            return None;
        }
        let theme = Theme::current(cx);
        let viewport_height = f32::from(window.viewport_size().height);
        let top = (viewport_height * 0.09).clamp(48.0, 72.0);
        let card_max_height = (viewport_height - top - 36.0)
            .max(SEARCH_ROW_HEIGHT)
            .min(MAX_CARD_HEIGHT);
        let selected = self
            .sync_branch
            .selected
            .min(self.sync_branch.results.len().saturating_sub(1));

        let mut body = div()
            .id("sync-branch-results")
            .flex_none()
            .overflow_y_scroll()
            .track_scroll(&self.sync_branch.scroll)
            .px(px(8.0))
            .pb(px(8.0));

        if let Some((_, branch)) = &self.sync_branch.syncing {
            let syncing_label = tr!("sync_branch.syncing", branch = branch.clone());
            body = body
                .h(px(RESULT_ROW_HEIGHT + RESULTS_BOTTOM_PADDING))
                .child(
                    div()
                        .h(px(RESULT_ROW_HEIGHT))
                        .px(px(11.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(motion::spin(icon(
                            "icons/loader-circle.svg",
                            14.0,
                            theme.text_secondary,
                        )))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(sp(13.0))
                                .text_color(theme.text_secondary)
                                .child(syncing_label),
                        ),
                );
        } else {
            let show_placeholder = self.sync_branch.results.is_empty();
            let results_height =
                sync_branch_results_height(self.sync_branch.results.len(), show_placeholder)
                    .min((card_max_height - SEARCH_ROW_HEIGHT).max(0.0));
            body = body.h(px(results_height));
            if show_placeholder {
                let (icon_path, title, hint, spinning) = match &self.sync_branch.fetch {
                    SyncBranchFetch::Loading => (
                        "icons/loader-circle.svg",
                        tr!("sync_branch.loading"),
                        None,
                        true,
                    ),
                    SyncBranchFetch::Unavailable => (
                        "icons/git-branch.svg",
                        tr!("git_panel.not_a_repository"),
                        None,
                        false,
                    ),
                    SyncBranchFetch::Failed(error) => {
                        ("icons/git-branch.svg", error.clone(), None, false)
                    }
                    SyncBranchFetch::Ready => (
                        "icons/git-branch.svg",
                        tr!("sync_branch.no_results"),
                        Some(tr!("sync_branch.no_results_hint")),
                        false,
                    ),
                };
                let empty_icon = icon(icon_path, 18.0, theme.text_ghost);
                body = body.child(
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
                                .mt(px(10.0))
                                .max_w(px(360.0))
                                .text_size(sp(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_secondary)
                                .child(title),
                        )
                        .when_some(hint, |empty, hint| {
                            empty.child(
                                div()
                                    .mt(px(4.0))
                                    .max_w(px(360.0))
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .child(hint),
                            )
                        }),
                );
            } else {
                for (index, &target_index) in self.sync_branch.results.iter().enumerate() {
                    let target = &self.sync_branch.targets[target_index];
                    body = body.child(self.render_sync_branch_row(
                        index,
                        target,
                        index == selected,
                        &theme,
                        cx,
                    ));
                }
            }
        }

        let card = div()
            .id("sync-branch-card")
            .key_context(CARD_CONTEXT)
            .on_action(cx.listener(Self::sync_branch_action))
            .on_action(
                cx.listener(|this, _: &SelectNext, _, cx| this.move_sync_branch_selection(1, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                this.move_sync_branch_selection(-1, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                this.move_sync_branch_selection(isize::MIN, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                this.move_sync_branch_selection(isize::MAX, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                this.move_sync_branch_selection(PAGE_STEP, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                this.move_sync_branch_selection(-PAGE_STEP, cx)
            }))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.execute_sync_branch(None, cx)))
            .on_action(cx.listener(|this, _: &Dismiss, _, cx| this.close_sync_branch(cx)))
            .w_full()
            .max_w(px(520.0))
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
                    .px(px(17.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .border_b(hairline())
                    .border_color(theme.separator)
                    .text_size(sp(14.5))
                    .text_color(theme.text)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(self.sync_branch.search.clone()),
                    ),
            )
            .child(body);

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.26)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.14)
        };
        let layer = div()
            .id("sync-branch-layer")
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
                cx.listener(|this, _, _, cx| this.close_sync_branch(cx)),
            )
            .child(motion::modal_enter("sync-branch-card-enter", card));
        Some(
            deferred(motion::fade_in("sync-branch-layer-enter", layer))
                .with_priority(3)
                .into_any_element(),
        )
    }

    /// A branch row: the label — "Current branch" for the session's own
    /// checkout — then its branch name or tracking ref dimmed on the right.
    fn render_sync_branch_row(
        &self,
        index: usize,
        target: &SyncBranchTarget,
        highlighted: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = if target.current {
            tr!("sync_branch.current")
        } else {
            target.branch.clone()
        };
        let detail = if target.current {
            target.branch.clone()
        } else {
            target.upstream.clone()
        };
        div()
            .id(SharedString::from(format!("sync-branch-row-{index}")))
            .h(px(RESULT_ROW_HEIGHT))
            .px(px(11.0))
            .rounded(px(10.0))
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
                    this.set_sync_branch_selection(index, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.execute_sync_branch(Some(index), cx);
                cx.stop_propagation();
            }))
            .child(icon("icons/git-branch.svg", 13.0, theme.text_secondary))
            .child(
                div()
                    .flex_none()
                    .max_w(px(240.0))
                    .truncate()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .text_color(theme.text_ghost)
                    .child(detail),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str, upstream: Option<&str>, checked_out_in: Option<&str>) -> RepoBranch {
        RepoBranch {
            name: name.to_owned(),
            remote: None,
            sha: "a".repeat(40),
            upstream: upstream.map(str::to_owned),
            ahead: None,
            behind: None,
            last_commit_at: None,
            last_commit_subject: None,
            checked_out_in: checked_out_in.map(PathBuf::from),
        }
    }

    /// Only local branches that both track an upstream and sit in a checkout
    /// are pickable — remote refs, untracked branches, and branches with no
    /// working tree have nothing a `git pull` could integrate into.
    #[test]
    fn targets_keep_only_checked_out_tracked_local_branches() {
        let mut remote = branch("origin/main", Some("origin/main"), None);
        remote.remote = Some("origin".to_owned());
        let entries = vec![
            remote,
            branch("untracked", None, Some("/repo")),
            branch("tracked-no-checkout", Some("origin/topic"), None),
            branch("main", Some("origin/main"), Some("/repo")),
            branch("worktree-branch", Some("origin/topic"), Some("/repo-wt")),
        ];
        let targets = sync_branch_targets(&entries, None);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].branch, "main");
        assert_eq!(targets[0].cwd, PathBuf::from("/repo"));
        assert_eq!(targets[1].branch, "worktree-branch");
    }

    /// The session's own checkout leads as the "Current branch" row, matching
    /// by the worktree path itself or a session rooted inside it.
    #[test]
    fn session_checkout_leads_as_current() {
        let entries = vec![
            branch("other", Some("origin/other"), Some("/other")),
            branch("main", Some("origin/main"), Some("/repo")),
        ];
        let targets = sync_branch_targets(&entries, Some(Path::new("/repo/sub/dir")));
        assert_eq!(targets.len(), 2);
        assert!(targets[0].current);
        assert_eq!(targets[0].branch, "main");
        assert!(!targets[1].current);
    }
}
