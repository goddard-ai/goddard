//! Send-file dialog for Settings → Friends. The file picker hands the chosen
//! path here; the modal collects the optional note that rides with the offer
//! and lands above the file in the receiver's chat.

use gpui::{KeyBinding, Subscription, actions};

use super::*;

actions!(
    waku_send_file_dialog,
    [ConfirmSendFileDialog, DismissSendFileDialog]
);

const DIALOG_CONTEXT: &str = "SendFileDialog";
const DIALOG_INPUT_CONTEXT: &str = "SendFileDialog > TextInput";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(
            "secondary-enter",
            ConfirmSendFileDialog,
            Some(DIALOG_INPUT_CONTEXT),
        ),
        KeyBinding::new(
            "secondary-enter",
            ConfirmSendFileDialog,
            Some(DIALOG_CONTEXT),
        ),
        KeyBinding::new("enter", ConfirmSendFileDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissSendFileDialog, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct SendFileDialogState {
    node_id: String,
    peer_name: String,
    path: PathBuf,
    file_name: String,
    is_dir: bool,
    note: Entity<TextInput>,
    send_focus: FocusHandle,
    cancel_focus: FocusHandle,
    file_focus: FocusHandle,
    _subscription: Subscription,
}

impl Waku {
    /// After the file picker resolves: collect the note that rides with the
    /// offer — the receiver's chat shows it above the delivered file.
    pub(super) fn open_send_file_dialog(
        &mut self,
        node_id: String,
        peer_name: String,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .submit_on_enter()
                .accessibility_label(tr!("friends.send_dialog_message"))
                .placeholder(tr!("friends.send_dialog_message_placeholder"))
        });
        let note_focus = note.read(cx).focus();
        let subscription = cx.subscribe(&note, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Submit(_)) {
                this.confirm_send_file_dialog(cx);
            }
        });
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.send_file_dialog = Some(SendFileDialogState {
            node_id,
            peer_name,
            is_dir: path.is_dir(),
            path,
            file_name,
            note,
            send_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            _subscription: subscription,
        });
        // Like Goddard's other deferred surfaces, the modal joins the dispatch
        // tree only after it has drawn. Focus it two frames later so typing
        // cannot fall through to the page beneath it.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&note_focus, cx));
        });
        cx.notify();
    }

    fn confirm_send_file_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.send_file_dialog.take() else {
            return;
        };
        let note = dialog.note.read(cx).content().trim().to_owned();
        self.friends_command(
            waku_client::Command::SendFileToFriend {
                node_id: dialog.node_id.clone(),
                path: dialog.path.clone(),
                note: (!note.is_empty()).then_some(note),
            },
            cx,
        );
        self.restore_send_file_dialog_focus(cx);
    }

    fn close_send_file_dialog(&mut self, cx: &mut Context<Self>) {
        if self.send_file_dialog.take().is_none() {
            return;
        }
        self.restore_send_file_dialog_focus(cx);
    }

    /// The submit subscription has no `Window`; focus returns through the
    /// stored handle — the settings surface the dialog opened from, or the
    /// composer everywhere else.
    fn restore_send_file_dialog_focus(&mut self, cx: &mut Context<Self>) {
        let focus = if self.settings_page.is_some() {
            self.settings_focus.clone()
        } else {
            self.composer_focus(cx)
        };
        let window_handle = self.window_handle;
        let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
        cx.notify();
    }

    pub(super) fn render_send_file_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.send_file_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let title = tr!(
            "friends.send_dialog_title",
            name = dialog.peer_name.as_str()
        );
        let file_icon = if dialog.is_dir {
            "icons/folder.svg"
        } else {
            "icons/file.svg"
        };
        let note = dialog.note.clone();

        let send_row = render_send_file_action_row(
            "send-file-dialog-send",
            &dialog.send_focus,
            "icons/send.svg",
            tr!("friends.send_dialog_send"),
            weak.clone(),
            &theme,
            |waku, cx| waku.confirm_send_file_dialog(cx),
        );
        let cancel_row = render_send_file_action_row(
            "send-file-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak.clone(),
            &theme,
            |waku, cx| waku.close_send_file_dialog(cx),
        );

        let card = div()
            .id("send-file-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmSendFileDialog, _window, cx| {
                waku.confirm_send_file_dialog(cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissSendFileDialog, _window, cx| {
                waku.close_send_file_dialog(cx)
            }))
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(420.0))
            .overflow_hidden()
            .rounded(px(21.0))
            .bg(theme.composer)
            .shadow_xl()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(48.0))
                    .px(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .text_size(sp(14.0))
                    .text_color(theme.text)
                    .child(icon("icons/send.svg", 15.0, theme.text))
                    .child(div().min_w_0().truncate().child(title)),
            )
            .child(
                div()
                    .mx(px(12.0))
                    .h(px(30.0))
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .bg(theme.inset)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(icon(file_icon, 13.0, theme.text_tertiary))
                    .child(file_link(
                        div()
                            .id("send-file-dialog-name")
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(dialog.file_name.clone()),
                        &dialog.file_focus,
                        dialog.path.to_string_lossy().into_owned(),
                        &weak,
                        "file-link-menu-send-file-dialog",
                        cx,
                    )),
            )
            .child(
                div()
                    .h(px(88.0))
                    .px(px(16.0))
                    .py(px(10.0))
                    .text_size(sp(14.0))
                    .line_height(sp(21.0))
                    .text_color(theme.text)
                    .child(note),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(send_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("send-file-dialog-layer")
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
                cx.listener(|waku, _, _window, cx| waku.close_send_file_dialog(cx)),
            )
            .child(motion::modal_enter("send-file-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("send-file-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

fn render_send_file_action_row(
    id: &'static str,
    focus: &FocusHandle,
    icon_path: &'static str,
    label: String,
    weak: WeakEntity<Waku>,
    theme: &Theme,
    action: fn(&mut Waku, &mut Context<Waku>),
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
        .on_click(move |_, _window, cx| {
            let _ = click_weak.update(cx, |waku, cx| action(waku, cx));
        })
        .on_key_down(move |event: &KeyDownEvent, _window, cx| {
            if !event.keystroke.modifiers.modified()
                && matches!(event.keystroke.key.as_str(), "enter" | "space")
            {
                let _ = key_weak.update(cx, |waku, cx| action(waku, cx));
                cx.stop_propagation();
            }
        })
}
