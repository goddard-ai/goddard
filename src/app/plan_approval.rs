use super::*;
use crate::ui::ActivationExt;
use crate::ui::shortcut::ShortcutHint;

/// The canned-prompt id the approval chip sends — a CANNED_PROMPTS entry,
/// so the text is localizable, user-editable under Settings' suggested
/// prompts, and journaled with its own id.
pub(super) const PLAN_APPROVAL_ACTION: &str = "approve-plan";

/// Whether the approval prompt has already landed in the session — a sent
/// user message or a queued follow-up. Both persist in the session
/// document, so the gate survives restarts; a prompt parked for an offline
/// remote has not landed yet and the chip stays. Text matches trimmed and
/// case-insensitive, the same normalization `canned_prompt_id` applies.
pub(super) fn session_plan_approval_sent(session: &AgentSession, prompt: &str) -> bool {
    let sent = |text: &str| text.trim().eq_ignore_ascii_case(prompt.trim());
    session
        .queued_messages
        .iter()
        .any(|queued| sent(queued.display_content.as_deref().unwrap_or(&queued.content)))
        || session.messages.iter().any(|message| {
            message.role == MessageRole::User
                && !message.hidden
                && sent(
                    message
                        .display_content
                        .as_deref()
                        .unwrap_or(&message.content),
                )
        })
}

impl Waku {
    /// The configured approval prompt — Settings overrides apply, so the
    /// text the chip sends and the text the sent-state check looks for
    /// always agree.
    fn plan_approval_prompt(&self) -> Option<String> {
        action_predictions::suggested_prompt(PLAN_APPROVAL_ACTION, &self.state.suggested_prompts)
    }

    /// `session_plan_approval_sent` with the configured prompt.
    fn plan_approval_sent(&self, session: &AgentSession) -> bool {
        self.plan_approval_prompt()
            .is_some_and(|prompt| session_plan_approval_sent(session, &prompt))
    }

    /// Whether the composer gives way to the sealed card: the plan is
    /// frozen, a finalize dispatch is still in flight, or the approval
    /// prompt already went out and the session's boss owns the transcript
    /// from there — a finalized planning session is read-only for the
    /// human.
    pub(super) fn plan_execution_locked(&self, session: &AgentSession) -> bool {
        let Some(planning) = session.planning.as_ref() else {
            return false;
        };
        self.boss_ui.plan_finalizing.contains(&session.id)
            || planning.finalized_at.is_some()
            || self.plan_approval_sent(session)
    }

    /// The gate behind the approval chip: an unfinalized planning session
    /// with no approval prompt sent yet and no finalize dispatch in
    /// flight — a press lifts the chip until the reply lands.
    pub(super) fn plan_approval_pending(&self, session: &AgentSession) -> bool {
        session
            .planning
            .as_ref()
            .is_some_and(|planning| planning.finalized_at.is_none())
            && !self.boss_ui.plan_finalizing.contains(&session.id)
            && !self.plan_approval_sent(session)
    }

    /// Send the human's `finalizePlan` for a still-pending planning
    /// session — the approval chip's activation and the empty-composer
    /// ⌘⏎ share this dispatch. The pending gate is re-checked here so a
    /// call arriving after finalization or approval is a no-op.
    pub(super) fn request_plan_finalization(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if !self.plan_approval_pending(session) {
            return;
        }
        let Some(plan_file) = session
            .planning
            .as_ref()
            .map(|planning| planning.plan_file.clone())
        else {
            return;
        };
        let key = self.daemons.session_owner(session_id);
        self.boss_request(
            key,
            waku_client::boss::BossOperation::FinalizePlan {
                plan_file: Some(plan_file),
                // Approval only seals the document — an already-declared
                // work breakdown stays as it is.
                items: None,
            },
            super::boss::BossReply::Finalize(session_id),
            cx,
        );
    }

    /// The "Finalize plan" approval chip — suggestion-chip shell, same as
    /// the voice-briefing pause. It rides the composer suggestion rows
    /// when one claims the slot; otherwise `render_composer_float_chips`
    /// hangs it off the composer card's top edge in the same place.
    /// Clicking sends the human's `finalizePlan` request directly, so this
    /// action is the approval and does not ask the user to confirm it again.
    pub(super) fn plan_approval_chip(
        &self,
        window: &Window,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let session = self.composer_session()?;
        if !self.plan_approval_pending(session) {
            return None;
        }
        let session_id = session.id;
        // ⌘⏎ fires the chip while the composer is empty — advertise the
        // chord exactly when it is bound, resolved as if the field were
        // focused (the binding lives on the TextInput context). Big
        // Picture's empty-draft chord does not reach the suggestion lane,
        // so the chip stays silent there.
        let shortcut_label = if !self.big_picture.is_open() && self.composer_is_empty(cx) {
            ShortcutHint::action_in(&crate::input::SubmitSteer, &self.composer_focus(cx))
                .resolve(window, cx)
        } else {
            None
        };
        let (icon_path, label) = self.suggestion_parts(PLAN_APPROVAL_ACTION)?;
        Some(
            div()
                .id("plan-approval")
                .tab_index(0)
                .flex()
                .items_center()
                .gap(px(5.0))
                .h(px(24.0))
                .px(px(9.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.raised)
                .cursor_default()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                // The wash composites over `raised` — painting the
                // translucent token alone would let the transcript ghost
                // through.
                .hover(|element| element.bg(theme.raised.blend(theme.overlay_strong)))
                .child(icon(icon_path, 11.0, theme.text_secondary))
                .child(label)
                .when_some(shortcut_label, |chip, label| {
                    chip.child(
                        div()
                            .flex_none()
                            .relative()
                            .top(px(2.0))
                            .text_size(sp(11.0))
                            .text_color(theme.text_tertiary)
                            .child(label),
                    )
                })
                .tooltip(Tooltip::text(tr_cow!("boss.plan_approve_hint")))
                .on_activation(cx, move |this, _, cx| {
                    this.request_plan_finalization(session_id, cx);
                }),
        )
    }

    /// The floating chips' standalone mount — an absolute row off the
    /// composer card's top edge at the suggestion chips' left inset. While
    /// a suggestion row claims the slot the chips ride that row instead, so
    /// the two never overlap; Big Picture mounts no suggestion row, so the
    /// float always stands alone there.
    pub(super) fn render_composer_float_chips(
        &self,
        window: &Window,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if self.voice_scratchpad_visible()
            || (self.action_suggestion_row_visible() && !self.big_picture.is_open())
        {
            return None;
        }
        let approval = self.plan_approval_chip(window, theme, cx);
        if approval.is_none() {
            return None;
        }
        // A parked queue card sits flush on the composer card's top edge, so
        // the float rises past it rather than covering its last row. Big
        // Picture mounts no queue card — the probe's last write stays parked
        // in the cell, and there the card's top edge is the right anchor.
        let queue_height = if self.big_picture.is_open() {
            0.0
        } else {
            self.composer_autocomplete.queue_card_height()
        };
        Some(
            div()
                .absolute()
                .top(px(-32.0 - queue_height))
                .left(px(COMPOSER_CHIP_INSET))
                .flex()
                .gap(px(6.0))
                .children(approval),
        )
    }

    /// The sealed planning session's card in the composer slot — the same
    /// chrome as the quarantine card. The session no longer takes prompts,
    /// so the field and its affordances stay unmounted and a muted strip
    /// carries the state instead.
    pub(super) fn render_plan_sealed_card(
        &self,
        session: &AgentSession,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let finalized = session
            .planning
            .as_ref()
            .is_some_and(|planning| planning.finalized_at.is_some());
        let (title, hint) = if finalized {
            (tr!("boss.plan_finalized"), tr!("boss.plan_sealed_hint"))
        } else {
            (
                tr!("boss.plan_sent_for_execution"),
                tr!("boss.plan_sent_hint"),
            )
        };
        div().flex_none().px(px(20.0 - COMPOSER_OVERHANG)).child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0))
                .mx_auto()
                .rounded(px(18.0))
                .border(hairline())
                .border_color(theme.border)
                .bg(theme.composer)
                .py(px(10.0))
                .px(px(14.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(icon("icons/compass.svg", 14.0, theme.text_tertiary))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .whitespace_normal()
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(sp(11.5))
                                .text_color(theme.text_tertiary)
                                .child(hint),
                        ),
                ),
        )
    }
}
