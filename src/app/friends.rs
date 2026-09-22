//! Settings → Friends: friend-code exchange, request cards, the friend
//! roster with lazy presence, and the live transfer list. All state comes
//! from the daemon's `friendsChanged` document; this file only renders it
//! and sends commands.

use super::settings::{SettingSearch, settings_search_text, settings_title_jump};
use super::*;
use waku_client::friends::{
    FriendInfo, FriendSyncAlertAction, IncomingShareInfo, SharedSessionSummary, SyncAlertInfo,
    SyncAlertKind, SyncLinkInfo, TransferDirection, TransferInfo, TransferStatus,
};

/// A session we're watching on a friend's shared project — the friend
/// owns the runtime; our driver handle is a read-only no-op.
pub(super) struct FriendWatch {
    pub peer_name: String,
    /// `Some` once the stream ended — `true` the friend revoked, `false`
    /// the connection dropped or the friend went offline.
    pub closed: Option<bool>,
}

/// A `GetFriendSessions` answer for one incoming share — empty `Ready`
/// means the project simply has no sessions yet.
pub(super) enum FriendSessionList {
    Loading,
    Ready(Vec<SharedSessionSummary>),
    Error(String),
}

/// `friend_session_lists` key — one entry per (peer, shared origin).
pub(super) fn friend_session_list_key(node_id: &str, origin_url: &str) -> String {
    format!("{node_id}|{origin_url}")
}

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
            .focus_visible(|style| style.bg(theme.focus_highlight()))
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

    /// Send `command` only after a native confirmation names what it
    /// destroys. The button rows hand over no window, so the prompt rides
    /// the window handle — a declined answer or a closed window sends
    /// nothing.
    fn friends_confirm_command(
        &mut self,
        message: String,
        detail: Option<String>,
        confirm: String,
        command: waku_client::Command,
        cx: &mut Context<Self>,
    ) {
        let Ok(answer) = self.window_handle.update(cx, |_, window, cx| {
            window.prompt(
                gpui::PromptLevel::Warning,
                &message,
                detail.as_deref(),
                &[
                    gpui::PromptButton::cancel(tr!("common.cancel")),
                    gpui::PromptButton::ok(confirm),
                ],
                cx,
            )
        }) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| this.friends_command(command, cx));
        })
        .detach();
    }

    pub(super) fn render_friends_settings(
        &mut self,
        search: &SettingSearch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        // The document is cloned once so the sharing panel's lazy branch
        // fetch and `&mut self` helpers can run mid-render.
        let friends = self.friends_state.clone();

        // -- Your display name ----------------------------------------------
        let name_card = self.friends_card(
            &theme,
            [
                self.friends_section_title(&theme, tr!("friends.display_name"))
                    .into_any_element(),
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
            search.matched(&title, &hint).map(|matched| {
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
                                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
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
        let add_card = {
            let title = tr!("friends.add_friend");
            let hint = tr!("friends.add_hint");
            search.matched(&title, &hint).map(|matched| {
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
                                TextField::new("friend-code-field", self.friend_code_input.clone())
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
                                .opacity(if can_send {
                                    1.0
                                } else {
                                    0.55
                                }),
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
            let share_id = node_id.clone();
            let remove_id = node_id.clone();
            // Nicknames are searchable too — the row renders the resolved
            // name, so match on it rather than the self-reported one.
            let Some(matched) = search.matched(&display, "") else {
                continue;
            };
            let edit_id = node_id.clone();
            let remove_name = display.clone();
            let mut row = div()
                .id(SharedString::from(format!("friend-row-{node_id}")))
                .mt(px(10.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(div().size(px(8.0)).rounded_full().flex_none().bg(dot_color));
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
                        SharedString::from(format!("friend-share-{node_id}")),
                        tr!("friends.sharing"),
                        &theme,
                        move |this, cx| this.toggle_friend_share_panel(share_id.clone(), cx),
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
                            this.friends_confirm_command(
                                tr!("friends.confirm_remove", name = remove_name.clone()),
                                Some(tr!("friends.confirm_remove_detail")),
                                tr!("common.remove"),
                                waku_client::Command::RemoveFriend {
                                    node_id: remove_id.clone(),
                                },
                                cx,
                            );
                        },
                        cx,
                    ));
            }
            if self.expanded_share_friend.as_deref() == Some(node_id.as_str()) {
                friend_rows.push(
                    div()
                        .flex()
                        .flex_col()
                        .child(row)
                        .child(self.render_friend_share_panel(&node_id, &theme, cx))
                        .into_any_element(),
                );
            } else {
                friend_rows.push(row.into_any_element());
            }
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

        // -- Pair requests + paired devices -----------------------------------
        // A device discovered this daemon over the LAN and asked for a
        // token; approving mints one just for it, so revoking later drops
        // that device without disturbing the master credential.
        let pairing = &self.pairing_state;
        let mut pair_rows = Vec::new();
        for request in &pairing.pending {
            let device_name = request.device_name.clone();
            let transport = request.transport.clone();
            let request_id = request.request_id;
            let approve_id = request_id;
            let decline_id = request_id;
            if search.matched(&device_name, "").is_none() {
                continue;
            }
            pair_rows.push(
                div()
                    .mt(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(sp(13.0))
                                    .text_color(theme.text)
                                    .child(format!("{device_name} · {transport}")),
                            )
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(tr!("pairing.wants_to_pair")),
                            ),
                    )
                    .child(self.friends_button(
                        SharedString::from(format!("pair-approve-{approve_id}")),
                        tr!("pairing.approve"),
                        &theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::RespondPairRequest {
                                    request_id: approve_id,
                                    accept: true,
                                },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("pair-decline-{decline_id}")),
                        tr!("pairing.decline"),
                        &theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::RespondPairRequest {
                                    request_id: decline_id,
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
        let pairing_title_matched = search.matched(&tr!("pairing.requests"), "").is_some();
        let pairing_card = (!pair_rows.is_empty() || pairing_title_matched).then(|| {
            self.friends_card(
                &theme,
                std::iter::once(
                    self.friends_section_title(&theme, tr!("pairing.requests"))
                        .into_any_element(),
                )
                .chain(pair_rows)
                .collect::<Vec<_>>(),
            )
        });

        let mut paired_rows = Vec::new();
        for client in &pairing.clients {
            let client_id = client.client_id;
            let name = client.name.clone();
            let revoke_name = name.clone();
            let ago =
                format_time_ago(unix_time_millis().saturating_sub(client.added_at_ms) / 1_000);
            if search.matched(&name, "").is_none() {
                continue;
            }
            paired_rows.push(
                div()
                    .mt(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_size(sp(13.0)).text_color(theme.text).child(name))
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(tr!("pairing.added", ago = ago)),
                            ),
                    )
                    .child(self.friends_button(
                        SharedString::from(format!("pair-revoke-{client_id}")),
                        tr!("pairing.revoke"),
                        &theme,
                        move |this, cx| {
                            this.friends_confirm_command(
                                tr!("pairing.confirm_revoke", name = revoke_name.clone()),
                                Some(tr!("pairing.confirm_revoke_detail")),
                                tr!("common.revoke"),
                                waku_client::Command::RevokePairedClient { client_id },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        if paired_rows.is_empty() && !search.active() {
            paired_rows.push(
                div()
                    .mt(px(10.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("pairing.none"))
                    .into_any_element(),
            );
        }
        let paired_title_matched = search.matched(&tr!("pairing.paired"), "").is_some();
        let paired_card = (paired_title_matched || !search.active() || !paired_rows.is_empty())
            .then(|| {
                self.friends_card(
                    &theme,
                    std::iter::once(
                        self.friends_section_title(&theme, tr!("pairing.paired"))
                            .into_any_element(),
                    )
                    .chain(paired_rows)
                    .collect::<Vec<_>>(),
                )
            });

        let requests_title_matched = search.matched(&tr!("friends.requests"), "").is_some();
        // Sync alerts lead the page — a stopped rebase blocks sync until
        // someone picks a decision.
        let sync_alerts_card = (!friends.sync_alerts.is_empty()
            || search.matched(&tr!("friends.sync_alerts"), "").is_some())
        .then(|| {
            self.friends_card(
                &theme,
                std::iter::once(
                    self.friends_section_title(&theme, tr!("friends.sync_alerts"))
                        .into_any_element(),
                )
                .chain(
                    friends
                        .sync_alerts
                        .iter()
                        .filter(|alert| {
                            search
                                .matched(&format!("{} · {}", alert.peer_name, alert.branch), "")
                                .is_some()
                        })
                        .map(|alert| self.render_sync_alert_row(alert, &theme, cx)),
                )
                .collect::<Vec<_>>(),
            )
        });
        let mut column = div()
            .mt(px(15.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .children(sync_alerts_card)
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
        if let Some(pairing_card) = pairing_card {
            column = column.child(pairing_card);
        }
        if let Some(paired_card) = paired_card {
            column = column.child(paired_card);
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
        let our_name = self.friends_state.display_name.trim().to_string();
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
        let name = self.friend_name_input.read(cx).content().trim().to_string();
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

    /// Forward a transcript message to a friend — confirm first, then the
    /// daemon dials; failures arrive through the friends toast.
    pub(super) fn confirm_send_chat_to_friend(
        &mut self,
        node_id: String,
        peer_name: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        self.friends_confirm_command(
            tr!("friends.confirm_send_chat", name = peer_name),
            None,
            tr!("common.send"),
            waku_client::Command::SendMessageToFriend { node_id, text },
            cx,
        );
    }

    /// Expand/collapse the per-friend sharing panel — project toggles,
    /// their shares of matching repos, and the sync links between you.
    fn toggle_friend_share_panel(&mut self, node_id: String, cx: &mut Context<Self>) {
        if self.expanded_share_friend.as_deref() == Some(node_id.as_str()) {
            self.expanded_share_friend = None;
        } else {
            self.expanded_share_friend = Some(node_id.clone());
            // Warm the branch lists this panel renders.
            let link_ids: Vec<String> = self
                .friends_state
                .sync_links
                .iter()
                .filter(|link| link.peer_id == node_id)
                .map(|link| link.id.clone())
                .collect();
            for link_id in link_ids {
                self.ensure_sync_link_branches(&link_id, cx);
            }
        }
        cx.notify();
    }

    /// Lazily fetch a link's local branches for its config panel; the
    /// result lands in `sync_link_branches` and repaints the page.
    fn ensure_sync_link_branches(&mut self, link_id: &str, cx: &mut Context<Self>) {
        if self.sync_link_branches.contains_key(link_id)
            || !self.sync_branch_fetch_pending.insert(link_id.to_owned())
        {
            return;
        }
        let client = self.daemon.client();
        let link_id = link_id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let link_id = link_id.clone();
                    async move {
                        match client.request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::GetFriendSyncBranches { link_id },
                        ) {
                            Ok(waku_client::ResponsePayload::FriendSyncBranches {
                                branches,
                                default_branch,
                                ..
                            }) => Some((branches, default_branch)),
                            _ => None,
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.sync_branch_fetch_pending.remove(&link_id);
                if let Some(fetched) = result {
                    this.sync_link_branches.insert(link_id.clone(), fetched);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The expanded sharing panel under a friend row: which of your
    /// projects they can see, what they've shared with you, and the sync
    /// links' branch/auto-push configuration.
    fn render_friend_share_panel(
        &mut self,
        node_id: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let friends = self.friends_state.clone();
        let mut children: Vec<AnyElement> = Vec::new();

        // -- Projects we share with them ---------------------------------
        children.push(
            div()
                .mt(px(12.0))
                .text_size(sp(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary)
                .child(tr!("friends.shared_projects"))
                .into_any_element(),
        );
        let mut project_rows = 0;
        for project in &self.state.projects {
            if project.temporary {
                continue;
            }
            project_rows += 1;
            let shared = friends
                .shared_projects
                .iter()
                .find(|share| share.peer_id == node_id && share.repo_path == project.path);
            let project_path = project.path.clone();
            let origin_url = shared.map(|share| share.origin_url.clone());
            let unshare_message = tr!("friends.confirm_unshare", name = project.name.clone());
            let peer = node_id.to_owned();
            children.push(
                div()
                    .mt(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(sp(12.5))
                            .text_color(theme.text)
                            .child(project.name.clone()),
                    )
                    .child(toggle_switch(
                        SharedString::from(format!("share-{}-{}", node_id, project.id)),
                        shared.is_some(),
                        false,
                        *theme,
                        cx,
                        move |this, _window, cx| {
                            if let Some(origin) = origin_url.clone() {
                                this.friends_confirm_command(
                                    unshare_message.clone(),
                                    Some(tr!("friends.confirm_unshare_detail")),
                                    tr!("friends.unshare"),
                                    waku_client::Command::UnshareProjectWithFriend {
                                        node_id: peer.clone(),
                                        origin_url: origin,
                                    },
                                    cx,
                                );
                            } else {
                                this.friends_command(
                                    waku_client::Command::ShareProjectWithFriend {
                                        node_id: peer.clone(),
                                        project_path: project_path.clone(),
                                    },
                                    cx,
                                );
                            }
                        },
                    ))
                    .into_any_element(),
            );
            // Opt-in second layer: the friend can peek at this project's
            // sessions, live and read-only, independent of repo sync.
            if let Some(share) = shared {
                let peer = node_id.to_owned();
                let origin = share.origin_url.clone();
                let next = !share.share_sessions;
                children.push(
                    div()
                        .mt(px(2.0))
                        .ml(px(12.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(sp(11.5))
                                .text_color(theme.text_secondary)
                                .child(tr!("friends.share_sessions")),
                        )
                        .child(toggle_switch(
                            SharedString::from(format!(
                                "share-sessions-{}-{}",
                                node_id, project.id
                            )),
                            share.share_sessions,
                            false,
                            *theme,
                            cx,
                            move |this, _window, cx| {
                                this.friends_command(
                                    waku_client::Command::SetFriendSessionSharing {
                                        node_id: peer.clone(),
                                        origin_url: origin.clone(),
                                        enabled: next,
                                    },
                                    cx,
                                );
                            },
                        ))
                        .into_any_element(),
                );
            }
        }
        if project_rows == 0 {
            children.push(
                div()
                    .mt(px(6.0))
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("friends.no_projects_hint"))
                    .into_any_element(),
            );
        }

        // -- What they've shared with us ---------------------------------
        let incoming: Vec<&IncomingShareInfo> = friends
            .incoming_shares
            .iter()
            .filter(|share| share.peer_id == node_id)
            .collect();
        if !incoming.is_empty() {
            children.push(
                div()
                    .mt(px(14.0))
                    .text_size(sp(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(tr!("friends.shared_with_you"))
                    .into_any_element(),
            );
            for share in incoming {
                let mut row = div().mt(px(6.0)).flex().items_center().gap(px(8.0)).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .text_color(theme.text)
                                .child(share.project_name.clone()),
                        )
                        .child(
                            div()
                                .text_size(sp(11.0))
                                .text_color(theme.text_tertiary)
                                .child(match &share.matched_project_name {
                                    Some(name) => {
                                        tr!("friends.matches_project", name = name.clone())
                                            .to_string()
                                    }
                                    None => tr!("friends.no_local_match").to_string(),
                                }),
                        ),
                );
                if share.matched_path.is_some() && !share.sync_enabled {
                    let peer = node_id.to_owned();
                    let origin = share.origin_url.clone();
                    row = row.child(self.friends_button(
                        SharedString::from(format!("sync-enable-{}-{}", node_id, share.origin_url)),
                        tr!("friends.enable_sync"),
                        theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::EnableFriendSync {
                                    node_id: peer.clone(),
                                    origin_url: origin.clone(),
                                },
                                cx,
                            );
                        },
                        cx,
                    ));
                }
                if share.share_sessions {
                    let key = friend_session_list_key(node_id, &share.origin_url);
                    let fetched = self.friend_session_lists.contains_key(&key);
                    if !fetched {
                        let peer = node_id.to_owned();
                        let origin = share.origin_url.clone();
                        row = row.child(self.friends_button(
                            SharedString::from(format!("sessions-{key}")),
                            tr!("friends.sessions"),
                            theme,
                            move |this, cx| {
                                this.fetch_friend_sessions(peer.clone(), origin.clone(), cx)
                            },
                            cx,
                        ));
                    }
                }
                children.push(row.into_any_element());
                if share.share_sessions {
                    children.push(self.render_friend_session_list(node_id, share, theme, cx));
                }
            }
        }

        // -- Sync links ---------------------------------------------------
        for link in friends
            .sync_links
            .iter()
            .filter(|link| link.peer_id == node_id)
        {
            children.push(self.render_sync_link(link, theme, cx));
        }

        div()
            .flex()
            .flex_col()
            .pl(px(16.0))
            .pr(px(4.0))
            .pb(px(4.0))
            .children(children)
            .into_any_element()
    }

    /// The strip above a watched friend session's transcript: who it
    /// belongs to, that it's read-only, and the stream's closed state.
    pub(super) fn friend_watch_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let watch = self.selected_friend_watch()?;
        let session_id = self.state.selected_session?;
        let theme = Theme::current(cx);
        let text = match watch.closed {
            Some(true) => tr!("friends.watch_revoked", name = watch.peer_name.clone()).to_string(),
            Some(false) => tr!("friends.watch_ended").to_string(),
            None => tr!("friends.watching_session", name = watch.peer_name.clone()).to_string(),
        };
        Some(
            div()
                .flex_none()
                .px(px(12.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .bg(theme.inset)
                .border_b(hairline())
                .border_color(theme.border)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(sp(11.5))
                        .text_color(theme.text_secondary)
                        .child(text),
                )
                .child(
                    div()
                        .id("friend-watch-stop")
                        .tab_index(0)
                        .px(px(8.0))
                        .h(px(20.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .cursor_default()
                        .text_size(sp(11.5))
                        .text_color(theme.text_tertiary)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .hover(|element| element.bg(theme.overlay).text_color(theme.text))
                        .child(tr!("friends.stop_watching"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stop_watching_friend_session(session_id, cx);
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                            if !event.keystroke.modifiers.modified()
                                && matches!(event.keystroke.key.as_str(), "enter" | "space")
                            {
                                this.stop_watching_friend_session(session_id, cx);
                                cx.stop_propagation();
                            }
                        })),
                )
                .into_any_element(),
        )
    }

    /// The fetched session list under an incoming share — each row opens
    /// the session as a read-only live view in the main area.
    fn render_friend_session_list(
        &mut self,
        node_id: &str,
        share: &IncomingShareInfo,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = friend_session_list_key(node_id, &share.origin_url);
        let Some(list) = self.friend_session_lists.get(&key) else {
            return div().into_any_element();
        };
        let mut children: Vec<AnyElement> = Vec::new();
        match list {
            FriendSessionList::Loading => children.push(
                div()
                    .mt(px(4.0))
                    .ml(px(12.0))
                    .text_size(sp(11.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("friends.sessions_loading"))
                    .into_any_element(),
            ),
            FriendSessionList::Error(error) => children.push(
                div()
                    .mt(px(4.0))
                    .ml(px(12.0))
                    .text_size(sp(11.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("friends.sessions_error", error = error.clone()))
                    .into_any_element(),
            ),
            FriendSessionList::Ready(sessions) => {
                if sessions.is_empty() {
                    children.push(
                        div()
                            .mt(px(4.0))
                            .ml(px(12.0))
                            .text_size(sp(11.5))
                            .text_color(theme.text_tertiary)
                            .child(tr!("friends.sessions_empty"))
                            .into_any_element(),
                    );
                }
                for session in sessions {
                    let watched = self.friend_sessions.contains_key(&session.session_id);
                    let session_id = session.session_id;
                    let title = if session.title.is_empty() {
                        session
                            .auto_title
                            .clone()
                            .unwrap_or_else(|| tr!("friends.untitled_session").to_string())
                    } else {
                        session.title.clone()
                    };
                    children.push(
                        div()
                            .id(SharedString::from(format!("friend-session-{session_id}")))
                            .tab_index(0)
                            .mt(px(2.0))
                            .ml(px(12.0))
                            .px(px(6.0))
                            .py(px(3.0))
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .cursor_default()
                            .focus_visible(|style| style.bg(theme.focus_highlight()))
                            .hover(|element| element.bg(theme.overlay))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(sp(12.0))
                                    .text_color(theme.text_secondary)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(sp(10.5))
                                    .text_color(theme.text_tertiary)
                                    .child(if watched {
                                        tr!("friends.watching").to_string()
                                    } else if session.status.is_busy() {
                                        tr!("friends.session_running").to_string()
                                    } else {
                                        tr!("friends.watch").to_string()
                                    }),
                            )
                            .on_click(cx.listener({
                                let peer = node_id.to_owned();
                                let origin = share.origin_url.clone();
                                move |this, _, _, cx| {
                                    this.open_friend_session(
                                        peer.clone(),
                                        origin.clone(),
                                        session_id,
                                        cx,
                                    );
                                }
                            }))
                            .on_key_down(cx.listener({
                                let peer = node_id.to_owned();
                                let origin = share.origin_url.clone();
                                move |this, event: &KeyDownEvent, _, cx| {
                                    if !event.keystroke.modifiers.modified()
                                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                    {
                                        this.open_friend_session(
                                            peer.clone(),
                                            origin.clone(),
                                            session_id,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }
                                }
                            }))
                            .into_any_element(),
                    );
                }
            }
        }
        div().children(children).into_any_element()
    }

    /// One sync link's config block inside the sharing panel: auto-push
    /// toggle, per-branch sync toggles, paused state, and teardown.
    fn render_sync_link(
        &mut self,
        link: &SyncLinkInfo,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_sync_link_branches(&link.id, cx);
        let link_id = link.id.clone();
        let repo_name = link
            .repo_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| link.repo_path.display().to_string());
        let auto_push = link.auto_push;
        let enabled: Vec<String> = link.enabled_branches.clone();
        let paused = link.paused_branches.clone();
        let peer_enabled = link.peer_sync_enabled;
        let enabled_set: std::collections::BTreeSet<String> = enabled.iter().cloned().collect();
        let (branches, default_branch) = self
            .sync_link_branches
            .get(&link.id)
            .cloned()
            .unwrap_or_else(|| (enabled.clone(), None));

        let mut children: Vec<AnyElement> = Vec::new();
        children.push(
            div()
                .mt(px(14.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(sp(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(tr!(
                            "friends.sync_link_title",
                            name = repo_name.clone(),
                            peer = link.peer_name.clone()
                        )),
                )
                .when(!peer_enabled, |element| {
                    element.child(
                        div()
                            .text_size(sp(11.0))
                            .text_color(theme.warning)
                            .child(tr!("friends.sync_waiting")),
                    )
                })
                .into_any_element(),
        );

        // Auto-push toggle.
        let cfg_link = link_id.clone();
        let cfg_branches = enabled.clone();
        children.push(
            div()
                .mt(px(8.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(sp(12.5))
                                .text_color(theme.text)
                                .child(tr!("friends.auto_push")),
                        )
                        .child(
                            div()
                                .text_size(sp(11.0))
                                .text_color(theme.text_tertiary)
                                .child(tr!("friends.auto_push_hint")),
                        ),
                )
                .child(toggle_switch(
                    SharedString::from(format!("sync-autopush-{link_id}")),
                    auto_push,
                    false,
                    *theme,
                    cx,
                    move |this, _window, cx| {
                        this.friends_command(
                            waku_client::Command::SetFriendSyncConfig {
                                link_id: cfg_link.clone(),
                                auto_push: !auto_push,
                                enabled_branches: cfg_branches.clone(),
                            },
                            cx,
                        );
                    },
                ))
                .into_any_element(),
        );

        // Branch toggles — paused branches show a Sync now action.
        children.push(
            div()
                .mt(px(10.0))
                .text_size(sp(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary)
                .child(tr!("friends.sync_branches"))
                .into_any_element(),
        );
        for branch in &branches {
            let is_default = default_branch.as_deref() == Some(branch.as_str());
            let is_paused = paused.contains(branch);
            let is_enabled = enabled_set.contains(branch);
            let branch_name = branch.clone();
            let toggle_link = link_id.clone();
            let mut next_enabled = enabled_set.clone();
            if is_enabled {
                next_enabled.remove(branch);
            } else {
                next_enabled.insert(branch.clone());
            }
            let next_enabled: Vec<String> = next_enabled.into_iter().collect();
            let label = if is_default {
                format!("{branch} · default")
            } else {
                branch.clone()
            };
            let mut row = div().mt(px(6.0)).flex().items_center().gap(px(8.0)).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(sp(12.0))
                    .text_color(theme.text)
                    .child(label),
            );
            if is_enabled && is_paused {
                let resume_link = link_id.clone();
                let resume_branch = branch_name.clone();
                row = row
                    .child(
                        div()
                            .text_size(sp(11.0))
                            .text_color(theme.warning)
                            .child(tr!("friends.sync_paused")),
                    )
                    .child(self.friends_button(
                        SharedString::from(format!("sync-now-{link_id}-{branch}")),
                        tr!("friends.sync_now"),
                        theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::FriendSyncNow {
                                    link_id: resume_link.clone(),
                                    branch: resume_branch.clone(),
                                },
                                cx,
                            );
                        },
                        cx,
                    ));
            } else {
                row = row.child(toggle_switch(
                    SharedString::from(format!("sync-branch-{link_id}-{branch}")),
                    is_enabled,
                    false,
                    *theme,
                    cx,
                    move |this, _window, cx| {
                        this.friends_command(
                            waku_client::Command::SetFriendSyncConfig {
                                link_id: toggle_link.clone(),
                                auto_push,
                                enabled_branches: next_enabled.clone(),
                            },
                            cx,
                        );
                    },
                ));
            }
            children.push(row.into_any_element());
        }

        // Teardown.
        let disable_link = link_id.clone();
        let disable_message = tr!(
            "friends.confirm_disable_sync",
            name = repo_name,
            peer = link.peer_name.clone()
        );
        children.push(
            div()
                .mt(px(10.0))
                .flex()
                .justify_end()
                .child(self.friends_button(
                    SharedString::from(format!("sync-disable-{link_id}")),
                    tr!("friends.disable_sync"),
                    theme,
                    move |this, cx| {
                        this.friends_confirm_command(
                            disable_message.clone(),
                            Some(tr!("friends.confirm_disable_sync_detail")),
                            tr!("friends.disable_sync"),
                            waku_client::Command::DisableFriendSync {
                                link_id: disable_link.clone(),
                            },
                            cx,
                        );
                    },
                    cx,
                ))
                .into_any_element(),
        );

        div()
            .flex()
            .flex_col()
            .children(children)
            .into_any_element()
    }

    /// One sync-alert row: the conflict decisions or a dirty-worktree
    /// refusal. Resolve-in-chat opens an agent session on the stopped
    /// checkout.
    fn render_sync_alert_row(
        &self,
        alert: &SyncAlertInfo,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let alert_id = alert.id.clone();
        let (title, hint) = match alert.kind {
            SyncAlertKind::Conflict => {
                let integration = match alert.in_progress {
                    Some(waku_client::git::SyncInProgress::Merge) => "merge",
                    _ => "rebase",
                };
                (
                    tr!("friends.sync_conflict_title", branch = alert.branch.clone()).to_string(),
                    tr!(
                        "friends.sync_conflict_hint",
                        count = alert.files.len(),
                        integration = integration,
                        path = alert.worktree_path.display().to_string()
                    )
                    .to_string(),
                )
            }
            SyncAlertKind::RefusedDirtyWorktree => (
                tr!("friends.sync_refused_title", branch = alert.branch.clone()).to_string(),
                tr!(
                    "friends.sync_refused_hint",
                    path = alert.worktree_path.display().to_string()
                )
                .to_string(),
            ),
        };

        let mut row = div().mt(px(10.0)).flex().items_center().gap(px(8.0)).child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.text)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(sp(11.5))
                        .text_color(theme.text_tertiary)
                        .child(hint),
                ),
        );

        match alert.kind {
            SyncAlertKind::Conflict => {
                let merge_id = alert_id.clone();
                let abort_id = alert_id.clone();
                let abort_branch = alert.branch.clone();
                let dismiss_id = alert_id.clone();
                let resolve_alert = alert.clone();
                row = row
                    .child(self.friends_button(
                        SharedString::from(format!("sync-resolve-{alert_id}")),
                        tr!("friends.resolve_in_chat"),
                        theme,
                        move |this, cx| this.resolve_sync_alert_in_chat(resolve_alert.clone(), cx),
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("sync-merge-{alert_id}")),
                        tr!("friends.merge_instead"),
                        theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::FriendSyncAlertAction {
                                    alert_id: merge_id.clone(),
                                    action: FriendSyncAlertAction::MergeInstead,
                                },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("sync-abort-{alert_id}")),
                        tr!("friends.abort_sync"),
                        theme,
                        move |this, cx| {
                            this.friends_confirm_command(
                                tr!("friends.confirm_abort_sync", branch = abort_branch.clone()),
                                Some(tr!("friends.confirm_abort_sync_detail")),
                                tr!("friends.abort_sync"),
                                waku_client::Command::FriendSyncAlertAction {
                                    alert_id: abort_id.clone(),
                                    action: FriendSyncAlertAction::Abort,
                                },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("sync-dismiss-{alert_id}")),
                        tr!("friends.dismiss"),
                        theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::FriendSyncAlertAction {
                                    alert_id: dismiss_id.clone(),
                                    action: FriendSyncAlertAction::Dismiss,
                                },
                                cx,
                            );
                        },
                        cx,
                    ));
            }
            SyncAlertKind::RefusedDirtyWorktree => {
                let retry_link = alert.link_id.clone();
                let retry_branch = alert.branch.clone();
                let dismiss_id = alert_id.clone();
                row = row
                    .child(self.friends_button(
                        SharedString::from(format!("sync-retry-{alert_id}")),
                        tr!("friends.sync_now"),
                        theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::FriendSyncNow {
                                    link_id: retry_link.clone(),
                                    branch: retry_branch.clone(),
                                },
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(self.friends_button(
                        SharedString::from(format!("sync-dismiss-{alert_id}")),
                        tr!("friends.dismiss"),
                        theme,
                        move |this, cx| {
                            this.friends_command(
                                waku_client::Command::FriendSyncAlertAction {
                                    alert_id: dismiss_id.clone(),
                                    action: FriendSyncAlertAction::Dismiss,
                                },
                                cx,
                            );
                        },
                        cx,
                    ));
            }
        }
        row.into_any_element()
    }

    /// "Resolve in chat": open a task on the repo (bound to the stopped
    /// worktree when it isn't the primary checkout) with the conflict
    /// context as its first turn.
    fn resolve_sync_alert_in_chat(&mut self, alert: SyncAlertInfo, cx: &mut Context<Self>) {
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.path == alert.repo_path)
            .cloned()
        else {
            self.show_toast(tr!("friends.command_failed", error = "project not found"));
            return;
        };
        self.settings_page = None;
        self.create_session_for(project.id, self.state.last_provider, cx);
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        // A conflict stopped in a linked worktree binds the session to
        // that checkout; the repo root and our temp worktrees stay Local —
        // the prompt names the exact checkout either way.
        if !alert.temp_worktree && alert.worktree_path != alert.repo_path {
            let name = alert
                .worktree_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(session) = self.state.session_mut(session_id) {
                session.workspace = SessionWorkspace::Worktree {
                    path: alert.worktree_path.clone(),
                    name,
                    branch: Some(alert.branch.clone()),
                    base_branch: None,
                };
            }
        }
        let integration = match alert.in_progress {
            Some(waku_client::git::SyncInProgress::Merge) => "merge",
            _ => "rebase",
        };
        let files = alert
            .files
            .iter()
            .map(|file| format!("- {file}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "Automatic sync stopped a {integration} on `{branch}` in the checkout at {path}. \
             Resolve the conflicts and run `git {integration} --continue` to finish.\n\n\
             Conflicted files:\n{files}",
            integration = integration,
            branch = alert.branch,
            path = alert.worktree_path.display(),
            files = files,
        );
        self.submit_composer_submission_to(session_id, ComposerSubmission::plain(prompt), cx);
    }
}

/// What this install shows for a friend: local nickname, else their
/// self-reported name.
pub(super) fn friend_display_name(friend: &FriendInfo) -> &str {
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
