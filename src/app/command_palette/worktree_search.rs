use super::*;

#[derive(Default)]
pub(super) struct WorktreeSearch {
    project: Option<Uuid>,
    current: Option<PathBuf>,
    pub(super) path: Option<PathBuf>,
    entries: Vec<RepoWorktree>,
    pub(super) pending: bool,
    pub(super) error: Option<String>,
    pub(super) generation: u64,
    pub(super) archived: bool,
    history: Vec<(CommandPaletteView, String, Option<PaletteAction>)>,
}

fn task_in_worktree(session: &AgentSession, project: Uuid, checkout: &Path, path: &Path) -> bool {
    session.project_id == project
        && !session.boss_managed
        && !session.is_side_chat()
        && match &session.workspace {
            SessionWorkspace::Local => checkout == path,
            SessionWorkspace::Worktree {
                path: workspace, ..
            } => workspace == path,
            SessionWorkspace::NewWorktree { .. } => false,
        }
}

fn task_status_label(status: SessionStatus) -> String {
    match status {
        SessionStatus::Idle => tr!("command_palette.idle_task"),
        SessionStatus::Connecting => tr!("daemon.phase_connecting"),
        SessionStatus::Working => tr!("sidebar.status_working"),
        SessionStatus::Waiting => tr!("sidebar.status_waiting"),
        SessionStatus::Background => tr!("sidebar.status_background"),
        SessionStatus::Failed => tr!("sidebar.status_failed"),
    }
}

// Activity is task activity, never filesystem mtime or the age of HEAD's commit.
fn worktree_order(
    a: (&Path, Option<u64>),
    b: (&Path, Option<u64>),
    current: Option<&Path>,
) -> std::cmp::Ordering {
    (Some(b.0) == current)
        .cmp(&(Some(a.0) == current))
        .then(b.1.cmp(&a.1))
        .then_with(|| {
            worktrees::existing_worktree_name(a.0).cmp(&worktrees::existing_worktree_name(b.0))
        })
        .then_with(|| a.0.cmp(b.0))
}

impl Waku {
    pub(super) fn remember_worktree_search(&mut self, cx: &App) {
        self.command_palette.worktree_search.history.push((
            self.command_palette.view,
            self.command_palette.search.read(cx).content().to_owned(),
            self.command_palette
                .results
                .get(self.command_palette.selected)
                .map(|row| row.action.clone()),
        ));
    }

    pub(super) fn worktree_search_placeholder(&self) -> String {
        let search = &self.command_palette.worktree_search;
        let project = search
            .project
            .and_then(|id| self.state.projects.iter().find(|p| p.id == id));
        match self.command_palette.view {
            CommandPaletteView::WorktreeSearch => format!(
                "{} · {}",
                project.map(|p| p.display_name()).unwrap_or_default(),
                tr!("command_palette.run_script_worktree_placeholder")
            ),
            CommandPaletteView::WorktreeTasks => format!(
                "{} · {} · {}",
                project.map(|p| p.display_name()).unwrap_or_default(),
                search
                    .path
                    .as_deref()
                    .map(worktrees::existing_worktree_name)
                    .unwrap_or_default(),
                tr!("command_palette.worktree_task_placeholder")
            ),
            _ => tr!("command_palette.run_script_project_placeholder"),
        }
    }

    pub(super) fn set_worktree_search_view(
        &mut self,
        view: CommandPaletteView,
        cx: &mut Context<Self>,
    ) {
        self.command_palette.view = view;
        let placeholder = self.worktree_search_placeholder();
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
            input.clear(cx);
        });
        self.refresh_worktree_search("", false);
        cx.notify();
    }

    pub(super) fn start_worktree_search(&mut self, cx: &mut Context<Self>) {
        let generation = self
            .command_palette
            .worktree_search
            .generation
            .wrapping_add(1);
        self.command_palette.worktree_search = WorktreeSearch {
            generation,
            ..Default::default()
        };
        self.remember_worktree_search(cx);
        // A remembered task behind a page is not ambient context for that page.
        let target = if self.big_picture.is_open() {
            let (_, project) = self.workspace_subject();
            project.map(|id| (id, self.workspace_subject_path()))
        } else if self.settings_page.is_none()
            && matches!(
                self.main_screen(self.live_deliverable_page().is_some()),
                render::MainScreen::Session | render::MainScreen::Terminal(_)
            )
        {
            self.infer_issue_target(cx)
                .and_then(|target| target.project.map(|id| (id, Some(target.cwd))))
        } else {
            None
        };
        let target = target
            .or_else(|| self.focused_project_id().map(|id| (id, None)))
            .filter(|(id, _)| {
                self.state
                    .projects
                    .iter()
                    .any(|p| p.id == *id && !p.is_projectless())
            });
        if let Some((project, current)) = target {
            // Even an inferred project can be changed with Esc or the final row.
            self.command_palette.worktree_search.history.push((
                CommandPaletteView::WorktreeProjects,
                String::new(),
                Some(PaletteAction::SearchWorktreeProject(project)),
            ));
            self.command_palette.worktree_search.current = current;
            self.load_search_worktrees(project, cx);
        } else {
            self.set_worktree_search_view(CommandPaletteView::WorktreeProjects, cx);
        }
    }

    pub(super) fn back_worktree_search(&mut self, cx: &mut Context<Self>) {
        let Some((view, query, action)) = self.command_palette.worktree_search.history.pop() else {
            self.leave_command_palette_drill_in_view(cx);
            return;
        };
        if self.command_palette.view == CommandPaletteView::WorktreeSearch {
            let search = &mut self.command_palette.worktree_search;
            search.generation = search.generation.wrapping_add(1);
            search.pending = false;
        }
        self.command_palette.view = view;
        let placeholder = if view == CommandPaletteView::Commands {
            tr!("command_palette.placeholder")
        } else {
            self.worktree_search_placeholder()
        };
        self.command_palette.search.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
            input.set_content(query.clone(), cx);
        });
        self.refresh_command_palette_results(&query, false, cx);
        self.finish_drill_in_refresh(action, None);
        cx.notify();
    }

    pub(super) fn load_search_worktrees(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        let Some(project) = self.state.projects.iter().find(|p| p.id == project_id) else {
            return;
        };
        let cwd = project.path.clone();
        let search = &mut self.command_palette.worktree_search;
        search.project = Some(project_id);
        search.entries.clear();
        search.path = None;
        search.archived = false;
        search.error = None;
        search.pending = true;
        search.generation = search.generation.wrapping_add(1);
        let generation = search.generation;
        self.set_worktree_search_view(CommandPaletteView::WorktreeSearch, cx);
        let Some(workspace) = self.workspace_client_for_project(project_id) else {
            self.command_palette.worktree_search.pending = false;
            self.command_palette.worktree_search.error = Some(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace.request(waku_client::WorkspaceOperation::ListWorktrees {
                        cwd,
                        skip_status: true,
                    })? {
                        waku_client::WorkspaceResult::RepoWorktrees {
                            entries: Some(entries),
                        } => Ok(entries),
                        _ => anyhow::bail!("Could not load worktrees"),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.command_palette.open
                    || this.command_palette.worktree_search.generation != generation
                {
                    return;
                }
                let search = &mut this.command_palette.worktree_search;
                search.pending = false;
                match result {
                    Ok(entries) => search.entries = entries,
                    Err(error) => search.error = Some(error.to_string()),
                }
                let query = this.command_palette.search.read(cx).content().to_owned();
                this.refresh_worktree_search(&query, true);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn refresh_worktree_search(&mut self, query: &str, preserve: bool) {
        let selected = preserve
            .then(|| {
                self.command_palette
                    .results
                    .get(self.command_palette.selected)
                    .map(|r| r.action.clone())
            })
            .flatten();
        let view = self.command_palette.view;
        let search = &self.command_palette.worktree_search;
        let mut candidates = Vec::new();
        if view == CommandPaletteView::WorktreeProjects {
            let mut projects = self
                .state
                .projects
                .iter()
                .filter(|p| !p.is_projectless())
                .collect::<Vec<_>>();
            let mut recent = HashMap::new();
            for session in &self.state.sessions {
                recent
                    .entry(session.project_id)
                    .and_modify(|at: &mut u64| *at = (*at).max(session.updated_at))
                    .or_insert(session.updated_at);
            }
            projects.sort_by(|a, b| {
                recent
                    .get(&b.id)
                    .cmp(&recent.get(&a.id))
                    .then_with(|| a.display_name().cmp(&b.display_name()))
                    .then(a.id.cmp(&b.id))
            });
            for project in projects {
                let mut row = CommandPaletteItem::command(
                    PaletteSection::Projects,
                    project.display_name(),
                    "icons/folder.svg",
                    None,
                    PaletteAction::SearchWorktreeProject(project.id),
                    "",
                    candidates.len(),
                );
                row.search_text
                    .push_str(&format!(" {}", project.path.display()));
                row.detail = Some(settings::abbreviate_home_path(
                    &project.path,
                    self.home_directory.as_deref(),
                ));
                candidates.push(row);
            }
        } else if let Some(project) = search
            .project
            .and_then(|id| self.state.projects.iter().find(|p| p.id == id))
        {
            if view == CommandPaletteView::WorktreeSearch {
                let mut activity = HashMap::<PathBuf, (usize, u64)>::new();
                for session in &self.state.sessions {
                    if session.project_id != project.id
                        || session.boss_managed
                        || session.is_side_chat()
                    {
                        continue;
                    }
                    let path = match &session.workspace {
                        SessionWorkspace::Local => &project.path,
                        SessionWorkspace::Worktree { path, .. } => path,
                        SessionWorkspace::NewWorktree { .. } => continue,
                    };
                    let entry = activity.entry(path.clone()).or_default();
                    entry.0 += 1;
                    entry.1 = entry.1.max(session.updated_at);
                }
                let mut entries = search.entries.iter().collect::<Vec<_>>();
                entries.sort_by(|a, b| {
                    worktree_order(
                        (&a.path, activity.get(&a.path).map(|a| a.1)),
                        (&b.path, activity.get(&b.path).map(|a| a.1)),
                        search.current.as_deref(),
                    )
                });
                for entry in entries {
                    let name = worktrees::existing_worktree_name(&entry.path);
                    let label = if entry.is_main {
                        format!("{name} · {}", tr!("command_palette.main_checkout"))
                    } else {
                        name
                    };
                    let mut row = CommandPaletteItem::command(
                        PaletteSection::Worktrees,
                        label,
                        "icons/folder.svg",
                        None,
                        PaletteAction::SearchWorktree(entry.path.clone()),
                        "",
                        candidates.len(),
                    );
                    row.search_text.push_str(&format!(
                        " {} {}",
                        entry.branch.as_deref().unwrap_or_default(),
                        entry.path.display()
                    ));
                    let (count, at) = activity.get(&entry.path).copied().unwrap_or_default();
                    let age = if count == 0 {
                        String::new()
                    } else {
                        format!(
                            " · {}",
                            sidebar::format_time_ago(unix_time().saturating_sub(at))
                        )
                    };
                    row.detail = Some(format!(
                        "{} · {} · {}{age}{}",
                        entry.branch.as_deref().unwrap_or("HEAD"),
                        settings::abbreviate_home_path(&entry.path, self.home_directory.as_deref()),
                        tr!("command_palette.worktree_task_count", count = count),
                        if search.current.as_ref() == Some(&entry.path) {
                            format!(" · {}", tr!("command_palette.current"))
                        } else {
                            String::new()
                        }
                    ));
                    candidates.push(row);
                }
            } else if let Some(path) = &search.path {
                let mut tasks = self
                    .state
                    .sessions
                    .iter()
                    .filter(|s| {
                        task_in_worktree(s, project.id, &project.path, path)
                            && (search.archived || s.archived_at.is_none())
                    })
                    .collect::<Vec<_>>();
                tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
                for task in tasks {
                    let mut row = CommandPaletteItem::command(
                        PaletteSection::Tasks,
                        task.display_title().to_owned(),
                        "icons/message-square.svg",
                        None,
                        PaletteAction::SelectTask(task.id),
                        "",
                        candidates.len(),
                    );
                    row.detail = Some(format!(
                        "{} · {} · {}{}",
                        task.provider.display_name(),
                        task_status_label(task.status),
                        sidebar::format_time_ago(unix_time().saturating_sub(task.updated_at)),
                        if task.archived_at.is_some() {
                            format!(" · {}", tr!("command_palette.archived_task"))
                        } else {
                            String::new()
                        }
                    ));
                    candidates.push(row);
                }
            }
        }
        if !query.trim().is_empty() {
            if view == CommandPaletteView::WorktreeTasks {
                // Filtering preserves the promised newest-updated task order.
                candidates = self.score_run_script_items(candidates, query);
                candidates.sort_by_key(|r| r.order);
            } else {
                candidates = self.score_run_script_items(candidates, query);
            }
        }
        if view == CommandPaletteView::WorktreeTasks {
            candidates.insert(
                0,
                CommandPaletteItem::command(
                    PaletteSection::Tasks,
                    tr!("command_palette.new_task_in_worktree"),
                    "icons/plus.svg",
                    None,
                    PaletteAction::WorktreeNewTask,
                    "",
                    0,
                ),
            );
            candidates.push(CommandPaletteItem::command(
                PaletteSection::Tasks,
                if self.command_palette.worktree_search.archived {
                    tr!("command_palette.hide_archived_tasks")
                } else {
                    tr!("command_palette.show_archived_tasks")
                },
                "icons/archive.svg",
                None,
                PaletteAction::WorktreeToggleArchived,
                "",
                usize::MAX,
            ));
        }
        if view == CommandPaletteView::WorktreeSearch {
            candidates.push(CommandPaletteItem::command(
                PaletteSection::Projects,
                tr!("command_palette.worktree_change_project"),
                "icons/folder.svg",
                None,
                PaletteAction::WorktreeChangeProject,
                "",
                usize::MAX,
            ));
        }
        self.command_palette.results = candidates;
        // Opening an existing task is the common path; creation stays at a fixed row.
        let selected = selected.or_else(|| {
            (view == CommandPaletteView::WorktreeTasks)
                .then(|| {
                    self.command_palette
                        .results
                        .iter()
                        .find(|r| matches!(r.action, PaletteAction::SelectTask(_)))
                        .map(|r| r.action.clone())
                })
                .flatten()
        });
        self.finish_drill_in_refresh(selected, None);
    }

    pub(super) fn create_task_in_search_worktree(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search = &self.command_palette.worktree_search;
        let Some(project_id) = search.project else {
            return;
        };
        let Some(entry) = search
            .entries
            .iter()
            .find(|e| Some(&e.path) == search.path.as_ref())
        else {
            return;
        };
        let workspace = if self
            .state
            .projects
            .iter()
            .any(|p| p.id == project_id && p.path == entry.path)
        {
            SessionWorkspace::Local
        } else {
            SessionWorkspace::Worktree {
                path: entry.path.clone(),
                name: worktrees::existing_worktree_name(&entry.path),
                branch: entry.branch.clone(),
                base_branch: None,
                adopted_by: None,
                borrowed: false,
            }
        };
        self.close_command_palette(window, cx);
        self.settings_page = None;
        self.close_projects_page(cx);
        self.bind_new_draft_to_worktree(project_id, workspace, false, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_membership_uses_project_and_exact_materialized_checkout() {
        let project = Uuid::new_v4();
        let checkout = Path::new("/repo");
        let worktree = Path::new("/worktrees/repo/feature/repo");
        let mut task = AgentSession::new(project, ProviderKind::Codex);
        assert!(task_in_worktree(&task, project, checkout, checkout));
        assert!(!task_in_worktree(&task, project, checkout, worktree));
        task.workspace = SessionWorkspace::Worktree {
            path: worktree.to_owned(),
            name: "feature".into(),
            branch: None,
            base_branch: None,
            adopted_by: None,
            borrowed: false,
        };
        assert!(task_in_worktree(&task, project, checkout, worktree));
        assert!(!task_in_worktree(&task, Uuid::new_v4(), checkout, worktree));
        assert!(!task_in_worktree(&task, project, checkout, checkout));
        task.workspace = SessionWorkspace::NewWorktree { base_branch: None };
        assert!(!task_in_worktree(&task, project, checkout, checkout));
        task.workspace = SessionWorkspace::Local;
        task.boss_managed = true;
        assert!(!task_in_worktree(&task, project, checkout, checkout));
        task.boss_managed = false;
        task.side_chat_of = Some(Uuid::new_v4());
        assert!(!task_in_worktree(&task, project, checkout, checkout));
    }

    #[test]
    fn worktrees_rank_current_then_task_activity_then_name() {
        let current = Path::new("/repo/current");
        let mut entries = vec![
            (Path::new("/repo/zeta"), None),
            (Path::new("/repo/old"), Some(10)),
            (Path::new("/repo/new"), Some(20)),
            (current, None),
            (Path::new("/repo/alpha"), None),
        ];
        entries.sort_by(|a, b| worktree_order(*a, *b, Some(current)));
        assert_eq!(
            entries
                .iter()
                .map(|e| e.0.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            ["current", "new", "old", "alpha", "zeta"]
        );
    }
}
