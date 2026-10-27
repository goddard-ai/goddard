use super::*;

impl WakuBackend {
    pub(super) fn update_session_metadata(
        &self,
        session_id: Uuid,
        expected_revision: u64,
        mutation_id: Uuid,
        change: waku_protocol::SessionMetadataChange,
    ) -> anyhow::Result<ResponsePayload> {
        use waku_protocol::SessionMetadataChange;
        anyhow::ensure!(!mutation_id.is_nil(), "metadata mutation identity missing");
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let _owner = lifecycle.lock();
        let mut state = self.task_state.lock();
        let current = state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| anyhow!("task no longer exists"))?;
        if current.metadata_mutation_id == Some(mutation_id) {
            return Ok(ResponsePayload::SessionMetadataUpdated {
                session: current.list_projection(),
                applied: true,
            });
        }
        if current.metadata_revision != expected_revision {
            return Ok(ResponsePayload::SessionMetadataUpdated {
                session: current.list_projection(),
                applied: false,
            });
        }
        let now = crate::model::unix_time();
        let mut next = current.list_projection();
        match change {
            SessionMetadataChange::Title { title } => {
                next.title = title;
                next.agent_rename_allowed = false;
            }
            SessionMetadataChange::Pin { pinned } => {
                next.pinned_at = pinned.then_some(now);
            }
            SessionMetadataChange::Archive { archived } => {
                anyhow::ensure!(
                    !current.boss_managed,
                    "managed tasks require their lifecycle archive controls"
                );
                next.archived_at = archived.then_some(now);
            }
        }
        next.updated_at = next.updated_at.max(now);
        next.metadata_revision = expected_revision
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .context("task metadata revision exhausted")?;
        next.metadata_mutation_id = Some(mutation_id);
        let mut removed = HashSet::new();
        if next.archived_at.is_some() {
            let mut children: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
            for session in &state.sessions {
                if let Some(parent) = session.side_chat_of {
                    children.entry(parent).or_default().push(session.id);
                }
            }
            let mut queue = vec![session_id];
            while let Some(parent) = queue.pop() {
                if let Some(children) = children.get(&parent) {
                    for &child in children {
                        if child != session_id && removed.insert(child) {
                            queue.push(child);
                        }
                    }
                }
            }
        }
        self.task_store
            .write_session_metadata(&next, expected_revision, &removed)?;
        let current = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .expect("task held under state lock");
        current.title.clone_from(&next.title);
        current.agent_rename_allowed = next.agent_rename_allowed;
        current.archived_at = next.archived_at;
        current.pinned_at = next.pinned_at;
        current.updated_at = next.updated_at;
        current.metadata_revision = next.metadata_revision;
        current.metadata_mutation_id = next.metadata_mutation_id;
        if current.archived_at.is_none() {
            current.details_pruned = false;
        }
        let roots = state
            .sessions
            .iter()
            .filter(|session| removed.contains(&session.id))
            .filter_map(|session| session.workspace.path().map(Path::to_path_buf))
            .collect::<Vec<_>>();
        let mut dropped = Vec::new();
        let mut kept = Vec::with_capacity(state.sessions.len());
        for session in std::mem::take(&mut state.sessions) {
            if removed.contains(&session.id) {
                dropped.push(session);
            } else {
                kept.push(session);
            }
        }
        state.sessions = kept;
        self.removed_session_ids
            .lock()
            .extend(removed.iter().copied());
        drop(state);
        drop_detached(dropped);
        let ids = removed.into_iter().collect::<Vec<_>>();
        drop_detached(self.sweep_orphaned_terminals(&ids, &roots));
        for id in ids {
            drop_detached(self.remove_runtime(id, None, true));
        }
        if next.archived_at.is_some() {
            drop_detached(self.remove_runtime(session_id, None, true));
        }
        Ok(ResponsePayload::SessionMetadataUpdated {
            session: next,
            applied: true,
        })
    }

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

#[cfg(test)]
mod metadata_tests {
    use super::*;
    use waku_protocol::SessionMetadataChange;

    #[test]
    fn metadata_retries_conflicts_and_stale_full_saves_preserve_durable_archive() {
        let root = std::env::temp_dir().join(format!("metadata-cas-{}", Uuid::new_v4()));
        let (backend, id) = super::super::tests::surface_test_backend(&root);
        let mut stale = backend.task_store.load_session_detail(id).unwrap().unwrap();
        stale.push_message(crate::model::MessageRole::Assistant, "x".repeat(100_000));
        {
            let mut state = backend.task_state.lock();
            *state.session_mut(id).unwrap() = stale.clone();
            backend.task_store.save(&mut state).unwrap();
        }
        let mutation_id = Uuid::new_v4();
        for _ in 0..2 {
            let result = backend
                .update_session_metadata(
                    id,
                    0,
                    mutation_id,
                    SessionMetadataChange::Archive { archived: true },
                )
                .unwrap();
            let ResponsePayload::SessionMetadataUpdated { session, applied } = result else {
                panic!("expected metadata result")
            };
            assert!(applied);
            assert_eq!(session.metadata_revision, 1);
            assert!(session.archived_at.is_some());
            assert!(!session.detail_loaded);
            assert!(session.messages.is_empty());
        }
        let result = backend
            .update_session_metadata(
                id,
                0,
                Uuid::new_v4(),
                SessionMetadataChange::Archive { archived: false },
            )
            .unwrap();
        assert!(matches!(
            result,
            ResponsePayload::SessionMetadataUpdated { applied: false, .. }
        ));
        // A delayed full write carries new transcript content but old list
        // metadata. It may save the content, never clear the newer archive.
        stale.updated_at += 10;
        stale.push_message(crate::model::MessageRole::Assistant, "late output");
        backend
            .handle(
                waku_protocol::Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command: Command::SaveTaskState {
                        compact_response: None,
                        projects: Vec::new(),
                        live_session_ids: vec![id],
                        sessions: vec![stale],
                        session_tails: Vec::new(),
                    },
                },
                EventSink::detached(),
                None,
            )
            .unwrap();
        let stored = backend.task_store.load_session_detail(id).unwrap().unwrap();
        assert!(stored.archived_at.is_some());
        assert_eq!(stored.metadata_revision, 1);
        assert_eq!(stored.messages[1].content.len(), 100_000);
        assert_eq!(stored.messages.last().unwrap().content, "late output");
        // The retry identity survives a daemon restart, not just a live cache.
        drop(backend);
        let reopened = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let result = reopened
            .update_session_metadata(
                id,
                0,
                mutation_id,
                SessionMetadataChange::Archive { archived: true },
            )
            .unwrap();
        assert!(matches!(
            result,
            ResponsePayload::SessionMetadataUpdated { applied: true, .. }
        ));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
