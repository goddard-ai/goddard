use super::annotations::{
    annotation_bubble_content, annotation_display_content, annotation_prompt_prefix,
};
use super::*;
use crate::ui::ActivationExt;

use anyhow::Context as _;
use base64::Engine as _;
use gpui::AnyView;

/// Group on the session column's hitbox: the composer card reads it through
/// `group_drag_over` so it lights up wherever over the column an OS file drag
/// is held, and the column itself accepts the drop for the same staging.
pub(super) const SESSION_DROP_GROUP: &str = "session-file-drop";

/// A collapsed text paste past this size stops being a composer chip and is
/// stored as a durable `.txt` blob instead — a real file attachment the agent
/// opens itself, rather than a block of bytes folded into every draft sync.
const PASTED_TEXT_FILE_BYTES: usize = 64 * 1024;

/// How much of a paste its chip's hover preview shows.
const PASTED_TEXT_PREVIEW_CHARS: usize = 200;

const COMPUTER_USE_PREVIEW_WIDTH: f32 = 304.0;
const COMPUTER_USE_PREVIEW_HEIGHT: f32 = 172.0;
const COMPUTER_USE_PREVIEW_RADIUS: f32 = 15.0;
const COMPUTER_USE_PREVIEW_INNER_RADIUS: f32 = COMPUTER_USE_PREVIEW_RADIUS - 1.0;

struct ComputerUsePreviewDrag {
    cursor_offset: Cell<gpui::Point<Pixels>>,
}

/// A session row dragged out of the sidebar. Dropping it anywhere the
/// composer is reachable stages a session-reference chip — `title` rides
/// along so the drag preview and the chip never re-lookup the session.
#[derive(Clone)]
pub(super) struct SidebarSessionDrag {
    pub(super) session_id: Uuid,
    pub(super) title: SharedString,
}

/// The view GPUI drags under the cursor for a [`SidebarSessionDrag`]: the
/// same chip the drop stages, minus its remove affordance.
pub(super) struct SidebarSessionDragView {
    pub(super) title: SharedString,
}

impl gpui::Render for SidebarSessionDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div()
            .h(px(24.0))
            .pl(px(6.0))
            .pr(px(10.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.composer)
            .flex()
            .items_center()
            .gap(px(5.0))
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .child(icon("icons/chat.svg", 11.0, theme.text_tertiary))
            .child(self.title.clone())
    }
}

/// A starred model selection dragged within the picker to reorder the
/// favorites section. `index` is its position in `state.favorite_models`.
#[derive(Clone)]
pub(super) struct FavoriteModelDrag {
    pub index: usize,
    pub label: SharedString,
}

/// The view GPUI drags under the cursor for a [`FavoriteModelDrag`]: the
/// starred row's model name as a chip.
pub(super) struct FavoriteModelDragView {
    pub label: SharedString,
}

impl gpui::Render for FavoriteModelDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div()
            .h(px(24.0))
            .pl(px(6.0))
            .pr(px(10.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.composer)
            .flex()
            .items_center()
            .gap(px(5.0))
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .child(icon("icons/star-filled.svg", 11.0, theme.favorite))
            .child(self.label.clone())
    }
}

/// The hover card behind every "Pasted text" chip: the paste's leading
/// characters wrapped over the raised surface.
struct PastedTextPreview {
    preview: SharedString,
}

impl Render for PastedTextPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div().pt(px(4.0)).pl(px(2.0)).child(
            div()
                .max_w(px(340.0))
                .px(px(7.0))
                .py(px(5.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .bg(theme.raised)
                .shadow_md()
                .text_size(sp(12.0))
                .line_height(sp(16.0))
                .text_color(theme.text_secondary)
                .child(self.preview.clone()),
        )
    }
}

/// The provider-facing token a session chip contributes to the prompt: the
/// task id `goddard-agent prompt` addresses, with the title for legibility.
fn session_attachment_token(attachment: &MessageAttachment) -> Option<String> {
    attachment
        .session_id
        .map(|session_id| format!("[session \"{}\" (task_id: {session_id})]", attachment.name))
}

fn clamp_computer_use_preview_position(
    position: gpui::Point<Pixels>,
    size: gpui::Size<Pixels>,
    window: &Window,
) -> gpui::Point<Pixels> {
    let inset = window.client_inset().unwrap_or_default() + px(8.0);
    let viewport = window.viewport_size();
    point(
        position
            .x
            .clamp(inset, (viewport.width - size.width - inset).max(inset)),
        position
            .y
            .clamp(inset, (viewport.height - size.height - inset).max(inset)),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ComposerSubmitAction {
    Send,
    Preparing,
    Stop,
    /// The session's last turn ended unsettled and the composer is empty —
    /// the submit affordance continues that work with no prompt required.
    Continue,
}

/// Which session and project the composer workspace controls act on.
/// Outside Big Picture that's the selection; inside it, the armed card, or —
/// while nothing is armed — the standing new-task destination project and
/// its unstarted draft session when one exists.
pub(super) fn workspace_subject_for(
    big_picture_open: bool,
    target_session: Option<Uuid>,
    selected_session: Option<Uuid>,
    selected_project: Option<Uuid>,
    new_task_project: Option<Uuid>,
    sessions: &[AgentSession],
) -> (Option<Uuid>, Option<Uuid>) {
    if !big_picture_open {
        return (selected_session, selected_project);
    }
    if let Some(session_id) = target_session {
        let project_id = sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.project_id);
        return (Some(session_id), project_id);
    }
    let project_id = new_task_project;
    let session_id = sessions
        .iter()
        .find(|session| Some(session.project_id) == project_id && !session.has_started())
        .map(|session| session.id);
    (session_id, project_id)
}

/// Whether an idle session's last turn ended unsettled: explicit Stop, an app
/// quit mid-turn, and orphaned-runtime recovery settle the turn as
/// `Interrupted`, while a provider error or a runtime dying with its daemon
/// settles it as `Failed` — either way the work stopped mid-turn and is
/// resumable, so both qualify. A draft in the composer still wins —
/// see [`composer_submit_action`].
pub(super) fn session_awaits_continue(session: &AgentSession) -> bool {
    !session.status.is_busy()
        && session
            .turns
            .last()
            .is_some_and(|turn| matches!(turn.status, TurnStatus::Interrupted | TurnStatus::Failed))
}

pub(super) fn composer_submit_action(
    session: Option<&AgentSession>,
    preparing: bool,
    has_draft: bool,
) -> ComposerSubmitAction {
    if preparing {
        ComposerSubmitAction::Preparing
    } else if session.is_some_and(|session| session.status.is_busy()) {
        ComposerSubmitAction::Stop
    } else if !has_draft && session.is_some_and(session_awaits_continue) {
        ComposerSubmitAction::Continue
    } else {
        ComposerSubmitAction::Send
    }
}

impl Waku {
    // ── Permission ─────────────────────────────────────────────────────────

    pub(super) fn render_permission(&self, cx: &mut Context<Self>) -> Option<Div> {
        if let Some(input) = self.selected_runtime()?.pending_user_input.clone() {
            return Some(self.render_user_input(input, cx));
        }
        if let Some(permission) = self.selected_runtime()?.pending_computer_approval.as_ref() {
            return Some(self.render_computer_permission(permission, cx));
        }
        let permission = self.selected_runtime()?.pending_permission.as_ref()?;
        let theme = Theme::current(cx);
        let request_id = permission.request_id.clone();
        // Escape answers the request with the first deny option — a real
        // response, not a hide, so the waiting turn settles instead of
        // hanging on a card that is no longer visible.
        let deny_option = permission
            .options
            .iter()
            .find(|option| !option.allow)
            .map(|option| option.id.clone());
        let mut buttons = div().flex().items_center().gap(px(8.0)).mt(px(10.0));
        for option in &permission.options {
            let request_id = request_id.clone();
            let option_id = option.id.clone();
            let allow = option.allow;
            let focus = self.transcript_control_focus(
                format!("permission-{}-{}", permission.request_id, option.id),
                cx,
            );
            buttons = buttons.child(
                div()
                    .id(SharedString::from(format!(
                        "permission-{}-{}",
                        permission.request_id, option.id
                    )))
                    .track_focus(&focus)
                    .tab_index(0)
                    .h(px(28.0))
                    .px(px(13.0))
                    .rounded(px(9.0))
                    .border(hairline())
                    .border_color(if allow {
                        theme.inverse
                    } else {
                        theme.border_strong
                    })
                    .flex()
                    .items_center()
                    .cursor_default()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(allow, |element| {
                        element
                            .bg(theme.inverse)
                            .text_color(theme.on_inverse)
                            .hover(|element| element.opacity(0.9))
                    })
                    .when(!allow, |element| {
                        element
                            .text_color(theme.text_secondary)
                            .hover(|element| element.bg(theme.overlay).text_color(theme.text))
                    })
                    .active(|element| element.opacity(0.8))
                    .focus_visible(|style| style.border_color(theme.accent))
                    .child(SharedString::from(
                        option
                            .label_i18n
                            .as_ref()
                            .map(waku_client::WireTranslation::render)
                            .unwrap_or_else(|| option.label.clone()),
                    ))
                    .on_activation(cx, move |this, _, cx| {
                        this.respond_permission(request_id.clone(), option_id.clone(), cx);
                    }),
            );
        }
        let deny_request_id = request_id.clone();
        Some(
            div().px(px(20.0)).pb(px(8.0)).child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH))
                    .mx_auto()
                    .p(px(12.0))
                    .rounded(px(15.0))
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.raised)
                    .shadow_md()
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape"
                            && let Some(option_id) = deny_option.clone()
                        {
                            this.respond_permission(deny_request_id.clone(), option_id, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("icons/alert.svg", 13.0, theme.warning))
                            .child(
                                div()
                                    .text_size(sp(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(SharedString::from(
                                            permission
                                                .title_i18n
                                                .as_ref()
                                                .map(waku_client::WireTranslation::render)
                                                .unwrap_or_else(|| permission.title.clone()),
                                        )),
                            ),
                    )
                    .child(
                        div()
                            .id("permission-detail")
                            .mt(px(8.0))
                            .max_h(px(92.0))
                            .overflow_y_scroll()
                            .p(px(8.0))
                            .rounded(px(9.0))
                            .bg(theme.inset)
                            .font_family(crate::fonts::current(cx).code)
                            .text_size(sp(12.5))
                            .line_height(sp(16.0))
                            .text_color(theme.text_secondary)
                            .whitespace_normal()
                            .child(SharedString::from(
                                permission
                                    .detail_i18n
                                    .as_ref()
                                    .map(waku_client::WireTranslation::render)
                                    .unwrap_or_else(|| permission.detail.clone()),
                            )),
                    )
                    .child(buttons),
            ),
        )
    }

    fn render_user_input(&self, pending: PendingUserInput, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let Some(question) = pending.current_question().cloned() else {
            return div();
        };
        let selected = pending
            .selections
            .get(&question.id)
            .cloned()
            .unwrap_or_default();
        let has_custom = pending
            .custom_answers
            .get(&question.id)
            .is_some_and(|answer| !answer.trim().is_empty());
        let can_continue = has_custom || !selected.is_empty();
        let is_last = pending.question_index + 1 == pending.questions.len();
        let request_id = pending.request_id.clone();
        let question_index = pending.question_index;
        let mut options = div().mt(px(9.0)).flex().flex_col().gap(px(4.0));
        for (index, option) in question.options.iter().enumerate() {
            let is_selected = selected.iter().any(|answer| answer == &option.label);
            let click_label = option.label.clone();
            let key_label = option.label.clone();
            let focus = self.transcript_control_focus(
                format!("user-input-{request_id}-{question_index}-option-{index}"),
                cx,
            );
            options = options.child(
                div()
                    .id(SharedString::from(format!(
                        "user-input-{request_id}-{question_index}-option-{index}"
                    )))
                    .track_focus(&focus)
                    .tab_index(0)
                    .tab_stop(true)
                    .min_h(px(36.0))
                    .px(px(10.0))
                    .py(px(5.0))
                    .rounded(px(10.0))
                    .border(hairline())
                    .border_color(if is_selected {
                        theme.accent.opacity(0.34)
                    } else {
                        theme.border.opacity(0.0)
                    })
                    .bg(if is_selected {
                        theme.accent.opacity(0.08)
                    } else {
                        theme.overlay
                    })
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .cursor_default()
                    .focus_visible(|style| style.border_color(theme.accent))
                    .when(!is_selected, |row| {
                        row.hover(|style| style.border_color(theme.border).bg(theme.overlay_strong))
                    })
                    .active(|style| style.opacity(0.85))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(SharedString::from(option.label.clone())),
                            )
                            .children(option.description.as_ref().map(|description| {
                                div()
                                    .mt(px(1.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(15.0))
                                    .text_color(theme.text_secondary)
                                    .whitespace_normal()
                                    .child(SharedString::from(description.clone()))
                            })),
                    )
                    .when(is_selected, |row| {
                        row.child(icon("icons/check.svg", 12.0, theme.accent))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_user_input_option(click_label.clone(), cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.select_user_input_option(key_label.clone(), cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }

        let next_focus = self.transcript_control_focus(
            format!("user-input-{request_id}-{question_index}-continue"),
            cx,
        );
        let supports_actions = self
            .selected_runtime()
            .is_some_and(|runtime| runtime.driver.supports_user_input_actions());
        let dismiss = supports_actions.then(|| {
            let focus = self.transcript_control_focus(
                format!("user-input-{request_id}-{question_index}-dismiss"),
                cx,
            );
            div()
                .id(SharedString::from(format!(
                    "user-input-{request_id}-{question_index}-dismiss"
                )))
                .track_focus(&focus)
                .tab_index(0)
                .tab_stop(true)
                .h(px(26.0))
                .px(px(8.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary)
                .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                .hover(|style| style.bg(theme.overlay).text_color(theme.text_secondary))
                .active(|style| style.opacity(0.8))
                .child(tr!("user_input.dismiss"))
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_user_input(cx)))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.dismiss_user_input(cx);
                        cx.stop_propagation();
                    }
                }))
        });
        let clarify = supports_actions.then(|| {
            let focus = self.transcript_control_focus(
                format!("user-input-{request_id}-{question_index}-clarify"),
                cx,
            );
            div()
                .id(SharedString::from(format!(
                    "user-input-{request_id}-{question_index}-clarify"
                )))
                .track_focus(&focus)
                .tab_index(0)
                .tab_stop(has_custom)
                .h(px(26.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(if has_custom {
                    theme.border_strong
                } else {
                    theme.border
                })
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if has_custom {
                    theme.text
                } else {
                    theme.text_ghost
                })
                .when(has_custom, |button| {
                    button
                        .focus_visible(|style| style.border_color(theme.accent))
                        .hover(|style| style.bg(theme.overlay))
                        .active(|style| style.opacity(0.8))
                        .on_click(cx.listener(|this, _, _, cx| this.clarify_user_input(cx)))
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.clarify_user_input(cx);
                                cx.stop_propagation();
                            }
                        }))
                })
                .child(tr!("user_input.clarify"))
        });
        let back = (question_index > 0).then(|| {
            let focus = self.transcript_control_focus(
                format!("user-input-{request_id}-{question_index}-back"),
                cx,
            );
            div()
                .id(SharedString::from(format!(
                    "user-input-{request_id}-{question_index}-back"
                )))
                .track_focus(&focus)
                .tab_index(0)
                .tab_stop(true)
                .h(px(26.0))
                .px(px(8.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .cursor_default()
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary)
                .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                .hover(|style| style.bg(theme.overlay).text_color(theme.text_secondary))
                .active(|style| style.opacity(0.8))
                .child(tr!("user_input.back"))
                .on_click(cx.listener(|this, _, _, cx| this.previous_user_input(cx)))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.previous_user_input(cx);
                        cx.stop_propagation();
                    }
                }))
        });
        let continue_button = div()
            .id(SharedString::from(format!(
                "user-input-{request_id}-{question_index}-continue"
            )))
            .track_focus(&next_focus)
            .tab_index(0)
            .tab_stop(can_continue)
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .bg(if can_continue {
                theme.inverse
            } else {
                theme.overlay
            })
            .text_color(if can_continue {
                theme.on_inverse
            } else {
                theme.text_ghost
            })
            .when(can_continue, |button| {
                button
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .hover(|style| style.opacity(0.9))
                    .active(|style| style.opacity(0.8))
                    .on_click(cx.listener(|this, _, _, cx| this.advance_user_input(cx)))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.advance_user_input(cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(if is_last {
                tr!("user_input.submit")
            } else {
                tr!("user_input.next")
            });

        let progress = (pending.questions.len() > 1).then(|| {
            div()
                .h(px(18.0))
                .px(px(6.0))
                .rounded(px(5.0))
                .bg(theme.overlay)
                .flex()
                .items_center()
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary)
                .child(tr!(
                    "user_input.progress",
                    current = question_index + 1,
                    total = pending.questions.len()
                ))
        });

        div().flex_none().px(px(20.0)).pb(px(8.0)).child(
            div()
                .id(SharedString::from(format!("user-input-{request_id}")))
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .mx_auto()
                .px(px(14.0))
                .pt(px(12.0))
                .pb(px(10.0))
                .rounded(px(16.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.composer)
                .tab_index(0)
                .tab_group()
                .tab_stop(false)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_tertiary)
                                .child(SharedString::from(question.header.clone())),
                        )
                        .children(progress),
                )
                .child(
                    div()
                        .mt(px(5.0))
                        .text_size(sp(13.0))
                        .line_height(sp(18.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .whitespace_normal()
                        .child(SharedString::from(question.question.clone())),
                )
                .children((!question.options.is_empty()).then_some(options))
                .child(
                    div()
                        .mt(px(if question.options.is_empty() {
                            9.0
                        } else {
                            4.0
                        }))
                        .h(px(34.0))
                        .px(px(10.0))
                        .rounded(px(10.0))
                        .border(hairline())
                        .border_color(if has_custom {
                            theme.accent.opacity(0.34)
                        } else {
                            theme.border.opacity(0.0)
                        })
                        .bg(if has_custom {
                            theme.accent.opacity(0.06)
                        } else {
                            theme.overlay
                        })
                        .flex()
                        .items_center()
                        .gap(px(7.0))
                        .text_size(sp(12.5))
                        .line_height(sp(16.0))
                        .child(icon(
                            "icons/pencil.svg",
                            11.0,
                            if has_custom {
                                theme.accent
                            } else {
                                theme.text_ghost
                            },
                        ))
                        .child(self.user_input_answer.clone()),
                )
                .child(
                    div()
                        .mt(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .children(back)
                        .children(dismiss)
                        .child(div().flex_1())
                        .children(clarify)
                        .child(continue_button),
                ),
        )
    }

    fn render_computer_permission(
        &self,
        permission: &PendingComputerApproval,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let target = &permission.target;
        let mut buttons = div().mt(px(12.0)).flex().items_center().gap(px(8.0));
        let mut options = vec![
            ("task", tr!("computer_use.allow_for_task"), true),
            ("deny", tr!("common.deny"), false),
        ];
        if target.persistable() {
            options.insert(1, ("always", tr!("computer_use.always_allow_app"), false));
        }
        for (decision, label, primary) in options {
            let focus = self.transcript_control_focus(
                format!(
                    "computer-permission-{}-{decision}",
                    permission.request.call_id
                ),
                cx,
            );
            buttons = buttons.child(
                div()
                    .id(SharedString::from(format!(
                        "computer-permission-{}-{decision}",
                        permission.request.call_id
                    )))
                    .track_focus(&focus)
                    .tab_index(0)
                    .h(px(29.0))
                    .px(px(13.0))
                    .rounded(px(9.0))
                    .border(hairline())
                    .border_color(if primary {
                        theme.inverse
                    } else {
                        theme.border_strong
                    })
                    .flex()
                    .items_center()
                    .cursor_default()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(primary, |element| {
                        element
                            .bg(theme.inverse)
                            .text_color(theme.on_inverse)
                            .hover(|element| element.opacity(0.9))
                    })
                    .when(!primary, |element| {
                        element
                            .text_color(theme.text_secondary)
                            .hover(|element| element.bg(theme.overlay).text_color(theme.text))
                    })
                    .active(|element| element.opacity(0.8))
                    .focus_visible(|style| style.border_color(theme.accent))
                    .child(label)
                    .on_activation(cx, move |this, _, cx| {
                        this.respond_computer_permission(decision, cx);
                    }),
            );
        }

        div().px(px(20.0)).pb(px(8.0)).child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .mx_auto()
                .p(px(13.0))
                .rounded(px(15.0))
                .border(hairline())
                .border_color(theme.warning.opacity(0.5))
                .bg(theme.raised)
                .shadow_md()
                // Escape answers "deny" — a real rejection, not a hide, so
                // the waiting tool call settles instead of hanging.
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        this.respond_computer_permission("deny", cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(9.0))
                        .child(icon("icons/globe.svg", 14.0, theme.warning))
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(tr!("computer_use.allow_control", app = &target.app_name)),
                        ),
                )
                .child(
                    div()
                        .mt(px(7.0))
                        .text_size(sp(12.5))
                        .line_height(sp(14.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("computer_use.screenshot_shared")),
                )
                .child(
                    div()
                        .mt(px(8.0))
                        .p(px(9.0))
                        .rounded(px(10.0))
                        .bg(theme.inset)
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .truncate()
                                .child(SharedString::from(target.window_title.clone())),
                        )
                        .child(
                            div()
                                .mt(px(4.0))
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(SharedString::from(permission.request.summary())),
                        )
                        .when(permission.sensitive, |element| {
                            element.child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(sp(12.5))
                                    .text_color(theme.warning)
                                    .child(tr!("computer_use.sensitive_action")),
                            )
                        }),
                )
                .child(
                    div()
                        .mt(px(7.0))
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(if target.persistable() {
                            tr!("computer_use.bundle_id", id = &target.bundle_id)
                        } else {
                            tr!("computer_use.no_bundle_id")
                        }),
                )
                .child(buttons),
        )
    }

    pub(super) fn render_computer_use_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let previews = self
            .selected_runtime()?
            .computer_use_previews
            .iter()
            .filter(|state| {
                state.visible
                    && state.target.is_some()
                    && state.phase != ComputerUsePhase::AwaitingApproval
            })
            .collect::<Vec<_>>();
        if previews.is_empty() {
            return None;
        }
        let theme = Theme::current(cx);
        let stack_x_offset = 14.0;
        let stack_y_offset = 24.0;
        let deepest_x_offset = (previews.len().saturating_sub(1) as f32) * stack_x_offset;
        let deepest_y_offset = (previews.len().saturating_sub(1) as f32) * stack_y_offset;
        let stack_size = gpui::size(
            px(COMPUTER_USE_PREVIEW_WIDTH + deepest_x_offset),
            px(COMPUTER_USE_PREVIEW_HEIGHT + deepest_y_offset),
        );
        let position = clamp_computer_use_preview_position(
            self.computer_use_preview_position.unwrap_or_else(|| {
                let viewport = window.viewport_size();
                point(
                    viewport.width - px(self.right_panel_rendered_width + 16.0) - stack_size.width,
                    viewport.height - px(82.0) - stack_size.height,
                )
            }),
            stack_size,
            window,
        );
        let top_index = previews.len() - 1;
        let cards = previews
            .into_iter()
            .enumerate()
            .filter_map(|(index, state)| {
                let target = state.target.as_ref()?;
                let window_id = target.window_id;
                let app_name = target.app_name.clone();
                let screenshot = state
                    .frames
                    .current
                    .as_ref()
                    .map(|frame| frame.image.clone());
                let is_top = index == top_index;
                let depth = (top_index - index) as f32;
                let x_offset = depth * stack_x_offset;
                let y_offset = depth * stack_y_offset;
                let card_offset = point(
                    px(deepest_x_offset - x_offset),
                    px(deepest_y_offset - y_offset),
                );
                let focus =
                    self.transcript_control_focus(format!("computer-use-preview-{window_id}"), cx);
                let mouse_focus = focus.clone();
                let close_focus = self.transcript_control_focus(
                    format!("computer-use-preview-close-{window_id}"),
                    cx,
                );
                let group_name = SharedString::from(format!("computer-use-preview-{window_id}"));
                let keyboard_controls = window.last_input_was_keyboard()
                    && (focus.contains_focused(window, cx) || close_focus.is_focused(window));

                Some(
                    div()
                        .id(SharedString::from(format!(
                            "computer-use-preview-{window_id}"
                        )))
                        .track_focus(&focus)
                        .tab_index(0)
                        .tab_stop(true)
                        .group(group_name.clone())
                        .absolute()
                        .right(px(x_offset))
                        .bottom(px(y_offset))
                        .w(px(COMPUTER_USE_PREVIEW_WIDTH))
                        .h(px(COMPUTER_USE_PREVIEW_HEIGHT))
                        .rounded(px(COMPUTER_USE_PREVIEW_RADIUS))
                        .overflow_hidden()
                        .border(hairline())
                        .border_color(if is_top {
                            theme.border_strong
                        } else {
                            theme.border
                        })
                        .bg(theme.raised)
                        .shadow(vec![
                            gpui::BoxShadow::new(
                                px(0.0),
                                px(4.0),
                                gpui::black().opacity(if theme.is_dark { 0.32 } else { 0.16 }),
                            )
                            .blur_radius(px(16.0)),
                        ])
                        .occlude()
                        .cursor(gpui::CursorStyle::OpenHand)
                        .focus_visible(|style| style.border_color(theme.accent))
                        .on_drag(
                            ComputerUsePreviewDrag {
                                cursor_offset: Cell::default(),
                            },
                            move |drag, offset, _, cx| {
                                drag.cursor_offset.set(offset + card_offset);
                                cx.new(|_| gpui::EmptyView)
                            },
                        )
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.focus(&mouse_focus, cx);
                            cx.stop_propagation();
                        })
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if !focus.is_focused(window) {
                                return;
                            }
                            let step = px(if event.keystroke.modifiers.shift {
                                40.0
                            } else {
                                10.0
                            });
                            let delta = match event.keystroke.key.as_str() {
                                "left" => point(-step, px(0.0)),
                                "right" => point(step, px(0.0)),
                                "up" => point(px(0.0), -step),
                                "down" => point(px(0.0), step),
                                _ => return,
                            };
                            this.computer_use_preview_position =
                                Some(clamp_computer_use_preview_position(
                                    position + delta,
                                    stack_size,
                                    window,
                                ));
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .child(
                            // GPUI overflow clipping is rectangular. Round
                            // each painted layer inside the card's hairline border.
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(COMPUTER_USE_PREVIEW_INNER_RADIUS))
                                .overflow_hidden()
                                .bg(theme.inset)
                                .when_some(screenshot, |element, screenshot| {
                                    element.child(
                                        img(screenshot)
                                            .w_full()
                                            .h_full()
                                            .rounded(px(COMPUTER_USE_PREVIEW_INNER_RADIUS))
                                            .object_fit(ObjectFit::Contain),
                                    )
                                })
                                .when(state.frames.current.is_none(), |element| {
                                    element.child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(sp(12.0))
                                            .text_color(theme.text_secondary)
                                            .child(tr!("computer_use.preparing_preview")),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .w_full()
                                .h(px(64.0))
                                .rounded_t(px(COMPUTER_USE_PREVIEW_INNER_RADIUS))
                                .pt(px(6.0))
                                .pl(px(6.0))
                                .pr(px(10.0))
                                .flex()
                                .items_start()
                                .gap(px(6.0))
                                .opacity(if keyboard_controls { 1.0 } else { 0.0 })
                                .group_hover(group_name, |style| style.opacity(1.0))
                                .bg(linear_gradient(
                                    180.0,
                                    linear_color_stop(gpui::black().opacity(0.65), 0.5),
                                    linear_color_stop(gpui::transparent_black(), 1.0),
                                ))
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "computer-use-preview-close-{window_id}"
                                        )))
                                        .track_focus(&close_focus)
                                        .aria_label(tr!("common.close"))
                                        .tab_index(0)
                                        .tab_stop(true)
                                        .size(px(26.0))
                                        .flex_none()
                                        .rounded_full()
                                        .border(hairline())
                                        .border_color(gpui::transparent_black())
                                        .focus_visible(|style| style.border_color(gpui::white()))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_default()
                                        .hover(|style| style.bg(gpui::white().opacity(0.16)))
                                        .active(|style| style.bg(gpui::white().opacity(0.24)))
                                        .tooltip(Tooltip::text(tr!("common.close")))
                                        .child(icon("icons/x.svg", 11.0, gpui::white()))
                                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                            window.focus(&close_focus, cx);
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.dismiss_computer_use(window_id, cx);
                                            window.focus(&this.composer_focus(cx), cx);
                                        })),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .pt(px(5.0))
                                        .text_size(sp(12.0))
                                        .line_height(sp(16.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(gpui::white())
                                        .truncate()
                                        .child(SharedString::from(app_name)),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.bring_computer_use_to_front(window_id, cx);
                        })),
                )
            })
            .collect::<Vec<_>>();

        let stack = div()
            .id("computer-use-previews")
            .tab_group()
            .tab_stop(false)
            .relative()
            .w(stack_size.width)
            .h(stack_size.height)
            .children(cards)
            .on_drag_move::<ComputerUsePreviewDrag>(cx.listener(
                move |this, event: &gpui::DragMoveEvent<ComputerUsePreviewDrag>, window, cx| {
                    let position = event.event.position - event.drag(cx).cursor_offset.get();
                    this.computer_use_preview_position = Some(clamp_computer_use_preview_position(
                        position, stack_size, window,
                    ));
                    // Native drag dispatch already refreshes the window; avoid
                    // an additional root notify for every pointer movement.
                },
            ));

        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(stack),
            )
            .into_any_element(),
        )
    }

    // ── Composer ───────────────────────────────────────────────────────────

    pub(super) fn render_provider_model_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let session = self.composer_session();
        let provider = session.map(|session| session.provider).unwrap_or_default();
        let auto_route = session.is_some_and(|session| session.auto_route);
        let routed = session.is_some_and(|session| session.route_decision.is_some());
        let selected_model = session.and_then(|session| self.catalog_model_id_for_session(session));
        let selected_model_name = if auto_route {
            tr!("models.auto")
        } else if routed {
            tr!(
                "models.auto_routed",
                model = self.model_display_name(provider, selected_model)
            )
        } else {
            self.model_display_name(provider, selected_model)
        };
        let picker_enabled = session.is_some_and(|session| session.can_choose_model(provider));

        // Auto routes through Jev, not the provider the draft would land on —
        // brand the chip with the router's mark instead of that provider's.
        let chip = MenuChip::new("composer-provider-model");
        let chip = if auto_route {
            chip.icon("icons/provider-typesafe.svg", theme.text_tertiary)
                .label(selected_model_name)
                .suffix("Jev")
        } else {
            chip.provider(&theme, provider, theme.text_tertiary)
                .label(selected_model_name)
        };

        if !picker_enabled {
            return chip.caret(false).disabled(true).into_any_element();
        }

        let weak = cx.entity().downgrade();
        let search = self.model_search.clone();
        let search_focus = search.read(cx).focus_handle(cx);
        let empty_focus = self.model_picker_empty_focus.clone();
        let no_providers = self.model_picker_has_no_providers();

        let handle = {
            let reset_weak = weak.clone();
            let reset_search = search.clone();
            let picker_focus = search_focus.clone();
            let empty_picker_focus = empty_focus.clone();
            self.menu_handle_with(MODEL_PICKER_MENU_ID, cx, move |open, window, cx| {
                // The empty state draws no filter field, so the handle the
                // deferred focus below targets depends on which body opened.
                let mut empty = false;
                let _ = reset_weak.update(cx, |this, cx| {
                    if open {
                        this.model_picker_target = ModelPickerTarget::Composer;
                        empty = this.model_picker_has_no_providers();
                        let locked_provider = this.model_picker_locked_provider();
                        // Opening re-runs catalog discovery for every provider
                        // the merged list can draw, so models authored since
                        // launch appear without a restart.
                        for kind in ProviderKind::ALL {
                            if picker_lists_provider(
                                &this.probes,
                                &this.state.disabled_providers,
                                locked_provider,
                                this.daemon.is_remote(),
                                kind,
                            ) {
                                this.refresh_provider_model_discovery(kind);
                            }
                        }
                        this.model_picker_highlight = None;
                        reset_search.update(cx, |search, cx| search.clear(cx));
                        this.reveal_selected_picker_model(cx);
                    } else {
                        let focus_handle = this.composer.read(cx).focus();
                        window.focus(&focus_handle, cx);
                    }
                    cx.notify();
                });
                if open {
                    // The panel is deferred, so its input joins the dispatch
                    // tree only after the deferred draw — same two-frame wait
                    // the menus need before they can take focus. The reveal is
                    // re-issued here too: a parked scroll request resolves
                    // against the viewport bounds of the *previous* paint, so
                    // on the container's first-ever paint it reads a zeroed
                    // viewport, lands wrong, and is consumed. By this frame
                    // the panel has painted real bounds to resolve against.
                    let picker_focus = if empty {
                        empty_picker_focus.clone()
                    } else {
                        picker_focus.clone()
                    };
                    let reveal_weak = reset_weak.clone();
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| {
                            window.focus(&picker_focus, cx);
                            let _ = reveal_weak.update(cx, |this, cx| {
                                this.reveal_selected_picker_model(cx);
                            });
                        });
                    });
                }
            })
        };

        // With nothing to pick from, naming a model the app cannot run would
        // be a lie. The chip says so instead, and stays a trigger because the
        // panel behind it is where the fix lives. Icon plus wording carry the
        // state on their own, so the warning tint is never the only signal.
        let trigger = if no_providers {
            MenuChip::new("composer-provider-model")
                .icon("icons/alert.svg", theme.warning)
                .label(tr!("models.no_providers"))
        } else {
            chip.tooltip(tr!("command_palette.choose_model"))
                .shortcut_action(&ToggleModelPicker)
        };

        popover(
            trigger.caret(false).selected(handle.is_open()),
            &handle,
            MenuAlign::AboveLeft,
            move |popover, _window, cx| {
                let weak = weak.clone();
                let panel_weak = weak.clone();
                weak.read_with(cx, move |this, cx| {
                    this.render_model_picker_panel(&panel_weak, popover, cx)
                })
                .unwrap_or_else(|_| div().into_any_element())
            },
        )
    }

    /// The picker's panel — provider rail, filter field, virtualized rows.
    /// Shared by the composer chip and the automation editor's model control:
    /// `model_picker_target` decides what a pick writes and which granularity
    /// the rows name. The panel only paints while its popover is open, so the
    /// catalog clones below never run for a closed picker.
    pub(super) fn render_model_picker_panel(
        &self,
        weak: &WeakEntity<Waku>,
        popover: &ContextMenuHandle,
        cx: &App,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let weak = weak.clone();
        let popover = popover.clone();
        if self.model_picker_has_no_providers() {
            let empty_focus = self.model_picker_empty_focus.clone();
            return model_picker_empty_state(&theme, &empty_focus, popover, weak);
        }
        let search_query = self.model_search.read(cx).content().to_owned();
        let normalized_query = search_query.trim().to_ascii_lowercase();
        let searching = !normalized_query.is_empty();
        let probes = self.probes.clone();
        let disabled_providers = self.state.disabled_providers.clone();
        let remote = self.daemon.is_remote();
        let pending_discoveries = self.provider_model_discoveries_pending.clone();
        let favorites = self.state.favorite_models.clone();
        let recents = self.state.recent_model_uses.clone();
        let search = self.model_search.clone();
        let locked_provider = self.model_picker_locked_provider();
        let available_rows = Rc::new(visible_picker_rows(
            &probes,
            &favorites,
            &recents,
            &disabled_providers,
            locked_provider,
            &normalized_query,
            self.model_picker_offers_auto_route(),
            self.model_picker_granularity(),
        ));
        // A rail button's jump clears the query before scrolling, so which
        // sections exist is answered by the unfiltered list — not by what a
        // live search happens to leave. A provider whose models are all
        // filtered out of the merged list entirely (a locked session's other
        // providers, or one whose combos all sit in favorites/recents) gets
        // no rail button, and neither does a favorites or recents section
        // with nothing to scroll to.
        let section_rows = if searching {
            Rc::new(visible_picker_rows(
                &probes,
                &favorites,
                &recents,
                &disabled_providers,
                locked_provider,
                "",
                self.model_picker_offers_auto_route(),
                self.model_picker_granularity(),
            ))
        } else {
            available_rows.clone()
        };
        let highlight = self
            .model_picker_highlight
            .filter(|index| *index < available_rows.len());
        let list_state = self.model_picker_list.clone();
        let scrollbar_state = self.model_picker_scrollbar.clone();
        let session_selection = self.model_picker_selection(cx);
        let auto_route = self.model_picker_target == ModelPickerTarget::Composer
            && self
                .composer_session()
                .is_some_and(|session| session.auto_route);
        // The rail offers a favorites or recents jump only when the merged
        // list actually draws that section — a stored entry whose provider
        // fell out of the picker (switched off, locked out, uninstalled)
        // leaves nothing to scroll to.
        let rail_favorites = section_rows
            .iter()
            .any(|row| picker_row_section(row) == ModelPickerSection::Favorites);
        let rail_recents = section_rows
            .iter()
            .any(|row| picker_row_section(row) == ModelPickerSection::Recents);

        // The rail jumps rather than filters: a button drops the
        // query and brings its section's first row into view.
        let rail_target = |id: SharedString, section: ModelPickerSection| {
            let rail_weak = weak.clone();
            div()
                .id(id)
                .w(px(38.0))
                .h(px(38.0))
                .rounded(px(9.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .hover(|element| element.bg(theme.overlay))
                .on_click(move |_, _, cx| {
                    let _ = rail_weak.update(cx, |this, cx| {
                        this.scroll_model_picker_to_section(section, cx);
                    });
                })
        };
        let mut rail = div()
            .w(px(50.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .p(px(5.0))
            .rounded_tl(px(15.0))
            .rounded_bl(px(15.0))
            .bg(theme.canvas)
            .border_r(hairline())
            .border_color(theme.separator);
        if rail_favorites {
            rail = rail.child(
                rail_target("model-rail-favorites".into(), ModelPickerSection::Favorites)
                    .child(icon("icons/star.svg", 17.0, theme.text_tertiary)),
            );
        }
        if rail_recents {
            rail = rail.child(
                rail_target("model-rail-recents".into(), ModelPickerSection::Recents)
                    .child(icon("icons/hourglass.svg", 17.0, theme.text_tertiary)),
            );
        }
        if rail_favorites || rail_recents {
            rail = rail.child(
                div()
                    .w(px(34.0))
                    .h(hairline())
                    .my(px(3.0))
                    .bg(theme.separator),
            );
        }
        for kind in ProviderKind::ALL {
            if !picker_lists_provider(&probes, &disabled_providers, locked_provider, remote, kind) {
                continue;
            }
            // No provider block in the merged list means no scroll
            // target — a locked session's other providers land here,
            // as does one whose combos are all favorites or recents.
            if !section_rows
                .iter()
                .any(|row| picker_row_section(row) == ModelPickerSection::Provider(kind))
            {
                continue;
            }
            rail = rail.child(
                rail_target(
                    SharedString::from(format!("model-rail-{}", kind.id())),
                    ModelPickerSection::Provider(kind),
                )
                .child(provider_mark(&theme, kind, 18.0, theme.text_tertiary)),
            );
        }

        let search_input = div()
            .h(px(52.0))
            .px(px(12.0))
            .pt(px(10.0))
            .pb(px(8.0))
            .flex_none()
            .flex()
            .items_center()
            .child(
                div()
                    .w_full()
                    .h(px(34.0))
                    .px(px(10.0))
                    .rounded(px(11.0))
                    .bg(theme.raised)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(icon("icons/search.svg", 15.0, theme.text_secondary))
                    .child(div().flex_1().min_w_0().child(search.clone())),
            );

        let mut rows = div().id("model-picker-list").size_full();
        if available_rows.is_empty() {
            let label = if searching {
                tr!("models.none_found")
            } else if ProviderKind::ALL.into_iter().any(|kind| {
                pending_discoveries.contains(&kind)
                    && picker_lists_provider(
                        &probes,
                        &disabled_providers,
                        locked_provider,
                        remote,
                        kind,
                    )
            }) {
                tr!("models.loading")
            } else {
                tr!("models.none_reported")
            };
            rows = rows.p(px(9.0)).child(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(sp(12.5))
                    .text_color(theme.text_ghost)
                    .child(label),
            );
        } else {
            // The list is virtualized and every row is one height, so
            // the item count resyncs here and before every scroll —
            // `reset` drops the scroll position, which is also what a
            // changed total should mean.
            if list_state.item_count() != available_rows.len() {
                list_state.reset_with_uniform_height(
                    available_rows.len(),
                    MODEL_PICKER_ROW_HEIGHT,
                );
            }
            let list_rows = available_rows.clone();
            let list_weak = weak.clone();
            let list_popover = popover.clone();
            let list_selection = session_selection.clone();
            let list_auto = auto_route;
            let jev_credential_missing = self.jev_credential_missing();
            rows = rows.child(
                list(list_state.clone(), move |row_index, window, cx| {
                    let theme = Theme::current(cx);
                    let weak = &list_weak;
                    let popover = &list_popover;
                    let session_selection = &list_selection;
                    let auto_route = list_auto;
                    let Some(row) = list_rows.get(row_index) else {
                        return div().into_any_element();
                    };
                    let is_highlighted = highlight == Some(row_index);
                    if row.auto {
                        // The router row: same hit target and highlight
                        // treatment as a model row, but where a model
                        // row carries its star this one carries a
                        // shortcut to the Jev settings page — there is
                        // no concrete model to favorite.
                        let select_weak = weak.clone();
                        let select_popover = popover.clone();
                        let settings_weak = weak.clone();
                        let settings_popover = popover.clone();
                        return div()
                            .id("model-row-auto")
                            .w_full()
                            .h(MODEL_PICKER_ROW_HEIGHT)
                            .px(px(12.0))
                            .rounded(px(11.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .cursor_default()
                            .border(hairline())
                            .border_color(gpui::transparent_black())
                            .when(auto_route, |element| element.bg(theme.overlay_strong))
                            .when(is_highlighted, |element| {
                                element.bg(theme.overlay).border_color(theme.accent)
                            })
                            .hover(|element| element.bg(theme.overlay))
                            .active(|element| element.opacity(0.85))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(sp(13.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme.text)
                                                    .child(SharedString::from(tr!(
                                                        "models.auto"
                                                    ))),
                                            )
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .truncate()
                                                    .text_size(sp(12.5))
                                                    .text_color(theme.text_tertiary)
                                                    .child(SharedString::from(tr!(
                                                        "models.auto_hint"
                                                    ))),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.0))
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(icon(
                                                "icons/provider-typesafe.svg",
                                                12.0,
                                                theme.text_tertiary,
                                            ))
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(sp(12.5))
                                                    .text_color(theme.text_tertiary)
                                                    .child("Jev"),
                                            ),
                                    ),
                            )
                            // The selected backend has no usable
                            // credential — flag it beside the
                            // settings shortcut that fixes it.
                            .when(jev_credential_missing, |element| {
                                element.child(icon(
                                    "icons/alert.svg",
                                    13.0,
                                    theme.warning,
                                ))
                            })
                            .child(
                                div()
                                    .id("jev-settings")
                                    .w(px(28.0))
                                    .h(px(28.0))
                                    .rounded(px(8.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|element| element.bg(theme.overlay_strong))
                                    .tooltip(Tooltip::text(tr!("settings.jev")))
                                    .child(icon(
                                        "icons/settings.svg",
                                        14.0,
                                        theme.text_ghost,
                                    ))
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        open_settings_page_from_picker(
                                            &settings_weak,
                                            &settings_popover,
                                            SettingsPage::Jev,
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                            .on_click(move |_, window, cx| {
                                let _ = select_weak.update(cx, |this, cx| {
                                    this.choose_auto_route(cx);
                                });
                                select_popover.close(window, cx);
                            })
                            .into_any_element();
                    }
                    let kind = row.provider;
                    let model = &row.model;
                    let is_selected = session_selection.as_ref().is_some_and(
                        |(provider, model_id, effort, fast)| {
                            *provider == kind
                                && model_id == &model.id
                                && effort == &row.effort
                                && *fast == row.fast
                        },
                    );
                    let favorite_index = row.favorite_index;
                    let is_favorite = favorite_index.is_some();
                    let model_id = model.id.clone();
                    let favorite_model_id = model.id.clone();
                    let effort = row.effort.clone();
                    let favorite_effort = row.effort.clone();
                    let fast = row.fast;
                    let select_weak = weak.clone();
                    let select_popover = popover.clone();
                    let favorite_weak = weak.clone();
                    let drop_weak = weak.clone();
                    let effort_label = row.effort.as_deref().and_then(|effort| {
                        model
                            .reasoning_efforts
                            .iter()
                            .find(|option| option.id == effort)
                            .map(|option| {
                                option
                                    .label_i18n
                                    .as_ref()
                                    .map(waku_client::WireTranslation::render)
                                    .unwrap_or_else(|| option.label.clone())
                            })
                    });
                    let sub_provider = model
                        .sub_provider
                        .as_deref()
                        .map(str::trim)
                        .filter(|name| !name.is_empty());
                    let detail = model_picker_subtitle(kind, sub_provider);
                    // The ⌘⌥1–⌘⌥9 chord rides on the first nine starred rows.
                    // Resolve against the live keymap so a remapped chord
                    // still advertises itself; fall back to the default's
                    // label when the picker's own context path cannot see the
                    // scoped binding.
                    let shortcut_hint = favorite_index.filter(|index| *index < 9).map(|index| {
                        crate::ui::shortcut::ShortcutHint::action(&SelectFavoriteModel { index })
                            .resolve(window, cx)
                            .unwrap_or_else(|| {
                                crate::ui::shortcut::sequence_label(&format!(
                                    "secondary-alt-{}",
                                    index + 1
                                ))
                            })
                    });
                    let mut row_element = div()
                        .id(SharedString::from(format!(
                            "model-row-{}-{}-{}-{}",
                            kind.id(),
                            model.id,
                            row.effort.as_deref().unwrap_or("base"),
                            row.fast
                        )))
                        .w_full()
                        .h(MODEL_PICKER_ROW_HEIGHT)
                        .px(px(12.0))
                        .rounded(px(11.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .cursor_default()
                        // Reserved on every row so highlighting one cannot
                        // resize it and shift the list by a pixel.
                        .border(hairline())
                        .border_color(gpui::transparent_black())
                        .when(is_selected, |element| element.bg(theme.overlay_strong))
                        // The keyboard cursor reads as a ring rather than a
                        // fill, so it stays legible on the current model's
                        // already-filled row.
                        .when(is_highlighted, |element| {
                            element.bg(theme.overlay).border_color(theme.accent)
                        })
                        .hover(|element| element.bg(theme.overlay))
                        .active(|element| element.opacity(0.85))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(sp(13.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.text)
                                                .child(SharedString::from(
                                                    model
                                                        .name_i18n
                                                        .as_ref()
                                                        .map(waku_client::WireTranslation::render)
                                                        .unwrap_or_else(|| model.name.clone()),
                                                )),
                                        )
                                        .when_some(effort_label, |element, label| {
                                            element.child(
                                                div()
                                                    .flex_none()
                                                    .truncate()
                                                    .text_size(sp(12.5))
                                                    .text_color(theme.text_tertiary)
                                                    .child(SharedString::from(label)),
                                            )
                                        })
                                        .when(fast, |element| {
                                            element.child(icon(
                                                "icons/zap.svg",
                                                11.5,
                                                theme.text_tertiary,
                                            ))
                                        }),
                                )
                                .child(
                                    div()
                                        .mt(px(4.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(provider_mark(
                                            &theme,
                                            kind,
                                            12.0,
                                            theme.text_tertiary,
                                        ))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(sp(12.5))
                                                .text_color(theme.text_tertiary)
                                                .child(SharedString::from(detail)),
                                        ),
                                ),
                        )
                        .when_some(shortcut_hint, |element, hint| {
                            element.child(
                                div()
                                    .flex_none()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_ghost)
                                    .child(SharedString::from(hint)),
                            )
                        })
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "favorite-model-{}-{}-{}-{}",
                                    kind.id(),
                                    model.id,
                                    row.effort.as_deref().unwrap_or("base"),
                                    row.fast
                                )))
                                .w(px(28.0))
                                .h(px(28.0))
                                .rounded(px(8.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|element| element.bg(theme.overlay_strong))
                                .child(icon(
                                    if is_favorite {
                                        "icons/star-filled.svg"
                                    } else {
                                        "icons/star.svg"
                                    },
                                    14.0,
                                    if is_favorite {
                                        theme.favorite
                                    } else {
                                        theme.text_ghost
                                    },
                                ))
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = favorite_weak.update(cx, |this, cx| {
                                        this.toggle_favorite_model(
                                            kind,
                                            favorite_model_id.clone(),
                                            favorite_effort.clone(),
                                            fast,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .on_click(move |_, window, cx| {
                            let _ = select_weak.update(cx, |this, cx| {
                                this.choose_model(kind, model_id.clone(), effort.clone(), fast, cx);
                            });
                            select_popover.close(window, cx);
                        });
                    // Starred rows are the drag-reorder surface: dragging one
                    // onto another favorite takes that row's slot.
                    if let Some(target) = favorite_index {
                        let label = SharedString::from(model.name.clone());
                        row_element = row_element
                            .on_drag(
                                FavoriteModelDrag {
                                    index: target,
                                    label,
                                },
                                move |drag, _, _, cx| {
                                    cx.new(|_| FavoriteModelDragView {
                                        label: drag.label.clone(),
                                    })
                                },
                            )
                            .drag_over::<FavoriteModelDrag>(move |style, _, _, _| {
                                style.bg(theme.overlay_strong)
                            })
                            .on_drop(move |drag: &FavoriteModelDrag, _, cx| {
                                let _ = drop_weak.update(cx, |this, cx| {
                                    this.move_favorite_model(drag.index, target, cx);
                                });
                            });
                    }
                    row_element.into_any_element()
                })
                .size_full()
                .p(px(9.0)),
            );
        }

        let next_models = available_rows.clone();
        let previous_models = available_rows.clone();
        let confirm_models = available_rows.clone();
        let next_weak = weak.clone();
        let previous_weak = weak.clone();
        let next_section_weak = weak.clone();
        let previous_section_weak = weak.clone();
        let confirm_weak = weak.clone();
        let confirm_popover = popover.clone();
        div()
            .w(px(460.0))
            .h(px(390.0))
            .rounded(px(16.0))
            .overflow_hidden()
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.surface)
            .shadow_lg()
            .flex()
            // The filter field keeps focus and the selected row is only
            // drawn, never focused — the same split Zed's picker uses.
            // These arrive as actions bound to `WakuMenu > TextInput`,
            // which is the only way to claim a key out from under a
            // focused text field.
            .on_action(move |_: &SelectNextEntry, _, cx| {
                let _ = next_weak.update(cx, |this, cx| {
                    this.move_model_picker_highlight("down", &next_models, cx);
                });
            })
            .on_action(move |_: &SelectPreviousEntry, _, cx| {
                let _ = previous_weak.update(cx, |this, cx| {
                    this.move_model_picker_highlight("up", &previous_models, cx);
                });
            })
            .on_action(move |_: &SelectNextTab, _, cx| {
                let _ = next_section_weak.update(cx, |this, cx| {
                    this.cycle_model_picker_section("down", cx);
                });
            })
            .on_action(move |_: &SelectPreviousTab, _, cx| {
                let _ = previous_section_weak.update(cx, |this, cx| {
                    this.cycle_model_picker_section("up", cx);
                });
            })
            .on_action(move |_: &ConfirmEntry, window, cx| {
                let _ = confirm_weak.update(cx, |this, cx| {
                    this.choose_highlighted_model(&confirm_models, cx);
                });
                confirm_popover.close(window, cx);
                window.refresh();
            })
            .child(rail)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .rounded_tr(px(15.0))
                    .rounded_br(px(15.0))
                    .bg(theme.surface)
                    .child(search_input)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(rows)
                            .child(scrollbar::vertical(&list_state, &scrollbar_state)),
                    ),
            )
            .into_any_element()
    }

    /// The provider the picker cannot switch away from — a session that has
    /// already sent messages locks its provider. The automation editor never
    /// locks one.
    fn model_picker_locked_provider(&self) -> Option<ProviderKind> {
        match self.model_picker_target {
            ModelPickerTarget::Composer => self
                .composer_session()
                .filter(|session| !session.messages.is_empty())
                .map(|session| session.provider),
            ModelPickerTarget::AutomationEditor => None,
        }
    }

    /// The picker's "current" row — the composer session's effective combo, or
    /// the editor's bare provider/model pair (no effort, no tier).
    fn model_picker_selection(
        &self,
        cx: &App,
    ) -> Option<(ProviderKind, String, Option<String>, bool)> {
        match self.model_picker_target {
            ModelPickerTarget::Composer => self.composer_session().and_then(|session| {
                self.session_model_combo(session)
                    .map(|(model, effort, fast)| (session.provider, model, effort, fast))
            }),
            ModelPickerTarget::AutomationEditor => {
                let editor = self.automations_editor.as_ref()?;
                let model = editor.model.read(cx).content().trim().to_owned();
                (!model.is_empty()).then(|| (editor.provider, model, None, false))
            }
        }
    }

    /// The Auto row only exists for the composer — the editor stores a bare
    /// `provider:model` pair, so it cannot express routing.
    fn model_picker_offers_auto_route(&self) -> bool {
        self.model_picker_target == ModelPickerTarget::Composer && self.auto_route_available()
    }

    /// Composer picks carry effort and tier; the editor's bare pair cannot.
    fn model_picker_granularity(&self) -> PickerGranularity {
        match self.model_picker_target {
            ModelPickerTarget::Composer => PickerGranularity::Combos,
            ModelPickerTarget::AutomationEditor => PickerGranularity::Models,
        }
    }

    /// Move the picker's drawn selection. Nothing is focused: the filter field
    /// keeps focus so typing continues to narrow the list.
    fn move_model_picker_highlight(
        &mut self,
        key: &str,
        rows: &[ModelPickerRow],
        cx: &mut Context<Self>,
    ) {
        // With nothing highlighted yet the cursor sits on the session's
        // combo — the row the reveal scrolled into view — so the first arrow
        // moves relative to it rather than jumping to an end. A combo that is
        // not listed (an unknown model, an unlisted effort) keeps the old
        // behavior of starting at an edge.
        let auto_route = self.model_picker_target == ModelPickerTarget::Composer
            && self
                .composer_session()
                .is_some_and(|session| session.auto_route);
        let selection = self.model_picker_selection(cx);
        let current = self
            .model_picker_highlight
            .filter(|index| *index < rows.len())
            .or_else(|| picker_selected_row_index(selection.as_ref(), auto_route, rows));
        let Some(next) = next_picker_highlight(current, rows.len(), key) else {
            return;
        };
        self.model_picker_highlight = Some(next);
        self.model_picker_list.scroll_to_reveal_item(next);
        cx.notify();
    }

    /// Keep the virtualized list's item count in step with the rows a scroll
    /// is about to target — `reset` drops the scroll position, so it only
    /// runs when the total changed, right before the scroll is issued.
    fn sync_model_picker_list(&self, count: usize) {
        if self.model_picker_list.item_count() != count {
            self.model_picker_list
                .reset_with_uniform_height(count, MODEL_PICKER_ROW_HEIGHT);
        }
    }

    /// Bring the current selection's row into view whenever the picker shows
    /// the unfiltered list — on open and on a cleared query.
    ///
    /// The offset sits in the list state until the row list next paints, so
    /// it may be issued from the open toggle before the deferred panel
    /// exists, and a provider whose models are still loading reveals the row
    /// once they arrive. Without a row to reveal it falls back to the top, so
    /// a scroll offset from an earlier open never leaks into a fresh list.
    pub(super) fn reveal_selected_picker_model(&self, cx: &App) {
        let rows = visible_picker_rows(
            &self.probes,
            &self.state.favorite_models,
            &self.state.recent_model_uses,
            &self.state.disabled_providers,
            self.model_picker_locked_provider(),
            "",
            self.model_picker_offers_auto_route(),
            self.model_picker_granularity(),
        );
        let selection = self.model_picker_selection(cx);
        let auto_route = self.model_picker_target == ModelPickerTarget::Composer
            && self
                .composer_session()
                .is_some_and(|session| session.auto_route);
        let index =
            picker_selected_row_index(selection.as_ref(), auto_route, &rows).unwrap_or(0);
        self.sync_model_picker_list(rows.len());
        self.model_picker_list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: Pixels::ZERO,
        });
    }

    /// A rail button's jump: drop the filter and bring the section's first
    /// row into view, leaving the keyboard cursor on it so the next arrow
    /// moves from there. Clearing the query routes back through the search
    /// subscription's selection reveal, so the section scroll issued after
    /// it is the request that lands.
    fn scroll_model_picker_to_section(
        &mut self,
        section: ModelPickerSection,
        cx: &mut Context<Self>,
    ) {
        self.model_search.update(cx, |search, cx| search.clear(cx));
        let rows = visible_picker_rows(
            &self.probes,
            &self.state.favorite_models,
            &self.state.recent_model_uses,
            &self.state.disabled_providers,
            self.model_picker_locked_provider(),
            "",
            self.model_picker_offers_auto_route(),
            self.model_picker_granularity(),
        );
        let first = rows
            .iter()
            .position(|row| picker_row_section(row) == section);
        self.model_picker_highlight = first;
        if let Some(index) = first {
            self.sync_model_picker_list(rows.len());
            self.model_picker_list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item: Pixels::ZERO,
            });
        }
        cx.notify();
    }

    /// Step the rail to the adjacent section, wrapping at both ends.
    /// `tab`/`shift-tab` land here from under the focused filter field, the
    /// same route the arrows take. A live query filters across all
    /// sections, so cycling waits until the field is cleared.
    fn cycle_model_picker_section(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.model_search.read(cx).content().trim().is_empty() {
            return;
        }
        let rows = visible_picker_rows(
            &self.probes,
            &self.state.favorite_models,
            &self.state.recent_model_uses,
            &self.state.disabled_providers,
            self.model_picker_locked_provider(),
            "",
            self.model_picker_offers_auto_route(),
            self.model_picker_granularity(),
        );
        let mut sections = Vec::new();
        let mut previous = None;
        for (index, row) in rows.iter().enumerate() {
            let section = picker_row_section(row);
            if previous != Some(section) {
                sections.push(index);
                previous = Some(section);
            }
        }
        if sections.is_empty() {
            return;
        }
        // The section the keyboard cursor sits in — seeded from the
        // session combo's row the way the reveal lands, so the first tab
        // steps relative to the selection rather than an end.
        let auto_route = self.model_picker_target == ModelPickerTarget::Composer
            && self
                .composer_session()
                .is_some_and(|session| session.auto_route);
        let selection = self.model_picker_selection(cx);
        let current_row = self.model_picker_highlight.unwrap_or_else(|| {
            picker_selected_row_index(selection.as_ref(), auto_route, &rows).unwrap_or(0)
        });
        let current_section = sections
            .iter()
            .rposition(|start| *start <= current_row)
            .unwrap_or(0);
        let Some(next) = next_picker_highlight(Some(current_section), sections.len(), key) else {
            return;
        };
        self.model_picker_highlight = Some(sections[next]);
        self.sync_model_picker_list(rows.len());
        self.model_picker_list.scroll_to(ListOffset {
            item_ix: sections[next],
            offset_in_item: Pixels::ZERO,
        });
        cx.notify();
    }

    /// Take the row the selection is on, defaulting to the first so `enter`
    /// works the moment the panel opens.
    fn choose_highlighted_model(&mut self, rows: &[ModelPickerRow], cx: &mut Context<Self>) {
        let Some(row) = rows.get(self.model_picker_highlight.unwrap_or(0)) else {
            return;
        };
        if row.auto {
            self.choose_auto_route(cx);
            return;
        }
        let (kind, model_id, effort, fast) = (
            row.provider,
            row.model.id.clone(),
            row.effort.clone(),
            row.fast,
        );
        self.choose_model(kind, model_id, effort, fast, cx);
    }

    pub(super) fn render_model_traits_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        let session = self.composer_session()?;
        let model = self.model_metadata_for_session(session)?;
        if model.reasoning_efforts.is_empty()
            && model.service_tiers.is_empty()
            && model.context_windows.is_empty()
        {
            return None;
        }

        // A stored packed alias carries its traits in the id's suffix — decode
        // them so the chip shows what the session would run at.
        let packed_suffix = self
            .model_for_session(session)
            .and_then(|requested| {
                self.provider_probe(session.provider).and_then(|probe| {
                    waku_protocol::model_catalog::packed_catalog_model(
                        &probe.models,
                        requested,
                        session.provider,
                    )
                })
            })
            .map(|matched| matched.suffix)
            .unwrap_or_default();
        let suffix_effort = waku_protocol::model_catalog::packed_suffix_reasoning_effort(
            &packed_suffix,
            &model.reasoning_efforts,
        );
        let suffix_tier = waku_protocol::model_catalog::packed_suffix_service_tier(
            &packed_suffix,
            &model.service_tiers,
        );

        let supports_default_reset = supports_reasoning_default_reset(session.provider);
        let mut selected_effort = session
            .reasoning_effort
            .as_deref()
            .filter(|selected| {
                model
                    .reasoning_efforts
                    .iter()
                    .any(|option| option.id == *selected)
            })
            .or(suffix_effort.as_deref())
            .map(str::to_owned);
        if selected_effort.is_none() && !supports_default_reset {
            selected_effort = model.default_reasoning_effort.clone().or_else(|| {
                model
                    .reasoning_efforts
                    .first()
                    .map(|option| option.id.clone())
            });
        }
        let effort_label = if model.reasoning_efforts.is_empty() {
            None
        } else if selected_effort.is_none() {
            Some(tr!("common.default"))
        } else {
            selected_effort.as_deref().and_then(|selected| {
                model
                    .reasoning_efforts
                    .iter()
                    .find(|option| option.id == selected)
                    .map(|option| {
                        option
                            .label_i18n
                            .as_ref()
                            .map(waku_client::WireTranslation::render)
                            .unwrap_or_else(|| option.label.clone())
                    })
            })
        };

        let selected_tier = session
            .service_tier
            .as_deref()
            .filter(|selected| {
                *selected == "default"
                    || model
                        .service_tiers
                        .iter()
                        .any(|option| option.id == *selected)
            })
            .or(suffix_tier.as_deref())
            .or(model.default_service_tier.as_deref())
            .unwrap_or("default")
            .to_owned();
        let tier_label = if selected_tier == "default" {
            tr!("models.standard")
        } else {
            model
                .service_tiers
                .iter()
                .find(|option| option.id == selected_tier)
                .map(|option| {
                    option
                        .label_i18n
                        .as_ref()
                        .map(waku_client::WireTranslation::render)
                        .unwrap_or_else(|| option.label.clone())
                })
                .unwrap_or_else(|| selected_tier.clone())
        };
        let selected_window = session
            .context_window
            .as_deref()
            .filter(|selected| {
                model
                    .context_windows
                    .iter()
                    .any(|option| option.id == *selected)
            })
            .or(model.default_context_window.as_deref())
            .or_else(|| {
                model
                    .context_windows
                    .first()
                    .map(|option| option.id.as_str())
            })
            .map(str::to_owned);
        // A non-default window changes what the session costs and how much it
        // can hold, so it reads on the chip rather than only inside the menu.
        let window_label = selected_window
            .as_deref()
            .filter(|selected| model.default_context_window.as_deref() != Some(selected))
            .and_then(|selected| {
                model
                    .context_windows
                    .iter()
                    .find(|option| option.id == selected)
                    .map(|option| {
                        option
                            .label_i18n
                            .as_ref()
                            .map(waku_client::WireTranslation::render)
                            .unwrap_or_else(|| option.label.clone())
                    })
            });

        let fast = selected_tier == "fast" || tier_label.eq_ignore_ascii_case("fast");
        let trigger_label = match (
            effort_label.unwrap_or_else(|| tier_label.clone()),
            window_label,
        ) {
            (label, Some(window)) => format!("{label} · {window}"),
            (label, None) => label,
        };
        let reasoning_efforts = model.reasoning_efforts.clone();
        let service_tiers = model.service_tiers.clone();
        let context_windows = model.context_windows.clone();
        let weak = cx.entity().downgrade();
        let handle = self.menu_handle("model-traits", cx);
        Some(dropdown_menu(
            MenuChip::new("model-traits")
                .when(fast, |trigger| {
                    trigger.icon("icons/zap.svg", theme.text_tertiary)
                })
                .label(trigger_label)
                .label_color(theme.text_tertiary)
                .caret(false)
                .selected(handle.is_open()),
            "model-traits-menu",
            &handle,
            MenuAlign::AboveLeft,
            move |_| {
                let mut items = Vec::new();
                if !reasoning_efforts.is_empty() {
                    items.push(MenuItem::Header(tr!("models.reasoning").into()));
                    if supports_default_reset {
                        let weak_default = weak.clone();
                        items.push(
                            traits_choice(
                                theme,
                                tr!("common.default"),
                                selected_effort.is_none(),
                            )
                            .on_click(move |_, cx| {
                                let _ = weak_default.update(cx, |this, cx| {
                                    this.clear_reasoning_effort(cx);
                                });
                            }),
                        );
                    }
                    for option in reasoning_efforts.clone() {
                        let weak = weak.clone();
                        let label = option
                            .label_i18n
                            .as_ref()
                            .map(waku_client::WireTranslation::render)
                            .unwrap_or(option.label);
                        let effort = option.id;
                        let selected = selected_effort.as_deref() == Some(effort.as_str());
                        items.push(
                            traits_choice(theme, label, selected).on_click(
                                move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_reasoning_effort(effort.clone(), cx);
                                    });
                                },
                            ),
                        );
                    }
                }
                if !service_tiers.is_empty() {
                    if !reasoning_efforts.is_empty() {
                        items.push(MenuItem::Separator);
                    }
                    items.push(MenuItem::Header(tr!("models.service_tier").into()));
                    let weak_standard = weak.clone();
                    items.push(
                        traits_choice(
                            theme,
                            tr!("models.standard"),
                            selected_tier == "default",
                        )
                        .on_click(move |_, cx| {
                            let _ = weak_standard.update(cx, |this, cx| {
                                this.set_service_tier("default".to_owned(), cx);
                            });
                        }),
                    );
                    for option in service_tiers.clone() {
                        let weak = weak.clone();
                        let label = option
                            .label_i18n
                            .as_ref()
                            .map(waku_client::WireTranslation::render)
                            .unwrap_or(option.label);
                        let tier = option.id;
                        let selected = selected_tier == tier;
                        items.push(
                            traits_choice(theme, label, selected).on_click(
                                move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_service_tier(tier.clone(), cx);
                                    });
                                },
                            ),
                        );
                    }
                }
                if !context_windows.is_empty() {
                    if !reasoning_efforts.is_empty() || !service_tiers.is_empty() {
                        items.push(MenuItem::Separator);
                    }
                    items.push(MenuItem::Header(tr!("models.context_window").into()));
                    for option in context_windows.clone() {
                        let weak = weak.clone();
                        let label = option
                            .label_i18n
                            .as_ref()
                            .map(waku_client::WireTranslation::render)
                            .unwrap_or(option.label);
                        let window = option.id;
                        let selected = selected_window.as_deref() == Some(window.as_str());
                        items.push(
                            traits_choice(theme, label, selected).on_click(
                                move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_context_window(window.clone(), cx);
                                    });
                                },
                            ),
                        );
                    }
                }
                items
            },
        ))
    }

    pub(super) fn render_access_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let session = self.composer_session();
        let selected_mode = session
            .map(|session| session.runtime_mode)
            .unwrap_or_default();
        let sandboxed = session.is_some_and(|session| session.sandboxed);
        // The environment is provisioned when the session boots — a started
        // task's section still shows where it runs, but no longer changes it.
        let started = session.is_some_and(AgentSession::has_started);
        let weak = cx.entity().downgrade();
        let handle = self.menu_handle(RUNTIME_MODE_MENU_ID, cx);
        // One row shape for both sections: leading icon, label over a
        // description, a trailing check on the live choice. `enabled` dims a
        // row that can only be read, not picked.
        let choice_row = Rc::new(
            move |icon_path: &'static str,
                  label: String,
                  description: String,
                  selected: bool,
                  enabled: bool| {
                div()
                    .w(px(288.0))
                    .py(px(4.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(icon(icon_path, 14.0, theme.text_tertiary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(if enabled {
                                        theme.text
                                    } else {
                                        theme.text_tertiary
                                    })
                                    .child(label),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .mt(px(2.0))
                                    .text_size(sp(12.5))
                                    .line_height(sp(14.0))
                                    .whitespace_normal()
                                    .text_color(theme.text_tertiary)
                                    .child(description),
                            ),
                    )
                    .when(selected, |element| {
                        element.child(icon("icons/check.svg", 11.0, theme.text_tertiary))
                    })
                    .into_any_element()
            },
        );
        dropdown_menu(
            MenuChip::new("runtime-mode")
                // Sandboxed sessions trade the mode glyph for the container —
                // the same icon the badge wears while the task runs.
                .icon(
                    if sandboxed {
                        "icons/container.svg"
                    } else {
                        selected_mode.icon()
                    },
                    theme.text_tertiary,
                )
                .icon_size(14.0)
                .label(selected_mode.label())
                .caret(false)
                .selected(handle.is_open())
                .tooltip(tr!("mode.choose"))
                .shortcut_action(&ToggleRuntimeModePicker),
            "runtime-mode-menu",
            &handle,
            MenuAlign::AboveLeft,
            move |_| {
                let mut items: Vec<MenuItem> = RuntimeMode::ACCESS_OPTIONS
                    .into_iter()
                    .map(|option| {
                        let weak = weak.clone();
                        let choice_row = choice_row.clone();
                        let selected = option == selected_mode;
                        MenuItem::custom(move |_, _| {
                            choice_row(
                                option.icon(),
                                option.label(),
                                option.description(),
                                selected,
                                true,
                            )
                        })
                        .on_click(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_runtime_mode(option, window, cx)
                            });
                        })
                    })
                    .collect();
                items.push(MenuItem::Separator);
                items.push(MenuItem::Header(tr!("sandbox.environment").into()));
                for (icon_path, label, description, value) in [
                    (
                        "icons/laptop.svg",
                        tr!("sandbox.this_mac"),
                        tr!("sandbox.this_mac_description"),
                        false,
                    ),
                    (
                        "icons/container.svg",
                        tr!("sandbox.sandbox_vm"),
                        tr!("sandbox.sandbox_vm_description"),
                        true,
                    ),
                ] {
                    let selected = value == sandboxed;
                    let weak = weak.clone();
                    let choice_row = choice_row.clone();
                    let row = MenuItem::custom(move |_, _| {
                        choice_row(
                            icon_path,
                            label.clone(),
                            description.clone(),
                            selected,
                            !started,
                        )
                    });
                    // No `on_click` once the session exists — the row reports
                    // the environment rather than choosing it.
                    items.push(if started {
                        row
                    } else {
                        row.on_click(move |_, cx| {
                            let _ = weak.update(cx, |this, cx| this.set_sandboxed(value, cx));
                        })
                    });
                }
                items
            },
        )
    }

    pub(super) fn render_agent_preset_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let session = self
            .composer_session()
            .filter(|session| session.provider == ProviderKind::DeepSeek)?;
        if session.has_started() || session.is_busy() {
            return None;
        }
        let presets = self
            .provider_probe(ProviderKind::DeepSeek)
            .map(|probe| probe.agent_presets.clone())
            .unwrap_or_default();
        if presets.is_empty() {
            return None;
        }
        let selected_id = self.agent_preset_for_session(session)?;
        let selected_label = self.agent_preset_label_for_session(session)?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let refresh_weak = weak.clone();
        let handle = self.menu_handle_with("agent-preset", cx, move |open, _, cx| {
            if open {
                let _ = refresh_weak.update(cx, |this, _| {
                    this.refresh_provider_model_discovery(ProviderKind::DeepSeek);
                });
            }
        });
        let trigger = MenuChip::new("agent-preset")
            .icon("icons/bot.svg", theme.text_tertiary)
            .label(selected_label)
            .caret(false)
            .selected(handle.is_open());

        Some(dropdown_menu(
            trigger,
            "agent-preset-menu",
            &handle,
            MenuAlign::AboveLeft,
            move |_| {
                presets
                    .clone()
                    .into_iter()
                    .map(|preset| {
                        let weak = weak.clone();
                        let preset_id = preset.id.clone();
                        let selected = preset_id == selected_id;
                        let name = if preset.is_custom {
                            format!("{} · {}", preset.display_name(), tr!("agent_preset.custom"))
                        } else {
                            preset.display_name()
                        };
                        let description = preset
                            .display_description()
                            .unwrap_or_else(|| tr!("agent_preset.no_description"))
                            // GPUI wraps at Unicode line-break opportunities,
                            // but an underscored tool name is otherwise one
                            // indivisible word. The zero-width spaces preserve
                            // its visible spelling while allowing the menu to
                            // keep it inside the card.
                            .replace('_', "_\u{200b}");
                        MenuItem::custom(move |_, _| {
                            div()
                                .w(px(340.0))
                                .py(px(5.0))
                                .overflow_hidden()
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .w_full()
                                                .truncate()
                                                .text_size(sp(12.5))
                                                .font_weight(if selected {
                                                    FontWeight::SEMIBOLD
                                                } else {
                                                    FontWeight::MEDIUM
                                                })
                                                .text_color(theme.text)
                                                .child(name.clone()),
                                        )
                                        .child(
                                            div()
                                                .w_full()
                                                .mt(px(2.0))
                                                .text_size(sp(12.5))
                                                .line_height(sp(14.0))
                                                .whitespace_normal()
                                                .overflow_hidden()
                                                .text_color(theme.text_tertiary)
                                                .child(description.clone()),
                                        ),
                                )
                                .when(selected, |element| {
                                    element.child(icon(
                                        "icons/check.svg",
                                        11.0,
                                        theme.text_tertiary,
                                    ))
                                })
                                .into_any_element()
                        })
                        .on_click(move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_agent_preset(preset_id.clone(), cx);
                            });
                        })
                    })
                    .collect()
            },
        ))
    }

    /// The thread-goal chip: present only while the provider reports a goal,
    /// it pairs a target icon with the status phrase (and budget consumption)
    /// and opens the goal dialog. `/goal` is the keyboard route to the same
    /// surface.
    pub(super) fn render_goal_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let session = self.composer_session()?;
        let goal = session.thread_goal.as_ref()?;
        let session_id = session.id;
        let theme = Theme::current(cx);
        let color = super::goal_dialog::goal_status_color(goal.status, &theme);
        // Elapsed pursuit time accrues only while a turn actually runs,
        // matching how the provider accounts it.
        let live_elapsed_seconds = (goal.status == crate::model::ThreadGoalStatus::Active
            && session.is_busy())
        .then(|| self.goal_observed_at.get(&session_id))
        .flatten()
        .map_or(0, |observed| observed.elapsed().as_secs() as i64);
        let label = super::goal_dialog::goal_chip_label(goal, live_elapsed_seconds);
        let objective = goal.objective.clone();
        let weak = cx.entity().downgrade();
        Some(
            div()
                .id("composer-goal")
                .h(px(24.0))
                .px(px(7.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .text_size(sp(12.5))
                .line_height(sp(14.0))
                .text_color(color)
                .child(icon("icons/target.svg", 10.5, color))
                .child(div().max_w(px(220.0)).truncate().child(label))
                .hover(|element| element.bg(theme.overlay))
                .tooltip(Tooltip::text(objective))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.request_goal_dialog(session_id, None, false, cx);
                    });
                })
                .into_any_element(),
        )
    }

    /// The daemon's project-map state for the composer session, as a chip.
    /// Hidden entirely while the experiment emits nothing for the session.
    #[track_caller]
    pub(super) fn render_project_map_control(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session_id = self.composer_session()?.id;
        let status = self.runtimes.get(&session_id)?.project_map.as_ref()?;
        let theme = Theme::current(cx);
        let color = theme.text_tertiary;
        use crate::model::ProjectMapStatus;
        let (label, icon_path, busy) = match status {
            ProjectMapStatus::Building => (
                tr!("project_map.indexing"),
                "icons/loader-circle.svg",
                true,
            ),
            ProjectMapStatus::Ready { indexed_files } => (
                tr!("project_map.ready", files = *indexed_files),
                "icons/projects.svg",
                false,
            ),
            ProjectMapStatus::Refreshing => (
                tr!("project_map.refreshing"),
                "icons/loader-circle.svg",
                true,
            ),
            ProjectMapStatus::Sent {
                mapped_files,
                estimated_tokens,
                ..
            } => (
                tr!(
                    "project_map.sent",
                    files = *mapped_files,
                    tokens = *estimated_tokens
                ),
                "icons/projects.svg",
                false,
            ),
        };
        let glyph = icon(icon_path, 10.5, color);
        Some(
            div()
                .id("composer-project-map")
                .h(px(24.0))
                .px(px(7.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .text_size(sp(12.5))
                .line_height(sp(14.0))
                .text_color(color)
                .child(if busy {
                    motion::spin(glyph)
                } else {
                    glyph.into_any_element()
                })
                .child(div().max_w(px(220.0)).truncate().child(label))
                .into_any_element(),
        )
    }

    /// Stage files dropped onto the composer as attachment chips. The mention
    /// each chip will submit takes the autocomplete's form: relative to the
    /// project root when the file is inside it, absolute otherwise,
    /// directories with a trailing slash.
    pub(super) fn stage_dropped_files(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.stage_attachment_paths(paths.paths(), cx) {
            return;
        }
        let focus = self.composer.read(cx).focus();
        window.focus(&focus, cx);
    }

    /// The session a composer submission would go to right now — Big
    /// Picture's targeted card when the overlay is open, otherwise the
    /// selected session.
    pub(super) fn composer_target_session(&self) -> Option<Uuid> {
        self.big_picture.target().or(self.state.selected_session)
    }

    /// Stage a task dragged from the sidebar as a session-reference chip.
    /// Dropping a task onto the composer that already addresses it is a
    /// no-op, as is dropping one already staged.
    pub(super) fn stage_session_reference(
        &mut self,
        session_id: Uuid,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer_target_session() == Some(session_id)
            || self
                .composer_attachments
                .iter()
                .any(|attachment| attachment.session_id == Some(session_id))
        {
            return;
        }
        self.composer_attachments.push(ComposerAttachment {
            path: PathBuf::new(),
            client_preview_image: None,
            mention: format!("session:{session_id}"),
            name: SharedString::from(title.to_owned()),
            is_dir: false,
            is_image: false,
            blob_reference: None,
            pasted_text_preview: None,
            session_id: Some(session_id),
        });
        self.schedule_composer_draft_save(cx);
        let focus = self.composer.read(cx).focus();
        window.focus(&focus, cx);
        cx.notify();
    }

    fn stage_attachment_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) -> bool {
        if paths.is_empty() {
            return false;
        }
        let paths = paths.to_vec();
        let draft_owner = self.composer_draft_key();
        let daemon = draft_owner.and_then(|key| self.daemon_for_draft_key(key));
        let Some(daemon) = daemon else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return false;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut stored = Vec::with_capacity(paths.len());
                    for source_path in paths {
                        let (name, upload, image_bytes) =
                            attachment_upload_from_path(&source_path)?;
                        let is_image = image_bytes.is_some();
                        let preview_image = image_bytes.and_then(|bytes| {
                            image_preview::image_format_for_name(&name)
                                .map(|format| Arc::new(gpui::Image::from_bytes(format, bytes)))
                        });
                        let response = daemon.client().request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::ImportAttachment { name, upload },
                        )?;
                        let waku_client::ResponsePayload::AttachmentStored { attachment } =
                            response
                        else {
                            anyhow::bail!("the daemon returned an invalid attachment response");
                        };
                        stored.push((attachment, preview_image, is_image));
                    }
                    Ok::<_, anyhow::Error>(stored)
                })
                .await;
            let _ = waku.update(cx, |waku, cx| match result {
                Ok(stored) => {
                    if waku.composer_draft_key() != draft_owner {
                        return;
                    }
                    let mut changed = false;
                    for (attachment, preview_image, is_image) in stored {
                        changed |= waku.stage_daemon_attachment(
                            attachment.path,
                            attachment.name,
                            attachment.is_dir,
                            is_image,
                            attachment.reference,
                            preview_image,
                            None,
                        );
                    }
                    if changed {
                        waku.schedule_composer_draft_save(cx);
                        cx.notify();
                    }
                }
                Err(error) => {
                    waku.show_toast(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
        true
    }

    fn stage_daemon_attachment(
        &mut self,
        path: PathBuf,
        name: String,
        is_dir: bool,
        is_image: bool,
        reference: String,
        client_preview_image: Option<Arc<gpui::Image>>,
        pasted_text_preview: Option<String>,
    ) -> bool {
        if self.composer_attachments.iter().any(|attachment| {
            attachment.path == path
                || attachment.blob_reference.as_deref() == Some(reference.as_str())
        }) {
            return false;
        }
        let mut mention = path.display().to_string();
        if is_dir && !mention.ends_with('/') {
            mention.push('/');
        }
        self.composer_attachments.push(ComposerAttachment {
            path,
            client_preview_image,
            mention,
            name: SharedString::from(name),
            is_dir,
            is_image,
            blob_reference: Some(reference),
            pasted_text_preview,
            session_id: None,
        });
        true
    }

    /// Stage the clipboard's primary image/file representation. On-disk paths
    /// reuse drop handling immediately; raw image bytes are copied into Goddard's
    /// durable blob store on the background executor before their chip appears.
    pub(super) fn stage_pasted_attachments(
        &mut self,
        entries: Vec<ClipboardEntry>,
        cx: &mut Context<Self>,
    ) {
        let mut paths = Vec::new();
        let mut images = Vec::new();
        for entry in entries {
            match entry {
                ClipboardEntry::Image(image) if !image.bytes.is_empty() => images.push(image),
                ClipboardEntry::ExternalPaths(external) => {
                    paths.extend(external.paths().iter().cloned())
                }
                ClipboardEntry::String(_) | ClipboardEntry::Image(_) => {}
            }
        }
        self.stage_attachment_paths(&paths, cx);
        if images.is_empty() {
            return;
        }

        let draft_owner = self.composer_draft_key();
        let daemon = draft_owner.and_then(|key| self.daemon_for_draft_key(key));
        let Some(daemon) = daemon else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        cx.spawn(async move |waku, cx| {
            let stored = cx
                .background_executor()
                .spawn(async move {
                    let image_count = images.len();
                    images
                        .into_iter()
                        .enumerate()
                        .map(|(index, image)| {
                            let preview_image = Arc::new(image);
                            let bytes = preview_image.bytes.clone();
                            let response = daemon
                                .client()
                                .request(
                                    Uuid::nil(),
                                    Uuid::nil(),
                                    waku_client::Command::StoreBlob {
                                        mime_type: preview_image.format.mime_type().to_owned(),
                                        bytes,
                                    },
                                )
                                .map_err(|error| error.to_string())?;
                            let waku_client::ResponsePayload::BlobStored { reference, path } =
                                response
                            else {
                                return Err("the daemon returned an invalid blob response".into());
                            };
                            let extension = path
                                .extension()
                                .and_then(|extension| extension.to_str())
                                .unwrap_or("png");
                            let name = if image_count == 1 {
                                format!("image.{extension}")
                            } else {
                                format!("image-{}.{extension}", index + 1)
                            };
                            Ok::<_, String>((path, name, reference, preview_image))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .await;
            let _ = waku.update(cx, |waku, cx| match stored {
                Ok(stored) => {
                    if waku.composer_draft_key() != draft_owner {
                        return;
                    }
                    let mut staged = false;
                    for (path, name, reference, preview_image) in stored {
                        staged |= waku.stage_daemon_attachment(
                            path,
                            name,
                            false,
                            true,
                            reference,
                            Some(preview_image),
                            None,
                        );
                    }
                    if staged {
                        waku.schedule_composer_draft_save(cx);
                        cx.notify();
                    }
                }
                Err(error) => {
                    waku.show_toast(tr!("errors.store_pasted_image", error = error));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A text paste the field refused to splice. Past
    /// [`PASTED_TEXT_FILE_BYTES`] the text takes the same durable route as a
    /// pasted image — a `.txt` blob that submits as an ordinary file
    /// attachment; under it the paste becomes a collapsible block that still
    /// joins the submission verbatim.
    pub(super) fn stage_pasted_text(&mut self, text: String, cx: &mut Context<Self>) {
        if text.len() <= PASTED_TEXT_FILE_BYTES {
            self.composer_pasted_blocks.push(text);
            self.schedule_composer_draft_save(cx);
            cx.notify();
            return;
        }
        let draft_owner = self.composer_draft_key();
        let daemon = draft_owner.and_then(|key| self.daemon_for_draft_key(key));
        let Some(daemon) = daemon else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        let preview = pasted_text_preview(&text);
        cx.spawn(async move |waku, cx| {
            let stored = cx
                .background_executor()
                .spawn(async move {
                    let response = daemon
                        .client()
                        .request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::StoreBlob {
                                mime_type: "text/plain".to_owned(),
                                bytes: text.into_bytes(),
                            },
                        )
                        .map_err(|error| error.to_string())?;
                    let waku_client::ResponsePayload::BlobStored { reference, path } = response
                    else {
                        return Err("the daemon returned an invalid blob response".into());
                    };
                    Ok::<_, String>((path, reference))
                })
                .await;
            let _ = waku.update(cx, |waku, cx| match stored {
                Ok((path, reference)) => {
                    if waku.selected_composer_draft_key() != draft_owner {
                        return;
                    }
                    if waku.stage_daemon_attachment(
                        path,
                        "paste.txt".to_owned(),
                        false,
                        false,
                        reference,
                        None,
                        Some(preview.clone()),
                    ) {
                        waku.schedule_composer_draft_save(cx);
                        cx.notify();
                    }
                }
                Err(error) => {
                    waku.show_toast(tr!("errors.store_pasted_text", error = error));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Splice a collapsed paste back into the field at the caret, as though
    /// it had never left. The chip is consumed.
    fn expand_pasted_block(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.composer_pasted_blocks.len() {
            return;
        }
        let text = self.composer_pasted_blocks.remove(index);
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        self.composer
            .update(cx, |composer, cx| composer.insert_text(&text, cx));
        self.schedule_composer_draft_save(cx);
        cx.notify();
    }

    fn remove_pasted_block(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.composer_pasted_blocks.len() {
            self.composer_pasted_blocks.remove(index);
            self.schedule_composer_draft_save(cx);
            cx.notify();
        }
    }

    /// The text and attachment presentation accepted from the composer. The
    /// stored prompt keeps its `@` mentions and visible command syntax, while
    /// sent-message UI uses `display_content` and retained attachment metadata.
    pub(super) fn submission_with_attachments(
        &mut self,
        prompt: &str,
        cx: &mut Context<Self>,
    ) -> Option<ComposerSubmission> {
        if self.execute_local_composer_command(prompt, cx) {
            return None;
        }
        // Nothing installed or switched on can run this. Refuse before the
        // draft is consumed, so the text and its attachments survive until a
        // provider is available — every send route lands here, so `enter`,
        // the button, and steering are all covered by this one check.
        if self.model_picker_has_no_providers() {
            return None;
        }
        for attachment in &self.composer_attachments {
            if let (Some(reference), Some(image)) = (
                attachment.blob_reference.as_ref(),
                attachment.client_preview_image.as_ref(),
            ) {
                self.remote_images
                    .borrow_mut()
                    .insert(reference.clone(), RemoteImageState::Ready(image.clone()));
            }
        }
        let attachments = self
            .composer_attachments
            .drain(..)
            .map(MessageAttachment::from)
            .collect::<Vec<_>>();
        let pasted_blocks = std::mem::take(&mut self.composer_pasted_blocks);
        // Typed text leads; collapsed paste blocks follow in paste order,
        // ahead of the attachment tokens `merged_submission` still trails.
        let body = prompt_with_pasted_blocks(prompt, &pasted_blocks);
        let annotations = self.drain_annotations();
        let submission = match merged_submission(&body, &attachments) {
            Some(body) => {
                // Resolve command syntax while the body's leading `/` is
                // still visible — the annotation header would hide it from
                // the transport-boundary resolvers.
                let body = match (annotations.is_empty(), self.composer_session()) {
                    (false, Some(session)) => crate::composer_complete::resolved_submission(
                        session.provider,
                        &body,
                        &self.slash_command_index,
                    )
                    .unwrap_or(body),
                    _ => body,
                };
                format!("{}{}", annotation_prompt_prefix(&annotations), body)
            }
            // Annotations alone still send: their header is the whole prompt.
            None if !annotations.is_empty() => {
                annotation_prompt_prefix(&annotations).trim_end().to_owned()
            }
            None => return None,
        };
        // Presentation splits two ways: the bubble shows each annotation's
        // quote and comment above the typed text, while titles and a restored
        // draft keep the user's own words — the comments when nothing was
        // typed.
        let typed = prompt.trim();
        let human_content = (!annotations.is_empty() || !pasted_blocks.is_empty()).then(|| {
            if typed.is_empty() {
                annotation_display_content(&annotations)
            } else {
                typed.to_owned()
            }
        });
        let display_content = (!attachments.is_empty()
            || !annotations.is_empty()
            || !pasted_blocks.is_empty())
        .then(|| {
            if annotations.is_empty() {
                body.clone()
            } else {
                annotation_bubble_content(&annotations, &body)
            }
        });
        self.discard_current_composer_draft(cx);
        Some(ComposerSubmission {
            prompt: submission,
            display_content,
            human_content,
            attachments,
            pasted_blocks,
            annotations,
            hidden: false,
        })
    }

    pub(super) fn execute_local_composer_command(
        &mut self,
        prompt: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.execute_resume_composer_command(prompt, cx)
            || self.execute_land_composer_command(prompt, cx)
            || self.execute_compact_composer_command(prompt, cx)
            || self.execute_fast_mode_toggle(prompt, cx)
            || self.execute_goal_composer_command(prompt, cx)
    }

    fn execute_resume_composer_command(&mut self, prompt: &str, cx: &mut Context<Self>) -> bool {
        if !crate::composer_complete::is_resume_submission(prompt) {
            return false;
        }
        self.composer.update(cx, |input, cx| input.clear(cx));
        // Submission notifications already hold this entity mutably. Dispatch
        // after that effect returns so the window action can safely re-enter
        // Waku and move focus into the Resume picker.
        cx.defer(|cx| cx.dispatch_action(&OpenResumePicker));
        true
    }

    /// `/land` — the same operation the Git panel's land button runs. Unlike
    /// `/resume` this starts no window action, so it can run inline; errors
    /// and the conflict modal surface without the panel open.
    fn execute_land_composer_command(&mut self, prompt: &str, cx: &mut Context<Self>) -> bool {
        if !crate::composer_complete::is_land_submission(prompt) {
            return false;
        }
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.land_composer_session(waku_client::git::PullStrategy::Rebase, cx);
        true
    }

    /// `/compact` — asks the provider to compact the session's context
    /// through the daemon's control path. No user message is written: the
    /// compaction activity card is the transcript record.
    fn execute_compact_composer_command(&mut self, prompt: &str, cx: &mut Context<Self>) -> bool {
        if !crate::composer_complete::is_compact_submission(prompt, &self.slash_command_index) {
            return false;
        }
        let Some(session_id) = self.composer_session_id() else {
            return false;
        };
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.compact_session(session_id, cx);
        true
    }

    /// Ask the provider to compact `session_id`'s context — the shared entry
    /// point for `/compact`, the usage panel's compact action, and the
    /// command palette. Compaction is never queued behind live work and
    /// there is nothing to shrink on a session that has not started, so both
    /// cases refuse with a toast rather than reaching the provider.
    pub(super) fn compact_session(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if session.is_busy() {
            self.show_toast(tr!("commands.compact_turn_running"));
            return;
        }
        if !session.has_started() {
            self.show_toast(tr!("commands.compact_nothing_to_compact"));
            return;
        }
        // Interception already proved a path exists; the affordances gate on
        // the same check, so this only guards a stale catalog. The reserved
        // providers count unconditionally — their RPC exists whether or not
        // discovery has landed yet.
        if !session.provider.supports_compact()
            && !crate::composer_complete::has_compact_path(&self.slash_command_index)
        {
            return;
        }
        let Some(runtime) = self.runtimes.get(&session_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            return;
        };
        runtime.driver.compact();
        cx.notify();
    }

    /// Bridge Codex's native `/goal` command without starting a turn. Reads run
    /// against the session's cached goal; mutations go to the app-server and
    /// echo back as `GoalUpdated` events.
    fn execute_goal_composer_command(&mut self, prompt: &str, cx: &mut Context<Self>) -> bool {
        use crate::composer_complete::GoalCommand;
        use crate::model::{GoalOperation, ThreadGoalStatus};
        let Some((session_id, command, current_goal)) =
            self.composer_session().and_then(|session| {
                let command = crate::composer_complete::parse_goal_submission(
                    session.provider,
                    prompt,
                    &self.slash_command_index,
                )?;
                Some((session.id, command, session.thread_goal.clone()))
            })
        else {
            return false;
        };
        match command {
            GoalCommand::Show | GoalCommand::Edit => {
                self.request_goal_dialog(session_id, None, false, cx);
            }
            GoalCommand::Pause => {
                self.dispatch_goal_operation(
                    session_id,
                    GoalOperation::Set {
                        objective: None,
                        status: Some(ThreadGoalStatus::Paused),
                        replace: false,
                    },
                    cx,
                );
            }
            GoalCommand::Resume => {
                self.dispatch_goal_operation(
                    session_id,
                    GoalOperation::Set {
                        objective: None,
                        status: Some(ThreadGoalStatus::Active),
                        replace: false,
                    },
                    cx,
                );
            }
            GoalCommand::Clear => {
                self.dispatch_goal_operation(session_id, GoalOperation::Clear, cx);
            }
            GoalCommand::Set(objective) => match &current_goal {
                // Replacing unfinished work needs a look at what it replaces;
                // the dialog carries the confirmation.
                Some(goal) if !goal.status.is_terminal() => {
                    self.request_goal_dialog(session_id, Some(objective), true, cx);
                }
                Some(_) | None => {
                    self.dispatch_goal_operation(
                        session_id,
                        GoalOperation::Set {
                            objective: Some(objective),
                            status: Some(ThreadGoalStatus::Active),
                            replace: current_goal.is_some(),
                        },
                        cx,
                    );
                }
            },
        }
        self.composer.update(cx, |input, cx| input.clear(cx));
        cx.notify();
        true
    }

    fn execute_fast_mode_toggle(&mut self, prompt: &str, cx: &mut Context<Self>) -> bool {
        let Some(next_tier) = self.composer_session().and_then(|session| {
            if !crate::composer_complete::is_fast_mode_toggle_submission(
                session.provider,
                prompt,
                &self.slash_command_index,
            ) {
                return None;
            }
            let model = self.model_metadata_for_session(session)?;
            crate::composer_complete::toggled_fast_service_tier(
                session.service_tier.as_deref(),
                &model.service_tiers,
            )
        }) else {
            return false;
        };
        let enabled = next_tier != "default";
        // Clearing emits an Edited event. Apply the tier afterward so any
        // draft refresh caused by that event cannot repaint the old choice.
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.set_service_tier(next_tier, cx);
        self.show_success_toast(tr!(if enabled {
            "commands.fast_enabled"
        } else {
            "commands.fast_disabled"
        }));
        true
    }

    pub(super) fn restore_composer_submission(
        &mut self,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        if submission.hidden {
            // The continue nudge was never the user's draft; a failed send
            // leaves the composer as it was rather than revealing it.
            return;
        }
        self.composer_attachments = submission
            .attachments
            .into_iter()
            .map(ComposerAttachment::from)
            .collect();
        self.composer_pasted_blocks = submission.pasted_blocks;
        if !submission.annotations.is_empty() {
            // The drain consumed the highlights; hand them back so the
            // restored draft still carries its comments — file annotations
            // return to their editors, the rest to the transcript store. A
            // file whose editor is gone parks in `pending_file_annotations`
            // until it opens again.
            let (file_annotations, transcript_annotations): (Vec<_>, Vec<_>) = submission
                .annotations
                .into_iter()
                .partition(|annotation| annotation.file.is_some());
            self.transcript_selection
                .annotations
                .borrow_mut()
                .items
                .extend(transcript_annotations);
            for annotation in file_annotations {
                let Some(file) = &annotation.file else {
                    continue;
                };
                match self.right_panel_file_editors.get_mut(&file.path) {
                    Some(editor) => editor.annotations.borrow_mut().items.push(annotation),
                    None => self
                        .pending_file_annotations
                        .entry(file.path.clone())
                        .or_default()
                        .push(annotation),
                }
            }
        }
        let content = submission
            .human_content
            .or(submission.display_content)
            .unwrap_or(submission.prompt);
        self.composer
            .update(cx, |input, cx| input.set_content(content, cx));
        self.schedule_composer_draft_save(cx);
        cx.notify();
    }

    /// Collapsed paste blocks above the input: one compact "Pasted text" chip
    /// per block — hovering shows the paste's leading characters, activating
    /// splices it back into the field at the caret.
    pub(super) fn render_pasted_blocks(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let mut row = div()
            .px(px(14.0))
            .pt(px(2.0))
            .pb(px(8.0))
            .flex()
            .flex_wrap()
            .gap(px(8.0));
        for (index, block) in self.composer_pasted_blocks.iter().enumerate() {
            let preview = SharedString::from(pasted_text_preview(block));
            let chip = div()
                .id(SharedString::from(format!("composer-pasted-block-{index}")))
                .h(px(24.0))
                .pl(px(6.0))
                .pr(px(4.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.inset)
                .flex()
                .items_center()
                .gap(px(4.0))
                .cursor_default()
                .tab_index(0)
                .focus_visible(|style| style.border_color(theme.accent))
                .when(!preview.is_empty(), |element| {
                    element.tooltip(pasted_text_tooltip(preview.clone()))
                })
                .child(icon("icons/file.svg", 11.0, theme.text_tertiary))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(tr!("composer.pasted_block")),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "composer-pasted-block-remove-{index}"
                        )))
                        .w(px(16.0))
                        .h(px(16.0))
                        .flex_none()
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_default()
                        .tab_index(0)
                        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                        .hover(|element| element.bg(theme.overlay_strong))
                        .active(|element| element.opacity(0.8))
                        .child(icon("icons/x.svg", 9.0, theme.text_secondary))
                        .tooltip(Tooltip::text(tr!("composer.remove_pasted_block")))
                        .on_activation(cx, move |this, _, cx| {
                            this.remove_pasted_block(index, cx);
                        }),
                )
                .on_activation(cx, move |this, window, cx| {
                    this.expand_pasted_block(index, window, cx);
                })
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "backspace" | "delete") {
                        this.remove_pasted_block(index, cx);
                        cx.stop_propagation();
                    }
                }));
            row = row.child(chip);
        }
        row
    }

    /// The staged-attachment chips above the input: a thumbnail tile per
    /// image, a file-type icon and basename for everything else, each with a
    /// floating remove button — T3 Code's attachment row in graphite.
    pub(super) fn render_composer_attachments(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let mut row = div()
            .px(px(14.0))
            .pt(px(2.0))
            .pb(px(8.0))
            .flex()
            .flex_wrap()
            .gap(px(8.0));
        for (index, attachment) in self.composer_attachments.iter().enumerate() {
            if let Some(session_id) = attachment.session_id {
                row = row
                    .child(self.render_session_attachment_chip(index, session_id, attachment, cx));
                continue;
            }
            let menu = self.menu_handle(format!("composer-attachment-{index}-menu"), cx);
            if attachment.pasted_text_preview.is_some() {
                row = row
                    .child(self.render_pasted_text_attachment_chip(index, attachment, &menu, cx));
                continue;
            }
            let icon_path = if attachment.is_dir {
                "icons/folder.svg"
            } else {
                super::right_panel::file_icon_for_path(&attachment.mention)
            };
            let mut tile = div()
                .id(SharedString::from(format!("composer-attachment-{index}")))
                .relative()
                .w(px(64.0))
                .h(px(64.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.inset)
                .track_focus(menu.trigger_focus_handle())
                .tab_index(0)
                .focus_visible(|style| style.border_color(theme.accent))
                .tooltip(Tooltip::text(format!("@{}", attachment.mention)));
            let attachment_image = attachment.client_preview_image.clone().or_else(|| {
                attachment
                    .is_image
                    .then(|| {
                        attachment.blob_reference.as_deref().and_then(|reference| {
                            self.image_for_reference(
                                reference,
                                Some(&attachment.path),
                                Some(attachment.name.as_ref()),
                                cx,
                            )
                        })
                    })
                    .flatten()
            });
            // Reveal acts on this Mac's filesystem; a remote host's staged
            // path means nothing to Finder.
            let can_reveal = !self.is_remote_path(&attachment.path);
            if attachment.is_image {
                if let Some(attachment_image) = attachment_image.as_ref() {
                    let preview_image = attachment_image.clone();
                    let preview_name = attachment.name.clone();
                    tile = tile.child(
                        div()
                            .id(SharedString::from(format!(
                                "composer-attachment-{index}-preview"
                            )))
                            .size_full()
                            .cursor_default()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_image_preview(
                                    preview_image.clone(),
                                    preview_name.clone(),
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            }))
                            .child(
                                img(attachment_image.clone())
                                    .size_full()
                                    .object_fit(ObjectFit::Cover),
                            ),
                    );
                } else {
                    tile = tile.child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/file-types/image.svg", 16.0, theme.text_ghost)),
                    );
                }
            } else {
                tile = tile.child(
                    div()
                        .size_full()
                        .px(px(5.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(5.0))
                        .child(icon(icon_path, 16.0, theme.text_tertiary))
                        .child(
                            div().w_full().flex().justify_center().child(
                                div()
                                    .max_w_full()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .child(attachment.name.clone()),
                            ),
                        ),
                );
            }
            let key_menu = menu.clone();
            let key_image = attachment_image.clone();
            let key_name = attachment.name.clone();
            let is_image = attachment.is_image;
            tile = tile.on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if is_image
                    && matches!(key, "enter" | "space")
                    && let Some(key_image) = key_image.as_ref()
                {
                    this.open_image_preview(key_image.clone(), key_name.clone(), window, cx);
                    cx.stop_propagation();
                } else if key == "f10" && event.keystroke.modifiers.shift {
                    key_menu.open_context_menu(window, cx);
                    cx.stop_propagation();
                }
            }));
            let tile = tile.child(
                div()
                    .id(SharedString::from(format!(
                        "composer-attachment-remove-{index}"
                    )))
                    .absolute()
                    .top(px(3.0))
                    .right(px(3.0))
                    .w(px(16.0))
                    .h(px(16.0))
                    .tab_index(0)
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .bg(theme.canvas.opacity(0.8))
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .hover(|element| element.bg(theme.canvas.opacity(0.95)))
                    .active(|element| element.opacity(0.8))
                    .child(icon("icons/x.svg", 9.0, theme.text_secondary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if index < this.composer_attachments.len() {
                            this.composer_attachments.remove(index);
                            this.schedule_composer_draft_save(cx);
                            cx.notify();
                        }
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if index < this.composer_attachments.len() {
                                this.composer_attachments.remove(index);
                                this.schedule_composer_draft_save(cx);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }
                    })),
            );
            let reveal_path = attachment.path.clone();
            row = row.child(context_menu(
                tile,
                SharedString::from(format!("composer-attachment-{index}-context-menu")),
                &menu,
                move |_| image_preview::attachment_menu_items(reveal_path.clone(), can_reveal),
            ));
        }
        row
    }

    /// A paste too large to stay inline, stored as a durable `.txt` blob: the
    /// same compact "Pasted text" chip a collapsed block gets, with the
    /// paste's leading characters on hover and the reveal menu behind it.
    fn render_pasted_text_attachment_chip(
        &self,
        index: usize,
        attachment: &ComposerAttachment,
        menu: &ContextMenuHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let preview = attachment
            .pasted_text_preview
            .as_ref()
            .filter(|preview| !preview.is_empty())
            .map(|preview| SharedString::from(preview.clone()));
        let key_menu = menu.clone();
        let chip = div()
            .id(SharedString::from(format!("composer-attachment-{index}")))
            .h(px(24.0))
            .pl(px(6.0))
            .pr(px(4.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.inset)
            .flex()
            .items_center()
            .gap(px(4.0))
            .track_focus(menu.trigger_focus_handle())
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.accent))
            .when_some(preview, |element, preview| {
                element.tooltip(pasted_text_tooltip(preview))
            })
            .child(icon("icons/file.svg", 11.0, theme.text_tertiary))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .child(tr!("composer.pasted_block")),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "composer-attachment-remove-{index}"
                    )))
                    .w(px(16.0))
                    .h(px(16.0))
                    .flex_none()
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .tab_index(0)
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .hover(|element| element.bg(theme.overlay_strong))
                    .active(|element| element.opacity(0.8))
                    .child(icon("icons/x.svg", 9.0, theme.text_secondary))
                    .tooltip(Tooltip::text(tr!("composer.remove_pasted_block")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if index < this.composer_attachments.len() {
                            this.composer_attachments.remove(index);
                            this.schedule_composer_draft_save(cx);
                            cx.notify();
                        }
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if index < this.composer_attachments.len() {
                                this.composer_attachments.remove(index);
                                this.schedule_composer_draft_save(cx);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }
                    })),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if matches!(key, "backspace" | "delete") {
                    if index < this.composer_attachments.len() {
                        this.composer_attachments.remove(index);
                        this.schedule_composer_draft_save(cx);
                        cx.notify();
                    }
                    cx.stop_propagation();
                } else if key == "f10" && event.keystroke.modifiers.shift {
                    key_menu.open_context_menu(window, cx);
                    cx.stop_propagation();
                }
            }));
        let reveal_path = attachment.path.clone();
        let can_reveal = !self.is_remote_path(&attachment.path);
        context_menu(
            chip,
            SharedString::from(format!("composer-attachment-{index}-context-menu")),
            menu,
            move |_| image_preview::attachment_menu_items(reveal_path.clone(), can_reveal),
        )
    }

    /// The chip a dragged-in task gets: a chat bubble and the session title,
    /// sized inline with the text rather than the file tiles' 64px grid.
    /// Enter opens the task; backspace/delete unstages it.
    fn render_session_attachment_chip(
        &self,
        index: usize,
        session_id: Uuid,
        attachment: &ComposerAttachment,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let focus = self
            .menu_handle(format!("composer-attachment-{index}-menu"), cx)
            .trigger_focus_handle()
            .clone();
        div()
            .id(SharedString::from(format!("composer-attachment-{index}")))
            .h(px(24.0))
            .max_w(px(240.0))
            .pl(px(6.0))
            .pr(px(4.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.inset)
            .flex()
            .items_center()
            .gap(px(4.0))
            .track_focus(&focus)
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.accent))
            .tooltip(Tooltip::text(format!("{} — {session_id}", attachment.name)))
            .child(icon("icons/chat.svg", 11.0, theme.text_tertiary))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .child(attachment.name.clone()),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "composer-attachment-remove-{index}"
                    )))
                    .w(px(16.0))
                    .h(px(16.0))
                    .flex_none()
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .tab_index(0)
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .hover(|element| element.bg(theme.overlay_strong))
                    .active(|element| element.opacity(0.8))
                    .child(icon("icons/x.svg", 9.0, theme.text_secondary))
                    .tooltip(Tooltip::text(tr!("common.remove")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if index < this.composer_attachments.len() {
                            this.composer_attachments.remove(index);
                            this.schedule_composer_draft_save(cx);
                            cx.notify();
                        }
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if index < this.composer_attachments.len() {
                                this.composer_attachments.remove(index);
                                this.schedule_composer_draft_save(cx);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }
                    })),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if matches!(key, "backspace" | "delete") {
                    if index < this.composer_attachments.len() {
                        this.composer_attachments.remove(index);
                        this.schedule_composer_draft_save(cx);
                        cx.notify();
                    }
                    cx.stop_propagation();
                } else if matches!(key, "enter" | "space") {
                    this.select_session(session_id, cx);
                    cx.stop_propagation();
                }
            }))
    }

    /// The pending follow-up queue between the transcript and the composer: a
    /// single card tucked against the composer's top edge, one row per queued
    /// message. A row pulls its text back into the composer on click and
    /// carries steer/remove/more controls on the right.
    pub(super) fn render_queued_messages(&self, cx: &mut Context<Self>) -> Option<Div> {
        let session_id = self.state.selected_session?;
        let session = self.selected_session()?;
        if session.queued_messages.iter().all(|message| message.hidden) {
            return None;
        }
        let theme = Theme::current(cx);
        let steerable = self.session_can_steer(session);
        let mut list = div().flex().flex_col().py(px(4.0));
        for (index, message) in session.queued_messages.iter().enumerate() {
            if message.hidden {
                continue;
            }
            let message_id = message.id;
            let content = if message.visible_content().trim().is_empty() {
                message
                    .attachments
                    .iter()
                    .map(|attachment| attachment.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                message.visible_content().to_owned()
            };
            let steer_control = steerable.then(|| {
                div()
                    .id(SharedString::from(format!(
                        "queued-message-steer-{message_id}"
                    )))
                    .h(px(24.0))
                    .px(px(7.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .cursor_default()
                    .tab_index(0)
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .hover(|element| element.bg(theme.overlay_strong))
                    .active(|element| element.opacity(0.8))
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .child(icon(
                        "icons/corner-down-right.svg",
                        11.0,
                        theme.text_secondary,
                    ))
                    .child(tr!("composer.steer"))
                    .tooltip(Tooltip::text_with_action(
                        tr!("composer.steer_current"),
                        &crate::input::SubmitSteer,
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.steer_queued_message(session_id, message_id, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.steer_queued_message(session_id, message_id, cx);
                            cx.stop_propagation();
                        }
                    }))
            });
            let menu_handle = self.menu_handle(format!("queued-message-menu-{message_id}"), cx);
            let menu_open = menu_handle.is_open();
            let weak = cx.entity().downgrade();
            let more_control = dropdown_menu(
                div()
                    .id(SharedString::from(format!(
                        "queued-message-more-{message_id}"
                    )))
                    .w(px(24.0))
                    .h(px(24.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .when(menu_open, |element| element.bg(theme.overlay_strong))
                    .hover(|element| element.bg(theme.overlay_strong))
                    .active(|element| element.opacity(0.8))
                    .child(icon("icons/ellipsis.svg", 12.5, theme.text_secondary)),
                SharedString::from(format!("queued-message-more-menu-{message_id}")),
                &menu_handle,
                MenuAlign::BelowRight,
                move |_| {
                    let edit_weak = weak.clone();
                    let remove_weak = weak.clone();
                    vec![
                        MenuItem::new(tr!("composer.edit_in_composer"), move |window, cx| {
                            let _ = edit_weak.update(cx, |this, cx| {
                                this.edit_queued_message(session_id, message_id, window, cx);
                            });
                        })
                        .icon("icons/pencil.svg"),
                        MenuItem::new(tr!("composer.remove_followup"), move |_, cx| {
                            let _ = remove_weak.update(cx, |this, cx| {
                                this.remove_queued_message(session_id, message_id, cx);
                            });
                        })
                        .icon("icons/trash.svg"),
                    ]
                },
            );
            list = list.child(
                div()
                    .id(SharedString::from(format!("queued-message-{message_id}")))
                    .min_h(px(30.0))
                    .overflow_hidden()
                    .when(index > 0, |row| {
                        row.border_t(hairline()).border_color(theme.separator)
                    })
                    .pl(px(12.0))
                    .pr(px(6.0))
                    .flex()
                    .items_start()
                    .gap(px(9.0))
                    .cursor_default()
                    .tab_index(0)
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .hover(|element| element.bg(theme.overlay))
                    .tooltip(Tooltip::text(tr!("composer.edit_in_composer")))
                    .child(div().h(px(30.0)).flex().items_center().child(icon(
                        "icons/queue.svg",
                        12.0,
                        theme.text_tertiary,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            // Three lines is enough to recognise a prompt without
                            // the queue becoming a second transcript.
                            .py(px(6.0))
                            .line_height(sp(18.0))
                            .line_clamp(3)
                            .text_ellipsis()
                            .text_size(sp(12.5))
                            .text_color(theme.text)
                            .child(SharedString::from(content)),
                    )
                    .child(
                        div()
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .children(steer_control)
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "queued-message-remove-{message_id}"
                                    )))
                                    .w(px(24.0))
                                    .h(px(24.0))
                                    .rounded(px(8.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_default()
                                    .tab_index(0)
                                    .focus_visible(|style| {
                                        style.border(hairline()).border_color(theme.accent)
                                    })
                                    .hover(|element| element.bg(theme.overlay_strong))
                                    .active(|element| element.opacity(0.8))
                                    .child(icon("icons/trash.svg", 12.0, theme.text_secondary))
                                    .tooltip(Tooltip::text(tr!("composer.remove_followup")))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.remove_queued_message(session_id, message_id, cx);
                                    }))
                                    .on_key_down(cx.listener(
                                        move |this, event: &KeyDownEvent, _, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                this.remove_queued_message(
                                                    session_id, message_id, cx,
                                                );
                                                cx.stop_propagation();
                                            }
                                        },
                                    )),
                            )
                            .child(more_control),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.edit_queued_message(session_id, message_id, window, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.edit_queued_message(session_id, message_id, window, cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }
        Some(
            div().flex_none().px(px(20.0 - COMPOSER_OVERHANG)).child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0))
                    .mx_auto()
                    .px(px(14.0))
                    .child(
                        div()
                            .rounded_tl(px(15.0))
                            .rounded_tr(px(15.0))
                            .border_t(hairline())
                            .border_l(hairline())
                            .border_r(hairline())
                            .border_color(theme.separator)
                            .bg(theme.composer)
                            // Row hover fills are full-width rectangles; clip
                            // them to the card's rounded corners.
                            .overflow_hidden()
                            .child(list),
                    ),
            ),
        )
    }

    /// The session the composer's submit affordances answer to: the big-
    /// picture target while the overlay is open — `None` there means the next
    /// prompt starts a fresh task — `None` while the Projects page is up for
    /// the same reason, and the selected session everywhere else.
    pub(super) fn composer_session(&self) -> Option<&AgentSession> {
        if self.big_picture.is_open() {
            return self
                .big_picture
                .target()
                .and_then(|id| self.state.sessions.iter().find(|session| session.id == id));
        }
        // The Projects page binds the project's draft session, so the
        // composer answers to it the same way it does on a chat.
        self.selected_session()
    }

    /// `composer_session` as a bare id, for commands that only need to know
    /// which session the composer answers to.
    pub(super) fn composer_session_id(&self) -> Option<Uuid> {
        self.composer_session().map(|session| session.id)
    }

    /// The mutable counterpart of [`Self::composer_session`]: writes land on
    /// the armed card's session while the overlay is open — and on no session
    /// at all when nothing is armed — the selected session otherwise.
    pub(super) fn composer_session_mut(&mut self) -> Option<&mut AgentSession> {
        let id = if self.big_picture.is_open() {
            self.big_picture.target()?
        } else {
            self.state.selected_session?
        };
        self.state.session_mut(id)
    }

    /// The session and project the workspace footer's chips describe and its
    /// pickers configure. Outside Big Picture both come from the selection;
    /// while the overlay is open they follow the composer — the armed card's
    /// session and project, or the standing new-task destination: that
    /// project's unstarted draft when one already exists, and no session at
    /// all before a workspace choice materializes one.
    pub(super) fn workspace_subject(&self) -> (Option<Uuid>, Option<Uuid>) {
        workspace_subject_for(
            self.big_picture.is_open(),
            self.big_picture.target(),
            self.state.selected_session,
            self.state.selected_project,
            self.big_picture.new_task_project,
            &self.state.sessions,
        )
    }

    /// The subject's session, when one exists yet.
    pub(super) fn workspace_subject_session(&self) -> Option<&AgentSession> {
        let (session_id, _) = self.workspace_subject();
        session_id.and_then(|session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
        })
    }

    /// The directory the subject's workspace resolves to — the session's
    /// materialized worktree, else its project's ordinary checkout. `None`
    /// only when the subject names no project at all.
    pub(super) fn workspace_subject_path(&self) -> Option<PathBuf> {
        let (session_id, project_id) = self.workspace_subject();
        let session = session_id.and_then(|session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
        });
        let project = project_id.and_then(|project_id| {
            self.state
                .projects
                .iter()
                .find(|project| project.id == project_id)
        });
        session
            .and_then(|session| self.workspace_path_for_session(session))
            .or_else(|| project.map(|project| project.path.as_path()))
            .map(std::path::Path::to_path_buf)
    }

    /// The workspace the chips should display: the subject session's own,
    /// or — for the overlay's untargeted composer before its draft exists —
    /// the workspace a fresh task in the destination project would open
    /// with.
    pub(super) fn workspace_subject_workspace(&self) -> Option<SessionWorkspace> {
        if let Some(session) = self.workspace_subject_session() {
            return Some(session.workspace.clone());
        }
        let (_, project_id) = self.workspace_subject();
        project_id.map(|project_id| self.state.workspace_for_new_session(project_id))
    }

    /// A submit click goes where Enter would: the overlay's own routing while
    /// Big Picture is open, the page's task creation while the Projects page
    /// is open, the selected session otherwise.
    pub(super) fn route_composer_submission(
        &mut self,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture.is_open() {
            self.submit_big_picture_submission(submission, cx);
        } else if self.projects_page.is_some() {
            self.submit_projects_page_submission(submission, "", cx);
        } else {
            self.submit_composer_submission(submission, cx);
        }
    }

    /// The composer slot for a quarantined received-file session: a card
    /// explaining the boundary plus the Trust action that clears it.
    fn render_quarantine_card(&self, session_id: Uuid, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        div()
            .flex_none()
            .px(px(20.0 - COMPOSER_OVERHANG))
            .child(
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
                    .child(icon("icons/lock.svg", 14.0, theme.text_tertiary))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text)
                                    .child(tr!("friends.quarantined_title")),
                            )
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(tr!("friends.quarantined_hint")),
                            ),
                    )
                    .child(
                        div()
                            .id("trust-transfer")
                            .tab_index(0)
                            .focus_visible(|style| {
                                style.border(hairline()).border_color(theme.accent)
                            })
                            .h(px(26.0))
                            .px(px(10.0))
                            .flex_none()
                            .rounded(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_default()
                            .bg(theme.inverse)
                            .hover(|element| element.bg(theme.inverse.opacity(0.85)))
                            .child(icon("icons/lock-open.svg", 12.0, theme.on_inverse))
                            .child(
                                div()
                                    .text_size(sp(12.0))
                                    .text_color(theme.on_inverse)
                                    .child(tr!("friends.trust")),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.trust_transfer_session(session_id, cx);
                            }))
                            .on_key_down(cx.listener(
                                move |this, event: &KeyDownEvent, _window, cx| {
                                    if !event.keystroke.modifiers.modified()
                                        && matches!(
                                            event.keystroke.key.as_str(),
                                            "enter" | "space"
                                        )
                                    {
                                        this.trust_transfer_session(session_id, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            )),
                    ),
            )
    }

    pub(super) fn render_composer(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let session = self.composer_session();
        // A received-file session stays quarantined until the user trusts
        // it — the prompt field is replaced by a trust card so Enter can't
        // start the agent on untrusted files. The daemon refuses anyway;
        // this is the legible boundary.
        if let Some(session) =
            session.filter(|session| session.quarantined && session.detail_loaded)
        {
            return self.render_quarantine_card(session.id, cx);
        }
        let session_id = session.map(|session| session.id);
        let preparing = session.is_some_and(|session| {
            self.submission_preparations.contains(&session.id)
                || self.response_fork_preparations.contains_key(&session.id)
        });
        let has_draft = !self.composer.read(cx).content(cx).trim().is_empty()
            || !self.composer_attachments.is_empty()
            || !self.composer_pasted_blocks.is_empty()
            || !self
                .transcript_selection
                .annotations
                .borrow()
                .items
                .is_empty();
        // A typed draft always means Send — the continue affordance exists
        // only while the composer is completely empty.
        let submit_action = composer_submit_action(session, preparing, has_draft);
        let escape_stop_armed = session.is_some_and(|session| {
            self.escape_stop_confirmation
                .is_armed_for(EscapeStopTarget::for_session(session), Instant::now())
        });
        // With no provider to run it, a draft has nowhere to go. The button
        // reads as unavailable and the submission path refuses too, so
        // `enter` cannot slip past a disabled control.
        let no_providers = self.model_picker_has_no_providers();
        let can_send = has_draft && !no_providers;
        // Continue needs no draft — an interrupted session is exactly what
        // makes it available — but it still needs a provider to run.
        let can_continue = !no_providers;
        let (autocomplete, autocomplete_actionable) =
            match self.render_composer_autocomplete(window, cx) {
                Some((element, actionable)) => (Some(element), actionable),
                None => (None, false),
            };
        let autocomplete_loading = autocomplete.is_some() && !autocomplete_actionable;
        // Files dragged in from the OS light the card up as a drop target and
        // stage as attachment chips. The wash arrives pre-blended because a
        // drag-over refinement replaces the card's fill rather than
        // compositing over it.
        let drop_wash = theme.composer.blend(theme.overlay_strong);
        let drop_ring = theme.accent.opacity(0.7);
        div().flex_none().px(px(20.0 - COMPOSER_OVERHANG)).child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0))
                .mx_auto()
                .rounded(px(18.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.composer)
                // Horizontal insets live on each row (and inside the field's
                // scroll viewport, via `padding_x`) rather than on the card,
                // so the field's overlay scrollbar can hug the card's edge.
                .py(px(10.0))
                .drag_over::<ExternalPaths>(move |style, _, _, _| {
                    style.bg(drop_wash).border_color(drop_ring)
                })
                .drag_over::<SidebarSessionDrag>(move |style, _, _, _| {
                    style.bg(drop_wash).border_color(drop_ring)
                })
                // The same highlight when the drag is anywhere over the
                // session column — the card is where the chips will land.
                .group_drag_over::<ExternalPaths>(SESSION_DROP_GROUP, move |style| {
                    style.bg(drop_wash).border_color(drop_ring)
                })
                .group_drag_over::<SidebarSessionDrag>(SESSION_DROP_GROUP, move |style| {
                    style.bg(drop_wash).border_color(drop_ring)
                })
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    this.stage_dropped_files(paths, window, cx);
                }))
                .on_drop(cx.listener(|this, drag: &SidebarSessionDrag, window, cx| {
                    this.stage_session_reference(drag.session_id, &drag.title, window, cx);
                }))
                // Anchor for the bounds probe the autocomplete popup aligns to.
                .relative()
                .child(super::autocomplete::composer_card_bounds_probe(
                    self.composer_autocomplete.card_bounds_cell(),
                ))
                // Only while the popup has selectable rows: the key context
                // routes arrows, `enter`, `tab` and `escape` here as actions,
                // out from under the focused field. The loading state takes
                // only Escape, so it can dismiss without swallowing input.
                .when(autocomplete_actionable, |card| {
                    card.key_context("ComposerAutocomplete")
                        .on_action(cx.listener(|this, _: &SelectNextEntry, window, cx| {
                            this.move_autocomplete_highlight("down", window, cx);
                        }))
                        .on_action(cx.listener(|this, _: &SelectPreviousEntry, window, cx| {
                            this.move_autocomplete_highlight("up", window, cx);
                        }))
                        .on_action(cx.listener(|this, _: &ConfirmEntry, window, cx| {
                            this.accept_autocomplete(None, window, cx);
                        }))
                        .on_action(cx.listener(|this, _: &DismissMenu, _, cx| {
                            this.dismiss_autocomplete(cx);
                        }))
                })
                .when(autocomplete_loading, |card| {
                    card.key_context("ComposerAutocompleteLoading")
                        .on_action(cx.listener(|this, _: &DismissMenu, _, cx| {
                            this.dismiss_autocomplete(cx);
                        }))
                })
                .children(autocomplete)
                // Big Picture's "replying to" chip; absent everywhere else.
                .children(self.render_big_picture_target_chip(cx))
                .when(!self.composer_pasted_blocks.is_empty(), |card| {
                    card.child(self.render_pasted_blocks(cx))
                })
                .when(!self.composer_attachments.is_empty(), |card| {
                    card.child(self.render_composer_attachments(cx))
                })
                .children(self.render_annotation_chip(cx))
                .child(div().pt(px(2.0)).child(self.composer.clone()))
                .child(
                    div()
                        .mt(px(8.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .text_size(sp(12.5))
                        .line_height(sp(14.0))
                        .child(self.render_provider_model_control(cx))
                        .children(self.render_model_traits_control(cx))
                        .children(self.render_agent_preset_control(cx))
                        .child(self.render_access_control(cx))
                        .children(self.render_drafts_count_button(cx))
                        .children(self.render_goal_control(cx))
                        .children(self.render_project_map_control(cx))
                        .child(div().flex_1())
                        .child(match submit_action {
                            ComposerSubmitAction::Preparing => div()
                                .id("send-or-stop")
                                .w(px(28.0))
                                .h(px(28.0))
                                .flex_none()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_default()
                                .bg(theme.overlay_strong)
                                .child(motion::spin(icon(
                                    "icons/loader-circle.svg",
                                    15.0,
                                    theme.text_secondary,
                                )))
                                .tooltip(Tooltip::text(tr!("composer.preparing_task"))),
                            ComposerSubmitAction::Stop => div()
                                .id("working-actions")
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .id("send-or-stop")
                                        .w(px(28.0))
                                        .h(px(28.0))
                                        .flex_none()
                                        .rounded_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_default()
                                        .bg(theme.overlay_strong)
                                        .hover(|element| element.bg(theme.danger_soft))
                                        .active(|element| element.opacity(0.8))
                                        .when(escape_stop_armed, |element| {
                                            element.child(
                                                div()
                                                    .text_size(sp(12.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme.text)
                                                    .child("Esc"),
                                            )
                                        })
                                        .when(!escape_stop_armed, |element| {
                                            element.child(icon("icons/stop.svg", 18.0, theme.text))
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(session_id) = session_id {
                                                this.cancel_session_turn(session_id, cx);
                                            }
                                        })),
                                )
                                .when(can_send, |element| {
                                    element.child(
                                        div()
                                            .id("queue-follow-up")
                                            .w(px(28.0))
                                            .h(px(28.0))
                                            .flex_none()
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_default()
                                            .bg(theme.inverse)
                                            .hover(|element| element.opacity(0.9))
                                            .active(|element| element.opacity(0.8))
                                            .child(icon("icons/send.svg", 16.0, theme.on_inverse))
                                            .tooltip(Tooltip::text(tr!("composer.queue_followup")))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let prompt =
                                                    this.composer.read(cx).content(cx).to_owned();
                                                if let Some(submission) =
                                                    this.submission_with_attachments(&prompt, cx)
                                                {
                                                    this.composer
                                                        .update(cx, |input, cx| input.clear(cx));
                                                    this.route_composer_submission(submission, cx);
                                                }
                                            })),
                                    )
                                }),
                            ComposerSubmitAction::Send => div()
                                .id("send-or-stop")
                                .w(px(28.0))
                                .h(px(28.0))
                                .flex_none()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(if can_send {
                                    theme.inverse
                                } else {
                                    theme.overlay_strong
                                })
                                .when(can_send, |element| {
                                    element
                                        .cursor_default()
                                        .hover(|element| element.opacity(0.9))
                                        .active(|element| element.opacity(0.8))
                                })
                                .child(icon(
                                    "icons/send.svg",
                                    16.0,
                                    if can_send {
                                        theme.on_inverse
                                    } else {
                                        theme.text_ghost
                                    },
                                ))
                                // Says why the button is dead, for the case
                                // the draft is ready and the machine is not.
                                .when(no_providers, |element| {
                                    element.tooltip(Tooltip::text(tr!("composer.no_providers")))
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let prompt = this.composer.read(cx).content(cx).to_owned();
                                    if let Some(submission) =
                                        this.submission_with_attachments(&prompt, cx)
                                    {
                                        this.composer.update(cx, |input, cx| input.clear(cx));
                                        this.route_composer_submission(submission, cx);
                                    }
                                })),
                            ComposerSubmitAction::Continue => div()
                                .id("send-or-stop")
                                .w(px(28.0))
                                .h(px(28.0))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(if can_continue {
                                    theme.inverse
                                } else {
                                    theme.overlay_strong
                                })
                                .when(can_continue, |element| {
                                    element
                                        .cursor_default()
                                        .hover(|element| element.opacity(0.9))
                                        .active(|element| element.opacity(0.8))
                                })
                                .child(icon(
                                    "icons/play.svg",
                                    13.0,
                                    if can_continue {
                                        theme.on_inverse
                                    } else {
                                        theme.text_ghost
                                    },
                                ))
                                .tooltip(Tooltip::text(if no_providers {
                                    tr!("composer.no_providers")
                                } else {
                                    tr!("composer.continue")
                                }))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.continue_interrupted_session(cx);
                                })),
                        }),
                ),
        )
    }

    fn render_branch_selector(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        let (subject_session_id, subject_project_id) = self.workspace_subject();
        let session = subject_session_id.and_then(|session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
        });
        let subject_project = subject_project_id.and_then(|project_id| {
            self.state
                .projects
                .iter()
                .find(|project| project.id == project_id)
        });
        let project = subject_project.filter(|project| !project.is_projectless())?;
        let workspace = session
            .map(|session| session.workspace.clone())
            .unwrap_or_else(|| self.state.workspace_for_new_session(project.id));
        let workspace_path = session
            .and_then(|session| session.workspace.path())
            .unwrap_or(&project.path)
            .to_path_buf();
        let branch_enabled =
            !session.is_some_and(|session| session.is_busy()) && !self.branch_operation_pending;
        let picks_base = worktrees::workspace_picks_base(&workspace, session);
        let snapshot = self.branch_snapshot_for_workspace(&workspace_path, cx)?;
        let selected_branch = match &workspace {
            SessionWorkspace::Local => snapshot.display_branch().map(str::to_owned),
            SessionWorkspace::NewWorktree { base_branch } => base_branch
                .clone()
                .or_else(|| snapshot.default_branch.clone())
                .or_else(|| snapshot.display_branch().map(str::to_owned)),
            SessionWorkspace::Worktree { base_branch, .. } if picks_base => snapshot
                .current
                .clone()
                .or_else(|| base_branch.clone())
                .or_else(|| snapshot.default_branch.clone())
                .or_else(|| snapshot.detached_head.clone()),
            SessionWorkspace::Worktree { branch, .. } => snapshot
                .current
                .clone()
                .or_else(|| branch.clone())
                .or_else(|| snapshot.detached_head.clone()),
        }
        .unwrap_or_else(|| tr!("branches.detached_head"));

        let weak = cx.entity().downgrade();
        let search = self.branch_search.clone();
        let create_input = self.branch_create_input.clone();
        let search_focus = search.read(cx).focus_handle(cx);
        let handle = {
            let toggle_weak = weak.clone();
            let reset_search = search.clone();
            let reset_create = create_input.clone();
            let picker_focus = search_focus.clone();
            self.menu_handle_with(BRANCH_PICKER_MENU_ID, cx, move |open, window, cx| {
                let _ = toggle_weak.update(cx, |this, cx| {
                    if open {
                        this.branch_picker_mode = BranchPickerMode::Browse;
                        this.branch_picker_highlight = None;
                        let project_name = {
                            let (_, project_id) = this.workspace_subject();
                            project_id
                                .and_then(|project_id| {
                                    this.state
                                        .projects
                                        .iter()
                                        .find(|project| project.id == project_id)
                                })
                                .map(Project::display_name)
                                .unwrap_or_else(|| tr!("project.project_lower"))
                        };
                        reset_search.update(cx, |input, cx| {
                            input.set_placeholder(
                                tr!("branches.search_project", project = project_name),
                                cx,
                            );
                            input.clear(cx);
                        });
                        reset_create.update(cx, |input, cx| input.clear(cx));
                        if let Some(path) = this.workspace_subject_path() {
                            this.refresh_workspace_branch_snapshot(&path, cx);
                        }
                    } else {
                        this.branch_picker_mode = BranchPickerMode::Browse;
                        let focus = this.composer_focus(cx);
                        window.focus(&focus, cx);
                    }
                    cx.notify();
                });
                if open {
                    let picker_focus = picker_focus.clone();
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| window.focus(&picker_focus, cx));
                    });
                }
            })
        };

        let trigger = MenuChip::new("workspace-branch")
            .icon("icons/git-branch.svg", theme.text_tertiary)
            .label(if self.branch_operation_pending {
                tr!("branches.switching")
            } else {
                selected_branch.clone()
            })
            .caret(false)
            .disabled(!branch_enabled)
            .selected(branch_enabled && handle.is_open())
            .max_w(px(210.0))
            .when(branch_enabled, |chip| {
                chip.tooltip(tr!("branches.choose"))
                    .shortcut_action(&ToggleBranchPicker)
            });
        if !branch_enabled {
            return Some(trigger.into_any_element());
        }

        let normalized_query = self
            .branch_search
            .read(cx)
            .content()
            .trim()
            .to_ascii_lowercase();
        let visible_branches = Rc::new(
            if handle.is_open() && self.branch_picker_mode == BranchPickerMode::Browse {
                visible_branch_entries(&snapshot.branches, &selected_branch, &normalized_query)
            } else {
                Vec::new()
            },
        );
        let allow_create = !picks_base;
        let actions = Rc::new(
            visible_branches
                .iter()
                .filter(|branch| picks_base || !branch.checked_out_elsewhere)
                .map(|branch| BranchPickerAction::Checkout(branch.name.clone()))
                .chain(allow_create.then_some(BranchPickerAction::Create))
                .collect::<Vec<_>>(),
        );
        let highlight = self
            .branch_picker_highlight
            .filter(|index| *index < actions.len());
        let mode = self.branch_picker_mode;
        if handle.is_open() && mode == BranchPickerMode::Browse {
            self.sync_branch_picker_rows(&visible_branches);
        }
        let branch_list = self.branch_picker_list_state.clone();

        Some(popover(
            trigger,
            &handle,
            MenuAlign::AboveLeft,
            move |popover, _window, _cx| {
                let popover = popover.clone();
                let next_actions = actions.clone();
                let previous_actions = actions.clone();
                let confirm_actions = actions.clone();
                let dismiss_weak = weak.clone();
                let next_weak = weak.clone();
                let previous_weak = weak.clone();
                let confirm_weak = weak.clone();
                let confirm_popover = popover.clone();

                let body = if mode == BranchPickerMode::Create {
                    div()
                        .w_full()
                        .p(px(14.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .text_size(sp(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(icon("icons/plus.svg", 14.0, theme.text_secondary))
                                .child(tr!("branches.create_and_checkout")),
                        )
                        .child(
                            div()
                                .mt(px(12.0))
                                .h(px(36.0))
                                .px(px(10.0))
                                .rounded(px(11.0))
                                .border(hairline())
                                .border_color(theme.border_strong)
                                .bg(theme.surface)
                                .flex()
                                .items_center()
                                .child(div().flex_1().min_w_0().child(create_input.clone())),
                        )
                        .child(
                            div()
                                .mt(px(9.0))
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(tr!("branches.create_hint")),
                        )
                        .into_any_element()
                } else {
                    let rows = if visible_branches.is_empty() {
                        div()
                            .id("branch-picker-list-empty")
                            .h(px(64.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(sp(12.5))
                            .text_color(theme.text_ghost)
                            .child(tr!("branches.none_found"))
                            .into_any_element()
                    } else {
                        let list_branches = visible_branches.clone();
                        let list_actions = actions.clone();
                        let list_selected_branch = selected_branch.clone();
                        let list_weak = weak.clone();
                        let list_popover = popover.clone();
                        let height =
                            (visible_branches.len() as f32 * BRANCH_PICKER_ROW_HEIGHT).min(260.0);
                        div()
                            .id("branch-picker-list")
                            .w_full()
                            .h(px(height))
                            .flex_none()
                            .px(px(4.0))
                            .child(
                                list(branch_list.clone(), move |index, _window, _cx| {
                                    let Some(branch) = list_branches.get(index) else {
                                        return div().into_any_element();
                                    };
                                    let selected = branch.name == list_selected_branch;
                                    let disabled = branch.checked_out_elsewhere && !picks_base;
                                    let highlighted = highlight
                                        .and_then(|index| list_actions.get(index))
                                        .is_some_and(|action| {
                                            matches!(
                                                action,
                                                BranchPickerAction::Checkout(name)
                                                    if name == &branch.name
                                            )
                                        });
                                    let color = if disabled {
                                        theme.text_ghost
                                    } else {
                                        theme.text
                                    };
                                    let row = div()
                                        .id(SharedString::from(format!(
                                            "branch-row-{}",
                                            branch.name
                                        )))
                                        .w_full()
                                        .h(px(BRANCH_PICKER_ROW_HEIGHT))
                                        .px(px(8.0))
                                        .rounded(px(8.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .cursor_default()
                                        .when(highlighted, |element| {
                                            element.bg(theme.overlay_strong)
                                        })
                                        .when(!disabled, |element| {
                                            element
                                                .hover(|element| element.bg(theme.overlay))
                                                .active(|element| element.opacity(0.85))
                                        })
                                        .child(icon("icons/git-branch.svg", 12.0, color))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .truncate()
                                                .text_size(sp(12.5))
                                                .line_height(sp(15.0))
                                                .text_color(color)
                                                .child(SharedString::from(branch.name.clone())),
                                        )
                                        .when(selected, |element| {
                                            element.child(icon(
                                                "icons/check.svg",
                                                11.0,
                                                theme.text_secondary,
                                            ))
                                        });
                                    if disabled {
                                        row.into_any_element()
                                    } else {
                                        let branch_name = branch.name.clone();
                                        let select_weak = list_weak.clone();
                                        let select_popover = list_popover.clone();
                                        row.on_click(move |_, window, cx| {
                                            let should_close = select_weak
                                                .update(cx, |this, cx| {
                                                    this.choose_workspace_branch(
                                                        branch_name.clone(),
                                                        cx,
                                                    )
                                                })
                                                .unwrap_or(false);
                                            if should_close {
                                                select_popover.close(window, cx);
                                                window.refresh();
                                            }
                                        })
                                        .into_any_element()
                                    }
                                })
                                .size_full(),
                            )
                            .into_any_element()
                    };

                    let create_row = allow_create.then(|| {
                        let create_weak = weak.clone();
                        div()
                            .id("create-workspace-branch")
                            .mx(px(4.0))
                            .h(px(BRANCH_PICKER_ROW_HEIGHT))
                            .px(px(8.0))
                            .rounded(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .cursor_default()
                            .when(
                                highlight.and_then(|index| actions.get(index))
                                    == Some(&BranchPickerAction::Create),
                                |element| element.bg(theme.overlay_strong),
                            )
                            .hover(|element| element.bg(theme.overlay))
                            .active(|element| element.opacity(0.85))
                            .child(icon("icons/plus.svg", 12.0, theme.text_secondary))
                            .child(
                                div()
                                    .text_size(sp(12.5))
                                    .line_height(sp(15.0))
                                    .text_color(theme.text)
                                    .child(tr!("branches.create_and_checkout_ellipsis")),
                            )
                            .on_click(move |_, window, cx| {
                                let _ = create_weak.update(cx, |this, cx| {
                                    this.begin_branch_creation(window, cx);
                                });
                            })
                    });

                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(52.0))
                                .px(px(12.0))
                                .pt(px(10.0))
                                .pb(px(8.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(34.0))
                                        .px(px(10.0))
                                        .rounded(px(11.0))
                                        .bg(theme.surface)
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(icon("icons/search.svg", 15.0, theme.text_secondary))
                                        .child(div().flex_1().min_w_0().child(search.clone())),
                                ),
                        )
                        .child(
                            div()
                                .px(px(14.0))
                                .pt(px(3.0))
                                .pb(px(7.0))
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_tertiary)
                                .child(tr!("branches.title")),
                        )
                        .child(rows)
                        .when_some(create_row, |element, create_row| {
                            element
                                .child(
                                    div()
                                        .mx(px(6.0))
                                        .my(px(4.0))
                                        .h(hairline())
                                        .bg(theme.separator),
                                )
                                .child(create_row)
                                .child(div().h(px(4.0)))
                        })
                        .into_any_element()
                };

                div()
                    .w(px(360.0))
                    .max_h(px(390.0))
                    .rounded(px(16.0))
                    .overflow_hidden()
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.raised)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .on_action(move |_: &SelectNextEntry, _, cx| {
                        let _ = next_weak.update(cx, |this, cx| {
                            this.move_branch_picker_highlight("down", &next_actions, cx);
                        });
                    })
                    .on_action(move |_: &SelectPreviousEntry, _, cx| {
                        let _ = previous_weak.update(cx, |this, cx| {
                            this.move_branch_picker_highlight("up", &previous_actions, cx);
                        });
                    })
                    .on_action(move |_: &ConfirmEntry, window, cx| {
                        let should_close = confirm_weak
                            .update(cx, |this, cx| {
                                this.confirm_branch_picker_action(&confirm_actions, window, cx)
                            })
                            .unwrap_or(false);
                        if should_close {
                            confirm_popover.close(window, cx);
                            window.refresh();
                        }
                    })
                    // Escape backs the create form out to browsing. The rest
                    // of the peel is the fields' own clear-on-escape: a
                    // non-empty filter (or typed branch name) clears before
                    // this handler ever sees the keystroke, and an empty
                    // browse view propagates on to the menu's own dismiss.
                    .on_action(move |_: &DismissMenu, window, cx| {
                        let handled = dismiss_weak
                            .update(cx, |this, cx| {
                                if this.branch_picker_mode == BranchPickerMode::Create {
                                    this.cancel_branch_creation(window, cx);
                                    return true;
                                }
                                false
                            })
                            .unwrap_or(false);
                        if !handled {
                            cx.propagate();
                        }
                    })
                    .child(body)
                    .into_any_element()
            },
        ))
    }

    /// The new-task sync strip under the centered greeting: how far the
    /// workspace's checkout trails (and leads) its upstream, plus the button
    /// that runs `git pull` in a terminal tab. Counts come from the local
    /// tracking ref, so they reflect the last fetch. Only drafts show it — a
    /// started task's checkout state is its agent's concern — and only local
    /// workspaces, whose checkout a desktop terminal can actually reach.
    pub(super) fn render_sync_notice(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let session = self.selected_session()?;
        if session.has_started() || session.is_busy() || self.is_remote_session(session.id) {
            return None;
        }
        self.selected_project()
            .filter(|project| !project.is_projectless())?;
        let workspace_path = self.workspace_path_for_session(session)?.to_path_buf();
        let upstream = self
            .branch_snapshot_for_workspace(&workspace_path, cx)?
            .upstream
            .filter(|upstream| upstream.behind > 0 || upstream.ahead > 0)?;
        let theme = Theme::current(cx);
        // `-c core.editor/sequence.editor` rather than an env prefix: the
        // script is sourced by the user's shell, and `VAR=x cmd` is not
        // portable to PowerShell, cmd, nushell, or older fish.
        let pull_script = if self.state.sync_with_merge {
            "git -c core.editor=true -c sequence.editor=true pull --no-rebase"
        } else {
            "git -c core.editor=true -c sequence.editor=true pull --rebase"
        };
        let behind = upstream.behind > 0;
        let (script, action, action_icon, strip_icon, command_icon) = if behind {
            (
                pull_script,
                tr!("sync.pull"),
                "icons/rotate-cw.svg",
                "icons/download.svg",
                CustomCommandIcon::Refresh,
            )
        } else {
            (
                "git push",
                tr!("sync.push"),
                "icons/cloud-upload.svg",
                "icons/cloud-upload.svg",
                CustomCommandIcon::CloudUpload,
            )
        };
        let mut summary = String::new();
        if upstream.behind == 1 {
            summary = tr!("sync.behind_one", upstream = upstream.name);
        } else if upstream.behind > 1 {
            summary = tr!(
                "sync.behind_many",
                count = upstream.behind,
                upstream = upstream.name
            );
        }
        if upstream.ahead > 0 {
            let ahead = if upstream.ahead == 1 {
                tr!("sync.ahead_one")
            } else {
                tr!("sync.ahead_many", count = upstream.ahead)
            };
            summary = if summary.is_empty() {
                ahead
            } else {
                format!("{summary} · {ahead}")
            };
        }
        let focus = self.transcript_control_focus("workspace-sync", cx);
        Some(
            div()
                .h(px(26.0))
                .mt(px(12.0))
                .max_w_full()
                .pl(px(10.0))
                .pr(px(10.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(strip_icon, 12.0, theme.text_tertiary))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_color(theme.text_tertiary)
                        .child(summary),
                )
                .child(
                    div()
                        .id("sync-changes")
                        .h(px(20.0))
                        .px(px(8.0))
                        .rounded(px(5.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .cursor_default()
                        .track_focus(&focus)
                        .tab_index(0)
                        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                        .bg(theme.overlay)
                        .hover(|element| element.bg(theme.overlay_strong))
                        .active(|element| element.opacity(0.8))
                        .child(icon(action_icon, 11.0, theme.text_secondary))
                        .child(action.clone())
                        .tooltip(Tooltip::text(tr!("sync.command_hint", command = script)))
                        .on_click(cx.listener({
                            let action = action.clone();
                            move |this, _, _, cx| {
                                this.sync_workspace(script, action.clone(), command_icon, cx);
                            }
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.sync_workspace(script, action.clone(), command_icon, cx);
                                cx.stop_propagation();
                            }
                        })),
                )
                .into_any_element(),
        )
    }

    /// Run the checkout's sync command in a fresh terminal tab — the rebase
    /// pull by default, the merge form when the setting says so, a push when
    /// the checkout only leads upstream. The tab closes itself on success; a
    /// conflict or other failure stays open with its output visible.
    fn sync_workspace(
        &mut self,
        script: &'static str,
        name: String,
        icon: CustomCommandIcon,
        cx: &mut Context<Self>,
    ) {
        let mut command = CustomCommand::new(script.to_owned());
        command.name = Some(name);
        command.icon = icon;
        command.close_on_success = true;
        self.run_custom_command(command, cx);
    }

    pub(super) fn render_workspace_footer(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let (subject_session_id, subject_project_id) = self.workspace_subject();
        let (
            subject_configurable,
            subject_movable,
            subject_moving,
            subject_projectless,
            project_name,
            subject_project_path,
        ) = {
            let subject_session = subject_session_id.and_then(|session_id| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
            });
            let subject_project = subject_project_id.and_then(|project_id| {
                self.state
                    .projects
                    .iter()
                    .find(|project| project.id == project_id)
            });
            (
                subject_session.is_some_and(|session| !session.has_started() && !session.is_busy()),
                subject_session
                    .is_some_and(|session| self.can_move_session_to_worktree(session.id)),
                subject_session
                    .is_some_and(|session| self.worktree_move_pending.contains(&session.id)),
                subject_project.is_some_and(Project::is_projectless),
                subject_project
                    .map(|project| {
                        if project.is_projectless() {
                            tr!("project.choose_project")
                        } else {
                            project.display_name()
                        }
                    })
                    .unwrap_or_else(|| tr!("project.choose_project")),
                subject_project
                    .filter(|project| !project.is_projectless())
                    .map(|project| project.path.clone()),
            )
        };
        let projectless_selected = subject_projectless;
        // The overlay's untargeted composer has no draft session to inspect
        // yet — its new task is configurable by definition.
        let can_configure_workspace =
            subject_configurable || (self.big_picture.is_open() && subject_project_id.is_some());
        // The Projects page always stands in for a real project — "No
        // project" has no page to point at, so its picker omits the row.
        let on_projects_page = self.projects_page.is_some();
        // A started task can't reconfigure its workspace, but a local one
        // can still move into a worktree carrying its state.
        let can_move_to_worktree = subject_movable;
        let can_pick_worktree = can_configure_workspace || can_move_to_worktree;
        let moving_to_worktree = subject_moving;

        let project_handle = self.menu_handle("workspace-project", cx);
        let project_trigger = MenuChip::new("workspace-project")
            .icon("icons/folder.svg", theme.text_tertiary)
            .label(project_name)
            .caret(false)
            .disabled(!can_configure_workspace)
            .selected(can_configure_workspace && project_handle.is_open())
            .max_w(px(190.0))
            .when(can_configure_workspace, |chip| {
                chip.tooltip(tr!("project.choose"))
                    .shortcut_action(&SwitchProjectForward)
            });
        let project_selector = if can_configure_workspace {
            let project_options = self
                .state
                .projects
                .iter()
                .filter(|project| !project.is_projectless())
                .filter(|project| Some(project.id) == subject_project_id)
                .chain(
                    self.state
                        .projects
                        .iter()
                        .filter(|project| !project.is_projectless())
                        .filter(|project| Some(project.id) != subject_project_id),
                )
                .map(|project| (project.id, project.display_name()))
                .collect::<Vec<_>>();
            let weak = cx.entity().downgrade();
            dropdown_menu(
                project_trigger,
                "workspace-project-menu",
                &project_handle,
                MenuAlign::AboveLeft,
                move |_| {
                    let mut items = project_options
                        .clone()
                        .into_iter()
                        .map(|(project_id, project_name)| {
                            let weak = weak.clone();
                            MenuItem::new(project_name, move |window, cx| {
                                if Some(project_id) != subject_project_id {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.select_project_from_composer(project_id, window, cx);
                                    });
                                }
                            })
                            .selected(Some(project_id) == subject_project_id)
                        })
                        .collect::<Vec<_>>();
                    if !items.is_empty() {
                        items.push(MenuItem::Separator);
                    }
                    let add_project = weak.clone();
                    items.push(
                        MenuItem::new(tr!("project.new_project"), move |_, cx| {
                            let _ = add_project.update(cx, |this, cx| this.add_project(cx));
                        })
                        .icon("icons/folder-new.svg")
                        .shortcut_action(&NewProject),
                    );
                    let projectless = weak.clone();
                    if !on_projects_page {
                        items.push(
                            MenuItem::new(tr!("project.no_project"), move |_, cx| {
                                let _ = projectless.update(cx, |this, cx| {
                                    let subject_projectless = {
                                        let (_, project_id) = this.workspace_subject();
                                        project_id.is_some_and(|project_id| {
                                            this.state.projects.iter().any(|project| {
                                                project.id == project_id && project.is_projectless()
                                            })
                                        })
                                    };
                                    if !subject_projectless {
                                        this.create_projectless_session_from_composer(cx);
                                    }
                                });
                            })
                            .icon("icons/x.svg")
                            .selected(projectless_selected),
                        );
                    }
                    items
                },
            )
        } else {
            project_trigger.into_any_element()
        };

        let workspace = self.workspace_subject_workspace().unwrap_or_default();
        let workspace_label = match &workspace {
            SessionWorkspace::Local => SharedString::from(tr!("workspace.local")),
            SessionWorkspace::NewWorktree { .. } => {
                SharedString::from(tr!("workspace.new_worktree"))
            }
            SessionWorkspace::Worktree { name, .. } => SharedString::from(name.clone()),
        };
        let workspace_icon = if workspace.is_local() {
            "icons/local.svg"
        } else {
            "icons/fork.svg"
        };
        let worktree_name_input = self.worktree_name_input.clone();
        let worktree_handle = {
            let toggle_weak = cx.entity().downgrade();
            let name_input = worktree_name_input.clone();
            let input_focus = worktree_name_input.read(cx).focus_handle(cx);
            self.menu_handle_with("workspace-worktree", cx, move |open, window, cx| {
                let _ = toggle_weak.update(cx, |this, cx| {
                    if open {
                        this.worktree_picker_highlight = None;
                        name_input.update(cx, |input, cx| input.clear(cx));
                        // Base entries describe the project checkout, which
                        // may differ from the draft's materialized worktree.
                        let subject_project_path = {
                            let (_, project_id) = this.workspace_subject();
                            project_id.and_then(|project_id| {
                                this.state
                                    .projects
                                    .iter()
                                    .find(|project| project.id == project_id)
                                    .map(|project| project.path.clone())
                            })
                        };
                        if let Some(path) = subject_project_path {
                            this.branch_snapshots.invalidate(&path);
                        }
                    } else {
                        let focus = this.composer_focus(cx);
                        window.focus(&focus, cx);
                    }
                    cx.notify();
                });
                if open {
                    let input_focus = input_focus.clone();
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| window.focus(&input_focus, cx));
                    });
                }
            })
        };
        let creating_worktree = self.worktree_creation_pending;
        let worktree_trigger = MenuChip::new("workspace-worktree")
            .icon(workspace_icon, theme.text_tertiary)
            .label(if creating_worktree {
                SharedString::from(tr!("workspace.creating"))
            } else if moving_to_worktree {
                SharedString::from(tr!("workspace.moving"))
            } else {
                workspace_label
            })
            .caret(false)
            .disabled(!can_pick_worktree || creating_worktree || moving_to_worktree)
            .selected(can_pick_worktree && !creating_worktree && worktree_handle.is_open())
            .max_w(px(180.0))
            .when(can_pick_worktree, |chip| {
                chip.tooltip(tr!("menu.toggle_workspace"))
            });
        let worktree_selector = if can_pick_worktree {
            // The base entries describe the project's ordinary checkout, so
            // the snapshot is read for the project path even when the draft
            // is already bound to a worktree. Only the create rows consume
            // it, so a started session — which can only move — skips the
            // fetch.
            let project_path = subject_project_path.clone();
            let project_snapshot = if worktree_handle.is_open() && can_configure_workspace {
                project_path.and_then(|path| self.branch_snapshot_for_workspace(&path, cx))
            } else {
                None
            };
            let mut actions = vec![];
            if let SessionWorkspace::Worktree { name, .. } = &workspace {
                actions.push(worktrees::WorktreePickerAction::Current { name: name.clone() });
            }
            actions.push(worktrees::WorktreePickerAction::Local);
            if can_move_to_worktree {
                actions.push(worktrees::WorktreePickerAction::Move);
            }
            if !projectless_selected && can_configure_workspace {
                let current_ref = project_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.current.clone())
                    .or_else(|| {
                        project_snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.detached_head.clone())
                    })
                    .unwrap_or_else(|| "HEAD".to_owned());
                let default_ref = project_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.default_branch.clone());
                actions.extend(worktrees::worktree_picker_create_actions(
                    &workspace,
                    &current_ref,
                    default_ref.as_deref(),
                ));
            }
            let actions = Rc::new(actions);
            let highlight = self
                .worktree_picker_highlight
                .filter(|index| *index < actions.len());
            let weak = cx.entity().downgrade();
            popover(
                worktree_trigger,
                &worktree_handle,
                MenuAlign::AboveLeft,
                move |popover, _window, _cx| {
                    let theme = Theme::current(_cx);
                    let work_in_count = actions
                        .iter()
                        .take_while(|action| {
                            matches!(
                                action,
                                worktrees::WorktreePickerAction::Current { .. }
                                    | worktrees::WorktreePickerAction::Local
                            )
                        })
                        .count();
                    let rows = |range: std::ops::Range<usize>| {
                        let start = range.start;
                        actions[range]
                            .iter()
                            .enumerate()
                            .map(move |(offset, action)| (start + offset, action))
                            .collect::<Vec<_>>()
                    };
                    let render_row =
                        |(index, action): (usize, &worktrees::WorktreePickerAction)| {
                            let (icon_path, label, selected) = match action {
                                worktrees::WorktreePickerAction::Current { name } => {
                                    ("icons/fork.svg", name.clone(), true)
                                }
                                worktrees::WorktreePickerAction::Local => (
                                    "icons/local.svg",
                                    tr!("workspace.local"),
                                    workspace.is_local(),
                                ),
                                worktrees::WorktreePickerAction::Move => {
                                    ("icons/fork.svg", tr!("workspace.move_to_worktree"), false)
                                }
                                worktrees::WorktreePickerAction::Create { base_ref } => {
                                    let label = match base_ref.as_deref() {
                                        Some(reference) => {
                                            tr!("workspace.from_ref", branch = reference)
                                        }
                                        None => tr!("workspace.from_default"),
                                    };
                                    ("icons/fork.svg", label, false)
                                }
                            };
                            let highlighted = highlight == Some(index);
                            let row = div()
                                .id(SharedString::from(format!("worktree-row-{index}")))
                                .w_full()
                                .h(px(BRANCH_PICKER_ROW_HEIGHT))
                                .px(px(8.0))
                                .rounded(px(8.0))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .cursor_default()
                                .when(highlighted, |element| element.bg(theme.overlay_strong))
                                .hover(|element| element.bg(theme.overlay))
                                .active(|element| element.opacity(0.85))
                                .child(icon(icon_path, 12.0, theme.text))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .text_size(sp(12.5))
                                        .line_height(sp(15.0))
                                        .text_color(theme.text)
                                        .child(SharedString::from(label)),
                                )
                                .when(selected, |element| {
                                    element.child(icon(
                                        "icons/check.svg",
                                        11.0,
                                        theme.text_secondary,
                                    ))
                                });
                            let action = action.clone();
                            let select_weak = weak.clone();
                            let select_popover = popover.clone();
                            row.on_click(move |_, window, cx| {
                                let should_close = select_weak
                                    .update(cx, |this, cx| match &action {
                                        worktrees::WorktreePickerAction::Current { .. } => true,
                                        worktrees::WorktreePickerAction::Local => {
                                            if let Some(session_id) =
                                                this.ensure_workspace_subject_session(cx)
                                            {
                                                this.select_workspace_for(
                                                    session_id,
                                                    SessionWorkspace::Local,
                                                    cx,
                                                );
                                            }
                                            true
                                        }
                                        worktrees::WorktreePickerAction::Move => {
                                            let name = this
                                                .worktree_name_input
                                                .read(cx)
                                                .content()
                                                .trim()
                                                .to_owned();
                                            if let Some(session_id) = this.workspace_subject().0 {
                                                this.move_session_to_worktree(
                                                    session_id,
                                                    (!name.is_empty()).then_some(name),
                                                    cx,
                                                );
                                            }
                                            true
                                        }
                                        worktrees::WorktreePickerAction::Create { base_ref } => {
                                            let name = this
                                                .worktree_name_input
                                                .read(cx)
                                                .content()
                                                .trim()
                                                .to_owned();
                                            if let Some(session_id) =
                                                this.ensure_workspace_subject_session(cx)
                                            {
                                                this.create_workspace_worktree_for(
                                                    session_id,
                                                    (!name.is_empty()).then_some(name),
                                                    base_ref.clone(),
                                                    cx,
                                                );
                                            }
                                            true
                                        }
                                    })
                                    .unwrap_or(true);
                                if should_close {
                                    select_popover.close(window, cx);
                                    window.refresh();
                                }
                            })
                            .into_any_element()
                        };
                    let work_in_rows = rows(0..work_in_count)
                        .into_iter()
                        .map(&render_row)
                        .collect::<Vec<_>>();
                    let create_rows = rows(work_in_count..actions.len())
                        .into_iter()
                        .map(&render_row)
                        .collect::<Vec<_>>();
                    let next_actions = actions.clone();
                    let previous_actions = actions.clone();
                    let confirm_actions = actions.clone();
                    let next_weak = weak.clone();
                    let previous_weak = weak.clone();
                    let confirm_weak = weak.clone();
                    let confirm_popover = popover.clone();
                    div()
                        .w(px(320.0))
                        .rounded(px(16.0))
                        .overflow_hidden()
                        .border(hairline())
                        .border_color(theme.border_subtle)
                        .bg(theme.raised)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .on_action(move |_: &SelectNextEntry, _, cx| {
                            let _ = next_weak.update(cx, |this, cx| {
                                this.move_worktree_picker_highlight("down", &next_actions, cx);
                            });
                        })
                        .on_action(move |_: &SelectPreviousEntry, _, cx| {
                            let _ = previous_weak.update(cx, |this, cx| {
                                this.move_worktree_picker_highlight("up", &previous_actions, cx);
                            });
                        })
                        .on_action(move |_: &ConfirmEntry, window, cx| {
                            let should_close = confirm_weak
                                .update(cx, |this, cx| {
                                    this.confirm_worktree_picker_action(&confirm_actions, cx)
                                })
                                .unwrap_or(false);
                            if should_close {
                                confirm_popover.close(window, cx);
                                window.refresh();
                            }
                        })
                        .child(
                            div()
                                .h(px(52.0))
                                .px(px(12.0))
                                .pt(px(10.0))
                                .pb(px(8.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(34.0))
                                        .px(px(10.0))
                                        .rounded(px(11.0))
                                        .bg(theme.surface)
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(icon("icons/fork.svg", 15.0, theme.text_secondary))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .child(worktree_name_input.clone()),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .px(px(14.0))
                                .pt(px(3.0))
                                .pb(px(7.0))
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_tertiary)
                                .child(tr!("workspace.work_in")),
                        )
                        .child(div().px(px(4.0)).flex().flex_col().children(work_in_rows))
                        .when(!create_rows.is_empty(), |element| {
                            element
                                .child(
                                    div()
                                        .px(px(14.0))
                                        .pt(px(6.0))
                                        .pb(px(4.0))
                                        .text_size(sp(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_tertiary)
                                        .child(tr!("workspace.new_worktree")),
                                )
                                .child(
                                    div()
                                        .px(px(4.0))
                                        .pb(px(4.0))
                                        .flex()
                                        .flex_col()
                                        .children(create_rows),
                                )
                        })
                        .into_any_element()
                },
            )
        } else {
            worktree_trigger.into_any_element()
        };

        let branch_selector = self.render_branch_selector(cx);

        let usage_meter = self.render_usage_meter(cx);
        div()
            .flex_none()
            .px(px(20.0 - COMPOSER_OVERHANG))
            .pb(px(8.0))
            .pt(px(4.0))
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0))
                    .mx_auto()
                    .text_size(sp(12.5))
                    .line_height(sp(14.0))
                    .child(
                        div()
                            .h(px(28.0))
                            // The chip contributes 7px, lining its icon up with the
                            // composer's 10px padding plus the controls' 7px inset.
                            .pl(px(10.0))
                            .pr(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .tab_index(0)
                            .tab_group()
                            .tab_stop(false)
                            .child(project_selector)
                            .child(worktree_selector)
                            .children(branch_selector)
                            .child(div().flex_1())
                            .children(usage_meter),
                    ),
            )
    }
}

/// Branches matching the search, with an exact match first, then the
/// selected branch pinned, and every other row sorted by name. Disabled
/// worktree-owned rows stay in the result; the UI needs to explain why Git
/// cannot switch to them.
pub(super) fn visible_branch_entries(
    branches: &[crate::git_branch::BranchEntry],
    selected_branch: &str,
    normalized_query: &str,
) -> Vec<crate::git_branch::BranchEntry> {
    let normalized_query = normalized_query.to_ascii_lowercase();
    let mut visible = branches
        .iter()
        .filter(|branch| {
            normalized_query
                .split_whitespace()
                .all(|token| branch.name.to_ascii_lowercase().contains(token))
        })
        .cloned()
        .collect::<Vec<_>>();
    visible.sort_by(|left, right| {
        let left_exact = left.name.eq_ignore_ascii_case(&normalized_query);
        let right_exact = right.name.eq_ignore_ascii_case(&normalized_query);
        let left_selected = left.name == selected_branch;
        let right_selected = right.name == selected_branch;
        right_exact
            .cmp(&left_exact)
            .then_with(|| right_selected.cmp(&left_selected))
            .then_with(|| left.name.cmp(&right.name))
    });
    visible
}

/// The mention a dropped file submits: relative to the project root when the
/// file is inside it, absolute otherwise, directories with a trailing slash —
/// the same form the `@` autocomplete inserts. Dropping the root itself keeps
/// the absolute path rather than producing an empty mention.
// Base64 keeps the authenticated JSON transport browser-compatible but adds
// one third of wire overhead. Stay comfortably below tungstenite's default
// message limit until uploads move to a streaming content endpoint.
const MAX_ATTACHMENT_BYTES: u64 = waku_client::attachments::MAX_ATTACHMENT_BYTES as u64;

/// Reads a client-local drop into an upload payload. This is the explicit
/// client/daemon boundary: none of these source paths are persisted or handed
/// to a provider.
fn attachment_upload_from_path(
    source: &Path,
) -> anyhow::Result<(
    String,
    waku_client::attachments::AttachmentUpload,
    Option<Vec<u8>>,
)> {
    let metadata = std::fs::symlink_metadata(source)
        .with_context(|| format!("could not read attachment {}", source.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "symbolic-link attachments are not supported: {}",
            source.display()
        );
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow::anyhow!("attachment has no file name: {}", source.display()))?
        .to_owned();
    if metadata.is_file() {
        if metadata.len() > MAX_ATTACHMENT_BYTES {
            anyhow::bail!("attachment is larger than 32 MB: {}", source.display());
        }
        let bytes = std::fs::read(source)
            .with_context(|| format!("could not read attachment {}", source.display()))?;
        let is_image = is_image_attachment_path(source);
        return Ok((
            name,
            waku_client::attachments::AttachmentUpload::File {
                data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            },
            is_image.then_some(bytes),
        ));
    }
    if !metadata.is_dir() {
        anyhow::bail!(
            "attachment is not a file or directory: {}",
            source.display()
        );
    }

    let mut pending = vec![source.to_path_buf()];
    let mut entries = Vec::new();
    let mut total_bytes = 0u64;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).with_context(|| {
            format!(
                "could not read attachment directory {}",
                directory.display()
            )
        })? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            if entries.len() >= waku_client::attachments::MAX_ATTACHMENT_FILES {
                anyhow::bail!(
                    "attachment directory contains more than {} files",
                    waku_client::attachments::MAX_ATTACHMENT_FILES
                );
            }
            total_bytes = total_bytes.saturating_add(metadata.len());
            if total_bytes > MAX_ATTACHMENT_BYTES {
                anyhow::bail!("attachment directory is larger than 32 MB");
            }
            let relative_path = path
                .strip_prefix(source)
                .context("attachment entry escaped its source directory")?
                .to_path_buf();
            let bytes = std::fs::read(&path)
                .with_context(|| format!("could not read attachment {}", path.display()))?;
            entries.push(waku_client::attachments::AttachmentUploadEntry {
                relative_path,
                data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            });
        }
    }
    Ok((
        name,
        waku_client::attachments::AttachmentUpload::Directory { entries },
        None,
    ))
}

#[cfg(test)]
pub(super) fn dropped_file_mention(
    root: Option<&std::path::Path>,
    path: &std::path::Path,
    is_dir: bool,
) -> String {
    let mention = root
        .and_then(|root| path.strip_prefix(root).ok())
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
        .display()
        .to_string();
    if is_dir && !mention.ends_with('/') {
        format!("{mention}/")
    } else {
        mention
    }
}

fn is_image_attachment_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "webp"
                    | "bmp"
                    | "svg"
                    | "tif"
                    | "tiff"
                    | "ico"
                    | "pnm"
                    | "pbm"
                    | "pgm"
                    | "ppm"
            )
        })
}

/// The snippet a "Pasted text" chip shows on hover: the paste's leading
/// characters, trimmed, with an ellipsis when there is more.
pub(super) fn pasted_text_preview(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(PASTED_TEXT_PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

/// The hover card both pasted-text surfaces share — the collapsed-block chip
/// and the stored `.txt` attachment. `.tooltip(..)` takes a view builder, so
/// the preview rides GPUI's usual hover timing and placement.
pub(super) fn pasted_text_tooltip(
    preview: SharedString,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| {
        cx.new(|_| PastedTextPreview {
            preview: preview.clone(),
        })
        .into()
    }
}

/// Typed text first, then each collapsed paste block in paste order, split
/// by a blank line — blocks are almost always many lines themselves.
pub(super) fn prompt_with_pasted_blocks(prompt: &str, pasted_blocks: &[String]) -> String {
    pasted_blocks
        .iter()
        .map(|block| block.trim())
        .filter(|block| !block.is_empty())
        .fold(prompt.trim().to_owned(), |mut body, block| {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(block);
            body
        })
}

/// The prompt a submission sends: the typed text plus one token per staged
/// attachment appended at the end the way T3 Code appends dropped files —
/// `@path` for files, a `session` reference carrying title and task id for
/// session chips. `None` means there is nothing to send.
pub(super) fn merged_submission(prompt: &str, attachments: &[MessageAttachment]) -> Option<String> {
    let mentions = attachments
        .iter()
        .map(|attachment| {
            session_attachment_token(attachment)
                .unwrap_or_else(|| format!("@{}", attachment.mention))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let prompt = prompt.trim();
    match (prompt.is_empty(), mentions.is_empty()) {
        (true, true) => None,
        (false, true) => Some(prompt.to_owned()),
        (true, false) => Some(mentions),
        (false, false) => Some(format!("{prompt} {mentions}")),
    }
}

/// Where the picker's keyboard cursor lands, wrapping at both ends.
///
/// `None` for `current` means the cursor has not moved yet, so `down` opens on
/// the first row and `up` on the last. `None` in the result means the key does
/// not navigate.
pub(super) fn next_picker_highlight(
    current: Option<usize>,
    len: usize,
    key: &str,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match key {
        "down" => Some(current.map_or(0, |index| (index + 1) % len)),
        "up" => Some(current.map_or(len - 1, |index| (index + len - 1) % len)),
        _ => None,
    }
}

/// The row the session's effective selection occupies, when it is listed —
/// the Auto row while the draft is routed, else the row matching its combo.
/// Shared by the scroll reveal and by the keyboard cursor's seed so the
/// filled "current" row and the first arrow press agree on where the
/// selection sits.
pub(super) fn picker_selected_row_index(
    selection: Option<&(ProviderKind, String, Option<String>, bool)>,
    auto_route: bool,
    rows: &[ModelPickerRow],
) -> Option<usize> {
    if auto_route {
        return rows.iter().position(|row| row.auto);
    }
    let (provider, model_id, effort, fast) = selection?;
    rows.iter().position(|row| {
        !row.auto
            && row.provider == *provider
            && row.model.id == *model_id
            && row.effort == *effort
            && row.fast == *fast
    })
}

/// The picker's whole body when nothing can back a session: no agent CLI
/// found on this machine, and none left switched on.
///
/// A rail holding a lone star above an empty filter field would invite the
/// user to search a list that cannot have rows, so the panel names what is
/// missing and offers the page that fixes it. Its one button also carries the
/// panel's focus, which is what `escape` dispatches up from.
fn model_picker_empty_state(
    theme: &Theme,
    focus: &FocusHandle,
    popover: ContextMenuHandle,
    waku: WeakEntity<Waku>,
) -> AnyElement {
    let click_popover = popover.clone();
    let click_waku = waku.clone();
    div()
        .w(px(320.0))
        .rounded(px(16.0))
        .overflow_hidden()
        .border(hairline())
        .border_color(theme.border_subtle)
        .bg(theme.raised)
        .shadow_lg()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(9.0))
        .px(px(24.0))
        .py(px(22.0))
        .child(
            div()
                .w(px(40.0))
                .h(px(40.0))
                .rounded(px(12.0))
                .bg(theme.overlay)
                .flex()
                .items_center()
                .justify_center()
                .child(icon("icons/bot.svg", 19.0, theme.text_tertiary)),
        )
        .child(
            div()
                .text_size(sp(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(tr!("models.no_providers_title")),
        )
        .child(
            div()
                .text_size(sp(12.5))
                .line_height(sp(17.0))
                .text_center()
                .text_color(theme.text_secondary)
                .child(tr!("models.no_providers_description")),
        )
        .child(
            div()
                .id("model-picker-open-providers")
                .track_focus(focus)
                .tab_index(0)
                .tab_stop(true)
                .focus_visible(|style| style.border_color(theme.accent))
                .mt(px(3.0))
                .h(px(28.0))
                .px(px(11.0))
                .rounded(px(9.0))
                .border(hairline())
                .border_color(theme.border_strong)
                .flex()
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .child(icon("icons/settings.svg", 11.0, theme.text_tertiary))
                .child(tr!("models.open_provider_settings"))
                .on_click(move |_, window, cx| {
                    open_settings_page_from_picker(
                        &click_waku,
                        &click_popover,
                        SettingsPage::Providers,
                        window,
                        cx,
                    );
                })
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        open_settings_page_from_picker(
                            &waku,
                            &popover,
                            SettingsPage::Providers,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }
                }),
        )
        .into_any_element()
}

/// Dismiss the picker and land on the given settings page, for both the
/// empty state's click and its keyboard activation. Closing first matters:
/// the picker returns focus to the composer as it closes, which would
/// otherwise pull focus straight back out of the settings view.
fn open_settings_page_from_picker(
    waku: &WeakEntity<Waku>,
    popover: &ContextMenuHandle,
    page: SettingsPage,
    window: &mut Window,
    cx: &mut App,
) {
    popover.close(window, cx);
    let _ = waku.update(cx, |this, cx| {
        this.open_settings_action(&OpenSettings, window, cx);
        this.open_settings_page(page, window, cx);
    });
}

/// Whether the provider contributes rows to the merged list at all.
///
/// Installed on this machine and not switched off in the Providers settings.
/// Both of those are settings-level facts the user has already decided, so
/// the provider's rows are absent rather than dimmed — the list offers what
/// could be picked, not a catalog of everything Goddard can speak to. A
/// session locked to a provider switched off afterwards keeps its rows,
/// since the picker is that session's only route to another model.
pub(super) fn picker_lists_provider(
    probes: &[ProviderProbe],
    disabled_providers: &[ProviderKind],
    locked_provider: Option<ProviderKind>,
    remote: bool,
    kind: ProviderKind,
) -> bool {
    // Antigravity's surface is a local PTY running the CLI's TUI — a remote
    // daemon cannot host it, so the tab does not exist there.
    if remote && kind == ProviderKind::Antigravity {
        return false;
    }
    let installed = probes
        .iter()
        .any(|probe| probe.provider == kind && probe.installed);
    let switched_off = disabled_providers.contains(&kind) && locked_provider != Some(kind);
    installed && !switched_off
}

pub(super) fn model_picker_subtitle(provider: ProviderKind, sub_provider: Option<&str>) -> String {
    let provider_name = provider.short_name();
    match sub_provider.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if name.eq_ignore_ascii_case(provider_name) => provider_name.to_owned(),
        Some(name) => format!("{name} · {provider_name}"),
        None => provider_name.to_owned(),
    }
}

/// Whether the provider can return a live session to the base model after a
/// reasoning-effort pick. Both OpenCode majors express effort as a per-model
/// `variant` whose base selection is `default`, so the picker gets an explicit
/// Default row; other providers keep auto-selecting a catalog effort instead.
pub(super) fn supports_reasoning_default_reset(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::OpenCode | ProviderKind::OpenCode2)
}

/// Whether the picker has nothing left to offer, so the composer's trigger
/// and the panel behind it both swap to their empty state.
///
/// `detection_settled` gates the whole answer. Every probe is seeded as "not
/// installed" and detection answers off the UI thread, so a pass that has
/// never completed means "not known yet", never "nothing here" — otherwise
/// the trigger would flash an empty state during every launch.
pub(super) fn picker_has_no_providers(
    probes: &[ProviderProbe],
    disabled_providers: &[ProviderKind],
    locked_provider: Option<ProviderKind>,
    remote: bool,
    detection_settled: bool,
) -> bool {
    detection_settled
        && !ProviderKind::ALL.into_iter().any(|kind| {
            picker_lists_provider(probes, disabled_providers, locked_provider, remote, kind)
        })
}

/// Every picker row's fixed height — the list is uniform, which lets
/// `ListState` size its scrollbar before any row has painted.
pub(super) const MODEL_PICKER_ROW_HEIGHT: Pixels = px(58.0);

/// A rail button's destination in the merged picker: one of the two
/// leading sections or the first row of a provider's block.
#[derive(Clone, Copy, PartialEq)]
enum ModelPickerSection {
    /// The router row — its provider field is a placeholder, so it cannot
    /// borrow a provider's section.
    Auto,
    Favorites,
    Recents,
    Provider(ProviderKind),
}

/// A row's jump section in the merged list: Auto leads alone, then
/// favorites and recents each form one block ahead of the provider blocks.
fn picker_row_section(row: &ModelPickerRow) -> ModelPickerSection {
    if row.auto {
        ModelPickerSection::Auto
    } else if row.favorite_index.is_some() {
        ModelPickerSection::Favorites
    } else if row.recent_rank.is_some() {
        ModelPickerSection::Recents
    } else {
        ModelPickerSection::Provider(row.provider)
    }
}

/// One selectable row in the merged model picker: a model pinned to a
/// concrete effort and fast-tier choice. Favorites, recents, and the
/// ⌘⌥1–⌘⌥9 chords all address rows, not bare models. `auto` marks the
/// router row, which heads the list while the session can still be routed —
/// picking it defers the provider/model decision to the evaluation route at
/// first submit. Its model/effort fields are placeholders; `auto` guards
/// every read of them.
pub(super) struct ModelPickerRow {
    pub provider: ProviderKind,
    pub model: ProviderModel,
    /// The effort id the row selects; `None` when the model advertises no
    /// effort options at all.
    pub effort: Option<String>,
    /// Whether the row selects the `fast` service tier.
    pub fast: bool,
    /// Position in `favorite_models` when this exact selection is starred.
    pub favorite_index: Option<usize>,
    /// Rank among recently used selections — present only on the fast-variant
    /// that was actually started last, so one of `effort` and `effort-fast`
    /// ever carries it.
    pub recent_rank: Option<usize>,
    /// The Auto router row, not a concrete combo.
    pub auto: bool,
}

impl ModelPickerRow {
    /// The router row placeholder — only ever built by `visible_picker_rows`
    /// and always read through its `auto` guard.
    fn auto() -> Self {
        Self {
            provider: ProviderKind::default(),
            model: ProviderModel::new("", ""),
            effort: None,
            fast: false,
            favorite_index: None,
            recent_rank: None,
            auto: true,
        }
    }
}

/// The model's effective effort when the user picks it without naming one:
/// its declared default, else the first advertised option.
fn model_default_effort(model: &ProviderModel) -> Option<String> {
    model.default_reasoning_effort.clone().or_else(|| {
        model
            .reasoning_efforts
            .first()
            .map(|option| option.id.clone())
    })
}

/// Whether a starred entry marks this row. Favorites saved before rows were
/// combos carry no effort; one claims the model's default-effort row so a
/// bare `{provider, model}` star still lands on something selectable.
pub(super) fn favorite_matches_row(
    favorite: &FavoriteModel,
    provider: ProviderKind,
    model: &str,
    effort: Option<&str>,
    fast: bool,
    default_effort: Option<&str>,
) -> bool {
    if favorite.provider != provider || favorite.model != model || favorite.fast != fast {
        return false;
    }
    match favorite.effort.as_deref() {
        Some(favorite_effort) => Some(favorite_effort) == effort,
        None => effort == default_effort,
    }
}

/// How much of a model each picker row names: the composer's full combos, or
/// one row per model for pickers whose value is a bare `provider:model`
/// target — the routing policy cannot encode an effort or a tier.
#[derive(Clone, Copy)]
pub(super) enum PickerGranularity {
    Combos,
    Models,
}

/// Where a model picker's selection lands: the composer session (provider,
/// model, effort, tier) or the automation editor's bare provider/model pair.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ModelPickerTarget {
    #[default]
    Composer,
    AutomationEditor,
}

/// Every (effort, tier) combination a catalog model expands into: one row per
/// advertised effort — a single row when the model has none — crossed with
/// the standard/fast pair when the provider offers a fast tier.
fn picker_model_rows(provider: ProviderKind, model: ProviderModel) -> Vec<ModelPickerRow> {
    let efforts: Vec<Option<String>> = if model.reasoning_efforts.is_empty() {
        vec![None]
    } else {
        model
            .reasoning_efforts
            .iter()
            .map(|option| Some(option.id.clone()))
            .collect()
    };
    let fast_variants: &[bool] = if model.service_tiers.iter().any(|option| option.id == "fast") {
        &[false, true]
    } else {
        &[false]
    };
    let mut rows = Vec::with_capacity(efforts.len() * fast_variants.len());
    for effort in efforts {
        for fast in fast_variants {
            rows.push(ModelPickerRow {
                provider,
                model: model.clone(),
                effort: effort.clone(),
                fast: *fast,
                favorite_index: None,
                recent_rank: None,
                auto: false,
            });
        }
    }
    rows
}

/// The provider's slot in `ProviderKind::ALL`, which keeps the merged list's
/// fallback ordering stable and consistent with the rest of the app.
fn provider_sort_rank(provider: ProviderKind) -> usize {
    ProviderKind::ALL
        .iter()
        .position(|kind| *kind == provider)
        .unwrap_or(usize::MAX)
}

/// A row's slot within its model: the ladder index of its effort, or zero
/// for models that name no effort at all.
fn effort_sort_rank(row: &ModelPickerRow) -> usize {
    row.model
        .reasoning_efforts
        .iter()
        .position(|option| Some(option.id.as_str()) == row.effort.as_deref())
        .unwrap_or(0)
}

/// The rows the picker lists, in display order: the Auto row first while the
/// session can still be routed, then starred selections in their drag order,
/// then selections a session was actually started with — most recent first —
/// then everything else by provider, model name, and the model's own effort
/// ladder with the standard tier before its fast twin.
///
/// Shared by the panel body and by `enter`'s handler so a keyboard cursor
/// index always means the same row in both. `auto_route` prepends the router
/// row; a query keeps it only while "auto" matches the same token rule models
/// follow.
pub(super) fn visible_picker_rows(
    probes: &[ProviderProbe],
    favorites: &[FavoriteModel],
    recents: &[RecentModelUse],
    disabled_providers: &[ProviderKind],
    locked_provider: Option<ProviderKind>,
    normalized_query: &str,
    auto_route: bool,
    granularity: PickerGranularity,
) -> Vec<ModelPickerRow> {
    let searching = !normalized_query.is_empty();
    let mut rows: Vec<ModelPickerRow> = probes
        .iter()
        .filter(|probe| probe.installed)
        .flat_map(|probe| {
            probe
                .models
                .iter()
                .cloned()
                .flat_map(move |model| match granularity {
                    PickerGranularity::Combos => picker_model_rows(probe.provider, model),
                    PickerGranularity::Models => vec![ModelPickerRow {
                        provider: probe.provider,
                        model,
                        effort: None,
                        fast: false,
                        favorite_index: None,
                        recent_rank: None,
                        auto: false,
                    }],
                })
        })
        .filter(|row| locked_provider.is_none() || locked_provider == Some(row.provider))
        // Switched-off providers keep serving the session already locked to
        // them, but offer nothing to new work — including favorites.
        .filter(|row| {
            !disabled_providers.contains(&row.provider) || locked_provider == Some(row.provider)
        })
        .filter(|row| {
            if !searching {
                return true;
            }
            let searchable = format!(
                "{} {} {} {} {} {}",
                row.model.name,
                row.model.id,
                row.provider.short_name(),
                row.model.sub_provider.as_deref().unwrap_or(""),
                row.effort.as_deref().unwrap_or(""),
                if row.fast { "fast" } else { "" },
            )
            .to_ascii_lowercase();
            normalized_query
                .split_whitespace()
                .all(|token| searchable.contains(token))
        })
        .collect();

    // Stored selections may name a packed alias (`grok-4.6-xhigh-fast`) from
    // before the catalog folded aliases into base models — resolve each back
    // to base plus the traits its suffix carries so stars and recents still
    // land on their combo row.
    let packed_combo = |provider: ProviderKind,
                        stored: &str,
                        effort: &Option<String>,
                        fast: bool,
                        model: &ProviderModel|
     -> (String, Option<String>, bool) {
        let Some(selection) = probes
            .iter()
            .find(|probe| probe.provider == provider)
            .and_then(|probe| {
                waku_protocol::model_catalog::resolve_packed_model(
                    probe.models.iter().map(|model| model.id.as_str()),
                    stored,
                    provider,
                )
            })
            .filter(|selection| !selection.suffix.is_empty())
        else {
            return (stored.to_owned(), effort.clone(), fast);
        };
        (
            selection.value,
            effort.clone().or_else(|| {
                waku_protocol::model_catalog::packed_suffix_reasoning_effort(
                    &selection.suffix,
                    &model.reasoning_efforts,
                )
            }),
            fast || waku_protocol::model_catalog::packed_suffix_service_tier(
                &selection.suffix,
                &model.service_tiers,
            )
            .is_some(),
        )
    };
    for row in &mut rows {
        let default_effort = model_default_effort(&row.model);
        row.favorite_index = favorites.iter().position(|favorite| {
            let (model, effort, fast) = packed_combo(
                favorite.provider,
                &favorite.model,
                &favorite.effort,
                favorite.fast,
                &row.model,
            );
            let normalized = FavoriteModel {
                provider: favorite.provider,
                model,
                effort,
                fast,
            };
            favorite_matches_row(
                &normalized,
                row.provider,
                &row.model.id,
                row.effort.as_deref(),
                row.fast,
                default_effort.as_deref(),
            )
        });
        row.recent_rank = recents.iter().position(|use_| {
            let (model, effort, fast) = packed_combo(
                use_.provider,
                &use_.model,
                &use_.effort,
                use_.fast,
                &row.model,
            );
            use_.provider == row.provider
                && model == row.model.id
                && effort == row.effort
                && fast == row.fast
        });
    }
    rows.sort_by_key(|row| {
        (
            row.favorite_index.is_none(),
            row.favorite_index.unwrap_or(usize::MAX),
            row.recent_rank.is_none(),
            row.recent_rank.unwrap_or(usize::MAX),
            provider_sort_rank(row.provider),
            row.model.name.to_lowercase(),
            effort_sort_rank(row),
            row.fast,
        )
    });

    let router_row = auto_route
        && (!searching
            || normalized_query
                .split_whitespace()
                .all(|token| "auto jev".contains(token)));
    let mut picker_rows = Vec::with_capacity(rows.len() + usize::from(router_row));
    if router_row {
        picker_rows.push(ModelPickerRow::auto());
    }
    picker_rows.extend(rows);
    picker_rows
}
