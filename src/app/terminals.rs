use super::*;

/// One entry in the sidebar's Terminals group. `session` scopes the
/// terminal to the task whose right-panel surfaces own it; `None` is a
/// global terminal that exists only in the group.
pub(super) struct TerminalRecord {
    pub session: Option<Uuid>,
    /// Pinned terminals keep a sidebar row while the group is collapsed.
    pub pinned: bool,
    /// Where the PTY starts — resolved at creation from the owning
    /// session's workspace or the requested directory for global ones.
    pub working_directory: Option<PathBuf>,
    /// When the terminal opened (unix seconds) — the row's "…ago" label
    /// until the view reports a command's start.
    pub opened_at: u64,
}

/// A terminal row is a single line: title, location.
pub(super) const SIDEBAR_TERMINAL_ROW_HEIGHT: f32 = 30.0;
const SIDEBAR_TERMINAL_ROW_GAP: f32 = 1.0;

/// The nearest enclosing repository's root — `.git` may be a file in a
/// linked worktree, so existence rather than `is_dir` is the test.
fn nearest_repo_root(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Character budget for a row's location label before ancestors fold
/// under `…/` — beyond it a layout-level tail clip would saw through a
/// component's middle.
const SIDEBAR_TERMINAL_DETAIL_MAX_CHARS: usize = 32;

/// Shorten a slash-separated label by popping whole ancestors, never
/// cutting through a component: `~/a/b/c` becomes `…/b/c`, then `…/c`.
/// With `keep_first`, the leading component — a repository name — stays
/// put and the popped middle folds as `repo/…/leaf` instead.
fn truncate_path_ancestors(path: &str, max_chars: usize, keep_first: bool) -> String {
    if path.chars().count() <= max_chars {
        return path.to_owned();
    }
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.len() <= 1 {
        return path.to_owned();
    }
    if keep_first {
        // The leading component anchors; the `…` stands for the middle
        // components between it and the leaf, so it needs both to exist.
        for dropped in 2..components.len() {
            let candidate = format!("{}/…/{}", components[0], components[dropped..].join("/"));
            if candidate.chars().count() <= max_chars {
                return candidate;
            }
        }
        return if components.len() > 2 {
            format!("{}/…/{}", components[0], components[components.len() - 1])
        } else {
            path.to_owned()
        };
    }
    for dropped in 1..components.len() {
        let candidate = format!("…/{}", components[dropped..].join("/"));
        if candidate.chars().count() <= max_chars {
            return candidate;
        }
    }
    format!("…/{}", components[components.len() - 1])
}

impl Waku {
    /// Terminal ids in creation order — the group's flat listing.
    pub(super) fn sidebar_terminal_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.terminal_order
            .iter()
            .copied()
            .filter(|id| self.terminal_records.contains_key(id))
    }

    /// The directory a row reports — the live PTY cwd once the view is up,
    /// the recorded spawn directory otherwise.
    fn terminal_cwd(&self, terminal_id: Uuid, cx: &App) -> Option<PathBuf> {
        self.right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).working_directory().to_path_buf())
            .or_else(|| {
                self.terminal_records
                    .get(&terminal_id)
                    .and_then(|record| record.working_directory.clone())
            })
    }

    /// Resolve every terminal's nearest repository root in one background
    /// pass. Rows read only the store — the ancestor walk is a filesystem
    /// probe, and a miss just means "not known yet".
    pub(super) fn ensure_sidebar_terminal_repo_roots(&self, cx: &mut Context<Self>) {
        let mut fingerprint = 0x7e0e_5a1d_1e0c_a710;
        let mut directories = HashSet::new();
        for terminal_id in self.sidebar_terminal_ids() {
            fingerprint = mix_uuid(fingerprint, terminal_id);
            if let Some(cwd) = self.terminal_cwd(terminal_id, cx) {
                fingerprint = mix_str(fingerprint, &cwd.to_string_lossy());
                directories.insert(cwd);
            }
        }
        if self.sidebar_terminal_repo_scan_fingerprint.get() == Some(fingerprint) {
            return;
        }
        self.sidebar_terminal_repo_scan_fingerprint
            .set(Some(fingerprint));
        let generation = self
            .sidebar_terminal_repo_scan_generation
            .get()
            .wrapping_add(1);
        self.sidebar_terminal_repo_scan_generation.set(generation);

        if directories.is_empty() {
            self.sidebar_terminal_repo_roots.borrow_mut().clear();
            return;
        }
        cx.spawn(async move |waku, cx| {
            let roots = cx
                .background_executor()
                .spawn(async move {
                    directories
                        .into_iter()
                        .map(|cwd| (cwd.clone(), nearest_repo_root(&cwd)))
                        .collect::<HashMap<_, _>>()
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.sidebar_terminal_repo_scan_generation.get() != generation {
                    return;
                }
                *waku.sidebar_terminal_repo_roots.borrow_mut() = roots;
                cx.notify();
            });
        })
        .detach();
    }

    /// The next "…ago" boundary any terminal row crosses, in seconds from
    /// `now` — the same cadence contract `next_time_label_change` keeps
    /// for session rows, so the shared wake re-renders when a label flips.
    pub(super) fn next_terminal_time_label_change(&self, now: u64, cx: &App) -> Option<u64> {
        let mut next: Option<u64> = None;
        for terminal_id in self.sidebar_terminal_ids() {
            let Some(record) = self.terminal_records.get(&terminal_id) else {
                continue;
            };
            let started_at = self
                .right_panel_terminals
                .get(&terminal_id)
                .and_then(|view| view.read(cx).last_command_started_at())
                .unwrap_or(record.opened_at);
            let elapsed = now.saturating_sub(started_at);
            let step = match elapsed {
                0..=3_599 => 60,
                3_600..=86_399 => 3_600,
                _ => 86_400,
            };
            let remaining = (step - elapsed % step).max(1);
            next = Some(next.map_or(remaining, |next| next.min(remaining)));
        }
        next
    }

    /// Register a terminal surface with the group. Called wherever a
    /// terminal surface is created; records carry what the surface lists
    /// cannot — creation order, pin state, and the spawn directory.
    pub(super) fn register_terminal(
        &mut self,
        terminal_id: Uuid,
        session: Option<Uuid>,
        working_directory: Option<PathBuf>,
    ) {
        if self.terminal_records.contains_key(&terminal_id) {
            return;
        }
        self.terminal_records.insert(
            terminal_id,
            TerminalRecord {
                session,
                pinned: false,
                working_directory,
                opened_at: unix_time(),
            },
        );
        self.terminal_order.push(terminal_id);
        self.sidebar_rows_fingerprint.set(None);
    }

    /// Drop every trace of a terminal: the view entity, its launch state,
    /// the group record, and any selection pointing at it.
    pub(super) fn drop_terminal(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        self.right_panel_terminals.remove(&terminal_id);
        self.right_panel_terminal_commands.remove(&terminal_id);
        self.custom_command_runs.remove(&terminal_id);
        self.terminal_records.remove(&terminal_id);
        self.terminal_order.retain(|id| *id != terminal_id);
        if self.selected_terminal == Some(terminal_id) {
            self.selected_terminal = None;
        }
        if self.last_visible_terminal == Some(terminal_id) {
            self.last_visible_terminal = None;
        }
        if self.terminal_order.is_empty() {
            // Closing the last terminal folds the group — expanding an
            // empty group is how a new global terminal is made, so a row of
            // nothing adds nothing.
            self.set_sidebar_group_collapsed(SidebarGroup::Terminals, true, cx);
        }
        self.sidebar_rows_fingerprint.set(None);
    }

    /// Spawn the PTY-backed view for a terminal id. Shared by the right
    /// panel's lazy ensure and the Terminals group's direct creation.
    pub(super) fn spawn_terminal_entity(
        &mut self,
        terminal_id: Uuid,
        working_directory: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let command = self.right_panel_terminal_commands.get(&terminal_id);
        let launch = command
            .cloned()
            .map(TerminalLaunch::CustomCommand)
            .unwrap_or(TerminalLaunch::Shell);
        let close_on_exit = command.is_some_and(|command| command.close_on_success);
        let view = cx.new(|cx| TerminalView::with_launch(working_directory.clone(), launch, cx));
        cx.subscribe(&view, move |this, view, event: &TerminalViewEvent, cx| {
            match event {
                // The command's startup line ends in `&& exit`, so the
                // shell only goes away on its own when the script
                // succeeded — the exit event is the close signal. A
                // terminal filling the main area has no tab strip to
                // retire into, so its shell exiting (ctrl+d, `exit`, a
                // signal) closes it too.
                TerminalViewEvent::Exited => {
                    if close_on_exit || this.selected_terminal == Some(terminal_id) {
                        this.close_terminal_view_surface(&view, cx);
                    }
                }
                TerminalViewEvent::CommandFinished(code) => {
                    // A finished command may have changed the
                    // checkout, so drop the cached snapshot; the
                    // next read refetches.
                    this.refresh_selected_branch_snapshot(cx);
                    this.custom_command_finished(terminal_id, *code, cx);
                }
                TerminalViewEvent::LocalhostUrl(url) => {
                    this.on_localhost_url_detected(&view, url.clone(), cx);
                }
                // Command status or cwd changed — the sidebar row reads
                // both straight off the view; the cwd fingerprint makes
                // the repo scan re-run on its own.
                TerminalViewEvent::ActivityChanged => cx.notify(),
            }
        })
        .detach();
        if command.is_some() {
            // The view's 24ms poll notifies on every PTY dirty flag;
            // each one is a chance to refresh the toast's output tail.
            cx.observe(&view, move |this, _, cx| {
                this.refresh_command_run_tail(terminal_id, cx);
            })
            .detach();
        }
        self.right_panel_terminals.insert(terminal_id, view);
    }

    /// Create a terminal rooted at `working_directory`. With `session` set
    /// it joins that task's right-panel surfaces; `None` makes it global —
    /// listed in the Terminals group but in no tab strip. A `command`
    /// replaces the plain shell launch: the PTY sources its script and the
    /// tab wears its icon.
    pub(super) fn create_terminal(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
        command: Option<CustomCommand>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        if self.daemon.is_remote() {
            return None;
        }
        let session = session.filter(|session_id| {
            self.state
                .sessions
                .iter()
                .any(|session| session.id == *session_id)
        });
        let terminal_id = Uuid::new_v4();
        self.register_terminal(terminal_id, session, Some(working_directory.clone()));
        if let Some(command) = command {
            self.right_panel_terminal_commands
                .insert(terminal_id, command);
        }
        if let Some(session_id) = session {
            let surface = RightPanelSurface::Terminal(terminal_id);
            if self.state.selected_session == Some(session_id) {
                // Push directly rather than add_right_panel_surface: the tab
                // joins the strip without activating or stealing focus.
                self.right_panel_surfaces.push(surface);
            } else {
                self.right_panel_session_states
                    .entry(session_id)
                    .or_insert_with(|| RightPanelSessionState::empty(false))
                    .surfaces
                    .push(surface);
            }
        }
        self.spawn_terminal_entity(terminal_id, working_directory, cx);
        cx.notify();
        Some(terminal_id)
    }

    /// Show a terminal full-width in the main area. Entering terminal mode
    /// parks the session selection — composer draft, transcript scroll, and
    /// right-panel state are stored exactly as a session switch stores them.
    pub(super) fn select_terminal(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.terminal_records.get(&terminal_id) else {
            return;
        };
        if !self.right_panel_terminals.contains_key(&terminal_id) {
            if self.daemon.is_remote() {
                return;
            }
            let working_directory = record.working_directory.clone().or_else(|| {
                record
                    .session
                    .and_then(|session_id| {
                        self.state
                            .sessions
                            .iter()
                            .find(|session| session.id == session_id)
                    })
                    .and_then(|session| self.workspace_path_for_session(session))
                    .map(Path::to_path_buf)
            });
            let Some(working_directory) = working_directory else {
                return;
            };
            self.spawn_terminal_entity(terminal_id, working_directory, cx);
        }
        if self.state.selected_session.is_some() {
            self.capture_and_save_current_composer_draft(cx);
            self.store_selected_right_panel_state();
            self.store_transcript_scroll_position();
            self.state.selected_session = None;
        }
        self.pending_session_activation = None;
        // A terminal claims the main area too: an open GitHub browser for
        // the selected project folds, keeping its state for the next visit.
        if let Some(project_id) = self.state.selected_project {
            self.deactivate_github_browser(project_id);
        }
        self.selected_terminal = Some(terminal_id);
        self.last_visible_terminal = Some(terminal_id);
        if let Some(terminal) = self.right_panel_terminals.get(&terminal_id) {
            let focus = terminal.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
        self.save();
        cx.notify();
    }

    /// Fold the group open and land on the last terminal that was on
    /// screen — the newest one if none has been shown yet, or a fresh
    /// global terminal in ~ when the group is empty.
    pub(super) fn expand_terminals_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
        let target = self
            .last_visible_terminal
            .filter(|id| self.terminal_records.contains_key(id))
            .or_else(|| {
                self.terminal_order
                    .iter()
                    .rev()
                    .copied()
                    .find(|id| self.terminal_records.contains_key(id))
            });
        let terminal_id = match target {
            Some(terminal_id) => Some(terminal_id),
            None => dirs::home_dir().and_then(|home| self.create_terminal(home, None, None, cx)),
        };
        if let Some(terminal_id) = terminal_id {
            self.select_terminal(terminal_id, window, cx);
        }
    }

    /// `secondary-t` — the Terminals chord. While a full-width terminal is
    /// active it opens another in the same directory and scope; otherwise
    /// it expands the group.
    pub(super) fn toggle_terminals_action(
        &mut self,
        _: &ToggleTerminals,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        if let Some(terminal_id) = self.selected_terminal {
            let working_directory = self
                .right_panel_terminals
                .get(&terminal_id)
                .map(|terminal| terminal.read(cx).working_directory().to_path_buf())
                .or_else(|| {
                    self.terminal_records
                        .get(&terminal_id)
                        .and_then(|record| record.working_directory.clone())
                });
            let session = self
                .terminal_records
                .get(&terminal_id)
                .and_then(|record| record.session);
            if let Some(working_directory) = working_directory
                && let Some(new_terminal) =
                    self.create_terminal(working_directory, session, None, cx)
            {
                self.select_terminal(new_terminal, window, cx);
            }
        } else {
            self.expand_terminals_group(window, cx);
        }
        cx.notify();
    }

    /// ⌘⌥P on a terminal row: pinned terminals keep a sidebar row while the
    /// group is collapsed.
    pub(super) fn toggle_terminal_pin(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        let Some(record) = self.terminal_records.get_mut(&terminal_id) else {
            return;
        };
        record.pinned = !record.pinned;
        self.sidebar_rows_fingerprint.set(None);
        cx.notify();
    }

    /// Close a terminal wherever it lives — the active session's tab
    /// strip, a background session's stored surfaces, or the global list.
    pub(super) fn close_terminal(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        if let Some(index) = self
            .right_panel_surfaces
            .iter()
            .position(|surface| surface.terminal_id() == Some(terminal_id))
        {
            self.close_right_panel_surface(index, cx);
            return;
        }
        for state in self.right_panel_session_states.values_mut() {
            let Some(index) = state
                .surfaces
                .iter()
                .position(|surface| surface.terminal_id() == Some(terminal_id))
            else {
                continue;
            };
            state.surfaces.remove(index);
            state.active_surface = state.active_surface.and_then(|active| {
                (!state.surfaces.is_empty()).then(|| match active.cmp(&index) {
                    std::cmp::Ordering::Greater => active - 1,
                    std::cmp::Ordering::Equal => index.saturating_sub(1),
                    std::cmp::Ordering::Less => active.min(state.surfaces.len() - 1),
                })
            });
            break;
        }
        self.drop_terminal(terminal_id, cx);
        cx.notify();
    }

    /// The selected terminal rendered full-width in the main area,
    /// standing in for the transcript while terminal mode is active.
    pub(super) fn render_main_terminal(
        &self,
        terminal_id: Uuid,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(terminal) = self.right_panel_terminals.get(&terminal_id).cloned() else {
            return div().into_any_element();
        };
        if (terminal.read(cx).panel_width() - width).abs() > 0.5 {
            terminal.update(cx, |terminal, _| terminal.set_panel_width(width));
        }
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(terminal)
            .into_any_element()
    }

    /// One terminal row in the Terminals group — also what a pinned
    /// terminal contributes to the collapsed group.
    pub(super) fn render_sidebar_terminal_item(
        &self,
        terminal_id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(record) = self.terminal_records.get(&terminal_id) else {
            return div().into_any_element();
        };
        let pinned = record.pinned;
        let terminal = self.right_panel_terminals.get(&terminal_id);
        let title = terminal
            .map(|terminal| single_line_label(terminal.read(cx).title()))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| tr!("right_panel.terminal"));
        let cwd = self.terminal_cwd(terminal_id, cx);
        // The leading slot is the command's status: spinning while one
        // runs, its exit mark after, empty when the shell reports nothing.
        // The slot is fixed-width so the title never shifts under it.
        let status_icon = terminal.and_then(|terminal| {
            let terminal = terminal.read(cx);
            if terminal.command_running() {
                Some(motion::spin_slow(icon(
                    "icons/loader-circle.svg",
                    12.0,
                    theme.text_tertiary,
                )))
            } else {
                match terminal.last_command_exit() {
                    Some(0) => {
                        Some(icon("icons/check.svg", 12.0, theme.success).into_any_element())
                    }
                    Some(_) => Some(icon("icons/x.svg", 12.0, theme.danger).into_any_element()),
                    None => None,
                }
            }
        });
        let shell_name = terminal
            .map(|terminal| terminal.read(cx).shell_name().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                crate::command_env::default_terminal_shell()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned()
            });
        // The trailing label is the terminal's location, named by its
        // nearest repository the way a session row names its project —
        // "waku" at the root, "waku/src/app" deeper in. Outside any
        // repository the shell's name stands in under a shell glyph
        // rather than a folder, with the cwd trailing it.
        let repo = cwd.as_deref().and_then(|cwd| {
            self.sidebar_terminal_repo_roots
                .borrow()
                .get(cwd)
                .cloned()
                .flatten()
                .map(|root| (cwd, root))
        });
        let (detail_icon, detail) = match repo {
            Some((cwd, root)) => {
                let name = root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let label = cwd
                    .strip_prefix(&root)
                    .ok()
                    .filter(|rel| !rel.as_os_str().is_empty())
                    .map(|rel| format!("{name}/{}", rel.display()))
                    .unwrap_or(name);
                // The repo name anchors the label — the folded middle
                // comes from the working directory's ancestors.
                (
                    "icons/folder.svg",
                    truncate_path_ancestors(&label, SIDEBAR_TERMINAL_DETAIL_MAX_CHARS, true),
                )
            }
            None => (
                "icons/terminal-square.svg",
                cwd.as_deref()
                    .map(|cwd| {
                        let cwd =
                            settings::abbreviate_home_path(cwd, self.home_directory.as_deref());
                        let budget = SIDEBAR_TERMINAL_DETAIL_MAX_CHARS
                            .saturating_sub(shell_name.chars().count() + 1);
                        let cwd = truncate_path_ancestors(&cwd, budget, false);
                        format!("{shell_name} {cwd}")
                    })
                    .unwrap_or_else(|| shell_name.clone()),
            ),
        };
        // "…ago" marks the latest command's start while one runs; before
        // the first command it is the terminal's own open time.
        let started_at = terminal
            .and_then(|terminal| terminal.read(cx).last_command_started_at())
            .unwrap_or(record.opened_at);
        let time_label = format_time_ago(unix_time().saturating_sub(started_at));
        let selected = self.selected_terminal == Some(terminal_id);
        let menu = self.menu_handle(format!("terminal-{terminal_id}"), cx);
        let row_focus = menu.trigger_focus_handle().clone();
        let keyboard_menu = menu.clone();
        let waku = cx.entity().downgrade();
        let row = div()
            .id(SharedString::from(format!(
                "sidebar-terminal-{terminal_id}"
            )))
            .w_full()
            .h(px(SIDEBAR_TERMINAL_ROW_HEIGHT - SIDEBAR_TERMINAL_ROW_GAP))
            .pl(px(8.0))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .rounded(px(9.0))
            .cursor_default()
            .when(selected, |element| {
                element.bg(theme.sidebar_item_background)
            })
            .hover(|element| element.bg(theme.sidebar_item_background))
            .active(|element| element.bg(theme.overlay_strong))
            .track_focus(&row_focus)
            .tab_index(0)
            .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
            .child(
                div()
                    .flex_none()
                    .w(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(status_icon),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .max_w(px(200.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(icon(detail_icon, 12.5, theme.text_tertiary))
                    // The char budget folds ancestors first; this clip is
                    // only the last resort for a single oversized leaf.
                    .child(div().min_w_0().truncate().child(SharedString::from(detail))),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(time_label)),
            )
            .when(pinned, |element| {
                element.child(icon("icons/pin-filled.svg", 12.0, theme.text_ghost))
            })
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if matches!(key, "enter" | "space") {
                    this.select_terminal(terminal_id, window, cx);
                    cx.stop_propagation();
                } else if key == "f10" && event.keystroke.modifiers.shift {
                    keyboard_menu.open_context_menu(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_terminal(terminal_id, window, cx);
            }));

        let row = context_menu(
            div().w_full().child(row),
            SharedString::from(format!("terminal-menu-{terminal_id}")),
            &menu,
            move |cx| {
                let pinned = waku
                    .update(cx, |waku, _| {
                        waku.terminal_records
                            .get(&terminal_id)
                            .is_some_and(|record| record.pinned)
                    })
                    .unwrap_or(false);
                let pin_waku = waku.clone();
                let close_waku = waku.clone();
                vec![
                    MenuItem::new(
                        if pinned {
                            tr!("session.unpin")
                        } else {
                            tr!("session.pin")
                        },
                        move |_, cx| {
                            let _ = pin_waku.update(cx, |waku, cx| {
                                waku.toggle_terminal_pin(terminal_id, cx);
                            });
                        },
                    )
                    .shortcut_action(&ToggleSessionPin)
                    .icon(if pinned {
                        "icons/pin-off.svg"
                    } else {
                        "icons/pin.svg"
                    }),
                    MenuItem::Separator,
                    MenuItem::new(tr!("common.close"), move |_, cx| {
                        let _ = close_waku.update(cx, |waku, cx| {
                            waku.close_terminal(terminal_id, cx);
                        });
                    })
                    .icon("icons/x.svg"),
                ]
            },
        );

        div()
            .w_full()
            .pb(px(SIDEBAR_TERMINAL_ROW_GAP))
            .child(row)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestor_truncation_leaves_short_paths_alone() {
        assert_eq!(truncate_path_ancestors("~/src", 32, false), "~/src");
        assert_eq!(truncate_path_ancestors("waku", 4, true), "waku");
    }

    #[test]
    fn ancestor_truncation_pops_whole_components() {
        assert_eq!(
            truncate_path_ancestors("~/very/long/ancestor/path", 12, false),
            "…/path"
        );
        assert_eq!(
            truncate_path_ancestors("~/very/long/ancestor/path", 16, false),
            "…/ancestor/path"
        );
    }

    #[test]
    fn ancestor_truncation_keeps_the_repo_name_anchored() {
        assert_eq!(
            truncate_path_ancestors("repo/alpha/beta/gamma", 14, true),
            "repo/…/gamma"
        );
        assert_eq!(
            truncate_path_ancestors("repo/alpha/beta/gamma", 18, true),
            "repo/…/beta/gamma"
        );
        // Two components have no ancestors to pop — nothing is faked.
        assert_eq!(truncate_path_ancestors("repo/leaf", 4, true), "repo/leaf");
    }

    #[test]
    fn ancestor_truncation_never_cuts_through_a_component() {
        let truncated = truncate_path_ancestors("~/a/averyverylongleafname", 8, false);
        assert_eq!(truncated, "…/averyverylongleafname");
        assert!(
            truncated.ends_with("averyverylongleafname"),
            "the leaf stays whole even past the budget: {truncated}"
        );
        // A lone component has no ancestors to pop.
        assert_eq!(
            truncate_path_ancestors("averyverylongname", 8, false),
            "averyverylongname"
        );
    }
}
