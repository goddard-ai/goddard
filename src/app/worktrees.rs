use super::*;

/// Keyboard-navigable rows in the composer worktree picker.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum WorktreePickerAction {
    /// The draft's existing worktree. It leads the list so a bare `enter`
    /// keeps the current choice, matching the branch picker's pinning.
    Current { name: String },
    /// Bind the draft to the project's ordinary checkout.
    Local,
    /// Move a task already bound to the ordinary checkout into a new
    /// worktree carrying the checkout's state.
    Move,
    /// Create a detached worktree at the given base ref — `None` resolves
    /// the repository's default branch on the daemon.
    Create { base_ref: Option<String> },
}

/// The "New worktree" rows: fork from the checkout's current ref, then the
/// repository's default branch, then the base the draft remembered — each
/// only while it names a ref no earlier row already offered. A checkout
/// sitting on the default branch would otherwise show "From main" twice.
pub(super) fn worktree_picker_create_actions(
    workspace: &SessionWorkspace,
    current_ref: &str,
    default_ref: Option<&str>,
) -> Vec<WorktreePickerAction> {
    let mut actions = vec![WorktreePickerAction::Create {
        base_ref: Some(current_ref.to_owned()),
    }];
    if default_ref != Some(current_ref) {
        actions.push(WorktreePickerAction::Create {
            base_ref: default_ref.map(str::to_owned),
        });
    }
    if let SessionWorkspace::NewWorktree {
        base_branch: Some(base),
    } = workspace
        && Some(base.as_str()) != default_ref
        && base != current_ref
    {
        actions.push(WorktreePickerAction::Create {
            base_ref: Some(base.clone()),
        });
    }
    actions
}

impl Waku {
    /// Create the draft's worktree now — at selection time rather than first
    /// submit — named by the user and detached at `base_ref`. The daemon call
    /// runs on the background executor; the draft binds the result when it
    /// lands.
    pub(super) fn create_workspace_worktree(
        &mut self,
        name: Option<String>,
        base_ref: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.selected_session() else {
            return;
        };
        if session.has_started() || session.is_busy() || self.worktree_creation_pending {
            return;
        }
        let project_id = session.project_id;
        let session_id = session.id;
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            return;
        };
        if project.is_projectless() {
            return;
        }
        // Creating another worktree replaces the draft's current one.
        let replaced = match &session.workspace {
            SessionWorkspace::Worktree { path, .. } => Some(path.clone()),
            _ => None,
        };
        self.worktree_creation_pending = true;
        cx.notify();
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let base_branch = base_ref.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace.request(waku_client::WorkspaceOperation::CreateWorktree {
                        project_path: project.path,
                        name,
                        base_ref,
                    })? {
                        waku_client::WorkspaceResult::WorktreeCreated { worktree } => Ok(worktree),
                        _ => anyhow::bail!("the daemon returned an invalid worktree response"),
                    }
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.worktree_creation_pending = false;
                match result {
                    Ok(created) => {
                        // The draft may have submitted or been deleted while
                        // the daemon worked; only a still-unstarted session
                        // binds the worktree, and anything else leaves it for
                        // removal.
                        let applied = waku.state.session_mut(session_id).is_some_and(|session| {
                            if session.has_started() {
                                return false;
                            }
                            session.workspace = SessionWorkspace::Worktree {
                                path: created.path.clone(),
                                name: created.name,
                                branch: None,
                                base_branch: base_branch.clone(),
                            };
                            true
                        });
                        if !applied {
                            waku.remove_draft_worktree(created.path, cx);
                        } else {
                            waku.state.remember_workspace(
                                project_id,
                                &SessionWorkspace::NewWorktree { base_branch: None },
                            );
                            if let Some(path) = replaced {
                                waku.remove_draft_worktree(path, cx);
                            }
                            waku.invalidate_workspace_queries(cx);
                            // A terminal opened while creation was in flight
                            // still points at the local checkout; the bound
                            // worktree is its cwd now.
                            if waku.state.selected_session == Some(session_id) {
                                waku.ensure_right_panel_terminals(cx);
                            }
                            waku.save();
                        }
                    }
                    Err(error) => {
                        waku.show_toast(tr!("errors.create_worktree", error = error.to_string()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Best-effort cleanup of an eagerly created worktree its draft no longer
    /// uses — switched back to Local, replaced, or deleted before its first
    /// submit. Git refuses to remove a dirty worktree, so this cannot destroy
    /// work.
    pub(super) fn remove_draft_worktree(&self, path: PathBuf, cx: &mut Context<Self>) {
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.background_executor()
            .spawn(async move {
                let _ = workspace.request(waku_client::WorkspaceOperation::RemoveWorktree {
                    path,
                    force: false,
                });
            })
            .detach();
    }

    /// Whether a task can move into a worktree right now: bound to the
    /// project's ordinary checkout, idle, backed by a real project, and not
    /// already moving. A move during a turn could split one turn's files
    /// across two directories, so busy tasks wait.
    pub(super) fn can_move_session_to_worktree(&self, session_id: Uuid) -> bool {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return false;
        };
        session.workspace.is_local()
            && !session.is_busy()
            && !self.worktree_move_pending.contains(&session_id)
            && self
                .state
                .projects
                .iter()
                .any(|project| project.id == session.project_id && !project.is_projectless())
    }

    /// Move a local task into a newly created worktree that adopts the
    /// checkout's current state — its HEAD commit plus uncommitted, including
    /// untracked, files. `name` is the picker's optional override; otherwise
    /// the daemon generates a random name. The daemon call runs on the
    /// background executor; [`Self::finish_move_to_worktree`] rebinds the
    /// task when it lands.
    pub(super) fn move_session_to_worktree(
        &mut self,
        session_id: Uuid,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.can_move_session_to_worktree(session_id) {
            return;
        }
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        let workspace_client = waku_client::WorkspaceClient::new(self.daemon.client());
        let Some(project_path) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };
        self.worktree_move_pending.insert(session_id);
        cx.notify();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace_client.request(
                        waku_client::WorkspaceOperation::CreateWorktreeFromCheckout {
                            project_path,
                            name,
                        },
                    )? {
                        waku_client::WorkspaceResult::WorktreeCreated { worktree } => Ok(worktree),
                        _ => Err(anyhow::anyhow!(
                            "the daemon returned an invalid worktree response"
                        )),
                    }
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_move_to_worktree(session_id, result, cx);
            });
        })
        .detach();
    }

    /// Rebind the task to the worktree the daemon created, or abandon that
    /// worktree when the move no longer applies — the task may have been
    /// removed, started a turn, or changed workspace while the request was
    /// in flight.
    fn finish_move_to_worktree(
        &mut self,
        session_id: Uuid,
        result: anyhow::Result<waku_client::worktree::CreatedWorktree>,
        cx: &mut Context<Self>,
    ) {
        if !self.worktree_move_pending.remove(&session_id) {
            return;
        }
        let created = match result {
            Ok(created) => created,
            Err(error) => {
                self.show_toast(tr!("errors.move_to_worktree", error = error.to_string()));
                self.drain_queued_message(session_id, cx);
                cx.notify();
                return;
            }
        };
        // The local workspace ran in the project checkout; that path is what
        // the resumed thread's context still names.
        let checkout_path = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| {
                self.state
                    .projects
                    .iter()
                    .find(|project| project.id == session.project_id)
                    .map(|project| project.path.clone())
            });
        let rebound = self.state.session_mut(session_id).is_some_and(|session| {
            if !session.workspace.is_local() || session.is_busy() {
                return false;
            }
            session.workspace = SessionWorkspace::Worktree {
                path: created.path.clone(),
                name: created.name.clone(),
                branch: None,
                // The worktree adopted the source checkout's state; the
                // primary-checkout fallback resolves that same branch.
                base_branch: None,
            };
            // A session that never started has no recorded paths to correct;
            // a started one's next prompt carries the move notice.
            if session.has_started() {
                session.workspace_moved_from = checkout_path;
            }
            true
        });
        if !rebound {
            // The created worktree only ever holds a copy of the source
            // checkout's state, so force-removing it loses nothing.
            self.discard_worktree_copy(created.path, cx);
            self.drain_queued_message(session_id, cx);
            cx.notify();
            return;
        }
        // A retained runtime still runs in the old checkout; drop it so the
        // next turn starts in the worktree.
        self.reset_session_runtime(session_id);
        self.save();
        if self.state.selected_session == Some(session_id) {
            self.invalidate_workspace_queries(cx);
            self.reload_clean_right_panel_file_editors(cx);
            self.ensure_right_panel_terminals(cx);
        }
        self.show_toast(tr!("session.moved_to_worktree", name = created.name));
        self.drain_queued_message(session_id, cx);
        cx.notify();
    }

    /// Force-remove a worktree created for a move the task can no longer
    /// take. Its content is only a copy of the source checkout's state, so
    /// nothing unique is lost.
    fn discard_worktree_copy(&self, path: PathBuf, cx: &mut Context<Self>) {
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.background_executor()
            .spawn(async move {
                let _ = workspace
                    .request(waku_client::WorkspaceOperation::RemoveWorktree { path, force: true });
            })
            .detach();
    }

    /// Recreates a session's worktree on the background executor when its
    /// directory is gone — archived tasks outlive their worktrees once
    /// archive cleanup removes them. Submission preparation runs the same
    /// restore before a turn, but surfaces like a right-panel terminal need
    /// the directory as soon as the session is back on screen, not at the
    /// first prompt. Prefers the snapshot archive cleanup left, then the
    /// session's latest checkpoint — the same order `prepare_submission`
    /// uses.
    pub(super) fn restore_missing_worktree(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.daemon.is_remote() {
            return;
        }
        let Some((project_path, path, branch)) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id && session.has_started())
            .and_then(|session| match &session.workspace {
                SessionWorkspace::Worktree { path, branch, .. } => self
                    .state
                    .projects
                    .iter()
                    .find(|project| project.id == session.project_id)
                    .map(|project| (project.path.clone(), path.clone(), branch.clone())),
                _ => None,
            })
        else {
            return;
        };
        if path.exists() {
            return;
        }
        let archive_ref = checkpoint::archive_ref(session_id);
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let ensured = cx
                .background_executor()
                .spawn(async move {
                    let archived = workspace
                        .request(waku_client::WorkspaceOperation::HasRef {
                            cwd: project_path.clone(),
                            git_ref: archive_ref.clone(),
                        })
                        .is_ok_and(|result| {
                            matches!(result, waku_client::WorkspaceResult::Bool { value: true })
                        });
                    let base_ref = if archived {
                        Some(archive_ref.clone())
                    } else {
                        match workspace.request(waku_client::WorkspaceOperation::SessionTurnRefs {
                            cwd: project_path.clone(),
                            session_id,
                        }) {
                            Ok(waku_client::WorkspaceResult::TurnRefs { turn_counts }) => {
                                turn_counts.into_iter().max().map(|turn_count| {
                                    checkpoint::checkpoint_ref(session_id, turn_count)
                                })
                            }
                            _ => None,
                        }
                    };
                    let ensured =
                        match workspace.request(waku_client::WorkspaceOperation::EnsureWorktree {
                            project_path: project_path.clone(),
                            path,
                            branch,
                            base_ref,
                        }) {
                            Ok(waku_client::WorkspaceResult::WorktreeEnsured {
                                created,
                                branch,
                            }) => Some((created, branch)),
                            _ => None,
                        };
                    // The archive snapshot is single-use, same as the
                    // submission path treats it.
                    if archived && ensured.is_some() {
                        let _ = workspace.request(waku_client::WorkspaceOperation::DeleteRef {
                            cwd: project_path,
                            git_ref: archive_ref,
                        });
                    }
                    ensured
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                let Some((created, branch)) = ensured else {
                    return;
                };
                if !created {
                    return;
                }
                if let Some(session) = waku.state.session_mut(session_id)
                    && let SessionWorkspace::Worktree { branch: stored, .. } =
                        &mut session.workspace
                {
                    *stored = branch;
                }
                waku.save();
                if waku.state.selected_session == Some(session_id) {
                    waku.invalidate_workspace_queries(cx);
                    // Terminals that skipped spawning against the missing
                    // directory can bind the real cwd now.
                    waku.ensure_right_panel_terminals(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Marks an archived session's workspace for snapshot-and-remove —
    /// its worktree, or its projectless directory.
    /// [`Self::drain_pending_workspace_cleanups`] runs it once nothing can
    /// still write into the directory.
    pub(super) fn queue_archived_workspace_cleanup(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        self.pending_workspace_cleanups.insert(session_id);
        self.drain_pending_workspace_cleanups(cx);
    }

    /// Runs queued archived-worktree cleanups whose sessions have gone quiet.
    ///
    /// Removal waits for every writer that could still touch the worktree: a
    /// settling turn, an in-flight submission preparation, live detached
    /// work, and a queued ending-checkpoint capture — deleting the directory
    /// first would fail that capture into an error toast. Sessions that left
    /// the archived set, or were never on a worktree, drop out here.
    ///
    /// A projectless task has no worktree to retire; when its project's
    /// every session is archived and quiet, the daemon zips the workspace
    /// into `~/.waku/archives` and removes it instead.
    pub(super) fn drain_pending_workspace_cleanups(&mut self, cx: &mut Context<Self>) {
        let mut deferred = HashSet::new();
        let mut ready = Vec::new();
        let mut projectless_ready = Vec::new();
        for session_id in std::mem::take(&mut self.pending_workspace_cleanups) {
            let Some(session) = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
            else {
                continue;
            };
            if session.archived_at.is_none() {
                continue;
            }
            let quiet = !session.is_busy()
                && !self.submission_preparations.contains(&session_id)
                && !self.session_has_live_detached_work(session_id)
                && !self.ending_checkpoint_pending(session_id);
            if !quiet {
                deferred.insert(session_id);
                continue;
            }
            if let SessionWorkspace::Worktree { path, .. } = &session.workspace {
                ready.push((session_id, path.clone()));
            } else if let Some(path) = self.archivable_projectless_workspace(session.project_id) {
                projectless_ready.push((session_id, path));
            }
        }
        self.pending_workspace_cleanups = deferred;
        for (session_id, path) in projectless_ready {
            let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
            cx.spawn(async move |waku, cx| {
                let workspace_path = path.clone();
                let removed = cx
                    .background_executor()
                    .spawn(async move {
                        workspace
                            .request(
                                waku_client::WorkspaceOperation::ArchiveProjectlessWorkspace {
                                    path: path.clone(),
                                },
                            )
                            .is_ok()
                            || !path.exists()
                    })
                    .await;
                if removed {
                    let _ = waku.update(cx, move |waku, cx| {
                        waku.close_workspace_terminals(session_id, &workspace_path, cx);
                    });
                }
            })
            .detach();
        }
        for (session_id, path) in ready {
            let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
            cx.spawn(async move |waku, cx| {
                let worktree_path = path.clone();
                let removed = cx
                    .background_executor()
                    .spawn(async move {
                        let git_ref = checkpoint::archive_ref(session_id);
                        let verified = workspace
                            .request(waku_client::WorkspaceOperation::CaptureRef {
                                cwd: path.clone(),
                                git_ref: git_ref.clone(),
                            })
                            .and_then(|_| {
                                workspace.request(waku_client::WorkspaceOperation::HasRef {
                                    cwd: path.clone(),
                                    git_ref,
                                })
                            })
                            .is_ok_and(|result| {
                                matches!(result, waku_client::WorkspaceResult::Bool { value: true })
                            });
                        // The force flag only runs behind a verified snapshot:
                        // a failed capture leaves the worktree on disk rather
                        // than destroying unsaved work.
                        verified
                            && (workspace
                                .request(waku_client::WorkspaceOperation::RemoveWorktree {
                                    path: path.clone(),
                                    force: true,
                                })
                                .is_ok()
                                || !path.exists())
                    })
                    .await;
                if removed {
                    let _ = waku.update(cx, move |waku, cx| {
                        waku.close_workspace_terminals(session_id, &worktree_path, cx);
                    });
                }
            })
            .detach();
        }
    }

    /// The workspace a projectless project's archived sessions can retire:
    /// every session on the project is archived and past the same writers
    /// the worktree cleanup waits for, and the path is a real workspace —
    /// never `~/.waku` itself, which the oldest layout used as a cwd and now
    /// holds configuration.
    fn archivable_projectless_workspace(&self, project_id: Uuid) -> Option<PathBuf> {
        let project = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)?;
        if !project.is_projectless()
            || crate::projectless::is_legacy_root_path(&project.path)
        {
            return None;
        }
        let all_retired = self
            .state
            .sessions
            .iter()
            .filter(|session| session.project_id == project_id)
            .all(|session| {
                session.archived_at.is_some()
                    && !session.is_busy()
                    && !self.submission_preparations.contains(&session.id)
                    && !self.session_has_live_detached_work(session.id)
                    && !self.ending_checkpoint_pending(session.id)
            });
        all_retired.then(|| project.path.clone())
    }

    /// Bring back a projectless workspace archive cleanup zipped away. The
    /// daemon extracts the recorded archive into the project path — or
    /// recreates an empty directory when none was captured — so an
    /// unarchived chat always lands in a real cwd.
    pub(super) fn restore_archived_projectless_workspace(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id && session.has_started())
            .and_then(|session| {
                self.state
                    .projects
                    .iter()
                    .find(|project| project.id == session.project_id)
            })
            .filter(|project| {
                project.is_projectless()
                    && !crate::projectless::is_legacy_root_path(&project.path)
            })
            .map(|project| project.path.clone())
        else {
            return;
        };
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let restored = cx
                .background_executor()
                .spawn(async move {
                    workspace
                        .request(
                            waku_client::WorkspaceOperation::RestoreProjectlessWorkspace {
                                path,
                            },
                        )
                        .is_ok_and(|result| {
                            matches!(result, waku_client::WorkspaceResult::Bool { value: true })
                        })
                })
                .await;
            if restored {
                let _ = waku.update(cx, move |waku, cx| {
                    if waku.state.selected_session == Some(session_id) {
                        waku.invalidate_workspace_queries(cx);
                        // Terminals that skipped spawning against the missing
                        // directory can bind the real cwd now.
                        waku.ensure_right_panel_terminals(cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Close every terminal whose PTY ran inside a removed workspace — the
    /// session's own, wherever their surfaces live, plus any global terminal
    /// spawned into the directory. A surviving shell would keep running
    /// against a deleted cwd.
    fn close_workspace_terminals(
        &mut self,
        session_id: Uuid,
        workspace_path: &Path,
        cx: &mut Context<Self>,
    ) {
        let terminal_ids = self
            .terminal_records
            .iter()
            .filter_map(|(terminal_id, record)| {
                (record.session == Some(session_id)
                    || record
                        .working_directory
                        .as_ref()
                        .is_some_and(|dir| dir.starts_with(workspace_path)))
                .then_some(*terminal_id)
            })
            .collect::<Vec<_>>();
        for terminal_id in terminal_ids {
            self.close_terminal(terminal_id, cx);
        }
    }

    /// Keyboard rows in order: Local, then each create entry.
    pub(super) fn move_worktree_picker_highlight(
        &mut self,
        direction: &str,
        actions: &[WorktreePickerAction],
        cx: &mut Context<Self>,
    ) {
        if actions.is_empty() {
            self.worktree_picker_highlight = None;
            cx.notify();
            return;
        }
        let step: i64 = if direction == "up" { -1 } else { 1 };
        let next = match self.worktree_picker_highlight {
            Some(index) => (index as i64 + step).rem_euclid(actions.len() as i64) as usize,
            None if step < 0 => actions.len() - 1,
            None => 0,
        };
        self.worktree_picker_highlight = Some(next);
        cx.notify();
    }

    /// `true` asks the caller to dismiss the picker after this entity update
    /// ends. Closing sooner runs the toggle observer, which re-enters `Waku`
    /// and double-leases the entity.
    pub(super) fn confirm_worktree_picker_action(
        &mut self,
        actions: &[WorktreePickerAction],
        cx: &mut Context<Self>,
    ) -> bool {
        let action = self
            .worktree_picker_highlight
            .and_then(|index| actions.get(index))
            .or_else(|| actions.first())
            .cloned();
        match action {
            Some(WorktreePickerAction::Current { .. }) => true,
            Some(WorktreePickerAction::Local) => {
                self.select_workspace(SessionWorkspace::Local, cx);
                true
            }
            Some(WorktreePickerAction::Move) => {
                let name = self
                    .worktree_name_input
                    .read(cx)
                    .content()
                    .trim()
                    .to_owned();
                if let Some(session_id) = self.state.selected_session {
                    self.move_session_to_worktree(
                        session_id,
                        (!name.is_empty()).then_some(name),
                        cx,
                    );
                }
                true
            }
            Some(WorktreePickerAction::Create { base_ref }) => {
                let name = self
                    .worktree_name_input
                    .read(cx)
                    .content()
                    .trim()
                    .to_owned();
                self.create_workspace_worktree((!name.is_empty()).then_some(name), base_ref, cx);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rows_skip_the_default_when_the_checkout_sits_on_it() {
        let actions =
            worktree_picker_create_actions(&SessionWorkspace::Local, "main", Some("main"));

        assert_eq!(
            actions,
            vec![WorktreePickerAction::Create {
                base_ref: Some("main".to_owned())
            }]
        );
    }

    #[test]
    fn create_rows_offer_the_default_alongside_a_different_checkout() {
        let actions =
            worktree_picker_create_actions(&SessionWorkspace::Local, "feature", Some("main"));

        assert_eq!(
            actions,
            vec![
                WorktreePickerAction::Create {
                    base_ref: Some("feature".to_owned())
                },
                WorktreePickerAction::Create {
                    base_ref: Some("main".to_owned())
                }
            ]
        );
    }

    #[test]
    fn remembered_base_joins_only_when_it_names_a_new_ref() {
        let distinct = SessionWorkspace::NewWorktree {
            base_branch: Some("develop".to_owned()),
        };
        let actions = worktree_picker_create_actions(&distinct, "feature", Some("main"));
        assert_eq!(actions.len(), 3);
        assert_eq!(
            actions[2],
            WorktreePickerAction::Create {
                base_ref: Some("develop".to_owned())
            }
        );

        // Matching the default branch or the checked-out ref adds no row.
        let on_default = SessionWorkspace::NewWorktree {
            base_branch: Some("main".to_owned()),
        };
        assert_eq!(
            worktree_picker_create_actions(&on_default, "feature", Some("main")).len(),
            2
        );
        let on_current = SessionWorkspace::NewWorktree {
            base_branch: Some("feature".to_owned()),
        };
        assert_eq!(
            worktree_picker_create_actions(&on_current, "feature", Some("main")).len(),
            2
        );
    }

    #[test]
    fn unknown_default_still_offers_the_daemon_resolved_row() {
        let actions = worktree_picker_create_actions(&SessionWorkspace::Local, "main", None);

        assert_eq!(
            actions,
            vec![
                WorktreePickerAction::Create {
                    base_ref: Some("main".to_owned())
                },
                WorktreePickerAction::Create { base_ref: None }
            ]
        );
    }
}
