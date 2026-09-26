use super::*;

enum BranchOperation {
    Checkout(String),
    Create(String),
    /// Re-point an unstarted draft worktree's detached HEAD at a new base.
    Reset(String),
}

/// `https://github.com/<owner>/<repo>` for a GitHub remote URL, `None` for any
/// other host or a value that does not parse. Handles the `scheme://` forms
/// (https, ssh, git) and the scp-style `git@github.com:owner/repo` shorthand.
pub(super) fn github_remote_base(remote_url: &str) -> Option<String> {
    let remote_url = remote_url.trim();
    let (host, path) = if let Some((_, rest)) = remote_url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        let host = authority
            .rsplit('@')
            .next()
            .unwrap_or(authority)
            .split(':')
            .next()
            .unwrap_or_default();
        (host, path)
    } else {
        let (authority, path) = remote_url.split_once(':')?;
        (authority.rsplit('@').next().unwrap_or(authority), path)
    };
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let owner = segments.next()?;
    let repo = segments.next()?;
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("https://github.com/{owner}/{repo}"))
}

/// Escapes the characters that would change a GitHub URL's meaning. Git
/// refnames already forbid space, `?`, `*`, `:` and friends; file paths keep
/// their `/` separators and non-ASCII names are fine as UTF-8.
pub(super) fn github_url_path_encode(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('?', "%3F")
}

/// `…/tree/<branch>` for the snapshot's checked-out branch, or its detached
/// HEAD commit — GitHub resolves either.
pub(super) fn github_branch_url(snapshot: &BranchSnapshot) -> Option<String> {
    let base = github_remote_base(snapshot.origin_url.as_deref()?)?;
    let reference = snapshot.display_branch()?;
    Some(format!("{base}/tree/{}", github_url_path_encode(reference)))
}

/// `…/blob/<branch>/<path>` for a file inside the workspace.
pub(super) fn github_file_url(snapshot: &BranchSnapshot, relative_path: &str) -> Option<String> {
    let base = github_remote_base(snapshot.origin_url.as_deref()?)?;
    let reference = snapshot.display_branch()?;
    Some(format!(
        "{base}/blob/{}/{}",
        github_url_path_encode(reference),
        github_url_path_encode(relative_path)
    ))
}

impl Waku {
    /// The selected workspace's last-resolved snapshot, `None` until a fetch
    /// for it has landed. A `&self` read for the command palette; renderers
    /// wanting to start a fetch use [`branch_snapshot_for_workspace`].
    ///
    /// [`branch_snapshot_for_workspace`]: Self::branch_snapshot_for_workspace
    pub(super) fn selected_branch_snapshot(&self) -> Option<&BranchSnapshot> {
        let path = self.selected_workspace_path()?;
        self.visible_branch_snapshot
            .as_ref()
            .filter(|(snapshot_path, _)| snapshot_path == path)
            .map(|(_, snapshot)| snapshot)
    }

    pub(super) fn sync_branch_picker_rows(&self, rows: &[crate::git_branch::BranchEntry]) {
        let mut cached = self.branch_picker_row_cache.borrow_mut();
        if cached.as_slice() == rows {
            return;
        }
        *cached = rows.to_vec();
        self.branch_picker_list_state
            .reset_with_uniform_height(rows.len(), px(PICKER_ROW_HEIGHT));
    }

    /// Read the selected workspace's cached Git branches, starting one
    /// background fetch on a miss. The previous selected-path snapshot remains
    /// drawable while an invalidation is being refreshed.
    pub(super) fn branch_snapshot_for_workspace(
        &mut self,
        workspace_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Option<BranchSnapshot> {
        let workspace_path = workspace_path.to_path_buf();
        let fallback = self
            .visible_branch_snapshot
            .as_ref()
            .filter(|(path, _)| path == &workspace_path)
            .map(|(_, snapshot)| snapshot.clone());

        match self.branch_snapshots.read(&workspace_path) {
            Query::Ready(result) => match result.as_ref() {
                Ok(Some(snapshot)) => {
                    let snapshot = snapshot.clone();
                    self.cache_sidebar_branch_label(&workspace_path, snapshot.display_branch());
                    self.visible_branch_snapshot = Some((workspace_path, snapshot.clone()));
                    Some(snapshot)
                }
                Ok(None) => {
                    self.cache_sidebar_branch_label(&workspace_path, None);
                    if self
                        .visible_branch_snapshot
                        .as_ref()
                        .is_some_and(|(path, _)| path == &workspace_path)
                    {
                        self.visible_branch_snapshot = None;
                    }
                    None
                }
                Err(_) => fallback,
            },
            Query::Pending => fallback,
            Query::Missing(token) => {
                let fetch_path = workspace_path.clone();
                let Some(workspace) = self.workspace_client_for_path(&fetch_path) else {
                    // Offline remote owner: leave the miss uncached so the
                    // next read retries once the host reconnects — dropping
                    // the token alone would leave the slot Loading forever.
                    self.branch_snapshots.abandon(token);
                    return fallback;
                };
                cx.spawn(async move |waku, cx| {
                    let result = cx
                        .background_executor()
                        .spawn({
                            let fetch_path = fetch_path.clone();
                            async move {
                                match workspace.request(
                                    waku_client::WorkspaceOperation::InspectBranches {
                                        cwd: fetch_path.clone(),
                                    },
                                ) {
                                    Ok(waku_client::WorkspaceResult::Branches { snapshot }) => {
                                        Ok(snapshot)
                                    }
                                    Ok(_) => {
                                        Err("the daemon returned an invalid branch response"
                                            .to_owned())
                                    }
                                    Err(error) => Err(error.to_string()),
                                }
                            }
                        })
                        .await;
                    let _ = waku.update(cx, |waku, cx| {
                        if !waku.branch_snapshots.fulfill(token, result.clone()) {
                            return;
                        }
                        match &result {
                            Ok(Some(snapshot)) => waku
                                .cache_sidebar_branch_label(&fetch_path, snapshot.display_branch()),
                            Ok(None) => waku.cache_sidebar_branch_label(&fetch_path, None),
                            Err(_) => {}
                        }
                        let selected = waku
                            .selected_workspace_path()
                            .is_some_and(|path| path == fetch_path);
                        if selected {
                            match result {
                                Ok(Some(snapshot)) => {
                                    let mut persisted_branch_changed = false;
                                    if let Some(current) = snapshot.current.as_deref()
                                        && let Some(session) = waku.selected_session_mut()
                                        && let SessionWorkspace::Worktree { branch, .. } =
                                            &mut session.workspace
                                        && branch.as_deref() != Some(current)
                                    {
                                        *branch = Some(current.to_owned());
                                        persisted_branch_changed = true;
                                    }
                                    waku.visible_branch_snapshot = Some((fetch_path, snapshot));
                                    if persisted_branch_changed {
                                        waku.save();
                                    }
                                }
                                Ok(None) => waku.visible_branch_snapshot = None,
                                Err(_) => {}
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
                fallback
            }
        }
    }

    pub(super) fn refresh_selected_branch_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            self.visible_branch_snapshot = None;
            return;
        };
        self.refresh_workspace_branch_snapshot(&path, cx);
    }

    /// Invalidate the cached snapshot for an explicit workspace — the
    /// composer's branch picker can describe a Big Picture subject whose
    /// checkout is not the selection's. Base push state derives from the
    /// same refs, so it goes too — a sync-strip push or pull run in a
    /// terminal reaches this through `refresh_selected_branch_snapshot`.
    pub(super) fn refresh_workspace_branch_snapshot(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        self.branch_snapshots.invalidate(&path.to_path_buf());
        self.invalidate_workspace_remote_files(path);
        self.invalidate_base_push_state(path);
        cx.notify();
    }

    /// Drop the remote-file verdicts for a workspace — they move with the
    /// same events as the branch snapshot (fetch, push, checkout, commit).
    pub(super) fn invalidate_workspace_remote_files(&mut self, path: &std::path::Path) {
        self.remote_files
            .invalidate_where(|(workspace, _)| workspace == path);
    }

    /// Read whether a workspace file is reachable on the `origin` remote,
    /// starting one background fetch on a miss. `Some(Ok(Some(_)))` is the
    /// verified hit, `Some(Ok(None))` the verified miss; `None` means the
    /// answer is still in flight so callers should draw nothing rather than
    /// a button that can vanish a frame later.
    pub(super) fn remote_file_for(
        &mut self,
        workspace_path: &std::path::Path,
        relative_path: &str,
        cx: &mut Context<Self>,
    ) -> Option<Result<Option<RemoteFileRef>, String>> {
        let key = (workspace_path.to_path_buf(), relative_path.to_owned());
        match self.remote_files.read(&key) {
            Query::Ready(result) => Some((*result).clone()),
            Query::Pending => None,
            Query::Missing(token) => {
                let Some(workspace) = self.workspace_client_for_path(workspace_path) else {
                    // Offline remote owner: drop the claim so the next read
                    // retries once the host reconnects.
                    self.remote_files.abandon(token);
                    return None;
                };
                let cwd = workspace_path.to_path_buf();
                let path = relative_path.to_owned();
                cx.spawn(async move |waku, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            match workspace.request(
                                waku_client::WorkspaceOperation::ResolveRemoteFile { cwd, path },
                            ) {
                                Ok(waku_client::WorkspaceResult::RemoteFile { file }) => Ok(file),
                                Ok(_) => Err("the daemon returned an invalid remote-file response"
                                    .to_owned()),
                                Err(error) => Err(error.to_string()),
                            }
                        })
                        .await;
                    let _ = waku.update(cx, |waku, cx| {
                        if waku.remote_files.fulfill(token, result) {
                            cx.notify();
                        }
                    });
                })
                .detach();
                None
            }
        }
    }

    /// Select an existing branch on the workspace subject. A planned
    /// worktree remembers it as the base ref without touching the ordinary
    /// checkout; concrete workspaces run a real `git switch` on the
    /// background executor.
    ///
    /// `true` asks the caller to dismiss the picker after this entity update
    /// ends. Closing sooner runs the toggle observer, which re-enters `Waku`
    /// and double-leases the entity.
    pub(super) fn choose_workspace_branch(
        &mut self,
        branch: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let (subject_session_id, _) = self.workspace_subject();
        let session = subject_session_id.and_then(|session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
        });
        if session.is_some_and(|session| session.is_busy()) || self.branch_operation_pending {
            return false;
        }
        let workspace = self.workspace_subject_workspace();
        if matches!(workspace, Some(SessionWorkspace::NewWorktree { .. })) {
            // The untargeted overlay composer may have no draft yet; the
            // base-branch pick materializes it so the choice has somewhere
            // to live.
            let Some(session_id) = self.ensure_workspace_subject_session(cx) else {
                return false;
            };
            let Some(project_id) = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.project_id)
            else {
                return false;
            };
            let changed = self.state.session_mut(session_id).is_some_and(|session| {
                let SessionWorkspace::NewWorktree { base_branch } = &mut session.workspace else {
                    return false;
                };
                if base_branch.as_deref() == Some(branch.as_str()) {
                    return false;
                }
                *base_branch = Some(branch.clone());
                true
            });
            if changed {
                self.state.remember_workspace(
                    project_id,
                    &SessionWorkspace::NewWorktree {
                        base_branch: Some(branch),
                    },
                );
                self.save();
                cx.notify();
            }
            return true;
        }

        // An unstarted draft's worktree is still picking its base: its HEAD
        // is detached, so no branch is ever taken. Re-point it in place
        // rather than checking out, keeping branches other worktrees own
        // selectable.
        if let Some(SessionWorkspace::Worktree { base_branch, .. }) = &workspace
            && session.is_some_and(|session| !session.has_started())
        {
            let Some(path) = self.workspace_subject_path() else {
                return false;
            };
            let current = self
                .visible_branch_snapshot
                .as_ref()
                .filter(|(snapshot_path, _)| snapshot_path == &path)
                .and_then(|(_, snapshot)| snapshot.current.as_deref());
            // Re-picking the effective base changes nothing: the ref the
            // worktree detached at, or a branch it was switched onto by hand.
            if current == Some(branch.as_str())
                || (current.is_none() && base_branch.as_deref() == Some(branch.as_str()))
            {
                return true;
            }
            self.start_branch_operation(
                subject_session_id,
                path,
                BranchOperation::Reset(branch),
                cx,
            );
            return true;
        }

        let Some(path) = self.workspace_subject_path() else {
            return false;
        };
        if self
            .visible_branch_snapshot
            .as_ref()
            .filter(|(snapshot_path, _)| snapshot_path == &path)
            .and_then(|(_, snapshot)| snapshot.current.as_deref())
            == Some(branch.as_str())
        {
            return true;
        }
        self.start_branch_operation(
            subject_session_id,
            path,
            BranchOperation::Checkout(branch),
            cx,
        );
        true
    }

    pub(super) fn begin_branch_creation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let subject_session = self.workspace_subject_session();
        if self.branch_operation_pending
            || subject_session.is_some_and(|session| session.is_busy())
            || !matches!(
                self.workspace_subject_workspace(),
                Some(SessionWorkspace::Local | SessionWorkspace::Worktree { .. })
            )
        {
            return;
        }
        self.branch_picker_mode = BranchPickerMode::Create;
        self.branch_picker_highlight = None;
        self.branch_create_input
            .update(cx, |input, cx| input.clear(cx));
        let focus = self.branch_create_input.read(cx).focus_handle(cx);
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
        cx.notify();
    }

    /// Escape from the create form: back to browsing with the filter
    /// refocused — the reverse of [`begin_branch_creation`], with the same
    /// double-frame focus dance because the search field only exists once
    /// the browse body has rendered.
    ///
    /// [`begin_branch_creation`]: Self::begin_branch_creation
    pub(super) fn cancel_branch_creation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.branch_picker_mode = BranchPickerMode::Browse;
        self.branch_picker_highlight = None;
        let focus = self.branch_search.read(cx).focus_handle(cx);
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
        cx.notify();
    }

    pub(super) fn confirm_branch_creation(&mut self, cx: &mut Context<Self>) -> bool {
        if self.branch_picker_mode != BranchPickerMode::Create || self.branch_operation_pending {
            return false;
        }
        let branch = self
            .branch_create_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        if branch.is_empty() {
            return false;
        }
        let (subject_session_id, _) = self.workspace_subject();
        let Some(path) = self.workspace_subject_path() else {
            return false;
        };
        self.start_branch_operation(
            subject_session_id,
            path,
            BranchOperation::Create(branch),
            cx,
        );
        true
    }

    pub(super) fn move_branch_picker_highlight(
        &mut self,
        key: &str,
        actions: &[BranchPickerAction],
        cx: &mut Context<Self>,
    ) {
        if self.branch_picker_mode != BranchPickerMode::Browse || actions.is_empty() {
            return;
        }
        let current = self
            .branch_picker_highlight
            .filter(|index| *index < actions.len());
        let next = match (key, current) {
            ("up", Some(0)) => actions.len() - 1,
            ("up", Some(index)) => index - 1,
            ("up", None) => actions.len() - 1,
            (_, Some(index)) => (index + 1) % actions.len(),
            (_, None) => 0,
        };
        self.branch_picker_highlight = Some(next);
        if let Some(BranchPickerAction::Checkout(branch)) = actions.get(next)
            && let Some(row) = self
                .branch_picker_row_cache
                .borrow()
                .iter()
                .position(|entry| entry.name == *branch)
        {
            self.branch_picker_list_state.scroll_to_reveal_item(row);
        }
        cx.notify();
    }

    /// Apply the keyboard-selected action, returning whether the caller should
    /// dismiss the picker after releasing its `Waku` update lease.
    pub(super) fn confirm_branch_picker_action(
        &mut self,
        actions: &[BranchPickerAction],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.branch_picker_mode == BranchPickerMode::Create {
            return self.confirm_branch_creation(cx);
        }
        let Some(action) = actions.get(self.branch_picker_highlight.unwrap_or(0)) else {
            return false;
        };
        match action {
            BranchPickerAction::Checkout(branch) => {
                self.choose_workspace_branch(branch.clone(), cx)
            }
            BranchPickerAction::Create => {
                self.begin_branch_creation(window, cx);
                false
            }
        }
    }

    /// `subject_session_id` is the workspace subject the operation ran for —
    /// the selection normally, the Big Picture subject while the overlay is
    /// open — so a worktree's persisted branch bookkeeping lands on the
    /// session the picker was describing, not whichever task sits
    /// underneath.
    fn start_branch_operation(
        &mut self,
        subject_session_id: Option<Uuid>,
        path: PathBuf,
        operation: BranchOperation,
        cx: &mut Context<Self>,
    ) {
        if self.branch_operation_pending {
            return;
        }
        let Some(workspace) = self.workspace_client_for_path(&path) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        self.branch_operation_pending = true;
        cx.notify();
        cx.spawn(async move |waku, cx| {
            let reset_base = match &operation {
                BranchOperation::Reset(base) => Some(base.clone()),
                _ => None,
            };
            let result = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move {
                        let request = match operation {
                            BranchOperation::Checkout(branch) => {
                                waku_client::WorkspaceOperation::CheckoutBranch {
                                    cwd: path,
                                    branch,
                                    create: false,
                                }
                            }
                            BranchOperation::Create(branch) => {
                                waku_client::WorkspaceOperation::CheckoutBranch {
                                    cwd: path,
                                    branch,
                                    create: true,
                                }
                            }
                            BranchOperation::Reset(base_ref) => {
                                waku_client::WorkspaceOperation::ResetWorktree { path, base_ref }
                            }
                        };
                        match workspace.request(request)? {
                            waku_client::WorkspaceResult::BranchChanged { snapshot } => {
                                Ok(snapshot)
                            }
                            _ => anyhow::bail!("the daemon returned an invalid branch response"),
                        }
                    }
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                waku.branch_operation_pending = false;
                match result {
                    Ok(snapshot) => {
                        let current = snapshot.current.clone();
                        waku.cache_sidebar_branch_label(&path, snapshot.display_branch());
                        waku.visible_branch_snapshot = Some((path.clone(), snapshot));
                        waku.branch_snapshots.invalidate(&path);
                        waku.invalidate_workspace_remote_files(&path);
                        // A worktree's persisted branch follows the checkout
                        // that just moved — for the subject session even
                        // when it is not the selection underneath.
                        if let Some(current) = current {
                            let mut persisted_branch_changed = false;
                            for session_id in [waku.state.selected_session, subject_session_id]
                                .into_iter()
                                .flatten()
                            {
                                if let Some(session) = waku.state.session_mut(session_id)
                                    && let SessionWorkspace::Worktree {
                                        branch,
                                        path: worktree_path,
                                        ..
                                    } = &mut session.workspace
                                    && worktree_path == &path
                                    && branch.as_deref() != Some(current.as_str())
                                {
                                    *branch = Some(current.clone());
                                    persisted_branch_changed = true;
                                }
                            }
                            if persisted_branch_changed {
                                waku.save();
                            }
                        }
                        // A draft's base pick re-pointed the worktree's
                        // detached HEAD: record the new base and drop any
                        // branch the worktree had been switched onto.
                        if let Some(base) = reset_base {
                            let mut base_changed = false;
                            let mut project_id = None;
                            for session_id in [waku.state.selected_session, subject_session_id]
                                .into_iter()
                                .flatten()
                            {
                                if let Some(session) = waku.state.session_mut(session_id)
                                    && let SessionWorkspace::Worktree {
                                        branch,
                                        base_branch,
                                        path: worktree_path,
                                        ..
                                    } = &mut session.workspace
                                    && worktree_path == &path
                                {
                                    project_id = Some(session.project_id);
                                    *branch = None;
                                    *base_branch = Some(base.clone());
                                    base_changed = true;
                                }
                            }
                            if base_changed {
                                if let Some(project_id) = project_id {
                                    waku.state.remember_workspace(
                                        project_id,
                                        &SessionWorkspace::NewWorktree {
                                            base_branch: Some(base),
                                        },
                                    );
                                }
                                waku.save();
                            }
                        }
                        let selected_path = waku
                            .selected_workspace_path()
                            .map(std::path::Path::to_path_buf);
                        if selected_path.as_ref() == Some(&path) {
                            waku.invalidate_workspace_queries(cx);
                            waku.reload_clean_right_panel_file_editors(cx);
                            waku.save();
                        }
                    }
                    Err(error) => {
                        waku.show_toast(tr!("errors.change_branch", error = error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(origin_url: Option<&str>, current: Option<&str>) -> BranchSnapshot {
        BranchSnapshot {
            repository: PathBuf::from("/repo"),
            current: current.map(str::to_owned),
            detached_head: None,
            default_branch: None,
            remote_default: None,
            origin_url: origin_url.map(str::to_owned),
            upstream: None,
            branches: Vec::new(),
            additions: 0,
            deletions: 0,
        }
    }

    #[test]
    fn github_remote_base_parses_common_remote_forms() {
        for remote in [
            "git@github.com:owner/repo.git",
            "git@github.com:owner/repo",
            "ssh://git@github.com/owner/repo.git",
            "ssh://git@github.com:22/owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo",
            "https://github.com/owner/repo/",
            "http://github.com/owner/repo",
            "git://github.com/owner/repo.git",
            "https://user@github.com/owner/repo.git",
        ] {
            assert_eq!(
                github_remote_base(remote).as_deref(),
                Some("https://github.com/owner/repo"),
                "{remote}"
            );
        }
    }

    #[test]
    fn github_remote_base_rejects_other_hosts_and_bad_input() {
        for remote in [
            "git@gitlab.com:owner/repo.git",
            "https://github.example.com/owner/repo",
            "https://github.com/owner",
            "",
            "not a url",
        ] {
            assert_eq!(github_remote_base(remote), None, "{remote}");
        }
    }

    #[test]
    fn github_urls_target_the_display_branch() {
        let github = snapshot(Some("git@github.com:owner/repo.git"), Some("feature/x"));
        assert_eq!(
            github_branch_url(&github).as_deref(),
            Some("https://github.com/owner/repo/tree/feature/x")
        );
        assert_eq!(
            github_file_url(&github, "src/a file.rs").as_deref(),
            Some("https://github.com/owner/repo/blob/feature/x/src/a%20file.rs")
        );

        let mut detached = snapshot(Some("git@github.com:owner/repo.git"), None);
        detached.detached_head = Some("abc1234".into());
        assert_eq!(
            github_branch_url(&detached).as_deref(),
            Some("https://github.com/owner/repo/tree/abc1234")
        );

        let non_github = snapshot(Some("git@gitlab.com:owner/repo.git"), Some("main"));
        assert_eq!(github_branch_url(&non_github), None);
        let no_branch = snapshot(Some("git@github.com:owner/repo.git"), None);
        assert_eq!(github_branch_url(&no_branch), None);
    }
}
