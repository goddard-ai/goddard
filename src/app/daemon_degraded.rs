//! Degraded-daemon surface: a persistent banner while the local daemon
//! stays alive but unreachable, and the confirmation that explains the
//! restart's blast radius — running tasks and terminals stop — before the
//! only action that still kills a live daemon.

use gpui::{KeyBinding, WeakEntity, actions};

use super::*;

actions!(
    waku_daemon_restart_dialog,
    [ConfirmDaemonRestart, DismissDaemonRestart]
);

const DIALOG_CONTEXT: &str = "DaemonRestartDialog";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmDaemonRestart, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissDaemonRestart, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct DaemonRestartDialogState {
    restart_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl Waku {
    /// Opens the restart confirmation. The restart row stays unfocused on
    /// open — cancel owns focus so a reflexive Enter cannot interrupt
    /// running tasks.
    fn open_daemon_restart_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.daemon_restart_pending {
            return;
        }
        let cancel_focus = cx.focus_handle();
        self.daemon_restart_dialog = Some(DaemonRestartDialogState {
            restart_focus: cx.focus_handle(),
            cancel_focus: cancel_focus.clone(),
        });
        window.focus(&cancel_focus, cx);
        cx.notify();
    }

    fn confirm_daemon_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.daemon_restart_dialog.take().is_none() {
            return;
        }
        let daemon = self.daemons.local();
        self.daemon_restart_pending = true;
        cx.notify();
        // The swap blocks on spawn and shutdown — keep it off the UI
        // thread, like every other daemon-lifecycle call.
        let restart = cx
            .background_executor()
            .spawn(async move { daemon.restart_local_daemon() });
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.spawn(async move |waku, cx| {
            let result = restart.await;
            let _ = waku.update(cx, |waku, cx| {
                waku.daemon_restart_pending = false;
                if let Err(error) = result {
                    waku.show_toast(tr!("daemon.restart_failed", error = error.to_string()));
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn dismiss_daemon_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.daemon_restart_dialog.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The banner mounts while the local daemon is alive but unreachable —
    /// the supervisor keeps reconnecting, and the action is the only way a
    /// live daemon still gets replaced.
    pub(super) fn daemon_degraded_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.local_daemon_status != waku_client::DaemonStatus::Degraded {
            return None;
        }
        let theme = Theme::current(cx);
        let pending = self.daemon_restart_pending;
        let label = if pending {
            tr!("daemon.restart_pending")
        } else {
            tr!("daemon.restart")
        };
        Some(
            div()
                .flex_none()
                .px(px(12.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .bg(theme.inset)
                .border_b(hairline())
                .border_color(theme.border)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(sp(11.5))
                        .text_color(theme.text_secondary)
                        .child(tr!("daemon.degraded_banner")),
                )
                .child(
                    div()
                        .id("daemon-restart")
                        .tab_index(0)
                        .px(px(8.0))
                        .h(px(20.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .cursor_default()
                        .text_size(sp(11.5))
                        .text_color(if pending {
                            theme.text_tertiary
                        } else {
                            theme.danger
                        })
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .hover(|element| element.bg(theme.overlay).text_color(theme.text))
                        .child(label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_daemon_restart_dialog(window, cx);
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if !event.keystroke.modifiers.modified()
                                && matches!(event.keystroke.key.as_str(), "enter" | "space")
                            {
                                this.open_daemon_restart_dialog(window, cx);
                                cx.stop_propagation();
                            }
                        })),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_daemon_restart_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.daemon_restart_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();

        let restart_row = render_daemon_restart_row(
            "daemon-restart-dialog-confirm",
            &dialog.restart_focus,
            "icons/refresh-cw.svg",
            tr!("daemon.restart_confirm_action"),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_daemon_restart(window, cx),
        );
        let cancel_row = render_daemon_restart_row(
            "daemon-restart-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.dismiss_daemon_restart(window, cx),
        );

        let card = div()
            .id("daemon-restart-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(
                cx.listener(|waku, _: &ConfirmDaemonRestart, window, cx| {
                    waku.confirm_daemon_restart(window, cx)
                }),
            )
            .on_action(
                cx.listener(|waku, _: &DismissDaemonRestart, window, cx| {
                    waku.dismiss_daemon_restart(window, cx)
                }),
            )
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
                            .child(tr!("daemon.restart_confirm_title")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("daemon.restart_confirm_body")),
                    ),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(restart_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("daemon-restart-dialog-layer")
            .absolute()
            .inset_0()
            .occlude()
            .child(motion::scrim_enter("daemon-restart-dialog-layer-enter", scrim))
            .p(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|waku, _, window, cx| waku.dismiss_daemon_restart(window, cx)),
            )
            .child(motion::modal_enter("daemon-restart-dialog-card-enter", card));
        Some(gpui::deferred(layer).with_priority(4).into_any_element())
    }
}

fn render_daemon_restart_row(
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
