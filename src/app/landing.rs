//! Task-owned landing projections. RPCs and repository discovery run on the
//! background executor; render reads only these prepared values.
use super::*;
use waku_client::landing::{
    LandingJob, LandingKind, LandingQueueSnapshot, LandingResult, LandingState,
};
use waku_client::workspace::{WorkspaceOperation, WorkspaceResult};

pub(super) struct NativeLanding {
    pub task: Option<Uuid>,
    pub initiated_here: bool,
    pub cancel_focus: FocusHandle,
    pub operation: git_panel::GitPanelOperation,
    pub job: Option<LandingJob>,
    pub unavailable: bool,
}

impl Waku {
    pub(super) fn start_queued_landing(
        &mut self,
        workspace: PathBuf,
        base: Option<String>,
        strategy: waku_client::git::PullStrategy,
        progress_toast: bool,
        cx: &mut Context<Self>,
    ) {
        if self.git_panel_operation.is_some()
            || self
                .landing_operations
                .values()
                .any(|entry| entry.operation.workspace == workspace)
        {
            return;
        }
        let id = Uuid::new_v4();
        let toast_id = progress_toast.then(|| {
            self.show_progress_toast(tr!("git_panel.land_requesting"), PROGRESS_TOAST_DURATION)
        });
        let operation = git_panel::GitPanelOperation {
            id,
            workspace: workspace.clone(),
            pending: git_panel::GitPanelPending::Landing,
            toast_id,
            sync_branch: false,
            sync_branch_name: None,
            base: base.clone(),
            archive_follow_up: None,
        };
        let task = if progress_toast {
            self.composer_session_id()
        } else {
            self.journal_session_for_workspace(&workspace)
        };
        self.landing_operations.insert(
            id,
            NativeLanding {
                initiated_here: true,
                task,
                cancel_focus: cx.focus_handle(),
                operation,
                job: None,
                unavailable: false,
            },
        );
        let task = task.unwrap_or_default();
        self.monitor_landing(id, workspace, Some((task, base, strategy)), cx);
        cx.notify();
    }

    fn monitor_landing(
        &mut self,
        id: Uuid,
        workspace: PathBuf,
        start: Option<(Uuid, Option<String>, waku_client::git::PullStrategy)>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let mut start = start;
            loop {
                let Some(client) = this
                    .update(cx, |this, _| this.workspace_client_for_path(&workspace))
                    .ok()
                    .flatten()
                else {
                    let _ = this.update(cx, |this, cx| this.mark_landing_unavailable(id, cx));
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    continue;
                };
                let admission = start.take(); // Never replay admission after an ambiguous response.
                let cwd = workspace.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        if let Some((task, base, strategy)) = admission {
                            client.request_for_session(
                                task,
                                WorkspaceOperation::StartLanding {
                                    id,
                                    cwd,
                                    base,
                                    strategy,
                                },
                            )?;
                        }
                        match client.request(WorkspaceOperation::LandingJob { id })? {
                            WorkspaceResult::LandingJob { job: Some(job) } => {
                                match client.request(WorkspaceOperation::LandingSnapshot {
                                    queue_id: job.queue_id,
                                })? {
                                    WorkspaceResult::LandingQueue { snapshot } => {
                                        let job = if job.state != LandingState::Finished
                                            && snapshot.jobs.iter().any(|entry| {
                                                entry.id == job.id
                                                    && entry.state == LandingState::Finished
                                            }) {
                                            match client
                                                .request(WorkspaceOperation::LandingJob { id })?
                                            {
                                                WorkspaceResult::LandingJob { job: Some(job) } => {
                                                    job
                                                }
                                                _ => anyhow::bail!("landing result expired"),
                                            }
                                        } else {
                                            job
                                        };
                                        Ok(Some((job, snapshot)))
                                    }
                                    _ => anyhow::bail!("unexpected landing snapshot"),
                                }
                            }
                            WorkspaceResult::LandingJob { job: None } => Ok(None),
                            _ => anyhow::bail!("unexpected landing response"),
                        }
                    })
                    .await;
                let terminal = this
                    .update(cx, |this, cx| match result {
                        Ok(Some((job, snapshot))) => this.apply_landing_job(id, job, snapshot, cx),
                        Ok(None) => {
                            this.finish_git_panel_op(
                                id,
                                Err(anyhow::anyhow!(tr!("git_panel.land_result_unknown"))),
                                cx,
                            );
                            true
                        }
                        Err(_) => {
                            this.mark_landing_unavailable(id, cx);
                            false
                        }
                    })
                    .unwrap_or(true);
                if terminal {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
            }
        })
        .detach();
    }

    fn mark_landing_unavailable(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(entry) = self.landing_operations.get_mut(&id) {
            entry.unavailable = true;
        }
        self.update_landing_toast(id);
        cx.notify();
    }
    fn apply_landing_job(
        &mut self,
        id: Uuid,
        mut job: LandingJob,
        snapshot: LandingQueueSnapshot,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(current) = snapshot.jobs.iter().find(|current| current.id == job.id) {
            if current.state != LandingState::Finished || current.result.is_some() {
                job = current.clone();
            }
        }
        let workspace = match self.landing_operations.get(&id) {
            Some(entry) => entry.operation.workspace.clone(),
            None => return true,
        };
        self.apply_landing_snapshot(workspace, snapshot, cx);
        if let Some(entry) = self.landing_operations.get_mut(&id) {
            entry.job = Some(job.clone());
            entry.unavailable = false;
        }
        if job.state != LandingState::Queued {
            self.landing_cancellations.remove(&id);
        }
        self.update_landing_toast(id);
        match job.state {
            LandingState::Canceled => {
                if let Some(entry) = self.landing_operations.remove(&id) {
                    self.clear_landing_toast_action(entry.operation.toast_id);
                    self.settle_operation_toast(
                        entry.operation.toast_id,
                        tr!("git_panel.land_canceled"),
                        ToastTone::Notice,
                    );
                }
                cx.notify();
                true
            }
            LandingState::Finished => {
                // Observers and duplicate callers may display the outcome,
                // but only the originating operation writes transcript notices
                // and action history. Reattachment must not duplicate them.
                let originator = self
                    .landing_operations
                    .get(&id)
                    .is_some_and(|entry| entry.initiated_here && job.id == id);
                if !originator {
                    if let Some(entry) = self.landing_operations.remove(&id) {
                        self.clear_landing_toast_action(entry.operation.toast_id);
                        let (message, tone) = match &job.result {
                            Some(LandingResult::Failed { message }) => {
                                (message.clone(), ToastTone::Failure)
                            }
                            Some(LandingResult::Land {
                                outcome: waku_client::git::LandOutcome::Conflict { base, .. },
                            }) => (
                                tr!("git_panel.land_conflict_description", base = base),
                                ToastTone::Notice,
                            ),
                            _ => (landing_job_label(&job), ToastTone::Success),
                        };
                        if entry.operation.toast_id.is_some() {
                            self.settle_operation_toast(entry.operation.toast_id, message, tone);
                        }
                    }
                    self.invalidate_workspace_queries(cx);
                    self.refresh_git_panel(cx);
                    self.refresh_git_panel_commits(cx);
                    cx.notify();
                    return true;
                }

                self.clear_landing_toast_action(
                    self.landing_operations
                        .get(&id)
                        .and_then(|entry| entry.operation.toast_id),
                );
                let result = match job.result {
                    Some(LandingResult::Land { outcome }) => Ok(WorkspaceResult::Land { outcome }),
                    Some(LandingResult::Failed { message }) => Err(anyhow::anyhow!(message)),
                    _ => Err(anyhow::anyhow!("unexpected landing result")),
                };
                self.finish_git_panel_op(id, result, cx);
                true
            }
            _ => {
                cx.notify();
                false
            }
        }
    }
    fn clear_landing_toast_action(&mut self, toast_id: Option<u64>) {
        if let Some(toast) = self
            .toast
            .as_mut()
            .filter(|toast| Some(toast.id) == toast_id)
        {
            toast.action = None;
        }
    }
    fn update_landing_toast(&mut self, id: Uuid) {
        let Some(entry) = self.landing_operations.get(&id) else {
            return;
        };
        let label = self.landing_label(id);
        let can_cancel = !entry.unavailable
            && !self.landing_cancellations.contains(&id)
            && entry
                .job
                .as_ref()
                .is_some_and(|job| job.state == LandingState::Queued);
        let toast_id = entry.operation.toast_id;
        if let Some(toast) = self
            .toast
            .as_mut()
            .filter(|toast| Some(toast.id) == toast_id)
        {
            toast.message = label;
            toast.action = can_cancel.then(|| ToastAction {
                label: tr!("git_panel.land_cancel_queued").into(),
                kind: ToastActionKind::CancelQueuedLanding(id),
            });
        }
    }
    pub(super) fn landing_label(&self, id: Uuid) -> String {
        let Some(entry) = self.landing_operations.get(&id) else {
            return tr!("git_panel.land_status_unavailable");
        };
        if entry.unavailable {
            return tr!("git_panel.land_status_unavailable");
        }
        if self.landing_cancellations.contains(&id) {
            return tr!("git_panel.land_canceling");
        }
        entry
            .job
            .as_ref()
            .map(landing_job_label)
            .unwrap_or_else(|| tr!("git_panel.land_requesting"))
    }
    pub(super) fn cancel_queued_landing(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if self.landing_cancellations.contains(&id) {
            return;
        }
        let workspace = self
            .landing_operations
            .get(&id)
            .filter(|entry| {
                !entry.unavailable
                    && entry
                        .job
                        .as_ref()
                        .is_some_and(|job| job.state == LandingState::Queued)
            })
            .map(|entry| entry.operation.workspace.clone())
            .or_else(|| {
                self.landing_panel_queue
                    .as_ref()
                    .filter(|(_, snapshot)| {
                        snapshot
                            .jobs
                            .iter()
                            .any(|job| job.id == id && job.state == LandingState::Queued)
                    })
                    .map(|(path, _)| path.clone())
            });
        let Some(workspace) = workspace else {
            return;
        };
        let Some(client) = self.workspace_client_for_path(&workspace) else {
            return;
        };
        self.landing_cancellations.insert(id);
        self.update_landing_toast(id);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { client.request(WorkspaceOperation::CancelQueuedLanding { id }) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                this.landing_cancellations.remove(&id);
                if let Err(error) = result {
                    this.mark_landing_unavailable(id, cx);
                    this.show_toast_with_tone(error.to_string(), ToastTone::Failure, None);
                }
                this.refresh_landing_queue(workspace, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn watch_landing_queue(
        &mut self,
        panel_id: Uuid,
        workspace: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.landing_queue_watch == Some(panel_id) {
            return;
        }
        let Some(client) = self
            .workspace_client_for_path(&workspace)
            .filter(|client| client.landing_queue_available())
        else {
            return;
        };
        self.landing_queue_watch = Some(panel_id);
        let events = client.subscribe_landing_queue();
        self.refresh_landing_queue(workspace.clone(), cx);
        cx.spawn(async move |this, cx| {
            let mut checked = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let changed = events.try_iter().next().is_some();
                let keep = this
                    .update(cx, |this, cx| {
                        if !this
                            .git_panel
                            .as_ref()
                            .is_some_and(|panel| panel.id == panel_id)
                        {
                            return false;
                        }
                        if changed || checked.elapsed() >= Duration::from_secs(5) {
                            checked = Instant::now();
                            this.refresh_landing_queue(workspace.clone(), cx);
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn refresh_landing_queue(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        let Some(client) = self
            .workspace_client_for_path(&workspace)
            .filter(|client| client.landing_queue_available())
        else {
            return;
        };
        let operation = self
            .landing_panel_queue
            .as_ref()
            .filter(|(path, _)| *path == workspace)
            .map(|(_, snapshot)| WorkspaceOperation::LandingSnapshot {
                queue_id: snapshot.queue_id,
            })
            .unwrap_or_else(|| WorkspaceOperation::LandingQueue {
                cwd: workspace.clone(),
            });
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { client.request(operation) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(WorkspaceResult::LandingQueue { snapshot }) = result {
                    let task = this.journal_session_for_workspace(&workspace);
                    let own = snapshot
                        .jobs
                        .iter()
                        .filter(|job| {
                            (job.cwd == workspace.to_string_lossy()
                                || (task.is_some() && job.task_id == task))
                                && job.kind == LandingKind::Land
                                && matches!(job.state, LandingState::Queued | LandingState::Running)
                        })
                        .max_by_key(|job| job.sequence)
                        .cloned();
                    this.apply_landing_snapshot(workspace.clone(), snapshot, cx);
                    if let Some(job) = own
                        && !this.landing_operations.values().any(|entry| {
                            entry.operation.workspace == workspace
                                || entry.job.as_ref().is_some_and(|known| known.id == job.id)
                        })
                    {
                        let id = job.id;
                        let operation = git_panel::GitPanelOperation {
                            id,
                            workspace: workspace.clone(),
                            pending: git_panel::GitPanelPending::Landing,
                            toast_id: None,
                            sync_branch: false,
                            sync_branch_name: None,
                            base: job.base.clone(),
                            archive_follow_up: None,
                        };
                        this.landing_operations.insert(
                            id,
                            NativeLanding {
                                initiated_here: false,
                                task: job.task_id,
                                cancel_focus: cx.focus_handle(),
                                operation,
                                job: Some(job),
                                unavailable: false,
                            },
                        );
                        this.monitor_landing(id, workspace.clone(), None, cx);
                    }
                } else if this
                    .landing_panel_queue
                    .as_ref()
                    .is_some_and(|(path, _)| *path == workspace)
                {
                    this.landing_panel_queue = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn apply_landing_snapshot(
        &mut self,
        workspace: PathBuf,
        snapshot: LandingQueueSnapshot,
        cx: &mut Context<Self>,
    ) {
        if !self
            .git_panel
            .as_ref()
            .is_some_and(|panel| panel.workspace == workspace)
        {
            return;
        }
        if self
            .landing_panel_queue
            .as_ref()
            .is_some_and(|(path, previous)| {
                *path == workspace
                    && previous.epoch == snapshot.epoch
                    && previous.revision > snapshot.revision
            })
        {
            return;
        }
        self.landing_panel_rows = snapshot
            .jobs
            .iter()
            .filter(|job| matches!(job.state, LandingState::Queued | LandingState::Running))
            .cloned()
            .collect();
        self.landing_panel_total = self.landing_panel_rows.len();
        self.landing_panel_rows.truncate(8);
        let wanted: std::collections::HashSet<_> = snapshot
            .jobs
            .iter()
            .filter(|job| matches!(job.state, LandingState::Queued | LandingState::Running))
            .take(8)
            .filter_map(|job| job.task_id)
            .collect();
        let titles: HashMap<_, _> = self
            .state
            .sessions
            .iter()
            .filter(|session| wanted.contains(&session.id))
            .map(|session| (session.id, session.title.clone()))
            .collect();
        self.landing_task_titles = snapshot
            .jobs
            .iter()
            .filter_map(|job| {
                job.task_id
                    .and_then(|task| titles.get(&task))
                    .map(|title| (job.id, title.clone()))
            })
            .collect();
        let waiting: std::collections::HashSet<_> = snapshot
            .jobs
            .iter()
            .filter(|job| job.state == LandingState::Queued)
            .map(|job| job.id)
            .collect();
        self.landing_cancel_focus
            .retain(|id, _| waiting.contains(id));
        for job in snapshot
            .jobs
            .iter()
            .filter(|job| job.state == LandingState::Queued)
        {
            self.landing_cancel_focus
                .entry(job.id)
                .or_insert_with(|| cx.focus_handle());
        }
        self.landing_panel_queue = Some((workspace, snapshot));
        cx.notify();
    }

    pub(super) fn render_current_landing_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(task) = self.composer_session_id() else {
            return div().into_any_element();
        };
        let Some((id, entry)) = self
            .landing_operations
            .iter()
            .find(|(_, entry)| entry.task == Some(task))
        else {
            return div().into_any_element();
        };
        let id = *id;
        let theme = Theme::current(cx);
        let queued = !entry.unavailable
            && !self.landing_cancellations.contains(&id)
            && entry
                .job
                .as_ref()
                .is_some_and(|job| job.state == LandingState::Queued);
        let mut row = div()
            .w_full()
            .max_w(px(crate::app::CONTENT_MAX_WIDTH))
            .mx_auto()
            .mb(px(4.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(sp(11.5))
            .text_color(theme.text_tertiary)
            .child(icon("icons/git-merge.svg", 11.0, theme.text_tertiary))
            .child(div().min_w_0().flex_1().child(self.landing_label(id)));
        if queued {
            row = row.child(
                div()
                    .id("composer-cancel-landing")
                    .track_focus(&entry.cancel_focus)
                    .tab_index(0)
                    .px(px(6.0))
                    .py(px(5.0))
                    .rounded(px(6.0))
                    .text_color(theme.accent)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(tr!("git_panel.land_cancel_queued"))
                    .on_click(cx.listener(move |this, _, _, cx| this.cancel_queued_landing(id, cx)))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.cancel_queued_landing(id, cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }
        row.into_any_element()
    }

    pub(super) fn render_landing_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some((workspace, _snapshot)) = self.landing_panel_queue.as_ref().filter(|(path, _)| {
            self.git_panel
                .as_ref()
                .is_some_and(|panel| panel.workspace == *path)
        }) else {
            return div().into_any_element();
        };
        let jobs = &self.landing_panel_rows;
        if jobs.is_empty() {
            return div().into_any_element();
        }
        let theme = Theme::current(cx);
        let mut section = div().p(px(8.0)).flex().flex_col().gap(px(6.0));
        let header = div()
            .id("landing-queue-disclosure")
            .track_focus(&self.landing_queue_focus)
            .tab_index(0)
            .rounded(px(6.0))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .text_size(sp(12.0))
            .child(format!(
                "{} {} ({})",
                if self.landing_queue_expanded {
                    "▾"
                } else {
                    "▸"
                },
                tr!("git_panel.land_queue"),
                self.landing_panel_total
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.landing_queue_expanded = !this.landing_queue_expanded;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.landing_queue_expanded = !this.landing_queue_expanded;
                    cx.notify();
                    cx.stop_propagation();
                }
            }));
        section = section.child(header);
        if self.landing_queue_expanded {
            let remaining = self.landing_panel_total.saturating_sub(8);
            for job in jobs {
                let title = self
                    .landing_task_titles
                    .get(&job.id)
                    .cloned()
                    .unwrap_or_else(|| {
                        if job.kind == LandingKind::Submit {
                            tr!("git_panel.land_queue_submit")
                        } else {
                            job.base
                                .clone()
                                .unwrap_or_else(|| workspace.display().to_string())
                        }
                    });
                let id = job.id;
                let canceling = self.landing_cancellations.contains(&id);
                let label = if canceling {
                    tr!("git_panel.land_canceling")
                } else {
                    landing_job_label(job)
                };
                let mut row = div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(sp(12.0))
                    .child(div().flex_1().min_w_0().child(format!("{title} · {label}")));
                if job.state == LandingState::Queued
                    && !canceling
                    && let Some(focus) = self.landing_cancel_focus.get(&id)
                {
                    row = row.child(
                        div()
                            .id(SharedString::from(format!("cancel-queued-landing-{id}")))
                            .track_focus(focus)
                            .tab_index(0)
                            .px(px(6.0))
                            .py(px(5.0))
                            .rounded(px(6.0))
                            .text_color(theme.accent)
                            .focus_visible(|style| style.bg(theme.focus_highlight()))
                            .child(tr!("git_panel.land_cancel_queued"))
                            .on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.cancel_queued_landing(id, cx)
                                }),
                            )
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.cancel_queued_landing(id, cx);
                                    cx.stop_propagation();
                                }
                            })),
                    );
                }
                section = section.child(row);
            }
            if remaining > 0 {
                section = section.child(tr!("git_panel.land_queue_more", count = remaining));
            }
        }
        section.into_any_element()
    }
}
fn landing_job_label(job: &LandingJob) -> String {
    match job.state {
        LandingState::Queued if job.position == Some(1) => tr!("git_panel.land_queued_next"),
        LandingState::Queued => tr!(
            "git_panel.land_queued_position",
            position = job.position.unwrap_or(1)
        ),
        LandingState::Running => job
            .base
            .as_ref()
            .map(|base| tr!("git_panel.landing_onto", base = base))
            .unwrap_or_else(|| tr!("git_panel.landing")),
        LandingState::Canceled => tr!("git_panel.land_canceled"),
        LandingState::Finished => tr!(
            "git_panel.landed",
            base = job.base.clone().unwrap_or_default()
        ),
    }
}
