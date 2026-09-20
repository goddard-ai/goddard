//! Confirmation shown before killing the terminal filling the main area
//! while its shell still reports a command in flight — ⌘W on an idle
//! terminal closes it outright, so this dialog only opens when a running
//! process would die with the surface.

use gpui::{KeyBinding, actions};

use super::*;

actions!(
    waku_terminal_close_dialog,
    [ConfirmTerminalClose, DismissTerminalClose]
);

const DIALOG_CONTEXT: &str = "TerminalCloseDialog";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmTerminalClose, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissTerminalClose, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct TerminalCloseDialogState {
    pub(super) terminal_id: Uuid,
    title: String,
    kill_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl Waku {
    /// Opens the confirmation for `terminal_id`, returning the handle to
    /// focus. Reopening for the same terminal keeps the dialog as is.
    pub(super) fn open_terminal_close_dialog(
        &mut self,
        terminal_id: Uuid,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        if let Some(dialog) = &self.terminal_close_dialog
            && dialog.terminal_id == terminal_id
        {
            return dialog.kill_focus.clone();
        }
        let title = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.custom_title.clone())
            .or_else(|| {
                self.right_panel_terminals
                    .get(&terminal_id)
                    .map(|terminal| terminal.read(cx).title().to_owned())
                    .filter(|title| !title.is_empty())
            })
            .unwrap_or_else(|| tr!("right_panel.terminal"));
        let kill_focus = cx.focus_handle();
        self.terminal_close_dialog = Some(TerminalCloseDialogState {
            terminal_id,
            title,
            cancel_focus: cx.focus_handle(),
            kill_focus: kill_focus.clone(),
        });
        cx.notify();
        kill_focus
    }

    fn confirm_terminal_close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.terminal_close_dialog.take() else {
            return;
        };
        self.close_terminal(dialog.terminal_id, cx);
        // drop_terminal already focused the successor terminal; with none
        // left the main area falls back to the task surface's composer.
        if self.selected_terminal.is_none() {
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
        }
    }

    fn dismiss_terminal_close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.terminal_close_dialog.take() else {
            return;
        };
        // Cancel returns focus to the terminal the user was about to kill.
        let focus = self
            .right_panel_terminals
            .get(&dialog.terminal_id)
            .map(|terminal| terminal.read(cx).focus_handle(cx))
            .unwrap_or_else(|| self.composer_focus(cx));
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn render_terminal_close_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.terminal_close_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let title = if dialog.title.trim().is_empty() {
            tr!("terminal.close_title")
        } else {
            tr!("terminal.close_title_named", name = dialog.title.as_str())
        };

        let kill_row = render_terminal_close_action_row(
            "terminal-close-dialog-kill",
            &dialog.kill_focus,
            "icons/trash.svg",
            tr!("terminal.kill"),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_terminal_close_dialog(window, cx),
        );
        let cancel_row = render_terminal_close_action_row(
            "terminal-close-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.dismiss_terminal_close_dialog(window, cx),
        );

        let card = div()
            .id("terminal-close-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmTerminalClose, window, cx| {
                waku.confirm_terminal_close_dialog(window, cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissTerminalClose, window, cx| {
                waku.dismiss_terminal_close_dialog(window, cx)
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
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("terminal.close_description")),
                    ),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(kill_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("terminal-close-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.dismiss_terminal_close_dialog(window, cx)),
            )
            .child(motion::modal_enter(
                "terminal-close-dialog-card-enter",
                card,
            ));
        Some(
            gpui::deferred(motion::fade_in("terminal-close-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

fn render_terminal_close_action_row(
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
