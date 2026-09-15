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
}

/// A terminal row is a single line: icon, title, directory.
pub(super) const SIDEBAR_TERMINAL_ROW_HEIGHT: f32 = 30.0;
const SIDEBAR_TERMINAL_ROW_GAP: f32 = 1.0;

impl Waku {
    /// Terminal ids in creation order — the group's flat listing.
    pub(super) fn sidebar_terminal_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.terminal_order
            .iter()
            .copied()
            .filter(|id| self.terminal_records.contains_key(id))
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
            },
        );
        self.terminal_order.push(terminal_id);
        self.sidebar_rows_fingerprint.set(None);
    }

    /// Drop every trace of a terminal: the view entity, its launch state,
    /// the group record, and any selection pointing at it.
    pub(super) fn drop_terminal(&mut self, terminal_id: Uuid) {
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
        let view =
            cx.new(|cx| TerminalView::with_launch(working_directory.clone(), launch, cx));
        cx.subscribe(&view, move |this, view, event: &TerminalViewEvent, cx| {
            match event {
                // The command's startup line ends in `&& exit`, so the
                // shell only goes away on its own when the script
                // succeeded — the exit event is the close signal.
                TerminalViewEvent::Exited => {
                    if close_on_exit {
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
    /// listed in the Terminals group but in no tab strip.
    pub(super) fn create_terminal(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
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
    pub(super) fn expand_terminals_group(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            None => dirs::home_dir().and_then(|home| self.create_terminal(home, None, cx)),
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
                    self.create_terminal(working_directory, session, cx)
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
        self.drop_terminal(terminal_id);
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
        let record_directory = record.working_directory.clone();
        let terminal = self.right_panel_terminals.get(&terminal_id);
        let title = terminal
            .map(|terminal| single_line_label(terminal.read(cx).title()))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| tr!("right_panel.terminal"));
        let directory = terminal
            .map(|terminal| terminal.read(cx).working_directory().to_path_buf())
            .or(record_directory)
            .map(|path| {
                if dirs::home_dir().as_deref() == Some(path.as_path()) {
                    "~".to_owned()
                } else {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| path.display().to_string())
                }
            })
            .unwrap_or_else(|| tr!("workspace.workspace"));
        let selected = self.selected_terminal == Some(terminal_id);
        let menu = self.menu_handle(format!("terminal-{terminal_id}"), cx);
        let row_focus = menu.trigger_focus_handle().clone();
        let keyboard_menu = menu.clone();
        let waku = cx.entity().downgrade();
        let row = div()
            .id(SharedString::from(format!("sidebar-terminal-{terminal_id}")))
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
            .focus_visible(|style| style.border_1().border_color(theme.accent))
            .child(icon("icons/terminal.svg", 13.0, theme.text_secondary))
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
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(directory)),
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
