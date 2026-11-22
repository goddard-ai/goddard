use super::*;

impl WakuBackend {
    pub(super) fn recover_task_workspace(
        &self,
        session_id: Uuid,
        path: Option<PathBuf>,
        restore: bool,
    ) -> anyhow::Result<Option<SessionWorkspace>> {
        anyhow::ensure!(
            !(restore && path.is_some()),
            "choose either an existing checkout or snapshot restoration"
        );
        // Serialize with runtime starts/stops and task removal, without holding
        // the daemon-wide task-state lock through filesystem work.
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let _guard = lifecycle.lock();
        let (workspace, project) = {
            let mut state = self.task_state.lock();
            let session = state
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                .context("task not found")?;
            // A skeleton save only updates list columns. Hydrate before changing
            // the assignment so the corrected workspace reaches the detail blob.
            // A concrete list-column assignment can be newer than that blob.
            let column_workspace = session.workspace.clone();
            let was_loaded = session.detail_loaded;
            self.task_store.hydrate(session)?;
            let corrected_column = !was_loaded
                && matches!(column_workspace, SessionWorkspace::Worktree { .. })
                && session.workspace != column_workspace;
            if corrected_column {
                session.workspace = column_workspace;
            }
            let workspace = session.workspace.clone();
            let project_id = session.project_id;
            let project = state
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .context("task project not found")?;
            let project = project.clone();
            if corrected_column {
                state.mark_session_dirty(session_id);
                self.task_store.save(&mut state)?;
            }
            (workspace, project)
        };
        let runtime_cwd = self
            .sessions
            .lock()
            .get(&session_id)
            .map(|entry| entry.cwd.clone());
        // The resident provider's cwd wins over a stale saved assignment.
        if let Some(cwd) = &runtime_cwd {
            if workspace.path() == Some(cwd.as_path())
                || (matches!(workspace, SessionWorkspace::Local) && cwd == &project.path)
            {
                return Ok(Some(workspace));
            }
        } else if !matches!(workspace, SessionWorkspace::NewWorktree { .. }) {
            // Archived concrete paths are restored by EnsureWorktree at send.
            return Ok(Some(workspace));
        }
        let borrowed = runtime_cwd.is_some() || path.is_some();
        let from_runtime = runtime_cwd.is_some();
        let recovered = if let Some(cwd) = runtime_cwd {
            Some(cwd)
        } else if let Some(path) = path {
            Some(dunce::canonicalize(path).context("the selected checkout is unavailable")?)
        } else if restore {
            // Never restore from today's default branch: only this task's saved
            // snapshot can reproduce the state represented by its transcript.
            let archive = crate::checkpoint::archive_ref(session_id);
            let (snapshot, recorded_base) = if crate::checkpoint::has_ref(&project.path, &archive) {
                (archive, None)
            } else {
                let turn = crate::checkpoint::session_turn_refs(&project.path, session_id)
                    .into_iter()
                    .max()
                    .context(
                        "no saved checkpoint is available; choose the existing worktree instead",
                    )?;
                let base = crate::checkpoint::turn_base_ref(session_id, turn);
                let base = crate::checkpoint::has_ref(&project.path, &base).then_some(base);
                (crate::checkpoint::checkpoint_ref(session_id, turn), base)
            };
            Some(
                crate::worktree::create_from_snapshot(
                    &project.path,
                    &snapshot,
                    recorded_base.as_deref(),
                )?
                .path,
            )
        } else {
            None
        };
        let Some(path) = recovered else {
            return Ok(None);
        };
        let expected = crate::repo::common_git_dir(&project.path)?
            .context("the task project is not a Git repository")?;
        if !(from_runtime && !path.exists())
            && crate::repo::common_git_dir(&path)?.as_ref() != Some(&expected)
        {
            bail!("choose a checkout belonging to this task's repository");
        }
        let recovered = if dunce::canonicalize(&project.path).ok().as_ref() == Some(&path) {
            SessionWorkspace::Local
        } else {
            SessionWorkspace::Worktree {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path,
                branch: None,
                base_branch: match &workspace {
                    SessionWorkspace::NewWorktree { base_branch } => base_branch.clone(),
                    _ => None,
                },
                adopted_by: None,
                borrowed,
            }
        };
        let mut state = self.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .context("task no longer exists")?;
        // Do not overwrite an assignment made while Git was running.
        if session.workspace != workspace {
            let current = session.workspace.clone();
            drop(state);
            if !borrowed && let Some(path) = recovered.path() {
                let _ = crate::worktree::remove(path, false);
            }
            return Ok(
                (!matches!(current, SessionWorkspace::NewWorktree { .. })).then_some(current)
            );
        }
        session.workspace = recovered.clone();
        state.mark_session_dirty(session_id);
        self.task_store.save(&mut state)?;
        Ok(Some(recovered))
    }
}
