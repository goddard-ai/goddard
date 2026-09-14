use super::*;

/// Keyboard-navigable rows in the composer worktree picker.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum WorktreePickerAction {
    /// The draft's existing worktree. It leads the list so a bare `enter`
    /// keeps the current choice, matching the branch picker's pinning.
    Current { name: String },
    /// Bind the draft to the project's ordinary checkout.
    Local,
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
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace.request(waku_client::WorkspaceOperation::CreateWorktree {
                        project_path: project.path,
                        name,
                        prompt: None,
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
                let _ = workspace.request(waku_client::WorkspaceOperation::RemoveWorktree { path });
            })
            .detach();
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
