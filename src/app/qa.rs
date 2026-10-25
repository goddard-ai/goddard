//! A QA session composes the normal chat, commit review, and terminal surfaces.
use super::*;
use gpui::{TitlebarOptions, WindowOptions};
use waku_client::persistence::QaSessionRecord;
use waku_client::workspace::{QaConfig, WorkspaceOperation, WorkspaceResult};

pub(super) struct QaSetup {
    project_id: Uuid,
    start_after_save: bool,
    dev: Entity<TextInput>,
    install: Entity<TextInput>,
    branch: Entity<TextInput>,
    pending: bool,
    error: Option<String>,
}

pub(super) struct QaWindow {
    pub record: QaSessionRecord,
    parent: WeakEntity<Waku>,
    terminal: Option<Uuid>,
    finishing: bool,
    completed: bool,
    clean: bool,
    error: Option<String>,
    check_error: Option<String>,
    status: Option<String>,
}

impl Waku {
    pub(super) fn qa_accepts_submission(&self, session_id: Uuid) -> bool {
        self.qa_window
            .as_ref()
            .is_none_or(|qa| qa.record.session_id == session_id && !qa.finishing && !qa.completed)
    }

    pub(super) fn begin_qa(
        &mut self,
        project_id: Uuid,
        edit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.qa_pending_projects.contains(&project_id) {
            self.show_toast(tr!("qa.preparing"));
            return;
        }
        if !edit {
            if let Some(record) = self
                .state
                .qa_sessions
                .values()
                .find(|r| r.project_id == project_id)
                .cloned()
            {
                self.resume_qa_window(record, cx);
                return;
            }
            if let Some(config) = self.state.qa_configs.get(&project_id).cloned() {
                self.prepare_qa(project_id, config, cx);
                return;
            }
        }
        let Some(project) = self.state.projects.iter().find(|p| p.id == project_id) else {
            return;
        };
        let cwd = project.path.clone();
        let preferred_branch = project
            .qa_branch
            .clone()
            .unwrap_or_else(|| self.state.qa_branch.clone());
        let saved = self.state.qa_configs.get(&project_id).cloned();
        let field = |label: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                TextInput::new(window, cx)
                    .tab_index(0)
                    .accessibility_label(label)
            })
        };
        let dev = field(tr!("qa.dev_command"), window, cx);
        let install = field(tr!("qa.install_command"), window, cx);
        let branch = field(tr!("qa.branch"), window, cx);
        window.focus(&dev.read(cx).focus_handle(cx), cx);
        self.qa_setup = Some(QaSetup {
            project_id,
            start_after_save: !edit,
            dev,
            install,
            branch,
            pending: true,
            error: None,
        });
        let client = self.daemon.client();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if let Some(config) = saved {
                        return Ok(config);
                    }
                    match waku_client::WorkspaceClient::new(client).request(
                        WorkspaceOperation::QaDefaults {
                            cwd,
                            preferred_branch,
                        },
                    )? {
                        WorkspaceResult::QaDefaults { config } => Ok(config),
                        _ => anyhow::bail!("unexpected QA setup response"),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(setup) = this
                    .qa_setup
                    .as_mut()
                    .filter(|s| s.project_id == project_id)
                else {
                    return;
                };
                setup.pending = false;
                match result {
                    Ok(config) => {
                        setup
                            .dev
                            .update(cx, |input, cx| input.set_content(config.dev_command, cx));
                        setup.install.update(cx, |input, cx| {
                            input.set_content(config.install_command, cx)
                        });
                        setup
                            .branch
                            .update(cx, |input, cx| input.set_content(config.branch, cx));
                    }
                    Err(error) => setup.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn confirm_qa_setup(&mut self, cx: &mut Context<Self>) {
        let Some(setup) = self.qa_setup.as_ref().filter(|s| !s.pending) else {
            return;
        };
        let project = setup.project_id;
        let start = setup.start_after_save;
        let config = QaConfig {
            dev_command: setup.dev.read(cx).content().trim().to_owned(),
            install_command: setup.install.read(cx).content().trim().to_owned(),
            branch: setup.branch.read(cx).content().trim().to_owned(),
        };
        if config.dev_command.is_empty() || config.branch.is_empty() {
            if let Some(setup) = &mut self.qa_setup {
                setup.error = Some(tr!("qa.required"));
            }
            cx.notify();
            return;
        }
        self.state.qa_configs.insert(project, config.clone());
        self.save();
        if start {
            self.prepare_qa(project, config, cx);
        } else {
            self.qa_setup = None;
            cx.notify();
        }
    }

    fn prepare_qa(&mut self, project_id: Uuid, config: QaConfig, cx: &mut Context<Self>) {
        let Some(project) = self.state.projects.iter().find(|p| p.id == project_id) else {
            return;
        };
        let cwd = project.path.clone();
        if !self.qa_pending_projects.insert(project_id) {
            return;
        }
        if let Some(setup) = &mut self.qa_setup {
            setup.pending = true;
            setup.error = None;
        }
        let daemon = self.daemon.clone();
        let branch = config.branch.clone();
        self.show_toast(tr!("qa.preparing"));
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let connection = daemon.independent_connection()?;
                    let checkout = match waku_client::WorkspaceClient::new(daemon.client())
                        .request(WorkspaceOperation::QaStart { cwd, branch })?
                    {
                        WorkspaceResult::QaStarted { checkout } => checkout,
                        _ => anyhow::bail!("unexpected QA checkout response"),
                    };
                    Ok::<_, anyhow::Error>((checkout, connection))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.qa_pending_projects.remove(&project_id);
                match result {
                    Ok((checkout, connection)) => {
                        let record = QaSessionRecord {
                            session_id: Uuid::new_v4(),
                            project_id,
                            checkout,
                            config,
                        };
                        this.state
                            .qa_sessions
                            .insert(record.session_id, record.clone());
                        this.save();
                        this.qa_setup = None;
                        this.open_qa_window(record, connection, cx);
                    }
                    Err(error) => {
                        if let Some(setup) = &mut this.qa_setup {
                            setup.pending = false;
                            setup.error = Some(format!("{error:#}"));
                        }
                        this.show_toast(format!("{error:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn resume_qa_window(&mut self, record: QaSessionRecord, cx: &mut Context<Self>) {
        if let Some(handle) = self.qa_windows.get(&record.session_id)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let project_id = record.project_id;
        if !self.qa_pending_projects.insert(project_id) {
            return;
        }
        let daemon = self.daemon.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { daemon.independent_connection() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.qa_pending_projects.remove(&project_id);
                match result {
                    Ok(connection) => this.open_qa_window(record, connection, cx),
                    Err(error) => this.show_toast(format!("{error:#}")),
                }
            });
        })
        .detach();
    }

    fn open_qa_window(
        &mut self,
        record: QaSessionRecord,
        daemon: waku_client::DaemonSupervisor,
        cx: &mut Context<Self>,
    ) {
        let id = record.session_id;
        let qa = QaWindow {
            record,
            parent: cx.entity().downgrade(),
            terminal: None,
            finishing: false,
            completed: false,
            clean: false,
            error: None,
            check_error: None,
            status: None,
        };
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        let result = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(tr!("qa.window_title").into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.0), px(600.0))),
                ..Default::default()
            },
            move |window, cx| Waku::new_window(window, cx, daemon, Some(qa)),
        );
        match result {
            Ok(handle) => {
                self.qa_windows.insert(id, handle);
            }
            Err(error) => self.show_toast(format!("{error:#}")),
        }
    }

    pub(super) fn initialize_qa_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(record) = self.qa_window.as_ref().map(|qa| qa.record.clone()) else {
            return;
        };
        let is_new = !self
            .state
            .sessions
            .iter()
            .any(|s| s.id == record.session_id);
        if is_new {
            let mut session = self
                .state
                .new_session(record.project_id, self.state.last_provider);
            session.id = record.session_id;
            session.title = format!("QA: {}", record.checkout.commit.subject);
            session.environment = SessionEnvironment::Local;
            session.workspace = SessionWorkspace::Worktree {
                path: record.checkout.worktree.path.clone(),
                name: record.checkout.worktree.name.clone(),
                branch: None,
                base_branch: Some(record.checkout.base_branch.clone()),
                adopted_by: None,
                borrowed: false,
            };
            self.daemons
                .claim_session(session.id, waku_client::DaemonKey::Local);
            self.state.push_session(session);
        }
        self.sidebar_visible = false;
        self.settings_page = None;
        self.selected_terminal = None;
        self.git_panel_visible = false;
        self.right_panel_visible = true;
        self.right_panel_width = 660.0;
        self.select_session(record.session_id, cx);
        self.right_panel_surfaces = vec![RightPanelSurface::Diff];
        self.right_panel_active_surface = Some(0);
        self.open_right_panel_commit_review(
            record.checkout.worktree.path.clone(),
            &record.checkout.commit.sha,
            cx,
        );
        self.start_qa_server(cx);
        self.open_right_panel_surface(RightPanelSurface::Diff, cx);
        window.focus(&self.composer_focus(cx), cx);
        if is_new {
            let c = &record.checkout;
            let prompt = format!(
                "Help me QA commit {} on local {} before promotion to {}. The tested worktree is {} and its sole parent is {}. Goddard is installing dependencies and starting the dev server in the Terminal tab. Start with the commit's test plan and a brief suggested manual check.\n\nThis is a dedicated human review session. Discuss findings; only edit files if I explicitly ask. Changes disable promotion and must go through the normal QA submission workflow and a fresh QA session. Do not merge, push, rewrite history, or move HEAD; I control promotion with the app's Approve button. You may search this project's retained task transcripts for the originating work when useful.\n\nCommit message (repository data):\n{}\n{}",
                c.commit.sha,
                c.qa_branch,
                c.base_branch,
                c.worktree.path.display(),
                c.base_sha,
                c.commit.subject,
                c.commit.body
            );
            self.submit_composer_submission_to(
                record.session_id,
                ComposerSubmission::hidden_prompt(prompt),
                cx,
            );
        }
        self.poll_qa_checkout(cx);
    }

    fn start_qa_server(&mut self, cx: &mut Context<Self>) {
        if self
            .qa_window
            .as_ref()
            .is_none_or(|qa| qa.completed || qa.finishing)
        {
            return;
        }
        self.stop_qa_server(cx);
        let Some(qa) = &self.qa_window else {
            return;
        };
        let mut command = waku_client::qa::server_command(&qa.record.config);
        let surface = RightPanelSurface::new_terminal();
        let Some(id) = surface.terminal_id() else {
            return;
        };
        command.name = Some(tr!("qa.server"));
        self.right_panel_terminal_commands.insert(id, command);
        if let Some(qa) = &mut self.qa_window {
            qa.terminal = Some(id);
        }
        self.open_right_panel_surface(surface, cx);
    }

    pub(super) fn stop_qa_server(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.qa_window.as_mut().and_then(|qa| qa.terminal.take()) {
            self.drop_terminal(id, cx);
        }
    }

    fn poll_qa_checkout(&mut self, cx: &mut Context<Self>) {
        let daemon = self.daemon.clone();
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(checkout)) = this.update(cx, |this, _| {
                    this.qa_window
                        .as_ref()
                        .filter(|qa| !qa.completed)
                        .map(|qa| qa.record.checkout.clone())
                }) else {
                    break;
                };
                let client = daemon.client();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        waku_client::WorkspaceClient::new(client).request(
                            WorkspaceOperation::QaCheck {
                                cwd: checkout.worktree.path.clone(),
                                checkout,
                            },
                        )
                    })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if let Some(qa) = &mut this.qa_window {
                            let clean = result.is_ok();
                            let error = result.err().map(|e| format!("{e:#}"));
                            if !qa.finishing
                                && !qa.completed
                                && (clean != qa.clean || error != qa.check_error)
                            {
                                qa.clean = clean;
                                qa.check_error = error;
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
            }
        })
        .detach();
    }

    fn promote_qa(&mut self, cx: &mut Context<Self>) {
        if self.selected_session().is_some_and(|s| s.is_busy()) {
            return;
        }
        let Some(qa) = self
            .qa_window
            .as_mut()
            .filter(|q| q.clean && !q.finishing && !q.completed)
        else {
            return;
        };
        qa.finishing = true;
        qa.error = None;
        let record = qa.record.clone();
        let parent = qa.parent.clone();
        let client = self.daemon.client();
        self.stop_qa_server(cx);
        cx.spawn(async move |this, cx| {
            let checkout = record.checkout.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    waku_client::WorkspaceClient::new(client.clone()).request(
                        WorkspaceOperation::QaPromote {
                            cwd: checkout.worktree.path.clone(),
                            checkout: checkout.clone(),
                        },
                    )?;
                    // Git refuses dirty removal. Preserve the checkout on any cleanup failure.
                    let workspace = waku_client::WorkspaceClient::new(client);
                    let cleanup = workspace
                        .request(WorkspaceOperation::QaCheck {
                            cwd: checkout.worktree.path.clone(),
                            checkout: checkout.clone(),
                        })
                        .and_then(|_| {
                            workspace.request(WorkspaceOperation::RemoveWorktree {
                                path: checkout.worktree.path.clone(),
                                force: false,
                            })
                        });
                    Ok::<_, anyhow::Error>(cleanup.err().map(|e| {
                        format!(
                            "Checkout kept at {}: {e:#}",
                            checkout.worktree.path.display()
                        )
                    }))
                })
                .await;
            let completed = result.is_ok();
            let _ = this.update(cx, |this, cx| {
                if completed {
                    this.stop_qa_server(cx);
                }
                if let Some(qa) = &mut this.qa_window {
                    qa.finishing = false;
                    match result {
                        Ok(cleanup_error) => {
                            qa.completed = true;
                            qa.check_error = None;
                            qa.status =
                                Some(tr!("qa.promoted", branch = qa.record.checkout.base_branch));
                            qa.error = cleanup_error;
                        }
                        Err(error) => qa.error = Some(format!("{error:#}")),
                    }
                }
                cx.notify();
            });
            if completed {
                let _ = parent.update(cx, |parent, _| {
                    parent.state.qa_sessions.remove(&record.session_id);
                    parent.save();
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn discard_qa(&mut self, cx: &mut Context<Self>) {
        if self.selected_session().is_some_and(|s| s.is_busy()) {
            self.show_toast(tr!("qa.wait_agent"));
            return;
        }
        let Some(qa) = self
            .qa_window
            .as_mut()
            .filter(|q| !q.finishing && !q.completed)
        else {
            return;
        };
        qa.finishing = true;
        qa.error = None;
        let record = qa.record.clone();
        let parent = qa.parent.clone();
        self.stop_qa_server(cx);
        let client = self.daemon.client();
        cx.spawn(async move |this, cx| {
            let retained = cx
                .background_executor()
                .spawn(async move {
                    let workspace = waku_client::WorkspaceClient::new(client);
                    let checkout = record.checkout;
                    // Ending a review never loses corrections: keep a changed
                    // checkout and its conversation, and allow the next review.
                    workspace
                        .request(WorkspaceOperation::QaCheck {
                            cwd: checkout.worktree.path.clone(),
                            checkout: checkout.clone(),
                        })
                        .and_then(|_| {
                            workspace.request(WorkspaceOperation::RemoveWorktree {
                                path: checkout.worktree.path.clone(),
                                force: false,
                            })
                        })
                        .err()
                        .map(|e| {
                            format!(
                                "Checkout kept at {}: {e:#}",
                                checkout.worktree.path.display()
                            )
                        })
                })
                .await;
            let _ = parent.update(cx, |parent, _| {
                parent.state.qa_sessions.remove(&record.session_id);
                parent.save();
            });
            let _ = this.update(cx, |this, cx| {
                if let Some(qa) = &mut this.qa_window {
                    qa.finishing = false;
                    qa.completed = true;
                    qa.check_error = None;
                    qa.error = retained;
                    qa.status = Some(tr!("qa.discarded"));
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn qa_server_status(&self, cx: &App) -> String {
        let terminal = self
            .qa_window
            .as_ref()
            .and_then(|qa| qa.terminal)
            .and_then(|id| self.right_panel_terminals.get(&id));
        match terminal {
            Some(view) if view.read(cx).command_running() => tr!("qa.command_running"),
            Some(view) => match view.read(cx).last_command_exit() {
                Some(code) => tr!("qa.command_exited", code = code),
                None => tr!("qa.command_stopped"),
            },
            None => tr!("qa.command_stopped"),
        }
    }

    pub(super) fn render_qa_setup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let setup = self.qa_setup.as_ref()?;
        let theme = Theme::current(cx);
        let name = self
            .state
            .projects
            .iter()
            .find(|p| p.id == setup.project_id)
            .map(|p| p.display_name())
            .unwrap_or_default();
        let field = |id: &'static str, label: String, input: Entity<TextInput>| {
            div()
                .w_full()
                .px(px(20.0))
                .py(px(16.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .gap(px(20.0))
                .child(div().w(px(180.0)).text_size(sp(13.0)).child(label))
                .child(TextField::new(id, input).flex_1())
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.35))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("qa-setup")
                        .tab_group()
                        .w(px(680.0))
                        .max_w_full()
                        .p(px(28.0))
                        .rounded(px(20.0))
                        .bg(theme.surface)
                        .text_color(theme.text)
                        .flex()
                        .flex_col()
                        .gap(px(20.0))
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape"
                                && this.qa_setup.as_ref().is_some_and(|s| !s.pending)
                            {
                                this.qa_setup = None;
                                window.focus(&this.composer_focus(cx), cx);
                                cx.notify();
                                cx.stop_propagation();
                            }
                        }))
                        .child(div().text_size(sp(20.0)).child(tr!("qa.setup")))
                        .child(
                            div()
                                .text_size(sp(13.0))
                                .text_color(theme.text_secondary)
                                .child(name),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(field("qa-dev", tr!("qa.dev_command"), setup.dev.clone()))
                                .child(field(
                                    "qa-install",
                                    tr!("qa.install_optional"),
                                    setup.install.clone(),
                                ))
                                .child(field("qa-branch", tr!("qa.branch"), setup.branch.clone())),
                        )
                        .children(setup.error.as_ref().map(|error| {
                            div()
                                .text_size(sp(12.0))
                                .text_color(theme.danger)
                                .child(error.clone())
                        }))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(8.0))
                                .child(settings::settings_button(
                                    "qa-cancel",
                                    tr!("common.cancel"),
                                    !setup.pending,
                                    false,
                                    false,
                                    theme,
                                    cx,
                                    |this, window, cx| {
                                        this.qa_setup = None;
                                        window.focus(&this.composer_focus(cx), cx);
                                        cx.notify();
                                    },
                                ))
                                .child(settings::settings_button(
                                    "qa-start",
                                    if setup.pending {
                                        tr!("qa.preparing")
                                    } else if !setup.start_after_save {
                                        tr!("common.save")
                                    } else {
                                        tr!("qa.start")
                                    },
                                    !setup.pending,
                                    false,
                                    false,
                                    theme,
                                    cx,
                                    |this, _, cx| this.confirm_qa_setup(cx),
                                )),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_qa_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_browser_webviews(cx);
        let theme = Theme::current(cx);
        let qa = self.qa_window.as_ref().expect("QA window renderer");
        let title = qa.record.checkout.commit.subject.clone();
        let branch = qa.record.checkout.base_branch.clone();
        let id = qa.record.session_id;
        let project_id = qa.record.project_id;
        let parent = qa.parent.clone();
        let completed = qa.completed;
        let enabled = qa.clean
            && !qa.finishing
            && !completed
            && !self.selected_session().is_some_and(|s| s.is_busy());
        let label = if completed {
            tr!("qa.next")
        } else if qa.finishing {
            tr!("qa.pushing")
        } else {
            tr!("qa.approve", branch = branch)
        };
        let error = qa.error.clone().or_else(|| qa.check_error.clone());
        let status = qa.status.clone();
        let accepts_submission = self.qa_accepts_submission(id);
        let detail = format!(
            "{} · {}",
            self.state
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .map(|p| p.display_name())
                .unwrap_or_default(),
            qa.record
                .checkout
                .commit
                .sha
                .chars()
                .take(8)
                .collect::<String>()
        );
        let header = div()
            .flex_none()
            .px(px(24.0))
            .py(px(16.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(div().text_size(sp(14.0)).truncate().child(title))
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(detail),
                    ),
            )
            .child(dropdown_menu(
                MenuChip::new("qa-actions").label(tr!("qa.actions")),
                "qa-actions-menu",
                &self.menu_handle("qa-actions-menu", cx),
                MenuAlign::BelowRight,
                {
                    let weak = cx.entity().downgrade();
                    let disabled = completed || qa.finishing;
                    move |_| {
                        let restart = weak.clone();
                        let discard = weak.clone();
                        vec![
                            MenuItem::new(tr!("qa.restart_server"), move |_, cx| {
                                let _ = restart.update(cx, |this, cx| this.start_qa_server(cx));
                            })
                            .disabled(disabled),
                            MenuItem::new(tr!("qa.discard"), move |_, cx| {
                                let _ = discard.update(cx, |this, cx| this.discard_qa(cx));
                            })
                            .disabled(disabled),
                        ]
                    }
                },
            ))
            .child(
                settings::settings_button(
                    "qa-approve",
                    label,
                    enabled || completed,
                    false,
                    false,
                    theme,
                    cx,
                    move |this, window, cx| {
                        if completed {
                            let _ = parent.update(cx, |parent, cx| {
                                parent.qa_windows.remove(&id);
                                if let Some(config) =
                                    parent.state.qa_configs.get(&project_id).cloned()
                                {
                                    parent.prepare_qa(project_id, config, cx);
                                }
                            });
                            window.remove_window();
                        } else {
                            this.promote_qa(cx);
                        }
                    },
                )
                .bg(theme.accent)
                .text_color(theme.surface),
            );
        let transcript = self.transcript_pane.clone();
        let panel = self.right_panel_pane.clone();
        let content = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .text_color(theme.text)
            .font_family(crate::fonts::current(cx).ui)
            .key_context("Workspace ComposerExists")
            .on_action(cx.listener(Self::quit_action))
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                this.request_window_close(window, cx)
            }))
            .on_action(cx.listener(Self::focus_composer_action))
            .on_action(cx.listener(Self::focus_terminal_action))
            .on_action(cx.listener(Self::cancel_turn_action))
            .on_action(cx.listener(Self::copy_selection_action))
            .on_action(cx.listener(Self::add_to_chat_action))
            .on_action(cx.listener(Self::toggle_model_picker_action))
            .on_action(cx.listener(Self::step_model_picker_backward_action))
            .on_action(cx.listener(Self::select_favorite_model_action))
            .on_action(cx.listener(Self::cycle_reasoning_effort_action))
            .on_action(cx.listener(Self::cycle_favorite_model_action))
            .on_action(cx.listener(Self::toggle_runtime_mode_picker_action))
            .on_action(cx.listener(Self::adjust_font_size_action))
            .on_action(cx.listener(Self::open_localhost_url_action))
            .on_action(cx.listener(Self::open_localhost_url_in_tab_action))
            .on_action(cx.listener(Self::save_right_panel_file_action))
            .on_action(cx.listener(Self::open_find_action))
            .on_action(cx.listener(Self::close_find_action))
            .on_action(cx.listener(Self::find_next_action))
            .on_action(cx.listener(Self::find_previous_action))
            .on_key_down(cx.listener(Self::enter_outside_composer))
            .on_key_down(cx.listener(Self::type_to_focus_composer))
            .on_key_down(cx.listener(Self::paste_into_composer))
            .on_modifiers_changed(cx.listener(Self::press_to_talk_modifiers_changed))
            .on_key_up(cx.listener(Self::press_to_talk_key_up))
            .on_mouse_move(cx.listener(Self::resize_panel_mouse_move))
            .capture_any_mouse_up(cx.listener(Self::finish_panel_resize))
            .child(header)
            .children(status.map(|status| {
                div()
                    .px(px(24.0))
                    .pb(px(8.0))
                    .text_size(sp(12.0))
                    .child(status)
            }))
            .children(error.map(|error| {
                div()
                    .px(px(24.0))
                    .pb(px(8.0))
                    .text_size(sp(12.0))
                    .text_color(theme.danger)
                    .child(error)
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(transcript.cached(StyleRefinement::default().flex_1().min_h_0()))
                            .children(self.render_queued_messages(cx))
                            .when(accepts_submission, |element| {
                                element.child(self.render_composer(window, cx))
                            }),
                    )
                    .child(
                        panel.cached(
                            StyleRefinement::default()
                                .w(px(self.right_panel_content_width))
                                .h_full(),
                        ),
                    ),
            )
            .children(self.render_full_access_dialog(cx))
            .children(self.render_close_dialog(cx))
            .children(self.render_daemon_restart_dialog(cx))
            .children(self.render_image_preview(cx))
            .children(self.render_computer_use_overlay(window, cx))
            .children(self.render_provider_switch_dialog(cx))
            .children(self.render_terminal_close_dialog(cx))
            .children(self.render_active_toast(window, cx));
        self.render_window_frame(content.into_any_element(), window, cx)
    }
}
