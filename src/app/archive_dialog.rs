//! Confirmation shown before archiving a task that still has something to
//! lose: a turn in progress that archiving would stop, or a checkout holding
//! uncommitted files or unpushed commits — the work the archive snapshot is
//! about to carry away. Settled sessions with clean, fully pushed checkouts
//! archive directly; this dialog never opens for them.

use gpui::{KeyBinding, actions};

use super::*;

actions!(
    waku_archive_dialog,
    [ConfirmArchiveDialog, DismissArchiveDialog]
);

const DIALOG_CONTEXT: &str = "ArchiveDialog";
const SUMMARY_MAX_HEIGHT: f32 = 220.0;

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmArchiveDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissArchiveDialog, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct ArchiveDialogState {
    session_id: Uuid,
    /// Sidebar row index when the archive came from a sidebar row — carried
    /// through to `finish_archive_session` so selection lands on a positional
    /// neighbor. `None` for every other archive entry point.
    sidebar_position: Option<usize>,
    title: String,
    /// Whether the session still had a live turn when the dialog opened —
    /// confirming stops it.
    active_turn: bool,
    preview: crate::git_commit::ArchivePreview,
    scroll: ScrollHandle,
    archive_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl Waku {
    /// Opens the confirmation for a session whose archive preview reported
    /// work in flight or whose turn is still running. The caller owns the
    /// pending-preview bookkeeping.
    pub(super) fn open_archive_dialog(
        &mut self,
        session_id: Uuid,
        preview: crate::git_commit::ArchivePreview,
        active_turn: bool,
        sidebar_position: Option<usize>,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        let title = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.title.clone())
            .unwrap_or_default();
        let archive_focus = cx.focus_handle();
        let state = ArchiveDialogState {
            session_id,
            sidebar_position,
            title,
            active_turn,
            preview,
            scroll: ScrollHandle::new(),
            cancel_focus: cx.focus_handle(),
            archive_focus: archive_focus.clone(),
        };
        self.archive_dialog = Some(state);
        cx.notify();
        archive_focus
    }

    fn confirm_archive_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.archive_dialog.take() else {
            return;
        };
        self.finish_archive_session(dialog.session_id, dialog.sidebar_position, window, cx);
    }

    fn close_archive_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.archive_dialog.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn render_archive_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.archive_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let files = dialog.preview.files.clone();
        let commits = dialog.preview.unpushed_commits.clone();
        let has_checkout_work = !files.is_empty() || !commits.is_empty();
        let description = match (dialog.active_turn, has_checkout_work) {
            (true, true) => tr!("archive.confirm_description_busy_worktree"),
            (true, false) => tr!("archive.confirm_description_busy"),
            (false, _) => tr!("archive.confirm_description"),
        };
        let title = if dialog.title.trim().is_empty() {
            tr!("archive.confirm_title")
        } else {
            tr!("archive.confirm_title_named", name = dialog.title.as_str())
        };

        let mut sections = Vec::new();
        if !files.is_empty() {
            sections.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .px(px(4.0))
                            .pb(px(4.0))
                            .text_size(sp(11.5))
                            .text_color(theme.text_secondary)
                            .child(tr!("archive.uncommitted", count = files.len())),
                    )
                    .children(files.into_iter().map(|file| {
                        div()
                            .h(px(24.0))
                            .px(px(4.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(26.0))
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_ghost)
                                    .child(file.status),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(12.5))
                                    .text_color(theme.text)
                                    .child(file.path),
                            )
                            .into_any_element()
                    }))
                    .into_any_element(),
            );
        }
        if !commits.is_empty() {
            sections.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .px(px(4.0))
                            .pb(px(4.0))
                            .text_size(sp(11.5))
                            .text_color(theme.text_secondary)
                            .child(tr!("archive.unpushed", count = commits.len())),
                    )
                    .children(commits.into_iter().map(|subject| {
                        div()
                            .h(px(24.0))
                            .px(px(4.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon(
                                "icons/git-commit-horizontal.svg",
                                12.0,
                                theme.text_ghost,
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text)
                                    .child(subject),
                            )
                            .into_any_element()
                    }))
                    .into_any_element(),
            );
        }

        let archive_row = render_archive_action_row(
            "archive-dialog-archive",
            &dialog.archive_focus,
            "icons/archive.svg",
            tr!("session.archive"),
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_archive_dialog(window, cx),
        );
        let cancel_row = render_archive_action_row(
            "archive-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.close_archive_dialog(window, cx),
        );

        let card = div()
            .id("archive-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmArchiveDialog, window, cx| {
                waku.confirm_archive_dialog(window, cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissArchiveDialog, window, cx| {
                waku.close_archive_dialog(window, cx)
            }))
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
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(description),
                    ),
            )
            .child(
                div()
                    .id("archive-dialog-summary")
                    .px(px(12.0))
                    .pb(px(8.0))
                    .max_h(px(SUMMARY_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .track_scroll(&dialog.scroll)
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .children(sections),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(archive_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("archive-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.close_archive_dialog(window, cx)),
            )
            .child(motion::modal_enter("archive-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("archive-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

fn render_archive_action_row(
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
        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
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
