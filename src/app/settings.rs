use super::composer::next_picker_highlight;
use super::*;
use crate::theme::{ThemeName, ThemeSettings};
use crate::ui::ActivationExt;
use gpui::{KeyBinding, actions};

const SETTINGS_CONTENT_MAX_WIDTH: f32 = 760.0;

/// The Usage page is a dashboard, not a form; it mirrors T3 Code's wide
/// two-column layout and needs the extra room for the chart.
const SETTINGS_USAGE_MAX_WIDTH: f32 = 1024.0;

/// The Git page's Worktrees/Branches tables carry five-column rows, so it
/// shares the Usage page's wide layout rather than the form width.
const SETTINGS_GIT_MAX_WIDTH: f32 = SETTINGS_USAGE_MAX_WIDTH;

/// Uniform height hint for the virtualized archived-chat rows, so the
/// scrollbar knows the total extent before rows are measured.
const ARCHIVED_SESSION_ROW_HEIGHT: f32 = 45.0;

/// Key context the settings sidebar declares around its search field.
const SETTINGS_SIDEBAR_CONTEXT: &str = "SettingsSidebar";

/// The search field while focused inside the sidebar. The field holds real
/// focus the whole time — the sidebar's selection is only drawn — so `up` and
/// `down` have to be claimed from under it, and only a binding can do that:
/// they arrive as actions, which consume the keystroke before the field sees
/// it.
const SETTINGS_SEARCH_CONTEXT: &str = "SettingsSidebar > TextInput";

/// The Commands page's editor claims Tab for focus traversal rather than
/// letting a field take it as text.
const CUSTOM_COMMAND_EDITOR_CONTEXT: &str = "CustomCommandEditor";

actions!(waku_settings, [FocusNext, FocusPrevious]);

/// The sidebar's rows in display order, each with the keyword haystack the
/// search field filters against.
const SETTINGS_PAGES: [(SettingsPage, &str, &str, &str); 13] = [
    (
        SettingsPage::General,
        "settings.general",
        "icons/settings.svg",
        "settings.general_keywords",
    ),
    (
        SettingsPage::Keybindings,
        "keybind.title",
        "icons/keyboard.svg",
        "keybind.keywords",
    ),
    (
        SettingsPage::Appearance,
        "settings.appearance",
        "icons/appearance.svg",
        "settings.appearance_keywords",
    ),
    (
        SettingsPage::Providers,
        "settings.providers",
        "icons/bot.svg",
        "settings.providers_keywords",
    ),
    (
        SettingsPage::Skills,
        "settings.skills",
        "icons/package.svg",
        "settings.skills_keywords",
    ),
    (
        SettingsPage::Friends,
        "settings.friends",
        "icons/send.svg",
        "settings.friends_keywords",
    ),
    (
        SettingsPage::Archived,
        "settings.archived",
        "icons/archive.svg",
        "settings.archived_keywords",
    ),
    (
        SettingsPage::Git,
        "settings.git",
        "icons/git-branch.svg",
        "settings.git_keywords",
    ),
    (
        SettingsPage::Commands,
        "settings.commands",
        "icons/terminal.svg",
        "settings.commands_keywords",
    ),
    (
        SettingsPage::Usage,
        "settings.usage",
        "icons/chart-column.svg",
        "settings.usage_keywords",
    ),
    (
        SettingsPage::Daemon,
        "settings.daemon",
        "icons/server.svg",
        "settings.daemon_keywords",
    ),
    (
        SettingsPage::ComputerUse,
        "settings.computer_use",
        "icons/cursor-spark.svg",
        "settings.computer_use_keywords",
    ),
    (
        SettingsPage::Experiments,
        "settings.experiments",
        "icons/beaker.svg",
        "settings.experiments_keywords",
    ),
];

/// Bind the settings fields' navigation keys. Called once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNextEntry, Some(SETTINGS_SEARCH_CONTEXT)),
        KeyBinding::new("up", SelectPreviousEntry, Some(SETTINGS_SEARCH_CONTEXT)),
        KeyBinding::new("tab", FocusNext, Some(CUSTOM_COMMAND_EDITOR_CONTEXT)),
        KeyBinding::new(
            "shift-tab",
            FocusPrevious,
            Some(CUSTOM_COMMAND_EDITOR_CONTEXT),
        ),
    ]);
}

/// The Commands settings page's open form. Input entities live for the
/// editor's lifetime rather than being pre-created with the other settings
/// fields.
pub(super) struct CustomCommandEditor {
    /// `None` while the form is creating a new command.
    id: Option<Uuid>,
    name: Entity<TextInput>,
    icon: CustomCommandIcon,
    shell: Entity<TextInput>,
    script: Entity<TextInput>,
    close_on_success: bool,
    /// The script text the command had when the editor opened — its hashed
    /// file is dropped on save once no other command still uses it.
    previous_script: Option<String>,
    /// Set when Save was pressed with an empty script.
    script_required: bool,
    /// A new-command editor opened from the command palette leaves Settings
    /// on save instead of landing back on the Commands page.
    pub(super) exit_settings_on_save: bool,
}

/// The Daemon settings page's open remote-host form. Input entities live for
/// the editor's lifetime rather than being pre-created with the other
/// settings fields.
pub(super) struct RemoteHostEditor {
    /// `Some` when re-pointing an existing record; `None` adds a host.
    pub(super) id: Option<Uuid>,
    name: Entity<TextInput>,
    address: Entity<TextInput>,
    token: Entity<TextInput>,
    /// `user@host` or a `~/.ssh/config` alias. When filled, the connection
    /// goes over the platform `ssh` and the direct fields are unused.
    destination: Entity<TextInput>,
    /// `Host` aliases from `~/.ssh/config`, loaded in the background after
    /// the editor opens; each chips-row entry fills the destination field.
    ssh_hosts: Vec<String>,
    /// Set when Save was pressed without an SSH destination and without a
    /// complete direct address + token pair.
    missing_fields: bool,
}

/// The sidebar rows the query leaves visible, in display order. `query` must
/// already be trimmed and lowercased; when it is empty every page matches.
pub(super) fn visible_settings_pages(
    query: &str,
) -> impl Iterator<Item = (SettingsPage, String, &'static str)> + '_ {
    SETTINGS_PAGES
        .into_iter()
        .filter(|(page, ..)| page.is_visible_in_navigation())
        .filter_map(move |(page, label_key, icon, keywords_key)| {
            let label = crate::i18n::translate(label_key);
            let keywords = crate::i18n::translate(keywords_key).to_lowercase();
            (query.is_empty() || keywords.contains(query)).then_some((page, label, icon))
        })
}

/// The archived rows the search query and project filter leave visible,
/// preserving the input order (callers sort newest-archived first). `query`
/// must already be trimmed and lowercased, and `project_names` must hold
/// each project id's lowercased display name — title and project both match.
pub(super) fn filter_archived_sessions(
    sessions: &[&AgentSession],
    query: &str,
    project_filter: Option<Uuid>,
    project_names: &HashMap<Uuid, String>,
) -> Vec<Uuid> {
    sessions
        .iter()
        .filter(|session| {
            if project_filter.is_some_and(|id| session.project_id != id) {
                return false;
            }
            query.is_empty()
                || session.display_title().to_lowercase().contains(query)
                || project_names
                    .get(&session.project_id)
                    .is_some_and(|name| name.contains(query))
        })
        .map(|session| session.id)
        .collect()
}

impl Waku {
    pub(super) fn render_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);

        div()
            .key_context("Waku")
            .track_focus(&self.settings_focus)
            .on_action(|_: &CloseWindow, window, _| crate::platform::hide_window(window))
            .on_action(cx.listener(Self::new_session_action))
            .on_action(cx.listener(Self::new_project_action))
            .on_action(cx.listener(Self::open_settings_action))
            .on_action(cx.listener(Self::toggle_sidebar_action))
            .on_action(cx.listener(Self::toggle_right_panel_action))
            .on_action(cx.listener(Self::toggle_command_palette_action))
            .on_action(cx.listener(Self::toggle_fps_counter_action))
            .on_action(cx.listener(Self::navigate_back_action))
            .on_action(cx.listener(Self::navigate_forward_action))
            .on_action(cx.listener(Self::focus_composer_action))
            .on_action(cx.listener(Self::focus_terminal_action))
            .on_action(cx.listener(Self::cancel_turn_action))
            .on_action(cx.listener(Self::archive_session_action))
            .capture_any_mouse_down(cx.listener(Self::navigation_mouse_down))
            .size_full()
            .flex()
            .bg(theme.canvas)
            .text_color(theme.text)
            .font_family(crate::fonts::current(cx).ui)
            .child(self.render_settings_sidebar(window, cx))
            .child(self.render_settings_content(window, cx))
            .into_any_element()
    }

    fn render_settings_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let current_page = self.settings_page.unwrap_or(SettingsPage::General);
        let query = self.settings_search_query(cx);
        let mut navigation = div().flex().flex_col().gap(px(3.0));

        for (page, label, icon_path) in visible_settings_pages(&query) {
            let selected = current_page == page;
            navigation = navigation.child(
                div()
                    .id(SharedString::from(format!(
                        "settings-tab-{}",
                        label.to_lowercase()
                    )))
                    .h(px(36.0))
                    .px(px(11.0))
                    .rounded(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_default()
                    .text_size(sp(13.0))
                    .text_color(if selected {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .when(selected, |element| {
                        element.bg(theme.sidebar_item_background)
                    })
                    .hover(|element| element.bg(theme.sidebar_item_background))
                    .active(|element| element.bg(theme.sidebar_item_background))
                    .child(icon(
                        icon_path,
                        15.0,
                        if selected {
                            theme.text_secondary
                        } else {
                            theme.text_tertiary
                        },
                    ))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_settings_page(page, window, cx);
                    })),
            );
        }

        div()
            .key_context(SETTINGS_SIDEBAR_CONTEXT)
            .on_action(cx.listener(|this, _: &SelectNextEntry, window, cx| {
                this.cycle_settings_page("down", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousEntry, window, cx| {
                this.cycle_settings_page("up", window, cx);
            }))
            .w(px(DEFAULT_SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .child(self.render_settings_sidebar_titlebar(window, cx))
            .child(
                div().px(px(12.0)).child(
                    div()
                        .id("settings-back")
                        .h(px(34.0))
                        .px(px(9.0))
                        .rounded(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(9.0))
                        .cursor_default()
                        .text_size(sp(13.0))
                        .text_color(theme.text_secondary)
                        .hover(|element| element.bg(theme.overlay))
                        .active(|element| element.bg(theme.overlay_strong))
                        .child(icon("icons/arrow-left.svg", 15.0, theme.text_tertiary))
                        .child(tr!("settings.back"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.settings_page = None;
                            let focus_handle = this.composer_focus(cx);
                            window.focus(&focus_handle, cx);
                            cx.notify();
                        })),
                ),
            )
            .child(
                div().px(px(12.0)).pt(px(8.0)).child(
                    TextField::new("settings-search-field", self.settings_search.clone())
                        .icon("icons/search.svg", 13.0),
                ),
            )
            .child(div().h(px(18.0)))
            .child(div().px(px(12.0)).child(navigation))
    }

    /// The search field's content, normalized the way the page filter expects.
    fn settings_search_query(&self, cx: &App) -> String {
        self.settings_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase()
    }

    /// Step the selected page through the rows the search leaves visible,
    /// wrapping at both ends. The field keeps focus so typing keeps narrowing
    /// the list; the landing page renders immediately, so there is no separate
    /// confirm step. A selection filtered out by the query re-enters the list
    /// from whichever end matches the key.
    fn cycle_settings_page(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.settings_search_query(cx);
        let pages = visible_settings_pages(&query)
            .map(|(page, ..)| page)
            .collect::<Vec<_>>();
        let current_page = self.settings_page.unwrap_or(SettingsPage::General);
        let current = pages.iter().position(|page| *page == current_page);
        let Some(next) = next_picker_highlight(current, pages.len(), key) else {
            return;
        };
        self.open_settings_page(pages[next], window, cx);
    }

    fn render_settings_sidebar_titlebar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let left_window_controls = self.render_client_window_controls(
            super::window_chrome::WindowControlSide::Left,
            window,
            cx,
        );
        // Only as tall as whatever actually sits in it: macOS's native
        // traffic lights, or the client-side buttons a Linux desktop puts on
        // this side. Windows keeps all three on the far side, and a desktop
        // like GNOME keeps none here, so there is nothing to clear and the
        // strip is only somewhere to drag the window by — the content
        // column's own titlebar carries the rest of that job.
        let height = if cfg!(target_os = "macos") || left_window_controls.is_some() {
            48.0
        } else {
            12.0
        };

        div()
            .id("settings-sidebar-titlebar")
            .h(px(height))
            .flex_none()
            .flex()
            .items_center()
            .children(left_window_controls)
            .child(
                self.window_drag_region(
                    div()
                        .id("settings-sidebar-traffic-light-drag-region")
                        .w(px(TRAFFIC_LIGHT_CLEARANCE))
                        .h_full()
                        .flex_none(),
                    cx,
                ),
            )
            .child(
                self.render_settings_drag_region("settings-sidebar-titlebar-drag-region", cx)
                    .h(px(height))
                    .flex_1(),
            )
    }

    fn render_settings_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let page = self.settings_page.unwrap_or(SettingsPage::General);
        let right_window_controls = self.render_client_window_controls(
            super::window_chrome::WindowControlSide::Right,
            window,
            cx,
        );
        // The Skills page is a mail-style split that owns the whole content
        // column — no page title, no titlebar strip, no width cap, no card.
        // Window dragging stays with the sidebar's own titlebar region.
        if page == SettingsPage::Skills {
            return div()
                .flex_1()
                .h_full()
                .min_w_0()
                .flex()
                .flex_col()
                .border_l(hairline())
                .border_color(theme.sidebar_border)
                .bg(theme.surface)
                .children(right_window_controls.map(|controls| {
                    self.render_settings_drag_region("settings-skills-titlebar", cx)
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(controls)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.render_skills_settings(cx)),
                );
        }
        // The Keybinding Manager is a self-contained surface like Skills:
        // fixed keyboard stage over its own virtualized table.
        if page == SettingsPage::Keybindings {
            return div()
                .flex_1()
                .h_full()
                .min_w_0()
                .flex()
                .flex_col()
                .border_l(hairline())
                .border_color(theme.sidebar_border)
                .bg(theme.surface)
                .children(right_window_controls.map(|controls| {
                    self.render_settings_drag_region("settings-keybindings-titlebar", cx)
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(controls)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.render_keybindings_page(window, cx)),
                );
        }
        // The Monthly and Projects list views own their own scrolling, so
        // their pages fill the viewport instead of riding the shared scroll
        // container; the Archived page's virtualized list needs the same.
        let fills_viewport = page == SettingsPage::Archived
            || page == SettingsPage::Git
            || (page == SettingsPage::Usage
                && matches!(
                    self.usage_view,
                    UsageViewMode::Monthly | UsageViewMode::Projects
                ));
        // The titlebar strip is transparent; once content slides under it, a
        // hairline marks the boundary so the clip edge reads as a header
        // rather than a glitch.
        let content_scrolled = !fills_viewport && self.settings_scroll.offset().y < px(-1.0);

        let inner = div()
            .w_full()
            .max_w(px(match page {
                SettingsPage::Usage => SETTINGS_USAGE_MAX_WIDTH,
                SettingsPage::Git => SETTINGS_GIT_MAX_WIDTH,
                _ => SETTINGS_CONTENT_MAX_WIDTH,
            }))
            .mx_auto()
            .when(fills_viewport, |element| {
                element.h_full().min_h_0().flex().flex_col()
            })
            .child(
                div()
                    .pt(px(2.0))
                    .pl(px(6.0))
                    .flex_none()
                    .text_size(sp(18.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(match page {
                        SettingsPage::General => tr!("settings.general"),
                        SettingsPage::Providers => tr!("settings.providers"),
                        SettingsPage::Skills => tr!("settings.skills"),
                        SettingsPage::Friends => tr!("settings.friends"),
                        SettingsPage::Archived => tr!("settings.archived"),
                        SettingsPage::Usage => tr!("settings.usage"),
                        SettingsPage::Daemon => tr!("settings.daemon"),
                        SettingsPage::ComputerUse => tr!("settings.computer_use"),
                        SettingsPage::Commands => tr!("settings.commands"),
                        SettingsPage::Appearance => tr!("settings.appearance"),
                        SettingsPage::Git => tr!("settings.git"),
                        SettingsPage::Experiments => tr!("settings.experiments"),
                        SettingsPage::Keybindings => tr!("keybind.title"),
                    }),
            )
            .child(match page {
                SettingsPage::General => self.render_general_settings(cx),
                SettingsPage::Providers => self.render_providers_settings(cx),
                SettingsPage::Skills => self.render_skills_settings(cx),
                SettingsPage::Friends => self.render_friends_settings(cx),
                SettingsPage::Archived => self.render_archived_settings(cx),
                SettingsPage::Usage => self.render_usage_settings(cx),
                SettingsPage::Daemon => self.render_daemon_settings(cx),
                SettingsPage::ComputerUse => self.render_computer_use_settings(cx),
                SettingsPage::Commands => self.render_commands_settings(cx),
                SettingsPage::Appearance => self.render_appearance_settings(cx),
                SettingsPage::Git => self.render_git_settings(window, cx),
                SettingsPage::Experiments => self.render_experiments_settings(cx),
                SettingsPage::Keybindings => div().into_any_element(),
            });

        let git_scrollbar = if page == SettingsPage::Git {
            self.render_git_settings_scrollbar()
        } else {
            None
        };

        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .border_l(hairline())
            .border_color(theme.sidebar_border)
            .bg(theme.surface)
            .child(
                self.render_settings_drag_region("settings-content-titlebar", cx)
                    .flex()
                    .items_center()
                    .justify_end()
                    .children(right_window_controls)
                    .when(content_scrolled, |element| {
                        element.border_b(hairline()).border_color(theme.separator)
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("settings-content-scroll")
                            .size_full()
                            .when(!fills_viewport, |element| {
                                element
                                    .overflow_y_scroll()
                                    .track_scroll(&self.settings_scroll)
                                    .pb(px(48.0))
                            })
                            .when(fills_viewport, |element| {
                                element.min_h_0().flex().flex_col()
                            })
                            .px(px(32.0))
                            .child(inner),
                    )
                    .when(!fills_viewport, |element| {
                        element.child(scrollbar::vertical(
                            &self.settings_scroll,
                            &self.settings_scrollbar,
                        ))
                    })
                    .children(git_scrollbar),
            )
    }

    fn render_general_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let updater_available = cx
            .try_global::<crate::updater::UpdaterState>()
            .is_some_and(|updater| updater.0.is_some());
        let analytics_enabled = self.state.analytics_enabled;
        let analytics_toggle = toggle_switch(
            "anonymous-analytics-toggle",
            analytics_enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_analytics_enabled(!analytics_enabled, cx),
        );
        div()
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .child(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!("settings.local_by_default")),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .text_size(sp(12.5))
                            .line_height(sp(18.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("settings.local_by_default_description")),
                    ),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.share_anonymous_usage_data")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.share_anonymous_usage_data_description")),
                            ),
                    )
                    .child(analytics_toggle),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.render_math")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.render_math_description")),
                            ),
                    )
                    .child(toggle_switch(
                        "render-math-toggle",
                        self.state.render_math,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.render_math;
                            move |this, _, cx| this.set_render_math(!enabled, cx)
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.show_response_token_speed")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.show_response_token_speed_description")),
                            ),
                    )
                    .child(toggle_switch(
                        "response-token-speed-toggle",
                        self.state.show_response_token_speed,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.show_response_token_speed;
                            move |this, _, cx| this.set_show_response_token_speed(!enabled, cx)
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.markdown_preview")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.markdown_preview_description")),
                            ),
                    )
                    .child(toggle_switch(
                        "markdown-preview-toggle",
                        self.state.markdown_preview,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.markdown_preview;
                            move |this, _, cx| this.set_markdown_preview(!enabled, cx)
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(13.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.open_at_last_prompt")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.open_at_last_prompt_description")),
                            ),
                    )
                    .child(toggle_switch(
                        "open-at-last-prompt-toggle",
                        self.state.open_at_last_prompt,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.open_at_last_prompt;
                            move |this, _, cx| this.set_open_at_last_prompt(!enabled, cx)
                        },
                    )),
            )
            .child({
                let navigation = self.state.archive_navigation;
                let weak = cx.entity().downgrade();
                let navigation_handle = self.menu_handle("archive-navigation-selector", cx);
                let navigation_selector = dropdown_menu(
                    MenuChip::new("archive-navigation-selector")
                        .label(tr!(navigation.label_key()))
                        .outlined()
                        .selected(navigation_handle.is_open())
                        .w(px(220.0))
                        .justify_between(),
                    "archive-navigation-selector-menu",
                    &navigation_handle,
                    MenuAlign::BelowRight,
                    move |_| {
                        ArchiveNavigation::ALL
                            .into_iter()
                            .map(|option| {
                                let weak = weak.clone();
                                MenuItem::new(tr!(option.label_key()), move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_archive_navigation(option, cx);
                                    });
                                })
                                .selected(option == navigation)
                            })
                            .collect()
                    },
                );
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(13.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.archive_navigation")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.archive_navigation_description")),
                            ),
                    )
                    .child(navigation_selector)
            })
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(13.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.sync_with_merge")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.sync_with_merge_description")),
                            ),
                    )
                    .child(toggle_switch(
                        "sync-with-merge-toggle",
                        self.state.sync_with_merge,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.sync_with_merge;
                            move |this, _, cx| this.set_sync_with_merge(!enabled, cx)
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(13.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.sidebar_shortcut_tags")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!(
                                        "settings.sidebar_shortcut_tags_description",
                                        keys = crate::platform::primary_shortcut(
                                            "⌘1–⌘9",
                                            "Ctrl+1–Ctrl+9"
                                        ),
                                        modifier = crate::platform::primary_shortcut("⌘", "Ctrl")
                                    )),
                            ),
                    )
                    .child(toggle_switch(
                        "sidebar-shortcut-tags-toggle",
                        self.state.sidebar_shortcut_tags,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.sidebar_shortcut_tags;
                            move |this, _, cx| this.set_sidebar_shortcut_tags(!enabled, cx)
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(13.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.new_worktree_default_branch")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.new_worktree_default_branch_description")),
                            ),
                    )
                    .child(toggle_switch(
                        "new-worktree-default-branch-toggle",
                        self.state.new_worktree_default_branch,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.new_worktree_default_branch;
                            move |this, _, cx| this.set_new_worktree_default_branch(!enabled, cx)
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(13.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.new_worktree_sync_default_branch")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!(
                                        "settings.new_worktree_sync_default_branch_description"
                                    )),
                            )
                            .child(
                                div().mt(px(9.0)).max_w(px(360.0)).child(
                                    TextField::new(
                                        "worktree-sync-branches-field",
                                        self.worktree_sync_branches_input.clone(),
                                    )
                                    .w_full(),
                                ),
                            ),
                    )
                    .child(toggle_switch(
                        "new-worktree-sync-default-branch-toggle",
                        self.state.new_worktree_sync_default_branch,
                        false,
                        theme,
                        cx,
                        {
                            let enabled = self.state.new_worktree_sync_default_branch;
                            move |this, _, cx| {
                                this.set_new_worktree_sync_default_branch(!enabled, cx)
                            }
                        },
                    )),
            )
            .when(cfg!(target_os = "macos"), |element| {
                // The platform recognizer reads the trackpad's touch stream,
                // which macOS only hands over when no system gesture claims
                // three-finger horizontal swipes.
                let enabled = self.state.three_finger_swipe_navigation;
                element.child(
                    div()
                        .mt(px(15.0))
                        .w_full()
                        .min_h(px(60.0))
                        .px(px(20.0))
                        .py(px(12.0))
                        .rounded(px(13.0))
                        .bg(theme.raised)
                        .flex()
                        .items_center()
                        .gap(px(24.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(sp(13.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child(tr!("settings.three_finger_swipe_navigation")),
                                )
                                .child(
                                    div()
                                        .mt(px(5.0))
                                        .text_size(sp(12.5))
                                        .line_height(sp(18.0))
                                        .text_color(theme.text_secondary)
                                        .child(tr!(
                                            "settings.three_finger_swipe_navigation_description"
                                        )),
                                ),
                        )
                        .child(toggle_switch(
                            "three-finger-swipe-toggle",
                            enabled,
                            false,
                            theme,
                            cx,
                            move |this, window, cx| {
                                this.set_three_finger_swipe_navigation(!enabled, window, cx)
                            },
                        )),
                )
            })
            .child({
                let enabled = self.state.completion_sound_enabled;
                let selected_sound = self.state.completion_sound;
                let volume = self.state.completion_sound_volume;
                let volume_shown = self.completion_volume_slider.shown(volume);
                let volume_slider = slider::slider(
                    "completion-volume-slider",
                    &self.completion_volume_slider,
                    crate::persistence::MAX_COMPLETION_SOUND_VOLUME,
                    volume,
                    cx,
                    |this, volume, cx| this.set_completion_sound_volume(volume, cx),
                );
                let weak = cx.entity().downgrade();
                let sound_handle = self.menu_handle("completion-sound-selector", cx);
                let sound_selector = dropdown_menu(
                    MenuChip::new("completion-sound-selector")
                        .label(selected_sound.label())
                        .outlined()
                        .selected(sound_handle.is_open())
                        .w(px(116.0))
                        .justify_between(),
                    "completion-sound-selector-menu",
                    &sound_handle,
                    MenuAlign::BelowRight,
                    move |_| {
                        CompletionSound::ALL
                            .into_iter()
                            .map(|sound| {
                                let weak = weak.clone();
                                MenuItem::new(sound.label(), move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_completion_sound(sound, cx);
                                    });
                                })
                                .selected(sound == selected_sound)
                                .on_highlight(move |_, _| {
                                    crate::platform::play_completion_sound(sound, volume);
                                })
                            })
                            .collect()
                    },
                );
                div()
                    .mt(px(15.0))
                    .w_full()
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .min_h(px(60.0))
                            .px(px(20.0))
                            .py(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(24.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(sp(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(tr!("settings.completion_sound")),
                                    )
                                    .child(
                                        div()
                                            .mt(px(5.0))
                                            .text_size(sp(12.5))
                                            .line_height(sp(18.0))
                                            .text_color(theme.text_secondary)
                                            .child(tr!("settings.completion_sound_description")),
                                    ),
                            )
                            .child(toggle_switch(
                                "completion-sound-toggle",
                                enabled,
                                false,
                                theme,
                                cx,
                                move |this, _, cx| this.set_completion_sound_enabled(!enabled, cx),
                            )),
                    )
                    .when(enabled, |card| {
                        card.child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
                            .child(
                                div()
                                    .w_full()
                                    .min_h(px(52.0))
                                    .px(px(20.0))
                                    .py(px(10.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(24.0))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(sp(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(tr!("settings.completion_sound_name")),
                                    )
                                    .child(sound_selector),
                            )
                            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
                            .child(
                                div()
                                    .w_full()
                                    .min_h(px(52.0))
                                    .px(px(20.0))
                                    .py(px(10.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(12.0))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(sp(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(tr!("settings.completion_sound_volume")),
                                    )
                                    .child(volume_slider.w(px(140.0)).flex_none())
                                    .child(
                                        div()
                                            .w(px(32.0))
                                            .flex_none()
                                            .flex()
                                            .justify_end()
                                            .text_size(sp(12.5))
                                            .text_color(theme.text_secondary)
                                            .child(format!(
                                                "{}%",
                                                (volume_shown * 100.0).round() as i32
                                            )),
                                    ),
                            )
                    })
            })
            .when(updater_available, |column| {
                let enabled = self.automatic_updates_enabled;
                let toggle = toggle_switch(
                    "automatic-updates-toggle",
                    enabled,
                    false,
                    theme,
                    cx,
                    move |this, _, cx| this.set_automatic_updates_enabled(!enabled, cx),
                );
                column.child(
                    div()
                        .mt(px(15.0))
                        .w_full()
                        .min_h(px(60.0))
                        .px(px(20.0))
                        .py(px(12.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .flex()
                        .items_center()
                        .gap(px(24.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(sp(13.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child(tr!("settings.automatic_updates")),
                                )
                                .child(
                                    div()
                                        .mt(px(5.0))
                                        .text_size(sp(12.5))
                                        .line_height(sp(18.0))
                                        .text_color(theme.text_secondary)
                                        .child(tr!("settings.automatic_updates_description")),
                                ),
                        )
                        .child(toggle),
                )
            })
            .into_any_element()
    }

    fn set_analytics_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.analytics_enabled = enabled;
        self.analytics.set_enabled(enabled);
        self.save();
        cx.notify();
    }

    fn set_completion_sound_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.completion_sound_enabled == enabled {
            return;
        }
        if !enabled {
            self.completion_volume_slider.cancel();
        }
        self.state.completion_sound_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_completion_sound(&mut self, sound: CompletionSound, cx: &mut Context<Self>) {
        if self.state.completion_sound != sound {
            self.state.completion_sound = sound;
            self.save();
        }
        // Picking from the menu previews the sound.
        crate::platform::play_completion_sound(sound, self.state.completion_sound_volume);
        cx.notify();
    }

    fn set_completion_sound_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = crate::persistence::sanitized_completion_sound_volume(volume);
        if self.state.completion_sound_volume == volume {
            return;
        }
        self.state.completion_sound_volume = volume;
        self.save();
        // Committing a new level previews the current sound at it.
        crate::platform::play_completion_sound(self.state.completion_sound, volume);
        cx.notify();
    }

    fn set_automatic_updates_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.automatic_updates_enabled = enabled;
        if let Some(updater) = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|updater| updater.0.as_ref())
        {
            updater.set_automatically_checks_for_updates(enabled);
        }
        cx.notify();
    }

    /// The Commands page's open editor form. Input entities live here rather
    /// than in `Waku::new` because they only exist while the form is open.
    pub(super) fn open_custom_command_editor(
        &mut self,
        command: Option<&CustomCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("commands.name"))
                .placeholder(tr!("commands.name_placeholder"));
            if let Some(name) = command.and_then(|command| command.name.as_deref()) {
                input.set_content(name, cx);
            }
            input
        });
        let shell = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("commands.shell"))
                .placeholder(tr!("commands.shell_placeholder"));
            if let Some(shell) = command.and_then(|command| command.shell.as_deref()) {
                input.set_content(shell, cx);
            }
            input
        });
        let script = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .multi_line()
                .auto_height()
                .max_lines(10)
                .syntax(Some("shell"))
                .accessibility_label(tr!("commands.script"))
                .placeholder(tr!("commands.script_placeholder"));
            if let Some(command) = command {
                input.set_content(command.script.clone(), cx);
            }
            input
        });
        self.custom_command_editor = Some(CustomCommandEditor {
            id: command.map(|command| command.id),
            name: name.clone(),
            icon: command.map(|command| command.icon).unwrap_or_default(),
            shell,
            script,
            close_on_success: command.is_some_and(|command| command.close_on_success),
            previous_script: command.map(|command| command.script.clone()),
            script_required: false,
            exit_settings_on_save: false,
        });
        let focus = name.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn save_custom_command_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.custom_command_editor else {
            return;
        };
        let script = editor.script.read(cx).content().trim_end().to_owned();
        if script.trim().is_empty() {
            self.custom_command_editor.as_mut().unwrap().script_required = true;
            cx.notify();
            return;
        }
        let name = editor.name.read(cx).content().trim().to_owned();
        let shell = editor.shell.read(cx).content().trim().to_owned();
        let previous_script = editor.previous_script.clone();
        let exit_settings_on_save = editor.exit_settings_on_save;
        let command = CustomCommand {
            id: editor.id.unwrap_or_else(Uuid::new_v4),
            name: (!name.is_empty()).then_some(name),
            icon: editor.icon,
            shell: (!shell.is_empty()).then_some(shell),
            script,
            close_on_success: editor.close_on_success,
            // A human editing an agent's command keeps its attribution.
            created_by_task: editor
                .id
                .and_then(|id| {
                    self.state
                        .custom_commands
                        .iter()
                        .find(|existing| existing.id == id)
                })
                .and_then(|existing| existing.created_by_task),
        };
        if let Some(index) = self
            .state
            .custom_commands
            .iter()
            .position(|existing| existing.id == command.id)
        {
            self.state.custom_commands[index] = command;
        } else {
            self.state.custom_commands.push(command);
        }
        self.custom_command_editor = None;
        // The old text's hashed script file is only safe to drop once the
        // list no longer carries it.
        if let Some(previous_script) = previous_script {
            crate::custom_commands::remove_script_if_unreferenced(
                &previous_script,
                &self.state.custom_commands,
            );
        }
        self.save();
        if exit_settings_on_save {
            self.settings_page = None;
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn delete_custom_command(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self
            .state
            .custom_commands
            .iter()
            .position(|command| command.id == id)
        else {
            return;
        };
        let removed = self.state.custom_commands.remove(index);
        if self
            .custom_command_editor
            .as_ref()
            .is_some_and(|editor| editor.id == Some(id))
        {
            self.custom_command_editor = None;
        }
        crate::custom_commands::remove_script_if_unreferenced(
            &removed.script,
            &self.state.custom_commands,
        );
        self.save();
        cx.notify();
    }

    pub(super) fn open_remote_host_editor(
        &mut self,
        host: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let record = host.and_then(|id| {
            self.state
                .remote_hosts
                .iter()
                .find(|record| record.id == id)
        });
        let name = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_name"))
                .placeholder(tr!("daemon.remote_host_name_placeholder"));
            if let Some(name) = record.map(|record| record.name.as_str()) {
                input.set_content(name, cx);
            }
            input
        });
        let address = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_address"))
                .placeholder(tr!("daemon.remote_host_address_placeholder"));
            if let Some(address) = record.map(|record| record.address.as_str()) {
                input.set_content(address, cx);
            }
            input
        });
        let token = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .masked()
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_token"))
                .placeholder(tr!("daemon.remote_host_token_placeholder"));
            if let Some(token) = record.map(|record| record.token.as_str()) {
                input.set_content(token, cx);
            }
            input
        });
        let destination = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .tab_index(0)
                .accessibility_label(tr!("daemon.remote_host_ssh"))
                .placeholder(tr!("daemon.remote_host_ssh_placeholder"));
            if let Some(ssh) = record.and_then(|record| record.ssh_destination.as_deref()) {
                input.set_content(ssh, cx);
            }
            input
        });
        self.remote_host_editor = Some(RemoteHostEditor {
            id: host,
            name: name.clone(),
            address,
            token,
            destination,
            ssh_hosts: Vec::new(),
            missing_fields: false,
        });
        #[cfg(unix)]
        cx.spawn(async move |waku, cx| {
            let hosts = cx
                .background_executor()
                .spawn(async move { crate::ssh::ssh_config_hosts() })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if let Some(editor) = &mut waku.remote_host_editor {
                    editor.ssh_hosts = hosts;
                    cx.notify();
                }
            });
        })
        .detach();
        let focus = name.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn save_remote_host_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.remote_host_editor else {
            return;
        };
        let name = editor.name.read(cx).content().trim().to_owned();
        let address = editor.address.read(cx).content().trim().to_owned();
        let token = editor.token.read(cx).content().trim().to_owned();
        let destination = editor.destination.read(cx).content().trim().to_owned();
        // An SSH destination is self-contained: the daemon and its token are
        // provisioned on the remote. A direct connection needs both fields.
        if destination.is_empty() && (address.is_empty() || token.is_empty()) {
            self.remote_host_editor.as_mut().unwrap().missing_fields = true;
            cx.notify();
            return;
        }
        let name = if name.is_empty() {
            if destination.is_empty() {
                address.clone()
            } else {
                destination.clone()
            }
        } else {
            name
        };
        let (address, token) = if destination.is_empty() {
            (address, token)
        } else {
            (String::new(), String::new())
        };
        let destination = (!destination.is_empty()).then_some(destination);
        if let Some(id) = editor.id {
            self.update_remote_host(id, name, address, token, destination, cx);
        } else {
            self.add_remote_host(name, address, token, destination, cx);
        }
        self.remote_host_editor = None;
        cx.notify();
    }

    fn render_commands_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let mut column = div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .w_full()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .child(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!("commands.title")),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .text_size(sp(12.5))
                            .line_height(sp(18.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("commands.description")),
                    ),
            );

        if let Some(editor) = &self.custom_command_editor {
            return column
                .child(self.render_custom_command_editor(editor, theme, cx))
                .into_any_element();
        }

        column = column.child(
            div()
                .id("new-custom-command")
                .tab_index(0)
                .w_full()
                .px(px(20.0))
                .py(px(12.0))
                .rounded(px(16.0))
                .bg(theme.raised)
                .flex()
                .items_center()
                .gap(px(10.0))
                .cursor_default()
                .text_size(sp(13.0))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .active(|element| element.bg(theme.overlay_strong))
                .focus_visible(|style| style.border_color(theme.accent))
                .child(icon("icons/plus.svg", 14.0, theme.text_tertiary))
                .child(tr!("commands.new_command"))
                .on_activation(cx, |this, window, cx| {
                    this.open_custom_command_editor(None, window, cx);
                }),
        );

        if self.state.custom_commands.is_empty() {
            column = column.child(
                div()
                    .w_full()
                    .px(px(20.0))
                    .py(px(24.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("commands.empty")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(tr!("commands.empty_description")),
                    ),
            );
        }

        for command in &self.state.custom_commands {
            let id = command.id;
            let label = command.display_name().to_owned();
            let script = command.script.clone();
            let edit_command = command.clone();
            let agent_added = command.created_by_task.is_some();
            column = column.child(
                div()
                    .w_full()
                    .px(px(20.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(icon(
                        crate::custom_commands::icon_path(command.icon),
                        15.0,
                        theme.text_tertiary,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(sp(13.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(label),
                                    )
                                    .when(agent_added, |row| {
                                        row.child(
                                            div()
                                                .flex_none()
                                                .px(px(6.0))
                                                .py(px(1.0))
                                                .rounded(px(5.0))
                                                .bg(theme.overlay)
                                                .text_size(sp(10.5))
                                                .text_color(theme.text_secondary)
                                                .child(tr!("commands.agent_badge")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .truncate()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(script),
                            ),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!("edit-custom-command-{id}")),
                            "icons/pencil.svg",
                            theme,
                        )
                        .tab_index(0)
                        .focus_visible(|style| style.border_color(theme.accent))
                        .tooltip(|window, cx| Tooltip::new(tr!("commands.edit")).build(window, cx))
                        .on_activation(cx, move |this, window, cx| {
                            this.open_custom_command_editor(Some(&edit_command), window, cx);
                        }),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!("delete-custom-command-{id}")),
                            "icons/trash.svg",
                            theme,
                        )
                        .tab_index(0)
                        .focus_visible(|style| style.border_color(theme.accent))
                        .tooltip(|window, cx| {
                            Tooltip::new(tr!("commands.delete")).build(window, cx)
                        })
                        .on_activation(cx, move |this, _, cx| {
                            this.delete_custom_command(id, cx);
                        }),
                    ),
            );
        }

        column.into_any_element()
    }

    fn render_custom_command_editor(
        &self,
        editor: &CustomCommandEditor,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let editing = editor.id.is_some();
        let close_on_success = editor.close_on_success;
        let field_label = |text: String| {
            div()
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(text)
        };
        let field_hint = |text: String| {
            div()
                .mt(px(3.0))
                .text_size(sp(12.0))
                .line_height(sp(15.0))
                .text_color(theme.text_tertiary)
                .child(text)
        };
        let ghost_button = |id: &'static str, label: String| {
            div()
                .id(id)
                .tab_index(0)
                .h(px(27.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|style| style.border_color(theme.accent))
                .child(label)
        };
        let selected_icon = editor.icon;
        let weak = cx.entity().downgrade();
        let icon_handle = self.menu_handle("custom-command-icon-selector", cx);
        let icon_selector = dropdown_menu(
            MenuChip::new("custom-command-icon-selector")
                .label(selected_icon.label())
                .icon(
                    crate::custom_commands::icon_path(selected_icon),
                    theme.text_secondary,
                )
                .outlined()
                .selected(icon_handle.is_open())
                .w(px(150.0))
                .justify_between(),
            "custom-command-icon-selector-menu",
            &icon_handle,
            MenuAlign::BelowRight,
            move |_| {
                CustomCommandIcon::ALL
                    .into_iter()
                    .map(|icon| {
                        let weak = weak.clone();
                        MenuItem::new(icon.label(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(editor) = this.custom_command_editor.as_mut() {
                                    editor.icon = icon;
                                }
                                cx.notify();
                            });
                        })
                        .icon(crate::custom_commands::icon_path(icon))
                        .selected(icon == selected_icon)
                    })
                    .collect()
            },
        );

        div()
            .key_context(CUSTOM_COMMAND_EDITOR_CONTEXT)
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
            .w_full()
            .px(px(20.0))
            .py(px(15.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(sp(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(if editing {
                        tr!("commands.edit_command")
                    } else {
                        tr!("commands.new_command")
                    }),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .child(field_label(tr!("commands.name")))
                    .child(field_hint(tr!("commands.name_description")))
                    .child(div().mt(px(6.0)).child(
                        TextField::new("custom-command-name", editor.name.clone()).w_full(),
                    )),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field_label(tr!("commands.icon")))
                            .child(field_hint(tr!("commands.icon_description"))),
                    )
                    .child(icon_selector),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .child(field_label(tr!("commands.shell")))
                    .child(field_hint(tr!("commands.shell_description")))
                    .child(div().mt(px(6.0)).child(
                        TextField::new("custom-command-shell", editor.shell.clone()).w_full(),
                    )),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .child(field_label(tr!("commands.script")))
                    .child(field_hint(tr!("commands.script_description")))
                    .child(
                        div()
                            .mt(px(6.0))
                            .w_full()
                            .px(px(8.0))
                            .py(px(6.0))
                            .rounded(px(8.0))
                            .border(hairline())
                            .border_color(theme.border_strong)
                            .bg(theme.inset)
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .child(editor.script.clone()),
                    )
                    .when(editor.script_required, |element| {
                        element.child(
                            div()
                                .mt(px(4.0))
                                .text_size(sp(12.0))
                                .text_color(theme.danger)
                                .child(tr!("commands.script_required")),
                        )
                    }),
            )
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field_label(tr!("commands.close_on_success")))
                            .child(field_hint(tr!("commands.close_on_success_description"))),
                    )
                    .child(toggle_switch(
                        "custom-command-close-on-success",
                        close_on_success,
                        false,
                        theme,
                        cx,
                        move |this, _, cx| {
                            if let Some(editor) = this.custom_command_editor.as_mut() {
                                editor.close_on_success = !editor.close_on_success;
                            }
                            cx.notify();
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        ghost_button("custom-command-cancel", tr!("commands.cancel"))
                            .on_activation(cx, |this, _, cx| {
                                this.custom_command_editor = None;
                                cx.notify();
                            }),
                    )
                    .child(
                        ghost_button("custom-command-save", tr!("commands.save"))
                            .on_activation(cx, |this, window, cx| {
                                this.save_custom_command_editor(window, cx)
                            }),
                    ),
            )
    }

    fn render_daemon_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let agent_tools_card = self.agent_tools_card(theme, cx);
        let agent_settings_card = self.agent_settings_card(theme, cx);
        let remote_hosts_card = self.render_remote_hosts_card(theme, cx);
        if self.daemon.is_externally_managed() {
            return div()
                .mt(px(15.0))
                .w_full()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(remote_hosts_card)
                .child(
                    div()
                        .px(px(20.0))
                        .py(px(16.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .child(
                            div()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(tr!("daemon.external_title")),
                        )
                        .child(
                            div()
                                .mt(px(5.0))
                                .text_size(sp(12.5))
                                .line_height(sp(18.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("daemon.external_description")),
                        ),
                )
                .child(agent_tools_card)
                .child(agent_settings_card)
                .into_any_element();
        }

        let enabled = self.state.daemon_exposure.enabled;
        let pending = self.daemon_reconfigure_pending;
        let fields_dirty = self.daemon_exposure_fields_dirty(cx);
        let port = self.state.daemon_exposure.port;
        let websocket_url = format!("ws://{}:{port}", self.daemon_hostname);
        let token = self.state.daemon_exposure.token.clone();

        let exposure_toggle = toggle_switch(
            "daemon-exposure-toggle",
            enabled,
            pending,
            theme,
            cx,
            move |this, _, cx| this.set_daemon_exposure_enabled(!enabled, cx),
        );

        let apply_disabled = pending || !fields_dirty;
        let apply_button = div()
            .id("apply-daemon-settings")
            .tab_index(0)
            .h(px(29.0))
            .px(px(11.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .opacity(if apply_disabled { 0.55 } else { 1.0 })
            .focus_visible(|style| style.border_color(theme.accent))
            .when(!apply_disabled, |element| {
                element
                    .hover(|element| element.bg(theme.overlay))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.apply_daemon_exposure_fields(cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            this.apply_daemon_exposure_fields(cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(if pending {
                tr!("daemon.restarting")
            } else {
                tr!("daemon.apply")
            });

        let copy_url_feedback_id = "daemon-url";
        let url_copied = self.control_was_copied(copy_url_feedback_id);
        let copy_url = websocket_url.clone();
        let copy_url_button = div()
            .id("copy-daemon-url")
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.border_color(theme.accent))
            .hover(|element| element.bg(theme.overlay))
            .child(icon(
                if url_copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                theme.text_tertiary,
            ))
            .child(if url_copied {
                tr!("common.copied")
            } else {
                tr!("common.copy")
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                this.show_control_copied(copy_url_feedback_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(websocket_url.clone()));
                    this.show_control_copied(copy_url_feedback_id, cx);
                    cx.stop_propagation();
                }
            }));

        let copy_token_feedback_id = "daemon-token";
        let token_copied = self.control_was_copied(copy_token_feedback_id);
        let click_token = token.clone();
        let key_token = token.clone();
        let token_revealed = self.daemon_token_revealed;
        let reveal_token_button = div()
            .id("reveal-daemon-token")
            .tab_index(0)
            .size(px(27.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.border_color(theme.accent))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon(
                if token_revealed {
                    "icons/eye-off.svg"
                } else {
                    "icons/eye.svg"
                },
                12.0,
                theme.text_tertiary,
            ))
            .tooltip(Tooltip::text(if token_revealed {
                tr!("daemon.hide_token")
            } else {
                tr!("daemon.reveal_token")
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.daemon_token_revealed = !this.daemon_token_revealed;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.daemon_token_revealed = !this.daemon_token_revealed;
                    cx.stop_propagation();
                    cx.notify();
                }
            }));
        let copy_token_button = div()
            .id("copy-daemon-token")
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.border_color(theme.accent))
            .hover(|element| element.bg(theme.overlay))
            .child(icon(
                if token_copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                theme.text_tertiary,
            ))
            .child(if token_copied {
                tr!("common.copied")
            } else {
                tr!("common.copy")
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(click_token.clone()));
                this.show_control_copied(copy_token_feedback_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(key_token.clone()));
                    this.show_control_copied(copy_token_feedback_id, cx);
                    cx.stop_propagation();
                }
            }));

        let regenerate_button = div()
            .id("regenerate-daemon-token")
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .opacity(if pending { 0.55 } else { 1.0 })
            .focus_visible(|style| style.border_color(theme.accent))
            .when(!pending, |element| {
                element
                    .hover(|element| element.bg(theme.overlay))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.regenerate_daemon_token(cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            this.regenerate_daemon_token(cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(tr!("daemon.regenerate_token"));

        div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .min_h(px(66.0))
                    .px(px(20.0))
                    .py(px(13.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.0))
                                    .child(
                                        div()
                                            .text_size(sp(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(tr!("daemon.expose_title")),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(2.0))
                                            .rounded_full()
                                            .text_size(sp(12.5))
                                            .text_color(if enabled {
                                                theme.success
                                            } else {
                                                theme.text_tertiary
                                            })
                                            .bg(theme.overlay)
                                            .child(if pending {
                                                tr!("daemon.status_restarting")
                                            } else if enabled {
                                                tr!("daemon.status_exposed")
                                            } else {
                                                tr!("daemon.status_local")
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("daemon.expose_description")),
                            ),
                    )
                    .child(exposure_toggle),
            )
            .when(enabled, |column| {
                column.child(
                    div()
                        .px(px(20.0))
                        .py(px(15.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .child(
                            div()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(tr!("daemon.connection_title")),
                        )
                        .child(
                            div()
                                .mt(px(4.0))
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(sp(12.5))
                                .line_height(sp(16.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("daemon.connection_description")),
                        )
                        .child(
                            div()
                                .mt(px(14.0))
                                .flex()
                                .items_start()
                                .gap(px(24.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_size(sp(12.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.text)
                                                .child(tr!("daemon.port")),
                                        )
                                        .child(
                                            div()
                                                .mt(px(3.0))
                                                .whitespace_normal()
                                                .text_size(sp(12.5))
                                                .line_height(sp(14.0))
                                                .text_color(theme.text_tertiary)
                                                .child(tr!("daemon.port_description")),
                                        ),
                                )
                                .child(
                                    div().flex_1().min_w_0().flex().justify_end().child(
                                        TextField::new(
                                            "daemon-port-field",
                                            self.daemon_port_input.clone(),
                                        )
                                        .w(px(150.0)),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .mt(px(14.0))
                                .flex()
                                .items_start()
                                .gap(px(24.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_size(sp(12.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.text)
                                                .child(tr!("daemon.allowed_origins")),
                                        )
                                        .child(
                                            div()
                                                .mt(px(3.0))
                                                .whitespace_normal()
                                                .text_size(sp(12.5))
                                                .line_height(sp(14.0))
                                                .text_color(theme.text_tertiary)
                                                .child(tr!("daemon.allowed_origins_description")),
                                        ),
                                )
                                .child(
                                    div().flex_1().min_w_0().flex().justify_end().child(
                                        TextField::new(
                                            "daemon-origins-field",
                                            self.daemon_origins_input.clone(),
                                        )
                                        .w_full()
                                        .max_w(px(360.0)),
                                    ),
                                ),
                        )
                        .child(div().mt(px(13.0)).flex().justify_end().child(apply_button)),
                )
            })
            .when(enabled, |column| {
                column.child(
                    div()
                        .px(px(20.0))
                        .py(px(15.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .child(
                            div()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(tr!("daemon.credentials_title")),
                        )
                        .child(
                            div()
                                .mt(px(4.0))
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(sp(12.5))
                                .line_height(sp(16.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("daemon.credentials_description")),
                        )
                        .child(
                            div()
                                .mt(px(13.0))
                                .py(px(8.0))
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .w(px(80.0))
                                        .flex_none()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_tertiary)
                                        .child(tr!("daemon.websocket_url")),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_family(crate::fonts::current(cx).code)
                                        .text_size(sp(12.5))
                                        .text_color(theme.text)
                                        .child(SharedString::from(format!(
                                            "ws://{}:{port}",
                                            self.daemon_hostname
                                        ))),
                                )
                                .child(copy_url_button),
                        )
                        .child(
                            div()
                                .py(px(8.0))
                                .border_t(hairline())
                                .border_color(theme.separator)
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .w(px(80.0))
                                        .flex_none()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_tertiary)
                                        .child(tr!("daemon.token")),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_family(crate::fonts::current(cx).code)
                                        .text_size(sp(12.5))
                                        .text_color(theme.text)
                                        .child(SharedString::from(if token_revealed {
                                            token.clone()
                                        } else {
                                            "••••••••••••••••••••••••••••••••".to_owned()
                                        })),
                                )
                                .child(reveal_token_button)
                                .child(copy_token_button)
                                .child(regenerate_button),
                        )
                        .child(
                            div()
                                .mt(px(7.0))
                                .px(px(10.0))
                                .py(px(8.0))
                                .rounded(px(10.0))
                                .bg(theme.inset)
                                .w_full()
                                .min_w_0()
                                .flex()
                                .gap(px(8.0))
                                .child(icon("icons/alert.svg", 13.0, theme.warning))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .whitespace_normal()
                                        .text_size(sp(12.5))
                                        .line_height(sp(15.0))
                                        .text_color(theme.text_secondary)
                                        .child(tr!("daemon.security_warning")),
                                ),
                        ),
                )
            })
            .child(remote_hosts_card)
            .child(agent_tools_card)
            .child(agent_settings_card)
            .into_any_element()
    }

    /// Saved remote daemons merged into this window's catalog. Each row shows
    /// the record's live connection state; editing re-points the same id so
    /// its projects and sessions keep their owner.
    fn render_remote_hosts_card(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut rows = div().flex().flex_col();
        for (index, host) in self.state.remote_hosts.iter().enumerate() {
            let host_id = host.id;
            let error = self.remote_errors.get(&host_id).cloned();
            let status = match self
                .daemons
                .supervisor(waku_client::DaemonKey::Remote(host_id))
            {
                Some(supervisor) => match supervisor.status() {
                    waku_client::DaemonStatus::Connected => {
                        (tr!("daemon.phase_connected"), theme.success)
                    }
                    waku_client::DaemonStatus::Recovering => {
                        (tr!("daemon.phase_connecting"), theme.warning)
                    }
                    waku_client::DaemonStatus::Unreachable => {
                        (tr!("daemon.phase_disconnected"), theme.danger)
                    }
                },
                None => match &error {
                    Some(_) => (tr!("daemon.phase_error"), theme.danger),
                    None => (tr!("daemon.phase_connecting"), theme.text_tertiary),
                },
            };
            let action_button = |id: SharedString, icon_path: &'static str, label: String| {
                div()
                    .id(id)
                    .tab_index(0)
                    .size(px(24.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .text_color(theme.text_secondary)
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .focus_visible(|element| element.border(hairline()).border_color(theme.accent))
                    .tooltip(Tooltip::text(label))
                    .child(icon(icon_path, 13.0, theme.text_tertiary))
            };
            let edit_button = action_button(
                SharedString::from(format!("remote-host-edit-{host_id}")),
                "icons/pencil.svg",
                tr!("daemon.remote_host_edit"),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_remote_host_editor(Some(host_id), window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.open_remote_host_editor(Some(host_id), window, cx);
                    cx.stop_propagation();
                }
            }));
            let remove_button = action_button(
                SharedString::from(format!("remote-host-remove-{host_id}")),
                "icons/trash.svg",
                tr!("daemon.remote_host_remove"),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.remove_remote_host(host_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.remove_remote_host(host_id, cx);
                    cx.stop_propagation();
                }
            }));
            rows = rows.child(
                div()
                    .when(index > 0, |element| {
                        element.border_t(hairline()).border_color(theme.separator)
                    })
                    .py(px(9.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(icon("icons/server.svg", 14.0, theme.text_secondary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(sp(13.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(SharedString::from(host.name.clone())),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded_full()
                                            .text_size(sp(11.5))
                                            .text_color(status.1)
                                            .bg(theme.overlay)
                                            .child(status.0),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(1.0))
                                    .truncate()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(12.0))
                                    .text_color(theme.text_tertiary)
                                    .child(SharedString::from(
                                        host.ssh_destination
                                            .clone()
                                            .unwrap_or_else(|| host.address.clone()),
                                    )),
                            )
                            .when_some(error, |element, error| {
                                element.child(
                                    div()
                                        .mt(px(1.0))
                                        .whitespace_normal()
                                        .line_height(sp(15.0))
                                        .text_size(sp(12.0))
                                        .text_color(theme.danger)
                                        .child(SharedString::from(error)),
                                )
                            }),
                    )
                    .child(edit_button)
                    .child(remove_button),
            );
        }

        let add_button = div()
            .id("remote-host-add")
            .tab_index(0)
            .h(px(27.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|element| element.border_color(theme.accent))
            .child(icon("icons/plus.svg", 12.0, theme.text_tertiary))
            .child(tr!("daemon.remote_host_add"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_remote_host_editor(None, window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.open_remote_host_editor(None, window, cx);
                    cx.stop_propagation();
                }
            }));

        let mut card = div()
            .px(px(20.0))
            .py(px(15.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .child(
                div()
                    .text_size(sp(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(tr!("daemon.remote_hosts_title")),
            )
            .child(
                div()
                    .mt(px(4.0))
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(sp(12.5))
                    .line_height(sp(16.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("daemon.remote_hosts_description")),
            );
        if let Some(editor) = &self.remote_host_editor {
            card = card.child(self.render_remote_host_editor(editor, theme, cx));
        } else {
            card = card
                .when(!self.state.remote_hosts.is_empty(), |card| {
                    card.child(div().mt(px(6.0)).child(rows))
                })
                .child(div().mt(px(12.0)).flex().justify_end().child(add_button));
        }
        card.into_any_element()
    }

    fn render_remote_host_editor(
        &self,
        editor: &RemoteHostEditor,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let editing = editor.id.is_some();
        let field_label = |text: String| {
            div()
                .mt(px(12.0))
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(text)
        };
        let ghost_button = |id: &'static str, label: String| {
            div()
                .id(id)
                .tab_index(0)
                .h(px(27.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .focus_visible(|style| style.border_color(theme.accent))
                .child(label)
        };
        div()
            .mt(px(12.0))
            .pt(px(4.0))
            .border_t(hairline())
            .border_color(theme.separator)
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(if editing {
                        tr!("daemon.remote_host_edit")
                    } else {
                        tr!("daemon.remote_host_new")
                    }),
            )
            .child(field_label(tr!("daemon.remote_host_name")))
            .child(
                div()
                    .mt(px(5.0))
                    .child(TextField::new("remote-host-name", editor.name.clone()).w_full()),
            )
            .child(field_label(tr!("daemon.remote_host_ssh")))
            .child(
                div()
                    .mt(px(5.0))
                    .child(TextField::new("remote-host-ssh", editor.destination.clone()).w_full()),
            )
            .child(
                div()
                    .mt(px(5.0))
                    .whitespace_normal()
                    .text_size(sp(12.0))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("daemon.remote_host_ssh_description")),
            )
            .when(!editor.ssh_hosts.is_empty(), |element| {
                element.child(div().mt(px(7.0)).flex().flex_wrap().gap(px(4.0)).children(
                    editor.ssh_hosts.iter().map(|alias| {
                        let alias = alias.clone();
                        let destination = editor.destination.clone();
                        div()
                            .id(SharedString::from(format!("ssh-host-{alias}")))
                            .tab_index(0)
                            .h(px(22.0))
                            .px(px(7.0))
                            .rounded(px(6.0))
                            .border(hairline())
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .cursor_default()
                            .font_family(crate::fonts::current(cx).code)
                            .text_size(sp(11.5))
                            .text_color(theme.text_secondary)
                            .hover(|element| element.bg(theme.overlay))
                            .focus_visible(|element| element.border_color(theme.accent))
                            .child(alias.clone())
                            .on_click(move |_, _, cx| {
                                destination.update(cx, |input, cx| input.set_content(&alias, cx));
                            })
                    }),
                ))
            })
            .child(field_label(tr!("daemon.remote_host_address")))
            .child(
                div()
                    .mt(px(5.0))
                    .child(TextField::new("remote-host-address", editor.address.clone()).w_full()),
            )
            .child(field_label(tr!("daemon.remote_host_token")))
            .child(
                div()
                    .mt(px(5.0))
                    .child(TextField::new("remote-host-token", editor.token.clone()).w_full()),
            )
            .when(editor.missing_fields, |element| {
                element.child(
                    div()
                        .mt(px(6.0))
                        .text_size(sp(12.0))
                        .text_color(theme.danger)
                        .child(tr!("daemon.remote_host_required")),
                )
            })
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        ghost_button("remote-host-cancel", tr!("common.cancel")).on_activation(
                            cx,
                            |this, _, cx| {
                                this.remote_host_editor = None;
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        ghost_button("remote-host-save", tr!("daemon.remote_host_save"))
                            .on_activation(cx, |this, _, cx| this.save_remote_host_editor(cx)),
                    ),
            )
    }

    /// The daemon-scoped opt-in for agent-to-agent commands. Toggling it only
    /// affects sessions started afterwards — running sessions keep the launch
    /// environment they already have.
    fn agent_tools_card(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.state.agent_tools_enabled;
        let toggle = toggle_switch(
            "agent-tools-toggle",
            enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_agent_tools_enabled(!enabled, cx),
        );
        div()
            .min_h(px(66.0))
            .px(px(20.0))
            .py(px(13.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .flex()
            .items_center()
            .gap(px(24.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!("daemon.agent_tools_title")),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(sp(12.5))
                            .line_height(sp(18.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("daemon.agent_tools_description")),
                    ),
            )
            .child(toggle)
            .into_any_element()
    }

    fn set_agent_tools_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.agent_tools_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The daemon-scoped settings surface agents may write — custom commands
    /// today. On by default; turning it off makes the daemon reject the
    /// `goddard-agent command` calls outright while `create`/`prompt` stay gated
    /// by their own switch above.
    fn agent_settings_card(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.state.agent_settings_enabled;
        let toggle = toggle_switch(
            "agent-settings-toggle",
            enabled,
            false,
            theme,
            cx,
            move |this, _, cx| this.set_agent_settings_enabled(!enabled, cx),
        );
        div()
            .min_h(px(66.0))
            .px(px(20.0))
            .py(px(13.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .flex()
            .items_center()
            .gap(px(24.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!("daemon.agent_settings_title")),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(sp(12.5))
                            .line_height(sp(18.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("daemon.agent_settings_description")),
                    ),
            )
            .child(toggle)
            .into_any_element()
    }

    fn set_agent_settings_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.agent_settings_enabled = enabled;
        self.save();
        cx.notify();
    }

    /// The Experiments page: one opt-in card per unfinished feature, each
    /// defaulting off. Subagents is daemon-owned — its flag travels with the
    /// daemon settings `save()` already syncs.
    fn render_experiments_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        div()
            .child(
                div()
                    .mt(px(15.0))
                    .w_full()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(18.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("settings.experiments_description")),
                    ),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(self.experiment_card(
                        "big-picture-experiment-toggle",
                        "experiments.big_picture_title",
                        "experiments.big_picture_description",
                        self.state.big_picture_enabled,
                        theme,
                        cx,
                        |this, enabled, cx| this.set_big_picture_enabled(enabled, cx),
                    ))
                    .child(self.experiment_card(
                        "git-panel-experiment-toggle",
                        "experiments.git_panel_title",
                        "experiments.git_panel_description",
                        self.state.git_panel_enabled,
                        theme,
                        cx,
                        |this, enabled, cx| this.set_git_panel_enabled(enabled, cx),
                    ))
                    .child(self.experiment_card(
                        "github-experiment-toggle",
                        "experiments.github_title",
                        "experiments.github_description",
                        self.state.github_enabled,
                        theme,
                        cx,
                        |this, enabled, cx| this.set_github_enabled(enabled, cx),
                    ))
                    .child(self.experiment_card(
                        "projects-page-experiment-toggle",
                        "experiments.projects_page_title",
                        "experiments.projects_page_description",
                        self.state.projects_page_enabled,
                        theme,
                        cx,
                        |this, enabled, cx| this.set_projects_page_enabled(enabled, cx),
                    ))
                    .child(self.experiment_card(
                        "subagents-experiment-toggle",
                        "experiments.subagents_title",
                        "experiments.subagents_description",
                        self.state.subagents_enabled,
                        theme,
                        cx,
                        |this, enabled, cx| this.set_subagents_enabled(enabled, cx),
                    )),
            )
            .into_any_element()
    }

    fn experiment_card(
        &self,
        id: &'static str,
        title_key: &'static str,
        description_key: &'static str,
        enabled: bool,
        theme: Theme,
        cx: &mut Context<Self>,
        set: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let toggle = toggle_switch(id, enabled, false, theme, cx, move |this, _, cx| {
            set(this, !enabled, cx)
        });
        div()
            .min_h(px(66.0))
            .px(px(20.0))
            .py(px(13.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .flex()
            .items_center()
            .gap(px(24.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!(title_key)),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(sp(12.5))
                            .line_height(sp(18.0))
                            .text_color(theme.text_secondary)
                            .child(tr!(description_key)),
                    ),
            )
            .child(toggle)
            .into_any_element()
    }

    fn set_big_picture_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.dismiss_big_picture(cx);
        }
        self.state.big_picture_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_git_panel_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.close_git_panel_state();
        }
        self.state.git_panel_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_github_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.notifications.reset();
        }
        self.state.github_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_projects_page_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.close_projects_page(cx);
        }
        self.state.projects_page_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn set_subagents_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.subagents_enabled = enabled;
        self.save();
        cx.notify();
    }

    fn daemon_exposure_from_fields(
        &self,
        cx: &App,
    ) -> Result<waku_client::DaemonExposureSettings, String> {
        let port = self
            .daemon_port_input
            .read(cx)
            .content()
            .trim()
            .parse::<u16>()
            .map_err(|_| tr!("daemon.invalid_port"))?;
        if port == 0 {
            return Err(tr!("daemon.invalid_port"));
        }
        let origins = self.daemon_origins_input.read(cx).content().to_owned();
        let mut settings = self.state.daemon_exposure.clone();
        settings.port = port;
        settings
            .with_allowed_origins_text(&origins)
            .and_then(waku_client::DaemonExposureSettings::validate)
            .map_err(|error| error.to_string())
    }

    fn daemon_exposure_fields_dirty(&self, cx: &App) -> bool {
        self.daemon_exposure_from_fields(cx)
            .map(|settings| {
                settings.port != self.state.daemon_exposure.port
                    || settings.allowed_origins != self.state.daemon_exposure.allowed_origins
            })
            .unwrap_or(true)
    }

    fn set_daemon_exposure_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !enabled {
            self.daemon_token_revealed = false;
        }
        let settings = if enabled {
            match self.daemon_exposure_from_fields(cx) {
                Ok(mut settings) => {
                    settings.enabled = true;
                    settings
                }
                Err(error) => {
                    self.show_toast(tr!("daemon.invalid_settings", error = error));
                    return;
                }
            }
        } else {
            let mut settings = self.state.daemon_exposure.clone();
            settings.enabled = false;
            settings
        };
        self.apply_daemon_exposure(settings, cx);
    }

    pub(super) fn apply_daemon_exposure_fields(&mut self, cx: &mut Context<Self>) {
        let settings = match self.daemon_exposure_from_fields(cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.show_toast(tr!("daemon.invalid_settings", error = error));
                return;
            }
        };
        self.apply_daemon_exposure(settings, cx);
    }

    fn regenerate_daemon_token(&mut self, cx: &mut Context<Self>) {
        let mut settings = match self.daemon_exposure_from_fields(cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.show_toast(tr!("daemon.invalid_settings", error = error));
                return;
            }
        };
        settings.token = waku_client::DaemonExposureSettings::new_token();
        self.daemon_token_revealed = false;
        self.apply_daemon_exposure(settings, cx);
    }

    fn apply_daemon_exposure(
        &mut self,
        settings: waku_client::DaemonExposureSettings,
        cx: &mut Context<Self>,
    ) {
        if self.daemon_reconfigure_pending || settings == self.state.daemon_exposure {
            return;
        }
        if self.daemon.is_externally_managed() {
            self.show_toast(tr!("daemon.external_description"));
            return;
        }
        if self
            .state
            .sessions
            .iter()
            .any(|session| !matches!(session.status, SessionStatus::Idle | SessionStatus::Failed))
        {
            self.show_toast(tr!("daemon.stop_active_tasks"));
            return;
        }

        let needs_restart = self.state.daemon_exposure.enabled || settings.enabled;
        if !needs_restart {
            self.state.daemon_exposure = settings;
            self.save();
            cx.notify();
            return;
        }

        self.daemon_reconfigure_pending = true;
        let daemon = self.daemon.clone();
        let applied = settings.clone();
        let restart = cx
            .background_executor()
            .spawn(async move { daemon.reconfigure(settings) });
        cx.spawn(async move |this, cx| {
            let result = restart.await;
            let _ = this.update(cx, |this, cx| {
                this.daemon_reconfigure_pending = false;
                match result {
                    Ok(()) => {
                        this.state.daemon_exposure = applied.clone();
                        this.runtimes.clear();
                        this.daemon_port_input.update(cx, |input, cx| {
                            input.set_content(applied.port.to_string(), cx)
                        });
                        this.daemon_origins_input.update(cx, |input, cx| {
                            input.set_content(applied.allowed_origins_text(), cx)
                        });
                        this.save();
                        this.show_success_toast(tr!("daemon.settings_applied"));
                    }
                    Err(error) => {
                        this.show_toast(tr!("daemon.restart_failed", error = error.to_string()))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_archived_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let query = self
            .archived_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();

        let mut archived = self
            .state
            .sessions
            .iter()
            .filter(|session| session.archived_at.is_some())
            .collect::<Vec<_>>();
        archived.sort_by_key(|session| std::cmp::Reverse(session.archived_at));
        let any_archived = !archived.is_empty();

        // The selector offers the projects the archive actually spans, in
        // name order, and earns its slot once a choice would narrow
        // anything — or while a set filter needs clearing.
        let mut project_options: Vec<(Uuid, String)> = Vec::new();
        for session in &archived {
            if project_options
                .iter()
                .any(|(id, _)| *id == session.project_id)
            {
                continue;
            }
            project_options.push((
                session.project_id,
                self.project_display_name(session.project_id),
            ));
        }
        project_options.sort_by_cached_key(|(_, name)| name.to_lowercase());
        let project_names: HashMap<Uuid, String> = project_options
            .iter()
            .map(|(id, name)| (*id, name.to_lowercase()))
            .collect();
        // A filter whose project left the archive matches nothing; treat it
        // as cleared so it cannot silently hide every row.
        let project_filter = self
            .archived_project_filter
            .filter(|id| project_names.contains_key(id));

        let visible = filter_archived_sessions(&archived, &query, project_filter, &project_names);
        self.sync_archived_session_rows(&visible);

        let mut page = div()
            .mt(px(15.0))
            .w_full()
            .flex_1()
            .min_h_0()
            .pb(px(32.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .text_size(sp(12.5))
                    .line_height(sp(18.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("settings.archived_description")),
            );
        if !any_archived {
            return page
                .child(
                    div()
                        .mt(px(12.0))
                        .text_size(sp(13.0))
                        .text_color(theme.text_tertiary)
                        .child(tr!("settings.archived_empty")),
                )
                .into_any_element();
        }

        let mut toolbar = div()
            .mt(px(14.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                TextField::new("archived-search-field", self.archived_search.clone())
                    .icon("icons/search.svg", 13.0)
                    .flex_1()
                    .min_w_0(),
            );
        if project_options.len() > 1 || project_filter.is_some() {
            let weak = cx.entity().downgrade();
            let selected_label = project_filter
                .and_then(|filter| {
                    project_options
                        .iter()
                        .find(|(id, _)| *id == filter)
                        .map(|(_, name)| name.clone())
                })
                .unwrap_or_else(|| tr!("settings.archived_all_projects"));
            let menu_handle = self.menu_handle("archived-project-selector", cx);
            toolbar = toolbar.child(dropdown_menu(
                MenuChip::new("archived-project-selector")
                    .icon("icons/folder.svg", theme.text_tertiary)
                    .label(selected_label)
                    .outlined()
                    .selected(menu_handle.is_open())
                    .max_w(px(220.0))
                    .flex_none(),
                "archived-project-selector-menu",
                &menu_handle,
                MenuAlign::BelowRight,
                move |_| {
                    let mut items = Vec::with_capacity(project_options.len() + 1);
                    items.push(
                        MenuItem::new(tr!("settings.archived_all_projects"), {
                            let weak = weak.clone();
                            move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.archived_project_filter = None;
                                    cx.notify();
                                });
                            }
                        })
                        .selected(project_filter.is_none()),
                    );
                    items.extend(project_options.iter().map(|(project_id, name)| {
                        let project_id = *project_id;
                        let weak = weak.clone();
                        MenuItem::new(name.clone(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.archived_project_filter = Some(project_id);
                                cx.notify();
                            });
                        })
                        .selected(project_filter == Some(project_id))
                    }));
                    items
                },
            ));
        }
        page = page.child(toolbar);

        if visible.is_empty() {
            page = page.child(
                div()
                    .mt(px(12.0))
                    .text_size(sp(13.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("settings.archived_no_match")),
            );
        } else {
            let entity = cx.entity().downgrade();
            page = page.child(
                div()
                    .mt(px(10.0))
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        list(
                            self.archived_sessions_list.clone(),
                            move |index, _window, cx| {
                                entity
                                    .upgrade()
                                    .map(|entity| {
                                        entity.update(cx, |this, cx| {
                                            this.archived_session_row(index, cx)
                                        })
                                    })
                                    .unwrap_or_else(|| div().into_any_element())
                            },
                        )
                        .size_full(),
                    )
                    .child(scrollbar::vertical(
                        &self.archived_sessions_list,
                        &self.archived_sessions_scrollbar,
                    )),
            );
        }
        page.into_any_element()
    }

    /// A project's display name, falling back to the "No project" label when
    /// the archived session's project is gone.
    fn project_display_name(&self, project_id: Uuid) -> String {
        self.state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(Project::display_name)
            .unwrap_or_else(|| tr!("project.no_project_name"))
    }

    /// Keep the virtualized archived list in sync with the filtered session
    /// ids. Filtering preserves order, so an unchanged prefix splices only
    /// the tail and the scroll position survives typing in the filter.
    fn sync_archived_session_rows(&self, sessions: &[Uuid]) {
        let mut cached = self.archived_session_rows.borrow_mut();
        if cached.as_slice() == sessions {
            return;
        }
        let prefix = cached
            .iter()
            .zip(sessions.iter())
            .take_while(|(cached, fresh)| cached == fresh)
            .count();
        let old_count = cached.len();
        *cached = sessions.to_vec();
        if old_count == 0 {
            self.archived_sessions_list
                .reset_with_uniform_height(sessions.len(), px(ARCHIVED_SESSION_ROW_HEIGHT));
        } else {
            self.archived_sessions_list
                .splice(prefix..old_count, sessions.len() - prefix);
            // Newly inserted rows have no measured height yet; the uniform
            // hint keeps the scrollbar's total height honest.
            self.archived_sessions_list
                .clone()
                .with_uniform_item_height(px(ARCHIVED_SESSION_ROW_HEIGHT));
        }
    }

    /// One archived-chat row, built only while visible. Reads the per-frame
    /// id cache; a stale index from a frame racing a state change renders as
    /// an empty row for that frame rather than panicking.
    fn archived_session_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let rows = self.archived_session_rows.borrow();
        let Some(session_id) = rows.get(row).copied() else {
            return div().into_any_element();
        };
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return div().into_any_element();
        };
        let group = SharedString::from(format!("archived-chat-{session_id}"));
        let project_name = self.project_display_name(session.project_id);
        let updated =
            super::sidebar::format_time_ago(unix_time().saturating_sub(session.updated_at));
        let detail = if session.landed_at.is_some() {
            format!("{project_name} · {updated} · {}", tr!("settings.archived_landed"))
        } else {
            format!("{project_name} · {updated}")
        };

        let unarchive_button = div()
            .id(SharedString::from(format!(
                "archived-chat-unarchive-{session_id}"
            )))
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            // Hover reveals the actions; keyboard focus has to reach the
            // same buttons, so focus makes them visible too.
            .opacity(0.0)
            .group_hover(group.clone(), |element| element.opacity(1.0))
            .focus_visible(|element| {
                element
                    .opacity(1.0)
                    .border(hairline())
                    .border_color(theme.accent)
            })
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(tr!("common.unarchive"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.unarchive_session(session_id, true, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.unarchive_session(session_id, true, cx);
                    cx.stop_propagation();
                }
            }));

        let remove_button = div()
            .id(SharedString::from(format!(
                "archived-chat-remove-{session_id}"
            )))
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.danger)
            .opacity(0.0)
            .group_hover(group.clone(), |element| element.opacity(1.0))
            .focus_visible(|element| {
                element
                    .opacity(1.0)
                    .border(hairline())
                    .border_color(theme.accent)
            })
            .hover(|element| element.bg(theme.danger.opacity(0.12)))
            .active(|element| element.bg(theme.danger.opacity(0.18)))
            .child(tr!("common.remove"))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.remove_session(session_id, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.remove_session(session_id, window, cx);
                    cx.stop_propagation();
                }
            }));

        div()
            .group(group)
            .w_full()
            .px(px(12.0))
            .py(px(7.0))
            .rounded(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .hover(|element| element.bg(theme.sidebar_item_background))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(session.display_title().to_owned()),
                    )
                    .child(
                        div()
                            .mt(px(1.0))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(sp(11.5))
                            .text_color(theme.text_tertiary)
                            .when(session.landed_at.is_some(), |element| {
                                element.child(icon("icons/check.svg", 11.0, theme.success))
                            })
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(unarchive_button)
                    .child(remove_button),
            )
            .into_any_element()
    }

    fn render_appearance_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let theme_settings = self.state.theme;
        let match_system = theme_settings.mode == ThemeMode::System;
        let selected_language = self.state.language;

        let weak = cx.entity().downgrade();
        let restore_preview = Self::theme_preview_restore(cx);
        let mode_handle = self.menu_handle_with("appearance-mode-selector", cx, restore_preview);
        let mode_selector = dropdown_menu(
            MenuChip::new("appearance-mode-selector")
                .label(match theme_settings.mode {
                    ThemeMode::Dark => tr!("settings.theme_dark"),
                    _ => tr!("settings.theme_light"),
                })
                .outlined()
                .selected(mode_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "appearance-mode-selector-menu",
            &mode_handle,
            MenuAlign::BelowRight,
            move |_| {
                [ThemeMode::Light, ThemeMode::Dark]
                    .into_iter()
                    .map(|mode| {
                        let weak = weak.clone();
                        MenuItem::new(
                            match mode {
                                ThemeMode::Dark => tr!("settings.theme_dark"),
                                _ => tr!("settings.theme_light"),
                            },
                            {
                                let weak = weak.clone();
                                move |window, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.update_theme_settings(
                                            |settings| settings.mode = mode,
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            },
                        )
                        .selected(mode == theme_settings.mode)
                        .on_highlight(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.preview_theme_settings(
                                    |settings| settings.mode = mode,
                                    window,
                                    cx,
                                );
                            });
                        })
                    })
                    .collect()
            },
        );

        let weak = cx.entity().downgrade();
        let slot_preview = Self::theme_slot_preview(ThemeMode::Light, cx);
        let light_handle = self.menu_handle_with("light-theme-selector", cx, slot_preview);
        let light_theme_selector = dropdown_menu(
            MenuChip::new("light-theme-selector")
                .label(theme_settings.light.label())
                .outlined()
                .selected(light_handle.is_open())
                .w(px(160.0))
                .justify_between(),
            "light-theme-selector-menu",
            &light_handle,
            MenuAlign::BelowRight,
            move |_| {
                ThemeName::LIGHT
                    .into_iter()
                    .map(|name| {
                        let weak = weak.clone();
                        MenuItem::new(name.label(), {
                            let weak = weak.clone();
                            move |window, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.update_theme_settings(
                                        |settings| settings.light = name,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        })
                        .selected(name == theme_settings.light)
                        .on_highlight(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.preview_theme_settings(
                                    |settings| {
                                        // A palette only shows while its slot
                                        // is live, so the preview claims the
                                        // slot too — otherwise light options
                                        // would stay invisible in dark mode.
                                        settings.light = name;
                                        settings.mode = ThemeMode::Light;
                                    },
                                    window,
                                    cx,
                                );
                            });
                        })
                    })
                    .collect()
            },
        );

        let weak = cx.entity().downgrade();
        let slot_preview = Self::theme_slot_preview(ThemeMode::Dark, cx);
        let dark_handle = self.menu_handle_with("dark-theme-selector", cx, slot_preview);
        let dark_theme_selector = dropdown_menu(
            MenuChip::new("dark-theme-selector")
                .label(theme_settings.dark.label())
                .outlined()
                .selected(dark_handle.is_open())
                .w(px(160.0))
                .justify_between(),
            "dark-theme-selector-menu",
            &dark_handle,
            MenuAlign::BelowRight,
            move |_| {
                ThemeName::DARK
                    .into_iter()
                    .map(|name| {
                        let weak = weak.clone();
                        MenuItem::new(name.label(), {
                            let weak = weak.clone();
                            move |window, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.update_theme_settings(
                                        |settings| settings.dark = name,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        })
                        .selected(name == theme_settings.dark)
                        .on_highlight(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.preview_theme_settings(
                                    |settings| {
                                        // A palette only shows while its slot
                                        // is live, so the preview claims the
                                        // slot too — otherwise dark options
                                        // would stay invisible in light mode.
                                        settings.dark = name;
                                        settings.mode = ThemeMode::Dark;
                                    },
                                    window,
                                    cx,
                                );
                            });
                        })
                    })
                    .collect()
            },
        );

        let ui_font_selector = self.font_family_selector(FontTarget::Ui, cx);
        let code_font_selector = self.font_family_selector(FontTarget::Code, cx);

        let selected_ui_font_size = self.state.ui_font_size;
        let weak = cx.entity().downgrade();
        let ui_font_size_handle = self.menu_handle("ui-font-size-selector", cx);
        let ui_font_size_selector = dropdown_menu(
            MenuChip::new("ui-font-size-selector")
                .label(font_size_label(selected_ui_font_size))
                .outlined()
                .selected(ui_font_size_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "ui-font-size-selector-menu",
            &ui_font_size_handle,
            MenuAlign::BelowRight,
            move |_| {
                FONT_SIZES
                    .into_iter()
                    .map(|size| {
                        let weak = weak.clone();
                        MenuItem::new(font_size_label(size), move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_ui_font_size(size, window, cx);
                            });
                        })
                        .selected(size == selected_ui_font_size)
                    })
                    .collect()
            },
        );

        let selected_code_font_size = self.state.code_font_size;
        let weak = cx.entity().downgrade();
        let code_font_size_handle = self.menu_handle("code-font-size-selector", cx);
        let code_font_size_selector = dropdown_menu(
            MenuChip::new("code-font-size-selector")
                .label(font_size_label(selected_code_font_size))
                .outlined()
                .selected(code_font_size_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "code-font-size-selector-menu",
            &code_font_size_handle,
            MenuAlign::BelowRight,
            move |_| {
                FONT_SIZES
                    .into_iter()
                    .map(|size| {
                        let weak = weak.clone();
                        MenuItem::new(font_size_label(size), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_code_font_size(size, cx);
                            });
                        })
                        .selected(size == selected_code_font_size)
                    })
                    .collect()
            },
        );

        let selected_terminal_font_size = self.state.terminal_font_size();
        let weak = cx.entity().downgrade();
        let terminal_font_size_handle = self.menu_handle("terminal-font-size-selector", cx);
        let terminal_font_size_selector = dropdown_menu(
            MenuChip::new("terminal-font-size-selector")
                .label(font_size_label(selected_terminal_font_size))
                .outlined()
                .selected(terminal_font_size_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "terminal-font-size-selector-menu",
            &terminal_font_size_handle,
            MenuAlign::BelowRight,
            move |_| {
                FONT_SIZES
                    .into_iter()
                    .map(|size| {
                        let weak = weak.clone();
                        MenuItem::new(font_size_label(size), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_terminal_font_size(size, cx);
                            });
                        })
                        .selected(size == selected_terminal_font_size)
                    })
                    .collect()
            },
        );

        let weak = cx.entity().downgrade();
        let language_handle = self.menu_handle("language-selector", cx);
        let language_selector = dropdown_menu(
            MenuChip::new("language-selector")
                .label(selected_language.label())
                .outlined()
                .selected(language_handle.is_open())
                .w(px(116.0))
                .justify_between(),
            "language-selector-menu",
            &language_handle,
            MenuAlign::BelowRight,
            move |_| {
                crate::i18n::AppLanguage::ALL
                    .into_iter()
                    .map(|language| {
                        let weak = weak.clone();
                        MenuItem::new(language.label(), move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_language(language, window, cx);
                            });
                        })
                        .selected(language == selected_language)
                    })
                    .collect()
            },
        );

        div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .rounded(px(16.0))
            .overflow_hidden()
            .bg(theme.raised)
            .child(settings_row(
                tr!("settings.match_system"),
                tr!("settings.match_system_description"),
                toggle_switch(
                    "match-system-appearance",
                    match_system,
                    false,
                    theme,
                    cx,
                    move |this, window, cx| {
                        // Unchecking freezes the appearance on screen right now.
                        let mode = if match_system {
                            match window.appearance() {
                                gpui::WindowAppearance::Dark
                                | gpui::WindowAppearance::VibrantDark => ThemeMode::Dark,
                                _ => ThemeMode::Light,
                            }
                        } else {
                            ThemeMode::System
                        };
                        this.update_theme_settings(|s| s.mode = mode, window, cx);
                    },
                ),
                theme,
            ))
            .when(!match_system, |element| {
                element
                    .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
                    .child(settings_row(
                        tr!("settings.appearance"),
                        tr!("settings.appearance_mode_description"),
                        mode_selector,
                        theme,
                    ))
            })
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(settings_row(
                tr!("settings.light_theme"),
                tr!("settings.light_theme_description"),
                light_theme_selector,
                theme,
            ))
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(settings_row(
                tr!("settings.dark_theme"),
                tr!("settings.dark_theme_description"),
                dark_theme_selector,
                theme,
            ))
            .when(cfg!(target_os = "macos"), |element| {
                // Vibrancy is a macOS-only effect; on other platforms the
                // sidebar is already a solid fill and there is nothing to
                // switch.
                let transparent = self.state.sidebar_transparency;
                element
                    .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
                    .child(
                        div()
                            .w_full()
                            .min_h(px(60.0))
                            .px(px(20.0))
                            .py(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(24.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(sp(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(tr!("settings.sidebar_transparency")),
                                    )
                                    .child(
                                        div()
                                            .mt(px(5.0))
                                            .text_size(sp(12.5))
                                            .line_height(sp(18.0))
                                            .text_color(theme.text_secondary)
                                            .child(tr!(
                                                "settings.sidebar_transparency_description"
                                            )),
                                    ),
                            )
                            .child(toggle_switch(
                                "sidebar-transparency-toggle",
                                transparent,
                                false,
                                theme,
                                cx,
                                move |this, window, cx| {
                                    this.set_sidebar_transparency(!transparent, window, cx)
                                },
                            )),
                    )
            })
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("language.title")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("language.description")),
                            ),
                    )
                    .child(language_selector),
            )
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(settings_row(
                tr!("settings.ui_font"),
                tr!("settings.ui_font_description"),
                ui_font_selector,
                theme,
            ))
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(settings_row(
                tr!("settings.code_font"),
                tr!("settings.code_font_description"),
                code_font_selector,
                theme,
            ))
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.ui_font_size")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.ui_font_size_description")),
                            ),
                    )
                    .child(ui_font_size_selector),
            )
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.code_font_size")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.code_font_size_description")),
                            ),
                    )
                    .child(code_font_size_selector),
            )
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("settings.terminal_font_size")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("settings.terminal_font_size_description")),
                            ),
                    )
                    .child(terminal_font_size_selector),
            )
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(settings_row(
                tr!("settings.thick_borders"),
                tr!("settings.thick_borders_description"),
                toggle_switch(
                    "thick-borders-toggle",
                    self.state.thick_borders,
                    false,
                    theme,
                    cx,
                    {
                        let enabled = self.state.thick_borders;
                        move |this, _, cx| this.set_thick_borders(!enabled, cx)
                    },
                ),
                theme,
            ))
            .child(div().mx(px(20.0)).h(hairline()).bg(theme.separator))
            .child(settings_row(
                tr!("settings.high_contrast"),
                tr!("settings.high_contrast_description"),
                toggle_switch(
                    "high-contrast-toggle",
                    self.state.high_contrast || crate::platform::increase_contrast(),
                    crate::platform::increase_contrast(),
                    theme,
                    cx,
                    {
                        let enabled = self.state.high_contrast;
                        move |this, window, cx| this.set_high_contrast(!enabled, window, cx)
                    },
                ),
                theme,
            ))
            .into_any_element()
    }

    /// The Appearance page's family dropdown: a filter field pinned above a
    /// virtualized list of installed families. The field holds real focus
    /// while arrows move a drawn cursor — `up`/`down`/`enter` reach this card
    /// as actions under the `WakuMenu > TextInput` bindings.
    fn font_family_selector(&self, target: FontTarget, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let selector = self.font_selector(target);
        let search = selector.search.clone();
        let list_state = selector.list.clone();
        let scrollbar_state = selector.scrollbar.clone();
        let highlight = selector.highlight;
        let selected = self.font_family_setting(target);
        let weak = cx.entity().downgrade();

        let search_focus = search.read(cx).focus_handle(cx);
        let handle = self.menu_handle_with(target.menu_id(), cx, {
            let weak = weak.clone();
            move |open, window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if open {
                        this.open_font_selector(target, window, cx);
                    } else {
                        let focus = this.settings_focus.clone();
                        window.focus(&focus, cx);
                    }
                });
                if open {
                    let search_focus = search_focus.clone();
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| window.focus(&search_focus, cx));
                    });
                }
            }
        });

        let families = Rc::new(if handle.is_open() {
            let families = self.visible_font_families(target, cx);
            self.sync_font_selector_rows(target, &families);
            // `open_font_selector` asks the first frame — or the frame the
            // background enumeration lands on — to park on the current face.
            if crate::fonts::installed(cx).is_some()
                && selector.pending_reveal.replace(false)
                && let Some(index) = families.iter().position(|name| *name == selected)
            {
                selector.list.scroll_to_reveal_item(index);
            }
            families
        } else {
            Vec::new()
        });
        let highlight = highlight.filter(|index| *index < families.len());
        let default_family = SharedString::from(target.default_family());

        let trigger = MenuChip::new(target.menu_id())
            .label(font_row_label(&selected))
            .outlined()
            .selected(handle.is_open())
            .w(px(180.0))
            .justify_between();

        popover(
            trigger,
            &handle,
            MenuAlign::BelowRight,
            move |popover, _window, _cx| {
                let popover = popover.clone();
                let next_families = families.clone();
                let previous_families = families.clone();
                let confirm_families = families.clone();
                let next_weak = weak.clone();
                let previous_weak = weak.clone();
                let confirm_weak = weak.clone();
                let confirm_popover = popover.clone();

                let rows = if families.is_empty() {
                    div()
                        .h(px(64.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(sp(12.5))
                        .text_color(theme.text_ghost)
                        .child(tr!("settings.no_fonts"))
                        .into_any_element()
                } else {
                    let row_families = families.clone();
                    let row_default = default_family.clone();
                    let row_selected = selected.clone();
                    let row_weak = weak.clone();
                    let row_popover = popover.clone();
                    let height = (families.len() as f32 * FONT_PICKER_ROW_HEIGHT)
                        .min(FONT_PICKER_LIST_MAX_HEIGHT);
                    div()
                        .w_full()
                        .h(px(height))
                        .flex_none()
                        .relative()
                        .px(px(4.0))
                        .child(
                            list(list_state.clone(), move |index, _window, _cx| {
                                let Some(name) = row_families.get(index).cloned() else {
                                    return div().into_any_element();
                                };
                                let is_selected = name == row_selected;
                                let is_default = name == row_default;
                                let highlighted = highlight == Some(index);
                                div()
                                    .id(SharedString::from(format!("font-row-{name}")))
                                    .w_full()
                                    .h(px(FONT_PICKER_ROW_HEIGHT))
                                    .px(px(8.0))
                                    .rounded(px(8.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .cursor_default()
                                    .when(highlighted, |element| element.bg(theme.overlay_strong))
                                    .hover(|element| element.bg(theme.overlay))
                                    .active(|element| element.opacity(0.85))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .font_family(name.clone())
                                            .text_size(sp(12.5))
                                            .line_height(sp(15.0))
                                            .text_color(if is_selected {
                                                theme.text
                                            } else {
                                                theme.text_secondary
                                            })
                                            .child(font_row_label(&name)),
                                    )
                                    .when(is_default, |element| {
                                        element.child(
                                            div()
                                                .flex_none()
                                                .text_size(sp(11.5))
                                                .text_color(theme.text_ghost)
                                                .child(tr!("settings.default_font")),
                                        )
                                    })
                                    .when(is_selected, |element| {
                                        element.child(icon(
                                            "icons/check.svg",
                                            11.0,
                                            theme.text_secondary,
                                        ))
                                    })
                                    .on_click({
                                        let weak = row_weak.clone();
                                        let popover = row_popover.clone();
                                        move |_, window, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.choose_font_family(
                                                    target,
                                                    name.clone(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                            popover.close(window, cx);
                                            window.refresh();
                                        }
                                    })
                                    .into_any_element()
                            })
                            .size_full(),
                        )
                        .child(scrollbar::vertical(&list_state, &scrollbar_state))
                        .into_any_element()
                };

                div()
                    .w(px(280.0))
                    .rounded(px(16.0))
                    .overflow_hidden()
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.raised)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .on_action(move |_: &SelectNextEntry, _, cx| {
                        let _ = next_weak.update(cx, |this, cx| {
                            this.move_font_selector_highlight(target, "down", &next_families, cx);
                        });
                    })
                    .on_action(move |_: &SelectPreviousEntry, _, cx| {
                        let _ = previous_weak.update(cx, |this, cx| {
                            this.move_font_selector_highlight(target, "up", &previous_families, cx);
                        });
                    })
                    .on_action(move |_: &ConfirmEntry, window, cx| {
                        let should_close = confirm_weak
                            .update(cx, |this, cx| {
                                this.confirm_font_selector(target, &confirm_families, window, cx)
                            })
                            .unwrap_or(false);
                        if should_close {
                            confirm_popover.close(window, cx);
                            window.refresh();
                        }
                    })
                    .child(
                        div().flex_none().p(px(8.0)).child(
                            TextField::new(
                                SharedString::from(format!("{}-filter", target.menu_id())),
                                search.clone(),
                            )
                            .icon("icons/search.svg", 13.0),
                        ),
                    )
                    .child(
                        div()
                            .mx(px(8.0))
                            .h(hairline())
                            .flex_none()
                            .bg(theme.separator),
                    )
                    .child(rows)
                    .child(div().h(px(4.0)))
                    .into_any_element()
            },
        )
    }

    fn font_selector(&self, target: FontTarget) -> &FontSelector {
        match target {
            FontTarget::Ui => &self.ui_font_selector,
            FontTarget::Code => &self.code_font_selector,
        }
    }

    fn font_selector_mut(&mut self, target: FontTarget) -> &mut FontSelector {
        match target {
            FontTarget::Ui => &mut self.ui_font_selector,
            FontTarget::Code => &mut self.code_font_selector,
        }
    }

    /// The name the picker checks: the stored choice, or the built-in default
    /// a `None` resolves to.
    fn font_family_setting(&self, target: FontTarget) -> SharedString {
        let stored = match target {
            FontTarget::Ui => &self.state.ui_font_family,
            FontTarget::Code => &self.state.code_font_family,
        };
        stored
            .as_deref()
            .map(SharedString::from)
            .unwrap_or_else(|| SharedString::from(target.default_family()))
    }

    /// The installed families filtered by the picker's query. Before the
    /// background enumeration lands the list holds only the current face, so
    /// the card still has a row to point at.
    fn visible_font_families(&self, target: FontTarget, cx: &App) -> Vec<SharedString> {
        let query = self
            .font_selector(target)
            .search
            .read(cx)
            .content()
            .trim()
            .to_lowercase();
        crate::fonts::installed(cx)
            .unwrap_or_else(|| vec![self.font_family_setting(target)])
            .into_iter()
            .filter(|name| query.is_empty() || name.to_lowercase().contains(query.as_str()))
            .collect()
    }

    /// Keep the row list in sync with the filtered names — same discipline as
    /// the branch picker so an unchanged frame never resets the scroll.
    fn sync_font_selector_rows(&self, target: FontTarget, rows: &[SharedString]) {
        let selector = self.font_selector(target);
        let mut cached = selector.rows.borrow_mut();
        if cached.as_slice() == rows {
            return;
        }
        *cached = rows.to_vec();
        drop(cached);
        selector
            .list
            .reset_with_uniform_height(rows.len(), px(FONT_PICKER_ROW_HEIGHT));
    }

    /// Reset the picker for a fresh open: clear the filter, drop the cursor,
    /// and flag the reveal so the current family is scrolled into view once
    /// the deferred card has mounted. The family list is warmed at startup,
    /// but a very early open waits on the background enumeration and asks
    /// for a repaint when it lands.
    fn open_font_selector(
        &mut self,
        target: FontTarget,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selector = self.font_selector_mut(target);
        selector.pending_reveal.set(true);
        selector.highlight = None;
        let search = selector.search.clone();
        search.update(cx, |input, cx| input.clear(cx));
        if crate::fonts::installed(cx).is_none() {
            crate::fonts::prefetch(cx);
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(30))
                        .await;
                    let loaded = this
                        .update(cx, |_, cx| crate::fonts::installed(cx).is_some())
                        .unwrap_or(true);
                    if loaded {
                        break;
                    }
                }
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn font_selector_query_edited(
        &mut self,
        target: FontTarget,
        cx: &mut Context<Self>,
    ) {
        if self
            .font_selector(target)
            .search
            .read(cx)
            .content()
            .trim()
            .is_empty()
        {
            self.font_selector_mut(target).highlight = None;
            // Re-center on the applied family, same as the branch picker.
            let families = self.visible_font_families(target, cx);
            let selected = self.font_family_setting(target);
            if let Some(index) = families.iter().position(|name| *name == selected) {
                self.font_selector(target).list.scroll_to_reveal_item(index);
            }
        } else {
            let selector = self.font_selector_mut(target);
            selector.highlight = Some(0);
            selector.list.scroll_to_reveal_item(0);
        }
        cx.notify();
    }

    fn move_font_selector_highlight(
        &mut self,
        target: FontTarget,
        key: &str,
        families: &[SharedString],
        cx: &mut Context<Self>,
    ) {
        let selector = self.font_selector_mut(target);
        let Some(next) = next_picker_highlight(selector.highlight, families.len(), key) else {
            return;
        };
        selector.highlight = Some(next);
        selector.list.scroll_to_reveal_item(next);
        cx.notify();
    }

    /// Apply the keyboard-selected family, returning whether the picker
    /// should dismiss afterward.
    fn confirm_font_selector(
        &mut self,
        target: FontTarget,
        families: &[SharedString],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(name) = families
            .get(self.font_selector(target).highlight.unwrap_or(0))
            .cloned()
        else {
            return false;
        };
        self.choose_font_family(target, name, window, cx);
        true
    }

    /// Store `family` for `target` — `None` when it is the built-in default —
    /// then repaint every surface that shapes with it.
    fn choose_font_family(
        &mut self,
        target: FontTarget,
        family: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = waku_client::persistence::sanitized_font_family(
            (family.as_ref() != target.default_family()).then(|| family.to_string()),
        );
        let stored = match target {
            FontTarget::Ui => &mut self.state.ui_font_family,
            FontTarget::Code => &mut self.state.code_font_family,
        };
        if *stored == value {
            return;
        }
        *stored = value;
        crate::fonts::install(
            self.state.ui_font_family.as_deref(),
            self.state.code_font_family.as_deref(),
            cx,
        );
        // Prose, code, and terminal cells all shape against these faces;
        // markdown flats are dropped by `MarkdownView::sync_style`'s family
        // key and the rest of the measured surfaces by this reset.
        self.remeasure_font_sized_surfaces();
        self.save();
        window.refresh();
        cx.notify();
    }

    fn set_render_math(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.render_math == enabled {
            return;
        }
        self.state.render_math = enabled;
        self.remeasure_font_sized_surfaces();
        self.save();
        cx.notify();
    }

    fn set_show_response_token_speed(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.show_response_token_speed == enabled {
            return;
        }
        self.state.show_response_token_speed = enabled;
        self.save();
        cx.notify();
    }

    pub(super) fn set_markdown_preview(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.markdown_preview == enabled {
            return;
        }
        self.state.markdown_preview = enabled;
        self.save();
        cx.notify();
    }

    fn set_open_at_last_prompt(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.open_at_last_prompt == enabled {
            return;
        }
        self.state.open_at_last_prompt = enabled;
        self.save();
        cx.notify();
    }

    fn set_archive_navigation(&mut self, navigation: ArchiveNavigation, cx: &mut Context<Self>) {
        if self.state.archive_navigation == navigation {
            return;
        }
        self.state.archive_navigation = navigation;
        self.save();
        cx.notify();
    }

    fn set_sync_with_merge(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.sync_with_merge == enabled {
            return;
        }
        self.state.sync_with_merge = enabled;
        self.save();
        cx.notify();
    }

    fn set_sidebar_shortcut_tags(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.sidebar_shortcut_tags == enabled {
            return;
        }
        self.state.sidebar_shortcut_tags = enabled;
        if !enabled {
            // Take down chips already up and cancel a pending reveal.
            self.sidebar_shortcut_hint_generation =
                self.sidebar_shortcut_hint_generation.wrapping_add(1);
            self.sidebar_shortcut_hints = false;
        }
        self.save();
        cx.notify();
    }

    fn set_new_worktree_default_branch(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.new_worktree_default_branch == enabled {
            return;
        }
        self.state.new_worktree_default_branch = enabled;
        self.save();
        cx.notify();
    }

    fn set_new_worktree_sync_default_branch(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.new_worktree_sync_default_branch == enabled {
            return;
        }
        self.state.new_worktree_sync_default_branch = enabled;
        self.save();
        cx.notify();
    }

    /// Persist the whitelist field's current names as the user types; the
    /// list applies to the next worktree created.
    pub(super) fn apply_worktree_sync_branches(&mut self, cx: &mut Context<Self>) {
        let branches = sync_branches_from_text(
            &self.worktree_sync_branches_input.read(cx).content(),
        );
        if branches != self.state.new_worktree_sync_branches {
            self.state.new_worktree_sync_branches = branches;
            self.save();
        }
        cx.notify();
    }

    fn set_ui_font_size(&mut self, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        let size = waku_client::persistence::sanitized_ui_font_size(size);
        if self.state.ui_font_size == size {
            return;
        }
        self.state.ui_font_size = size;
        // Chrome is authored in `sp` rems; the rem size is the setting.
        window.set_rem_size(px(size));
        self.remeasure_font_sized_surfaces();
        self.save();
        window.refresh();
        cx.notify();
    }

    fn set_code_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = waku_client::persistence::sanitized_code_font_size(size);
        if self.state.code_font_size == size {
            return;
        }
        // The terminal follows the code size only while the two match; `None`
        // keeps it resolving to whatever the code size becomes.
        let terminal_follows = self.state.terminal_font_size() == self.state.code_font_size;
        self.state.code_font_size = size;
        if terminal_follows {
            self.state.terminal_font_size = None;
            crate::terminal::install_font_size(size, cx);
        }
        self.remeasure_font_sized_surfaces();
        self.save();
        cx.notify();
    }

    fn set_terminal_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let Some(size) = waku_client::persistence::sanitized_terminal_font_size(Some(size)) else {
            return;
        };
        // Picking the code size re-links the two: `None` resolves to the code
        // size, so the terminal keeps following future code-size changes.
        let stored = (size != self.state.code_font_size).then_some(size);
        if self.state.terminal_font_size == stored {
            return;
        }
        self.state.terminal_font_size = stored;
        crate::terminal::install_font_size(size, cx);
        self.save();
        cx.notify();
    }

    /// `secondary-=` / `secondary--`: step one `FONT_SIZES` preset in the
    /// direction given, against whichever surface the binding's key context
    /// routed the chord to.
    pub(super) fn adjust_font_size_action(
        &mut self,
        action: &crate::AdjustFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = match action.target {
            crate::FontSizeTarget::Ui => self.state.ui_font_size,
            crate::FontSizeTarget::Code => self.state.code_font_size,
            crate::FontSizeTarget::Terminal => self.state.terminal_font_size(),
        };
        let size = stepped_font_size(current, action.direction);
        match action.target {
            crate::FontSizeTarget::Ui => self.set_ui_font_size(size, window, cx),
            crate::FontSizeTarget::Code => self.set_code_font_size(size, cx),
            crate::FontSizeTarget::Terminal => self.set_terminal_font_size(size, cx),
        }
    }

    /// Drop every cached row height that a font size participates in. The
    /// virtualized lists remember measured heights, so a stale entry would
    /// misplace scroll anchors until the row happened to remeasure. The
    /// sidebar list keeps its uniform row height and needs no reset.
    fn remeasure_font_sized_surfaces(&self) {
        self.reset_transcript_rows(self.transcript_row_count());
        let line_count = self
            .right_panel_diff_snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.lines.len());
        self.right_panel_diff_list_state.reset(line_count);
        self.right_panel_diff_tree_list_state
            .reset(self.right_panel_diff_tree_rows.borrow().len());
        self.skills_list_state
            .reset(self.skills_rows.borrow().len());
    }

    fn render_providers_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let checking = self.provider_detection_remaining > 0;
        let checked_label = self
            .provider_detection_checked_at
            .filter(|_| !checking)
            .map(|checked_at| detection_checked_label(checked_at.elapsed()));

        let refresh = div()
            .id("refresh-providers")
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.accent))
            .h(px(28.0))
            .px(px(11.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .opacity(if checking { 0.6 } else { 1.0 })
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/rotate-cw.svg", 11.0, theme.text_tertiary))
            .child(if checking {
                tr!("common.checking")
            } else {
                tr!("common.refresh")
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.refresh_provider_detection(None);
                cx.notify();
            }));

        let mut rows = div().mt(px(4.0)).flex().flex_col();
        let provider_count = ProviderKind::ALL.len();
        for (index, kind) in ProviderKind::ALL.into_iter().enumerate() {
            let probe = self.provider_probe(kind);
            let installed = probe.is_some_and(|probe| probe.installed);
            let binary_path = probe
                .filter(|probe| probe.installed)
                .and_then(|probe| probe.path.as_deref())
                .map(|path| abbreviate_home_path(path, self.home_directory.as_deref()));
            let model_count = probe.map(|probe| probe.models.len()).unwrap_or(0);
            let version = self
                .provider_versions
                .get(&kind)
                .and_then(|version| version.clone());
            let disabled = self.state.disabled_providers.contains(&kind);

            let dot_color = if !installed {
                theme.text_ghost
            } else if disabled {
                theme.warning
            } else {
                theme.success
            };

            let detail: AnyElement = if installed {
                let mut parts = Vec::new();
                if let Some(path) = binary_path {
                    parts.push(path);
                }
                if disabled {
                    parts.push(tr!("providers.disabled_for_new_tasks"));
                } else if model_count > 0 {
                    parts.push(if model_count == 1 {
                        tr!("providers.model_count_one", count = model_count)
                    } else {
                        tr!("providers.model_count_many", count = model_count)
                    });
                }
                div()
                    .truncate()
                    .child(SharedString::from(parts.join("  ·  ")))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .items_baseline()
                    .child(SharedString::from(tr!(
                        "providers.not_detected_as",
                        command = kind.command()
                    )))
                    .into_any_element()
            };

            let toggle_on = !disabled;
            let toggle = toggle_switch(
                SharedString::from(format!("provider-enabled-{}", kind.id())),
                toggle_on,
                false,
                theme,
                cx,
                move |this, _, cx| this.set_provider_enabled(kind, disabled, cx),
            );

            let expanded = self.expanded_provider_settings == Some(kind);
            let expand_button = icon_button(
                SharedString::from(format!("provider-expand-{}", kind.id())),
                if expanded {
                    "icons/chevron-down.svg"
                } else {
                    "icons/chevron-right.svg"
                },
                theme,
            )
            .tab_index(0)
            .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.toggle_provider_expanded(kind, window, cx);
            }));

            let setup_button = (!installed).then(|| {
                div()
                    .id(SharedString::from(format!("provider-setup-{}", kind.id())))
                    .tab_index(0)
                    .focus_visible(|style| style.border_color(theme.accent))
                    .h(px(28.0))
                    .px(px(11.0))
                    .rounded(px(9.0))
                    .border(hairline())
                    .border_color(theme.border_strong)
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_default()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .hover(|element| element.bg(theme.overlay))
                    .child(icon("icons/download.svg", 11.0, theme.text_tertiary))
                    .child(tr!("providers.set_up"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.provider_setup_clicked(kind, window, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.provider_setup_clicked(kind, window, cx);
                            cx.stop_propagation();
                        }
                    }))
            });

            let header = div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .relative()
                        .w(px(30.0))
                        .h(px(30.0))
                        .flex_none()
                        .rounded(px(9.0))
                        .bg(theme.overlay)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(provider_mark(
                            &theme,
                            kind,
                            16.0,
                            provider_color(&theme, kind).opacity(if installed { 1.0 } else { 0.5 }),
                        ))
                        .child(
                            div()
                                .absolute()
                                .bottom(px(-2.0))
                                .right(px(-2.0))
                                .w(px(10.0))
                                .h(px(10.0))
                                .rounded_full()
                                .border_2()
                                .border_color(theme.raised)
                                .bg(dot_color),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(7.0))
                                .child(
                                    div()
                                        .text_size(sp(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if installed {
                                            theme.text
                                        } else {
                                            theme.text_secondary
                                        })
                                        .child(kind.display_name()),
                                )
                                .when_some(version, |element, version| {
                                    element.child(
                                        div()
                                            .font_family(crate::fonts::current(cx).code)
                                            .text_size(sp(12.5))
                                            .text_color(theme.text_tertiary)
                                            .child(SharedString::from(format!("v{version}"))),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .mt(px(3.0))
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(detail),
                        ),
                )
                .when_some(setup_button, |element, button| element.child(button))
                .child(expand_button)
                .when(installed, |element| element.child(toggle));

            rows = rows.child(
                div()
                    .py(px(11.0))
                    .flex()
                    .flex_col()
                    .when(index + 1 != provider_count, |element| {
                        element.border_b(hairline()).border_color(theme.separator)
                    })
                    .child(header)
                    .when(expanded, |element| {
                        element.child(self.render_provider_expanded_settings(kind, theme, cx))
                    }),
            );
        }

        div()
            .mt(px(15.0))
            .w_full()
            .px(px(20.0))
            .py(px(14.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(20.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("providers.coding_agents")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("providers.description")),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap(px(6.0))
                            .child(refresh)
                            .when_some(checked_label, |element, label| {
                                element.child(
                                    div()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_ghost)
                                        .child(SharedString::from(label)),
                                )
                            }),
                    ),
            )
            .child(rows)
            .into_any_element()
    }

    /// The expanded row's settings body: the binary override for this
    /// provider, with the detection result as its caption.
    fn render_provider_expanded_settings(
        &self,
        kind: ProviderKind,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let override_value = self.state.provider_binary_overrides.get(&kind).cloned();
        let full_path = self
            .provider_probe(kind)
            .filter(|probe| probe.installed)
            .and_then(|probe| probe.path.as_ref())
            .map(|path| path.display().to_string());

        let caption = match (&override_value, full_path) {
            (Some(_), Some(path)) => tr!("providers.using_override", path = path),
            (Some(_), None) => tr!("providers.invalid_override"),
            (None, Some(path)) => tr!("providers.detected_at", path = path),
            (None, None) => tr!("providers.searches_path", command = kind.command()),
        };

        let reset = div()
            .id(SharedString::from(format!(
                "provider-path-reset-{}",
                kind.id()
            )))
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.accent))
            .h(px(29.0))
            .px(px(10.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .flex_none()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .child(tr!("common.reset"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.provider_path_input
                    .update(cx, |input, cx| input.clear(cx));
                this.apply_provider_path_override(cx);
            }));

        div()
            .mt(px(10.0))
            .pl(px(42.0))
            .flex()
            .flex_col()
            .gap(px(5.0))
            .child(self.render_provider_setup_section(kind, theme, cx))
            .child(
                div()
                    .mt(px(6.0))
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(tr!("providers.binary_path")),
            )
            .child(
                div()
                    .text_size(sp(12.5))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(tr!(
                        "providers.binary_path_description",
                        provider = kind.short_name()
                    ))),
            )
            .child(
                div()
                    .mt(px(3.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        TextField::new(
                            SharedString::from(format!("provider-path-field-{}", kind.id())),
                            self.provider_path_input.clone(),
                        )
                        .flex_1()
                        .max_w(px(430.0)),
                    )
                    .when(override_value.is_some(), |element| element.child(reset)),
            )
            .child(
                div()
                    .text_size(sp(12.5))
                    .text_color(theme.text_ghost)
                    .child(SharedString::from(caption)),
            )
    }

    /// The expanded row's setup block: the provider's documented install and
    /// sign-in commands verbatim, each with a copy button, plus a Docs link
    /// and a Run button that executes them in a terminal embedded in the row.
    /// A remote daemon can't use a local PTY, so there Run stays hidden.
    fn render_provider_setup_section(
        &self,
        kind: ProviderKind,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let setup = kind.setup();
        let installed = self
            .provider_probe(kind)
            .is_some_and(|probe| probe.installed);
        let terminal = self.provider_setup_terminals.get(&kind).cloned();

        let mut steps = div().mt(px(2.0)).flex().flex_col().gap(px(6.0));
        if !installed {
            steps = steps.child(self.provider_setup_step_row(
                kind,
                "install",
                tr!("providers.install_label"),
                setup.install,
                theme,
                cx,
            ));
        }
        if let Some(sign_in) = setup.sign_in {
            steps = steps.child(self.provider_setup_step_row(
                kind,
                "sign-in",
                tr!("providers.sign_in_label"),
                sign_in,
                theme,
                cx,
            ));
        }

        let mut actions = div().mt(px(4.0)).flex().items_center().gap(px(8.0));
        if !self.daemon.is_remote()
            && let Some(script) = self.provider_setup_script(kind)
        {
            let run = div()
                .id(SharedString::from(format!(
                    "provider-run-setup-{}",
                    kind.id()
                )))
                .tab_index(0)
                .focus_visible(|style| style.border_color(theme.accent))
                .h(px(29.0))
                .px(px(10.0))
                .rounded(px(9.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .child(icon("icons/terminal.svg", 11.0, theme.text_tertiary))
                .child(tr!("providers.run_in_terminal"))
                .tooltip(Tooltip::text(script))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_provider_setup(kind, window, cx);
                }))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.run_provider_setup(kind, window, cx);
                        cx.stop_propagation();
                    }
                }));
            actions = actions.child(run);
        }
        let docs_url = setup.docs_url;
        let docs = div()
            .id(SharedString::from(format!("provider-docs-{}", kind.id())))
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.accent))
            .h(px(29.0))
            .px(px(10.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/external-link.svg", 11.0, theme.text_tertiary))
            .child(tr!("providers.docs"))
            .on_click(move |_, _, cx| cx.open_url(docs_url))
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.open_url(docs_url);
                    cx.stop_propagation();
                }
            });
        actions = actions.child(docs);
        if let Some(env) = setup.api_key_env {
            actions = actions.child(
                div()
                    .text_size(sp(12.5))
                    .text_color(theme.text_ghost)
                    .child(SharedString::from(tr!("providers.api_key_hint", var = env))),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(5.0))
            .child(
                div()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(tr!("providers.setup")),
            )
            .child(steps)
            .child(actions)
            .when_some(terminal, |element, view| {
                element.child(
                    div()
                        .mt(px(4.0))
                        .h(px(280.0))
                        .border(hairline())
                        .border_color(theme.border)
                        .relative()
                        .child(view)
                        .child(
                            div().absolute().top(px(4.0)).right(px(6.0)).child(
                                icon_button(
                                    SharedString::from(format!(
                                        "provider-setup-close-{}",
                                        kind.id()
                                    )),
                                    "icons/x.svg",
                                    theme,
                                )
                                .tab_index(0)
                                .focus_visible(|style| {
                                    style.border(hairline()).border_color(theme.accent)
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.dismiss_provider_setup_terminal(kind, cx);
                                }))
                                .on_key_down(cx.listener(
                                    move |this, event: &KeyDownEvent, _, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            this.dismiss_provider_setup_terminal(kind, cx);
                                            cx.stop_propagation();
                                        }
                                    },
                                )),
                            ),
                        ),
                )
            })
    }

    /// One setup step: its label, the exact command in code, and a copy
    /// button.
    fn provider_setup_step_row(
        &self,
        kind: ProviderKind,
        step: &'static str,
        label: String,
        command: &'static str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let copy_id = format!("provider-setup-copy-{}-{}", kind.id(), step);
        let copied = self.control_was_copied(&copy_id);
        let click_id = copy_id.clone();
        let key_id = copy_id.clone();
        let copy = div()
            .id(SharedString::from(copy_id))
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.accent))
            .size(px(22.0))
            .rounded(px(7.0))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .child(icon(
                if copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                },
                11.0,
                theme.text_tertiary,
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(command.to_owned()));
                this.show_control_copied(click_id.clone(), cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.write_to_clipboard(ClipboardItem::new_string(command.to_owned()));
                    this.show_control_copied(key_id.clone(), cx);
                    cx.stop_propagation();
                }
            }));
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(56.0))
                    .flex_none()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "provider-setup-command-{}-{}",
                        kind.id(),
                        step
                    )))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(crate::fonts::current(cx).code)
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .tooltip(Tooltip::text(command))
                    .child(SharedString::from(command)),
            )
            .child(copy)
    }

    /// The script a provider's Run button executes: install when the CLI is
    /// undetected — chained into sign-in when the provider has one — or
    /// sign-in alone for an installed CLI. `None` when nothing is runnable.
    fn provider_setup_script(&self, provider: ProviderKind) -> Option<String> {
        let setup = provider.setup();
        let installed = self
            .provider_probe(provider)
            .is_some_and(|probe| probe.installed);
        match (installed, setup.sign_in) {
            (false, Some(sign_in)) => Some(format!("{} && {}", setup.install, sign_in)),
            (false, None) => Some(setup.install.to_owned()),
            (true, Some(sign_in)) => Some(sign_in.to_owned()),
            (true, None) => None,
        }
    }

    /// The row's Set up button: expand the provider's settings and start the
    /// setup script. A remote daemon gets the expanded copy/docs row only —
    /// a desktop PTY would install the binary on the wrong host.
    fn provider_setup_clicked(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.expanded_provider_settings != Some(provider) {
            self.toggle_provider_expanded(provider, window, cx);
        }
        if self.daemon.is_remote() || self.provider_setup_terminals.contains_key(&provider) {
            return;
        }
        self.run_provider_setup(provider, window, cx);
    }

    /// Run the setup script in a terminal embedded in the expanded row. The
    /// command closes its own shell on success; the exit event drops the
    /// embed and re-detects the provider.
    fn run_provider_setup(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.daemon.is_remote() {
            return;
        }
        let Some(script) = self.provider_setup_script(provider) else {
            return;
        };
        let mut command = CustomCommand::new(script);
        command.name = Some(tr!(
            "providers.setup_terminal_title",
            provider = provider.display_name()
        ));
        command.icon = CustomCommandIcon::Download;
        command.close_on_success = true;
        let cwd = self
            .home_directory
            .clone()
            .unwrap_or_else(|| PathBuf::from("/"));
        let view =
            cx.new(|cx| TerminalView::embedded(cwd, TerminalLaunch::CustomCommand(command), cx));
        cx.subscribe(&view, move |this, _, _: &TerminalViewEvent, cx| {
            this.provider_setup_terminal_exited(provider, cx);
        })
        .detach();
        self.provider_setup_terminals.insert(provider, view.clone());
        let focus = view.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The setup terminal's shell exited — drop the embed and re-detect so
    /// the row reflects whatever the script changed.
    fn provider_setup_terminal_exited(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        self.provider_setup_terminals.remove(&provider);
        self.refresh_provider_detection(Some(provider));
        cx.notify();
    }

    fn dismiss_provider_setup_terminal(&mut self, provider: ProviderKind, cx: &mut Context<Self>) {
        self.provider_setup_terminals.remove(&provider);
        cx.notify();
    }

    fn toggle_provider_expanded(
        &mut self,
        provider: ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Commit any pending edit for the previously expanded provider before
        // the input is handed to another row.
        self.apply_provider_path_override(cx);
        if self.expanded_provider_settings == Some(provider) {
            self.expanded_provider_settings = None;
        } else {
            self.expanded_provider_settings = Some(provider);
            let override_value = self
                .state
                .provider_binary_overrides
                .get(&provider)
                .cloned()
                .unwrap_or_default();
            self.provider_path_input
                .update(cx, |input, cx| input.set_content(override_value, cx));
            let focus = self.provider_path_input.read(cx).focus();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Commit the binary override edit for the expanded provider: empty means
    /// detect from PATH. Re-detects that provider and refreshes every catalog
    /// keyed by the executable path.
    pub(super) fn apply_provider_path_override(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.expanded_provider_settings else {
            return;
        };
        let text = self
            .provider_path_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        let current = self
            .state
            .provider_binary_overrides
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        if text == current {
            return;
        }
        if text.is_empty() {
            self.state.provider_binary_overrides.remove(&provider);
        } else {
            self.state.provider_binary_overrides.insert(provider, text);
        }
        self.save();
        self.refresh_provider_detection(Some(provider));
        self.refresh_composer_sources(cx);
        cx.notify();
    }

    /// Providers switched off here stop offering models to new sessions;
    /// sessions already locked to them keep working.
    fn set_provider_enabled(
        &mut self,
        provider: ProviderKind,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if enabled {
            self.state
                .disabled_providers
                .retain(|kind| *kind != provider);
        } else if !self.state.disabled_providers.contains(&provider) {
            self.state.disabled_providers.push(provider);
        }
        if !enabled
            && let Some(fallback) = ProviderKind::ALL
                .into_iter()
                .find(|kind| self.provider_enabled(*kind))
        {
            // New work must land somewhere usable: move the new-session
            // default and any unstarted drafts off the switched-off provider.
            // The remembered model belongs to the old provider, so it resets
            // with it.
            if self.state.last_provider == provider {
                self.state.last_provider = fallback;
                self.state.last_model = None;
                self.state.last_reasoning_effort = None;
                self.state.last_service_tier = None;
                self.state.last_context_window = None;
            }
            let draft_ids = self
                .state
                .sessions
                .iter()
                .filter(|session| session.provider == provider && !session.has_started())
                .map(|session| session.id)
                .collect::<Vec<_>>();
            for id in draft_ids {
                if let Some(session) = self.state.session_mut(id) {
                    session.provider = fallback;
                    session.model = None;
                    session.reasoning_effort = None;
                    session.service_tier = None;
                    session.context_window = None;
                }
            }
        }
        self.save();
        cx.notify();
    }

    fn render_computer_use_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let enabled = self.state.computer_use_enabled;
        let permissions = self.computer_permissions.clone();
        let pending = self.computer_permission_request_pending;
        let helper_name = crate::computer_use::helper_display_name();
        let mut allowed_apps = div().flex().flex_col().gap(px(1.0));
        if self.state.computer_use_allowed_apps.is_empty() {
            allowed_apps = allowed_apps.child(
                div()
                    .py(px(12.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("computer_use.no_always_allowed_apps")),
            );
        } else {
            for (index, grant) in self.state.computer_use_allowed_apps.iter().enumerate() {
                let key = grant.key();
                let is_last = index + 1 == self.state.computer_use_allowed_apps.len();
                let app_icon = self.computer_use_app_icon(&grant.bundle_id, cx);
                allowed_apps = allowed_apps.child(
                    div()
                        .py(px(9.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .when(!is_last, |element| {
                            element.border_b(hairline()).border_color(theme.separator)
                        })
                        .child(
                            div()
                                .w(px(32.0))
                                .h(px(32.0))
                                .flex_none()
                                .rounded(px(9.0))
                                .when_some(app_icon, |element, app_icon| {
                                    element.child(img(app_icon).size_full().rounded(px(9.0)))
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(sp(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child(SharedString::from(grant.app_name.clone())),
                                )
                                .child(
                                    div()
                                        .mt(px(2.0))
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_tertiary)
                                        .truncate()
                                        .child(SharedString::from(grant.bundle_id.clone())),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("revoke-computer-app-{key}")))
                                .h(px(25.0))
                                .px(px(9.0))
                                .rounded(px(8.0))
                                .border(hairline())
                                .border_color(theme.border_strong)
                                .flex()
                                .items_center()
                                .cursor_default()
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .hover(|element| element.bg(theme.overlay).text_color(theme.danger))
                                .child(tr!("common.revoke"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.revoke_computer_app(&key, cx);
                                })),
                        ),
                );
            }
        }

        div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(20.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(tr!("computer_use.allow_apps")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(tr!("computer_use.availability")),
                            ),
                    )
                    .child(toggle_switch(
                        "computer-use-enabled",
                        enabled,
                        false,
                        theme,
                        cx,
                        move |this, _, cx| this.set_computer_use_enabled(!enabled, cx),
                    )),
            )
            .when(cfg!(target_os = "macos"), |element| {
                element.child(
                    div()
                        .px(px(20.0))
                        .py(px(14.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .child(
                            div()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(tr!("computer_use.macos_access")),
                        )
                        .child(
                            div()
                                .mt(px(4.0))
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(SharedString::from(tr!(
                                    "computer_use.helper_access",
                                    helper = helper_name
                                ))),
                        )
                        .child(permission_status_row(
                            tr!("computer_use.screen_recording"),
                            tr!("computer_use.screen_recording_description"),
                            permissions.screen_recording,
                            "screen-recording-settings",
                            theme,
                            cx,
                        ))
                        .child(permission_status_row(
                            tr!("computer_use.accessibility"),
                            tr!("computer_use.accessibility_description"),
                            permissions.accessibility,
                            "accessibility-settings",
                            theme,
                            cx,
                        ))
                        .child(
                            div().mt(px(11.0)).flex().items_center().gap(px(8.0)).child(
                                div()
                                    .id("recheck-computer-permissions")
                                    .h(px(28.0))
                                    .px(px(11.0))
                                    .rounded(px(9.0))
                                    .border(hairline())
                                    .border_color(theme.border_strong)
                                    .text_color(theme.text_secondary)
                                    .flex()
                                    .items_center()
                                    .cursor_default()
                                    .text_size(sp(12.5))
                                    .opacity(if pending { 0.6 } else { 1.0 })
                                    .child(if pending {
                                        tr!("common.checking")
                                    } else {
                                        tr!("common.recheck")
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_computer_permissions(false, cx);
                                    })),
                            ),
                        ),
                )
            })
            .when(!cfg!(target_os = "macos"), |element| {
                element.child(
                    div()
                        .px(px(20.0))
                        .py(px(14.0))
                        .rounded(px(16.0))
                        .bg(theme.raised)
                        .child(
                            div()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(tr!("computer_use.desktop_access")),
                        )
                        .child(
                            div()
                                .mt(px(5.0))
                                .text_size(sp(12.5))
                                .line_height(sp(18.0))
                                .text_color(theme.text_secondary)
                                .child(if cfg!(target_os = "windows") {
                                    tr!("computer_use.windows_access_description")
                                } else {
                                    tr!("computer_use.linux_access_description")
                                }),
                        ),
                )
            })
            .child(
                div()
                    .px(px(20.0))
                    .py(px(14.0))
                    .rounded(px(16.0))
                    .bg(theme.raised)
                    .child(
                        div()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(tr!("computer_use.always_allowed_apps")),
                    )
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(tr!("computer_use.always_allowed_apps_description")),
                    )
                    .child(allowed_apps),
            )
            .into_any_element()
    }

    fn set_computer_use_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.computer_use_enabled = enabled;
        self.save();
        if enabled {
            self.request_computer_permissions(true, cx);
        }
        cx.notify();
    }

    pub(super) fn request_computer_permissions(&mut self, prompt: bool, cx: &mut Context<Self>) {
        if !cfg!(target_os = "macos")
            || !crate::computer_use::is_available()
            || self.computer_permission_request_pending
        {
            return;
        }
        self.computer_permission_request_pending = true;
        let tx = self.computer_permission_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let daemon = self.daemon.client();
        std::thread::Builder::new()
            .name("waku-computer-permission-request".into())
            .spawn(move || {
                let result = match daemon.request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::ProbeComputerPermissions { prompt },
                ) {
                    Ok(waku_client::ResponsePayload::ComputerPermissions { permissions }) => {
                        Ok(permissions)
                    }
                    Ok(_) => Err("the daemon returned an invalid permission response".into()),
                    Err(error) => Err(error.to_string()),
                };
                if tx.send(result).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .ok();
        cx.notify();
    }

    fn revoke_computer_app(&mut self, key: &str, cx: &mut Context<Self>) {
        self.state
            .computer_use_allowed_apps
            .retain(|grant| grant.key() != key);
        self.save();
        cx.notify();
    }

    fn computer_use_app_icon(
        &self,
        bundle_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<std::sync::Arc<gpui::Image>> {
        if let Some(icon) = self.computer_use_app_icons.borrow().get(bundle_id) {
            return icon.clone();
        }

        let bundle_id = bundle_id.to_owned();
        if self
            .computer_use_app_icon_loads
            .borrow_mut()
            .insert(bundle_id.clone())
        {
            cx.spawn(async move |this, cx| {
                let load_bundle_id = bundle_id.clone();
                let icon =
                    cx.background_executor()
                        .spawn(async move {
                            crate::platform::load_app_icon_for_bundle_id(&load_bundle_id)
                        })
                        .await;
                let _ = this.update(cx, |this, cx| {
                    this.computer_use_app_icon_loads
                        .borrow_mut()
                        .remove(&bundle_id);
                    this.computer_use_app_icons
                        .borrow_mut()
                        .insert(bundle_id, icon);
                    cx.notify();
                });
            })
            .detach();
        }
        None
    }

    fn render_settings_drag_region(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let region = div().id(id);
        // Windows drags from the hit test rather than a mouse-move handler.
        #[cfg(target_os = "windows")]
        let region = region.window_control_area(gpui::WindowControlArea::Drag);

        region
            .h(px(48.0))
            .flex_none()
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    crate::platform::titlebar_double_click(window);
                }
            })
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                this.header_drag_armed = false;
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = false;
                }),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.header_drag_armed {
                    this.header_drag_armed = false;
                    crate::platform::start_window_move(window);
                }
            }))
    }

    fn update_theme_settings(
        &mut self,
        update: impl FnOnce(&mut ThemeSettings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.state.theme;
        update(&mut settings);
        if self.state.theme == settings {
            return;
        }
        self.state.theme = settings;
        crate::theme::apply_theme_preference(settings, self.state.sidebar_transparency, window, cx);
        self.save();
        cx.notify();
    }

    /// Apply a would-be theme choice on screen without persisting it, so a
    /// hovered menu option shows itself. [`Self::restore_theme_preview`] puts
    /// the persisted theme back when the menu dismisses.
    fn preview_theme_settings(
        &mut self,
        update: impl FnOnce(&mut ThemeSettings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.state.theme;
        update(&mut settings);
        if self.state.theme == settings {
            return;
        }
        self.theme_preview_active = true;
        crate::theme::apply_theme_preference(settings, self.state.sidebar_transparency, window, cx);
    }

    /// Re-apply the persisted theme after a previewed choice is dismissed
    /// without being picked. A pick commits through
    /// [`Self::update_theme_settings`] right after close, so this stays a
    /// no-op then — the persisted theme is the picked one.
    fn restore_theme_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.theme_preview_active {
            return;
        }
        self.theme_preview_active = false;
        crate::theme::apply_theme_preference(
            self.state.theme,
            self.state.sidebar_transparency,
            window,
            cx,
        );
    }

    /// The menu toggle observer that calls [`Self::restore_theme_preview`] on
    /// close. Pass to [`Self::menu_handle_with`] for selectors whose options
    /// preview through [`Self::preview_theme_settings`].
    fn theme_preview_restore(
        cx: &mut Context<Self>,
    ) -> impl Fn(bool, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |open, window, cx| {
            if !open {
                let _ = weak.update(cx, |this, cx| {
                    this.restore_theme_preview(window, cx);
                });
            }
        }
    }

    /// [`Self::theme_preview_restore`] plus an open-time claim on the selector's
    /// mode slot, so its menu and options are browsable while the other slot
    /// owns the window — light themes under a dark appearance and vice versa —
    /// even when "Match system appearance" is on. The claim is a preview: it
    /// never persists, and a pick only commits the palette, not the slot.
    fn theme_slot_preview(
        mode: ThemeMode,
        cx: &mut Context<Self>,
    ) -> impl Fn(bool, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |open, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                if open {
                    this.preview_theme_settings(|settings| settings.mode = mode, window, cx);
                } else {
                    this.restore_theme_preview(window, cx);
                }
            });
        }
    }

    pub(crate) fn sidebar_transparency(&self) -> bool {
        self.state.sidebar_transparency
    }

    fn set_thick_borders(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.thick_borders == enabled {
            return;
        }
        self.state.thick_borders = enabled;
        crate::theme::set_thick_borders(enabled);
        self.save();
        cx.notify();
    }

    /// The "High contrast" preference widens the border-tier floors, so it
    /// rebuilds the theme rather than only flagging a static. The OS's own
    /// Increase Contrast setting forces it on — the toggle renders disabled
    /// in that case.
    fn set_high_contrast(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.high_contrast == enabled {
            return;
        }
        self.state.high_contrast = enabled;
        crate::theme::set_high_contrast(enabled);
        crate::theme::apply_theme_preference(
            self.state.theme,
            self.state.sidebar_transparency,
            window,
            cx,
        );
        self.save();
        cx.notify();
    }

    fn set_sidebar_transparency(
        &mut self,
        transparent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.sidebar_transparency == transparent {
            return;
        }
        self.state.sidebar_transparency = transparent;
        crate::theme::apply_theme_preference(self.state.theme, transparent, window, cx);
        self.save();
        cx.notify();
    }

    fn set_three_finger_swipe_navigation(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.three_finger_swipe_navigation == enabled {
            return;
        }
        self.state.three_finger_swipe_navigation = enabled;
        crate::platform::set_trackpad_navigation_swipe_enabled(window, enabled);
        self.save();
        cx.notify();
    }

    fn set_language(
        &mut self,
        language: crate::i18n::AppLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.language == language {
            return;
        }

        self.state.language = language;
        crate::i18n::set_language(language);

        self.composer.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.composer"), cx);
            input.set_placeholder(tr!("input.do_anything"), cx)
        });
        self.model_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.search_models"), cx);
            input.set_placeholder(tr!("input.search_models"), cx)
        });
        self.branch_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.search_branches"), cx);
            input.set_placeholder(tr!("input.search_branches"), cx)
        });
        self.branch_create_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.new_branch_name"), cx);
            input.set_placeholder(tr!("input.new_branch_name"), cx)
        });
        self.worktree_name_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.worktree_name"), cx);
            input.set_placeholder(tr!("input.worktree_name"), cx)
        });
        self.settings_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("settings.search"), cx);
            input.set_placeholder(tr!("settings.search"), cx)
        });
        self.archived_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("settings.archived_search"), cx);
            input.set_placeholder(tr!("settings.archived_search"), cx)
        });
        for selector in [&self.ui_font_selector, &self.code_font_selector] {
            selector.search.update(cx, |input, cx| {
                input.set_accessibility_label(tr!("input.search_fonts"), cx);
                input.set_placeholder(tr!("input.search_fonts"), cx)
            });
        }
        self.skills_search.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("skills.search"), cx);
            input.set_placeholder(tr!("skills.search"), cx)
        });
        self.provider_path_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("providers.binary_path"), cx);
            input.set_placeholder(tr!("input.detected_automatically"), cx)
        });
        self.usage_project_filter.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.filter_projects"), cx);
            input.set_placeholder(tr!("input.filter_projects"), cx)
        });
        self.user_input_answer.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.answer"), cx);
            input.set_placeholder(tr!("user_input.other_placeholder"), cx)
        });
        self.annotation_comment_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.comment"), cx);
            input.set_placeholder(tr!("annotations.comment_placeholder"), cx)
        });
        self.session_rename_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("a11y.task_name"), cx)
        });
        self.daemon_port_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("daemon.port"), cx)
        });
        self.daemon_origins_input.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("daemon.allowed_origins"), cx)
        });
        self.right_panel_diff_filter.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("diff.filter_files"), cx);
            input.set_placeholder(tr!("diff.filter_files"), cx)
        });
        self.refresh_command_palette_localized_text(cx);
        self.refresh_file_finder_localized_text(cx);
        self.refresh_file_search_localized_text(cx);
        self.refresh_transcript_search_localized_text(cx);
        for browser in self.right_panel_browsers.values() {
            browser.update(cx, |browser, cx| browser.refresh_localized_text(cx));
        }
        for terminal in self.right_panel_terminals.values() {
            terminal.update(cx, |terminal, cx| terminal.refresh_localized_text(cx));
        }
        for probe in &mut self.probes {
            probe.models = crate::model_catalog::fallback_models(probe.provider);
        }
        self.refresh_provider_detection(None);
        self.invalidate_composer_sources(cx);

        let updater_available = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|updater| updater.0.as_ref())
            .is_some();
        crate::set_app_menus(cx, updater_available);
        self.save();
        window.refresh();
        cx.notify();
    }
}

/// Sizes offered by the font-size dropdowns. A hand-edited `app.json` may
/// hold values outside this list; they render as-is and simply select
/// nothing here.
const FONT_SIZES: [f32; 8] = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0];

/// Next `FONT_SIZES` preset in the direction given — a between-presets value
/// lands on the first step past it, and the list ends hold their ground.
fn stepped_font_size(current: f32, direction: crate::FontSizeDirection) -> f32 {
    match direction {
        crate::FontSizeDirection::Increase => FONT_SIZES
            .into_iter()
            .find(|size| *size > current)
            .unwrap_or(current),
        crate::FontSizeDirection::Decrease => FONT_SIZES
            .into_iter()
            .rev()
            .find(|size| *size < current)
            .unwrap_or(current),
    }
}

fn font_size_label(size: f32) -> String {
    if size.fract() == 0.0 {
        format!("{size:.0} px")
    } else {
        format!("{size} px")
    }
}

const FONT_PICKER_ROW_HEIGHT: f32 = 30.0;
const FONT_PICKER_LIST_MAX_HEIGHT: f32 = 300.0;

/// Which configurable face a font picker edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FontTarget {
    Ui,
    Code,
}

impl FontTarget {
    fn menu_id(self) -> &'static str {
        match self {
            Self::Ui => "ui-font-selector",
            Self::Code => "code-font-selector",
        }
    }

    /// The family a `None` setting resolves to — also the row that resets
    /// the field to default when chosen.
    fn default_family(self) -> &'static str {
        match self {
            Self::Ui => crate::fonts::DEFAULT_UI_FAMILY,
            Self::Code => crate::fonts::DEFAULT_CODE_FAMILY,
        }
    }
}

/// One Appearance font picker's state: its filter field, the virtualized row
/// list, and the keyboard cursor. `rows` mirrors the names the list was last
/// built for so an unchanged frame never resets the scroll position;
/// `pending_reveal` asks the next rendered frame to park on the selected row.
pub(super) struct FontSelector {
    pub search: Entity<TextInput>,
    pub list: ListState,
    pub scrollbar: Rc<ScrollbarState>,
    pub highlight: Option<usize>,
    pub pending_reveal: Cell<bool>,
    pub rows: RefCell<Vec<SharedString>>,
}

impl FontSelector {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            search: cx.new(|cx| {
                TextInput::new(window, cx)
                    .clear_on_escape()
                    .accessibility_label(tr!("input.search_fonts"))
                    .placeholder(tr!("input.search_fonts"))
            }),
            list: ListState::new(0, ListAlignment::Top, px(64.0)),
            scrollbar: ScrollbarState::new(),
            highlight: None,
            pending_reveal: Cell::new(false),
            rows: RefCell::new(Vec::new()),
        }
    }
}

/// What a family row shows: the platform alias reads better as "System
/// font" than ".SystemUIFont".
fn font_row_label(family: &SharedString) -> SharedString {
    if family.as_ref() == crate::fonts::DEFAULT_UI_FAMILY {
        SharedString::from(tr!("settings.system_font"))
    } else {
        family.clone()
    }
}

/// A settings card row: title and description on the left, control on the
/// right — the card's divider lines are drawn by the caller.
fn settings_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    theme: Theme,
) -> Div {
    let title = title.into();
    let description = description.into();
    div()
        .w_full()
        .min_h(px(60.0))
        .px(px(20.0))
        .py(px(12.0))
        .flex()
        .items_center()
        .gap(px(24.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(sp(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(title),
                )
                .child(
                    div()
                        .mt(px(5.0))
                        .text_size(sp(12.5))
                        .line_height(sp(18.0))
                        .text_color(theme.text_secondary)
                        .child(description),
                ),
        )
        .child(control)
}

/// "Checked …" caption for the Providers page. Recomputed whenever the page
/// redraws; precision beyond the minute is noise here.
fn detection_checked_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 90 {
        tr!("providers.checked_just_now")
    } else if seconds < 3600 {
        tr!("providers.checked_minutes_ago", count = seconds / 60)
    } else {
        tr!("providers.checked_hours_ago", count = seconds / 3600)
    }
}

/// Split the whitelist field's comma- or whitespace-separated text into
/// branch names — Git names admit neither separator — deduplicated in
/// first-seen order.
fn sync_branches_from_text(text: &str) -> Vec<String> {
    let mut branches = Vec::new();
    for name in text.split([',', ' ', '\t', '\n']) {
        let name = name.trim();
        if !name.is_empty() && !branches.iter().any(|seen| seen == name) {
            branches.push(name.to_owned());
        }
    }
    branches
}

/// Keep the full binary path, abbreviating only the user's home directory.
pub(super) fn abbreviate_home_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

fn permission_status_row(
    name: String,
    description: String,
    granted: bool,
    id: &'static str,
    theme: Theme,
    cx: &mut Context<Waku>,
) -> Div {
    let status = if granted {
        div()
            .id(id)
            .h(px(25.0))
            .px(px(4.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.success)
            .child(icon("icons/check.svg", 12.0, theme.success))
            .child(tr!("computer_use.access_granted"))
    } else {
        div()
            .id(id)
            .h(px(25.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay).text_color(theme.text))
            .child(tr!("computer_use.grant_access"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.request_computer_permissions(true, cx);
            }))
    };

    div()
        .mt(px(10.0))
        .pt(px(10.0))
        .border_t(hairline())
        .border_color(theme.separator)
        .flex()
        .items_center()
        .gap(px(10.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(sp(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(name),
                )
                .child(
                    div()
                        .mt(px(2.0))
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(description),
                ),
        )
        .child(status)
}

#[cfg(test)]
mod tests {
    use super::{
        CUSTOM_COMMAND_EDITOR_CONTEXT, FocusNext, FocusPrevious, SETTINGS_PAGES,
        abbreviate_home_path, sync_branches_from_text,
    };
    use crate::input::TextInput;
    use gpui::{Context, Entity, Render, TestAppContext, Window, div, prelude::*};
    use std::path::Path;

    struct TabHarness {
        name: Entity<TextInput>,
        shell: Entity<TextInput>,
        script: Entity<TextInput>,
    }

    impl Render for TabHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context(CUSTOM_COMMAND_EDITOR_CONTEXT)
                .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
                .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
                .child(self.name.clone())
                .child(div().id("icon-selector").tab_index(0))
                .child(self.shell.clone())
                .child(self.script.clone())
                .child(div().id("toggle").tab_index(0))
                .child(div().id("cancel").tab_index(0))
                .child(div().id("save").tab_index(0))
        }
    }

    #[gpui::test]
    fn tab_moves_through_custom_command_editor_controls(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::input::init(cx);
            super::init(cx);
        });
        let (harness, cx) = cx.add_window_view(|window, cx| {
            let name = cx.new(|cx| TextInput::new(window, cx).tab_index(0));
            let shell = cx.new(|cx| TextInput::new(window, cx).tab_index(0));
            let script = cx.new(|cx| TextInput::new(window, cx).tab_index(0));
            TabHarness {
                name,
                shell,
                script,
            }
        });
        let name = cx.read_entity(&harness, |harness, _| harness.name.clone());
        let shell = cx.read_entity(&harness, |harness, _| harness.shell.clone());
        let script = cx.read_entity(&harness, |harness, _| harness.script.clone());
        let name_focus = cx.read_entity(&name, |input, _| input.focus());
        let shell_focus = cx.read_entity(&shell, |input, _| input.focus());
        let script_focus = cx.read_entity(&script, |input, _| input.focus());
        cx.update(|window, cx| window.focus(&name_focus, cx));

        let mut forward = Vec::new();
        for _ in 0..7 {
            cx.simulate_keystrokes("tab");
            cx.update(|window, cx| {
                forward.push(window.focused(cx).expect("a tab stop should be focused"));
            });
        }

        assert_eq!(forward[1], shell_focus);
        assert_eq!(forward[2], script_focus);
        assert_eq!(forward[6], name_focus);
        for (index, focus) in forward[..6].iter().enumerate() {
            assert!(!forward[..index].contains(focus));
        }

        for expected in forward[..6].iter().rev() {
            cx.simulate_keystrokes("shift-tab");
            cx.update(|window, cx| {
                assert_eq!(window.focused(cx).as_ref(), Some(expected));
            });
        }
    }

    #[test]
    fn every_settings_page_icon_is_embedded() {
        use crate::assets::Assets;
        use gpui::AssetSource;

        for (.., icon, _) in SETTINGS_PAGES {
            assert!(
                Assets.load(icon).unwrap().is_some(),
                "missing embedded icon: {icon}"
            );
        }
    }

    #[test]
    fn sync_branches_parse_separators_and_deduplicate() {
        assert_eq!(
            sync_branches_from_text("develop, release/1.2\nnext develop"),
            ["develop", "release/1.2", "next"]
        );
        assert!(sync_branches_from_text(" , ").is_empty());
    }

    #[test]
    fn provider_paths_abbreviate_only_the_home_prefix() {
        let home = Path::new("/Users/example");

        assert_eq!(
            abbreviate_home_path(Path::new("/Users/example/.local/bin/amp"), Some(home)),
            "~/.local/bin/amp"
        );
        assert_eq!(
            abbreviate_home_path(Path::new("/opt/homebrew/bin/codex"), Some(home)),
            "/opt/homebrew/bin/codex"
        );
    }
}
