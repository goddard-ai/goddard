//! One-time confirmation shown the first time Full access is picked, the
//! mode that runs commands and edits without approval prompts. Confirming
//! persists `full_access_acknowledged` in app state and the gate never shows
//! again; canceling leaves the current mode untouched.

use gpui::{KeyBinding, actions};

use super::*;

actions!(
    waku_full_access_dialog,
    [ConfirmFullAccessDialog, DismissFullAccessDialog]
);

const DIALOG_CONTEXT: &str = "FullAccessDialog";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmFullAccessDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissFullAccessDialog, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct FullAccessDialogState {
    enable_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl Waku {
    /// Opens the one-time gate for Full access. The caller focuses the
    /// returned handle.
    pub(super) fn open_full_access_dialog(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        let enable_focus = cx.focus_handle();
        self.full_access_dialog = Some(FullAccessDialogState {
            cancel_focus: cx.focus_handle(),
            enable_focus: enable_focus.clone(),
        });
        cx.notify();
        enable_focus
    }

    fn confirm_full_access_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.full_access_dialog.take().is_none() {
            return;
        }
        self.state.full_access_acknowledged = true;
        self.save();
        self.set_runtime_mode(RuntimeMode::FullAccess, window, cx);
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    fn close_full_access_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.full_access_dialog.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn render_full_access_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.full_access_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();

        let enable_row = render_full_access_action_row(
            "full-access-dialog-enable",
            &dialog.enable_focus,
            "icons/lock-open.svg",
            tr!("full_access.confirm_enable"),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_full_access_dialog(window, cx),
        );
        let cancel_row = render_full_access_action_row(
            "full-access-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.close_full_access_dialog(window, cx),
        );

        let card = div()
            .id("full-access-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmFullAccessDialog, window, cx| {
                waku.confirm_full_access_dialog(window, cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissFullAccessDialog, window, cx| {
                waku.close_full_access_dialog(window, cx)
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
                            .child(tr!("full_access.confirm_title")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("full_access.confirm_description")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("full_access.confirm_effect")),
                    ),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(enable_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("full-access-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.close_full_access_dialog(window, cx)),
            )
            .child(motion::modal_enter("full-access-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("full-access-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

fn render_full_access_action_row(
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
        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
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
