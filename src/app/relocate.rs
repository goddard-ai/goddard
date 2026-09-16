use super::*;
use std::ffi::OsStr;
use std::path::Component;

/// What one reconciliation pass concluded about a project's folder.
enum ProjectLocation {
    /// The stored path still resolves; carries a Finder bookmark when the
    /// project lacked one.
    Present { id: Uuid, bookmark: Option<Vec<u8>> },
    /// The stored path is gone but the bookmark re-resolved — a rename or
    /// same-volume move the app follows on its own.
    Relocated { id: Uuid, new_path: PathBuf },
    /// Neither path nor bookmark resolves; the user has to locate it.
    Missing { id: Uuid },
}

/// Rewrite a session's worktree path for a project that moved from
/// `old_project` to `new_project`. Worktrees live in `worktrees/` beside the
/// repository root, so the shared prefix of the stored path and the old
/// project path is the repository's parent; swapping that prefix for the new
/// parent lands on the worktree's new location whether the project was
/// renamed in place or its whole parent directory moved.
///
/// Returns the session's new workspace path, the worktree root it sits
/// under, and the worktree root at the stored location — the roots are what
/// `git worktree repair` needs, and they differ exactly when the sibling
/// `worktrees/` directory did not travel with the project. `None` for paths
/// outside the `worktrees/` layout the app creates.
fn remapped_worktree_paths(
    old_project: &Path,
    new_project: &Path,
    stored: &Path,
) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let shared = old_project
        .components()
        .zip(stored.components())
        .take_while(|(a, b)| a == b)
        .count();
    // Past the shared prefix the stored path continues with
    // `worktrees/<repo>/<name>/<repo>`; anything else is not ours.
    if stored.components().nth(shared) != Some(Component::Normal(OsStr::new("worktrees"))) {
        return None;
    }
    // The old project path continues past the repository's parent with the
    // repository directory plus the project's repo-relative suffix. The
    // suffix repeats after the worktree root, so its length is what splits
    // the stored path into root and project directory.
    let suffix = old_project.components().count().checked_sub(shared + 1)?;
    let new_parent = new_project.ancestors().nth(suffix + 1)?;
    let workspace_path = new_parent.join(stored.components().skip(shared).collect::<PathBuf>());
    let new_root = workspace_path.ancestors().nth(suffix)?.to_path_buf();
    let old_root = stored.ancestors().nth(suffix)?.to_path_buf();
    Some((workspace_path, new_root, old_root))
}

impl Waku {
    /// Reconcile every stored project path with the filesystem on a
    /// background pass: backfill Finder bookmarks on live folders, follow a
    /// bookmark across a rename, and mark what is genuinely missing so the
    /// UI can offer relocation. The daemon host owns these paths, so the
    /// pass is skipped for remote connections.
    pub(super) fn refresh_project_locations(&mut self, cx: &mut Context<Self>) {
        if self.daemon.is_remote() {
            return;
        }
        let generation = self.project_location_generation.get().wrapping_add(1);
        self.project_location_generation.set(generation);
        let projects: Vec<Project> = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless())
            .cloned()
            .collect();
        cx.spawn(async move |waku, cx| {
            let outcomes = cx
                .background_executor()
                .spawn(async move {
                    projects
                        .into_iter()
                        .map(|project| {
                            if project.path.is_dir() {
                                return ProjectLocation::Present {
                                    id: project.id,
                                    bookmark: project
                                        .bookmark
                                        .is_none()
                                        .then(|| crate::bookmarks::create(&project.path))
                                        .flatten(),
                                };
                            }
                            match project
                                .bookmark
                                .as_deref()
                                .and_then(crate::bookmarks::resolve)
                            {
                                // A stale bookmark resolving to the stored
                                // path means the folder reappeared mid-pass.
                                Some((resolved, _))
                                    if resolved.is_dir() && resolved == project.path =>
                                {
                                    ProjectLocation::Present {
                                        id: project.id,
                                        bookmark: None,
                                    }
                                }
                                Some((resolved, _)) if resolved.is_dir() => {
                                    ProjectLocation::Relocated {
                                        id: project.id,
                                        new_path: resolved,
                                    }
                                }
                                _ => ProjectLocation::Missing { id: project.id },
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.project_location_generation.get() != generation {
                    return;
                }
                let mut changed = false;
                for outcome in outcomes {
                    match outcome {
                        ProjectLocation::Present { id, bookmark } => {
                            waku.missing_projects.remove(&id);
                            if let Some(bookmark) = bookmark
                                && let Some(project) =
                                    waku.state.projects.iter_mut().find(|p| p.id == id)
                            {
                                project.bookmark = Some(bookmark);
                                changed = true;
                            }
                        }
                        ProjectLocation::Relocated { id, new_path } => {
                            let name = waku
                                .state
                                .projects
                                .iter()
                                .find(|project| project.id == id)
                                .map(Project::display_name)
                                .unwrap_or_default();
                            if waku.apply_project_relocation(id, new_path, cx) {
                                waku.show_toast_with_tone(
                                    tr!("project.relocated", name = name),
                                    ToastTone::Notice,
                                    None,
                                );
                            }
                        }
                        ProjectLocation::Missing { id } => {
                            waku.missing_projects.insert(id);
                        }
                    }
                }
                if changed {
                    waku.save();
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The missing-folder recovery path: a native folder picker that repoints
    /// the project, keeping its id so every task on it survives.
    pub(super) fn relocate_project(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        if self.daemon.is_remote() {
            self.show_toast(tr!("errors.remote_project_locate"));
            cx.notify();
            return;
        }
        // A manual answer invalidates any in-flight reconciliation pass —
        // its outcomes predate the user's choice.
        self.project_location_generation
            .set(self.project_location_generation.get().wrapping_add(1));
        let name = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(Project::display_name)
            .unwrap_or_default();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("project.locate_prompt", name = name).into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    if this.apply_project_relocation(project_id, path.clone(), cx) {
                        this.show_success_toast(tr!(
                            "project.relocated_to",
                            path = settings::abbreviate_home_path(
                                &path,
                                this.home_directory.as_deref()
                            )
                        ));
                        // Other folders may have gone missing alongside this
                        // one; re-run the pass rather than leaving them
                        // unmarked until the next launch.
                        this.refresh_project_locations(cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Point `project_id` at `new_path`: rewrite the stored path, name, and
    /// bookmark, remap its sessions' worktree paths onto the new location,
    /// and ask the daemon to repair Git's worktree links. Returns whether
    /// the relocation was applied.
    fn apply_project_relocation(
        &mut self,
        project_id: Uuid,
        new_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self
            .state
            .projects
            .iter()
            .position(|project| project.id == project_id)
        else {
            return false;
        };
        if let Some(other) = self
            .state
            .projects
            .iter()
            .find(|project| project.id != project_id && project.path == new_path)
        {
            self.show_toast(tr!(
                "errors.project_path_in_use",
                name = other.display_name()
            ));
            cx.notify();
            return false;
        }
        let old_path = self.state.projects[index].path.clone();
        if old_path == new_path {
            self.missing_projects.remove(&project_id);
            return true;
        }
        // Sessions store absolute worktree paths beside the repository's old
        // parent. A path that still resolves survived the rename — the
        // sibling `worktrees/` directory does not move with the project —
        // while a missing one is repointed so `ensure` recreates it under
        // the new root rather than resurrecting the old directory. Repair is
        // told the root the filesystem actually holds: the surviving
        // worktree's old root for re-linking, the remapped one when the whole
        // parent moved.
        let session_ids: Vec<Uuid> = self
            .state
            .sessions
            .iter()
            .filter(|session| session.project_id == project_id)
            .map(|session| session.id)
            .collect();
        let mut worktree_roots = Vec::new();
        for session_id in session_ids {
            let Some(session) = self.state.session_mut(session_id) else {
                continue;
            };
            let SessionWorkspace::Worktree { path, .. } = &mut session.workspace else {
                continue;
            };
            let Some((workspace_path, new_root, old_root)) =
                remapped_worktree_paths(&old_path, &new_path, path)
            else {
                continue;
            };
            if path.exists() {
                worktree_roots.push(old_root);
                continue;
            }
            *path = workspace_path;
            worktree_roots.push(new_root);
        }
        let project = &mut self.state.projects[index];
        project.name = new_path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("Project")
            .to_owned();
        project.path = new_path.clone();
        project.bookmark = crate::bookmarks::create(&new_path);
        self.missing_projects.remove(&project_id);
        self.save();

        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .spawn(async move {
                    let _ = workspace.request(waku_client::WorkspaceOperation::RepairWorktrees {
                        cwd: new_path,
                        paths: worktree_roots,
                    });
                })
                .await;
        })
        .detach();

        if self.projects_page == Some(project_id) {
            self.projects_refresh(project_id, cx);
        }
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::remapped_worktree_paths;
    use std::path::Path;

    #[test]
    fn a_rename_in_place_leaves_sibling_worktree_paths_alone() {
        // Renaming `/code/app` to `/code/app2` does not move the sibling
        // `worktrees/` directory, so the stored path survives verbatim and
        // both roots name the existing worktree for repair.
        let (workspace, new_root, old_root) = remapped_worktree_paths(
            Path::new("/code/app"),
            Path::new("/code/app2"),
            Path::new("/code/worktrees/app/x/app"),
        )
        .unwrap();
        assert_eq!(workspace, Path::new("/code/worktrees/app/x/app"));
        assert_eq!(new_root, Path::new("/code/worktrees/app/x/app"));
        assert_eq!(old_root, new_root);
    }

    #[test]
    fn a_parent_move_remaps_the_worktree_under_the_new_parent() {
        let (workspace, new_root, old_root) = remapped_worktree_paths(
            Path::new("/code/app"),
            Path::new("/code2/app"),
            Path::new("/code/worktrees/app/x/app"),
        )
        .unwrap();
        assert_eq!(workspace, Path::new("/code2/worktrees/app/x/app"));
        assert_eq!(new_root, Path::new("/code2/worktrees/app/x/app"));
        assert_eq!(old_root, Path::new("/code/worktrees/app/x/app"));
    }

    #[test]
    fn a_subdirectory_project_drops_its_suffix_from_the_worktree_root() {
        // The project lives at `repo/sub`; the worktree path repeats `sub`
        // after the root, so the root is one component up.
        let (workspace, new_root, old_root) = remapped_worktree_paths(
            Path::new("/code/repo/sub"),
            Path::new("/moved/repo/sub"),
            Path::new("/code/worktrees/repo/wt/repo/sub"),
        )
        .unwrap();
        assert_eq!(workspace, Path::new("/moved/worktrees/repo/wt/repo/sub"));
        assert_eq!(new_root, Path::new("/moved/worktrees/repo/wt/repo"));
        assert_eq!(old_root, Path::new("/code/worktrees/repo/wt/repo"));
    }

    #[test]
    fn paths_outside_the_worktrees_layout_are_not_remapped() {
        assert!(
            remapped_worktree_paths(
                Path::new("/code/app"),
                Path::new("/code2/app"),
                Path::new("/elsewhere/app"),
            )
            .is_none()
        );
        assert!(
            remapped_worktree_paths(
                Path::new("/code/app"),
                Path::new("/code2/app"),
                Path::new("/code/app/inside"),
            )
            .is_none()
        );
    }
}
