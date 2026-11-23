use super::boss::BossReply;
use super::*;
use crate::ui::ActivationExt;
use gpui::{ElementId, Role};
use waku_client::DaemonKey;
use waku_protocol::boss::BossOperation;
use waku_protocol::counsel::{CounselDecision, CounselOperation, CounselStatus};

impl Waku {
    pub(super) fn counsel_chat_key(&self) -> Option<DaemonKey> {
        self.counsel_state_for_session(self.composer_session_id()?)
            .map(|(key, _)| key)
    }

    /// Match the current chat IDs so an employee or a retired chat never
    /// exposes a local reset for the conversation that replaced it.
    pub(super) fn managed_chat_clear_target(&self, id: Uuid) -> Option<(DaemonKey, bool)> {
        self.counsel_state_for_session(id)
            .map(|(key, _)| (key, true))
            .or_else(|| {
                self.boss_ui.states.iter().find_map(|(key, state)| {
                    (state.session_id == Some(id)).then_some((*key, false))
                })
            })
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
            .unwrap_or_else(|| {
                icon("icons/compass.svg", 24.0, theme.text_secondary).into_any_element()
            });
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
                    .child(div().text_size(sp(13.0)).text_color(theme.text).child(name))
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

    /// A recommendation stays anchored to its originating turn. The approval
    /// control only appears below the exact instruction for the open revision.
    pub(super) fn render_counsel_recommendation_row(
        &self,
        session_id: Uuid,
        id: Uuid,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((key, state)) = self.counsel_state_for_session(session_id) else {
            return div().into_any_element();
        };
        let Some(record) = state.recommendations.iter().find(|record| record.id == id) else {
            return div().into_any_element();
        };
        let revision = record.revision;
        let expanded = self.boss_ui.counsel_review_expanded.get(&id) == Some(&revision);
        let actionable = record.status.is_outstanding();
        let historical = matches!(
            record.status,
            CounselStatus::Dismissed | CounselStatus::Completed
        );
        let status = match record.status {
            CounselStatus::Proposed => tr!("counsel.recommendation"),
            CounselStatus::Later => tr!("counsel.saved_later"),
            CounselStatus::Dismissed => tr!("counsel.dismissed"),
            CounselStatus::Approved => tr!("counsel.approved"),
            CounselStatus::DeliveryPending => tr!("counsel.delivery_pending"),
            CounselStatus::DeliveryFailed => tr!("counsel.delivery_failed"),
            CounselStatus::Sent => tr!("counsel.sent"),
            CounselStatus::Completed => tr!("counsel.completed"),
        };

        let disclosure_label = if historical {
            format!("{} · {}", record.title, status)
        } else if expanded {
            tr!("counsel.collapse")
        } else if actionable {
            tr!("counsel.review")
        } else {
            tr!("counsel.show_details")
        };
        let disclosure = counsel_card_button(
            format!("counsel-review-{id}"),
            disclosure_label.clone(),
            actionable && !expanded,
            theme,
        )
        .aria_expanded(expanded)
        .on_activation(cx, move |this, _, cx| {
            if expanded {
                this.boss_ui.counsel_review_expanded.remove(&id);
            } else {
                this.boss_ui.counsel_review_expanded.insert(id, revision);
            }
            let row = super::transcript::TranscriptRowKind::CounselRecommendation(id);
            if this.transcript_session_id() == Some(session_id) {
                this.remeasure_transcript_row(row);
            }
            this.remeasure_big_picture_row(session_id, row);
            cx.notify();
        });

        let mut card = div()
            .id(format!("counsel-card-{id}"))
            .w_full()
            .min_w_0()
            .px(px(12.0))
            .py(px(if historical { 4.0 } else { 10.0 }))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(
                if actionable || record.status == CounselStatus::DeliveryFailed {
                    theme.accent.opacity(0.5)
                } else {
                    theme.border_subtle
                },
            )
            .bg(theme.surface)
            .flex()
            .flex_col()
            .gap(px(8.0));
        let mut footer = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .pt(px(6.0))
            .border_t(hairline())
            .border_color(theme.border_subtle);

        if historical {
            card = card.child(
                disclosure
                    .w_full()
                    .px_0()
                    .child(icon(
                        if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        },
                        12.0,
                        theme.text_secondary,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(!expanded, |title| title.truncate())
                            .child(record.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.text_secondary)
                            .child(status),
                    ),
            );
        } else {
            let mut header = div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(sp(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child(status),
                );
            if actionable {
                let menu_id = format!("counsel-actions-{id}");
                let weak = cx.entity().downgrade();
                let pending = self.boss_ui.pending;
                let can_save = record.status == CounselStatus::Proposed;
                header = header.child(dropdown_menu(
                    MenuChip::new(format!("counsel-more-{id}"))
                        .label(tr!("counsel.more"))
                        .height(px(28.0)),
                    menu_id.clone(),
                    &self.menu_handle(menu_id, cx),
                    MenuAlign::BelowRight,
                    move |_| {
                        let item = |label, decision| {
                            let weak = weak.clone();
                            MenuItem::new(label, move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.decide_counsel_recommendation(
                                        key, id, revision, decision, cx,
                                    );
                                });
                            })
                            .disabled(pending)
                        };
                        let mut items = Vec::new();
                        if can_save {
                            items.push(item(tr!("counsel.later"), CounselDecision::Later));
                        }
                        items.push(item(tr!("counsel.dismiss"), CounselDecision::Dismiss));
                        items
                    },
                ));
            }
            card = card.child(header).child(
                div()
                    .text_size(sp(15.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(record.title.clone()),
            );
            if expanded && actionable {
                footer = footer
                    .justify_between()
                    .child(disclosure.child(disclosure_label));
            } else {
                footer = footer
                    .justify_end()
                    .child(disclosure.child(disclosure_label));
            }
        }

        if actionable || expanded {
            card = card.child(
                div()
                    .text_size(sp(13.0))
                    .text_color(theme.text_secondary)
                    .child(record.reason.clone()),
            );
        }

        if record.status == CounselStatus::Approved {
            card = card.child(
                div()
                    .text_size(sp(13.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("counsel.awaiting_delivery")),
            );
        }

        if record.status == CounselStatus::DeliveryFailed {
            if let Some(error) = &record.delivery_error {
                card = card.child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.danger)
                        .child(error.clone()),
                );
            }
            if let Some(approved_revision) = record.approved_revision {
                card = card.child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("counsel.approval_saved")),
                );
                let label = tr!("counsel.retry");
                footer = footer.child(
                    counsel_card_button(format!("counsel-retry-{id}"), label.clone(), true, theme)
                        .child(label)
                        .on_activation(cx, move |this, window, cx| {
                            let prompt = tr!(
                                "counsel.retry_prompt",
                                id = id.to_string(),
                                revision = approved_revision
                            );
                            this.stage_counsel_reply(session_id, &prompt, window, cx);
                        }),
                );
            }
        }

        if expanded {
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(theme.inset)
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child(tr!("counsel.message_to_boss")),
                    )
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text)
                            .child(record.instruction.clone()),
                    ),
            );
            if !record.sources.is_empty() {
                let mut sources = div().flex().flex_wrap().items_center().gap(px(5.0)).child(
                    div()
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("counsel.sources")),
                );
                for source in &record.sources {
                    let source_id = source.session_id;
                    let title = self
                        .state
                        .sessions
                        .iter()
                        .find(|session| session.id == source_id)
                        .map(|session| session.display_title().to_owned())
                        .unwrap_or_else(|| tr!("counsel.source"));
                    sources = sources.child(
                        counsel_card_button(
                            format!("counsel-source-{id}-{source_id}"),
                            title.clone(),
                            false,
                            theme,
                        )
                        .max_w_full()
                        .text_color(theme.accent)
                        .child(div().min_w_0().truncate().child(title))
                        .on_activation(cx, move |this, _, cx| {
                            this.request_session_activation(
                                source_id,
                                SessionActivationTransition::Inspect,
                                cx,
                            );
                        }),
                    );
                }
                card = card.child(sources);
            }
            if let Some(evidence) = &record.completion_evidence {
                card = card.child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.text_secondary)
                        .child(evidence.clone()),
                );
            }
        }

        if expanded && actionable {
            let revise = tr!("counsel.revise");
            let title = record.title.clone();
            let approve = tr!("counsel.approve");
            let pending = self.boss_ui.pending;
            footer = footer.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        counsel_card_button(
                            format!("counsel-revise-{id}"),
                            revise.clone(),
                            false,
                            theme,
                        )
                        .child(revise)
                        .on_activation(cx, move |this, window, cx| {
                            let prompt = tr!(
                                "counsel.revise_prompt",
                                title = title.clone(),
                                id = id.to_string()
                            );
                            this.stage_counsel_reply(session_id, &prompt, window, cx);
                        }),
                    )
                    .child(
                        counsel_card_button(
                            format!("counsel-approve-{id}"),
                            approve.clone(),
                            true,
                            theme,
                        )
                        .child(approve)
                        .when(pending, |button| button.opacity(0.5))
                        .on_activation(cx, move |this, _, cx| {
                            if this.boss_ui.counsel_review_expanded.get(&id) == Some(&revision) {
                                this.decide_counsel_recommendation(
                                    key,
                                    id,
                                    revision,
                                    CounselDecision::Approve,
                                    cx,
                                );
                            }
                        }),
                    ),
            );
        }
        let show_boss =
            record.boss_session_id.is_some() && (record.status == CounselStatus::Sent || expanded);
        if show_boss {
            let label = tr!("counsel.open_boss");
            footer = footer.child(
                counsel_card_button(
                    format!("counsel-open-boss-{id}"),
                    label.clone(),
                    false,
                    theme,
                )
                .text_color(theme.accent)
                .child(label)
                .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
            );
        }

        div()
            .px(px(20.0))
            .py(px(4.0))
            .child(card.when(!historical || show_boss, |card| card.child(footer)))
            .into_any_element()
    }

    fn decide_counsel_recommendation(
        &mut self,
        key: DaemonKey,
        id: Uuid,
        expected_revision: u64,
        decision: CounselDecision,
        cx: &mut Context<Self>,
    ) {
        if self.boss_ui.pending {
            return;
        }
        self.boss_request(
            key,
            BossOperation::Counsel {
                action: CounselOperation::Decide {
                    id,
                    expected_revision,
                    decision,
                },
            },
            BossReply::CounselDecision,
            cx,
        );
    }

    fn stage_counsel_reply(
        &mut self,
        session_id: Uuid,
        prompt: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture_open()
            && self.big_picture_card_visible(session_id)
            && self.composer_session_id() != Some(session_id)
        {
            self.arm_big_picture_card(session_id, cx);
        }
        let composer = if self.composer_session_id() == Some(session_id) {
            &self.composer
        } else if self.visible_side_chat_id() == Some(session_id)
            && let Some(chat) = self.side_chat_composers.get(&session_id)
        {
            &chat.composer
        } else {
            return;
        };
        let focus = composer.update(cx, |input, cx| {
            let existing = input.content(cx);
            if !existing.starts_with(prompt) {
                let prefix = format!("{prompt}\n\n");
                input.replace_range(0..0, &prefix, cx);
            }
            input.focus()
        });
        window.focus(&focus, cx);
    }
}

#[track_caller]
fn counsel_card_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(id)
        .tab_index(0)
        .role(Role::Button)
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_h(px(28.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .border(hairline())
        .border_color(if primary {
            theme.accent.opacity(0.55)
        } else {
            gpui::transparent_black()
        })
        .when(primary, |button| button.bg(theme.accent.opacity(0.14)))
        .cursor_pointer()
        .text_size(sp(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text)
        .hover(move |style| {
            style.bg(if primary {
                theme.accent.opacity(0.22)
            } else {
                theme.overlay
            })
        })
        .focus_visible(move |style| style.bg(theme.focus_highlight()))
        .aria_label(label)
}
