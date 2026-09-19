//! Settings → Friends: friend-code exchange, request cards, the friend
//! roster with lazy presence, and the live transfer list. All state comes
//! from the daemon's `friendsChanged` document; this file only renders it
//! and sends commands.

use super::settings::{SettingSearch, settings_search_text, settings_title_jump};
use super::*;
use waku_client::friends::{FriendInfo, TransferDirection, TransferInfo, TransferStatus};

impl Waku {
    fn friends_card(&self, theme: &Theme, children: impl IntoIterator<Item = AnyElement>) -> Div {
        div()
            .w_full()
            .px(px(20.0))
            .py(px(16.0))
            .rounded(px(16.0))
            .bg(theme.raised)
            .flex()
            .flex_col()
            .children(children)
    }

    fn friends_section_title(&self, theme: &Theme, label: String) -> Div {
        div()
            .text_size(sp(13.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text)
            .child(label)
    }

    fn friends_button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<SharedString>,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let on_click = std::rc::Rc::new(on_click);
        let on_key = on_click.clone();
        div()
            .id(id)
            .tab_index(0)
            .h(px(27.0))
            .px(px(9.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .focus_visible(|style| style.border_color(theme.accent))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(label.into())
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    on_key(this, cx);
                    cx.stop_propagation();
                }
            }))
    }

    pub(super) fn render_friends_settings(
        &self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let friends = &self.friends_state;

        // -- Your display name ----------------------------------------------
        let name_card = self.friends_card(
            &theme,
            [
                self.friends_section_title(&theme, tr!("friends.display_name")).into_any_element(),
                div()
                    .mt(px(5.0))
                    .text_size(sp(12.5))
                    .line_height(sp(18.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("friends.display_name_hint"))
                    .into_any_element(),
                div()
                    .mt(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        TextField::new("friend-name-field", self.friend_name_input.clone())
                            .w_full(),
                    )
                    .child(self.friends_button(
                        "save-friend-name",
                        tr!("common.save"),
                        &theme,
                        move |this, cx| this.save_friend_display_name(cx),
                        cx,
                    ))
                    .into_any_element(),
            ],
        );

        // -- Your friend code ------------------------------------------------
        let code = friends.friend_code.clone();
        let copy_feedback = "friend-code";
        let code_copied = self.control_was_copied(copy_feedback);
        let code_card = {
            let title = tr!("friends.your_code");
            let hint = tr!("friends.code_hint");
            search
                .matched(&title, &hint)
                .map(|matched| {
                    let mut children: Vec<AnyElement> = vec![
                        settings_title_jump(
                            div()
                                .text_size(sp(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(settings_search_text(
                                    title,
                                    matched.title_ranges.clone(),
                                    theme,
                                )),
                            &matched,
                            theme,
                        ),
                        div()
                            .mt(px(5.0))
                            .text_size(sp(12.5))
                            .line_height(sp(18.0))
                            .text_color(theme.text_secondary)
                            .child(settings_search_text(
                                hint,
                                matched.description_ranges.clone(),
                                theme,
                            ))
                            .into_any_element(),
                    ];
                    if !search.active() {
                        children.push(
                            div()
                                .mt(px(10.0))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_size(sp(12.0))
                                        .font_family(crate::fonts::current(cx).code)
                                        .text_color(theme.text)
                                        .child(code.clone()),
                                )
                                .child(self.friends_button(
                                    "copy-friend-code",
                                    if code_copied {
                                        tr!("common.copied")
                                    } else {
                                        tr!("common.copy")
                                    },
                                    &theme,
                                    move |this, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            code.clone(),
                                        ));
                                        this.show_control_copied(copy_feedback, cx);
                                    },
                                    cx,
                                ))
                                .into_any_element(),
                        );
                    }
                    self.friends_card(&theme, children)
                })
        };

        // -- Add friend ------------------------------------------------------
        let code_input = self.friend_code_input.read(cx).content().trim().to_string();
        let can_send = code_input.starts_with("gfr-");
        let add_card =
            {
                let title = tr!("friends.add_friend");
                let hint = tr!("friends.add_hint");
                search
                    .matched(&title, &hint)
                    .map(|matched| {
                        let mut children: Vec<AnyElement> = vec![
                            settings_title_jump(
                                div()
                                    .text_size(sp(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(settings_search_text(
                                        title,
                                        matched.title_ranges.clone(),
                                        theme,
                                    )),
                                &matched,
                                theme,
                            ),
                            div()
                                .mt(px(5.0))
                                .text_size(sp(12.5))
                                .line_height(sp(18.0))
                                .text_color(theme.text_secondary)
                                .child(settings_search_text(
                                    hint,
                                    matched.description_ranges.clone(),
                                    theme,
                                ))
                                .into_any_element(),
                        ];
                        if !search.active() {
                            children.push(
                                div()
                                    .mt(px(10.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        TextField::new(
                                            "friend-code-field",
                                            self.friend_code_input.clone(),
                                        )
                                        .w_full(),
                                    )
                                    .child(
                                        self.friends_button(
                                            "send-friend-request",
                                            tr!("friends.send_request"),
                                            &theme,
                                            move |this, cx| this.send_friend_request(cx),
                                            cx,
                                        )
                                        .opacity(if can_send { 1.0 } else { 0.55 }),
                                    )
                                    .into_any_element(),
                            );
                        }
                        self.friends_card(&theme, children)
                    })
            };

        // -- Pending requests -------------------------------------------------
        let mut request_cards = Vec::new();
        for request in &friends.incoming_requests {
            let name = request.name.clone();
            let node_id = request.node_id.clone();
            let short = short_node_id(&request.node_id);
            let accept_id = node_id.clone();
            let decline_id = node_id.clone();
            let Some(matched) = search.matched(&format!("{name} · {short}"), "") else {
                continue;
            };
            request_cards.push(
                div()
                    .mt(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(settings_title_jump(
                                div().text_size(sp(13.0)).text_color(theme.text).child(
                                    settings_search_text(
                                        format!("{name} · {short}"),
                                        matched.title_ranges.clone(),
                                        theme,
                                    ),
                                ),
                                &matched,
                                theme,
                            ))
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(tr!("friends.wants_to_add")),
                            ),
                    )
                    .child(self.friends_button(
                        SharedString::from(format!("friend-accept-{node_id}")),
                        tr!("friends.accept"),
                        &theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::RespondFriendRequest {
                                    node_id: accept_id.clone(),
                                    accept: true,
                                },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("friend-decline-{node_id}")),
                        tr!("friends.decline"),
                        &theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::RespondFriendRequest {
                                    node_id: decline_id.clone(),
                                    accept: false,
                                },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        for request in &friends.outgoing_requests {
            let withdraw_id = request.node_id.clone();
            if search
                .matched(
                    &format!("{} · {}", request.name, short_node_id(&request.node_id)),
                    "",
                )
                .is_none()
            {
                continue;
            }
            request_cards.push(
                div()
                    .mt(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div().flex_1().min_w_0().child(
                            div()
                                .text_size(sp(13.0))
                                .text_color(theme.text_secondary)
                                .child(format!(
                                    "{} · {} — {}",
                                    request.name,
                                    short_node_id(&request.node_id),
                                    tr!("friends.pending")
                                )),
                        ),
                    )
                    .child(self.friends_button(
                        SharedString::from(format!("friend-withdraw-{}", request.node_id)),
                        tr!("friends.withdraw"),
                        &theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::WithdrawFriendRequest {
                                    node_id: withdraw_id.clone(),
                                },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .into_any_element(),
            );
        }

        // -- Friends list ------------------------------------------------------
        let mut friend_rows = Vec::new();
        for friend in &friends.friends {
            let node_id = friend.node_id.clone();
            let display = friend_display_name(friend).to_string();
            let (dot_color, status) = if friend.online {
                (theme.success, tr!("friends.online"))
            } else {
                (theme.text_tertiary, tr!("friends.unreachable"))
            };
            let mut meta = status.to_string();
            if friend.nickname.is_some() {
                meta = format!("{} · {meta}", friend.name);
            }
            if let Some(last_seen_ms) = friend.last_seen_ms {
                let ago = format_time_ago(unix_time_millis().saturating_sub(last_seen_ms) / 1_000);
                meta = format!("{meta} · {}", tr!("friends.last_seen", ago = ago));
            }
            let editing = self.editing_friend_nickname.as_deref() == Some(node_id.as_str());
            let send_id = node_id.clone();
            let remove_id = node_id.clone();
            // Nicknames are searchable too — the row renders the resolved
            // name, so match on it rather than the self-reported one.
            let Some(matched) = search.matched(&display, "") else {
                continue;
            };
            let edit_id = node_id.clone();
            let mut row = div()
                .id(SharedString::from(format!("friend-row-{node_id}")))
                .mt(px(10.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .size(px(8.0))
                        .rounded_full()
                        .flex_none()
                        .bg(dot_color),
                );
            if editing {
                row = row
                    .child(
                        div().flex_1().min_w_0().child(
                            TextField::new(
                                "friend-nickname-field",
                                self.friend_nickname_input.clone(),
                            )
                            .w_full(),
                        ),
                    )
                    .child(self.friends_button(
                        SharedString::from(format!("friend-nickname-save-{node_id}")),
                        tr!("common.save"),
                        &theme,
                        move |this, cx| this.commit_friend_nickname(cx),
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("friend-nickname-cancel-{node_id}")),
                        tr!("common.cancel"),
                        &theme,
                        move |this, cx| {
                            this.cancel_friend_nickname_edit(cx);
                        },
                        cx,
                    ));
            } else {
                row = row
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(settings_title_jump(
                                div().text_size(sp(13.0)).text_color(theme.text).child(
                                    settings_search_text(
                                        display.clone(),
                                        matched.title_ranges.clone(),
                                        theme,
                                    ),
                                ),
                                &matched,
                                theme,
                            ))
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(meta),
                            ),
                    )
                    .child(self.friends_button(
                        SharedString::from(format!("friend-nickname-{node_id}")),
                        tr!("friends.nickname"),
                        &theme,
                        move |this, cx| this.begin_friend_nickname_edit(edit_id.clone(), cx),
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("friend-send-{node_id}")),
                        tr!("friends.send_file"),
                        &theme,
                        move |this, cx| this.pick_and_send_file(send_id.clone(), cx),
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("friend-remove-{node_id}")),
                        tr!("friends.remove"),
                        &theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::RemoveFriend {
                                    node_id: remove_id.clone(),
                                },
                                cx,
                            );
                        },
                        cx,
                    ));
            }
            friend_rows.push(row.into_any_element());
        }
        if friend_rows.is_empty() && !search.active() {
            friend_rows.push(
                div()
                    .mt(px(10.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("friends.empty"))
                    .into_any_element(),
            );
        }
        // The roster card stays when its title matched or a member did.
        let friends_title_matched = search.matched(&tr!("friends.list"), "").is_some();
        let friends_card = (friends_title_matched || !search.active() || !friend_rows.is_empty())
            .then(|| {
                self.friends_card(
                    &theme,
                    std::iter::once(
                        self.friends_section_title(&theme, tr!("friends.list"))
                            .into_any_element(),
                    )
                    .chain(friend_rows)
                    .collect::<Vec<_>>(),
                )
            });

        // -- Transfers ----------------------------------------------------------
        let transfer_cards: Vec<AnyElement> = friends
            .transfers
            .iter()
            .filter(|transfer| {
                search
                    .matched(&format!("{} · {}", transfer.title, transfer.peer_name), "")
                    .is_some()
            })
            .map(|transfer| self.render_transfer_row(transfer, &theme, cx))
            .collect();
        let transfers_title_matched = search.matched(&tr!("friends.transfers"), "").is_some();
        let transfers_card = (!transfer_cards.is_empty() || transfers_title_matched).then(|| {
            self.friends_card(
                &theme,
                std::iter::once(
                    self.friends_section_title(&theme, tr!("friends.transfers"))
                        .into_any_element(),
                )
                .chain(transfer_cards)
                .collect::<Vec<_>>(),
            )
        });

        let requests_title_matched = search.matched(&tr!("friends.requests"), "").is_some();
        let mut column = div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(name_card)
            .children(code_card)
            .children(add_card)
            .children(friends_card);
        if !request_cards.is_empty() || requests_title_matched {
            column = column.child(
                self.friends_card(
                    &theme,
                    std::iter::once(
                        self.friends_section_title(&theme, tr!("friends.requests"))
                            .into_any_element(),
                    )
                    .chain(request_cards)
                    .collect::<Vec<_>>(),
                ),
            );
        }
        if let Some(transfers_card) = transfers_card {
            column = column.child(transfers_card);
        }
        column.into_any_element()
    }

    fn render_transfer_row(
        &self,
        transfer: &TransferInfo,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let direction_icon = match transfer.direction {
            TransferDirection::Outgoing => "icons/arrow-up.svg",
            TransferDirection::Incoming => "icons/arrow-down.svg",
        };
        let status = match transfer.status {
            TransferStatus::Pending => tr!("friends.status_pending"),
            TransferStatus::Transferring => {
                if transfer.bytes_total > 0 {
                    format!(
                        "{} / {}",
                        format_bytes(transfer.bytes_done),
                        format_bytes(transfer.bytes_total)
                    )
                    .into()
                } else {
                    tr!("friends.status_transferring")
                }
            }
            TransferStatus::Done => tr!("friends.status_done"),
            TransferStatus::Failed => tr!("friends.status_failed"),
            TransferStatus::Cancelled => tr!("friends.status_cancelled"),
        };
        let cancellable = matches!(
            transfer.status,
            TransferStatus::Pending | TransferStatus::Transferring
        );
        let peer_name = self
            .friends_state
            .friends
            .iter()
            .find(|f| f.node_id == transfer.peer_id)
            .map(|f| friend_display_name(f).to_string())
            .unwrap_or_else(|| transfer.peer_name.clone());
        let transfer_id = transfer.id;
        let session_id = transfer.session_id;
        let reveal_dir = (transfer.direction == TransferDirection::Incoming
            && transfer.status == TransferStatus::Done)
            .then(|| transfer.dest_dir.clone())
            .flatten();
        div()
            .mt(px(10.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(icon(direction_icon, 13.0, theme.text_tertiary))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(sp(13.0))
                            .text_color(theme.text)
                            .child(format!("{} · {}", transfer.title, peer_name)),
                    )
                    .child(
                        div()
                            .text_size(sp(11.5))
                            .text_color(theme.text_tertiary)
                            .child(status),
                    ),
            )
            .when_some(session_id, |element, session_id| {
                element.child(self.friends_button(
                    SharedString::from(format!("transfer-open-{transfer_id}")),
                    tr!("friends.open_chat"),
                    theme,
                    move |this, cx| {
                        this.settings_page = None;
                        this.select_session(session_id, cx);
                    },
                    cx,
                ))
            })
            .when_some(reveal_dir, |element, dir| {
                element.child(self.friends_button(
                    SharedString::from(format!("transfer-reveal-{transfer_id}")),
                    tr!("friends.show_in_finder"),
                    theme,
                    move |_, cx| crate::platform::reveal_in_file_manager(&dir, cx),
                    cx,
                ))
            })
            .when(cancellable, |element| {
                element.child(self.friends_button(
                    SharedString::from(format!("transfer-cancel-{transfer_id}")),
                    tr!("common.cancel"),
                    theme,
                    move |this, cx| {
                        this.friends_command(
                            waku_client::Command::CancelTransfer { transfer_id },
                            cx,
                        );
                    },
                    cx,
                ))
            })
            .into_any_element()
    }

    /// "Send request" for the add-friend field.
    pub(super) fn send_friend_request(&self, cx: &mut Context<Self>) {
        let code = self.friend_code_input.read(cx).content().trim().to_string();
        if !code.starts_with("gfr-") {
            return;
        }
        let our_name = self
            .friends_state
            .display_name
            .trim()
            .to_string();
        let our_name = if our_name.is_empty() {
            "Goddard".to_owned()
        } else {
            our_name
        };
        self.friend_code_input.update(cx, |input, cx| {
            input.set_content("", cx);
        });
        self.friends_command(
            waku_client::Command::SendFriendRequest {
                code,
                name: our_name,
            },
            cx,
        );
    }

    /// Persist the display-name field (Save button or Enter).
    pub(super) fn save_friend_display_name(&mut self, cx: &mut Context<Self>) {
        let name = self
            .friend_name_input
            .read(cx)
            .content()
            .trim()
            .to_string();
        self.friends_command(waku_client::Command::SetFriendDisplayName { name }, cx);
    }

    /// Swap the friend's name for the shared nickname editor, prefilled
    /// with their current nickname (empty when they have none).
    fn begin_friend_nickname_edit(&mut self, node_id: String, cx: &mut Context<Self>) {
        let current = self
            .friends_state
            .friends
            .iter()
            .find(|f| f.node_id == node_id)
            .and_then(|f| f.nickname.clone())
            .unwrap_or_default();
        self.editing_friend_nickname = Some(node_id);
        self.friend_nickname_input.update(cx, |input, cx| {
            input.set_content(current, cx);
        });
        let focus = self.friend_nickname_input.read(cx).focus();
        let _ = self
            .window_handle
            .update(cx, |_, window, cx| window.focus(&focus, cx));
        cx.notify();
    }

    /// Commit the shared nickname editor to the daemon (Save or Enter).
    /// Blank clears the override.
    pub(super) fn commit_friend_nickname(&mut self, cx: &mut Context<Self>) {
        let Some(node_id) = self.editing_friend_nickname.take() else {
            return;
        };
        let nickname = self
            .friend_nickname_input
            .read(cx)
            .content()
            .trim()
            .to_string();
        self.friends_command(
            waku_client::Command::SetFriendNickname {
                node_id,
                nickname: (!nickname.is_empty()).then_some(nickname),
            },
            cx,
        );
        cx.notify();
    }

    fn cancel_friend_nickname_edit(&mut self, cx: &mut Context<Self>) {
        self.editing_friend_nickname = None;
        cx.notify();
    }

    /// File picker → the send dialog's optional note → `SendFileToFriend`.
    /// The daemon dials fresh regardless of the cached probe verdict.
    fn pick_and_send_file(&self, node_id: String, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(tr!("friends.send_file_prompt").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    let peer_name = this
                        .friends_state
                        .friends
                        .iter()
                        .find(|friend| friend.node_id == node_id)
                        .map(|friend| friend_display_name(friend).to_owned())
                        .unwrap_or_default();
                    this.open_send_file_dialog(node_id, peer_name, path, window, cx);
                });
            }
        })
        .detach();
    }
}

/// What this install shows for a friend: local nickname, else their
/// self-reported name.
fn friend_display_name(friend: &FriendInfo) -> &str {
    friend
        .nickname
        .as_deref()
        .filter(|n| !n.is_empty())
        .unwrap_or(&friend.name)
}

fn short_node_id(node_id: &str) -> String {
    let chars: String = node_id.chars().take(8).collect();
    format!("{chars}…")
}

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
