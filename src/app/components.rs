use super::*;

use chrono::{Datelike, Days};
use std::path::Path;
use waku_client::git::CommitEntry;

const USER_MESSAGE_MAX_HEIGHT: f32 = 400.0;
const USER_MESSAGE_VIEWPORT_MAX_HEIGHT: f32 = USER_MESSAGE_MAX_HEIGHT - 16.0;
/// Room the "Show more" row takes inside a capped bubble; the clipped viewport
/// yields it so the whole bubble stays within the cap.
const USER_MESSAGE_EXPANDER_HEIGHT: f32 = 24.0;

pub(super) fn pulse_dot(size: f32, color: Hsla) -> AnyElement {
    motion::pulse(Duration::from_millis(1600), move |phase| {
        div()
            .w(px(size))
            .h(px(size))
            .flex_none()
            .rounded_full()
            .bg(color)
            .opacity(pulsating_between(0.3, 1.0)(phase))
            .into_any_element()
    })
    // Mounted for whole activities; its pane must not tick at full rate.
    .every(2)
    .into_any_element()
}

pub(super) fn format_message_time(created_at: u64) -> String {
    format_message_time_at(created_at, Local::now())
}

/// Rough output-rate estimate for a settled response: ~4 chars per token
/// over the seconds between the message's first delta and `completed_at`.
fn response_tokens_per_second(message: &Message, completed_at: u64) -> Option<u64> {
    if message.role != MessageRole::Assistant || message.content.is_empty() {
        return None;
    }
    let estimated_tokens = message.content.chars().count().div_ceil(4) as u64;
    let elapsed_seconds = completed_at.saturating_sub(message.created_at).max(1);
    Some(estimated_tokens / elapsed_seconds)
}

fn format_message_time_at(created_at: u64, now: DateTime<Local>) -> String {
    let Ok(seconds) = i64::try_from(created_at) else {
        return String::new();
    };
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .map(|timestamp| {
            let timestamp = timestamp.with_timezone(&Local);
            let message_date = timestamp.date_naive();
            let today = now.date_naive();
            if crate::i18n::uses_east_asian_date_format() {
                let time = timestamp.format("%H:%M").to_string();
                if message_date >= today {
                    return time;
                }
                if today.pred_opt() == Some(message_date) {
                    return tr!("time.yesterday_at", time = time);
                }
                let week_start = today
                    .checked_sub_days(Days::new(today.weekday().num_days_from_monday().into()))
                    .unwrap_or(today);
                if message_date >= week_start {
                    let weekday = match timestamp.weekday() {
                        chrono::Weekday::Mon => tr!("time.monday"),
                        chrono::Weekday::Tue => tr!("time.tuesday"),
                        chrono::Weekday::Wed => tr!("time.wednesday"),
                        chrono::Weekday::Thu => tr!("time.thursday"),
                        chrono::Weekday::Fri => tr!("time.friday"),
                        chrono::Weekday::Sat => tr!("time.saturday"),
                        chrono::Weekday::Sun => tr!("time.sunday"),
                    };
                    return tr!("time.weekday_at", weekday = weekday, time = time);
                }
                if message_date.year() == today.year() {
                    return tr!(
                        "time.date_at",
                        month = timestamp.month(),
                        day = timestamp.day(),
                        time = time
                    );
                }
                return tr!(
                    "time.full_date_at",
                    year = timestamp.year(),
                    month = timestamp.month(),
                    day = timestamp.day(),
                    time = time
                );
            }
            let time = timestamp
                .format("%I:%M %p")
                .to_string()
                .trim_start_matches('0')
                .to_owned();

            if message_date >= today {
                return time;
            }

            if today.pred_opt() == Some(message_date) {
                return tr!("time.yesterday_at", time = time);
            }

            let week_start = today
                .checked_sub_days(Days::new(today.weekday().num_days_from_monday().into()))
                .unwrap_or(today);
            if message_date >= week_start {
                return format!("{} {time}", timestamp.format("%A"));
            }

            let day = timestamp.day();
            let ordinal_suffix = match day % 100 {
                11..=13 => "th",
                _ => match day % 10 {
                    1 => "st",
                    2 => "nd",
                    3 => "rd",
                    _ => "th",
                },
            };
            let date = if message_date.year() == today.year() {
                format!("{} {day}{ordinal_suffix}", timestamp.format("%b"))
            } else {
                format!(
                    "{} {day}{ordinal_suffix} {}",
                    timestamp.format("%b"),
                    timestamp.year()
                )
            };
            format!("{date}, {time}")
        })
        .unwrap_or_default()
}

impl Waku {
    pub(super) fn control_was_copied(&self, control_id: &str) -> bool {
        self.copied_control_feedback.contains_key(control_id)
    }

    pub(super) fn show_control_copied(
        &mut self,
        control_id: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let control_id = control_id.into();
        self.copied_control_generation = self.copied_control_generation.wrapping_add(1);
        let generation = self.copied_control_generation;
        self.copied_control_feedback
            .insert(control_id.clone(), generation);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied_control_feedback.get(&control_id) == Some(&generation) {
                    this.copied_control_feedback.remove(&control_id);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Expansion is one-way within a session visit: there is no "Show less",
    /// and `reset_visible_state` re-caps every prompt on the next activation.
    pub(super) fn expand_user_message(&mut self, message_id: Uuid, cx: &mut Context<Self>) {
        self.expanded_user_messages.insert(message_id);
        cx.notify();
    }

    fn show_message_copied(&mut self, message_id: Uuid, cx: &mut Context<Self>) {
        self.copied_message_generation = self.copied_message_generation.wrapping_add(1);
        let generation = self.copied_message_generation;
        self.copied_message_feedback.insert(message_id, generation);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied_message_feedback.get(&message_id) == Some(&generation) {
                    this.copied_message_feedback.remove(&message_id);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_message_footer(
    theme: &Theme,
    message: &Message,
    footer_time: u64,
    copy_content: SharedString,
    copied: bool,
    show_token_speed: bool,
    group_name: SharedString,
    force_visible: bool,
    align_right: bool,
    assistant_message_action: Option<AssistantMessageAction>,
    user_message_action: Option<UserMessageAction>,
    waku: gpui::WeakEntity<Waku>,
) -> AnyElement {
    let theme = *theme;
    let message_id = message.id;
    let copy_waku = waku.clone();
    let footer_color = theme.text_ghost;
    let timestamp = div()
        .h(px(27.0))
        .px(px(4.0))
        .flex()
        .items_center()
        .text_size(sp(12.5))
        .line_height(sp(14.0))
        .text_color(footer_color)
        .child(format_message_time(footer_time))
        .when_some(
            show_token_speed
                .then(|| response_tokens_per_second(message, footer_time))
                .flatten(),
            |element, tps| element.child(" · ").child(format!("{tps} tok/s")),
        );
    let copy_button = div()
        .id(SharedString::from(format!("copy-message-{message_id}")))
        .w(px(27.0))
        .h(px(27.0))
        .rounded(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_default()
        .hover(|element| element.bg(theme.overlay_strong))
        .child(icon(
            if copied {
                "icons/check.svg"
            } else {
                "icons/copy.svg"
            },
            14.0,
            footer_color,
        ))
        .tooltip(Tooltip::text(if copied {
            tr!("common.copied")
        } else {
            tr!("common.copy_message")
        }))
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copy_content.to_string()));
            let _ = copy_waku.update(cx, |this, cx| {
                this.show_message_copied(message_id, cx);
            });
        });
    let mut footer = div()
        .w_full()
        .h(px(27.0))
        .flex()
        .items_center()
        .gap(px(1.0))
        .when(!force_visible, |element| {
            element
                .invisible()
                .group_hover(group_name, |element| element.visible())
        })
        .when(!align_right, |element| element.ml(-px(7.0)))
        .when(align_right, |element| element.justify_end());

    if align_right {
        footer = footer.child(timestamp).child(copy_button);
    } else {
        footer = footer.child(copy_button);
        if let Some(action) = assistant_message_action {
            let fork_waku = waku.clone();
            let fork_icon = if action.preparing {
                motion::spin(icon("icons/loader-circle.svg", 14.0, footer_color))
            } else {
                icon("icons/fork.svg", 14.0, footer_color).into_any_element()
            };
            let fork_button = div()
                .id(SharedString::from(format!("fork-response-{message_id}")))
                .w(px(27.0))
                .h(px(27.0))
                .rounded(px(10.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .when(!action.enabled && !action.preparing, |element| {
                    element.opacity(0.45)
                })
                .child(fork_icon)
                .tooltip(Tooltip::text(if action.enabled {
                    tr_cow!("session.fork_task")
                } else {
                    tr_cow!("session.forking_task")
                }));
            footer = footer.child(if action.enabled {
                fork_button
                    .hover(|element| element.bg(theme.overlay_strong))
                    .on_click(move |_, _, cx| {
                        let _ = fork_waku.update(cx, |this, cx| {
                            this.fork_session_from_response(
                                action.session_id,
                                action.turn_count,
                                cx,
                            );
                        });
                    })
            } else {
                fork_button
            });
        }
        footer = footer.child(timestamp);
    }

    if let Some(action) = user_message_action {
        let edit_waku = waku;
        footer = footer.child(
            div()
                .id(SharedString::from(format!(
                    "user-message-action-{message_id}"
                )))
                .w(px(27.0))
                .h(px(27.0))
                .rounded(px(10.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .hover(|element| element.bg(theme.overlay_strong))
                .child(icon("icons/rewind.svg", 14.0, footer_color))
                .tooltip(Tooltip::text(tr_cow!("session.revert_to_here")))
                .on_click(move |_, window, cx| {
                    let _ = edit_waku.update(cx, |this, cx| {
                        this.begin_message_edit(action, window, cx);
                    });
                }),
        );
    }

    footer.into_any_element()
}

/// Everything one transcript message row needs to render itself. Bundled
/// because these travel together from `transcript_row` and nowhere else.
pub(super) struct MessageRender<'a> {
    pub(super) theme: &'a Theme,
    pub(super) message: &'a Message,
    pub(super) assistant_footer_copy_content: Option<SharedString>,
    pub(super) assistant_footer_time: Option<u64>,
    pub(super) copied: bool,
    pub(super) show_response_token_speed: bool,
    pub(super) assistant_message_action: Option<AssistantMessageAction>,
    pub(super) user_message_action: Option<UserMessageAction>,
    pub(super) user_message_viewport: Option<&'a UserMessageScrollViewport>,
    /// Whether the prompt's height cap is lifted, and the focus handle its
    /// "Show more" button tracks. Only meaningful for user messages.
    pub(super) user_message_expanded: bool,
    pub(super) user_message_expand_focus: Option<FocusHandle>,
    pub(super) message_edit_input: Option<Entity<ComposerInput>>,
    pub(super) attachment_menus: Vec<ContextMenuHandle>,
    pub(super) attachment_images: Vec<Option<Arc<gpui::Image>>>,
    /// Captured from the selected daemon before the virtualized row is built.
    /// A row is laid out while the root `Waku` entity is already updating, so
    /// it must not read that entity again just to decide whether Finder reveal
    /// is available.
    pub(super) attachments_can_reveal: bool,
    /// The parsed human or assistant body. System messages remain verbatim.
    pub(super) markdown: Option<&'a MarkdownView>,
    /// `#N` mentions in a user message resolved against the workspace's
    /// work-item store — the chips under the bubble. Empty when none.
    pub(super) work_item_refs: Vec<ComposerWorkItem>,
    pub(super) ctx: &'a MarkdownCtx<'a>,
    pub(super) menu: ContextMenuHandle,
    /// The "Sent by agent" chip's live link target: `sent_by_task` resolved
    /// against session state before layout — `None` when the source task is
    /// archived or deleted, leaving the chip inert.
    pub(super) sent_by_task_link: Option<Uuid>,
    /// An enabled auto prompt identified from its labeled transcript row.
    pub(super) auto_prompt_rule: Option<(Uuid, String)>,
    pub(super) waku: gpui::WeakEntity<Waku>,
    pub(super) composer: Entity<ComposerInput>,
    /// Disclosure state for a `TranscriptNotice::Landed` row — `None` in the
    /// card renderings (side chat, big picture), where the notice stays a
    /// collapsed summary.
    pub(super) landed_notice: Option<LandedNoticeState>,
    /// Quarantine, disclosure, and focus state for a
    /// `TranscriptNotice::TransferReceived` row — `None` for every other
    /// message, which renders the card's static face only.
    pub(super) transfer_notice: Option<TransferNoticeState>,
}

/// The live bits a received-transfer card needs that the persisted notice
/// cannot carry: quarantine decides whether Open is armed, `show_all` lifts
/// the folder listing's preview cap, `image` is the lazily-read thumbnail,
/// and the focus handles keep the controls keyboard-reachable across the
/// list's re-renders.
pub(super) struct TransferNoticeState {
    /// `session.quarantined` — or pessimistically true while the session's
    /// detail is still a skeleton, so Open never arms early.
    pub(super) quarantined: bool,
    /// The receipt's own session — the Trust button's target.
    pub(super) session_id: Uuid,
    /// Remote sessions can't open or reveal their host's paths locally.
    pub(super) can_open: bool,
    pub(super) show_all: bool,
    pub(super) image: Option<Arc<gpui::Image>>,
    pub(super) open_focus: FocusHandle,
    pub(super) reveal_focus: FocusHandle,
    pub(super) trust_focus: FocusHandle,
    pub(super) entries_focus: FocusHandle,
    pub(super) image_focus: FocusHandle,
}

/// How far a landed notice is opened and the focus handles its controls
/// track. `expanded` reveals the commit list; `show_all` lifts the
/// [`LANDED_NOTICE_SHOWN_COMMITS`] preview cap within it. `push` is the
/// base branch's live push state — the card's push affordance — and
/// `push_focus` its button's focus target.
pub(super) struct LandedNoticeState {
    pub(super) expanded: bool,
    pub(super) show_all: bool,
    pub(super) header_focus: FocusHandle,
    pub(super) commits_focus: FocusHandle,
    pub(super) push: push_base::LandedPush,
    pub(super) push_focus: FocusHandle,
}

/// Filename text that opens the file in its OS default app — the user's
/// editor for source, Preview for images, Finder for directories.
///
/// The caller styles and identifies the element; this adds the link
/// affordance (pointer cursor, underline on hover or keyboard focus) and
/// click/Enter/Space activation routed through `open_path_in_default_app`,
/// which resolves workspace-relative paths and toasts on remote hosts. The
/// press is stopped so a containing row's own click action stays silent.
/// Right-click (or Shift+F10) raises the same menu the file browser's rows
/// get; `menu_id` keys that menu, so it must be unique to the link's site —
/// the element's own id string works.
pub(super) fn file_link(
    element: Stateful<Div>,
    focus: &FocusHandle,
    path: String,
    this: &Waku,
    waku: &gpui::WeakEntity<Waku>,
    menu_id: impl Into<SharedString>,
    cx: &mut App,
) -> AnyElement {
    let menu_id = menu_id.into();
    // `waku` is leased while transcript rows render, so the menu handle must be
    // resolved on the already-borrowed entity — `waku.update` here panics.
    let menu = this.menu_handle(menu_id.clone(), cx);
    let click_waku = waku.clone();
    let key_waku = waku.clone();
    let click_path = path.clone();
    let key_path = path.clone();
    let key_menu = menu.clone();
    let element = element
        .track_focus(focus)
        .tab_index(0)
        .cursor_pointer()
        .hover(|style| style.underline())
        .focus_visible(|style| style.underline())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            let _ = click_waku.update(cx, |this, cx| {
                this.open_path_in_default_app(&click_path, cx);
            });
            cx.stop_propagation();
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                let _ = key_waku.update(cx, |this, cx| {
                    this.open_path_in_default_app(&key_path, cx);
                });
                cx.stop_propagation();
            } else if event.keystroke.key == "f10" && event.keystroke.modifiers.shift {
                key_menu.open_context_menu(window, cx);
                cx.stop_propagation();
            }
        });
    let menu_waku = waku.clone();
    context_menu(element, menu_id, &menu, move |cx| {
        menu_waku
            .read_with(cx, |this, cx| this.file_link_menu(&menu_waku, &path, cx))
            .ok()
            .unwrap_or_default()
    })
}

/// The on-disk path a file-backed activity row's detail names, when the
/// detail text is a filename: reads and lists carry `display_target`, a
/// single-file change carries `change.path`. Anything else — commands,
/// searches, multi-file counts — renders prose, not a filename.
pub(super) fn activity_file_link_path(activity: &ActivityItem) -> Option<String> {
    match activity.kind {
        ActivityKind::FileRead | ActivityKind::FileList => activity
            .display_target
            .as_deref()
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .map(str::to_owned),
        ActivityKind::FileChange => match activity.file_changes.as_slice() {
            [change] => Some(change.path.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn render_sent_message_attachments(
    message_id: Uuid,
    attachments: &[MessageAttachment],
    attachment_menus: &[ContextMenuHandle],
    attachment_images: &[Option<Arc<gpui::Image>>],
    can_reveal: bool,
    waku: &gpui::WeakEntity<Waku>,
    theme: &Theme,
) -> Option<AnyElement> {
    if attachments.is_empty() {
        return None;
    }
    let mut row = div()
        .max_w(px(540.0))
        .flex()
        .flex_wrap()
        .justify_end()
        .gap(px(8.0));
    for (index, attachment) in attachments.iter().enumerate() {
        if let Some(session_id) = attachment.session_id {
            let navigate_waku = waku.clone();
            let key_waku = waku.clone();
            let chip = div()
                .id(SharedString::from(format!(
                    "message-{message_id}-attachment-{index}"
                )))
                .h(px(24.0))
                .max_w(px(240.0))
                .pl(px(6.0))
                .pr(px(10.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border)
                .bg(theme.inset)
                .flex()
                .items_center()
                .gap(px(5.0))
                .cursor_default()
                .tab_index(0)
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .hover(|element| element.bg(theme.overlay))
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
                .on_click(move |_, _, cx| {
                    let _ = navigate_waku.update(cx, |this, cx| {
                        this.select_session(session_id, cx);
                    });
                    cx.stop_propagation();
                })
                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        let _ = key_waku.update(cx, |this, cx| {
                            this.select_session(session_id, cx);
                        });
                        cx.stop_propagation();
                    }
                });
            row = row.child(chip);
            continue;
        }
        let Some(menu) = attachment_menus.get(index) else {
            continue;
        };
        if let Some(preview) = attachment.pasted_text_preview.as_ref() {
            let key_menu = menu.clone();
            let mut chip = div()
                .id(SharedString::from(format!(
                    "message-{message_id}-attachment-{index}"
                )))
                .h(px(24.0))
                .pl(px(6.0))
                .pr(px(10.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border)
                .bg(theme.inset)
                .flex()
                .items_center()
                .gap(px(5.0))
                .track_focus(menu.trigger_focus_handle())
                .tab_index(0)
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .child(icon("icons/file.svg", 11.0, theme.text_tertiary))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(tr!("composer.pasted_block")),
                );
            if !preview.is_empty() {
                chip = chip.tooltip(composer::pasted_text_tooltip(SharedString::from(
                    preview.clone(),
                )));
            }
            chip = chip.on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "f10" && event.keystroke.modifiers.shift {
                    key_menu.open_context_menu(window, cx);
                    cx.stop_propagation();
                }
            });
            let reveal_path = attachment.path.clone();
            row = row.child(context_menu(
                chip,
                SharedString::from(format!("message-{message_id}-attachment-{index}-menu")),
                menu,
                move |_| image_preview::attachment_menu_items(reveal_path.clone(), can_reveal),
            ));
            continue;
        }
        let icon_path = if attachment.is_dir {
            "icons/folder.svg"
        } else {
            right_panel::file_icon_for_path(&attachment.mention)
        };
        let attachment_image = attachment_images.get(index).and_then(|image| image.clone());
        let mut tile = div()
            .id(SharedString::from(format!(
                "message-{message_id}-attachment-{index}"
            )))
            .w(px(96.0))
            .h(px(80.0))
            .rounded(px(11.0))
            .overflow_hidden()
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.inset)
            .track_focus(menu.trigger_focus_handle())
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .tooltip(Tooltip::text(attachment.name.clone()));
        if attachment.is_image {
            let key_menu = menu.clone();
            if let Some(attachment_image) = attachment_image.as_ref() {
                let preview_waku = waku.clone();
                let key_waku = waku.clone();
                let preview_image = attachment_image.clone();
                let key_image = attachment_image.clone();
                let preview_name = SharedString::from(attachment.name.clone());
                let key_name = preview_name.clone();
                let preview_path = attachment.path.clone();
                let key_path = attachment.path.clone();
                tile = tile.child(
                    div()
                        .id(SharedString::from(format!(
                            "message-{message_id}-attachment-{index}-preview"
                        )))
                        .size_full()
                        .cursor_default()
                        .on_click(move |_, window, cx| {
                            let _ = preview_waku.update(cx, |this, cx| {
                                this.open_image_preview(
                                    preview_image.clone(),
                                    preview_name.clone(),
                                    preview_path.clone(),
                                    window,
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        })
                        .child(
                            img(attachment_image.clone())
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        ),
                );
                tile = tile.on_key_down(move |event: &KeyDownEvent, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if matches!(key, "enter" | "space") {
                        let _ = key_waku.update(cx, |this, cx| {
                            this.open_image_preview(
                                key_image.clone(),
                                key_name.clone(),
                                key_path.clone(),
                                window,
                                cx,
                            );
                        });
                        cx.stop_propagation();
                    } else if key == "f10" && event.keystroke.modifiers.shift {
                        key_menu.open_context_menu(window, cx);
                        cx.stop_propagation();
                    }
                });
            } else {
                tile = tile
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/file-types/image.svg", 18.0, theme.text_ghost)),
                    )
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "f10" && event.keystroke.modifiers.shift {
                            key_menu.open_context_menu(window, cx);
                            cx.stop_propagation();
                        }
                    });
            }
        } else {
            let key_menu = menu.clone();
            let click_waku = waku.clone();
            let key_waku = waku.clone();
            let click_path = attachment.path.to_string_lossy().into_owned();
            let key_path = click_path.clone();
            tile = tile
                .cursor_pointer()
                .child(
                    div()
                        .size_full()
                        .px(px(7.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(7.0))
                        .child(icon(icon_path, 18.0, theme.text_tertiary))
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .text_center()
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(attachment.name.clone()),
                        ),
                )
                .on_click(move |_, _, cx| {
                    let _ = click_waku.update(cx, |this, cx| {
                        this.open_path_in_default_app(&click_path, cx);
                    });
                    cx.stop_propagation();
                })
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if matches!(key, "enter" | "space") {
                        let _ = key_waku.update(cx, |this, cx| {
                            this.open_path_in_default_app(&key_path, cx);
                        });
                        cx.stop_propagation();
                    } else if key == "f10" && event.keystroke.modifiers.shift {
                        key_menu.open_context_menu(window, cx);
                        cx.stop_propagation();
                    }
                });
        }
        let reveal_path = attachment.path.clone();
        row = row.child(context_menu(
            tile,
            SharedString::from(format!("message-{message_id}-attachment-{index}-menu")),
            menu,
            move |_| image_preview::attachment_menu_items(reveal_path.clone(), can_reveal),
        ));
    }
    Some(row.into_any_element())
}

/// The GitHub references under a sent user message: one chip per `#N` the
/// composer resolved — mark, number, and title — each opening the item's URL.
fn render_work_item_ref_chips(
    message_id: Uuid,
    refs: &[ComposerWorkItem],
    theme: &Theme,
) -> AnyElement {
    let mut row = div()
        .max_w(px(540.0))
        .flex()
        .flex_wrap()
        .justify_end()
        .gap(px(6.0));
    for item in refs {
        let url = item.url.clone();
        let key_url = item.url.clone();
        let kind_icon = match item.kind {
            waku_client::WorkItemKind::Issue => "icons/info.svg",
            waku_client::WorkItemKind::PullRequest => "icons/git-pull-request-arrow.svg",
        };
        let chip = div()
            .id(SharedString::from(format!(
                "message-{message_id}-ref-{}",
                item.number
            )))
            .h(px(22.0))
            .max_w(px(280.0))
            .pl(px(5.0))
            .pr(px(8.0))
            .rounded(px(7.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.inset)
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|element| element.bg(theme.overlay))
            .tooltip(Tooltip::text(format!("#{} — {}", item.number, item.title)))
            .child(icon("icons/github.svg", 11.0, theme.text_tertiary))
            .child(icon(kind_icon, 10.0, theme.text_ghost))
            .child(
                div()
                    .flex_none()
                    .text_size(sp(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child(format!("#{}", item.number)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .child(item.title.clone()),
            )
            .on_click(move |_, _, cx| {
                cx.open_url(&url);
                cx.stop_propagation();
            })
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.open_url(&key_url);
                    cx.stop_propagation();
                }
            });
        row = row.child(chip);
    }
    row.into_any_element()
}

fn render_markdown_message_body<'a>(
    content: &str,
    markdown: Option<&'a MarkdownView>,
    theme: &Theme,
    ctx: &MarkdownCtx<'a>,
) -> AnyElement {
    markdown
        .and_then(|markdown| md::render::markdown(markdown, ctx))
        // Empty or not-yet-parsed content still needs a selectable fallback.
        .unwrap_or_else(|| {
            md::render::plain_text(
                content.to_owned(),
                ctx.families().ui.clone(),
                FontWeight::NORMAL,
                theme.text,
                ctx,
            )
        })
}

pub(super) fn render_message(params: MessageRender, cx: &mut App) -> AnyElement {
    let MessageRender {
        theme,
        message,
        assistant_footer_copy_content,
        assistant_footer_time,
        copied,
        show_response_token_speed,
        assistant_message_action,
        user_message_action,
        user_message_viewport,
        user_message_expanded,
        user_message_expand_focus,
        message_edit_input,
        attachment_menus,
        attachment_images,
        attachments_can_reveal,
        markdown,
        work_item_refs,
        ctx,
        menu,
        sent_by_task_link,
        auto_prompt_rule,
        waku,
        composer,
        landed_notice,
        transfer_notice,
    } = params;

    let content = message.visible_content().to_owned();
    // "Copy Message" must match what the row presents. The terminal part of a
    // settled response stands in for the whole visible answer, so its menu
    // shares the footer's copy content — parts hidden behind "Worked for X"
    // stay out — rather than copying the final part alone.
    let menu_copy_content = assistant_footer_copy_content
        .clone()
        .unwrap_or_else(|| SharedString::from(waku_protocol::model::atom_visible_text(&content)));
    let message_id = message.id;
    let role = message.role;
    let offer_speed_reader =
        role == MessageRole::Assistant && message.notice.is_none() && !content.trim().is_empty();
    let element = match role {
        MessageRole::User => {
            let group_name = SharedString::from(format!("user-message-{message_id}"));
            let mut column = div()
                .w_full()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(3.0))
                .group(group_name.clone());
            if message.sent_by_task.is_some() {
                // The chip looks the same whether or not its source task can
                // still be opened; a live target only adds activation.
                let chip = div()
                    .id(SharedString::from(format!("sent-by-agent-{message_id}")))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(7.0))
                    .py(px(2.0))
                    .rounded_full()
                    .bg(theme.overlay)
                    .child(icon("icons/bot.svg", 10.0, theme.text_tertiary))
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(tr!("transcript.sent_by_agent")),
                    )
                    .when_some(sent_by_task_link, |chip, task_id| {
                        let click_waku = waku.clone();
                        let key_waku = waku.clone();
                        chip.cursor_pointer()
                            .tab_index(0)
                            .focus_visible(|style| style.bg(theme.focus_highlight()))
                            .on_click(move |_, _, cx| {
                                let _ = click_waku.update(cx, |this, cx| {
                                    this.open_sent_by_task(task_id, cx);
                                });
                                cx.stop_propagation();
                            })
                            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    let _ = key_waku.update(cx, |this, cx| {
                                        this.open_sent_by_task(task_id, cx);
                                    });
                                    cx.stop_propagation();
                                }
                            })
                    });
                column = column.child(chip);
            }
            if let Some((rule_id, name)) = auto_prompt_rule {
                let click_waku = waku.clone();
                let key_waku = waku.clone();
                column = column.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .text_color(theme.text_tertiary)
                                .child(tr!("auto_prompts.sent_by", name = name)),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "disable-auto-prompt-{message_id}"
                                )))
                                .tab_index(0)
                                .px(px(6.0))
                                .py(px(3.0))
                                .rounded(px(5.0))
                                .focus_visible(|style| style.bg(theme.focus_highlight()))
                                .hover(|style| style.bg(theme.overlay))
                                .cursor_pointer()
                                .text_size(sp(12.5))
                                .text_color(theme.accent)
                                .child(tr!("auto_prompts.disable"))
                                .on_click(move |_, _, cx| {
                                    let _ = click_waku.update(cx, |this, cx| {
                                        this.set_auto_prompt_enabled(rule_id, false, cx);
                                    });
                                    cx.stop_propagation();
                                })
                                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        let _ = key_waku.update(cx, |this, cx| {
                                            this.set_auto_prompt_enabled(rule_id, false, cx);
                                        });
                                        cx.stop_propagation();
                                    }
                                }),
                        ),
                );
            }
            if let Some(attachments) = render_sent_message_attachments(
                message_id,
                &message.attachments,
                &attachment_menus,
                &attachment_images,
                attachments_can_reveal,
                &waku,
                theme,
            ) {
                column = column.child(attachments);
            }
            if let Some(edit_input) = message_edit_input {
                let can_submit = !edit_input.read(cx).content(cx).trim().is_empty()
                    || !message.attachments.is_empty();
                let cancel_waku = waku.clone();
                let submit_waku = waku.clone();
                column = column.child(
                    div()
                        .w_full()
                        .max_w(px(540.0))
                        .rounded(px(15.0))
                        .bg(theme.raised)
                        .pt(px(9.0))
                        .pb(px(8.0))
                        .child(edit_input)
                        .child(
                            div()
                                .mt(px(7.0))
                                .px(px(12.0))
                                .flex()
                                .justify_end()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "cancel-message-edit-{message_id}"
                                        )))
                                        .h(px(26.0))
                                        .px(px(10.0))
                                        .rounded(px(9.0))
                                        .border(hairline())
                                        .border_color(theme.border)
                                        .bg(theme.overlay)
                                        .flex()
                                        .items_center()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_secondary)
                                        .cursor_default()
                                        .hover(|element| element.bg(theme.overlay_strong))
                                        .child(tr_cow!("common.cancel"))
                                        .on_click(move |_, window, cx| {
                                            let _ = cancel_waku.update(cx, |this, cx| {
                                                this.cancel_message_edit(window, cx);
                                            });
                                        }),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "submit-message-edit-{message_id}"
                                        )))
                                        .h(px(26.0))
                                        .px(px(11.0))
                                        .rounded(px(9.0))
                                        .bg(if can_submit {
                                            theme.inverse
                                        } else {
                                            theme.overlay_strong
                                        })
                                        .flex()
                                        .items_center()
                                        .text_size(sp(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if can_submit {
                                            theme.on_inverse
                                        } else {
                                            theme.text_ghost
                                        })
                                        .when(can_submit, |element| {
                                            element
                                                .cursor_default()
                                                .hover(|element| element.opacity(0.9))
                                        })
                                        .child(tr_cow!("common.send"))
                                        .on_click(move |_, _, cx| {
                                            if can_submit {
                                                let _ = submit_waku.update(cx, |this, cx| {
                                                    this.submit_message_edit(cx);
                                                });
                                            }
                                        }),
                                ),
                        ),
                );
            } else {
                if !content.trim().is_empty() {
                    let body = render_markdown_message_body(&content, markdown, theme, ctx);
                    // A table claims no intrinsic width — its columns are
                    // fractions of the container — so without the full row a
                    // short message would shrink-wrap the bubble around its
                    // text and squash the table into it.
                    let has_table = markdown.is_some_and(MarkdownView::contains_table);
                    let overflowing = user_message_viewport
                        .map(|viewport| viewport.overflowing.get())
                        .unwrap_or(false);
                    let show_expander = !user_message_expanded && overflowing;
                    let viewport_max = USER_MESSAGE_VIEWPORT_MAX_HEIGHT
                        - if show_expander {
                            USER_MESSAGE_EXPANDER_HEIGHT
                        } else {
                            0.0
                        };
                    column = column.child(
                        div()
                            .id(SharedString::from(format!(
                                "user-message-bubble-{message_id}"
                            )))
                            .max_w(px(540.0))
                            .when(has_table, |bubble| bubble.w_full())
                            .when(!user_message_expanded, |bubble| {
                                bubble.max_h(px(USER_MESSAGE_MAX_HEIGHT))
                            })
                            .min_w_0()
                            .relative()
                            .overflow_hidden()
                            .rounded(px(15.0))
                            .border(hairline())
                            .border_color(theme.raised)
                            .bg(theme.raised)
                            .px(px(11.0))
                            .py(px(7.0))
                            .text_size(sp(14.0))
                            .line_height(sp(20.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .relative()
                                    .when(!user_message_expanded, |body| {
                                        body.max_h(px(viewport_max))
                                    })
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "user-message-scroll-{message_id}"
                                            )))
                                            .min_w_0()
                                            .when(!user_message_expanded, |body| {
                                                body.max_h(px(viewport_max))
                                            })
                                            // Track, don't scroll: the handle
                                            // still reports `max_offset` under
                                            // `overflow_hidden`, which both the
                                            // overflow probe and transcript
                                            // search's set_offset reveal use.
                                            .when_some(user_message_viewport, |body, viewport| {
                                                body.track_scroll(&viewport.scroll_handle)
                                            })
                                            .child(body),
                                    )
                                    // The clipped child records its overflow
                                    // during prepaint, so a later sibling sees
                                    // this frame's measurement. A flip re-renders
                                    // once, which is when the expander appears
                                    // or disappears.
                                    .when_some(user_message_viewport, |body, viewport| {
                                        let scroll = viewport.scroll_handle.clone();
                                        let overflowing = viewport.overflowing.clone();
                                        let owner = waku.entity_id();
                                        body.child(
                                            canvas(
                                                move |_, _, cx| {
                                                    let clipped = scroll.max_offset().y > px(0.5);
                                                    if overflowing.replace(clipped) != clipped {
                                                        cx.notify(owner);
                                                    }
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .absolute()
                                            .top_0()
                                            .left_0()
                                            .w(px(1.0))
                                            .h(px(1.0)),
                                        )
                                    })
                                    // Keep the fades inside the bubble's padding so
                                    // square fade quads cannot paint over its rounded corners.
                                    .when_some(user_message_viewport, |body, viewport| {
                                        body.child(scrollbar::edge_fade(
                                            viewport.scroll_handle.clone(),
                                            scrollbar::FadeEdge::Top,
                                            theme.raised,
                                        ))
                                        .child(
                                            scrollbar::edge_fade(
                                                viewport.scroll_handle.clone(),
                                                scrollbar::FadeEdge::Bottom,
                                                theme.raised,
                                            ),
                                        )
                                    }),
                            )
                            .when(show_expander, |bubble| {
                                let click_waku = waku.clone();
                                let key_waku = waku.clone();
                                bubble.child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "user-message-expand-{message_id}"
                                        )))
                                        .when_some(
                                            user_message_expand_focus.clone(),
                                            |button, focus| button.track_focus(&focus),
                                        )
                                        .tab_index(0)
                                        .tab_stop(true)
                                        .flex_none()
                                        .self_start()
                                        .mt(px(4.0))
                                        .h(px(20.0))
                                        .px(px(4.0))
                                        .ml(-px(4.0))
                                        .flex()
                                        .items_center()
                                        .cursor_default()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text_tertiary)
                                        .hover(|style| style.text_color(theme.text))
                                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                                        .child(tr!("transcript.show_more"))
                                        .on_click(move |_, _, cx| {
                                            let _ = click_waku.update(cx, |this, cx| {
                                                this.expand_user_message(message_id, cx);
                                            });
                                        })
                                        .on_key_down(move |event: &KeyDownEvent, _, cx| {
                                            if !event.keystroke.modifiers.modified()
                                                && matches!(
                                                    event.keystroke.key.as_str(),
                                                    "enter" | "space"
                                                )
                                            {
                                                let _ = key_waku.update(cx, |this, cx| {
                                                    this.expand_user_message(message_id, cx);
                                                });
                                                cx.stop_propagation();
                                            }
                                        }),
                                )
                            })
                            .when_some(user_message_viewport, |bubble, _| {
                                let key_menu = menu.clone();
                                bubble
                                    .track_focus(menu.trigger_focus_handle())
                                    .tab_group()
                                    .tab_index(0)
                                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if event.keystroke.key == "f10"
                                            && event.keystroke.modifiers.shift
                                        {
                                            key_menu.open_context_menu(window, cx);
                                            cx.stop_propagation();
                                        }
                                    })
                            }),
                    );
                }
                column = column.child(render_message_footer(
                    theme,
                    message,
                    message.created_at,
                    SharedString::from(content.clone()),
                    copied,
                    false,
                    group_name,
                    false,
                    true,
                    None,
                    user_message_action,
                    waku.clone(),
                ));
            }
            if !work_item_refs.is_empty() {
                column = column.child(render_work_item_ref_chips(
                    message_id,
                    &work_item_refs,
                    theme,
                ));
            }
            column
        }
        MessageRole::Assistant => {
            // Synthesized fallbacks ("Stopped", "Turn completed") stand in
            // for a reply the turn never produced; the kind's leading icon
            // keeps them from reading as agent prose.
            if let Some(TranscriptNotice::Status { kind }) = &message.notice {
                status_notice_row(*kind, &content, theme, ctx)
            } else if let Some(notice @ TranscriptNotice::TransferReceived { .. }) = &message.notice
            {
                transfer_notice_row(theme, message_id, notice, transfer_notice.as_ref(), &waku)
            } else {
                let group_name = SharedString::from(format!("assistant-message-{message_id}"));
                let body = render_markdown_message_body(&content, markdown, theme, ctx);
                let mut column = div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .py(px(4.0))
                    .gap(px(3.0))
                    .group(group_name.clone())
                    .child(body);
                // Turn-backed responses render one footer after every ordered row
                // in the turn. Only legacy/unkeyed assistant messages retain an
                // inline footer because they have no turn boundary to target.
                if message.turn_id.is_none()
                    && let Some(copy_content) = assistant_footer_copy_content
                {
                    column = column.child(render_message_footer(
                        theme,
                        message,
                        assistant_footer_time.unwrap_or(message.created_at),
                        copy_content,
                        copied,
                        show_response_token_speed,
                        group_name,
                        false,
                        false,
                        assistant_message_action,
                        None,
                        waku.clone(),
                    ));
                }
                column
            }
        }
        MessageRole::System => match &message.notice {
            Some(TranscriptNotice::Landed {
                base,
                commits,
                ahead,
            }) => landed_notice_row(
                theme,
                message_id,
                base,
                commits,
                *ahead,
                landed_notice.as_ref(),
                &waku,
                ctx,
            ),
            _ => {
                let status_icon = match &message.notice {
                    Some(TranscriptNotice::Status { kind }) => Some(status_notice_icon(*kind)),
                    _ => None,
                };
                div().w_full().flex().justify_center().child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded_full()
                        .bg(theme.overlay)
                        .text_size(sp(12.5))
                        .line_height(sp(16.0))
                        .when_some(status_icon, |row, path| {
                            row.child(icon(path, 11.0, theme.text_tertiary))
                        })
                        .child(md::render::plain_text(
                            content.clone(),
                            ctx.families().ui.clone(),
                            FontWeight::NORMAL,
                            theme.text_tertiary,
                            ctx,
                        )),
                )
            }
        },
    };

    let selection = ctx.selection().clone();
    context_menu(
        element.id(message_id),
        SharedString::from(format!("message-menu-{message_id}")),
        &menu,
        move |cx| {
            message_menu_items(
                &menu_copy_content,
                role,
                user_message_action,
                assistant_message_action,
                &selection,
                &composer,
                &waku,
                offer_speed_reader,
                cx,
            )
        },
    )
}

/// Which icon a [`TranscriptNotice::Status`] leads with — one Lucide glyph
/// per [`TranscriptNoticeStatus`].
fn status_notice_icon(status: TranscriptNoticeStatus) -> &'static str {
    match status {
        TranscriptNoticeStatus::Stopped => "icons/hand.svg",
        TranscriptNoticeStatus::Completed => "icons/circle-check.svg",
        TranscriptNoticeStatus::StoppedBeforeResponse => "icons/octagon-x.svg",
        TranscriptNoticeStatus::OutOfContext => "icons/battery-low.svg",
        TranscriptNoticeStatus::Declined => "icons/ban.svg",
        TranscriptNoticeStatus::StoppedWithReason => "icons/octagon-alert.svg",
        TranscriptNoticeStatus::Exited => "icons/unplug.svg",
        TranscriptNoticeStatus::StartFailed => "icons/circle-alert.svg",
        TranscriptNoticeStatus::Error => "icons/alert.svg",
        TranscriptNoticeStatus::Goal => "icons/goal.svg",
    }
}

/// A [`TranscriptNotice::Status`] rendered in the assistant column: the
/// kind's icon leading the notice text at pill weight so it reads as
/// chrome, not a reply.
#[track_caller]
fn status_notice_row(
    status: TranscriptNoticeStatus,
    content: &str,
    theme: &Theme,
    ctx: &MarkdownCtx,
) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(6.0))
        .py(px(4.0))
        .text_size(sp(12.5))
        .line_height(sp(16.0))
        .child(icon(status_notice_icon(status), 12.0, theme.text_tertiary))
        .child(md::render::plain_text(
            content.to_owned(),
            ctx.families().ui.clone(),
            FontWeight::NORMAL,
            theme.text_tertiary,
            ctx,
        ))
}

/// How many commits an expanded landed notice lists before folding the rest
/// behind a "Show N more commits" row.
const LANDED_NOTICE_SHOWN_COMMITS: usize = 5;

/// The landed card's push affordance: a button while the base is ahead of
/// its upstream, a spinner while a push runs, a check once the upstream
/// holds every commit. `None` hides it — no upstream, or the read has not
/// landed yet.
fn landed_push_control(
    message_id: Uuid,
    state: &LandedNoticeState,
    theme: &Theme,
    waku: &gpui::WeakEntity<Waku>,
) -> Option<AnyElement> {
    match &state.push {
        push_base::LandedPush::Pushable {
            workspace,
            base,
            upstream,
        } => {
            let click_waku = waku.clone();
            let key_waku = waku.clone();
            let click_workspace = workspace.clone();
            let click_base = base.clone();
            let key_workspace = workspace.clone();
            let key_base = base.clone();
            Some(
                div()
                    .id(SharedString::from(format!("landed-push-{message_id}")))
                    .track_focus(&state.push_focus)
                    .tab_index(0)
                    .h(px(22.0))
                    .px(px(7.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .cursor_default()
                    .text_size(sp(11.5))
                    .text_color(theme.text_secondary)
                    .hover(|style| style.bg(theme.overlay_strong).text_color(theme.text))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(icon("icons/cloud-upload.svg", 11.0, theme.text_secondary))
                    .child(tr!("push_base.button"))
                    .tooltip(Tooltip::text_with_action(
                        tr!(
                            "push_base.tooltip",
                            base = base.clone(),
                            upstream = upstream.clone()
                        ),
                        &PushBaseBranch,
                    ))
                    // The press stays inside the control: the header's own
                    // click toggles the disclosure.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = click_waku.update(cx, |waku, cx| {
                            waku.start_push_base(click_workspace.clone(), click_base.clone(), cx);
                        });
                    })
                    .on_key_down(move |event: &KeyDownEvent, _, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            cx.stop_propagation();
                            let _ = key_waku.update(cx, |waku, cx| {
                                waku.start_push_base(key_workspace.clone(), key_base.clone(), cx);
                            });
                        }
                    })
                    .into_any_element(),
            )
        }
        push_base::LandedPush::Pushing => Some(
            div()
                .h(px(22.0))
                .px(px(7.0))
                .flex()
                .items_center()
                .gap(px(5.0))
                .text_size(sp(11.5))
                .text_color(theme.text_secondary)
                .child(motion::spin(icon(
                    "icons/loader-circle.svg",
                    11.0,
                    theme.text_secondary,
                )))
                .child(tr!("commit.pushing"))
                .into_any_element(),
        ),
        push_base::LandedPush::Pushed => Some(
            div()
                .h(px(22.0))
                .px(px(7.0))
                .flex()
                .items_center()
                .gap(px(5.0))
                .text_size(sp(11.5))
                .text_color(theme.text_ghost)
                .child(icon("icons/circle-check.svg", 11.0, theme.success))
                .child(tr!("push_base.pushed_chip"))
                .into_any_element(),
        ),
        push_base::LandedPush::Hidden => None,
    }
}

/// The "Landed on `base`" card a [`TranscriptNotice::Landed`] renders as: a
/// collapsed disclosure header over the commit list, wearing the changed-files
/// card's chrome. The SHAs ride the ctx's commit-ref detection — enabled for
/// notice messages — so each one underlines and opens the commit diff on
/// click. `ahead` is the true landed count; `commits` may be capped shorter,
/// and what the daemon never sent can only surface as a count.
#[allow(clippy::too_many_arguments)]
fn landed_notice_row(
    theme: &Theme,
    message_id: Uuid,
    base: &str,
    commits: &[CommitEntry],
    ahead: u64,
    state: Option<&LandedNoticeState>,
    waku: &gpui::WeakEntity<Waku>,
    ctx: &MarkdownCtx,
) -> Div {
    let expanded = state.is_some_and(|state| state.expanded);
    let show_all = state.is_some_and(|state| state.show_all);
    let shown = if show_all {
        commits.len()
    } else {
        commits.len().min(LANDED_NOTICE_SHOWN_COMMITS)
    };
    let can_show_more = commits.len() > LANDED_NOTICE_SHOWN_COMMITS;
    let unloaded = ahead.saturating_sub(commits.len() as u64);

    let header_waku = waku.clone();
    let header_key_waku = waku.clone();
    let mut header = div()
        .id(SharedString::from(format!("landed-notice-{message_id}")))
        .h(px(34.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(icon("icons/git-merge.svg", 12.0, theme.text_tertiary))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .font_family(ctx.families().ui.clone())
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_secondary)
                .child(tr!("transcript.landed", base = base)),
        );
    if let Some(state) = state {
        header = header
            .track_focus(&state.header_focus)
            .tab_index(0)
            .cursor_default()
            // `overflow_hidden` clips to a rectangle, not the card's corner
            // curve, so the header's hover fill carries its own radius —
            // all four corners when the collapsed card ends with it.
            .rounded_t(px(13.0))
            .when(!expanded, |header| header.rounded_b(px(13.0)))
            .hover(|style| style.bg(theme.overlay_strong))
            .focus_visible(|style| style.bg(theme.overlay_strong))
            .when_some(
                landed_push_control(message_id, state, theme, waku),
                |header, control| header.child(control),
            )
            .child(icon(
                if expanded {
                    "icons/chevron-down.svg"
                } else {
                    "icons/chevron-right.svg"
                },
                11.0,
                theme.affordance_icon(),
            ))
            .on_click(move |_, _, cx| {
                let _ = header_waku.update(cx, |this, cx| {
                    this.toggle_landed_notice(message_id, cx);
                });
            })
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    let _ = header_key_waku.update(cx, |this, cx| {
                        this.toggle_landed_notice(message_id, cx);
                    });
                    cx.stop_propagation();
                }
            });
    }

    let mut card = div()
        .w_full()
        .min_w_0()
        .rounded(px(15.0))
        .border(hairline())
        .border_color(theme.border_subtle)
        .bg(theme.overlay)
        .text_size(sp(12.5))
        .line_height(sp(16.0))
        .overflow_hidden()
        .child(header);

    if expanded {
        let mut rows = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .border_t(hairline())
            .border_color(theme.separator);
        for commit in commits.iter().take(shown) {
            rows = rows.child(
                div()
                    .h(px(31.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_none().child(md::render::plain_text(
                        commit.short_sha.clone(),
                        ctx.families().code.clone(),
                        FontWeight::NORMAL,
                        theme.text_tertiary,
                        ctx,
                    )))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(md::render::plain_text(
                                commit.subject.clone(),
                                ctx.families().ui.clone(),
                                FontWeight::NORMAL,
                                theme.text_secondary,
                                ctx,
                            )),
                    ),
            );
        }
        // When every loaded commit is listed and `ahead` is still larger, the
        // daemon's cap — not this card — is what hides the rest.
        if !can_show_more && unloaded > 0 {
            rows = rows.child(div().h(px(31.0)).px(px(12.0)).flex().items_center().child(
                md::render::plain_text(
                    tr!("transcript.landed_more", count = unloaded),
                    ctx.families().ui.clone(),
                    FontWeight::NORMAL,
                    theme.text_ghost,
                    ctx,
                ),
            ));
        }
        card = card.child(rows);
    }

    if expanded && can_show_more {
        let commits_focus = state.map(|state| state.commits_focus.clone());
        let toggle_waku = waku.clone();
        let toggle_key_waku = waku.clone();
        let label = if show_all {
            tr!("transcript.show_fewer_commits")
        } else {
            tr!(
                "transcript.show_more_commits",
                count = commits.len() - LANDED_NOTICE_SHOWN_COMMITS
            )
        };
        card = card.child(
            div()
                .id(SharedString::from(format!("landed-commits-{message_id}")))
                .when_some(commits_focus, |row, focus| row.track_focus(&focus))
                .tab_index(0)
                .h(px(34.0))
                .px(px(12.0))
                .border_t(hairline())
                .border_color(theme.separator)
                .rounded_b(px(13.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .cursor_default()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_secondary)
                .focus_visible(|style| style.bg(theme.overlay_strong))
                .hover(|style| style.bg(theme.overlay_strong).text_color(theme.text))
                .active(|style| style.bg(theme.overlay))
                .child(SharedString::from(label))
                .when(show_all && unloaded > 0, |row| {
                    row.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme.text_ghost)
                            .child(tr!(
                                "transcript.showing_first_commits",
                                count = commits.len(),
                                total = ahead
                            )),
                    )
                })
                .child(div().flex_1())
                .child(icon(
                    if show_all {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    },
                    11.0,
                    theme.affordance_icon(),
                ))
                .on_click(move |_, _, cx| {
                    let _ = toggle_waku.update(cx, |this, cx| {
                        this.toggle_landed_notice_commits(message_id, cx);
                    });
                })
                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        let _ = toggle_key_waku.update(cx, |this, cx| {
                            this.toggle_landed_notice_commits(message_id, cx);
                        });
                        cx.stop_propagation();
                    }
                }),
        );
    }

    card
}

/// How many folder entries a transfer receipt lists before the rest fold
/// behind a "Show N more" row.
const TRANSFER_NOTICE_SHOWN_ENTRIES: usize = 8;

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// A [`TranscriptNotice::TransferReceived`] rendered as a payload card:
/// sender and title up top, then a folder's children or an image thumbnail —
/// a plain file needs no body, its meta line already carries kind and size.
/// Preview works while quarantined (decoding is read-only); Open stays
/// disabled until Trust clears the flag, with the reason on its tooltip.
fn transfer_notice_row(
    theme: &Theme,
    message_id: Uuid,
    notice: &TranscriptNotice,
    state: Option<&TransferNoticeState>,
    waku: &gpui::WeakEntity<Waku>,
) -> Div {
    let TranscriptNotice::TransferReceived {
        peer_name,
        title,
        path,
        is_dir,
        is_image,
        size_bytes,
        entries,
        entry_count,
    } = notice
    else {
        return div();
    };
    // `state` is missing only where the card renders without the
    // transcript's live session — fail closed rather than arming Open on an
    // unanswered quarantine flag.
    let quarantined = state.is_none_or(|state| state.quarantined);
    let can_open = state.is_some_and(|state| state.can_open);
    let show_all = state.is_some_and(|state| state.show_all);

    let from = tr!("friends.transfer_from", name = peer_name.clone()).to_string();
    let detail = if *is_dir {
        if *entry_count == 0 {
            tr!("friends.transfer_empty_folder").to_string()
        } else {
            tr!("friends.transfer_items", count = *entry_count).to_string()
        }
    } else {
        Path::new(title.as_str())
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_uppercase())
            .unwrap_or_else(|| tr!("friends.transfer_file").to_string())
    };
    let meta = format!("{from} · {detail} · {}", format_bytes(*size_bytes));

    let mut controls = div().flex_none().flex().items_center().gap(px(6.0));
    if let Some(state) = state {
        if quarantined {
            let session_id = state.session_id;
            let click_waku = waku.clone();
            let key_waku = waku.clone();
            controls = controls.child(
                div()
                    .id(SharedString::from(format!("transfer-trust-{message_id}")))
                    .track_focus(&state.trust_focus)
                    .tab_index(0)
                    .h(px(22.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .cursor_default()
                    .bg(theme.inverse)
                    .text_size(sp(11.5))
                    .text_color(theme.on_inverse)
                    .hover(|style| style.bg(theme.inverse.opacity(0.85)))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .child(icon("icons/lock-open.svg", 11.0, theme.on_inverse))
                    .child(tr!("friends.trust"))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = click_waku.update(cx, |this, cx| {
                            this.trust_transfer_session(session_id, cx);
                        });
                    })
                    .on_key_down(move |event: &KeyDownEvent, _, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            cx.stop_propagation();
                            let _ = key_waku.update(cx, |this, cx| {
                                this.trust_transfer_session(session_id, cx);
                            });
                        }
                    }),
            );
        }

        // Reveal only ever shows the payload's location — safe while
        // quarantined, meaningless for a remote host's path.
        let reveal_waku = waku.clone();
        let reveal_key_waku = waku.clone();
        let reveal_path = path.clone();
        let reveal_key_path = path.clone();
        let mut reveal = div()
            .id(SharedString::from(format!("transfer-reveal-{message_id}")))
            .track_focus(&state.reveal_focus)
            .tab_index(0)
            .h(px(22.0))
            .w(px(24.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default();
        if can_open {
            reveal = reveal
                .tooltip(Tooltip::text(tr!("common.reveal_in_finder")))
                .hover(|style| style.bg(theme.overlay_strong))
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .child(icon("icons/folder-open.svg", 12.0, theme.text_secondary))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = reveal_waku.update(cx, |_, cx| {
                        crate::platform::reveal_in_file_manager(&reveal_path, cx);
                    });
                })
                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                    if !event.keystroke.modifiers.modified()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        cx.stop_propagation();
                        let _ = reveal_key_waku.update(cx, |_, cx| {
                            crate::platform::reveal_in_file_manager(&reveal_key_path, cx);
                        });
                    }
                });
        } else {
            reveal = reveal
                .opacity(0.45)
                .tooltip(Tooltip::text(tr!("errors.remote_host_path")))
                .child(icon("icons/folder-open.svg", 12.0, theme.text_ghost));
        }
        controls = controls.child(reveal);

        let open_reason = if quarantined {
            Some(tr!("friends.transfer_open_quarantined"))
        } else if !can_open {
            Some(tr!("errors.remote_host_path"))
        } else {
            None
        };
        let open_waku = waku.clone();
        let open_key_waku = waku.clone();
        let open_path = path.to_string_lossy().into_owned();
        let open_key_path = open_path.clone();
        let mut open = div()
            .id(SharedString::from(format!("transfer-open-{message_id}")))
            .track_focus(&state.open_focus)
            .tab_index(0)
            .h(px(22.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(11.5));
        if let Some(reason) = open_reason {
            // The reason rides the button's tooltip; keyboard activation
            // toasts it so the cause isn't hover-only.
            let toast_reason = reason.clone();
            let key_reason = reason.clone();
            open = open
                .opacity(0.45)
                .text_color(theme.text_secondary)
                .tooltip(Tooltip::text(reason))
                .child(icon("icons/lock.svg", 11.0, theme.text_secondary))
                .child(tr!("friends.transfer_open"))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let reason = toast_reason.clone();
                    let _ = open_waku.update(cx, |this, cx| {
                        this.show_toast(reason.clone());
                        cx.notify();
                    });
                })
                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                    if !event.keystroke.modifiers.modified()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        cx.stop_propagation();
                        let reason = key_reason.clone();
                        let _ = open_key_waku.update(cx, |this, cx| {
                            this.show_toast(reason.clone());
                            cx.notify();
                        });
                    }
                });
        } else {
            open = open
                .text_color(theme.text_secondary)
                .hover(|style| style.bg(theme.overlay_strong).text_color(theme.text))
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .child(icon("icons/external-link.svg", 11.0, theme.text_secondary))
                .child(tr!("friends.transfer_open"))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = open_waku.update(cx, |this, cx| {
                        this.open_path_in_default_app(&open_path, cx);
                    });
                })
                .on_key_down(move |event: &KeyDownEvent, _, cx| {
                    if !event.keystroke.modifiers.modified()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        cx.stop_propagation();
                        let _ = open_key_waku.update(cx, |this, cx| {
                            this.open_path_in_default_app(&open_key_path, cx);
                        });
                    }
                });
        }
        controls = controls.child(open);
    }

    let header_icon = if *is_dir {
        "icons/folder.svg"
    } else {
        right_panel::file_icon_for_path(title)
    };
    let mut card = div()
        .w_full()
        .min_w_0()
        .rounded(px(15.0))
        .border(hairline())
        .border_color(theme.border_subtle)
        .bg(theme.overlay)
        .text_size(sp(12.5))
        .line_height(sp(16.0))
        .overflow_hidden()
        .child(
            div()
                .py(px(9.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(9.0))
                .child(icon(header_icon, 16.0, theme.text_tertiary))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(sp(11.5))
                                .text_color(theme.text_tertiary)
                                .child(meta),
                        ),
                )
                .child(controls),
        );

    if *is_dir && !entries.is_empty() {
        let shown = if show_all {
            entries.len()
        } else {
            entries.len().min(TRANSFER_NOTICE_SHOWN_ENTRIES)
        };
        let mut rows = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .border_t(hairline())
            .border_color(theme.separator);
        for entry in entries.iter().take(shown) {
            rows = rows.child(
                div()
                    .h(px(26.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(icon(
                        if entry.is_dir {
                            "icons/folder.svg"
                        } else {
                            right_panel::file_icon_for_path(&entry.name)
                        },
                        11.0,
                        theme.text_tertiary,
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_color(theme.text_secondary)
                            .child(entry.name.clone()),
                    )
                    .when(!entry.is_dir, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(sp(11.0))
                                .text_color(theme.text_ghost)
                                .child(format_bytes(entry.size_bytes)),
                        )
                    }),
            );
        }
        card = card.child(rows);

        let hidden_stored = entries.len().saturating_sub(shown);
        let unlisted = entry_count.saturating_sub(entries.len() as u64);
        if let Some(state) = state {
            if hidden_stored > 0 || show_all {
                let toggle_waku = waku.clone();
                let toggle_key_waku = waku.clone();
                let label = if show_all {
                    tr!("friends.transfer_show_fewer").to_string()
                } else {
                    tr!("friends.transfer_show_more", count = hidden_stored).to_string()
                };
                card = card.child(
                    div()
                        .id(SharedString::from(format!("transfer-entries-{message_id}")))
                        .track_focus(&state.entries_focus)
                        .tab_index(0)
                        .h(px(30.0))
                        .px(px(12.0))
                        .border_t(hairline())
                        .border_color(theme.separator)
                        .rounded_b(px(13.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .cursor_default()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .focus_visible(|style| style.bg(theme.overlay_strong))
                        .hover(|style| style.bg(theme.overlay_strong).text_color(theme.text))
                        .active(|style| style.bg(theme.overlay))
                        .child(SharedString::from(label))
                        .when(show_all && unlisted > 0, |row| {
                            row.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(theme.text_ghost)
                                    .child(tr!(
                                        "friends.transfer_showing_first",
                                        count = entries.len(),
                                        total = *entry_count
                                    )),
                            )
                        })
                        .child(div().flex_1())
                        .child(icon(
                            if show_all {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            },
                            11.0,
                            theme.affordance_icon(),
                        ))
                        .on_click(move |_, _, cx| {
                            let _ = toggle_waku.update(cx, |this, cx| {
                                this.toggle_transfer_notice_entries(message_id, cx);
                            });
                        })
                        .on_key_down(move |event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                let _ = toggle_key_waku.update(cx, |this, cx| {
                                    this.toggle_transfer_notice_entries(message_id, cx);
                                });
                                cx.stop_propagation();
                            }
                        }),
                );
            }
        } else if *entry_count > shown as u64 {
            // No live state (side surfaces) — the listing stays capped and
            // a ghost line reports the remainder.
            card = card.child(
                div()
                    .h(px(30.0))
                    .px(px(12.0))
                    .border_t(hairline())
                    .border_color(theme.separator)
                    .flex()
                    .items_center()
                    .text_color(theme.text_ghost)
                    .child(tr!(
                        "friends.transfer_more",
                        count = *entry_count - shown as u64
                    )),
            );
        }
    }

    if *is_image {
        let frame = match state.and_then(|state| state.image.clone()) {
            Some(image) => {
                let preview_waku = waku.clone();
                let key_waku = waku.clone();
                let preview_image = image.clone();
                let key_image = image.clone();
                let preview_name = SharedString::from(title.clone());
                let key_name = preview_name.clone();
                let preview_path = path.clone();
                let key_path = path.clone();
                let mut frame = div()
                    .id(SharedString::from(format!("transfer-image-{message_id}")))
                    .max_w(px(ACTIVITY_IMAGE_WIDTH))
                    .h(px(160.0))
                    .rounded(px(10.0))
                    .overflow_hidden()
                    .cursor_default()
                    .tooltip(Tooltip::text(tr!("friends.transfer_preview")))
                    .child(img(image).size_full().object_fit(ObjectFit::Cover));
                if let Some(state) = state {
                    frame = frame
                        .track_focus(&state.image_focus)
                        .tab_index(0)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = preview_waku.update(cx, |this, cx| {
                                this.open_image_preview(
                                    preview_image.clone(),
                                    preview_name.clone(),
                                    preview_path.clone(),
                                    window,
                                    cx,
                                );
                            });
                        })
                        .on_key_down(move |event: &KeyDownEvent, window, cx| {
                            if !event.keystroke.modifiers.modified()
                                && matches!(event.keystroke.key.as_str(), "enter" | "space")
                            {
                                cx.stop_propagation();
                                let _ = key_waku.update(cx, |this, cx| {
                                    this.open_image_preview(
                                        key_image.clone(),
                                        key_name.clone(),
                                        key_path.clone(),
                                        window,
                                        cx,
                                    );
                                });
                            }
                        });
                }
                frame
            }
            None => div()
                .id(SharedString::from(format!(
                    "transfer-image-empty-{message_id}"
                )))
                .max_w(px(ACTIVITY_IMAGE_WIDTH))
                .h(px(160.0))
                .rounded(px(10.0))
                .bg(theme.inset)
                .flex()
                .items_center()
                .justify_center()
                .child(icon("icons/file-types/image.svg", 18.0, theme.text_ghost)),
        };
        card = card.child(div().px(px(12.0)).pt(px(2.0)).pb(px(10.0)).child(frame));
    }

    card
}

/// The message row's context menu. Rebuilt on each open, so availability checks
/// here always reflect the current session state.
#[allow(clippy::too_many_arguments)]
fn message_menu_items(
    content: &str,
    role: MessageRole,
    user_message_action: Option<UserMessageAction>,
    assistant_message_action: Option<AssistantMessageAction>,
    selection: &TranscriptSelection,
    composer: &Entity<ComposerInput>,
    waku: &gpui::WeakEntity<Waku>,
    offer_speed_reader: bool,
    _cx: &mut App,
) -> Vec<MenuItem> {
    let mut items = Vec::new();

    let selected_text = selection.selection.borrow().selected_text();
    if let Some(selected) = selected_text.as_ref() {
        let copy = selected.clone();
        items.push(
            MenuItem::new(tr!("common.copy_selection"), move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
            })
            .shortcut_action(&CopySelection),
        );
        let selected = selected.clone();
        items.push(MenuItem::new(
            tr!("common.search_with_google"),
            move |_, cx| {
                cx.open_url(&crate::browser::search_url(&selected));
            },
        ));
    }

    let copy_content: Rc<str> = Rc::from(content);
    let copied_content = copy_content.clone();
    items.push(MenuItem::new(
        tr!("common.copy_message_title"),
        move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copied_content.to_string()));
        },
    ));

    if role == MessageRole::User && user_message_action.is_none() {
        let composer = composer.clone();
        let edit_content = content.to_owned();
        items.push(MenuItem::new(
            tr!("common.copy_to_composer"),
            move |window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_content(edit_content.clone(), cx);
                });
                let focus_handle = composer.read(cx).focus();
                window.focus(&focus_handle, cx);
            },
        ));
    }

    if let Some(code) = fenced_code(content) {
        items.push(MenuItem::new(tr!("common.copy_code"), move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
        }));
    }

    if offer_speed_reader {
        let reader_content = copy_content;
        let waku = waku.clone();
        items.push(MenuItem::Separator);
        items.push(MenuItem::new(tr!("speed_reader.go_fast"), move |window, cx| {
            let source = selected_text
                .clone()
                .unwrap_or_else(|| reader_content.to_string());
            let _ = waku.update(cx, |this, cx| {
                this.open_speed_reader(
                    tr!("speed_reader.agent_response"),
                    source.clone(),
                    window,
                    cx,
                );
            });
        }));
    }

    // A bot reply can ride the friends channel like a sent file — pick the
    // friend, confirm, and the daemon lands it as a chat on their side.
    if role == MessageRole::Assistant {
        let friends = waku
            .upgrade()
            .map(|waku| {
                let this = waku.read(_cx);
                this.state
                    .friends_enabled
                    .then(|| {
                        this.friends_state
                            .friends
                            .iter()
                            .map(|friend| {
                                (
                                    friend.node_id.clone(),
                                    friends::friend_display_name(friend).to_owned(),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if !friends.is_empty() {
            items.push(MenuItem::Separator);
            let text = content.to_owned();
            let waku = waku.clone();
            items.push(MenuItem::Submenu {
                label: tr!("friends.send_to_friend").into(),
                value: None,
                items: Rc::new(move |_cx| {
                    friends
                        .iter()
                        .map(|(node_id, name)| {
                            let waku = waku.clone();
                            let node_id = node_id.clone();
                            let name = name.clone();
                            let text = text.clone();
                            MenuItem::new(name.clone(), move |_, cx| {
                                let _ = waku.update(cx, |this, cx| {
                                    this.confirm_send_chat_to_friend(
                                        node_id.clone(),
                                        name.clone(),
                                        text.clone(),
                                        cx,
                                    );
                                });
                            })
                        })
                        .collect()
                }),
            });
        }
    }

    if let Some(action) = user_message_action {
        let waku = waku.clone();
        items.push(MenuItem::Separator);
        items.push(
            MenuItem::new(tr!("session.revert_to_here_title"), move |window, cx| {
                let _ = waku.update(cx, |this, cx| {
                    this.begin_message_edit(action, window, cx);
                });
            })
            .icon("icons/rewind.svg"),
        );
    }

    if let Some(action) = assistant_message_action {
        let waku = waku.clone();
        items.push(MenuItem::Separator);
        items.push(
            MenuItem::new(
                if action.enabled {
                    tr!("session.fork_task_title")
                } else {
                    tr!("session.forking_task_title")
                },
                move |_, cx| {
                    let _ = waku.update(cx, |this, cx| {
                        this.fork_session_from_response(action.session_id, action.turn_count, cx);
                    });
                },
            )
            .icon("icons/fork.svg")
            .disabled(!action.enabled),
        );
    }

    items
}

pub(super) fn fenced_code(content: &str) -> Option<String> {
    let mut code_blocks = Vec::new();
    let mut segments = content.split("```");
    let _ = segments.next();
    while let Some(fenced) = segments.next() {
        let (language, code) = fenced
            .split_once('\n')
            .map(|(language, code)| (language.trim(), code))
            .unwrap_or(("", fenced));
        let code = if language.is_empty() && !fenced.contains('\n') {
            fenced
        } else {
            code
        };
        if !code.trim().is_empty() {
            code_blocks.push(code.trim_end().to_owned());
        }
        let _ = segments.next();
    }
    (!code_blocks.is_empty()).then(|| code_blocks.join("\n\n"))
}

pub(super) fn activity_summary(activities: &[ActivityItem]) -> String {
    let mut counts: Vec<(crate::model::ActivityKind, usize)> = Vec::new();
    for activity in activities {
        if let Some(entry) = counts.iter_mut().find(|(kind, _)| *kind == activity.kind) {
            entry.1 += 1;
        } else {
            counts.push((activity.kind, 1));
        }
    }
    let parts = counts
        .into_iter()
        .map(|(kind, count)| {
            let (singular, plural) = activity_noun(kind);
            tr!(
                "activity.count",
                count = count,
                activity = if count == 1 { singular } else { plural }
            )
        })
        .collect::<Vec<_>>();
    let running = activities.iter().any(|activity| !activity.complete);
    if running {
        tr!("activity.running", activities = parts.join(" · "))
    } else {
        tr!("activity.ran", activities = parts.join(" · "))
    }
}

pub(super) fn activity_group_is_live(
    live_turn: bool,
    latest_block: bool,
    after_message: usize,
    message_count: usize,
) -> bool {
    live_turn && latest_block && after_message == message_count
}

pub(super) fn activity_header_title(
    activities: &[ActivityItem],
    live_group: bool,
    live_reasoning_id: Option<Uuid>,
) -> String {
    if live_group && let Some(activity) = activities.last() {
        return activity.reasoning.as_ref().map_or_else(
            || activity_display_title(activity),
            |reasoning| reasoning_activity_title(reasoning, live_reasoning_id == Some(activity.id)),
        );
    }

    activity_summary(activities)
}

fn tool_name_leaf(name: &str) -> &str {
    let name = name.trim();
    let leaf = name.rsplit("__").next().unwrap_or(name);
    leaf.rsplit([':', '.', '/']).next().unwrap_or(leaf)
}

fn is_ask_user_question(activity: &ActivityItem) -> bool {
    activity.kind == crate::model::ActivityKind::Tool
        && tool_name_leaf(&activity.title)
            .chars()
            .filter(|character| !matches!(*character, '_' | '-' | ' '))
            .flat_map(char::to_lowercase)
            .collect::<String>()
            == "askuserquestion"
}

fn humanize_tool_name(name: &str) -> String {
    let name = name.trim();
    if name.chars().any(char::is_whitespace) {
        return name.to_owned();
    }

    let leaf = tool_name_leaf(name);
    let characters = leaf.chars().collect::<Vec<_>>();
    let mut display = String::with_capacity(leaf.len() + 4);
    for (index, character) in characters.iter().copied().enumerate() {
        if matches!(character, '_' | '-') {
            if !display.ends_with(' ') {
                display.push(' ');
            }
            continue;
        }
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let starts_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_ascii_uppercase()
                        && next.is_some_and(|next| next.is_ascii_lowercase()))
            });
        if starts_word && !display.ends_with(' ') {
            display.push(' ');
        }
        display.push(character);
    }

    let display = display.trim();
    let mut characters = display.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
        .unwrap_or_else(|| tr!("activity.tool"))
}

fn activity_tool_display_name(activity: &ActivityItem) -> String {
    if is_ask_user_question(activity) {
        return tr!("activity.ask_questions");
    }
    if let Some(target) = activity
        .display_target
        .as_deref()
        .map(str::trim)
        .filter(|target| !target.is_empty())
    {
        return target.to_owned();
    }
    if !crate::model::is_generic_activity_title(activity.kind, &activity.title) {
        return humanize_tool_name(&activity.title);
    }
    tr!("activity.tool")
}

pub(super) fn activity_display_title(activity: &ActivityItem) -> String {
    use crate::model::ActivityKind;

    // A daemon-composed keyed label (e.g. "Searching for %{query}") renders in
    // this client's locale before any kind-label heuristic runs.
    if let Some(i18n) = &activity.title_i18n {
        return i18n.render();
    }
    match activity.kind {
        ActivityKind::FileChange => {
            let subject = match activity.file_changes.as_slice() {
                [change] => Some(change.display_name().to_owned()),
                changes if !changes.is_empty() => {
                    Some(tr!("activity.file_count", count = changes.len()))
                }
                _ => None,
            };
            if subject.is_none()
                && !crate::model::is_generic_activity_title(activity.kind, &activity.title)
            {
                return activity.title.clone();
            }
            match (activity.complete, activity.failed, subject) {
                (false, _, Some(file)) => tr!("activity.editing_named_file", file = file),
                (true, false, Some(file)) => tr!("activity.edited_named_file", file = file),
                (true, true, Some(file)) => tr!("activity.edit_failed_named_file", file = file),
                (false, _, None) => tr!("activity.editing_files"),
                (true, false, None) => tr!("activity.edited_files"),
                (true, true, None) => tr!("activity.edit_failed"),
            }
        }
        ActivityKind::FileRead => {
            let file = activity.display_target.as_deref().map(activity_path_name);
            if file.is_none()
                && !crate::model::is_generic_activity_title(activity.kind, &activity.title)
            {
                return activity.title.clone();
            }
            match (activity.complete, activity.failed, file) {
                (false, _, Some(file)) => tr!("activity.reading_named_file", file = file),
                (true, false, Some(file)) => tr!("activity.read_named_file", file = file),
                (true, true, Some(file)) => tr!("activity.read_named_file_failed", file = file),
                (false, _, None) => tr!("activity.reading_file"),
                (true, false, None) => tr!("activity.read_file_completed"),
                (true, true, None) => tr!("activity.read_file_failed"),
            }
        }
        ActivityKind::FileSearch => {
            let query = activity.display_target.as_deref();
            if query.is_none()
                && !crate::model::is_generic_activity_title(activity.kind, &activity.title)
            {
                return activity.title.clone();
            }
            match (activity.complete, activity.failed, query) {
                (false, _, Some(query)) => tr!("activity.searching_files_for", query = query),
                (true, false, Some(query)) => tr!("activity.searched_files_for", query = query),
                (true, true, Some(query)) => tr!("activity.file_search_failed_for", query = query),
                (false, _, None) => tr!("activity.searching_files"),
                (true, false, None) => tr!("activity.searched_files"),
                (true, true, None) => tr!("activity.file_search_failed"),
            }
        }
        ActivityKind::FileList => {
            let directory = activity.display_target.as_deref().map(activity_path_name);
            if directory.is_none()
                && !crate::model::is_generic_activity_title(activity.kind, &activity.title)
            {
                return activity.title.clone();
            }
            match (activity.complete, activity.failed, directory) {
                (false, _, Some(directory)) => {
                    tr!("activity.listing_files_in", directory = directory)
                }
                (true, false, Some(directory)) => {
                    tr!("activity.listed_files_in", directory = directory)
                }
                (true, true, Some(directory)) => {
                    tr!("activity.file_list_failed_in", directory = directory)
                }
                (false, _, None) => tr!("activity.listing_files"),
                (true, false, None) => tr!("activity.listed_files"),
                (true, true, None) => tr!("activity.file_list_failed"),
            }
        }
        ActivityKind::Command => {
            if let Some(description) = activity.display_description.as_deref() {
                return match (activity.complete, activity.failed) {
                    (false, _) => {
                        tr!(
                            "activity.running_described_command",
                            description = description
                        )
                    }
                    (true, false) => {
                        tr!("activity.ran_described_command", description = description)
                    }
                    (true, true) => {
                        tr!(
                            "activity.described_command_failed",
                            description = description
                        )
                    }
                };
            }
            if let Some(command) = activity.display_target.as_deref() {
                return match (activity.complete, activity.failed) {
                    (false, _) => tr!("activity.running_named_command", command = command),
                    (true, false) => tr!("activity.ran_named_command", command = command),
                    (true, true) => tr!("activity.named_command_failed", command = command),
                };
            }
            if !crate::model::is_generic_activity_title(activity.kind, &activity.title) {
                return activity.title.clone();
            }
            match (activity.complete, activity.failed) {
                (false, _) => tr!("activity.running_command"),
                (true, false) => tr!("activity.ran_command"),
                (true, true) => tr!("activity.command_failed"),
            }
        }
        ActivityKind::Search => {
            if let Some(query) = activity.display_target.as_deref() {
                return match (activity.complete, activity.failed) {
                    (false, _) => tr!("activity.searching_web_for", query = query),
                    (true, false) => tr!("activity.searched_web_for", query = query),
                    (true, true) => tr!("activity.web_search_failed_for", query = query),
                };
            }
            if ActivityKind::from_tool_name(&activity.title) == ActivityKind::Search {
                return match (activity.complete, activity.failed) {
                    (false, _) => tr!("activity.searching_web"),
                    (true, false) => tr!("activity.searched_the_web"),
                    (true, true) => tr!("activity.web_search_failed"),
                };
            }
            activity.title.clone()
        }
        ActivityKind::Plan => {
            if !crate::model::is_generic_activity_title(activity.kind, &activity.title) {
                return activity.title.clone();
            }
            match (activity.complete, activity.failed) {
                (false, _) => tr!("activity.updating_plan"),
                (true, false) => tr!("activity.updated_plan"),
                (true, true) => tr!("activity.plan_update_failed"),
            }
        }
        ActivityKind::Tool => activity_tool_display_name(activity),
        ActivityKind::ProjectMap | ActivityKind::Reasoning => activity.title.clone(),
    }
}

pub(super) fn activity_action_label(activity: &ActivityItem) -> String {
    use crate::model::ActivityKind;

    match activity.kind {
        ActivityKind::Reasoning => tr!("activity.action_think"),
        ActivityKind::Command => tr!("activity.action_run"),
        ActivityKind::FileChange => tr!("activity.action_edit"),
        ActivityKind::FileRead => tr!("activity.action_read"),
        ActivityKind::FileSearch | ActivityKind::Search => tr!("activity.action_search"),
        ActivityKind::FileList => tr!("activity.action_list"),
        ActivityKind::Plan => tr!("activity.action_plan"),
        ActivityKind::Tool if is_ask_user_question(activity) => tr!("activity.ask_questions"),
        ActivityKind::Tool => tr!("activity.tool"),
        ActivityKind::ProjectMap => tr!("project_map.action_label"),
    }
}

pub(super) fn activity_row_detail(activity: &ActivityItem, reasoning_live: bool) -> String {
    use crate::model::ActivityKind;

    let custom_title = || {
        (!crate::model::is_generic_activity_title(activity.kind, &activity.title))
            .then(|| activity.title.clone())
    };
    match activity.kind {
        ActivityKind::Reasoning => activity.reasoning.as_ref().map_or_else(
            || activity.title.clone(),
            |reasoning| reasoning_activity_title(reasoning, reasoning_live),
        ),
        ActivityKind::Command => activity
            .display_description
            .clone()
            .or_else(|| activity.display_target.clone())
            .or_else(custom_title)
            .unwrap_or_default(),
        ActivityKind::FileChange => match activity.file_changes.as_slice() {
            [change] => change.display_name().to_owned(),
            changes if !changes.is_empty() => {
                tr!("activity.file_count", count = changes.len())
            }
            _ => custom_title().unwrap_or_default(),
        },
        ActivityKind::FileRead | ActivityKind::FileList => activity
            .display_target
            .as_deref()
            .map(activity_path_name)
            .or_else(custom_title)
            .unwrap_or_default(),
        ActivityKind::FileSearch => activity_display_title(activity),
        ActivityKind::Search => activity.display_target.as_deref().map_or_else(
            || custom_title().unwrap_or_default(),
            |query| tr!("activity.search_for", query = query),
        ),
        ActivityKind::Plan => custom_title().unwrap_or_default(),
        ActivityKind::Tool if is_ask_user_question(activity) => String::new(),
        ActivityKind::Tool => {
            let has_name = activity
                .display_target
                .as_deref()
                .is_some_and(|target| !target.trim().is_empty())
                || !crate::model::is_generic_activity_title(activity.kind, &activity.title);
            has_name
                .then(|| activity_tool_display_name(activity))
                .unwrap_or_default()
        }
        // The title already says files and tokens; the map itself lives in
        // the disclosure's detail section.
        ActivityKind::ProjectMap => String::new(),
    }
}

pub(super) fn reasoning_activity_title(reasoning: &ReasoningBlock, live: bool) -> String {
    if live {
        tr!("transcript.thinking")
    } else {
        tr!(
            "transcript.thought_for",
            duration = format_worked_duration(
                reasoning
                    .finished_at_ms
                    .saturating_sub(reasoning.started_at_ms)
                    .div_ceil(1000)
                    .max(1)
            )
        )
    }
}

fn activity_path_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// Whether this activity's expanded view shows a diff instead of the tool
/// arguments that produced it.
pub(super) fn activity_shows_diff(activity: &ActivityItem) -> bool {
    activity.kind == ActivityKind::FileChange
        && activity
            .file_changes
            .iter()
            .any(|change| change.diff.is_some())
}

pub(super) fn activity_file_change_stats(activity: &ActivityItem) -> Option<(u64, u64)> {
    if activity.kind != crate::model::ActivityKind::FileChange
        || !activity.complete
        || activity.failed
        || activity.file_changes.is_empty()
    {
        return None;
    }
    let additions = activity
        .file_changes
        .iter()
        .map(|change| change.additions)
        .sum::<Option<u64>>()?;
    let deletions = activity
        .file_changes
        .iter()
        .map(|change| change.deletions)
        .sum::<Option<u64>>()?;
    Some((additions, deletions))
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum ActivityDisclosureSectionKind {
    McpServer,
    ToolName,
    Command,
    Arguments,
    Output,
    Detail,
}

impl ActivityDisclosureSectionKind {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::McpServer => "mcp-server",
            Self::ToolName => "tool-name",
            Self::Command => "command",
            Self::Arguments => "arguments",
            Self::Output => "output",
            Self::Detail => "detail",
        }
    }

    pub(super) fn label(self) -> Option<String> {
        match self {
            Self::McpServer => Some(tr!("activity.mcp_server")),
            Self::ToolName => Some(tr!("activity.tool_name")),
            Self::Command => Some(tr!("activity.command_detail")),
            Self::Arguments => Some(tr!("activity.arguments")),
            Self::Output => Some(tr!("activity.output")),
            Self::Detail => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ActivityDisclosureSection {
    pub(super) kind: ActivityDisclosureSectionKind,
    pub(super) content: String,
}

pub(super) fn activity_disclosure_sections(
    activity: &ActivityItem,
) -> Vec<ActivityDisclosureSection> {
    let mut sections = Vec::new();
    for (kind, content) in [
        (
            ActivityDisclosureSectionKind::McpServer,
            activity.mcp_server.as_deref(),
        ),
        (
            ActivityDisclosureSectionKind::ToolName,
            activity.tool_name.as_deref(),
        ),
    ] {
        if let Some(content) = content.map(str::trim).filter(|content| !content.is_empty()) {
            sections.push(ActivityDisclosureSection {
                kind,
                content: content.to_owned(),
            });
        }
    }
    let metadata_count = sections.len();
    if activity.kind == ActivityKind::Command {
        if let Some(command) = activity
            .arguments
            .as_deref()
            .or(activity.display_target.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sections.push(ActivityDisclosureSection {
                kind: ActivityDisclosureSectionKind::Command,
                content: command.to_owned(),
            });
        }
        if let Some(output) = activity
            .output
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let truncated = if activity.output_truncated {
                tr!("activity.output_truncated")
            } else {
                String::new()
            };
            sections.push(ActivityDisclosureSection {
                kind: ActivityDisclosureSectionKind::Output,
                content: format!("{output}{truncated}"),
            });
        } else if !activity.image_urls.is_empty() {
            sections.push(ActivityDisclosureSection {
                kind: ActivityDisclosureSectionKind::Output,
                content: String::new(),
            });
        }
        return sections;
    }
    // An edit renders as a diff, which says everything the raw arguments would
    // and reads. What the tool replied is only worth the room when it failed.
    let shows_diff = activity_shows_diff(activity);
    if let Some(arguments) = activity
        .arguments
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| !shows_diff)
    {
        sections.push(ActivityDisclosureSection {
            kind: ActivityDisclosureSectionKind::Arguments,
            content: arguments.to_owned(),
        });
    }
    if let Some(output) = activity
        .output
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| !shows_diff || activity.failed)
    {
        let truncated = if activity.output_truncated {
            tr!("activity.output_truncated")
        } else {
            String::new()
        };
        sections.push(ActivityDisclosureSection {
            kind: ActivityDisclosureSectionKind::Output,
            content: format!("{output}{truncated}"),
        });
    } else if !activity.image_urls.is_empty() {
        sections.push(ActivityDisclosureSection {
            kind: ActivityDisclosureSectionKind::Output,
            content: String::new(),
        });
    }
    if sections.len() == metadata_count
        && let Some(detail) = activity
            .detail
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        sections.push(ActivityDisclosureSection {
            kind: ActivityDisclosureSectionKind::Detail,
            content: detail.to_owned(),
        });
    }
    sections
}

pub(super) fn activity_preview(activity: &ActivityItem) -> String {
    let detail = activity.detail.as_deref().unwrap_or_default().trim();
    if detail.eq_ignore_ascii_case("failed")
        && let Some(output) = activity.output.as_deref()
        && let Some(first_line) = output.lines().find(|line| !line.trim().is_empty())
    {
        return first_line.trim().to_owned();
    }
    if (detail.is_empty() || detail.eq_ignore_ascii_case("failed"))
        && !activity.image_urls.is_empty()
    {
        return tr!("activity.image_output");
    }
    detail.to_owned()
}

#[cfg(test)]
mod message_time_tests {
    use super::*;
    use chrono::TimeZone;

    /// Test-only rendering of disclosure sections into plain text; production
    /// renders them interactively via [`activity_disclosure_sections`].
    fn activity_disclosure_text(activity: &ActivityItem) -> Option<String> {
        let sections = activity_disclosure_sections(activity);
        (!sections.is_empty()).then(|| {
            sections
                .into_iter()
                .map(
                    |section| match (section.kind.label(), section.content.is_empty()) {
                        (Some(label), false) => format!("{label}\n{}", section.content),
                        (Some(label), true) => label.to_owned(),
                        (None, _) => section.content,
                    },
                )
                .collect::<Vec<_>>()
                .join("\n\n")
        })
    }

    fn local_datetime(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .expect("test date should be valid in the local timezone")
    }

    fn unix_seconds(timestamp: DateTime<Local>) -> u64 {
        timestamp
            .timestamp()
            .try_into()
            .expect("test date should have a positive Unix timestamp")
    }

    #[test]
    fn response_footer_shows_estimated_tokens_per_second() {
        let mut message = Message::new(MessageRole::Assistant, "a".repeat(40));
        message.created_at = 10;
        assert_eq!(response_tokens_per_second(&message, 12), Some(5));
        assert_eq!(response_tokens_per_second(&message, 10), Some(10));
        assert_eq!(
            response_tokens_per_second(&Message::new(MessageRole::User, "a".repeat(40)), 12),
            None
        );
    }

    #[test]
    fn message_time_includes_calendar_context_for_older_messages() {
        let now = local_datetime(2026, 8, 9, 16, 0); // Sunday

        assert_eq!(
            format_message_time_at(unix_seconds(local_datetime(2026, 8, 9, 9, 5)), now),
            "9:05 AM"
        );
        assert_eq!(
            format_message_time_at(unix_seconds(local_datetime(2026, 8, 8, 17, 0)), now),
            "Yesterday 5:00 PM"
        );
        assert_eq!(
            format_message_time_at(unix_seconds(local_datetime(2026, 8, 7, 13, 12)), now),
            "Friday 1:12 PM"
        );
        assert_eq!(
            format_message_time_at(unix_seconds(local_datetime(2026, 5, 12, 23, 0)), now),
            "May 12th, 11:00 PM"
        );
        assert_eq!(
            format_message_time_at(unix_seconds(local_datetime(2024, 8, 4, 11, 0)), now),
            "Aug 4th 2024, 11:00 AM"
        );
    }

    #[test]
    fn message_time_uses_correct_ordinal_suffixes() {
        let now = local_datetime(2026, 8, 9, 16, 0);

        for (day, suffix) in [
            (1, "st"),
            (2, "nd"),
            (3, "rd"),
            (11, "th"),
            (12, "th"),
            (13, "th"),
            (21, "st"),
        ] {
            let formatted =
                format_message_time_at(unix_seconds(local_datetime(2026, 5, day, 9, 0)), now);
            assert!(formatted.starts_with(&format!("May {day}{suffix},")));
        }
    }

    #[test]
    fn activity_disclosure_distinguishes_mcp_identity_from_the_title() {
        let activity = ActivityItem::new(
            Some("tool-1".into()),
            crate::model::ActivityKind::Tool,
            "List running apps via CUA",
            None,
            true,
        )
        .with_tool_name(Some("js"))
        .with_mcp_server(Some("goddard_js_repl"))
        .with_arguments(Some("{}".into()));
        assert_eq!(
            activity_display_title(&activity),
            "List running apps via CUA"
        );
        assert_eq!(
            activity_disclosure_sections(&activity),
            vec![
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::McpServer,
                    content: "goddard_js_repl".into()
                },
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::ToolName,
                    content: "js".into()
                },
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::Arguments,
                    content: "{}".into()
                },
            ]
        );

        let regular = ActivityItem::new(
            None,
            crate::model::ActivityKind::Tool,
            "Read notes",
            Some("Could not read notes".into()),
            true,
        )
        .with_tool_name(Some("read_file"));
        assert_eq!(
            activity_disclosure_sections(&regular),
            vec![
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::ToolName,
                    content: "read_file".into()
                },
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::Detail,
                    content: "Could not read notes".into()
                },
            ]
        );
    }

    #[test]
    fn activity_disclosure_keeps_arguments_and_output() {
        let activity = ActivityItem::new(
            Some("tool-1".into()),
            crate::model::ActivityKind::Tool,
            "Use Helium",
            Some("failed".into()),
            true,
        )
        .with_arguments(Some("{\n  \"actions\": []\n}".into()))
        .with_output(Some("Computer Use helper closed its session".into()))
        .with_failed(true);

        assert_eq!(
            activity_disclosure_sections(&activity),
            vec![
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::Arguments,
                    content: "{\n  \"actions\": []\n}".into(),
                },
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::Output,
                    content: "Computer Use helper closed its session".into(),
                },
            ]
        );
        assert_eq!(
            activity_disclosure_text(&activity).as_deref(),
            Some(
                "Arguments\n{\n  \"actions\": []\n}\n\nOutput\nComputer Use helper closed its session"
            )
        );
        assert_eq!(
            activity_preview(&activity),
            "Computer Use helper closed its session"
        );

        let image_only = ActivityItem::new(
            Some("tool-2".into()),
            crate::model::ActivityKind::Tool,
            "Screenshot",
            None,
            true,
        )
        .with_image_urls(vec!["data:image/png;base64,aGVsbG8=".into()]);
        assert_eq!(
            activity_disclosure_text(&image_only).as_deref(),
            Some("Output")
        );
        assert_eq!(activity_preview(&image_only), "Image output");
    }

    #[test]
    fn command_disclosure_shows_only_the_command_and_output() {
        let activity = ActivityItem::new(
            Some("command-1".into()),
            crate::model::ActivityKind::Command,
            "bash",
            Some("Completed".into()),
            true,
        )
        .with_arguments(Some(
            r#"{"command":"git status --short","description":"Check status"}"#.into(),
        ))
        .with_output(Some("clean".into()));

        assert_eq!(
            activity_disclosure_sections(&activity),
            vec![
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::Command,
                    content: "git status --short".into(),
                },
                ActivityDisclosureSection {
                    kind: ActivityDisclosureSectionKind::Output,
                    content: "clean".into(),
                },
            ]
        );
        assert_eq!(
            activity_disclosure_text(&activity).as_deref(),
            Some("Command\ngit status --short\n\nOutput\nclean")
        );
    }

    #[test]
    fn activity_display_title_prefers_the_human_facing_tool_argument() {
        let titled = ActivityItem::new(
            Some("tool-1".into()),
            crate::model::ActivityKind::Tool,
            "Js",
            None,
            true,
        )
        .with_arguments(Some(
            r#"{"title":"Inspect Helium browser","code":"sky.get_app_state()"}"#.into(),
        ));
        let untitled = ActivityItem::new(
            Some("tool-2".into()),
            crate::model::ActivityKind::Tool,
            "Js",
            None,
            true,
        )
        .with_arguments(Some(r#"{"code":"sky.list_apps()"}"#.into()));

        assert_eq!(activity_display_title(&titled), "Inspect Helium browser");
        assert_eq!(activity_display_title(&untitled), "Js");
    }

    #[test]
    fn generic_tool_rows_keep_a_humanized_provider_name() {
        let named = ActivityItem::new(
            Some("tool-1".into()),
            crate::model::ActivityKind::Tool,
            "mcp__threads__create_thread",
            None,
            true,
        );
        let unnamed = ActivityItem::new(
            Some("tool-2".into()),
            crate::model::ActivityKind::Tool,
            "Tool",
            None,
            true,
        );

        assert_eq!(activity_action_label(&named), "Tool");
        assert_eq!(activity_row_detail(&named, false), "Create thread");
        assert_eq!(activity_display_title(&named), "Create thread");
        assert_eq!(activity_action_label(&unnamed), "Tool");
        assert_eq!(activity_row_detail(&unnamed, false), "");
    }

    #[test]
    fn ask_user_question_has_a_purpose_specific_label() {
        let activity = ActivityItem::new(
            Some("tool-1".into()),
            crate::model::ActivityKind::Tool,
            "AskUserQuestion",
            None,
            true,
        )
        .with_arguments(Some(r#"{"questions":[]}"#.into()));

        assert_eq!(activity_action_label(&activity), "Ask questions");
        assert_eq!(activity_row_detail(&activity, false), "");
        assert_eq!(activity_display_title(&activity), "Ask questions");
    }

    #[test]
    fn activity_header_summarizes_only_after_the_group_leaves_the_live_tail() {
        let reasoning = ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "Inspecting history".into(),
                started_at_ms: 1_000,
                finished_at_ms: 2_000,
            },
            true,
        );
        let command = ActivityItem::new(
            Some("command-1".into()),
            crate::model::ActivityKind::Command,
            "bash",
            None,
            false,
        )
        .with_arguments(Some(
            serde_json::json!({"command": "git log --oneline -15"}).to_string(),
        ));
        let mut activities = vec![reasoning, command];

        assert_eq!(
            activity_header_title(&activities, true, None),
            "Running git log --oneline -15"
        );
        assert!(activity_group_is_live(true, true, 1, 1));
        activities[1].complete = true;
        assert_eq!(
            activity_header_title(&activities, true, None),
            "Ran git log --oneline -15"
        );
        assert!(!activity_group_is_live(true, true, 1, 2));
        assert_eq!(
            activity_header_title(&activities, false, None),
            "Ran 1 thought · 1 command"
        );
        assert!(!activity_group_is_live(true, false, 1, 1));
        assert!(!activity_group_is_live(false, true, 1, 1));
        assert_eq!(activity_action_label(&activities[1]), "Run");
        assert_eq!(
            activity_row_detail(&activities[1], false),
            "git log --oneline -15"
        );
    }

    #[test]
    fn file_edit_title_and_stats_follow_the_activity_state() {
        let mut activity = ActivityItem::new(
            Some("edit-1".into()),
            crate::model::ActivityKind::FileChange,
            "apply_patch",
            None,
            false,
        )
        .with_arguments(Some(
            serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: /tmp/waku/src/app.rs\n@@\n-old\n+new\n+more\n*** End Patch"
            })
            .to_string(),
        ));

        assert_eq!(activity_display_title(&activity), "Editing app.rs");
        assert_eq!(activity_file_change_stats(&activity), None);

        activity.complete = true;
        assert_eq!(activity_display_title(&activity), "Edited app.rs");
        assert_eq!(activity_file_change_stats(&activity), Some((2, 1)));

        activity.failed = true;
        assert_eq!(activity_display_title(&activity), "Failed to edit app.rs");
        assert_eq!(activity_file_change_stats(&activity), None);
    }

    #[test]
    fn multi_file_edits_use_a_compact_count() {
        let activity = ActivityItem::new(
            Some("edit-2".into()),
            crate::model::ActivityKind::FileChange,
            "apply_patch",
            None,
            true,
        )
        .with_arguments(Some(
            serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/a.rs\n@@\n-a\n+b\n*** Update File: src/b.rs\n@@\n-c\n+d\n*** End Patch"
            })
            .to_string(),
        ));

        assert_eq!(activity_display_title(&activity), "Edited 2 files");
        assert_eq!(activity_file_change_stats(&activity), Some((2, 2)));
    }

    #[test]
    fn file_tool_titles_include_the_target_and_state() {
        let mut read = ActivityItem::new(
            Some("read-1".into()),
            crate::model::ActivityKind::FileRead,
            "read",
            None,
            false,
        )
        .with_arguments(Some(
            serde_json::json!({"filePath": "/tmp/waku/src/model.rs"}).to_string(),
        ));
        assert_eq!(activity_display_title(&read), "Reading model.rs");
        read.complete = true;
        assert_eq!(activity_display_title(&read), "Read model.rs");
        read.failed = true;
        assert_eq!(activity_display_title(&read), "Failed to read model.rs");

        let search = ActivityItem::new(
            Some("grep-1".into()),
            crate::model::ActivityKind::FileSearch,
            "grep",
            None,
            true,
        )
        .with_arguments(Some(
            serde_json::json!({"pattern": "ActivityKind"}).to_string(),
        ));
        assert_eq!(
            activity_display_title(&search),
            "Searched files for ActivityKind"
        );

        let list = ActivityItem::new(
            Some("list-1".into()),
            crate::model::ActivityKind::FileList,
            "ls",
            None,
            false,
        )
        .with_arguments(Some(
            serde_json::json!({"path": "/tmp/waku/src"}).to_string(),
        ));
        assert_eq!(activity_display_title(&list), "Listing files in src");

        let custom = ActivityItem::new(
            Some("read-2".into()),
            crate::model::ActivityKind::FileRead,
            "Inspect generated manifest",
            None,
            true,
        );
        assert_eq!(
            activity_display_title(&custom),
            "Inspect generated manifest"
        );
    }

    #[test]
    fn command_web_search_and_plan_titles_include_their_state() {
        let mut command = ActivityItem::new(
            Some("command-1".into()),
            crate::model::ActivityKind::Command,
            "bash",
            None,
            true,
        )
        .with_arguments(Some(
            serde_json::json!({
                "description": "Run focused tests",
                "command": "cargo test activity"
            })
            .to_string(),
        ));
        assert_eq!(
            activity_display_title(&command),
            "Ran command: Run focused tests"
        );
        command.complete = false;
        assert_eq!(
            activity_display_title(&command),
            "Running command: Run focused tests"
        );

        let web_search = ActivityItem::new(
            Some("search-1".into()),
            crate::model::ActivityKind::Search,
            "web_search",
            None,
            true,
        )
        .with_arguments(Some(serde_json::json!({"query": "Waku GPUI"}).to_string()));
        assert_eq!(
            activity_display_title(&web_search),
            "Searched the web for Waku GPUI"
        );

        let plan = ActivityItem::new(
            Some("plan-1".into()),
            crate::model::ActivityKind::Plan,
            "update_plan",
            None,
            false,
        );
        assert_eq!(activity_display_title(&plan), "Updating plan");
    }
}
