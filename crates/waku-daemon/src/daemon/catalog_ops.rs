use super::*;

impl WakuBackend {
    /// Replay the hub's task-state broadcast after a daemon-side catalog
    /// mutation so attached clients re-read the rows. No-op until `serve`
    /// installs the notifier.
    pub(super) fn notify_task_state(&self) {
        if let Some(notifier) = self.task_notifier.lock().clone() {
            notifier();
        }
    }

    /// The configured QA branch for the repository `cwd` belongs to — the
    /// owning registered project's override when set, else the
    /// daemon-global `qa_branch` setting. Returns the raw configured
    /// string; callers run it through `qa_branch_name`/`qa_branch_checked`.
    pub(super) fn project_qa_branch(&self, cwd: &Path) -> String {
        let state = self.task_state.lock();
        project_for_path(&state.projects, cwd)
            .and_then(|project| project.qa_branch.clone())
            .filter(|branch| !branch.trim().is_empty())
            .unwrap_or_else(|| self.settings.get().qa_branch)
    }

    pub(super) fn agent_merge_submit(
        &self,
        owner: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        let owner = owner.context("merge submit requires an employee task credential")?;
        let (worktree, project, base_branch) = {
            let mut state = self.task_state.lock();
            let index = state
                .sessions
                .iter()
                .position(|session| session.id == owner)
                .ok_or_else(|| anyhow!("employee task is missing"))?;
            self.task_store.hydrate(&mut state.sessions[index])?;
            let session = &state.sessions[index];
            let SessionWorkspace::Worktree {
                path,
                base_branch,
                adopted_by: None,
                ..
            } = &session.workspace
            else {
                bail!("merge submit requires this employee's daemon-managed worktree");
            };
            let project = state
                .projects
                .iter()
                .find(|project| project.id == session.project_id)
                .cloned()
                .ok_or_else(|| anyhow!("employee project is missing"))?;
            (path.clone(), project, base_branch.clone())
        };
        anyhow::ensure!(
            project.submissions_enabled,
            "submissions not enabled for this project — the boss opts a project in with the setProjectSubmissions operation"
        );
        if !crate::worktree::is_worktree_of(&project.path, &worktree) {
            bail!("employee worktree is no longer linked to its recorded project");
        }

        let settings = self.settings.get();
        let configured = project
            .qa_branch
            .as_deref()
            .filter(|branch| !branch.trim().is_empty())
            .unwrap_or(&settings.qa_branch);
        let branch = crate::review::qa_branch_name(configured);
        let sha =
            crate::agent_merge::submit(&project.path, &worktree, base_branch.as_deref(), &branch)?;
        Ok(ResponsePayload::AgentMergeSubmitted { sha })
    }

    /// Refresh a friend's name on their delivered sessions — called when
    /// a nickname changes so the sidebar row keeps showing the name the
    /// user knows them by. Deliveries do the same inline.
    pub(super) fn rename_friend_sessions(&self, node_id: &str) -> anyhow::Result<()> {
        let name = self
            .share
            .state()
            .friends
            .iter()
            .find(|friend| friend.node_id == node_id)
            .map(|friend| {
                friend
                    .nickname
                    .clone()
                    .filter(|nickname| !nickname.is_empty())
                    .unwrap_or_else(|| friend.name.clone())
            });
        let Some(name) = name else {
            return Ok(());
        };
        let mut state = self.task_state.lock();
        if rename_friend_sessions(&mut state, node_id, &name) {
            self.task_store.save(&mut state)?;
        }
        Ok(())
    }

    /// A review op moved `refs/notes/qa`, the QA branch, or the base
    /// branch on a shared origin — tell friends sharing it and bump review
    /// surfaces locally. Best-effort: no notices go out without an
    /// `origin`.
    pub(super) fn notify_review_moved(
        &self,
        cwd: &Path,
        review_move: ReviewMove,
        result: &WorkspaceResult,
    ) {
        let Ok(Some(origin_url)) = crate::git_branch::remote_url(cwd, "origin") else {
            return;
        };
        match review_move {
            ReviewMove::Approved => {
                self.share.notify_refs(
                    origin_url.clone(),
                    vec![crate::review::NOTES_REF.to_owned()],
                );
            }
            ReviewMove::Rejected => {
                self.share.notify_refs(
                    origin_url.clone(),
                    vec![crate::review::NOTES_REF.to_owned()],
                );
                self.share.notify_push(
                    origin_url.clone(),
                    vec![crate::review::qa_branch_name(&self.project_qa_branch(cwd))],
                );
            }
            ReviewMove::Promoted => {
                let base = match result {
                    WorkspaceResult::ReviewQueue { queue: Some(queue) } => {
                        queue.base_branch.clone()
                    }
                    _ => None,
                };
                self.share.notify_push(
                    origin_url.clone(),
                    vec![base.unwrap_or_else(|| "main".to_owned())],
                );
            }
        }
        self.share.review_changed(origin_url);
    }
}
