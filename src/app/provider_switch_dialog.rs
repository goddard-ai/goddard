//! Confirmation for switching a started session to a different provider.
//! The pick that triggered it rides along so confirming applies the full
//! provider/model/effort/tier row. Canceling leaves the session untouched.

use gpui::{KeyBinding, actions};

use super::provider_switch::{ProviderSwitchPick, provider_switch_estimate};
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
    /// How many turns of transcript the handoff covers.
    pub estimate: usize,
    /// The session's daemon has no usable eval backend — the switch still
    /// proceeds, degraded to a pointer-only handoff, so this is a warning
    /// with an optional fix path, not a gate.
    pub eval_missing: bool,
    /// `eval_missing` on a remote host — the Jev page edits the local
    /// daemon's backend, so this UI's setup path can't repair it.
    pub eval_missing_remote: bool,
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
            .unwrap_or((false, 0));
        // A daemon that can't be inspected (offline remote) isn't "missing a
        // backend" — its state is unknown, and confirm reports disconnected.
        let eval_missing = self
            .daemons
            .daemon_for_session(session_id)
            .is_some_and(|daemon| {
                daemon
                    .settings()
                    .eval
                    .is_none_or(|eval| eval.credential_missing())
            });
        let eval_missing_remote =
            eval_missing && self.daemons.session_owner(session_id).is_remote();
        let switch_focus = cx.focus_handle();
        self.provider_switch_dialog = Some(ProviderSwitchDialogState {
            session_id,
            pick,
            returning,
            estimate,
            eval_missing,
            eval_missing_remote,
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

    /// The dialog's Enter: confirm the switch. A missing eval backend
    /// degrades the handoff to a transcript pointer instead of failing, so
    /// confirming is always primary.
    fn provider_switch_primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_switch_dialog.is_some() {
            self.confirm_provider_switch(window, cx);
        }
    }

    /// The warning's fix path: dismiss the dialog and deep-link to the Jev
    /// page, bypassing its navigation gate — provider switching is shipped,
    /// so its backend surface can't depend on an experiment being on.
    fn open_jev_settings_from_switch_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.provider_switch_dialog = None;
        self.open_settings_page_direct(SettingsPage::Jev, window, cx);
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
        let turns = dialog.estimate;

        let eval_missing = dialog.eval_missing;
        let eval_missing_remote = dialog.eval_missing_remote;
        // The missing-backend warning is informational — the switch still
        // runs, degraded to a pointer-only handoff. The Jev settings row
        // stays as an optional fix for local daemons only: a remote host's
        // backend can't be configured from this UI.
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
        let jev_row = render_provider_switch_action_row(
            "provider-switch-dialog-jev-settings",
            &dialog.switch_focus,
            "icons/settings.svg",
            tr!("experiments.open_jev_settings"),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.open_jev_settings_from_switch_dialog(window, cx),
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
                    waku.provider_switch_primary_action(window, cx)
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
                            .child(tr!("provider_switch.estimate", turns = turns)),
                    )
                    .when(eval_missing, |header| {
                        header.child(
                            div()
                                .pt(px(4.0))
                                .flex()
                                .gap(px(7.0))
                                .child(icon("icons/alert.svg", 12.0, theme.warning).mt(px(2.0)))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .text_size(sp(12.5))
                                        .line_height(sp(17.0))
                                        .text_color(theme.text_secondary)
                                        .child(if eval_missing_remote {
                                            tr!(
                                                "provider_switch.needs_eval_backend_remote",
                                                provider = dialog.pick.provider.display_name()
                                            )
                                        } else {
                                            tr!(
                                                "provider_switch.needs_eval_backend",
                                                provider = dialog.pick.provider.display_name()
                                            )
                                        }),
                                ),
                        )
                    }),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(switch_row)
                    .when(eval_missing && !eval_missing_remote, |rows| {
                        rows.child(jev_row)
                    })
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
