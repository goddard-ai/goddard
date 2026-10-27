//! Confirmation shown before archiving or sweeping a task that still has
//! something to lose: a turn in progress that the move would stop, or a
//! checkout holding uncommitted files or unpushed commits — the work the
//! snapshot is about to carry away. Settled sessions with clean, fully
//! pushed checkouts move directly; this dialog never opens for them.

use gpui::{KeyBinding, ScrollStrategy, UniformListScrollHandle, actions, uniform_list};

use super::*;

actions!(
    waku_archive_dialog,
    [
        ConfirmArchiveDialog,
        DismissArchiveDialog,
        PreviewNextRow,
        PreviewPreviousRow,
        PreviewNextPage,
        PreviewPreviousPage,
        PreviewFirstRow,
        PreviewLastRow,
        OpenPreviewFile,
        OpenPreviewFileMenu
    ]
);

const DIALOG_CONTEXT: &str = "ArchiveDialog";
const SUMMARY_MAX_HEIGHT: f32 = 220.0;
const PREVIEW_ROW_HEIGHT: f32 = 24.0;
const SUMMARY_BOTTOM_PADDING: f32 = 8.0;
const PREVIEW_CONTEXT: &str = "ArchivePreview";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewRow {
    FilesHeading,
    File(usize),
    CommitsHeading,
    Commit(usize),
}

/// Index the two sections without building or copying a row per preview entry.
#[derive(Clone, Copy)]
struct PreviewRows {
    files: usize,
    commits: usize,
}

impl PreviewRows {
    fn new(preview: &crate::git_commit::ArchivePreview) -> Self {
        Self {
            files: preview.files.len(),
            commits: preview.unpushed_commits.len(),
        }
    }

    fn len(self) -> usize {
        self.files + usize::from(self.files > 0) + self.commits + usize::from(self.commits > 0)
    }

    fn at(self, mut index: usize) -> Option<PreviewRow> {
        if self.files > 0 {
            if index == 0 {
                return Some(PreviewRow::FilesHeading);
            }
            if index <= self.files {
                return Some(PreviewRow::File(index - 1));
            }
            index -= self.files + 1;
        }
        if self.commits > 0 {
            if index == 0 {
                return Some(PreviewRow::CommitsHeading);
            }
            if index <= self.commits {
                return Some(PreviewRow::Commit(index - 1));
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
enum PreviewNavigation {
    Next,
    Previous,
    NextPage,
    PreviousPage,
    First,
    Last,
}

fn preview_cursor(current: usize, count: usize, movement: PreviewNavigation) -> usize {
    let last = count.saturating_sub(1);
    let page =
        ((SUMMARY_MAX_HEIGHT - SUMMARY_BOTTOM_PADDING) / PREVIEW_ROW_HEIGHT).floor() as usize;
    match movement {
        PreviewNavigation::Next => current.saturating_add(1).min(last),
        PreviewNavigation::Previous => current.saturating_sub(1).min(last),
        PreviewNavigation::NextPage => current.saturating_add(page).min(last),
        PreviewNavigation::PreviousPage => current.saturating_sub(page).min(last),
        PreviewNavigation::First => 0,
        PreviewNavigation::Last => last,
    }
}

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
        KeyBinding::new("down", PreviewNextRow, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("up", PreviewPreviousRow, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("pagedown", PreviewNextPage, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("pageup", PreviewPreviousPage, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("home", PreviewFirstRow, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("end", PreviewLastRow, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("enter", OpenPreviewFile, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("space", OpenPreviewFile, Some(PREVIEW_CONTEXT)),
        KeyBinding::new("shift-f10", OpenPreviewFileMenu, Some(PREVIEW_CONTEXT)),
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
    // Every abandoned/closed/duplicate dialog retires its plain payload on
    // the worker pool; focus and scroll state stay on the UI thread.
    preview: Arc<BackgroundOwned<crate::git_commit::ArchivePreview>>,
    scroll: UniformListScrollHandle,
    preview_focus: FocusHandle,
    preview_cursor: usize,
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
        preview: BackgroundOwned<crate::git_commit::ArchivePreview>,
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
        preview: BackgroundOwned<crate::git_commit::ArchivePreview>,
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
        preview: BackgroundOwned<crate::git_commit::ArchivePreview>,
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
            preview: Arc::new(preview),
            scroll: UniformListScrollHandle::new(),
            preview_focus: cx.focus_handle(),
            preview_cursor: 0,
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

    fn navigate_archive_preview(
        &mut self,
        movement: PreviewNavigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.archive_dialog.as_mut() {
            dialog.preview_cursor = preview_cursor(
                dialog.preview_cursor,
                PreviewRows::new(dialog.preview.get()).len(),
                movement,
            );
            dialog
                .scroll
                .scroll_to_item(dialog.preview_cursor, ScrollStrategy::Nearest);
            window.focus(&dialog.preview_focus, cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn archive_preview_file_path(&self) -> Option<std::path::PathBuf> {
        let dialog = self.archive_dialog.as_ref()?;
        let PreviewRow::File(index) =
            PreviewRows::new(dialog.preview.get()).at(dialog.preview_cursor)?
        else {
            return None;
        };
        let path = &dialog.preview.get().files[index].path;
        let workspace = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == dialog.session_id)
            .and_then(|session| self.workspace_path_for_session(session));
        Some(
            workspace
                .map(|root| root.join(path))
                .unwrap_or_else(|| path.into()),
        )
    }

    fn render_archive_preview_row(
        &mut self,
        preview: &crate::git_commit::ArchivePreview,
        row: PreviewRow,
        workspace: Option<&std::path::Path>,
        selected: bool,
        weak: &WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let base = div()
            .h(px(PREVIEW_ROW_HEIGHT))
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .when(selected, |row| row.bg(theme.overlay));
        match row {
            PreviewRow::FilesHeading | PreviewRow::CommitsHeading => base
                .text_size(sp(11.5))
                .text_color(theme.text_secondary)
                .child(match row {
                    PreviewRow::FilesHeading => {
                        tr!("archive.uncommitted", count = preview.files.len())
                    }
                    _ => tr!("archive.unpushed", count = preview.unpushed_commits.len()),
                })
                .into_any_element(),
            PreviewRow::File(index) => {
                let file = &preview.files[index];
                let absolute = workspace
                    .map(|root| root.join(&file.path))
                    .unwrap_or_else(|| file.path.clone().into());
                // The list is one keyboard stop; arrows/page/home/end reach
                // every row, Enter/Space open the selected file, Shift-F10
                // opens its menu. Individual pointer links keep their menus.
                let focus = self
                    .transcript_control_focus(format!("archive-file-{}", file.path), cx)
                    .tab_stop(false);
                base.child(
                    div()
                        .flex_none()
                        .w(px(26.0))
                        .font_family(crate::fonts::current(cx).code)
                        .text_size(sp(11.0))
                        .text_color(theme.text_ghost)
                        .child(file.status.clone()),
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
                    weak,
                    format!("file-link-menu-archive-file-{}", file.path),
                    cx,
                ))
                .into_any_element()
            }
            PreviewRow::Commit(index) => base
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
                        .child(preview.unpushed_commits[index].clone()),
                )
                .into_any_element(),
        }
    }

    pub(super) fn render_archive_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.archive_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let preview = Arc::clone(&dialog.preview);
        let rows = PreviewRows::new(preview.get());
        // The listed paths are relative to the session's checkout.
        let session_workspace = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == dialog.session_id)
            .and_then(|session| self.workspace_path_for_session(session))
            .map(std::path::Path::to_path_buf);
        let has_checkout_work = rows.len() > 0;
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

        let row_weak = weak.clone();
        let preview_focus = dialog.preview_focus.clone();
        let cursor = dialog.preview_cursor;
        let summary = uniform_list(
            "archive-preview-rows",
            rows.len(),
            move |range, window, cx| {
                let focused = preview_focus.is_focused(window);
                // This follows the projects-list boundary: layout calls the range
                // builder after the root render lease. Row helpers use that one
                // explicit Waku lease and never re-enter it.
                row_weak
                    .update(cx, |this, cx| {
                        range
                            .filter_map(|index| {
                                rows.at(index).map(|row| {
                                    this.render_archive_preview_row(
                                        preview.get(),
                                        row,
                                        session_workspace.as_deref(),
                                        focused && index == cursor,
                                        &row_weak,
                                        cx,
                                    )
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            },
        )
        .track_scroll(&dialog.scroll)
        .h(px((rows.len() as f32 * PREVIEW_ROW_HEIGHT)
            .min(SUMMARY_MAX_HEIGHT - SUMMARY_BOTTOM_PADDING)))
        .w_full();

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
                    .key_context(PREVIEW_CONTEXT)
                    .track_focus(&dialog.preview_focus)
                    .tab_index(0)
                    .on_action(cx.listener(|this, _: &PreviewNextRow, window, cx| {
                        this.navigate_archive_preview(PreviewNavigation::Next, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PreviewPreviousRow, window, cx| {
                        this.navigate_archive_preview(PreviewNavigation::Previous, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PreviewNextPage, window, cx| {
                        this.navigate_archive_preview(PreviewNavigation::NextPage, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PreviewPreviousPage, window, cx| {
                        this.navigate_archive_preview(PreviewNavigation::PreviousPage, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PreviewFirstRow, window, cx| {
                        this.navigate_archive_preview(PreviewNavigation::First, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PreviewLastRow, window, cx| {
                        this.navigate_archive_preview(PreviewNavigation::Last, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &OpenPreviewFile, _, cx| {
                        if let Some(path) = this.archive_preview_file_path() {
                            this.open_path_in_default_app(&path.to_string_lossy(), cx);
                        }
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &OpenPreviewFileMenu, window, cx| {
                        if let Some(dialog) = this.archive_dialog.as_ref()
                            && let Some(PreviewRow::File(index)) =
                                PreviewRows::new(dialog.preview.get()).at(dialog.preview_cursor)
                        {
                            let path = &dialog.preview.get().files[index].path;
                            this.menu_handle(format!("file-link-menu-archive-file-{path}"), cx)
                                .open_context_menu(window, cx);
                        }
                        cx.stop_propagation();
                    }))
                    .px(px(12.0))
                    .pb(px(SUMMARY_BOTTOM_PADDING))
                    .overflow_hidden()
                    .child(summary),
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
            preview: Arc::new(BackgroundOwned::new(crate::git_commit::ArchivePreview {
                files: Vec::new(),
                unpushed_commits: vec![format!("Commit {session_id}")],
            })),
            scroll: UniformListScrollHandle::new(),
            preview_focus: cx.focus_handle(),
            preview_cursor: 0,
            archive_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            pending: VecDeque::new(),
        })
    }

    #[test]
    fn preview_sections_are_lossless_without_allocating_a_row_vector() {
        for (files, commits, expected) in [
            (0, 0, vec![]),
            (
                2,
                0,
                vec![
                    PreviewRow::FilesHeading,
                    PreviewRow::File(0),
                    PreviewRow::File(1),
                ],
            ),
            (
                0,
                1,
                vec![PreviewRow::CommitsHeading, PreviewRow::Commit(0)],
            ),
            (
                1,
                2,
                vec![
                    PreviewRow::FilesHeading,
                    PreviewRow::File(0),
                    PreviewRow::CommitsHeading,
                    PreviewRow::Commit(0),
                    PreviewRow::Commit(1),
                ],
            ),
        ] {
            let rows = PreviewRows { files, commits };
            assert_eq!(rows.len(), expected.len());
            assert_eq!(
                (0..rows.len())
                    .map(|index| rows.at(index).unwrap())
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(rows.at(rows.len()), None);
            assert_eq!(rows.at(usize::MAX), None);
        }
        let rows = PreviewRows {
            files: 100_000,
            commits: 20_000,
        };
        assert_eq!(rows.at(100_000), Some(PreviewRow::File(99_999)));
        assert_eq!(rows.at(100_001), Some(PreviewRow::CommitsHeading));
        assert_eq!(rows.at(rows.len() - 1), Some(PreviewRow::Commit(19_999)));
        let last = preview_cursor(0, rows.len(), PreviewNavigation::Last);
        assert_eq!(last, rows.len() - 1);
        assert_eq!(
            preview_cursor(last, rows.len(), PreviewNavigation::Next),
            last
        );
        assert_eq!(
            preview_cursor(last, rows.len(), PreviewNavigation::First),
            0
        );
        assert_eq!(
            preview_cursor(0, rows.len(), PreviewNavigation::PreviousPage),
            0
        );
        assert!(preview_cursor(0, rows.len(), PreviewNavigation::NextPage) > 1);
        assert_eq!(preview_cursor(0, 0, PreviewNavigation::Last), 0);
    }

    struct PreviewKeyScope {
        preview_focus: FocusHandle,
        confirm_focus: FocusHandle,
        opened: usize,
        confirmed: usize,
    }

    impl Render for PreviewKeyScope {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context(DIALOG_CONTEXT)
                .on_action(cx.listener(|this, _: &ConfirmArchiveDialog, _, _| this.confirmed += 1))
                .child(
                    div()
                        .key_context(PREVIEW_CONTEXT)
                        .track_focus(&self.preview_focus)
                        .on_action(cx.listener(|this, _: &OpenPreviewFile, _, cx| {
                            this.opened += 1;
                            cx.stop_propagation();
                        })),
                )
                .child(div().track_focus(&self.confirm_focus))
        }
    }

    #[gpui::test]
    fn opening_a_preview_file_does_not_confirm_the_archive(cx: &mut TestAppContext) {
        cx.update(init);
        let (view, cx) = cx.add_window_view(|_, cx| PreviewKeyScope {
            preview_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            opened: 0,
            confirmed: 0,
        });
        let (preview, confirm) = view.read_with(cx, |this, _| {
            (this.preview_focus.clone(), this.confirm_focus.clone())
        });
        cx.update(|window, cx| window.focus(&preview, cx));
        cx.simulate_keystrokes("enter space");
        view.read_with(cx, |this, _| {
            assert_eq!(this.opened, 2);
            assert_eq!(this.confirmed, 0);
        });
        cx.update(|window, cx| window.focus(&confirm, cx));
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |this, _| assert_eq!(this.confirmed, 1));
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
        assert_eq!(second.preview.get().unpushed_commits, ["Commit 3"]);
        let mut third = second.take_next().unwrap();
        assert_eq!(third.session_id, Uuid::from_u128(2));
        assert_eq!(third.landing_row, Some(2));
        assert_eq!(third.preview.get().unpushed_commits, ["Commit 2"]);
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
