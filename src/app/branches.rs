use super::*;

enum BranchOperation {
    Checkout(String),
    Create(String),
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
fn github_url_path_encode(value: &str) -> String {
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
            .reset_with_uniform_height(rows.len(), px(BRANCH_PICKER_ROW_HEIGHT));
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
                let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
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
        self.branch_snapshots.invalidate(&path);
        cx.notify();
    }

    /// Select an existing branch. A planned worktree remembers it as the base
    /// ref without touching the ordinary checkout; concrete workspaces run a
    /// real `git switch` on the background executor.
    ///
    /// `true` asks the caller to dismiss the picker after this entity update
    /// ends. Closing sooner runs the toggle observer, which re-enters `Waku`
    /// and double-leases the entity.
    pub(super) fn choose_workspace_branch(
        &mut self,
        branch: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = self.selected_session() else {
            return false;
        };
        if session.is_busy() || self.branch_operation_pending {
            return false;
        }
        if matches!(session.workspace, SessionWorkspace::NewWorktree { .. }) {
            let project_id = session.project_id;
            let changed = self.selected_session_mut().is_some_and(|session| {
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

        let Some(path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
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
        self.start_branch_operation(path, BranchOperation::Checkout(branch), cx);
        true
    }

    pub(super) fn begin_branch_creation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.branch_operation_pending
            || self.selected_session().is_none_or(|session| {
                session.is_busy()
                    || matches!(session.workspace, SessionWorkspace::NewWorktree { .. })
            })
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
        let Some(path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            return false;
        };
        self.start_branch_operation(path, BranchOperation::Create(branch), cx);
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

    fn start_branch_operation(
        &mut self,
        path: PathBuf,
        operation: BranchOperation,
        cx: &mut Context<Self>,
    ) {
        if self.branch_operation_pending {
            return;
        }
        self.branch_operation_pending = true;
        cx.notify();
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move {
                        let (branch, create) = match operation {
                            BranchOperation::Checkout(branch) => (branch, false),
                            BranchOperation::Create(branch) => (branch, true),
                        };
                        match workspace.request(
                            waku_client::WorkspaceOperation::CheckoutBranch {
                                cwd: path,
                                branch,
                                create,
                            },
                        )? {
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
                        let selected_path = waku
                            .selected_workspace_path()
                            .map(std::path::Path::to_path_buf);
                        if selected_path.as_ref() == Some(&path) {
                            if let Some(current) = current
                                && let Some(session) = waku.selected_session_mut()
                                && let SessionWorkspace::Worktree { branch, .. } =
                                    &mut session.workspace
                            {
                                *branch = Some(current);
                            }
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
