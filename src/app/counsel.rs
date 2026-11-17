use super::boss::BossReply;
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_protocol::boss::BossOperation;
use waku_protocol::counsel::{CounselDecision, CounselOperation, CounselStatus};

impl Waku {
    pub(super) fn counsel_chat_key(&self) -> Option<DaemonKey> {
        let id = self.composer_session_id()?;
        self.boss_ui
            .states
            .iter()
            .find_map(|(key, state)| (state.counsel.session_id == Some(id)).then_some(*key))
    }

    pub(super) fn chat_with_counsel(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        if !self.state.boss_experiment_enabled {
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
        if prompt.trim() != "/counsel" || !self.state.boss_experiment_enabled {
            return false;
        }
        let Some(key) = self.boss_chat_key().or_else(|| self.counsel_chat_key()) else {
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
            .aria_label(tr!("counsel.title"))
            .on_activation(cx, move |this, _, cx| this.chat_with_counsel(key, cx))
            .child(icon("icons/compass.svg", 24.0, theme.text_secondary))
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
                            .child(tr!("counsel.title")),
                    )
                    .child(
                        div()
                            .text_size(sp(11.0))
                            .text_color(theme.text_secondary)
                            .child(subtitle),
                    ),
            )
            .when_some(indicator, |row, indicator| {
                row.child(Waku::sidebar_status_slot(indicator))
            })
            .into_any_element()
    }

    /// Rebuild only when a Boss snapshot changes. Frames render at most
    /// three cached cards, independent of the accumulated Counsel history.
    pub(super) fn refresh_counsel_recommendations(&mut self, key: DaemonKey) {
        let Some(state) = self.boss_ui.states.get(&key) else {
            self.boss_ui.counsel_visible.remove(&key);
            return;
        };
        let recommendations = &state.counsel.recommendations;
        let mut records: Vec<_> = recommendations
            .iter()
            .filter(|record| record.status == CounselStatus::Proposed)
            .take(3)
            .cloned()
            .collect();
        if records.is_empty() {
            let receipt = recommendations
                .iter()
                .rev()
                .find(|record| record.status == CounselStatus::Later)
                .or_else(|| {
                    recommendations.iter().rev().find(|record| {
                        matches!(
                            record.status,
                            CounselStatus::Sent | CounselStatus::Completed
                        )
                    })
                });
            if let Some(record) = receipt {
                records.push(record.clone());
            }
        }
        self.boss_ui.counsel_visible.insert(key, records);
    }

    pub(super) fn render_counsel_recommendations(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let key = self.counsel_chat_key()?;
        let records = self.boss_ui.counsel_visible.get(&key)?;
        let theme = Theme::current(cx);
        if records.is_empty() {
            return None;
        }
        let pending_request = self.boss_ui.pending;
        Some(
            div()
                .px(px(20.0))
                .pb(px(8.0))
                .child(
                    div()
                        .id("counsel-recommendations")
                        .max_h(px(300.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .children(records.iter().map(|record| {
                            let id = record.id;
                            let revision = record.revision;
                            let actionable = matches!(
                                record.status,
                                CounselStatus::Proposed | CounselStatus::Later
                            );
                            let label = match record.status {
                                CounselStatus::Proposed => tr!("counsel.recommendation"),
                                CounselStatus::Later => tr!("counsel.saved_later"),
                                CounselStatus::Dismissed => tr!("counsel.dismissed"),
                                CounselStatus::Sent => tr!("counsel.sent"),
                                CounselStatus::Completed => tr!("counsel.completed"),
                            };
                            let mut controls = div().flex().flex_wrap().gap(px(8.0));
                            if actionable {
                                for (name, label, decision) in [
                                    ("send", tr!("counsel.send"), CounselDecision::Send),
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
                                                    .focus_visible(|style| {
                                                        style.bg(theme.focus_highlight())
                                                    })
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
                                            let existing =
                                                this.composer.read(cx).content(cx).to_string();
                                            let text = format!(
                                                "{}\n\n{}",
                                                tr!(
                                                    "counsel.revise_prompt",
                                                    title = title.clone(),
                                                    id = id.to_string()
                                                ),
                                                existing
                                            );
                                            this.composer.update(cx, |input, cx| {
                                                input.set_content(text, cx)
                                            });
                                            window.focus(&this.composer_focus(cx), cx);
                                        }),
                                );
                            }
                            let mut card = div()
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
                                )
                                .child(
                                    div()
                                        .text_size(sp(11.0))
                                        .text_color(theme.text_secondary)
                                        .child(tr!("counsel.message_to_boss")),
                                )
                                .child(
                                    div()
                                        .text_size(sp(12.0))
                                        .text_color(theme.text)
                                        .child(record.instruction.clone()),
                                );
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
                            card.child(controls)
                        })),
                )
                .into_any_element(),
        )
    }
}
