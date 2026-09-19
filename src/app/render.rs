use super::*;

use crate::ui::ActivationExt;

fn should_render_empty_state(session: Option<&AgentSession>) -> bool {
    // Turns count as content even before any message exists: a
    // provider-initiated turn (Codex goal continuation) reasons for a while
    // before its first text delta, and the transcript's working indicator —
    // not the new-task greeting — is what represents that state.
    session
        .map(|session| {
            session.detail_loaded && session.messages.is_empty() && session.turns.is_empty()
        })
        .unwrap_or(true)
}

impl Waku {
    pub(super) fn render_panel_resize_handle(
        &self,
        id: &'static str,
        target: PanelResizeTarget,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let active = self
            .panel_resize_drag
            .is_some_and(|drag| drag.target == target);
        let bar = |element: Div| {
            element
                .bg(if active {
                    theme.resize_handle
                } else {
                    gpui::transparent_black()
                })
                .group_hover("panel-resize-handle", |element| {
                    element.bg(theme.resize_handle)
                })
        };
        let mut strip = div().id(id).absolute().group("panel-resize-handle");
        // The right panel's left edge abuts the browser webview, a native view
        // that composites above every base-scene pixel at or beyond the edge.
        // Its bar and hover strip therefore sit entirely left of the edge,
        // where GPUI still owns rendering and input; the other edges keep the
        // conventional straddle. The Git panel's top divider is horizontal and
        // pinned inside the region's bottom edge.
        strip = match target {
            PanelResizeTarget::RightPanel => strip
                .top_0()
                .left(px(-7.0))
                .w(px(8.0))
                .h_full()
                .cursor_col_resize()
                .child(bar(
                    div().absolute().top_0().left(px(5.0)).w(px(2.0)).h_full(),
                )),
            PanelResizeTarget::Sidebar | PanelResizeTarget::FileTree => strip
                .top_0()
                .left(px(-5.0))
                .w(px(10.0))
                .h_full()
                .cursor_col_resize()
                .child(bar(
                    div().absolute().top_0().left(px(5.0)).w(px(2.0)).h_full(),
                )),
            PanelResizeTarget::GitPanelTop => strip
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(8.0))
                .cursor_row_resize()
                .child(bar(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(3.0))
                        .h(px(2.0)),
                )),
        };
        strip.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event, window, cx| {
                this.begin_panel_resize(target, event, window, cx);
            }),
        )
    }
}

/// Panel geometry for the frame being built.
#[derive(Clone, Copy)]
struct PanelFrame {
    /// Width each panel lays its content out at, sliding or not.
    sidebar_content: f32,
    right_panel_content: f32,
    /// Width each panel occupies on screen: the eased slide while one runs.
    sidebar: f32,
    right_panel: f32,
    /// Which edge is mid-slide. The clip that keeps a sliding panel inside its
    /// narrowing container also cuts whatever that panel draws outside its own
    /// bounds — the right panel's resize handle sits entirely left of its edge
    /// — so each clip only goes on while its own panel is actually moving.
    sidebar_sliding: bool,
    right_panel_sliding: bool,
    /// An edge is still moving, so the frame loop has to keep going.
    sliding: bool,
    /// The fullscreen surface layer is on screen: the mode is active or an
    /// exit slide is still traveling. While it is, the docked slot stays an
    /// empty spacer so the pane is only ever mounted in one place.
    panel_fullscreen: bool,
    panel_fullscreen_width: f32,
}

/// Advance one panel's slide: the eased width while it runs, the settled
/// target once it is over. Retiring the tween here is what lets a closed
/// panel leave the element tree instead of lingering at zero width, still
/// rebuilding itself on every notify.
fn slide_width(slide: &mut Option<motion::WidthTween>, target: f32) -> f32 {
    match slide.and_then(|slide| slide.width_toward(target)) {
        Some(width) => width,
        None => {
            *slide = None;
            target
        }
    }
}

impl Waku {
    /// An edge is currently animating. While this holds, the pane islands'
    /// root observer stops fanning root notifies out to every island (see
    /// [`WakuPane::bind`]) and lets the cached-view geometry checks decide
    /// which islands a slide tick actually rebuilds.
    pub(super) fn panels_sliding(&self) -> bool {
        self.sidebar_slide.is_some()
            || self.right_panel_slide.is_some()
            || self.panel_fullscreen_slide.is_some()
    }

    /// Settle both panel slides for this frame and publish the widths the
    /// pane islands — which render later, during layout — have to agree with.
    fn settle_panel_slides(&mut self, window: &Window) -> PanelFrame {
        let was_sliding = self.panels_sliding();
        if self.settings_page.is_some() {
            // Settings covers the workspace, so there is no edge on screen to
            // move. Retire the slide rather than animate a layout nobody can
            // see; reopening the workspace finds the panels where they belong.
            self.sidebar_slide = None;
            self.right_panel_slide = None;
            self.panel_fullscreen_slide = None;
        }
        // Fullscreen belongs to the right-panel tab it was opened on: the
        // tab closing, another surface or file taking its place, the panel
        // hiding, or a session swap all end it. Each of those notifies the
        // root, so reconciling once per frame covers them without every
        // caller remembering to clear the flag — and ends without a slide,
        // since the surface it animated to is already gone.
        if let Some((surface, path)) = &self.fullscreen_surface
            && (!self.right_panel_visible
                || self.active_right_panel_surface() != Some(surface)
                || &self.visible_right_panel_file_path() != path)
        {
            self.fullscreen_surface = None;
            self.panel_fullscreen_slide = None;
        }
        let (sidebar_content, right_panel_fitted) = self.effective_panel_widths(window);
        // An open commit turns the Git panel into the workspace's main
        // surface: the slot stretches to the sidebar so the diff column gets
        // everything the transcript had. The pane reads the unexpanded width
        // back out of `effective_panel_widths` for its own column.
        let right_panel_content = if self.git_panel_visible && self.git_panel_commit_diff.is_some()
        {
            (f32::from(window.viewport_size().width) - sidebar_content).max(right_panel_fitted)
        } else {
            right_panel_fitted
        };
        let sidebar = slide_width(
            &mut self.sidebar_slide,
            if self.sidebar_visible {
                sidebar_content
            } else {
                0.0
            },
        );
        let right_panel = slide_width(
            &mut self.right_panel_slide,
            if self.right_panel_visible || self.git_panel_visible {
                right_panel_content
            } else {
                0.0
            },
        );
        let panel_fullscreen = slide_width(
            &mut self.panel_fullscreen_slide,
            if self.fullscreen_surface.is_some() {
                f32::from(window.viewport_size().width)
            } else {
                right_panel_content
            },
        );
        self.sidebar_rendered_width = sidebar;
        self.right_panel_rendered_width = right_panel;
        self.panel_fullscreen_rendered_width = panel_fullscreen;
        let sliding = self.panels_sliding();
        if was_sliding && !sliding {
            // The observer gate held root-state fan-out away from any island
            // the slide left geometry-stable. One ungated notify now that
            // the slide is over rebuilds every island once, so whatever
            // root state changed during those 200ms lands the next frame.
            let root = window.current_view();
            window.on_next_frame(move |_, cx| cx.notify(root));
        }
        PanelFrame {
            sidebar_content,
            right_panel_content,
            sidebar,
            right_panel,
            sidebar_sliding: self.sidebar_slide.is_some(),
            right_panel_sliding: self.right_panel_slide.is_some(),
            sliding,
            panel_fullscreen: self.panel_fullscreen_active(),
            panel_fullscreen_width: panel_fullscreen,
        }
    }

    /// Width left for the chat column once the panels take theirs — the
    /// widths they are painted at this frame, so a transcript measured
    /// mid-slide matches the column it is laid out in.
    fn chat_viewport_width(&self, window: &Window) -> f32 {
        f32::from(window.viewport_size().width)
            - self.sidebar_rendered_width
            - self.right_panel_rendered_width
    }

    /// The peek overlay mounts only while the docked slot has fully released
    /// the pane — a sidebar that is open or still sliding keeps it.
    fn sidebar_peek_allowed(&self) -> bool {
        !self.sidebar_visible && self.sidebar_slide.is_none()
    }

    /// The peek overlay's width: the configured sidebar width a touch wider,
    /// still fitted to leave the main panel on screen.
    fn sidebar_peek_width(&self, window: &Window) -> f32 {
        let viewport_width = f32::from(window.viewport_size().width);
        (sanitize_panel_width(
            self.sidebar_width,
            DEFAULT_SIDEBAR_WIDTH,
            SIDEBAR_MIN_WIDTH,
            SIDEBAR_MAX_WIDTH,
        ) * SIDEBAR_PEEK_WIDTH_FACTOR)
            .min((viewport_width - MAIN_PANEL_MIN_WIDTH).max(SIDEBAR_PEEK_STRIP))
    }

    /// Entering the edge strip reveals the overlay at once; the nudge plays
    /// out from there.
    fn sidebar_peek_strip_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !*hovered
            || !self.sidebar_peek_allowed()
            || matches!(self.sidebar_peek, SidebarPeek::Shown { .. })
        {
            return;
        }
        self.sidebar_peek = SidebarPeek::Shown {
            entered: Instant::now(),
        };
        self.sidebar_peek_action_hold = false;
        cx.notify();
    }

    /// Whether a menu card is up over the workspace — or an inline rename
    /// editor is holding the sidebar. While one is, the pointer leaving the
    /// peek overlay is it moving *into the menu* — the card is a deferred
    /// layer outside the overlay's element tree — or simply away from the
    /// editor it is typing in, not a real exit, so the overlay must not
    /// dismiss underneath it.
    fn any_menu_open(&self, cx: &App) -> bool {
        self.menus.borrow().values().any(ContextMenuHandle::is_open)
            || self.composer.read(cx).context_menu_open(cx)
            || self.session_rename.is_some()
    }

    /// The overlay is the hover surface once it is up — it covers the strip —
    /// so leaving it (or the window) starts the nudge-out, and returning to
    /// it mid-exit settles it back.
    fn sidebar_peek_overlay_hover(
        &mut self,
        hovered: &bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *hovered {
            // The pointer is back on the panel — a row action's hold is
            // spent, and a mid-exit return settles the overlay back.
            self.sidebar_peek_action_hold = false;
            if matches!(self.sidebar_peek, SidebarPeek::Exiting { .. }) {
                self.sidebar_peek = SidebarPeek::Shown {
                    entered: Instant::now(),
                };
                cx.notify();
            }
            return;
        }
        if self.any_menu_open(cx) {
            // An open menu claimed the pointer. Defer the exit to
            // `settle_sidebar_peek`, which re-checks it once the menu closes.
            self.sidebar_peek_menu_hold = true;
            return;
        }
        self.begin_sidebar_peek_exit(cx);
        cx.notify();
    }

    /// A row action run from the peek-mounted sidebar (pin, archive) keeps
    /// the overlay after its menu closes; the hold releases the next time
    /// the pointer enters the panel.
    pub(super) fn hold_sidebar_peek(&mut self) {
        if matches!(
            self.sidebar_peek,
            SidebarPeek::Shown { .. } | SidebarPeek::Exiting { .. }
        ) {
            self.sidebar_peek_action_hold = true;
        }
    }

    /// Start the overlay's nudge-out when it is on screen.
    fn begin_sidebar_peek_exit(&mut self, cx: &App) {
        if matches!(self.sidebar_peek, SidebarPeek::Shown { .. }) {
            self.sidebar_peek = if cx.reduce_motion() {
                SidebarPeek::Hidden
            } else {
                SidebarPeek::Exiting {
                    started: Instant::now(),
                }
            };
        }
    }

    /// Advance the peek nudge and retire the overlay when it ends. Returns
    /// the `left` inset and opacity for the frame while the overlay stays
    /// mounted.
    fn settle_sidebar_peek(&mut self, window: &Window, cx: &App) -> Option<(f32, f32)> {
        // A sidebar that opens or starts sliding takes the pane back, and
        // settings covers the workspace outright — either way the overlay
        // retires without its nudge-out.
        if self.settings_page.is_some() || !self.sidebar_peek_allowed() {
            self.sidebar_peek = SidebarPeek::Hidden;
            self.sidebar_peek_menu_hold = false;
            self.sidebar_peek_action_hold = false;
            return None;
        }
        // Settle a hover exit a menu card claimed: once no menu is open, a
        // pointer that ended up outside the panel dismisses it here rather
        // than waiting for the next mouse move to resend the hover. A row
        // action run from that menu keeps the overlay instead — the action
        // hold releases the next time the pointer enters the panel.
        if self.sidebar_peek_menu_hold && !self.any_menu_open(cx) {
            self.sidebar_peek_menu_hold = false;
            let right_edge = px(self.sidebar_peek_width(window));
            if window.mouse_position().x > right_edge && !self.sidebar_peek_action_hold {
                self.begin_sidebar_peek_exit(cx);
            }
        }
        if cx.reduce_motion() {
            return (!matches!(self.sidebar_peek, SidebarPeek::Hidden)).then_some((0.0, 1.0));
        }
        match self.sidebar_peek {
            SidebarPeek::Hidden => None,
            SidebarPeek::Shown { entered } => {
                let progress =
                    (entered.elapsed().as_secs_f32() / SIDEBAR_PEEK_SLIDE.as_secs_f32()).min(1.0);
                if progress < 1.0 {
                    window.request_animation_frame();
                }
                Some((
                    -SIDEBAR_PEEK_NUDGE * (1.0 - ease_out_quint()(progress)),
                    1.0,
                ))
            }
            SidebarPeek::Exiting { started } => {
                let progress = started.elapsed().as_secs_f32() / SIDEBAR_PEEK_SLIDE.as_secs_f32();
                if progress >= 1.0 {
                    self.sidebar_peek = SidebarPeek::Hidden;
                    self.sidebar_peek_action_hold = false;
                    return None;
                }
                window.request_animation_frame();
                // Ease-in (the entry's quint mirrored): the panel accelerates
                // off the edge instead of braking into its disappearance,
                // while a linear fade keeps the exit from reading as a pop.
                Some((
                    -SIDEBAR_PEEK_NUDGE * progress.max(0.0).powi(5),
                    1.0 - progress.max(0.0),
                ))
            }
        }
    }

    /// [`WakuPane`] delegate for the sidebar island.
    pub(super) fn sidebar_pane_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Mounted in the peek overlay, the pane lays out at the overlay's
        // width; docked, at the fitted sidebar width.
        let width =
            if self.sidebar_peek_allowed() && !matches!(self.sidebar_peek, SidebarPeek::Hidden) {
                self.sidebar_peek_width(window)
            } else {
                self.effective_panel_widths(window).0
            };
        self.render_sidebar(width, window, cx).into_any_element()
    }

    /// [`WakuPane`] delegate for the transcript island.
    pub(super) fn transcript_pane_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chat_viewport_width = self.chat_viewport_width(window);
        // The transcript's own element sizes itself with `flex_1`, which only
        // stretches inside a flex parent. A cached pane lays its content out
        // as a root, so give it that parent here or its height collapses to
        // the zero flex basis.
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .child(self.render_transcript(window, chat_viewport_width, cx))
            .into_any_element()
    }

    /// [`WakuPane`] delegate for the right-panel island.
    pub(super) fn right_panel_pane_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Inside the fullscreen layer the panel lays out at the layer's own
        // (possibly still sliding) width; docked, at its fitted width.
        let width = if self.panel_fullscreen_active() {
            self.panel_fullscreen_rendered_width
        } else {
            self.effective_panel_widths(window).1
        };
        self.render_right_panel(width, window, cx)
            .into_any_element()
    }

    /// Measure live frame rate by counting renders over a sliding one-second
    /// window and keep requesting animation frames so the counter stays current.
    fn tick_fps(&mut self, window: &Window) {
        let now = Instant::now();
        self.fps_frame_count = self.fps_frame_count.saturating_add(1);
        if now.duration_since(self.fps_last_frame) >= Duration::from_secs(1) {
            self.fps_value = self.fps_frame_count as u32;
            self.fps_frame_count = 0;
            self.fps_last_frame = now;
        }
        window.request_animation_frame();
    }
}

impl Render for Waku {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Panel geometry first: the browser sync right below reads whether a
        // panel is mid-slide, and settling here rather than at the point of
        // use keeps the pane islands and the transcript on one set of widths.
        let panels = self.settle_panel_slides(window);
        if panels.sliding {
            // The manual drive for the width tweens — the same scheduling
            // `with_animation` would do, minus its element-id keying.
            window.request_animation_frame();
        }
        let sidebar_peek = self.settle_sidebar_peek(window, cx);
        // Before anything can early-return (the settings page below), settle
        // whether each native browser webview belongs on screen this frame —
        // it floats above everything GPUI paints.
        self.sync_browser_webviews(cx);
        if self.fps_counter_visible {
            self.tick_fps(window);
        }
        let image_preview = self.render_image_preview(cx);
        let task_switcher = self.render_task_switcher(window, cx);
        let project_switcher = self.render_project_switcher(window, cx);
        let big_picture = self.render_big_picture(window, cx);
        if self.settings_page.is_some() {
            let command_palette = self.render_command_palette(window, cx);
            let file_finder = self.render_file_finder(window, cx);
            let commit_dialog = self.render_commit_dialog(cx);
            let issue_dialog = self.render_issue_dialog(cx);
            let archive_dialog = self.render_archive_dialog(cx);
            let shortcuts_dialog = self.render_shortcuts_dialog(cx);
            let goal_dialog = self.render_goal_dialog(window, cx);
            let send_file_dialog = self.render_send_file_dialog(cx);
            let ssh_prompt = self.render_ssh_prompt(window, cx);
            let toast = self.render_active_toast(window, cx);
            let content = div()
                .relative()
                .size_full()
                .on_action(cx.listener(Self::toggle_command_palette_action))
                .on_action(cx.listener(Self::toggle_file_finder_action))
                .on_action(cx.listener(Self::toggle_big_picture_action))
                .on_action(cx.listener(Self::open_resume_picker_action))
                .on_action(cx.listener(Self::run_project_script_action))
                .on_action(cx.listener(Self::switch_task_forward_action))
                .on_action(cx.listener(Self::switch_task_backward_action))
                .on_action(cx.listener(Self::select_first_task_action))
                .on_action(cx.listener(Self::select_last_task_action))
                .on_action(cx.listener(Self::confirm_task_switch_action))
                .on_action(cx.listener(Self::cancel_task_switch_action))
                .on_action(cx.listener(Self::switch_project_forward_action))
                .on_action(cx.listener(Self::switch_project_backward_action))
                .on_action(cx.listener(Self::select_first_project_action))
                .on_action(cx.listener(Self::select_last_project_action))
                .on_action(cx.listener(Self::confirm_project_switch_action))
                .on_action(cx.listener(Self::cancel_project_switch_action))
                .on_action(cx.listener(Self::select_sidebar_session_action))
                .on_action(cx.listener(Self::adjust_font_size_action))
                .on_action(cx.listener(Self::open_localhost_url_action))
                .on_action(cx.listener(Self::open_localhost_url_in_tab_action))
                .on_action(cx.listener(Self::new_terminal_action))
                .on_action(cx.listener(Self::open_created_issue_in_github_action))
                .on_action(cx.listener(Self::open_toast_session_action))
                .on_action(cx.listener(Self::toggle_terminals_action))
                .on_action(cx.listener(Self::toggle_projects_page_action))
                .on_action(cx.listener(Self::toggle_inbox_page_action))
                .on_action(cx.listener(Self::select_projects_tab_action))
                .on_modifiers_changed(cx.listener(Self::task_switcher_modifiers_changed))
                .on_modifiers_changed(cx.listener(Self::project_switcher_modifiers_changed))
                .on_modifiers_changed(cx.listener(Self::sidebar_shortcuts_modifiers_changed))
                .capture_key_down(cx.listener(Self::sidebar_shortcuts_key_down))
                .child(self.render_settings(window, cx))
                .children(toast)
                .children(command_palette)
                .children(file_finder)
                .children(commit_dialog)
                .children(issue_dialog)
                .children(archive_dialog)
                .children(shortcuts_dialog)
                .children(goal_dialog)
                .children(send_file_dialog)
                .children(ssh_prompt)
                .children(image_preview)
                .children(task_switcher)
                .children(project_switcher)
                .children(big_picture)
                .into_any_element();
            return self.render_window_frame(content, window, cx);
        }
        // Re-armed every frame this window shows time labels; parks while
        // settings covers them and while the window isn't drawing at all.
        self.schedule_time_label_wake(cx);

        let theme = Theme::current(cx);
        let empty = should_render_empty_state(self.selected_session());
        let projects_page = self.projects_page;
        let permission = self.render_permission(cx);
        // A started Antigravity session shows its TUI terminal as the whole
        // surface — no transcript, no composer.
        let agy_surface = self.selected_session().is_some_and(|session| {
            session.provider == ProviderKind::Antigravity && session.has_started()
        });
        let computer_use = self.render_computer_use_overlay(window, cx);
        let command_palette = self.render_command_palette(window, cx);
        let file_finder = self.render_file_finder(window, cx);
        let commit_dialog = self.render_commit_dialog(cx);
        let issue_dialog = self.render_issue_dialog(cx);
        let archive_dialog = self.render_archive_dialog(cx);
        let shortcuts_dialog = self.render_shortcuts_dialog(cx);
        let goal_dialog = self.render_goal_dialog(window, cx);
        let send_file_dialog = self.render_send_file_dialog(cx);
        let automation_editor = self.render_automation_editor(window, cx);
        let ssh_prompt = self.render_ssh_prompt(window, cx);
        let sync_branch_modal = self.render_sync_branch(window, cx);
        let git_panel_overlays = self.render_git_panel_overlays(window, cx);
        let toast = self.render_active_toast(window, cx);
        let content = div()
            .key_context("Waku")
            .on_action(cx.listener(Self::close_window_or_right_panel_tab_action))
            .on_action(cx.listener(Self::new_session_action))
            .on_action(cx.listener(Self::new_task_in_action))
            .on_action(cx.listener(Self::new_project_action))
            .on_action(cx.listener(Self::open_settings_action))
            .on_action(cx.listener(Self::toggle_sidebar_action))
            .on_action(cx.listener(Self::toggle_right_panel_action))
            .on_action(cx.listener(Self::toggle_git_panel_action))
            .on_action(cx.listener(Self::toggle_command_palette_action))
            .on_action(cx.listener(Self::toggle_file_finder_action))
            .on_action(cx.listener(Self::toggle_big_picture_action))
            .on_action(cx.listener(Self::open_resume_picker_action))
            .on_action(cx.listener(Self::run_project_script_action))
            .on_action(cx.listener(Self::toggle_fps_counter_action))
            .on_action(cx.listener(Self::navigate_back_action))
            .on_action(cx.listener(Self::navigate_forward_action))
            .on_action(cx.listener(Self::undo_draft_use_action))
            .on_action(cx.listener(Self::go_to_next_unread_completion_action))
            .on_action(cx.listener(Self::mark_session_unread_action))
            .on_action(cx.listener(Self::mark_unread_and_go_to_next_idle_action))
            .on_action(cx.listener(Self::go_to_previous_turn_action))
            .on_action(cx.listener(Self::go_to_next_turn_action))
            .on_action(cx.listener(Self::switch_task_forward_action))
            .on_action(cx.listener(Self::switch_task_backward_action))
            .on_action(cx.listener(Self::select_first_task_action))
            .on_action(cx.listener(Self::select_last_task_action))
            .on_action(cx.listener(Self::confirm_task_switch_action))
            .on_action(cx.listener(Self::cancel_task_switch_action))
            .on_action(cx.listener(Self::switch_project_forward_action))
            .on_action(cx.listener(Self::switch_project_backward_action))
            .on_action(cx.listener(Self::select_first_project_action))
            .on_action(cx.listener(Self::select_last_project_action))
            .on_action(cx.listener(Self::confirm_project_switch_action))
            .on_action(cx.listener(Self::cancel_project_switch_action))
            .on_action(cx.listener(Self::focus_composer_action))
            .on_action(cx.listener(Self::focus_terminal_action))
            .on_action(cx.listener(Self::toggle_model_picker_action))
            .on_action(cx.listener(Self::select_favorite_model_action))
            .on_action(cx.listener(Self::cycle_reasoning_effort_action))
            .on_action(cx.listener(Self::toggle_branch_picker_action))
            .on_action(cx.listener(Self::toggle_runtime_mode_picker_action))
            .on_action(cx.listener(Self::toggle_environment_action))
            .on_action(cx.listener(Self::toggle_workspace_action))
            .on_action(cx.listener(Self::toggle_usage_panel_action))
            .on_action(cx.listener(Self::save_right_panel_file_action))
            .on_action(cx.listener(Self::sync_branch_action))
            .on_action(cx.listener(Self::cancel_turn_action))
            .on_action(cx.listener(Self::archive_session_action))
            .on_action(cx.listener(Self::toggle_session_pin_action))
            .on_action(cx.listener(Self::copy_selection_action))
            .on_action(cx.listener(Self::add_to_chat_action))
            .on_action(cx.listener(Self::copy_working_directory_action))
            .on_action(cx.listener(Self::open_go_to_line_action))
            .on_action(cx.listener(Self::open_find_action))
            .on_action(cx.listener(Self::open_find_replace_action))
            .on_action(cx.listener(Self::close_find_action))
            .on_action(cx.listener(Self::find_next_action))
            .on_action(cx.listener(Self::find_previous_action))
            .on_action(cx.listener(Self::toggle_find_case_action))
            .on_action(cx.listener(Self::toggle_find_whole_word_action))
            .on_action(cx.listener(Self::toggle_find_regex_action))
            .on_action(cx.listener(Self::replace_all_matches_action))
            .on_action(cx.listener(Self::select_sidebar_session_action))
            .on_action(cx.listener(Self::adjust_font_size_action))
            .on_action(cx.listener(Self::open_localhost_url_action))
            .on_action(cx.listener(Self::open_localhost_url_in_tab_action))
            .on_action(cx.listener(Self::new_terminal_action))
            .on_action(cx.listener(Self::open_created_issue_in_github_action))
            .on_action(cx.listener(Self::open_toast_session_action))
            .on_action(cx.listener(Self::toggle_terminals_action))
            .on_action(cx.listener(Self::toggle_projects_page_action))
            .on_action(cx.listener(Self::toggle_automations_page_action))
            .on_action(cx.listener(Self::select_automations_tab_action))
            .on_action(cx.listener(Self::toggle_inbox_page_action))
            .on_action(cx.listener(Self::select_projects_tab_action))
            .on_modifiers_changed(cx.listener(Self::task_switcher_modifiers_changed))
            .on_modifiers_changed(cx.listener(Self::project_switcher_modifiers_changed))
            .on_modifiers_changed(cx.listener(Self::sidebar_shortcuts_modifiers_changed))
            .capture_key_down(cx.listener(Self::sidebar_shortcuts_key_down))
            // Enter-to-continue and type-to-focus are the last listeners on
            // every dispatch path through the workspace: an unclaimed
            // keystroke from a focused descendant — or from nothing, on
            // platforms where this div is the dispatch root — either fires
            // the stopped-turn Continue or lands in the composer.
            .on_key_down(cx.listener(Self::enter_to_continue))
            .on_key_down(cx.listener(Self::type_to_focus_composer))
            .capture_any_mouse_down(cx.listener(Self::navigation_mouse_down))
            .capture_any_mouse_down(cx.listener(Self::sidebar_multi_selection_mouse_down))
            .on_mouse_move(cx.listener(Self::resize_panel_mouse_move))
            .capture_any_mouse_up(cx.listener(Self::finish_panel_resize))
            .size_full()
            .relative()
            .flex()
            .text_color(theme.text)
            .font_family(crate::fonts::current(cx).ui)
            // Both panels slide through a container that narrows while their
            // content keeps its full width and is clipped: the sidebar list
            // and the right panel's surfaces never reflow on the way in or
            // out, and their bounds stay put so only the clip moves.
            .when(panels.sidebar > 0.0, |root| {
                root.child(
                    div()
                        .h_full()
                        .flex_none()
                        .w(px(panels.sidebar))
                        .when(panels.sidebar_sliding, |element| element.overflow_hidden())
                        .child(
                            self.sidebar_pane.clone().cached(
                                StyleRefinement::default()
                                    .w(px(panels.sidebar_content))
                                    .h_full()
                                    .flex_none(),
                            ),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .bg(theme.surface)
                    .when(panels.sidebar > 0.0, |element| {
                        element
                            .border_l(hairline())
                            .border_color(theme.sidebar_border)
                    })
                    // Files dropped anywhere in the session column stage as
                    // composer attachments. The group marks the column's
                    // hitbox so the composer card can light itself up as the
                    // landing zone wherever the drag is held.
                    .when(
                        self.selected_project().is_some()
                            && self.selected_terminal.is_none()
                            && self.projects_page.is_none()
                            && !self.notifications.open
                            && !self.drafts_page
                            && !self.automations_page
                            && !agy_surface,
                        |element| {
                            element
                                .group(composer::SESSION_DROP_GROUP)
                                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                                    this.stage_dropped_files(paths, window, cx);
                                }))
                                .on_drop(cx.listener(
                                    |this, drag: &composer::SidebarSessionDrag, window, cx| {
                                        this.stage_session_reference(
                                            drag.session_id,
                                            &drag.title,
                                            window,
                                            cx,
                                        );
                                    },
                                ))
                        },
                    )
                    .child(self.render_header(window, cx))
                    // A selected terminal takes the column in place of the
                    // transcript, the Projects page, or the new-task prompt.
                    .child(
                        if let Some(terminal_id) = self
                            .selected_terminal
                            .filter(|id| self.right_panel_terminals.contains_key(id))
                        {
                            self.render_main_terminal(
                                terminal_id,
                                self.chat_viewport_width(window),
                                cx,
                            )
                        } else if self.drafts_page {
                            self.render_drafts_page(cx)
                        } else if self.automations_page {
                            self.render_automations_page(cx)
                        } else if projects_page.is_some() {
                            self.render_projects_page(window, cx)
                        } else if self.notifications.open {
                            self.render_inbox_page(window, cx)
                        } else if agy_surface {
                            self.render_agy_surface(self.chat_viewport_width(window), cx)
                        } else if empty {
                            self.render_empty_state(cx).into_any_element()
                        } else {
                            self.transcript_pane
                                .clone()
                                .cached(StyleRefinement::default().flex_1().min_h(px(0.0)).w_full())
                                .into_any_element()
                        },
                    )
                    .children(permission)
                    // Big Picture remounts the one composer entity inside its
                    // own layer; mounting it here too would collide. The
                    // Projects page docks its own composer instead. While the
                    // overlay is open a spacer holds the lane at its last
                    // measured height so the transcript's frame — and with it
                    // the scroll anchor — does not shift.
                    .when(
                        self.selected_project().is_some()
                            && self.selected_terminal.is_none()
                            && self.projects_page.is_none()
                            && !self.notifications.open
                            && !self.drafts_page
                            && !self.automations_page
                            && !agy_surface,
                        |element| {
                            if self.big_picture.is_open() {
                                element
                                    .child(div().flex_none().h(px(self.composer_lane_height.get())))
                            } else {
                                let lane_height = self.composer_lane_height.clone();
                                element.child(
                                    div()
                                        .flex_none()
                                        .relative()
                                        .child(
                                            canvas(
                                                move |bounds, _, _| {
                                                    lane_height.set(f32::from(bounds.size.height))
                                                },
                                                |_, _, _, _| (),
                                            )
                                            .absolute()
                                            .inset_0(),
                                        )
                                        .children(self.render_queued_messages(cx))
                                        .child(self.render_composer(window, cx))
                                        .child(self.render_workspace_footer(cx)),
                                )
                            }
                        },
                    )
                    .relative()
                    .children(toast)
                    .when(self.sidebar_visible, |element| {
                        element.child(self.render_panel_resize_handle(
                            "sidebar-resize-handle",
                            PanelResizeTarget::Sidebar,
                            cx,
                        ))
                    }),
            )
            .when(panels.right_panel > 0.0, |root| {
                root.child(
                    div()
                        .h_full()
                        .flex_none()
                        .w(px(panels.right_panel))
                        .flex()
                        .relative()
                        .when(panels.right_panel_sliding, |element| {
                            element.overflow_hidden()
                        })
                        // Pinned to the window's right edge, so the panel is
                        // uncovered from that edge inward rather than dragged
                        // across the screen. While the fullscreen layer owns
                        // the pane this slot is only a spacer: the docked
                        // layout underneath never disturbs, and the pane is
                        // never mounted in two places at once.
                        // The Git panel and the right panel are alternatives
                        // in the same slot — the flag decides which pane is
                        // mounted; the slot's width and slide are shared.
                        .when(!panels.panel_fullscreen, |element| {
                            let pane = if self.git_panel_visible {
                                self.git_panel_pane.clone()
                            } else {
                                self.right_panel_pane.clone()
                            };
                            element.child(
                                pane.cached(
                                    StyleRefinement::default()
                                        .absolute()
                                        .top_0()
                                        .right_0()
                                        .w(px(panels.right_panel_content))
                                        .h_full(),
                                ),
                            )
                        }),
                )
            })
            // The maximized panel surface: the right panel's own island
            // remounted as a right-pinned layer over the whole window.
            // Escape is bound on the PanelFullscreen context, which sits
            // between FileEditorPane (close-find) and Waku (cancel-turn), so
            // an open find bar still eats the first escape and a turn is
            // never cancelled from here. The binding excludes Terminal, so a
            // focused terminal inside the layer keeps Escape for the pty.
            .when(panels.panel_fullscreen, |root| {
                root.child(
                    div()
                        .key_context("PanelFullscreen")
                        .occlude()
                        .absolute()
                        .top_0()
                        .right_0()
                        .h_full()
                        .w(px(panels.panel_fullscreen_width))
                        .on_action(cx.listener(Self::exit_panel_fullscreen_action))
                        .child(
                            self.right_panel_pane
                                .clone()
                                .cached(StyleRefinement::default().size_full()),
                        ),
                )
            })
            // The closed sidebar's hover zone: a transparent strip along the
            // left edge. Its hitbox is Normal, so the header and transcript
            // beneath keep their clicks and hover — it only reports whether
            // the pointer is on the edge.
            .when(self.sidebar_peek_allowed(), |root| {
                root.child(
                    div()
                        .id("sidebar-peek-strip")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(px(SIDEBAR_PEEK_STRIP))
                        .cursor_default()
                        .on_hover(cx.listener(Self::sidebar_peek_strip_hover)),
                )
            })
            // The peek overlay: the real sidebar pane — scroll position and
            // all — mounted over the content rather than in the layout. Once
            // up it covers the strip and is itself the hover surface, so only
            // leaving the panel dismisses it. On macOS the pane's own fill is
            // transparent — it borrows the native vibrancy strip behind the
            // window, which the opaque surface under this overlay hides — so
            // the overlay carries the sidebar's solid color itself.
            .when_some(sidebar_peek, |root, (offset, opacity)| {
                root.child(
                    div()
                        .id("sidebar-peek")
                        .occlude()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(offset))
                        .opacity(opacity)
                        .w(px(self.sidebar_peek_width(window)))
                        .bg(theme.sidebar_drag_background)
                        .border_r(hairline())
                        .border_color(theme.sidebar_border)
                        .on_hover(cx.listener(Self::sidebar_peek_overlay_hover))
                        .child(
                            self.sidebar_pane
                                .clone()
                                .cached(StyleRefinement::default().size_full()),
                        ),
                )
            })
            .children(computer_use)
            .children(command_palette)
            .children(file_finder)
            .children(commit_dialog)
            .children(issue_dialog)
            .children(archive_dialog)
            .children(shortcuts_dialog)
            .children(goal_dialog)
            .children(send_file_dialog)
            .children(automation_editor)
            .children(ssh_prompt)
            .children(sync_branch_modal)
            .children(git_panel_overlays)
            .children(image_preview)
            .children(task_switcher)
            .children(project_switcher)
            .children(big_picture)
            .into_any_element();

        self.render_window_frame(content, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unloaded_history_never_renders_the_new_task_prompt() {
        let mut stored = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        stored.detail_loaded = false;

        assert!(!should_render_empty_state(Some(&stored)));

        let draft = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        assert!(should_render_empty_state(Some(&draft)));
        assert!(should_render_empty_state(None));
    }
}

impl Waku {
    /// Arm the dismiss timer and build the floating toast layer, if a toast
    /// is active. Every full-window surface (workspace and settings alike)
    /// must include this, or a toast raised there stays invisible until the
    /// user navigates away.
    fn render_active_toast(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.start_toast_dismiss_timer(cx);
        let toast = self.toast.as_ref().map(|toast| {
            (
                toast.message.clone(),
                toast.detail.clone(),
                toast.tone,
                toast.action.clone(),
                toast.id,
            )
        });
        toast.map(|(message, detail, tone, action, generation)| {
            self.render_toast(message, detail, tone, action, generation, window, cx)
                .into_any_element()
        })
    }

    fn render_toast(
        &self,
        message: String,
        detail: Option<Vec<String>>,
        tone: ToastTone,
        action: Option<ToastAction>,
        generation: u64,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let status_icon = match tone {
            ToastTone::Alert => icon("icons/alert.svg", 14.0, theme.danger).into_any_element(),
            ToastTone::Success => icon("icons/check.svg", 14.0, theme.success).into_any_element(),
            ToastTone::Failure => icon("icons/x.svg", 14.0, theme.danger).into_any_element(),
            ToastTone::Notice => icon("icons/server.svg", 14.0, theme.accent).into_any_element(),
            ToastTone::Progress => {
                motion::spin(icon("icons/loader-circle.svg", 14.0, theme.text_tertiary))
            }
        };
        let palette = MarkdownPalette::from_theme(&theme);
        let text_ctx = MarkdownCtx::new(
            format!("toast-{generation}"),
            &palette,
            self.scaled_markdown_metrics(MarkdownMetrics::COMPACT),
            self.toast_selection.clone(),
        )
        .with_families(crate::fonts::current(cx));
        let message = md::render::plain_text(
            message,
            text_ctx.families().ui.clone(),
            FontWeight::NORMAL,
            theme.text,
            &text_ctx,
        );
        let dismiss = div()
            .id(SharedString::from(format!("dismiss-toast-{generation}")))
            .tab_index(0)
            .size(px(26.0))
            .flex_none()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .tooltip(Tooltip::text(tr!("common.dismiss_notification")))
            .child(icon("icons/x.svg", 12.0, theme.text_tertiary))
            .on_click(cx.listener(|this, _, _, cx| {
                this.hide_toast();
                cx.notify();
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space" | "escape") {
                    this.hide_toast();
                    cx.notify();
                    cx.stop_propagation();
                }
            }));

        let has_detail = detail.as_ref().is_some_and(|lines| !lines.is_empty());
        let detail_block = detail.filter(|lines| !lines.is_empty()).map(|lines| {
            div()
                .mt(px(6.0))
                .pt(px(6.0))
                .border_t(hairline())
                .border_color(theme.separator)
                .flex()
                .flex_col()
                .font_family(crate::fonts::current(cx).code)
                .text_size(sp(11.5))
                .line_height(sp(15.0))
                .text_color(theme.text_tertiary)
                .children(lines.into_iter().map(|line| {
                    div()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .overflow_hidden()
                        .child(SharedString::from(line))
                }))
        });

        let action_button = action.map(|action| {
            let kind = action.kind.clone();
            let mut label = action.label.to_string();
            let mut tooltip = None;
            if matches!(kind, ToastActionKind::Session(_)) {
                if let Some(open) =
                    crate::ui::shortcut::ShortcutHint::action(&crate::OpenToastSession)
                        .resolve(window, cx)
                {
                    label = format!("{label} {open}");
                }
            }
            if matches!(kind, ToastActionKind::LocalhostUrl) {
                // The unarchive toast's binding wins the shared ⌘⌥O but
                // propagates back here when its session is not the offer.
                if let Some(open) =
                    crate::ui::shortcut::ShortcutHint::action(&crate::OpenLocalhostUrl)
                        .shadowed_by(&crate::OpenToastSession)
                        .resolve(window, cx)
                {
                    label = format!("{label} {open}");
                }
                if let Some(in_tab) =
                    crate::ui::shortcut::ShortcutHint::action(&crate::OpenLocalhostUrlInTab)
                        .resolve(window, cx)
                {
                    tooltip = Some(Tooltip::text(tr!(
                        "terminal.localhost_open_in_tab",
                        keys = in_tab
                    )));
                }
            }
            if matches!(kind, ToastActionKind::GitHubIssue { .. }) {
                if let Some(open) =
                    crate::ui::shortcut::ShortcutHint::action(&crate::OpenCreatedIssueInGitHub)
                        .resolve(window, cx)
                {
                    label = format!("{label} {open}");
                }
            }
            div()
                .id(SharedString::from(format!("toast-action-{generation}")))
                .tab_index(0)
                .h(px(22.0))
                .px(px(8.0))
                .flex_none()
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .whitespace_nowrap()
                .text_color(theme.accent)
                .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                .hover(|element| element.bg(theme.overlay))
                .active(|element| element.bg(theme.overlay_strong))
                .when_some(tooltip, |element, tooltip| element.tooltip(tooltip))
                .child(label)
                .on_click(cx.listener({
                    let kind = kind.clone();
                    move |this, event: &ClickEvent, window, cx| {
                        match &kind {
                            ToastActionKind::Session(session_id) => {
                                this.open_toast_session(*session_id, cx)
                            }
                            ToastActionKind::Unarchive(session_ids) => {
                                this.undo_archived_sessions(session_ids, cx)
                            }
                            ToastActionKind::LocalhostUrl => this.open_detected_localhost_url(
                                event.modifiers().shift,
                                window,
                                cx,
                            ),
                            ToastActionKind::RelocateProject(project_id) => {
                                this.hide_toast();
                                this.relocate_project(*project_id, cx);
                            }
                            ToastActionKind::GitHubIssue {
                                project,
                                number,
                                url,
                            } => this.open_created_issue(
                                *project,
                                *number,
                                url.as_ref(),
                                window,
                                cx,
                            ),
                        }
                        cx.stop_propagation();
                    }
                }))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        match &kind {
                            ToastActionKind::Session(session_id) => {
                                this.open_toast_session(*session_id, cx)
                            }
                            ToastActionKind::Unarchive(session_ids) => {
                                this.undo_archived_sessions(session_ids, cx)
                            }
                            ToastActionKind::LocalhostUrl => this.open_detected_localhost_url(
                                event.keystroke.modifiers.shift,
                                window,
                                cx,
                            ),
                            ToastActionKind::RelocateProject(project_id) => {
                                this.hide_toast();
                                this.relocate_project(*project_id, cx);
                            }
                            ToastActionKind::GitHubIssue {
                                project,
                                number,
                                url,
                            } => this.open_created_issue(
                                *project,
                                *number,
                                url.as_ref(),
                                window,
                                cx,
                            ),
                        }
                        cx.stop_propagation();
                    }
                }))
        });

        div()
            .id(SharedString::from(format!("toast-layer-{generation}")))
            .absolute()
            .left_0()
            .top(px(56.0))
            .w_full()
            .px(px(20.0))
            .flex()
            .justify_center()
            .child(
                div()
                    .id(SharedString::from(format!("toast-{generation}")))
                    .occlude()
                    .max_w(px(560.0))
                    .min_w_0()
                    // A command's output tail republishes on a ~125ms
                    // cadence; hold the toast at its max width while detail
                    // is showing so each refresh doesn't resize it.
                    .when(has_detail, |element| element.w_full())
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(12.0))
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.raised)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .text_size(sp(12.5))
                    .line_height(sp(16.0))
                    .text_color(theme.text)
                    .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                        this.set_toast_hovered(*hovering, cx);
                    }))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(md::render::frame_reset(self.toast_selection.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_icon)
                            .child(div().flex_1().min_w_0().whitespace_normal().child(message))
                            .children(action_button)
                            .child(dismiss),
                    )
                    .children(detail_block)
                    .child(self.toast_selection_input()),
            )
            // Keep the toast top-centered just beneath Goddard's 48px header.
            // GPUI's animation path honors the system reduce-motion preference
            // and resolves immediately.
            .with_animation(
                SharedString::from(format!("toast-enter-{generation}")),
                Animation::new(TOAST_ANIMATION_DURATION).with_easing(ease_out_quint()),
                |element, delta| {
                    element
                        .top(px(48.0 + 8.0 * delta))
                        .opacity(0.4 + 0.6 * delta)
                },
            )
    }

    /// The modal an ssh askpass request becomes. The prompt text is whatever
    /// ssh asked for — `host's password:` or `Enter passphrase for key: …` —
    /// and Enter submits the field straight back to the waiting helper.
    #[cfg(not(unix))]
    pub(super) fn render_ssh_prompt(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }

    /// The modal an ssh askpass request becomes. The prompt text is whatever
    /// ssh asked for — `host's password:` or `Enter passphrase for key: …` —
    /// and Enter submits the field straight back to the waiting helper.
    #[cfg(unix)]
    pub(super) fn render_ssh_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.pending_ssh_prompt.is_none() {
            return None;
        }
        if self
            .pending_ssh_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.input.is_none())
        {
            let input = cx.new(|cx| {
                TextInput::new(window, cx)
                    .masked()
                    .tab_index(0)
                    .accessibility_label(tr!("daemon.ssh_password_label"))
                    .placeholder(tr!("daemon.ssh_password_label"))
            });
            cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Submit(_)) {
                    this.answer_ssh_prompt(false, cx);
                }
            })
            .detach();
            self.pending_ssh_prompt.as_mut().unwrap().input = Some(input);
        }
        let prompt = self.pending_ssh_prompt.as_ref().unwrap();
        let input = prompt.input.clone().unwrap();
        if !input.read(cx).focus().is_focused(window) {
            let focus = input.read(cx).focus();
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        }
        let theme = Theme::current(cx);
        let button = |id: &'static str, label: String| {
            div()
                .id(id)
                .tab_index(0)
                .h(px(28.0))
                .px(px(12.0))
                .rounded(px(9.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(13.0))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|style| style.border_color(theme.accent))
                .child(label)
        };
        let card = div()
            .id("ssh-prompt-card")
            .key_context("SshPrompt")
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(400.0))
            .overflow_hidden()
            .rounded(px(21.0))
            .bg(theme.composer)
            .shadow_xl()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.answer_ssh_prompt(true, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .text_size(sp(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(icon("icons/server.svg", 15.0, theme.text))
                    .child(tr!("daemon.ssh_password_title")),
            )
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(8.0))
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(sp(12.5))
                    .line_height(sp(16.0))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(prompt.prompt.clone())),
            )
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(10.0))
                    .child(TextField::new("ssh-prompt-input", input).w_full()),
            )
            .child(
                div()
                    .p(px(12.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        button("ssh-prompt-cancel", tr!("common.cancel")).on_activation(
                            cx,
                            |this, _, cx| {
                                this.answer_ssh_prompt(true, cx);
                            },
                        ),
                    )
                    .child(
                        button("ssh-prompt-submit", tr!("daemon.ssh_password_submit"))
                            .on_activation(cx, |this, _, cx| {
                                this.answer_ssh_prompt(false, cx);
                            }),
                    ),
            );
        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("ssh-prompt-layer")
            .absolute()
            .inset_0()
            .occlude()
            .bg(scrim)
            .p(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .child(motion::modal_enter("ssh-prompt-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("ssh-prompt-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}
