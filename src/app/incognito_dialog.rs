//! One-time explanation shown before Incognito is used for the first time.
//! Confirming stores the acknowledgement in app state; canceling leaves the
//! pending action untouched.

use gpui::{KeyBinding, actions};

use super::*;

actions!(
    waku_incognito_dialog,
    [ConfirmIncognitoDisclosure, DismissIncognitoDisclosure]
);

const DIALOG_CONTEXT: &str = "IncognitoDisclosureDialog";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmIncognitoDisclosure, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissIncognitoDisclosure, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) enum IncognitoDisclosureAction {
    NewTask,
    NewTaskIn,
    SameWorktree,
    ComposerCommand(String),
}

pub(super) struct IncognitoDialogState {
    action: IncognitoDisclosureAction,
    return_focus: FocusHandle,
    confirm_focus: FocusHandle,
    cancel_focus: FocusHandle,
    focus_pending: bool,
}

impl Waku {
    /// Returns true when the action is waiting for the user's first-use
    /// confirmation. The caller should stop before changing Incognito state.
    pub(super) fn require_incognito_disclosure(
        &mut self,
        action: IncognitoDisclosureAction,
        return_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.state.incognito_disclosure_acknowledged {
            return false;
        }
        if self.incognito_dialog.is_some() {
            return true;
        }
        self.incognito_dialog = Some(IncognitoDialogState {
            action,
            return_focus,
            confirm_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            focus_pending: true,
        });
        cx.notify();
        true
    }

    fn confirm_incognito_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.incognito_dialog.take() else {
            return;
        };
        self.state.incognito_disclosure_acknowledged = true;
        self.save();
        window.focus(&dialog.return_focus, cx);
        match dialog.action {
            IncognitoDisclosureAction::NewTask => self.new_incognito_session_action(window, cx),
            IncognitoDisclosureAction::NewTaskIn => {
                self.open_command_palette_new_task_view(true, cx)
            }
            IncognitoDisclosureAction::SameWorktree => {
                self.new_incognito_task_in_same_worktree(window, cx)
            }
            IncognitoDisclosureAction::ComposerCommand(prompt) => {
                self.execute_incognito_composer_command(&prompt, cx);
            }
        }
        cx.notify();
    }

    fn close_incognito_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.incognito_dialog.take() else {
            return;
        };
        window.focus(&dialog.return_focus, cx);
        cx.notify();
    }

    pub(super) fn render_incognito_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (focus_pending, confirm_focus) = {
            let dialog = self.incognito_dialog.as_ref()?;
            (dialog.focus_pending, dialog.confirm_focus.clone())
        };
        if focus_pending {
            if let Some(dialog) = self.incognito_dialog.as_mut() {
                dialog.focus_pending = false;
            }
            window.on_next_frame(move |window, cx| window.focus(&confirm_focus, cx));
        }
        let dialog = self.incognito_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let confirm_row = render_incognito_action_row(
            "incognito-dialog-confirm",
            &dialog.confirm_focus,
            "icons/eye-off.svg",
            tr!("session.incognito_disclosure_continue"),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_incognito_dialog(window, cx),
        );
        let cancel_row = render_incognito_action_row(
            "incognito-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.close_incognito_dialog(window, cx),
        );

        let card = div()
            .id("incognito-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(
                cx.listener(|waku, _: &ConfirmIncognitoDisclosure, window, cx| {
                    waku.confirm_incognito_dialog(window, cx)
                }),
            )
            .on_action(
                cx.listener(|waku, _: &DismissIncognitoDisclosure, window, cx| {
                    waku.close_incognito_dialog(window, cx)
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
                    .pb(px(12.0))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(sp(14.0))
                            .text_color(theme.text)
                            .child(tr!("session.incognito_disclosure_title")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("session.incognito_disclosure_description")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("session.incognito_disclosure_memory")),
                    ),
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
            .id("incognito-dialog-layer")
            .absolute()
            .inset_0()
            .occlude()
            .child(motion::scrim_enter("incognito-dialog-layer-enter", scrim))
            .p(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|waku, _, window, cx| waku.close_incognito_dialog(window, cx)),
            )
            .child(motion::modal_enter("incognito-dialog-card-enter", card));
        Some(gpui::deferred(layer).with_priority(4).into_any_element())
    }
}

fn render_incognito_action_row(
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
