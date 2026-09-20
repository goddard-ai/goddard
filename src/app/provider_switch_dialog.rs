//! Confirmation for switching a started session to a different provider.
//! The pick that triggered it rides along so confirming applies the full
//! provider/model/effort/tier row. Canceling leaves the session untouched.

use gpui::{KeyBinding, actions};

use super::provider_switch::{ProviderSwitchPick, provider_switch_estimate};
use super::usage_page::format_tokens_compact;
use super::*;

actions!(
    waku_provider_switch_dialog,
    [ConfirmProviderSwitchDialog, DismissProviderSwitchDialog]
);

const DIALOG_CONTEXT: &str = "ProviderSwitchDialog";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmProviderSwitchDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissProviderSwitchDialog, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct ProviderSwitchDialogState {
    pub session_id: Uuid,
    pub pick: ProviderSwitchPick,
    /// The target already has a suspended provider session: confirming
    /// resumes it with the compacted delta rather than seeding fresh.
    pub returning: bool,
    /// Transcript items and rough token count the target would ingest.
    pub estimate: (usize, u64),
    switch_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl Waku {
    /// Opens the provider-switch confirmation for `session`'s composer
    /// session. The caller focuses the returned handle.
    pub(super) fn open_provider_switch_dialog(
        &mut self,
        session_id: Uuid,
        pick: ProviderSwitchPick,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        let (returning, estimate) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| {
                (
                    session
                        .suspended_provider_sessions
                        .iter()
                        .any(|entry| entry.provider == pick.provider),
                    provider_switch_estimate(session, pick.provider),
                )
            })
            .unwrap_or((false, (0, 0)));
        let switch_focus = cx.focus_handle();
        self.provider_switch_dialog = Some(ProviderSwitchDialogState {
            session_id,
            pick,
            returning,
            estimate,
            cancel_focus: cx.focus_handle(),
            switch_focus: switch_focus.clone(),
        });
        cx.notify();
        switch_focus
    }

    fn close_provider_switch_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_switch_dialog.take().is_none() {
            return;
        }
        self.refocus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn render_provider_switch_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.provider_switch_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let from = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == dialog.session_id)
            .map(|session| session.provider)
            .unwrap_or_default();
        let (items, tokens) = dialog.estimate;

        let switch_row = render_provider_switch_action_row(
            "provider-switch-dialog-confirm",
            &dialog.switch_focus,
            "icons/arrow-right.svg",
            tr!(
                "provider_switch.confirm",
                provider = dialog.pick.provider.display_name()
            ),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_provider_switch(window, cx),
        );
        let cancel_row = render_provider_switch_action_row(
            "provider-switch-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.close_provider_switch_dialog(window, cx),
        );

        let description = if dialog.returning {
            tr!(
                "provider_switch.description_return",
                provider = dialog.pick.provider.display_name()
            )
        } else {
            tr!(
                "provider_switch.description_new",
                from = from.display_name(),
                provider = dialog.pick.provider.display_name()
            )
        };

        let card = div()
            .id("provider-switch-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(
                cx.listener(|waku, _: &ConfirmProviderSwitchDialog, window, cx| {
                    waku.confirm_provider_switch(window, cx)
                }),
            )
            .on_action(
                cx.listener(|waku, _: &DismissProviderSwitchDialog, window, cx| {
                    waku.close_provider_switch_dialog(window, cx)
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
                    .child(div().text_size(sp(14.0)).text_color(theme.text).child(tr!(
                        "provider_switch.title",
                        provider = dialog.pick.provider.display_name()
                    )))
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(description),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!(
                                "provider_switch.estimate",
                                items = items,
                                tokens = format_tokens_compact(tokens as f64)
                            )),
                    ),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(switch_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("provider-switch-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.close_provider_switch_dialog(window, cx)),
            )
            .child(motion::modal_enter(
                "provider-switch-dialog-card-enter",
                card,
            ));
        Some(
            gpui::deferred(motion::fade_in("provider-switch-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

fn render_provider_switch_action_row(
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
