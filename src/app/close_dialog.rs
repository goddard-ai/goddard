//! Confirmation shown before quitting Goddard or hiding its window while
//! sessions or terminals still have work in flight — an idle app quits and
//! hides outright, so this dialog only opens when live work would be
//! abandoned. Quitting kills the app-spawned daemon and every terminal PTY
//! with it; only sessions hosted on remote daemons keep running.

use gpui::{KeyBinding, actions};

use crate::identity::APP_NAME;

use super::*;

actions!(
    waku_close_dialog,
    [ConfirmAppClose, DismissAppClose]
);

const DIALOG_CONTEXT: &str = "AppCloseDialog";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmAppClose, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissAppClose, Some(DIALOG_CONTEXT)),
    ]);
}

/// What the confirmation guards: ⌘Q ends the app; ⌘W's last step and the
/// red close button hide the window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CloseDialogIntent {
    Quit,
    HideWindow,
}

pub(super) struct CloseDialogState {
    intent: CloseDialogIntent,
    counts: BusyCloseCounts,
    /// Focus taken on open lands back here on cancel.
    restore_focus: Option<FocusHandle>,
    confirm_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

/// Snapshot of the work a quit or window close interrupts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct BusyCloseCounts {
    /// Busy sessions on the app-spawned daemon — quitting kills it.
    local_sessions: usize,
    /// Busy sessions on remote daemons — those outlive the app.
    remote_sessions: usize,
    /// Terminals with a command still running — they die with the app.
    terminals: usize,
}

impl BusyCloseCounts {
    fn is_idle(self) -> bool {
        self.local_sessions == 0 && self.remote_sessions == 0 && self.terminals == 0
    }
}

/// Busy sessions the app owns, split by whose daemon runs them — friend
/// watches are read-only views of a session on someone else's daemon, so
/// a quit doesn't abandon them. Remote-host sessions run on a daemon the
/// quit cannot touch; local ones die with the app-spawned daemon.
pub(super) fn busy_owned_session_counts(
    sessions: &[AgentSession],
    is_watched: impl Fn(Uuid) -> bool,
    is_remote: impl Fn(Uuid) -> bool,
) -> (usize, usize) {
    let mut local = 0;
    let mut remote = 0;
    for session in sessions {
        if !session.is_busy() || is_watched(session.id) {
            continue;
        }
        if is_remote(session.id) {
            remote += 1;
        } else {
            local += 1;
        }
    }
    (local, remote)
}

impl Waku {
    /// Live work the close confirmation warns about: busy sessions split
    /// by owning daemon plus terminals with a command still running.
    fn busy_close_counts(&self, cx: &App) -> BusyCloseCounts {
        let (local_sessions, remote_sessions) = busy_owned_session_counts(
            &self.state.sessions,
            |id| self.friend_sessions.contains_key(&id),
            |id| self.daemons.is_remote_session(id),
        );
        let terminals = self
            .right_panel_terminals
            .values()
            .filter(|terminal| terminal.read(cx).command_running())
            .count();
        BusyCloseCounts {
            local_sessions,
            remote_sessions,
            terminals,
        }
    }

    /// ⌘Q / the Quit menu item: an idle app quits outright, work in flight
    /// earns a confirmation first.
    pub(super) fn quit_action(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy_close_counts(cx).is_idle() {
            cx.quit();
            return;
        }
        // The window may be hidden with the app still frontmost — bring it
        // back before asking.
        window.activate_window();
        let focus = self.open_close_dialog(CloseDialogIntent::Quit, window, cx);
        window.focus(&focus, cx);
    }

    /// The last ⌘W step and the red close button share this gate: hiding
    /// the window while work runs gets the same confirmation quit does.
    pub(super) fn request_window_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy_close_counts(cx).is_idle() {
            crate::platform::hide_window(window);
            return;
        }
        let focus = self.open_close_dialog(CloseDialogIntent::HideWindow, window, cx);
        window.focus(&focus, cx);
    }

    /// Opens the confirmation for `intent`, returning the handle to focus.
    /// Reopening while already up re-points the dialog at the newest
    /// gesture and re-snapshots the counts.
    fn open_close_dialog(
        &mut self,
        intent: CloseDialogIntent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        let counts = self.busy_close_counts(cx);
        if let Some(dialog) = &mut self.close_dialog {
            dialog.intent = intent;
            dialog.counts = counts;
            cx.notify();
            return dialog.confirm_focus.clone();
        }
        let confirm_focus = cx.focus_handle();
        self.close_dialog = Some(CloseDialogState {
            intent,
            counts,
            restore_focus: window.focused(cx),
            cancel_focus: cx.focus_handle(),
            confirm_focus: confirm_focus.clone(),
        });
        cx.notify();
        confirm_focus
    }

    fn confirm_close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.close_dialog.take() else {
            return;
        };
        match dialog.intent {
            CloseDialogIntent::Quit => cx.quit(),
            CloseDialogIntent::HideWindow => crate::platform::hide_window(window),
        }
    }

    fn dismiss_close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.close_dialog.take() else {
            return;
        };
        // Cancel returns focus to whatever was focused when the dialog
        // opened — a dialog row, a terminal, or the composer as fallback.
        let focus = dialog
            .restore_focus
            .unwrap_or_else(|| self.composer_focus(cx));
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn render_close_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.close_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let title = match dialog.intent {
            CloseDialogIntent::Quit => tr!("app_close.quit_title", app = APP_NAME),
            CloseDialogIntent::HideWindow => tr!("app_close.close_title"),
        };

        let mut lines: Vec<String> = Vec::new();
        match dialog.counts.local_sessions + dialog.counts.remote_sessions {
            1 => lines.push(tr!("app_close.busy_sessions_one")),
            n if n > 1 => lines.push(tr!("app_close.busy_sessions_many", count = n)),
            _ => {}
        }
        match dialog.counts.terminals {
            1 => lines.push(tr!("app_close.busy_terminals_one")),
            n if n > 1 => lines.push(tr!("app_close.busy_terminals_many", count = n)),
            _ => {}
        }
        // Hiding on macOS abandons nothing — the app keeps running behind
        // the hidden window. Everywhere else the close ends the app, which
        // takes the app-spawned daemon and the terminals with it; only
        // tasks on remote hosts keep going.
        if dialog.intent == CloseDialogIntent::HideWindow && cfg!(target_os = "macos") {
            lines.push(tr!("app_close.hide_consequence"));
        } else {
            if dialog.counts.local_sessions > 0 {
                lines.push(tr!("app_close.sessions_consequence_local", app = APP_NAME));
            }
            if dialog.counts.remote_sessions > 0 {
                lines.push(tr!("app_close.sessions_consequence_remote"));
            }
            if dialog.counts.terminals > 0 {
                lines.push(tr!("app_close.terminals_consequence"));
            }
        }

        let (confirm_label, confirm_icon) = match dialog.intent {
            CloseDialogIntent::Quit => {
                (tr!("menu.quit", app = APP_NAME), "icons/unplug.svg")
            }
            CloseDialogIntent::HideWindow => {
                (tr!("menu.close_window"), "icons/eye-off.svg")
            }
        };
        let confirm_row = render_close_dialog_row(
            "close-dialog-confirm",
            &dialog.confirm_focus,
            confirm_icon,
            confirm_label,
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_close_dialog(window, cx),
        );
        let cancel_row = render_close_dialog_row(
            "close-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.dismiss_close_dialog(window, cx),
        );

        let card = div()
            .id("close-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmAppClose, window, cx| {
                waku.confirm_close_dialog(window, cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissAppClose, window, cx| {
                waku.dismiss_close_dialog(window, cx)
            }))
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(460.0))
            .overflow_hidden()
            .rounded(px(21.0))
            .bg(theme.composer)
            .shadow_xl()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(10.0))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(sp(14.0))
                            .text_color(theme.text)
                            .child(title),
                    )
                    .children(lines.into_iter().map(|line| {
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(line)
                    })),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(confirm_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("close-dialog-layer")
            .absolute()
            .inset_0()
            .occlude()
            .bg(scrim)
            .p(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|waku, _, window, cx| waku.dismiss_close_dialog(window, cx)),
            )
            .child(motion::modal_enter("close-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("close-dialog-layer-enter", layer))
                // Above the dialogs other surfaces stack at priority 4 —
                // this one guards the whole window, so it wins the top.
                .with_priority(5)
                .into_any_element(),
        )
    }
}

fn render_close_dialog_row(
    id: &'static str,
    focus: &FocusHandle,
    icon_path: &'static str,
    label: String,
    weak: WeakEntity<Waku>,
    theme: &Theme,
    action: fn(&mut Waku, &mut Window, &mut Context<Waku>),
) -> Stateful<Div> {
    let click_weak = weak.clone();
    let key_weak = weak;
    div()
        .id(id)
        .track_focus(focus)
        .tab_index(0)
        .h(px(38.0))
        .w_full()
        .px(px(10.0))
        .rounded(px(11.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_default()
        .text_size(sp(14.0))
        .text_color(theme.text)
        .focus_visible(|style| style.bg(theme.focus_highlight()))
        .hover(|style| style.bg(theme.overlay_strong))
        .child(icon(icon_path, 15.0, theme.text))
        .child(div().min_w_0().flex_1().truncate().child(label))
        .on_click(move |_, window, cx| {
            let _ = click_weak.update(cx, |waku, cx| action(waku, window, cx));
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if !event.keystroke.modifiers.modified()
                && matches!(event.keystroke.key.as_str(), "enter" | "space")
            {
                let _ = key_weak.update(cx, |waku, cx| action(waku, window, cx));
                cx.stop_propagation();
            }
        })
}
