//! Confirmation shown before archiving or sweeping a task that still has
//! something to lose: a turn in progress that the move would stop, or a
//! checkout holding uncommitted files or unpushed commits — the work the
//! snapshot is about to carry away. Settled sessions with clean, fully
//! pushed checkouts move directly; this dialog never opens for them.

use gpui::{KeyBinding, actions};

use super::*;

actions!(
    waku_archive_dialog,
    [ConfirmArchiveDialog, DismissArchiveDialog]
);

const DIALOG_CONTEXT: &str = "ArchiveDialog";
const SUMMARY_MAX_HEIGHT: f32 = 220.0;

/// What confirming the dialog does: archive hides the task outright, while a
/// sweep only parks it in the sidebar's Dormant group. Both snapshot and
/// remove the worktree the same way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ArchiveDialogKind {
    Archive,
    Dormant,
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmArchiveDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissArchiveDialog, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct ArchiveDialogState {
    session_id: Uuid,
    kind: ArchiveDialogKind,
    /// The sidebar row the session occupied when the archive started —
    /// carried through to `finish_archive_session` untouched by the dialog.
    landing_row: Option<usize>,
    title: String,
    /// Whether the session still had a live turn when the dialog opened —
    /// confirming stops it.
    active_turn: bool,
    preview: crate::git_commit::ArchivePreview,
    scroll: ScrollHandle,
    archive_focus: FocusHandle,
    cancel_focus: FocusHandle,
    /// Confirmations from the same batch or overlapping background previews
    /// wait here instead of replacing the task currently being reviewed.
    pending: VecDeque<Self>,
}

impl ArchiveDialogState {
    fn enqueue(&mut self, dialog: Self) -> FocusHandle {
        if !(self.session_id == dialog.session_id && self.kind == dialog.kind)
            && !self.pending.iter().any(|pending| {
                pending.session_id == dialog.session_id && pending.kind == dialog.kind
            })
        {
            self.pending.push_back(dialog);
        }
        self.archive_focus.clone()
    }

    fn take_next(&mut self) -> Option<Self> {
        let mut next = self.pending.pop_front()?;
        next.pending = std::mem::take(&mut self.pending);
        Some(next)
    }
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
        landing_row: Option<usize>,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        self.open_kind_dialog(
            session_id,
            ArchiveDialogKind::Archive,
            preview,
            active_turn,
            landing_row,
            cx,
        )
    }

    /// The dormant sweep's confirmation — same warnings and the same
    /// landing-row bookkeeping as archive, different outcome.
    pub(super) fn open_dormant_dialog(
        &mut self,
        session_id: Uuid,
        preview: crate::git_commit::ArchivePreview,
        active_turn: bool,
        landing_row: Option<usize>,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        self.open_kind_dialog(
            session_id,
            ArchiveDialogKind::Dormant,
            preview,
            active_turn,
            landing_row,
            cx,
        )
    }

    fn open_kind_dialog(
        &mut self,
        session_id: Uuid,
        kind: ArchiveDialogKind,
        preview: crate::git_commit::ArchivePreview,
        active_turn: bool,
        landing_row: Option<usize>,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        let title = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.display_title().to_owned())
            .unwrap_or_default();
        let archive_focus = cx.focus_handle();
        let state = ArchiveDialogState {
            session_id,
            kind,
            landing_row,
            title,
            active_turn,
            preview,
            scroll: ScrollHandle::new(),
            cancel_focus: cx.focus_handle(),
            archive_focus: archive_focus.clone(),
            pending: VecDeque::new(),
        };
        if let Some(dialog) = self.archive_dialog.as_mut() {
            return dialog.enqueue(state);
        }
        self.archive_dialog = Some(state);
        cx.notify();
        archive_focus
    }

    fn confirm_archive_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut dialog) = self.archive_dialog.take() else {
            return;
        };
        self.archive_dialog = dialog.take_next();
        match dialog.kind {
            ArchiveDialogKind::Archive => {
                self.finish_archive_session(dialog.session_id, dialog.landing_row, window, cx)
            }
            ArchiveDialogKind::Dormant => {
                self.finish_sweep_session(dialog.session_id, dialog.landing_row, window, cx)
            }
        }
        self.focus_next_archive_dialog(window, cx);
    }

    fn close_archive_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut dialog) = self.archive_dialog.take() else {
            return;
        };
        self.archive_dialog = dialog.take_next();
        if self.focus_next_archive_dialog(window, cx) {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn focus_next_archive_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(dialog) = self.archive_dialog.as_ref() else {
            return false;
        };
        let focus = dialog.archive_focus.clone();
        cx.notify();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
        true
    }

    pub(super) fn render_archive_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.archive_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let files = dialog.preview.files.clone();
        let commits = dialog.preview.unpushed_commits.clone();
        // The listed paths are relative to the session's checkout.
        let session_workspace = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == dialog.session_id)
            .and_then(|session| self.workspace_path_for_session(session))
            .map(std::path::Path::to_path_buf);
        let has_checkout_work = !files.is_empty() || !commits.is_empty();
        let description = match (dialog.active_turn, has_checkout_work) {
            (true, true) => match dialog.kind {
                ArchiveDialogKind::Archive => tr!("archive.confirm_description_busy_worktree"),
                ArchiveDialogKind::Dormant => tr!("dormant.confirm_description_busy_worktree"),
            },
            (true, false) => match dialog.kind {
                ArchiveDialogKind::Archive => tr!("archive.confirm_description_busy"),
                ArchiveDialogKind::Dormant => tr!("dormant.confirm_description_busy"),
            },
            (false, _) => match dialog.kind {
                ArchiveDialogKind::Archive => tr!("archive.confirm_description"),
                ArchiveDialogKind::Dormant => tr!("dormant.confirm_description"),
            },
        };
        let title = if dialog.title.trim().is_empty() || dialog.title == AgentSession::DEFAULT_TITLE
        {
            match dialog.kind {
                ArchiveDialogKind::Archive => tr!("archive.confirm_title"),
                ArchiveDialogKind::Dormant => tr!("dormant.confirm_title"),
            }
        } else {
            match dialog.kind {
                ArchiveDialogKind::Archive => {
                    tr!("archive.confirm_title_named", name = dialog.title.as_str())
                }
                ArchiveDialogKind::Dormant => {
                    tr!("dormant.confirm_title_named", name = dialog.title.as_str())
                }
            }
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
                        let absolute = session_workspace
                            .as_ref()
                            .map(|workspace| workspace.join(&file.path))
                            .unwrap_or_else(|| std::path::PathBuf::from(&file.path));
                        let focus = self
                            .transcript_control_focus(format!("archive-file-{}", file.path), cx);
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
                            .child(file_link(
                                div()
                                    .id(SharedString::from(format!("archive-file-{}", file.path)))
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_family(crate::fonts::current(cx).code)
                                    .text_size(sp(12.5))
                                    .text_color(theme.text)
                                    .child(file.path.clone()),
                                &focus,
                                absolute.to_string_lossy().into_owned(),
                                self,
                                &weak,
                                format!("file-link-menu-archive-file-{}", file.path),
                                cx,
                            ))
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
            match dialog.kind {
                ArchiveDialogKind::Archive => "icons/archive.svg",
                ArchiveDialogKind::Dormant => "icons/broom.svg",
            },
            match dialog.kind {
                ArchiveDialogKind::Archive => tr!("session.archive"),
                ArchiveDialogKind::Dormant => tr!("session.sweep"),
            },
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
            .child(motion::scrim_enter("archive-dialog-layer-enter", scrim))
            .p(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|waku, _, window, cx| waku.close_archive_dialog(window, cx)),
            )
            .child(motion::modal_enter("archive-dialog-card-enter", card));
        Some(gpui::deferred(layer).with_priority(4).into_any_element())
    }
}

pub(super) fn render_archive_action_row(
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
        .focus_visible(|style| style.bg(theme.focus_highlight()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn dialog(session_id: u128, cx: &mut TestAppContext) -> ArchiveDialogState {
        cx.update(|cx| ArchiveDialogState {
            session_id: Uuid::from_u128(session_id),
            kind: ArchiveDialogKind::Archive,
            landing_row: Some(session_id as usize),
            title: format!("Task {session_id}"),
            active_turn: true,
            preview: crate::git_commit::ArchivePreview {
                files: Vec::new(),
                unpushed_commits: vec![format!("Commit {session_id}")],
            },
            scroll: ScrollHandle::new(),
            archive_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            pending: VecDeque::new(),
        })
    }

    #[gpui::test]
    fn overlapping_archive_confirmations_preserve_every_task(cx: &mut TestAppContext) {
        let mut first = dialog(1, cx);
        let first_focus = first.archive_focus.clone();
        // Background previews can finish while the first modal is visible.
        assert_eq!(first.enqueue(dialog(3, cx)), first_focus);
        assert_eq!(first.enqueue(dialog(2, cx)), first_focus);
        assert_eq!(first.session_id, Uuid::from_u128(1));

        // Both confirm and cancel advance to the next retained warning.
        let mut second = first.take_next().unwrap();
        assert_eq!(second.session_id, Uuid::from_u128(3));
        assert_eq!(second.landing_row, Some(3));
        assert_eq!(second.preview.unpushed_commits, ["Commit 3"]);
        let mut third = second.take_next().unwrap();
        assert_eq!(third.session_id, Uuid::from_u128(2));
        assert_eq!(third.landing_row, Some(2));
        assert_eq!(third.preview.unpushed_commits, ["Commit 2"]);
        assert!(third.take_next().is_none());
    }

    #[gpui::test]
    fn repeated_archive_requests_do_not_duplicate_confirmations(cx: &mut TestAppContext) {
        let mut first = dialog(1, cx);
        first.enqueue(dialog(1, cx));
        first.enqueue(dialog(2, cx));
        first.enqueue(dialog(2, cx));
        let mut second = first.take_next().unwrap();
        assert_eq!(second.session_id, Uuid::from_u128(2));
        assert!(second.take_next().is_none());
    }
}
