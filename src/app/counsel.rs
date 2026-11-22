use super::boss::BossReply;
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_protocol::boss::BossOperation;
use waku_protocol::counsel::{CounselDecision, CounselOperation, CounselStatus};

impl Waku {
    pub(super) fn counsel_chat_key(&self) -> Option<DaemonKey> {
        self.counsel_state_for_session(self.composer_session_id()?)
            .map(|(key, _)| key)
    }

    pub(super) fn counsel_state_for_session(
        &self,
        id: Uuid,
    ) -> Option<(DaemonKey, &waku_protocol::counsel::CounselState)> {
        self.boss_ui.states.iter().find_map(|(key, state)| {
            (state.counsel.session_id == Some(id)).then_some((*key, &state.counsel))
        })
    }

    pub(super) fn counsel_can_continue(&self, session: &AgentSession) -> bool {
        !session.status.is_busy()
            && session.active_turn_id().is_none()
            && self.session_detail_is_ready(session)
            && !self.submission_preparations.contains(&session.id)
            && !self.response_fork_preparations.contains_key(&session.id)
            && !self.runtime_submission_checks.contains_key(&session.id)
            && self
                .counsel_state_for_session(session.id)
                .is_some_and(|(key, _)| self.counsel_enabled_on(key))
    }

    /// Counsel needs both experiment flags on the owning host — its own
    /// opt-in and the Boss experiment it rides on.
    pub(super) fn counsel_enabled_on(&self, key: DaemonKey) -> bool {
        let settings = self.daemon_settings_for_key(key);
        settings.boss_experiment_enabled && settings.counsel_experiment_enabled
    }

    pub(super) fn chat_with_counsel(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        if !self.counsel_enabled_on(key) {
            return;
        }
        let provider = self
            .selected_session()
            .map(|session| session.provider)
            .unwrap_or(ProviderKind::Codex);
        let model = self
            .selected_session()
            .and_then(|session| session.model.clone());
        let mode = self
            .selected_session()
            .map(|session| session.runtime_mode)
            .unwrap_or(self.state.last_runtime_mode);
        self.boss_request(
            key,
            BossOperation::Counsel {
                action: CounselOperation::Open {
                    provider,
                    model,
                    mode,
                },
            },
            BossReply::CounselOpen(self.state.selected_session),
            cx,
        );
    }

    pub(super) fn execute_counsel_composer_command(
        &mut self,
        prompt: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if prompt.trim() != "/counsel" {
            return false;
        }
        let Some(key) = self
            .boss_chat_key()
            .or_else(|| self.counsel_chat_key())
            .filter(|key| self.counsel_enabled_on(*key))
        else {
            return false;
        };
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.chat_with_counsel(key, cx);
        true
    }

    pub(super) fn render_counsel_sidebar_row(
        &self,
        key: DaemonKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.boss_ui.states.get(&key) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let selected = state
            .counsel
            .session_id
            .is_some_and(|id| self.state.selected_session == Some(id));
        let name = state
            .counsel
            .identity
            .as_ref()
            .map(|identity| identity.name.clone())
            .unwrap_or_else(|| tr!("counsel.title"));
        let subtitle = match key {
            DaemonKey::Local => tr!("counsel.subtitle"),
            DaemonKey::Remote(host) => format!(
                "{} · {}",
                tr!("counsel.subtitle"),
                self.remote_host_name(host).unwrap_or_default()
            ),
        };
        let indicator = state
            .counsel
            .session_id
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
            .and_then(|session| self.session_status_indicator(session, &theme));
        let face: AnyElement = state
            .counsel
            .identity
            .as_ref()
            .map(|identity| self.boss_avatar_animated(identity, 24.0, cx))
            .unwrap_or_else(|| icon("icons/compass.svg", 24.0, theme.text_secondary).into_any_element());
        div()
            .id(format!("counsel-{key:?}"))
            .tab_index(0)
            .h(px(42.0))
            .w_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme.sidebar_item_background))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .aria_label(name.clone())
            .on_activation(cx, move |this, _, cx| this.chat_with_counsel(key, cx))
            .child(face)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text_tertiary)
                            .child(subtitle),
                    ),
            )
            .when_some(indicator, |row, indicator| {
                row.child(Waku::sidebar_status_slot(indicator))
            })
            .into_any_element()
    }

    /// One recommendation card at its seat in Counsel history. Summary,
    /// reason, links, and the action row render immediately; the exact Boss
    /// message sits behind the collapsed "Message to Boss" disclosure, whose
    /// expanded state is exposed through `aria_expanded`. Approval binds the
    /// displayed revision whether or not the disclosure is open.
    pub(super) fn render_counsel_recommendation_row(
        &self,
        session_id: Uuid,
        id: Uuid,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = window;
        let Some((key, record)) = self.boss_ui.states.iter().find_map(|(key, state)| {
            (state.counsel.session_id == Some(session_id)).then(|| {
                (
                    *key,
                    state
                        .counsel
                        .recommendations
                        .iter()
                        .find(|record| record.id == id)
                        .cloned(),
                )
            })
        }) else {
            return div().into_any_element();
        };
        let Some(record) = record else {
            return div().into_any_element();
        };
        let revision = record.revision;
        let expanded = self.boss_ui.counsel_message_expanded.contains(&id);
        // Only a live, unresolved revision may be approved — obsolete and
        // resolved records render their status without action controls.
        let actionable = matches!(
            record.status,
            CounselStatus::Proposed | CounselStatus::Later
        );
        let label = match record.status {
            CounselStatus::Proposed => tr!("counsel.recommendation"),
            CounselStatus::Later => tr!("counsel.saved_later"),
            CounselStatus::Dismissed => tr!("counsel.dismissed"),
            CounselStatus::Approved => tr!("counsel.approved"),
            CounselStatus::DeliveryPending => tr!("counsel.delivery_pending"),
            CounselStatus::DeliveryFailed => tr!("counsel.delivery_failed"),
            CounselStatus::Sent => tr!("counsel.sent"),
            CounselStatus::Completed => tr!("counsel.completed"),
        };
        let pending_request = self.boss_ui.pending;
        let mut controls = div().flex().flex_wrap().gap(px(8.0));
        if actionable {
            for (name, label, decision) in [
                ("approve", tr!("counsel.approve"), CounselDecision::Approve),
                ("later", tr!("counsel.later"), CounselDecision::Later),
                ("dismiss", tr!("counsel.dismiss"), CounselDecision::Dismiss),
            ] {
                controls = controls.child(
                    div()
                        .id(format!("counsel-{name}-{id}"))
                        .tab_index(0)
                        .px(px(9.0))
                        .py(px(5.0))
                        .rounded(px(6.0))
                        .text_size(sp(12.0))
                        .text_color(theme.text)
                        .bg(theme.overlay)
                        .aria_label(label.clone())
                        .child(label)
                        .when(pending_request, |button| button.opacity(0.5))
                        .when(!pending_request, |button| {
                            button
                                .cursor_pointer()
                                .hover(|style| style.bg(theme.overlay_strong))
                                .focus_visible(|style| style.bg(theme.focus_highlight()))
                                .on_activation(cx, move |this, _, cx| {
                                    this.boss_request(
                                        key,
                                        BossOperation::Counsel {
                                            action: CounselOperation::Decide {
                                                id,
                                                expected_revision: revision,
                                                decision,
                                            },
                                        },
                                        BossReply::CounselDecision,
                                        cx,
                                    )
                                })
                        }),
                );
            }
            let title = record.title.clone();
            controls = controls.child(
                div()
                    .id(format!("counsel-revise-{id}"))
                    .tab_index(0)
                    .px(px(9.0))
                    .py(px(5.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(sp(12.0))
                    .text_color(theme.text)
                    .bg(theme.overlay)
                    .hover(|style| style.bg(theme.overlay_strong))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(tr!("counsel.revise"))
                    .on_activation(cx, move |this, window, cx| {
                        let existing = this.composer.read(cx).content(cx).to_string();
                        let text = format!(
                            "{}\n\n{}",
                            tr!(
                                "counsel.revise_prompt",
                                title = title.clone(),
                                id = id.to_string()
                            ),
                            existing
                        );
                        this.composer
                            .update(cx, |input, cx| input.set_content(text, cx));
                        window.focus(&this.composer_focus(cx), cx);
                    }),
            );
        }
        let mut card = div()
            .id(format!("counsel-card-{id}"))
            .w_full()
            .p(px(12.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.surface)
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(sp(11.0))
                    .text_color(theme.text_secondary)
                    .child(label),
            )
            .child(
                div()
                    .text_size(sp(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(record.title.clone()),
            )
            .child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(record.reason.clone()),
            );
        // "Message to Boss" — collapsed by default, keyboard-operable, and
        // the expanded state is exposed to assistive tech. Approval stays
        // bound to the exact message whether or not it is shown.
        let chevron = if expanded {
            "icons/chevron-down.svg"
        } else {
            "icons/chevron-right.svg"
        };
        card = card.child(
            div()
                .id(format!("counsel-message-toggle-{id}"))
                .tab_index(0)
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded(px(4.0))
                .cursor_pointer()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .aria_label(tr!("counsel.message_to_boss"))
                .aria_expanded(expanded)
                .on_activation(cx, move |this, _, cx| {
                    if !this.boss_ui.counsel_message_expanded.remove(&id) {
                        this.boss_ui.counsel_message_expanded.insert(id);
                    }
                    let row = super::transcript::TranscriptRowKind::CounselRecommendation(id);
                    this.remeasure_transcript_row(row);
                    this.remeasure_big_picture_row(session_id, row);
                    cx.notify();
                })
                .child(icon(chevron, 12.0, theme.text_secondary))
                .child(
                    div()
                        .text_size(sp(11.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("counsel.message_to_boss")),
                ),
        );
        if expanded {
            card = card.child(
                div()
                    .id(format!("counsel-message-body-{id}"))
                    .text_size(sp(12.0))
                    .text_color(theme.text)
                    .child(record.instruction.clone()),
            );
        }
        let mut links = div().flex().flex_wrap().gap(px(8.0));
        for source in &record.sources {
            let source_id = source.session_id;
            let title = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == source_id)
                .map(|session| session.display_title().to_owned())
                .unwrap_or_else(|| tr!("counsel.source"));
            links = links.child(
                div()
                    .id(format!("counsel-source-{id}-{source_id}"))
                    .tab_index(0)
                    .cursor_pointer()
                    .text_size(sp(11.0))
                    .text_color(theme.accent)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(title)
                    .on_activation(cx, move |this, _, cx| {
                        this.request_session_activation(
                            source_id,
                            SessionActivationTransition::Inspect,
                            cx,
                        );
                    }),
            );
        }
        card = card.child(links);
        if let Some(error) = &record.delivery_error {
            card = card.child(
                div()
                    .text_size(sp(11.0))
                    .text_color(theme.danger)
                    .child(format!("{}: {error}", tr!("counsel.delivery_failed"))),
            );
        }
        if record.boss_session_id.is_some() {
            controls = controls.child(
                div()
                    .id(format!("counsel-open-boss-{id}"))
                    .tab_index(0)
                    .cursor_pointer()
                    .px(px(9.0))
                    .py(px(5.0))
                    .text_size(sp(12.0))
                    .text_color(theme.accent)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(tr!("counsel.open_boss"))
                    .on_activation(cx, move |this, _, cx| {
                        this.chat_with_boss(key, cx);
                    }),
            );
        }
        if let Some(evidence) = &record.completion_evidence {
            card = card.child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(evidence.clone()),
            );
        }
        div()
            .px(px(20.0))
            .py(px(4.0))
            .child(card.child(controls))
            .into_any_element()
    }
}
