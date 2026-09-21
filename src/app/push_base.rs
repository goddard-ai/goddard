//! Push-after-land: the transcript's landed notice offers to push the base
//! branch to its upstream, and this module owns that affordance end to end —
//! the cached upstream/ahead read the button renders from, the workspace
//! op, the ⌘⇧↩ chord, and the failure modal.
//!
//! The push state is a property of the workspace's base branch, not the
//! notice: every landed card on that base renders the same live answer, so
//! a card whose commits are already pushed shows "Pushed" rather than a
//! stale button.

use gpui::{KeyBinding, actions};

use waku_client::git::{BasePushState, PullOutcome, PullStrategy, PushBaseOutcome};
use waku_client::workspace::{WorkspaceOperation, WorkspaceResult};

use super::git_panel::{GitPanelOperation, GitPanelPending, SyncConflict};
use super::*;

actions!(
    waku_push_base_dialog,
    [ConfirmPushBaseDialog, DismissPushBaseDialog]
);

const DIALOG_CONTEXT: &str = "PushBaseDialog";

/// How often one (workspace, base) pair may auto-fetch its upstream —
/// enough to catch a colleague's merge while the draft sits open, bounded
/// so a visible strip is not a fetch loop.
const UPSTREAM_FETCH_INTERVAL: Duration = Duration::from_secs(60);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmPushBaseDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissPushBaseDialog, Some(DIALOG_CONTEXT)),
    ]);
}

/// What a landed notice's push affordance renders as — derived per frame
/// from the cached base-push read and the in-flight op, never stored.
#[derive(Clone, Debug)]
pub(super) enum LandedPush {
    /// No upstream, or the read has not landed (or failed) — nothing shown.
    Hidden,
    /// The base is (or may be) ahead of its upstream; the button pushes it.
    Pushable {
        workspace: PathBuf,
        base: String,
        upstream: String,
    },
    /// A base push is in flight for this workspace.
    Pushing,
    /// The upstream already holds every commit on the base.
    Pushed,
}

/// The failure modal's payload. `kind` picks the primary action: a
/// non-fast-forward rejection offers "Sync & retry push", a plain push
/// failure a bare retry, and a failed recovery pull retries the sync.
pub(super) struct PushBaseFailure {
    pub workspace: PathBuf,
    pub base: String,
    pub upstream: Option<String>,
    /// Git's own output, shown in the modal's detail block.
    pub detail: String,
    pub kind: PushBaseFailureKind,
    primary_focus: FocusHandle,
    copy_focus: FocusHandle,
    dismiss_focus: FocusHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PushBaseFailureKind {
    /// The remote refused a non-fast-forward update — sync, then push.
    Rejected,
    /// The push itself failed (auth, hook, network).
    Push,
    /// The "Sync & retry" pull failed.
    Sync,
}

impl Waku {
    /// The push affordance for a landed notice on `base` in `workspace`. A
    /// cache miss starts the daemon read and draws `Hidden` until it
    /// lands — the notice degrades to the plain card.
    pub(super) fn landed_push_state(
        &self,
        workspace: &Path,
        base: &str,
        cx: &mut Context<Self>,
    ) -> LandedPush {
        if self.git_panel_operation.as_ref().is_some_and(|op| {
            op.pending == GitPanelPending::PushingBase && op.workspace == workspace
        }) {
            return LandedPush::Pushing;
        }
        match self.base_push_state_for(workspace, base, cx) {
            Some(BasePushState {
                upstream: Some(upstream),
                ahead,
                ..
            }) => {
                if ahead == Some(0) {
                    LandedPush::Pushed
                } else {
                    LandedPush::Pushable {
                        workspace: workspace.to_path_buf(),
                        base: base.to_owned(),
                        upstream,
                    }
                }
            }
            _ => LandedPush::Hidden,
        }
    }

    /// The cached `BasePushState` read for `base` in `workspace`, shared by
    /// the landed notice's push button and the draft's sync strip. A miss
    /// starts the daemon read and reports `None` until it lands — callers
    /// degrade to showing nothing.
    pub(super) fn base_push_state_for(
        &self,
        workspace: &Path,
        base: &str,
        cx: &mut Context<Self>,
    ) -> Option<BasePushState> {
        // Every reader is a surface that advises sync or push — refresh the
        // tracking ref on a TTL so the advice describes the remote as it is
        // now, not as of the last manual fetch.
        self.maybe_fetch_upstream(workspace, base, cx);
        let key = (workspace.to_path_buf(), base.to_owned());
        // Bind before matching: the scrutinee's RefMut would otherwise live
        // through the arms, and `Missing` re-borrows the cache to abandon.
        let query = self.base_push_states.borrow_mut().read(&key);
        match query {
            Query::Ready(result) => result.as_ref().clone().ok(),
            Query::Pending => None,
            Query::Missing(token) => {
                let Some(client) = self.workspace_client_for_path(workspace) else {
                    // Offline remote owner: abandon so the next read retries
                    // once the host reconnects instead of pending forever.
                    self.base_push_states.borrow_mut().abandon(token);
                    return None;
                };
                let fetch_workspace = workspace.to_path_buf();
                let fetch_base = base.to_owned();
                cx.spawn(async move |waku, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            match client.request(WorkspaceOperation::BasePushState {
                                cwd: fetch_workspace,
                                base: fetch_base,
                            }) {
                                Ok(WorkspaceResult::BasePushState { state }) => Ok(state),
                                Ok(_) => {
                                    Err("the daemon returned an invalid push state".to_owned())
                                }
                                Err(error) => Err(error.to_string()),
                            }
                        })
                        .await;
                    let _ = waku.update(cx, |waku, cx| {
                        if waku.base_push_states.borrow_mut().fulfill(token, result) {
                            cx.notify();
                        }
                    });
                })
                .detach();
                None
            }
        }
    }

    /// The push affordance for a landed notice's `base`, resolved against
    /// the selected session's workspace.
    pub(super) fn landed_push_for_base(&self, base: &str, cx: &mut Context<Self>) -> LandedPush {
        self.selected_session()
            .and_then(|session| {
                self.workspace_path_for_session(session)
                    .map(Path::to_path_buf)
            })
            .map(|workspace| self.landed_push_state(&workspace, base, cx))
            .unwrap_or(LandedPush::Hidden)
    }

    /// Auto-fetch `base`'s upstream at most once per
    /// `UPSTREAM_FETCH_INTERVAL`, so divergence counts describe the remote
    /// as it is now rather than the last manual fetch. The attempt's
    /// timestamp is marked up front — a failed fetch (offline, auth) just
    /// keeps the last-known numbers and is not retried every frame.
    pub(super) fn maybe_fetch_upstream(
        &self,
        workspace: &Path,
        base: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.state.auto_fetch_remotes {
            return;
        }
        {
            let key = (workspace.to_path_buf(), base.to_owned());
            let mut times = self.upstream_fetch_times.borrow_mut();
            if times
                .get(&key)
                .is_some_and(|at| at.elapsed() < UPSTREAM_FETCH_INTERVAL)
            {
                return;
            }
            // Aged-out entries are dropped as each insert runs, keeping the
            // map sized to what is actually being asked about.
            times.retain(|_, at| at.elapsed() < UPSTREAM_FETCH_INTERVAL);
            times.insert(key, Instant::now());
        }
        let Some(client) = self.workspace_client_for_path(workspace) else {
            return;
        };
        let fetch_workspace = workspace.to_path_buf();
        let fetch_base = base.to_owned();
        let workspace = workspace.to_path_buf();
        cx.spawn(async move |waku, cx| {
            let fetched = cx
                .background_executor()
                .spawn(async move {
                    matches!(
                        client.request(WorkspaceOperation::FetchUpstream {
                            cwd: fetch_workspace,
                            base: fetch_base,
                        }),
                        Ok(WorkspaceResult::Bool { value: true })
                    )
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                if !fetched {
                    return;
                }
                // Tracking refs moved — every divergence read derived from
                // them is stale.
                waku.branch_snapshots.invalidate(&workspace);
                waku.invalidate_base_push_state(&workspace);
                if waku
                    .git_panel
                    .as_ref()
                    .is_some_and(|panel| panel.workspace == workspace)
                {
                    waku.refresh_git_panel(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Drop the cached push read for a workspace — land, push, sync, and
    /// base changes all move the answer.
    pub(super) fn invalidate_base_push_state(&mut self, workspace: &Path) {
        self.base_push_states
            .borrow_mut()
            .invalidate_where(|(path, _)| path == workspace);
    }

    /// Push the landed notice's base to its upstream. Shares the git-op
    /// slot with the panel — a click while one runs is a no-op — and raises
    /// the spinner toast the outcome settles.
    pub(super) fn start_push_base(
        &mut self,
        workspace: PathBuf,
        base: String,
        cx: &mut Context<Self>,
    ) {
        let Some((op_id, workspace)) =
            self.begin_workspace_op(GitPanelPending::PushingBase, workspace, cx)
        else {
            return;
        };
        let toast_id = self.show_progress_toast(tr!("commit.pushing"), PROGRESS_TOAST_DURATION);
        if let Some(op) = self.git_panel_operation.as_mut() {
            op.base = Some(base.clone());
            op.toast_id = Some(toast_id);
        }
        let Some(client) = self.workspace_client_for_path(&workspace) else {
            self.finish_git_panel_op(
                op_id,
                Err(anyhow::anyhow!(tr!("errors.daemon_disconnected"))),
                cx,
            );
            return;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(WorkspaceOperation::PushBase {
                        cwd: workspace,
                        base,
                    })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_git_panel_op(op_id, result, cx);
            });
        })
        .detach();
    }

    /// The failure modal's "Sync & retry push": pull the base's upstream
    /// inside its owning checkout, then — on a clean pull — re-fire the
    /// push. `push_base_retry` is the memory of that second step.
    fn start_sync_base(&mut self, workspace: PathBuf, base: String, cx: &mut Context<Self>) {
        let Some((op_id, workspace)) =
            self.begin_workspace_op(GitPanelPending::SyncingBase, workspace, cx)
        else {
            return;
        };
        let toast_id =
            self.show_progress_toast(tr!("git_panel.syncing_rebase"), PROGRESS_TOAST_DURATION);
        if let Some(op) = self.git_panel_operation.as_mut() {
            op.base = Some(base.clone());
            op.toast_id = Some(toast_id);
        }
        self.push_base_retry = Some((workspace.clone(), base.clone()));
        let strategy = if self.state.sync_with_merge {
            PullStrategy::Merge
        } else {
            PullStrategy::Rebase
        };
        let Some(client) = self.workspace_client_for_path(&workspace) else {
            self.finish_git_panel_op(
                op_id,
                Err(anyhow::anyhow!(tr!("errors.daemon_disconnected"))),
                cx,
            );
            return;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(WorkspaceOperation::SyncBase {
                        cwd: workspace,
                        base,
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

    /// A `PushBase` op resolved. `Rejected` opens the failure modal — the
    /// remote moved and the base must sync first — everything else settles
    /// the progress toast.
    pub(super) fn finish_push_base(
        &mut self,
        op: &GitPanelOperation,
        outcome: PushBaseOutcome,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            PushBaseOutcome::Pushed { base, upstream } => {
                self.settle_operation_toast(
                    op.toast_id,
                    tr!("push_base.pushed", base = base, upstream = upstream),
                    ToastTone::Success,
                );
            }
            PushBaseOutcome::UpToDate { base, upstream } => {
                self.settle_operation_toast(
                    op.toast_id,
                    tr!("push_base.up_to_date", base = base, upstream = upstream),
                    ToastTone::Notice,
                );
            }
            PushBaseOutcome::NoUpstream { base } => {
                self.settle_operation_toast(
                    op.toast_id,
                    tr!("push_base.no_upstream", base = base),
                    ToastTone::Notice,
                );
            }
            PushBaseOutcome::Rejected {
                base,
                upstream,
                message,
            } => {
                self.dismiss_operation_toast(op.toast_id);
                self.open_push_base_failure(
                    op.workspace.clone(),
                    base,
                    Some(upstream),
                    message,
                    PushBaseFailureKind::Rejected,
                    cx,
                );
            }
        }
        self.invalidate_base_push_state(&op.workspace);
        self.invalidate_workspace_queries(cx);
        self.refresh_git_panel(cx);
        self.refresh_git_panel_commits(cx);
    }

    /// A `SyncBase` op resolved. `Clean` re-fires the push the modal was
    /// recovering; `Conflict` hands off to the sync-conflict modal rooted
    /// at the checkout that owns the base — its `workspace` drives the
    /// modal's resolve/abort/merge actions, so it must be that checkout,
    /// not the session worktree the request came from.
    pub(super) fn finish_sync_base(
        &mut self,
        op: &GitPanelOperation,
        checkout: PathBuf,
        outcome: PullOutcome,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            PullOutcome::Clean => {
                self.invalidate_base_push_state(&op.workspace);
                self.invalidate_workspace_queries(cx);
                self.refresh_git_panel(cx);
                self.refresh_git_panel_commits(cx);
                if let Some((workspace, base)) = self.push_base_retry.take() {
                    self.start_push_base(workspace, base, cx);
                } else {
                    self.settle_operation_toast(
                        op.toast_id,
                        tr!("push_base.synced"),
                        ToastTone::Success,
                    );
                }
            }
            PullOutcome::Conflict { in_progress, files } => {
                self.push_base_retry = None;
                self.dismiss_operation_toast(op.toast_id);
                self.git_panel_conflict_files_scroll
                    .set_offset(gpui::Point::default());
                let conflict = SyncConflict::Pull {
                    in_progress,
                    workspace: checkout,
                    files,
                    new_chat: false,
                };
                if self.state.auto_resolve_in_chat {
                    self.auto_resolve_sync_conflict(conflict, cx);
                } else {
                    self.git_panel_sync_conflict = Some(conflict);
                }
                self.invalidate_workspace_queries(cx);
                cx.notify();
            }
        }
    }

    /// ⌘⇧↩ — push the selected session's base branch to its upstream: the
    /// landed notice's button without the card. The base is the newest
    /// landed notice's, falling back to the session's recorded base.
    pub(super) fn push_base_branch_action(
        &mut self,
        _: &PushBaseBranch,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.selected_session().and_then(|session| {
            let workspace = self
                .workspace_path_for_session(session)
                .map(Path::to_path_buf)?;
            let base = session
                .messages
                .iter()
                .rev()
                .find_map(|message| match &message.notice {
                    Some(TranscriptNotice::Landed { base, .. }) => Some(base.clone()),
                    _ => None,
                })
                .or_else(|| match &session.workspace {
                    SessionWorkspace::Worktree { base_branch, .. } => base_branch.clone(),
                    _ => None,
                })?;
            Some((workspace, base))
        });
        match target {
            Some((workspace, base)) => self.start_push_base(workspace, base, cx),
            None => {
                self.show_toast(tr!("push_base.no_base"));
                cx.notify();
            }
        }
    }

    pub(super) fn open_push_base_failure(
        &mut self,
        workspace: PathBuf,
        base: String,
        upstream: Option<String>,
        detail: String,
        kind: PushBaseFailureKind,
        cx: &mut Context<Self>,
    ) {
        self.push_base_failure = Some(PushBaseFailure {
            workspace,
            base,
            upstream,
            detail,
            kind,
            primary_focus: cx.focus_handle(),
            copy_focus: cx.focus_handle(),
            dismiss_focus: cx.focus_handle(),
        });
        cx.notify();
    }

    /// The modal's primary action: sync-then-push for a rejection or a
    /// failed sync, a bare retry for a failed push.
    fn confirm_push_base_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(failure) = self.push_base_failure.take() else {
            return;
        };
        match failure.kind {
            PushBaseFailureKind::Rejected | PushBaseFailureKind::Sync => {
                self.start_sync_base(failure.workspace, failure.base, cx);
            }
            PushBaseFailureKind::Push => {
                self.start_push_base(failure.workspace, failure.base, cx);
            }
        }
    }

    fn dismiss_push_base_dialog(&mut self, cx: &mut Context<Self>) {
        self.push_base_failure = None;
        cx.notify();
    }

    fn copy_push_base_error(&mut self, cx: &mut Context<Self>) {
        if let Some(failure) = &self.push_base_failure {
            cx.write_to_clipboard(ClipboardItem::new_string(failure.detail.clone()));
            self.show_success_toast(tr!("common.copied"));
            cx.notify();
        }
    }

    pub(super) fn render_push_base_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let failure = self.push_base_failure.as_ref()?;
        // An open modal holds focus so Enter/Escape reach its context no
        // matter where the pointer was last — the same dance the panel
        // modals do.
        if !self.push_base_modal_focus.contains_focused(window, cx) {
            window.focus(&self.push_base_modal_focus, cx);
        }
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();

        let (primary_icon, primary_label) = match failure.kind {
            PushBaseFailureKind::Rejected | PushBaseFailureKind::Sync => {
                ("icons/rotate-cw.svg", tr!("push_base.sync_and_retry"))
            }
            PushBaseFailureKind::Push => ("icons/cloud-upload.svg", tr!("push_base.retry_push")),
        };
        let description = match failure.kind {
            PushBaseFailureKind::Rejected => tr!(
                "push_base.rejected_description",
                base = failure.base.clone(),
                upstream = failure.upstream.clone().unwrap_or_default()
            ),
            PushBaseFailureKind::Push => tr!("push_base.failed_description"),
            PushBaseFailureKind::Sync => {
                tr!(
                    "push_base.sync_failed_description",
                    base = failure.base.clone()
                )
            }
        };

        let primary_row = push_base_dialog_row(
            "push-base-dialog-primary",
            &failure.primary_focus,
            primary_icon,
            primary_label,
            weak.clone(),
            &theme,
            |waku, _window, cx| waku.confirm_push_base_dialog(cx),
        );
        let copy_row = push_base_dialog_row(
            "push-base-dialog-copy",
            &failure.copy_focus,
            "icons/copy.svg",
            tr!("push_base.copy_error"),
            weak.clone(),
            &theme,
            |waku, _window, cx| waku.copy_push_base_error(cx),
        );
        let dismiss_row = push_base_dialog_row(
            "push-base-dialog-dismiss",
            &failure.dismiss_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, _window, cx| waku.dismiss_push_base_dialog(cx),
        );

        let card = div()
            .id("push-base-dialog-card")
            .track_focus(&self.push_base_modal_focus)
            .tab_index(0)
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmPushBaseDialog, _, cx| {
                waku.confirm_push_base_dialog(cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissPushBaseDialog, _, cx| {
                waku.dismiss_push_base_dialog(cx)
            }))
            .tab_group()
            .w(px(440.0))
            .overflow_hidden()
            .rounded(px(16.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_xl()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
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
                            .child(tr!("push_base.failed_title", base = failure.base.clone())),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(description),
                    )
                    .child(
                        div()
                            .max_h(px(120.0))
                            .overflow_hidden()
                            .rounded(px(8.0))
                            .border(hairline())
                            .border_color(theme.border_subtle)
                            .bg(theme.overlay)
                            .px(px(10.0))
                            .py(px(8.0))
                            .text_size(sp(11.5))
                            .line_height(sp(16.0))
                            .font_family(crate::fonts::current(cx).code)
                            .text_color(theme.text_secondary)
                            .child(failure.detail.clone()),
                    ),
            )
            .child(
                div()
                    .px(px(16.0))
                    .pb(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(primary_row)
                    .child(copy_row)
                    .child(dismiss_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("push-base-dialog-layer")
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
                cx.listener(|waku, _, _, cx| waku.dismiss_push_base_dialog(cx)),
            )
            .child(motion::modal_enter("push-base-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("push-base-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

/// One actionable row in the push-failure dialog — the provider-switch
/// dialog's row shape.
fn push_base_dialog_row(
    id: &'static str,
    focus: &FocusHandle,
    icon_path: &'static str,
    label: String,
    weak: WeakEntity<Waku>,
    theme: &Theme,
    action: fn(&mut Waku, &mut Window, &mut Context<Waku>),
) -> Stateful<Div> {
    let click_weak = weak.clone();
    let key_weak = weak;
    div()
        .id(id)
        .track_focus(focus)
        .tab_index(0)
        .h(px(34.0))
        .w_full()
        .px(px(10.0))
        .rounded(px(10.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_default()
        .text_size(sp(13.0))
        .text_color(theme.text)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .hover(|style| style.bg(theme.overlay_strong))
        .child(icon(icon_path, 13.0, theme.text_secondary))
        .child(div().min_w_0().flex_1().truncate().child(label))
        .on_click(move |_, window, cx| {
            let _ = click_weak.update(cx, |waku, cx| action(waku, window, cx));
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if !event.keystroke.modifiers.modified()
                && matches!(event.keystroke.key.as_str(), "enter" | "space")
            {
                let _ = key_weak.update(cx, |waku, cx| action(waku, window, cx));
                cx.stop_propagation();
            }
        })
}
