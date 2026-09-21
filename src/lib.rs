#![recursion_limit = "256"]

rust_i18n::i18n!("locales", fallback = "en");

// rust-i18n expands locale data in a proc macro, which Cargo does not always
// discover as an input when only a YAML file changes. Keep explicit source
// dependencies so the watcher rebuilds the translation registry itself.
const _LOCALE_SOURCES: [&str; 3] = [
    include_str!("../locales/app.yml"),
    include_str!("../locales/zh-CN.yml"),
    include_str!("../locales/ja.yml"),
];

macro_rules! tr {
    ($key:expr) => {
        crate::i18n::translate($key)
    };
    ($key:expr, $($args:tt)*) => {
        rust_i18n::t!($key, $($args)*).into_owned()
    };
}

/// Borrow static translations on hot render paths; interpolation uses `tr!`
/// because formatted messages necessarily allocate.
macro_rules! tr_cow {
    ($key:literal) => {
        rust_i18n::t!($key)
    };
}

mod agy;
mod analytics;
mod app;
mod assets;
mod bookmarks;
mod browser;
mod computer_use;
mod custom_commands;
pub mod daemon;
mod driver;
mod fonts;
mod input;
mod keybindings;
mod md;
mod platform;
mod query;
mod review_diff;
mod shell_integration;
#[cfg(unix)]
mod ssh;
mod terminal;
mod theme;
mod ui;
mod updater;

pub use waku_client::{
    checkpoint, command_env, composer_complete, git_branch, git_commit, i18n, identity, model,
    model_catalog, persistence, projectless, skills, usage, usage_history, worktree,
};

use gpui::{
    App, Application, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, actions, point, px, size,
};

use crate::app::Waku;
use crate::identity::{APP_ID, APP_NAME};
actions!(
    waku,
    [
        Quit,
        About,
        Hide,
        HideOthers,
        ShowAll,
        CloseWindow,
        NewSession,
        NewTaskIn,
        NewProject,
        OpenSettings,
        CheckForUpdates,
        ToggleSidebar,
        ToggleRightPanel,
        ToggleGitPanel,
        ToggleCommandPalette,
        ToggleFileFinder,
        ToggleProjectsPage,
        SelectAllProjectsRows,
        FocusProjectsFilter,
        DismissProjectsLayer,
        ToggleInboxPage,
        DismissInbox,
        DismissDraftsLayer,
        ToggleAutomationsPage,
        ToggleBigPicture,
        OpenResumePicker,
        ToggleFpsCounter,
        NavigateBack,
        NavigateForward,
        GoToNextUnreadCompletion,
        MarkSessionUnread,
        MarkUnreadAndGoToNextIdle,
        GoToPreviousTurn,
        GoToNextTurn,
        SwitchTaskForward,
        SwitchTaskBackward,
        SelectFirstTask,
        SelectLastTask,
        ConfirmTaskSwitch,
        CancelTaskSwitch,
        SwitchProjectForward,
        SwitchProjectBackward,
        SelectFirstProject,
        SelectLastProject,
        ConfirmProjectSwitch,
        CancelProjectSwitch,
        FocusComposer,
        FocusTerminal,
        ToggleModelPicker,
        ToggleBranchPicker,
        ToggleRuntimeModePicker,
        ToggleEnvironment,
        ToggleUsagePanel,
        ToggleWorkspace,
        SaveFile,
        SyncBranch,
        ArchiveSession,
        PushBaseBranch,
        ToggleSessionPin,
        ToggleTerminals,
        NewTerminal,
        RunProjectScript,
        CopySelection,
        CopyWorkingDirectory,
        AddToChat,
        OpenFind,
        OpenFindReplace,
        CloseFind,
        FindNext,
        FindPrevious,
        ToggleFindCaseSensitive,
        ToggleFindWholeWord,
        ToggleFindRegex,
        ReplaceAllMatches,
        OpenGoToLine,
        ExitPanelFullscreen,
        BrowserBack,
        BrowserForward,
        BrowserReload,
        BrowserHardReload,
        BrowserStop,
        BrowserDevtools,
        FocusBrowserAddress,
        BrowserAddressCancel,
        WebviewCopy,
        WebviewCut,
        WebviewPaste,
        WebviewSelectAll,
        OpenLocalhostUrl,
        OpenLocalhostUrlInTab,
        OpenCreatedIssueInGitHub,
        OpenToastSession
    ]
);

/// Stop the selected session's running turn. Bare Escape binds it with the
/// second-press confirmation, and never inside a focused terminal where
/// Escape is real pty input; there ⌥Escape carries `immediate` so a single
/// press still stops the turn.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct CancelTurn {
    pub immediate: bool,
}

/// Jump to the nth task currently listed in the sidebar (⌘1–⌘9). Carries the
/// target index so nine bindings share one action.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct SelectSidebarSession {
    pub index: usize,
}

/// Switch the Projects page to its nth tab (⌘⌥1–⌘⌥3), or deep-link to that
/// tab from anywhere when the page is closed.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct SelectProjectsTab {
    pub index: usize,
}

/// Switch the Automations page to its nth tab (⌘⌥1–⌘⌥2). Only bound inside
/// the page's key context.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct SelectAutomationsTab {
    pub index: usize,
}

/// Apply the nth starred model selection to the composer session (⌘⌥1–⌘⌥9),
/// ordered as in the model picker's favorites section. Carries the target
/// index so nine bindings share one action.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct SelectFavoriteModel {
    pub index: usize,
}

/// Step the composer session through its starred model+effort combos plus
/// the most recently used selection, wrapping at the end (⌥Tab).
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct CycleFavoriteModel;

/// Step the composer session's reasoning effort through the current model's
/// ladder, wrapping at the ends. ⌘E moves `Forward`, ⌘⇧E `Backward`.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct CycleReasoningEffort {
    pub direction: EffortCycleDirection,
}

#[derive(Clone, Copy, PartialEq)]
pub enum EffortCycleDirection {
    Forward,
    Backward,
}

/// Step a surface's font size one preset in `direction`. The same ⌘= / ⌘-
/// chords bind it once per key context — Terminal, the code surfaces, or
/// everywhere else for the interface — so the chord resizes whatever the
/// user is looking at.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku, no_json)]
pub struct AdjustFontSize {
    pub target: FontSizeTarget,
    pub direction: FontSizeDirection,
}

#[derive(Clone, Copy, PartialEq)]
pub enum FontSizeTarget {
    Ui,
    Code,
    Terminal,
}

#[derive(Clone, Copy, PartialEq)]
pub enum FontSizeDirection {
    Increase,
    Decrease,
}

const DEFAULT_WINDOW_WIDTH: f32 = 1380.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 880.0;
const MIN_WINDOW_WIDTH: f32 = 980.0;
const MIN_WINDOW_HEIGHT: f32 = 680.0;
/// How much titlebar must stay on the display for the window to be dragged
/// back by hand.
const TITLEBAR_GRAB_WIDTH: f32 = 160.0;
const TITLEBAR_GRAB_HEIGHT: f32 = 22.0;

/// Reopen the main window where the user last left it, on the display it was
/// left on. GPUI window bounds are display-relative, so the persisted frame is
/// anchored by resolving the saved display UUID against the connected
/// displays — Zed's scheme — and `display_id` rides along in `WindowOptions`.
/// When that display is gone the same offsets re-anchor on the primary
/// display, and the origin is clamped so the titlebar stays grabbable after
/// any display change.
fn restored_window_placement(cx: &App) -> (WindowBounds, Option<gpui::DisplayId>) {
    let centered = |cx: &App| {
        (
            WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)),
                cx,
            )),
            None,
        )
    };
    let Some(saved) = crate::persistence::load_window_state().filter(|saved| {
        [saved.x, saved.y, saved.width, saved.height]
            .iter()
            .all(|value| value.is_finite())
    }) else {
        return centered(cx);
    };
    let display = saved.display.and_then(|uuid| {
        cx.displays()
            .into_iter()
            .find(|display| display.uuid().ok() == Some(uuid))
    });
    let display_id = display.as_ref().map(|display| display.id());
    let Some(anchor) = display.or_else(|| cx.primary_display()) else {
        return centered(cx);
    };
    let anchor_size = anchor.bounds().size;
    let width = saved.width.max(MIN_WINDOW_WIDTH);
    let height = saved.height.max(MIN_WINDOW_HEIGHT);
    let x = saved.x.clamp(
        TITLEBAR_GRAB_WIDTH - width,
        (f32::from(anchor_size.width) - TITLEBAR_GRAB_WIDTH).max(0.0),
    );
    let y = saved.y.clamp(
        0.0,
        (f32::from(anchor_size.height) - TITLEBAR_GRAB_HEIGHT).max(0.0),
    );
    let bounds = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
    let window_bounds = if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    (window_bounds, display_id)
}

trait WakuApplicationExt {
    fn with_main_window_reopen(self) -> Self;
}

impl WakuApplicationExt for Application {
    fn with_main_window_reopen(self) -> Self {
        self.on_reopen(|cx| {
            if let Some(window) = cx.windows().into_iter().next() {
                window
                    .update(cx, |_, window, _| window.activate_window())
                    .ok();
            }
            cx.activate(true);
        });
        self
    }
}

pub fn run() {
    // Adopt pre-Goddard state under ~/.goddard before anything reads the new
    // location — small files copy, workspaces and worktrees link over. The
    // daemon migrates the data directory it owns. Failures are non-fatal: the
    // app runs on whatever did migrate and retries the rest next launch.
    let migration = waku_protocol::migration::migrate_home_directory();
    let daemon = crate::daemon::start_process()
        .unwrap_or_else(|error| panic!("failed to start Goddard daemon: {error:#}"));
    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        // Remote `img()` sources (commit-author avatars) go through this
        // client; without one GPUI's null client fails every load silently.
        .with_http_client(std::sync::Arc::new(
            reqwest_client::ReqwestClient::user_agent(&format!(
                "{}/{}",
                APP_NAME,
                env!("CARGO_PKG_VERSION")
            ))
            .expect("failed to build the app's HTTP user agent"),
        ))
        .with_main_window_reopen()
        .run(move |cx: &mut App| {
            // Linux uses this for Wayland app_id/X11 WM_CLASS and notification
            // attribution. Other platforms also benefit from one stable
            // process identity.
            cx.set_app_identity(APP_ID, APP_NAME);
            crate::assets::register_fonts(cx).expect("failed to register bundled fonts");
            // Enumerating the platform's families is too slow for startup;
            // warm the settings pickers' list in the background.
            crate::fonts::prefetch(cx);
            crate::input::init(cx);
            crate::ui::menu::init(cx);
            crate::app::init_composer_autocomplete(cx);
            crate::app::init_settings_keys(cx);
            crate::app::init_command_palette(cx);
            crate::app::init_element_inspector(cx);
            crate::app::init_file_finder(cx);
            crate::app::init_sync_branch(cx);
            crate::app::init_commit_dialog_keys(cx);
            crate::app::init_issue_dialog_keys(cx);
            crate::app::init_git_panel_keys(cx);
            crate::app::init_archive_dialog_keys(cx);
            crate::app::init_full_access_dialog_keys(cx);
            crate::app::init_terminal_close_dialog_keys(cx);
            crate::app::init_provider_switch_dialog_keys(cx);
            crate::app::init_push_base_dialog_keys(cx);
            crate::app::init_big_picture_keys(cx);
            crate::app::init_goal_dialog_keys(cx);
            crate::app::init_send_file_dialog_keys(cx);
            crate::app::init_annotation_keys(cx);
            crate::app::init_automations_keys(cx);
            crate::app::init_image_preview_keys(cx);
            crate::app::init_sidebar_keys(cx);
            crate::app::init_skills_keys(cx);
            crate::app::init_drafts_keys(cx);
            crate::app::init_shortcuts_dialog_keys(cx);
            crate::terminal::init_command_bar_keys(cx);
            crate::theme::init(cx);
            crate::platform::init_reduce_motion(cx);

            // Platform updaters only run from a supported release layout (or
            // when explicitly forced for development); everywhere else the
            // menu item is omitted along with the updater itself.
            let updater = crate::updater::Updater::init();
            let updater_available = updater.is_some();
            cx.set_global(crate::updater::UpdaterState(updater));
            cx.on_action(|_: &CheckForUpdates, cx| {
                if let Some(updater) = &cx.global::<crate::updater::UpdaterState>().0 {
                    updater.check_for_updates();
                }
            });
            cx.on_action(|_: &About, _| crate::platform::show_about_panel());
            // AppKit's default Hide items live on the application menu. Replacing
            // that menu without these actions leaves ⌘H / ⌥⌘H unbound.
            cx.on_action(|_: &Hide, cx| cx.hide());
            cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
            cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

            bind_keys(cx);
            // Saved remaps layer over the catalog map before the first
            // window opens, so customized chords work from launch.
            keybindings::apply_saved_overrides(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());

            // Unlike AppKit, Linux has no Dock activation path that can
            // restore a hidden last window. Follow Zed's GPUI precedent and
            // terminate when the final window closes.
            #[cfg(not(target_os = "macos"))]
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let (window_bounds, display_id) = restored_window_placement(cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        titlebar: Some(TitlebarOptions {
                            title: Some(APP_NAME.into()),
                            // Windows creates the window without `WS_CAPTION`
                            // either way; asking for the transparent titlebar
                            // is what extends the client area over the frame
                            // so Goddard's own header can host the caption
                            // buttons and drag region.
                            appears_transparent: cfg!(any(
                                target_os = "macos",
                                target_os = "windows"
                            )),
                            traffic_light_position: cfg!(target_os = "macos")
                                .then(|| point(px(16.0), px(17.0))),
                        }),
                        // Goddard moves its custom macOS titlebar explicitly. Keep
                        // the NSWindow movable so native controls and Window-menu
                        // tiling remain enabled.
                        is_movable: true,
                        app_owns_titlebar_drag: cfg!(target_os = "macos"),
                        window_background: if cfg!(target_os = "macos") {
                            WindowBackgroundAppearance::Blurred
                        } else {
                            WindowBackgroundAppearance::Opaque
                        },
                        app_id: Some(APP_ID.to_owned()),
                        // GPUI defaults to compositor/server decorations. If a
                        // Wayland compositor declines them, it reports the
                        // client fallback and Goddard renders that frame itself.
                        #[cfg(target_os = "linux")]
                        icon: crate::platform::linux_app_icon(),
                        window_bounds: Some(window_bounds),
                        display_id,
                        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
                        ..Default::default()
                    },
                    move |window, cx| {
                        crate::platform::configure_main_window_close_behavior(window, cx);
                        let waku = Waku::new(window, cx, daemon);
                        let composer_focus = waku.read(cx).composer_focus(cx);
                        window.focus(&composer_focus, cx);
                        waku
                    },
                )
                .expect("failed to open Goddard window");

            cx.on_system_notification_response({
                let window = window;
                move |response, cx| {
                    let Some(session_id) = crate::app::task_id_from_notification_tag(&response.tag)
                    else {
                        return;
                    };
                    window
                        .update(cx, |waku, window, cx| {
                            waku.open_task_from_notification(session_id, cx);
                            window.activate_window();
                            cx.activate(true);
                        })
                        .ok();
                    cx.dismiss_system_notification(&response.tag);
                }
            });

            window
                .update(cx, |waku, window, cx| {
                    let theme = crate::theme::Theme::current(cx);
                    crate::platform::configure_sidebar_material(
                        window,
                        theme.sidebar_drag_background,
                        theme.is_dark,
                        waku.sidebar_transparency(),
                    );
                    cx.activate(true);
                })
                .ok();

            if migration.failed() {
                let paths = migration
                    .failures
                    .iter()
                    .map(|failure| {
                        format!(
                            "• {} → {}",
                            failure.legacy.display(),
                            failure.destination.display()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                window
                    .update(cx, move |_, window, cx| {
                        let _ = window.prompt(
                            gpui::PromptLevel::Warning,
                            "Some of your previous Goddard data could not be copied",
                            Some(&format!(
                                "Goddard moved its data to a new location and the copy did not finish:\n\n{paths}\n\nYour old data was left untouched and the app is running with fresh state. Restart Goddard to retry the copy."
                            )),
                            &["OK"],
                            cx,
                        );
                    })
                    .ok();
            }

            set_app_menus(cx, updater_available);
            // A Linux handoff retains the previous prefix until this freshly
            // relaunched build has successfully opened its main window.
            crate::updater::signal_relaunch_ready();
        });
}

/// The window-level key bindings, registered once at startup. Kept callable
/// so tests can populate a keymap identical to the app's.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        // `secondary` is Command on macOS and Control elsewhere.
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-w", CloseWindow, None),
        KeyBinding::new("secondary-n", NewSession, None),
        // ⌘⇧N opens the "New task in…" directory picker in the palette when
        // no draft can host the project switcher — the same fall-through
        // ⌘N gives New Session.
        KeyBinding::new("secondary-shift-n", NewTaskIn, None),
        KeyBinding::new("secondary-o", NewProject, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-b", ToggleSidebar, None),
        KeyBinding::new("secondary-alt-b", ToggleRightPanel, None),
        KeyBinding::new("secondary-alt-g", ToggleGitPanel, None),
        // ⌘K opens the palette even in the terminal; ⌘⇧K clears the
        // scrollback there instead — hand-rolled in the terminal's
        // `on_key_down`. Elsewhere Ctrl+K is a shell shortcut
        // (readline kill-line), so it keeps passing through.
        KeyBinding::new(
            "secondary-k",
            ToggleCommandPalette,
            if cfg!(target_os = "macos") {
                None
            } else {
                Some("!Terminal")
            },
        ),
        KeyBinding::new("secondary-p", ToggleFileFinder, None),
        KeyBinding::new("secondary-alt-shift-f", ToggleFpsCounter, None),
        KeyBinding::new("secondary-[", NavigateBack, Some("Workspace")),
        KeyBinding::new("secondary-]", NavigateForward, Some("Workspace")),
        // ⌘1–⌘9 jump to the nth visible task in the sidebar; holding
        // ⌘ shows the same numbers as chips on the rows.
        KeyBinding::new("secondary-1", SelectSidebarSession { index: 0 }, None),
        KeyBinding::new("secondary-2", SelectSidebarSession { index: 1 }, None),
        KeyBinding::new("secondary-3", SelectSidebarSession { index: 2 }, None),
        KeyBinding::new("secondary-4", SelectSidebarSession { index: 3 }, None),
        KeyBinding::new("secondary-5", SelectSidebarSession { index: 4 }, None),
        KeyBinding::new("secondary-6", SelectSidebarSession { index: 5 }, None),
        KeyBinding::new("secondary-7", SelectSidebarSession { index: 6 }, None),
        KeyBinding::new("secondary-8", SelectSidebarSession { index: 7 }, None),
        KeyBinding::new("secondary-9", SelectSidebarSession { index: 8 }, None),
        // ⌘0 zooms out to Big Picture mode: the sessions most worth a
        // glance, side by side, with the composer docked underneath.
        KeyBinding::new("secondary-0", ToggleBigPicture, None),
        // ⌘⇧P opens the Projects page; pressed while open, it starts
        // the recent-project cycle the modifier release commits.
        KeyBinding::new("secondary-shift-p", ToggleProjectsPage, None),
        // ⌘⇧I opens the notification inbox — the same page contract the
        // Projects page has.
        KeyBinding::new("secondary-shift-i", ToggleInboxPage, None),
        // ⌘⇧U opens the Automations page — scheduling's letter in the
        // page-toggle family.
        KeyBinding::new("secondary-shift-u", ToggleAutomationsPage, None),
        // ⌘⌥1–3 switch the page's tabs while it is open. The chords used
        // to deep-link to a tab from anywhere; the model picker's
        // ⌘⌥1–⌘⌥9 favorite jump owns the workspace scope now.
        KeyBinding::new(
            "secondary-alt-1",
            SelectProjectsTab { index: 0 },
            Some("ProjectsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-2",
            SelectProjectsTab { index: 1 },
            Some("ProjectsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-3",
            SelectProjectsTab { index: 2 },
            Some("ProjectsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-1",
            SelectAutomationsTab { index: 0 },
            Some("AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-2",
            SelectAutomationsTab { index: 1 },
            Some("AutomationsPage"),
        ),
        // ⌘⌥1–⌘⌥9 apply the nth starred model selection to the composer
        // session — a draft or an idle task, ordered as in the picker's
        // favorites section. The terminal keeps every chord as pty input;
        // the Projects page keeps its own ⌘⌥ tab chords.
        KeyBinding::new(
            "secondary-alt-1",
            SelectFavoriteModel { index: 0 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-2",
            SelectFavoriteModel { index: 1 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-3",
            SelectFavoriteModel { index: 2 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-4",
            SelectFavoriteModel { index: 3 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-5",
            SelectFavoriteModel { index: 4 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-6",
            SelectFavoriteModel { index: 5 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-7",
            SelectFavoriteModel { index: 6 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-8",
            SelectFavoriteModel { index: 7 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-alt-9",
            SelectFavoriteModel { index: 8 },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        // ⌘E cycles the composer session's reasoning effort through the
        // current model's ladder; ⌘⇧E walks it in reverse.
        KeyBinding::new(
            "secondary-e",
            CycleReasoningEffort {
                direction: EffortCycleDirection::Forward,
            },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        KeyBinding::new(
            "secondary-shift-e",
            CycleReasoningEffort {
                direction: EffortCycleDirection::Backward,
            },
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        // ⌥Tab rotates the composer session's combo through the starred
        // selections plus the most recently used one.
        KeyBinding::new(
            "alt-tab",
            CycleFavoriteModel,
            Some("Workspace && !Terminal && !ProjectsPage && !AutomationsPage"),
        ),
        // Page-scoped list conventions — active only while focus is
        // inside the page, so a focused filter field keeps its own
        // ⌘A and first Escape. The Settings → Git page keeps the same
        // row set, so it claims the same chords on its own context.
        KeyBinding::new("secondary-a", SelectAllProjectsRows, Some("ProjectsPage")),
        KeyBinding::new("secondary-f", FocusProjectsFilter, Some("ProjectsPage")),
        KeyBinding::new("escape", DismissProjectsLayer, Some("ProjectsPage")),
        KeyBinding::new(
            "secondary-a",
            SelectAllProjectsRows,
            Some("GitSettingsPage"),
        ),
        KeyBinding::new("secondary-f", FocusProjectsFilter, Some("GitSettingsPage")),
        KeyBinding::new("escape", DismissProjectsLayer, Some("GitSettingsPage")),
        KeyBinding::new("escape", DismissInbox, Some("InboxPage")),
        // Step between turn prompts — the navigation rail's
        // landmarks. ⌘⌥ arrows are unclaimed by text fields, so the
        // pair works with the composer focused; in the terminal the
        // keys pass through to the shell.
        KeyBinding::new(
            "secondary-alt-up",
            GoToPreviousTurn,
            Some("Workspace && !Terminal"),
        ),
        KeyBinding::new(
            "secondary-alt-down",
            GoToNextTurn,
            Some("Workspace && !Terminal"),
        ),
        // Same spelling VS Code gives its terminal toggle; unclaimed
        // in text fields, so it fires with the composer focused too.
        // ⌘D reads as "done" and is the left-hand-only alternative.
        KeyBinding::new("ctrl-`", GoToNextUnreadCompletion, Some("Workspace")),
        KeyBinding::new("secondary-d", GoToNextUnreadCompletion, Some("Workspace")),
        // ⌘⇧D keeps the viewed task unread for a later ⌘D, then
        // moves down the sidebar to the next non-busy task.
        KeyBinding::new(
            "secondary-shift-d",
            MarkUnreadAndGoToNextIdle,
            Some("Workspace"),
        ),
        // ⌘⌥U is the sidebar's "Mark as Unread" on the viewed task,
        // without ⌘⇧D's jump to the next one waiting.
        KeyBinding::new("secondary-alt-u", MarkSessionUnread, Some("Workspace")),
        KeyBinding::new("ctrl-tab", SwitchTaskForward, Some("Workspace")),
        KeyBinding::new("ctrl-shift-tab", SwitchTaskBackward, Some("Workspace")),
        KeyBinding::new("ctrl-escape", CancelTaskSwitch, Some("Workspace")),
        KeyBinding::new("ctrl-shift-escape", CancelTaskSwitch, Some("Workspace")),
        KeyBinding::new("down", SwitchTaskForward, Some("TaskSwitcher")),
        KeyBinding::new("right", SwitchTaskForward, Some("TaskSwitcher")),
        KeyBinding::new("up", SwitchTaskBackward, Some("TaskSwitcher")),
        KeyBinding::new("left", SwitchTaskBackward, Some("TaskSwitcher")),
        KeyBinding::new("home", SelectFirstTask, Some("TaskSwitcher")),
        KeyBinding::new("end", SelectLastTask, Some("TaskSwitcher")),
        KeyBinding::new("enter", ConfirmTaskSwitch, Some("TaskSwitcher")),
        KeyBinding::new("escape", CancelTaskSwitch, Some("TaskSwitcher")),
        // The project switcher opens from the New Task page's draft and
        // commits when the platform modifier is released, the same
        // gesture as ctrl-tab above. Registered after New Session at
        // the same depth, the chord wins the tie and only falls
        // through to it when the New Task page is not on screen —
        // creating or revisiting a draft navigates there from any
        // other surface.
        KeyBinding::new("secondary-n", SwitchProjectForward, None),
        // ⌘⇧N mirrors the forward chord at the root: the overlay's focus
        // lands on a two-frame defer, so only a root binding keeps a fast
        // ⌘N-then-⌘⇧N from slipping to "New task in…" — which still gets
        // the keystroke when no draft can take the switcher.
        KeyBinding::new("secondary-shift-n", SwitchProjectBackward, None),
        KeyBinding::new("secondary-escape", CancelProjectSwitch, Some("Workspace")),
        KeyBinding::new(
            "secondary-shift-escape",
            CancelProjectSwitch,
            Some("Workspace"),
        ),
        // Re-bound on the overlay context so the chord cancels when
        // the switcher's focus path does not pass "Workspace" (the
        // settings branch renders the layer as its sibling).
        KeyBinding::new(
            "secondary-escape",
            CancelProjectSwitch,
            Some("ProjectSwitcher"),
        ),
        KeyBinding::new(
            "secondary-shift-escape",
            CancelProjectSwitch,
            Some("ProjectSwitcher"),
        ),
        KeyBinding::new("down", SwitchProjectForward, Some("ProjectSwitcher")),
        KeyBinding::new("right", SwitchProjectForward, Some("ProjectSwitcher")),
        KeyBinding::new("up", SwitchProjectBackward, Some("ProjectSwitcher")),
        KeyBinding::new("left", SwitchProjectBackward, Some("ProjectSwitcher")),
        KeyBinding::new("home", SelectFirstProject, Some("ProjectSwitcher")),
        KeyBinding::new("end", SelectLastProject, Some("ProjectSwitcher")),
        KeyBinding::new("enter", ConfirmProjectSwitch, Some("ProjectSwitcher")),
        KeyBinding::new("escape", CancelProjectSwitch, Some("ProjectSwitcher")),
        KeyBinding::new("secondary-l", FocusComposer, None),
        // With the transcript or a file editor focused — which a text
        // selection guarantees — ⌘L is the "Add to chat" pill's
        // shortcut. The action falls back to FocusComposer when no
        // annotatable selection is on screen, so the chord keeps its
        // global meaning everywhere else.
        KeyBinding::new(
            "secondary-l",
            AddToChat,
            Some("Transcript || FileEditorPane"),
        ),
        KeyBinding::new("secondary-j", FocusTerminal, None),
        // ⌘R opens the run-a-script picker everywhere except the
        // browser surface, whose deeper context keeps it as reload.
        KeyBinding::new("secondary-r", RunProjectScript, None),
        // ⌘T always spawns a terminal — rooted in the selected
        // terminal's directory, the selected session's workspace, or ~
        // when the main area shows neither.
        KeyBinding::new("secondary-t", NewTerminal, None),
        KeyBinding::new("secondary-/", ToggleModelPicker, None),
        KeyBinding::new("secondary-alt-shift-n", ToggleBranchPicker, None),
        KeyBinding::new("secondary-.", ToggleRuntimeModePicker, None),
        // ⌘⇧. flips the draft between this Mac and the sandbox VM — the
        // Environment section of the same menu, without opening it.
        KeyBinding::new("secondary-shift-.", ToggleEnvironment, None),
        // ⌘⇧T is the Terminals group chord: it expands the sidebar
        // section (selecting the last-shown terminal, or spawning one
        // in ~ when none exists), and once a full-width terminal is
        // active it opens another in the same directory.
        KeyBinding::new("secondary-shift-t", ToggleTerminals, None),
        // ⌘⌥N flips a draft's workspace between local and a new
        // worktree — ⌘⇧N is reserved for "New task in…".
        KeyBinding::new("secondary-alt-n", ToggleWorkspace, None),
        KeyBinding::new("secondary-u", ToggleUsagePanel, None),
        KeyBinding::new("secondary-s", SaveFile, None),
        // Font-size zoom follows focus: in the terminal it sizes the
        // terminal, on a code surface the code setting, and anywhere
        // else the interface. `secondary-=` covers ⌘= while
        // `secondary-shift-=` catches the ⌘+ spelling on layouts
        // where + is shift-=.
        KeyBinding::new(
            "secondary-=",
            AdjustFontSize {
                target: FontSizeTarget::Terminal,
                direction: FontSizeDirection::Increase,
            },
            Some("Terminal"),
        ),
        KeyBinding::new(
            "secondary-shift-=",
            AdjustFontSize {
                target: FontSizeTarget::Terminal,
                direction: FontSizeDirection::Increase,
            },
            Some("Terminal"),
        ),
        KeyBinding::new(
            "secondary--",
            AdjustFontSize {
                target: FontSizeTarget::Terminal,
                direction: FontSizeDirection::Decrease,
            },
            Some("Terminal"),
        ),
        KeyBinding::new(
            "secondary-=",
            AdjustFontSize {
                target: FontSizeTarget::Code,
                direction: FontSizeDirection::Increase,
            },
            Some("ReviewDiff || FileEditorPane"),
        ),
        KeyBinding::new(
            "secondary-shift-=",
            AdjustFontSize {
                target: FontSizeTarget::Code,
                direction: FontSizeDirection::Increase,
            },
            Some("ReviewDiff || FileEditorPane"),
        ),
        KeyBinding::new(
            "secondary--",
            AdjustFontSize {
                target: FontSizeTarget::Code,
                direction: FontSizeDirection::Decrease,
            },
            Some("ReviewDiff || FileEditorPane"),
        ),
        // The browser webview keeps ⌘± for its own page zoom.
        KeyBinding::new(
            "secondary-=",
            AdjustFontSize {
                target: FontSizeTarget::Ui,
                direction: FontSizeDirection::Increase,
            },
            Some("!Browser"),
        ),
        KeyBinding::new(
            "secondary-shift-=",
            AdjustFontSize {
                target: FontSizeTarget::Ui,
                direction: FontSizeDirection::Increase,
            },
            Some("!Browser"),
        ),
        KeyBinding::new(
            "secondary--",
            AdjustFontSize {
                target: FontSizeTarget::Ui,
                direction: FontSizeDirection::Decrease,
            },
            Some("!Browser"),
        ),
        // Escape is real input for a focused terminal — vim, fzf, and
        // agent TUIs all need it — so bare Escape is excluded from
        // CancelTurn there. ⌥Escape remains the one-press stop and
        // works with the terminal focused, skipping the confirmation
        // a bare Escape requires.
        KeyBinding::new(
            "escape",
            CancelTurn { immediate: false },
            Some("Workspace && !Terminal"),
        ),
        KeyBinding::new(
            "alt-escape",
            CancelTurn { immediate: true },
            Some("Workspace"),
        ),
        KeyBinding::new("secondary-shift-a", ArchiveSession, Some("Workspace")),
        // Push the landed session's base branch to its upstream — the
        // landed notice's button, without the card. A stronger submit
        // chord: `secondary-enter` already means "send" in the composer.
        KeyBinding::new("secondary-shift-enter", PushBaseBranch, Some("Workspace")),
        KeyBinding::new("secondary-alt-p", ToggleSessionPin, Some("Workspace")),
        KeyBinding::new("secondary-c", CopySelection, Some("Workspace")),
        KeyBinding::new("secondary-shift-c", CopyWorkingDirectory, Some("Workspace")),
        // Find and replace in the right panel's file editor, on the
        // conventional VS Code bindings. The primary shortcut + G cycles matches from
        // the editor without moving focus to the bar.
        KeyBinding::new("secondary-f", OpenFind, Some("Workspace")),
        // The text input's macOS-style Ctrl-F caret binding is more
        // specific than Workspace's root context. Reassert the platform
        // primary shortcut for inputs inside this window so Ctrl-F
        // remains find-in-page on Linux/Windows while Cmd-F keeps the
        // native behavior on macOS.
        KeyBinding::new("secondary-f", OpenFind, Some("Workspace > TextInput")),
        KeyBinding::new("secondary-alt-f", OpenFindReplace, Some("Workspace")),
        KeyBinding::new("secondary-g", FindNext, Some("Workspace")),
        KeyBinding::new("secondary-shift-g", FindPrevious, Some("Workspace")),
        // VS Code's other half of the pair: ctrl-g opens go-to-line. The
        // terminal keeps the keystroke — ^G is real input to a pty.
        KeyBinding::new("ctrl-g", OpenGoToLine, Some("Workspace && !Terminal")),
        // Scoped to the editor pane: escape closes the bar there and
        // falls through to CancelTurn anywhere else.
        KeyBinding::new("escape", CloseFind, Some("FileEditorPane")),
        KeyBinding::new("escape", CloseFind, Some("FindBar")),
        // Between FileEditorPane and Workspace: escape in a maximized
        // panel tab exits the mode once no find bar claims it,
        // instead of reaching CancelTurn. A terminal tab keeps the
        // keystroke for the pty even while maximized.
        KeyBinding::new(
            "escape",
            ExitPanelFullscreen,
            Some("PanelFullscreen && !Terminal"),
        ),
        KeyBinding::new(
            "secondary-alt-c",
            ToggleFindCaseSensitive,
            Some("FileEditorPane"),
        ),
        KeyBinding::new(
            "secondary-alt-w",
            ToggleFindWholeWord,
            Some("FileEditorPane"),
        ),
        KeyBinding::new("secondary-alt-r", ToggleFindRegex, Some("FileEditorPane")),
        KeyBinding::new("shift-enter", FindPrevious, Some("FindBar")),
        KeyBinding::new("secondary-alt-enter", ReplaceAllMatches, Some("FindBar")),
        // Browser surface. Deeper than "Workspace", so while focus is on the
        // page or its address bar the browser reads the platform's
        // conventional navigation shortcuts; the same keys elsewhere
        // keep their app meanings. The clipboard trio is rebound
        // because GPUI's window view claims key equivalents before
        // AppKit can walk the responder chain into the webview.
        KeyBinding::new("secondary-l", FocusBrowserAddress, Some("Browser")),
        KeyBinding::new("secondary-r", BrowserReload, Some("Browser")),
        KeyBinding::new("secondary-shift-r", BrowserHardReload, Some("Browser")),
        KeyBinding::new("secondary-[", BrowserBack, Some("Browser")),
        KeyBinding::new("secondary-]", BrowserForward, Some("Browser")),
        KeyBinding::new("escape", BrowserStop, Some("Browser")),
        KeyBinding::new("secondary-alt-i", BrowserDevtools, Some("Browser")),
        KeyBinding::new("secondary-c", WebviewCopy, Some("Browser")),
        KeyBinding::new("secondary-x", WebviewCut, Some("Browser")),
        KeyBinding::new("secondary-v", WebviewPaste, Some("Browser")),
        KeyBinding::new("secondary-a", WebviewSelectAll, Some("Browser")),
        KeyBinding::new("escape", BrowserAddressCancel, Some("BrowserAddress")),
        // A terminal's detected localhost URL: opens externally;
        // adding shift opens it in a built-in browser tab instead.
        KeyBinding::new("secondary-alt-o", OpenLocalhostUrl, None),
        KeyBinding::new("secondary-alt-shift-o", OpenLocalhostUrlInTab, None),
        // The unarchive toast's "View now" — shares ⌘⌥O with the localhost
        // open above. A session toast takes the chord; anything else
        // propagates back to the URL open.
        KeyBinding::new("secondary-alt-o", OpenToastSession, None),
        // The last-created GitHub issue — the toast's "View" without the
        // mouse. Deep-links the GitHub browser when its project and
        // number are known, falls back to the external URL.
        KeyBinding::new("secondary-alt-i", OpenCreatedIssueInGitHub, None),
    ]);

    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
    ]);
}

/// Rebuild the native menu bar in the active locale. GPUI menus own their
/// labels, so changing language must replace the model as well as redraw the
/// window.
pub(crate) fn set_app_menus(cx: &mut App, updater_available: bool) {
    cx.set_menus(vec![
        Menu {
            name: APP_NAME.into(),
            disabled: false,
            items: {
                let mut items = vec![MenuItem::action(tr!("menu.about", app = APP_NAME), About)];
                if updater_available {
                    items.push(MenuItem::action(
                        tr!("menu.check_for_updates"),
                        CheckForUpdates,
                    ));
                }
                items.push(MenuItem::separator());
                items.push(MenuItem::action(tr!("menu.settings"), OpenSettings));
                items.push(MenuItem::separator());
                #[cfg(target_os = "macos")]
                {
                    items.extend([
                        MenuItem::action(tr!("menu.hide", app = APP_NAME), Hide),
                        MenuItem::action(tr!("menu.hide_others"), HideOthers),
                        MenuItem::action(tr!("menu.show_all"), ShowAll),
                        MenuItem::separator(),
                    ]);
                }
                items.push(MenuItem::action(tr!("menu.quit", app = APP_NAME), Quit));
                items
            },
        },
        Menu {
            name: tr!("menu.file").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr!("menu.new_task"), NewSession),
                MenuItem::action(tr!("menu.new_task_in"), NewTaskIn),
                MenuItem::action(tr!("menu.new_project"), NewProject),
                MenuItem::action(tr!("menu.run_project_script"), RunProjectScript),
            ],
        },
        Menu {
            name: tr!("menu.edit").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr!("menu.undo"), input::Undo),
                MenuItem::action(tr!("menu.redo"), input::Redo),
                MenuItem::separator(),
                MenuItem::action(tr!("menu.cut"), input::Cut),
                MenuItem::action(tr!("menu.copy"), input::Copy),
                MenuItem::action(tr!("menu.paste"), input::Paste),
                MenuItem::action(tr!("menu.select_all"), input::SelectAll),
            ],
        },
        Menu {
            name: tr!("menu.view").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr!("menu.command_palette"), ToggleCommandPalette),
                MenuItem::separator(),
                MenuItem::action(tr!("menu.toggle_sidebar"), ToggleSidebar),
                MenuItem::action(tr!("menu.toggle_right_panel"), ToggleRightPanel),
                MenuItem::action(tr!("menu.toggle_git_panel"), ToggleGitPanel),
                MenuItem::action(tr!("menu.focus_composer"), FocusComposer),
                MenuItem::action(tr!("menu.focus_terminal"), FocusTerminal),
                MenuItem::action(tr!("menu.toggle_model_picker"), ToggleModelPicker),
                MenuItem::action(tr!("menu.toggle_branch_picker"), ToggleBranchPicker),
                MenuItem::action(
                    tr!("menu.toggle_runtime_mode_picker"),
                    ToggleRuntimeModePicker,
                ),
                MenuItem::action(tr!("menu.toggle_workspace"), ToggleWorkspace),
                MenuItem::action(tr!("menu.toggle_usage_panel"), ToggleUsagePanel),
            ],
        },
        Menu {
            name: tr!("menu.window").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr!("menu.toggle_fps_counter"), ToggleFpsCounter),
                MenuItem::action(tr!("menu.close_window"), CloseWindow),
            ],
        },
    ]);

    // Dock menus dispatch through the same app-menu action path, so the
    // taskbar jump list on Windows and the Dock menu on macOS get these
    // entries with no extra plumbing.
    let mut dock_items = vec![MenuItem::action(tr!("menu.new_task"), NewSession)];
    if updater_available {
        dock_items.push(MenuItem::action(
            tr!("menu.check_for_updates"),
            CheckForUpdates,
        ));
    }
    dock_items.push(MenuItem::action(tr!("menu.settings"), OpenSettings));
    cx.set_dock_menu(dock_items);
}
