//! The "Reclaim Disk Space" dialog: a user-invoked listing of the
//! regenerable output — dependency installs, build artifacts — each idle
//! worktree still holds. A session qualifies when it is started,
//! unarchived, not busy, worktree-backed, and untouched past the idle
//! window; the daemon reports only git-ignored directories on its
//! reproducible-name allowlist, so nothing tracked or hand-authored can
//! be selected. Deletion is always the user's click — nothing here
//! purges on its own.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{KeyBinding, actions};

use super::*;

actions!(
    waku_reclaim_dialog,
    [ConfirmReclaimDialog, DismissReclaimDialog]
);

const DIALOG_CONTEXT: &str = "ReclaimDialog";
const LIST_MAX_HEIGHT: f32 = 260.0;

/// How long a worktree sits untouched before its regenerable output is
/// worth offering back — the "won't notice a cold install" window.
const RECLAIM_IDLE_SECS: u64 = 48 * 60 * 60;

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmReclaimDialog, Some(DIALOG_CONTEXT)),
        KeyBinding::new("escape", DismissReclaimDialog, Some(DIALOG_CONTEXT)),
    ]);
}

pub(super) struct ReclaimDialogState {
    /// Invalidates scan results still in flight for a replaced dialog.
    generation: u64,
    /// Worktree scans yet to answer; rows render only once all are in,
    /// so the list arrives already sorted by size.
    pending_scans: usize,
    rows: Vec<ReclaimDialogRow>,
    scroll: ScrollHandle,
    reclaim_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

struct ReclaimDialogRow {
    session_id: Uuid,
    title: String,
    /// The session workspace the scan ran against, in the daemon's path
    /// spelling — also the `cwd` the reclaim request goes back to.
    cwd: PathBuf,
    paths: Vec<PathBuf>,
    /// Counted directory names — "node_modules ×3, target" — so the row
    /// says what would go, not just how much.
    names: String,
    bytes: u64,
    selected: bool,
    focus: FocusHandle,
}

/// Totals for one confirm's background purges. The completion toast
/// fires when the last request lands; `generation` drops results of a
/// batch a newer confirm superseded.
#[derive(Default)]
pub(super) struct ReclaimBatch {
    generation: u64,
    pending: usize,
    reclaimed_bytes: u64,
    failures: usize,
}

impl Waku {
    /// The eligibility the palette command, the row menu, and the
    /// dialog's own scan all agree on: a started, unarchived session
    /// whose worktree has been idle past the window and isn't running
    /// anything the purge could surprise.
    pub(super) fn session_reclaim_eligible(&self, session: &AgentSession) -> bool {
        session.has_started()
            && session.archived_at.is_none()
            && !session.is_busy()
            && Some(session.id) != self.state.selected_session
            && matches!(session.workspace, SessionWorkspace::Worktree { .. })
            && unix_time().saturating_sub(session.updated_at) >= RECLAIM_IDLE_SECS
    }

    /// One row per worktree path — side chats share their parent's, so
    /// the same directories would otherwise list twice.
    fn reclaim_candidates(&self) -> Vec<(Uuid, String, PathBuf)> {
        let mut seen = HashSet::new();
        self.state
            .sessions
            .iter()
            .filter(|session| self.session_reclaim_eligible(session))
            .filter_map(|session| {
                let SessionWorkspace::Worktree { path, .. } = &session.workspace else {
                    return None;
                };
                seen.insert(path.clone())
                    .then(|| (session.id, session.display_title().to_owned(), path.clone()))
            })
            .collect()
    }

    pub(super) fn open_reclaim_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let generation = self.reclaim_dialog_generation.wrapping_add(1);
        self.reclaim_dialog_generation = generation;
        let reclaim_focus = cx.focus_handle();
        self.reclaim_dialog = Some(ReclaimDialogState {
            generation,
            pending_scans: 0,
            rows: Vec::new(),
            scroll: ScrollHandle::new(),
            cancel_focus: cx.focus_handle(),
            reclaim_focus: reclaim_focus.clone(),
        });
        let pending = self
            .reclaim_candidates()
            .into_iter()
            .filter_map(|(session_id, title, cwd)| {
                self.workspace_client_for_session(session_id)
                    .map(|client| (session_id, title, cwd, client))
            })
            .collect::<Vec<_>>();
        if let Some(dialog) = self.reclaim_dialog.as_mut() {
            dialog.pending_scans = pending.len();
        }
        for (session_id, title, cwd, client) in pending {
            let row_cwd = cwd.clone();
            cx.spawn(async move |waku, cx| {
                let entries = cx
                    .background_executor()
                    .spawn(async move {
                        match client
                            .request(waku_client::WorkspaceOperation::InspectReclaimable { cwd })
                        {
                            Ok(waku_client::WorkspaceResult::Reclaimable { entries }) => entries,
                            _ => Vec::new(),
                        }
                    })
                    .await;
                let _ = waku.update(cx, |waku, cx| {
                    let Some(dialog) = waku.reclaim_dialog.as_mut() else {
                        return;
                    };
                    if dialog.generation != generation {
                        return;
                    }
                    dialog.pending_scans = dialog.pending_scans.saturating_sub(1);
                    let bytes = entries.iter().map(|entry| entry.bytes).sum::<u64>();
                    if bytes > 0 {
                        dialog.rows.push(ReclaimDialogRow {
                            session_id,
                            title,
                            cwd: row_cwd,
                            names: reclaim_names(
                                &entries
                                    .iter()
                                    .map(|entry| entry.path.clone())
                                    .collect::<Vec<_>>(),
                            ),
                            paths: entries.into_iter().map(|entry| entry.path).collect(),
                            bytes,
                            selected: true,
                            focus: cx.focus_handle(),
                        });
                    }
                    if dialog.pending_scans == 0 {
                        dialog
                            .rows
                            .sort_by(|left, right| right.bytes.cmp(&left.bytes));
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        // Like the other deferred surfaces, focus lands two frames after
        // the modal joins the dispatch tree.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&reclaim_focus, cx));
        });
        cx.notify();
    }

    fn toggle_reclaim_row(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if let Some(dialog) = self.reclaim_dialog.as_mut()
            && let Some(row) = dialog
                .rows
                .iter_mut()
                .find(|row| row.session_id == session_id)
        {
            row.selected = !row.selected;
            cx.notify();
        }
    }

    fn confirm_reclaim_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.reclaim_dialog.take() else {
            return;
        };
        let selected_bytes = dialog
            .rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.bytes)
            .sum::<u64>();
        // Still scanning or nothing selected — confirm is inert, not a
        // dismiss.
        if dialog.pending_scans > 0 || selected_bytes == 0 {
            self.reclaim_dialog = Some(dialog);
            return;
        }
        let generation = self.reclaim_dialog_generation;
        let mut batch = ReclaimBatch {
            generation,
            ..Default::default()
        };
        for row in dialog.rows.iter().filter(|row| row.selected) {
            // Re-verify at execute time: a session that woke since the
            // scan is left alone rather than purged mid-install.
            let still_safe = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == row.session_id)
                .is_some_and(|session| {
                    !session.is_busy()
                        && matches!(
                            &session.workspace,
                            SessionWorkspace::Worktree { path, .. } if *path == row.cwd
                        )
                });
            let client = still_safe
                .then(|| self.workspace_client_for_session(row.session_id))
                .flatten();
            let Some(client) = client else {
                continue;
            };
            batch.pending += 1;
            let cwd = row.cwd.clone();
            let paths = row.paths.clone();
            cx.spawn(async move |waku, cx| {
                let (reclaimed_bytes, failures) = cx
                    .background_executor()
                    .spawn(async move {
                        match client
                            .request(waku_client::WorkspaceOperation::ReclaimPaths { cwd, paths })
                        {
                            Ok(waku_client::WorkspaceResult::Reclaim {
                                reclaimed_bytes,
                                failures,
                            }) => (reclaimed_bytes, failures.len()),
                            // The request itself failed — one failure for
                            // the batch, nothing freed.
                            _ => (0, 1),
                        }
                    })
                    .await;
                let _ = waku.update(cx, |waku, cx| {
                    let Some(batch) = waku.reclaim_batch.as_mut() else {
                        return;
                    };
                    if batch.generation != generation {
                        return;
                    }
                    batch.reclaimed_bytes += reclaimed_bytes;
                    batch.failures += failures;
                    batch.pending -= 1;
                    if batch.pending == 0 {
                        let bytes = batch.reclaimed_bytes;
                        let failures = batch.failures;
                        waku.reclaim_batch = None;
                        if failures > 0 {
                            waku.show_toast(tr!(
                                "reclaim.completed_partial",
                                size = format_reclaim_bytes(bytes),
                                count = failures
                            ));
                        } else {
                            waku.show_success_toast(tr!(
                                "reclaim.completed",
                                size = format_reclaim_bytes(bytes)
                            ));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        if batch.pending > 0 {
            self.reclaim_batch = Some(batch);
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn close_reclaim_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reclaim_dialog.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn render_reclaim_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.reclaim_dialog.as_ref()?;
        let theme = Theme::current(cx);
        let weak = cx.entity().downgrade();
        let scanning = dialog.pending_scans > 0;
        let selected_bytes = dialog
            .rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.bytes)
            .sum::<u64>();

        let body: AnyElement = if scanning {
            div()
                .h(px(40.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .child(tr!("reclaim.scanning"))
                .into_any_element()
        } else if dialog.rows.is_empty() {
            div()
                .h(px(40.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .text_size(sp(12.5))
                .text_color(theme.text_secondary)
                .child(tr!("reclaim.empty"))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .children(dialog.rows.iter().map(|row| {
                    let session_id = row.session_id;
                    let toggle = move |waku: &mut Waku, cx: &mut Context<Waku>| {
                        waku.toggle_reclaim_row(session_id, cx)
                    };
                    let click_weak = weak.clone();
                    let key_weak = weak.clone();
                    div()
                        .id(SharedString::from(format!("reclaim-row-{session_id}")))
                        .track_focus(&row.focus)
                        .tab_index(0)
                        .h(px(40.0))
                        .w_full()
                        .px(px(10.0))
                        .rounded(px(11.0))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .cursor_default()
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .hover(|style| style.bg(theme.overlay_strong))
                        .child(
                            div()
                                .flex_none()
                                .size(px(15.0))
                                .rounded(px(4.5))
                                .border(hairline())
                                .border_color(if row.selected {
                                    theme.accent
                                } else {
                                    theme.border_strong
                                })
                                .when(row.selected, |element| element.bg(theme.accent))
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(row.selected, |element| {
                                    element.child(icon("icons/check.svg", 10.0, theme.on_inverse))
                                }),
                        )
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
                                        .text_size(sp(12.5))
                                        .text_color(theme.text)
                                        .child(row.title.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(sp(11.0))
                                        .text_color(theme.text_ghost)
                                        .child(row.names.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(format_reclaim_bytes(row.bytes)),
                        )
                        .on_click(move |_, _, cx| {
                            let _ = click_weak.update(cx, |waku, cx| toggle(waku, cx));
                        })
                        .on_key_down(move |event: &KeyDownEvent, _, cx| {
                            if !event.keystroke.modifiers.modified()
                                && matches!(event.keystroke.key.as_str(), "enter" | "space")
                            {
                                let _ = key_weak.update(cx, |waku, cx| toggle(waku, cx));
                                cx.stop_propagation();
                            }
                        })
                        .into_any_element()
                }))
                .into_any_element()
        };

        let reclaim_row = archive_dialog::render_archive_action_row(
            "reclaim-dialog-reclaim",
            &dialog.reclaim_focus,
            "icons/container.svg",
            if selected_bytes > 0 {
                tr!(
                    "reclaim.confirm",
                    size = format_reclaim_bytes(selected_bytes)
                )
            } else {
                tr!("reclaim.confirm_generic")
            },
            weak.clone(),
            &theme,
            |waku, window, cx| waku.confirm_reclaim_dialog(window, cx),
        );
        let cancel_row = archive_dialog::render_archive_action_row(
            "reclaim-dialog-cancel",
            &dialog.cancel_focus,
            "icons/x.svg",
            tr!("common.cancel"),
            weak,
            &theme,
            |waku, window, cx| waku.close_reclaim_dialog(window, cx),
        );

        let card = div()
            .id("reclaim-dialog-card")
            .key_context(DIALOG_CONTEXT)
            .on_action(cx.listener(|waku, _: &ConfirmReclaimDialog, window, cx| {
                waku.confirm_reclaim_dialog(window, cx)
            }))
            .on_action(cx.listener(|waku, _: &DismissReclaimDialog, window, cx| {
                waku.close_reclaim_dialog(window, cx)
            }))
            .tab_group()
            .tab_stop(false)
            .w_full()
            .max_w(px(480.0))
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
                            .child(tr!("reclaim.title")),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("reclaim.description")),
                    ),
            )
            .child(
                div()
                    .id("reclaim-dialog-list")
                    .px(px(12.0))
                    .pb(px(8.0))
                    .max_h(px(LIST_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .track_scroll(&dialog.scroll)
                    .child(body),
            )
            .child(div().mx(px(8.0)).h(hairline()).bg(theme.separator))
            .child(
                div()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(reclaim_row)
                    .child(cancel_row),
            );

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let layer = div()
            .id("reclaim-dialog-layer")
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
                cx.listener(|waku, _, window, cx| waku.close_reclaim_dialog(window, cx)),
            )
            .child(motion::modal_enter("reclaim-dialog-card-enter", card));
        Some(
            gpui::deferred(motion::fade_in("reclaim-dialog-layer-enter", layer))
                .with_priority(4)
                .into_any_element(),
        )
    }
}

/// The "what would go" line under a row's title: each distinct directory
/// name, with a count when the worktree holds more than one.
fn reclaim_names(paths: &[PathBuf]) -> String {
    let mut names: Vec<(String, usize)> = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        match names.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, count)) => *count += 1,
            None => names.push((name, 1)),
        }
    }
    names
        .into_iter()
        .map(|(name, count)| {
            if count > 1 {
                format!("{name} ×{count}")
            } else {
                name
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_reclaim_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
