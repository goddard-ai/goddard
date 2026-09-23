//! Confirmation before spending a banked Codex reset credit. Redemption is
//! irreversible and moves the account's weekly reset anchor, so the usage
//! panel's "Use reset" row always stops here first. Mounted as a deferred
//! layer at the window root — opening it from the usage popover cannot
//! unmount it when the popover dismisses.

use gpui::{KeyBinding, actions};

use super::*;

actions!(
    waku_reset_credit_dialog,
    [ConfirmResetCreditDialog, DismissResetCreditDialog]
);

const DIALOG_CONTEXT: &str = "ResetCreditDialog";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmResetCreditDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissResetCreditDialog, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct ResetCreditDialogState {
    confirm_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl Waku {
    /// Opens the redemption confirmation. Returns the confirm row's focus
    /// handle — callers run the two-frame deferred-focus handoff.
    pub(super) fn open_reset_credit_dialog(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        let confirm_focus = cx.focus_handle();
        self.reset_credit_dialog = Some(ResetCreditDialogState {
            confirm_focus: confirm_focus.clone(),
            cancel_focus: cx.focus_handle(),
        });
        cx.notify();
        confirm_focus
    }

    /// Confirming closes the dialog and spends one credit through the
    /// daemon — which serializes redemptions and re-reads the account so the
    /// outcome arrives with fresh windows. The verdict lands on the toast.
    fn confirm_reset_credit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reset_credit_dialog.take().is_none() {
            return;
        }
        window.focus(&self.composer_focus(cx), cx);
        let daemon = self.daemon.client();
        let tx = self.reset_credit_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        cx.background_executor()
            .spawn(async move {
                let result = match daemon.request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::ConsumeCodexResetCredit {
                        redeem_request_id: Uuid::new_v4().to_string(),
                    },
                ) {
                    Ok(waku_client::ResponsePayload::CodexResetCredit { outcome, usage }) => {
                        Ok((outcome, usage))
                    }
                    Ok(_) => Err(anyhow::anyhow!(
                        "the daemon returned an invalid reset-credit response"
                    )),
                    Err(error) => Err(error),
                };
                if tx
                    .send(result.map_err(|error| format!("{error:#}")))
                    .is_ok()
                {
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    fn close_reset_credit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reset_credit_dialog.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The consume verdict: a fresh snapshot refreshes the meter when the
    /// daemon's confirm read landed; the outcome itself becomes toast copy.
    pub(super) fn drain_reset_credit_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.reset_credit_events.try_recv() {
            changed = true;
            match result {
                Ok((outcome, usage)) => {
                    let confirmed = usage.is_some();
                    if let Some(usage) = usage {
                        self.plan_usage.insert(ProviderKind::Codex, usage);
                    }
                    match outcome {
                        crate::usage::CodexResetCreditOutcome::Reset if confirmed => {
                            self.show_success_toast(tr!("usage.reset_credit_applied"))
                        }
                        crate::usage::CodexResetCreditOutcome::Reset => {
                            self.show_toast(tr!("usage.reset_credit_unconfirmed"))
                        }
                        crate::usage::CodexResetCreditOutcome::NothingToReset => {
                            self.show_toast(tr!("usage.reset_credit_nothing"))
                        }
                        crate::usage::CodexResetCreditOutcome::NoCredit => {
                            self.show_toast(tr!("usage.reset_credit_none"))
                        }
                        crate::usage::CodexResetCreditOutcome::AlreadyRedeemed => {
                            self.show_toast(tr!("usage.reset_credit_redeemed"))
                        }
                    }
                }
                Err(error) => self.show_toast(tr!("usage.reset_credit_failed", error = error)),
            }
        }
        changed
    }

    pub(super) fn render_reset_credit_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.reset_credit_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();

        let confirm_row = render_reset_credit_action_row(
            "reset-credit-dialog-confirm",
            &dialog.confirm_focus,
            "icons/rotate-cw.svg",
            tr!("usage.use_reset_credit"),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_reset_credit_dialog(window, cx),
        );
        let cancel_row = render_reset_credit_action_row(
            "reset-credit-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.close_reset_credit_dialog(window, cx),
        );

        let card = div()
            .id("reset-credit-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(
                cx.listener(|waku, _: &ConfirmResetCreditDialog, window, cx| {
                    waku.confirm_reset_credit_dialog(window, cx)
                }),
            )
            .on_action(
                cx.listener(|waku, _: &DismissResetCreditDialog, window, cx| {
                    waku.close_reset_credit_dialog(window, cx)
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
                            .child(tr!("usage.reset_credit_title")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("usage.reset_credit_description")),
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
            .id("reset-credit-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.close_reset_credit_dialog(window, cx)),
            )
            .child(motion::modal_enter("reset-credit-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("reset-credit-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

fn render_reset_credit_action_row(
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
