//! The "Reclaim Disk Space" flow: a topbar indicator whose dropdown lists
//! the regenerable output — dependency installs, build artifacts, the
//! task's scratch pool — each idle task still holds. A worktree session
//! qualifies when it is started, unarchived, not busy, and untouched past
//! the idle window; the daemon reports only git-ignored directories on
//! its reproducible-name allowlist, so nothing tracked or hand-authored
//! can be selected. A scratch row qualifies on the same idle rules at
//! any workspace kind — the daemon-owned pool is reproducible by
//! contract — provided no side-chat descendant is busy. Deletion is
//! always the user's click — nothing here purges on its own. Dismissing
//! the dropdown never cancels the flow: the indicator stays put until
//! the discovery card's cancel aborts it or a finished batch is
//! acknowledged.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{KeyBinding, actions, relative};

use super::*;

actions!(waku_reclaim_dialog, [ConfirmReclaimDialog]);

const DIALOG_CONTEXT: &str = "ReclaimDialog";
const LIST_MAX_HEIGHT: f32 = 260.0;
const RECLAIM_PROGRESS_MENU_ID: &str = "reclaim-progress";
const RECLAIM_RESULT_DONE_FOCUS: &str = "reclaim-result-done";
const DISCOVERY_CARD_WIDTH: f32 = 380.0;

/// How long a worktree sits untouched before its regenerable output is
/// worth offering back — the "won't notice a cold install" window.
const RECLAIM_IDLE_SECS: u64 = 48 * 60 * 60;

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "enter",
        ConfirmReclaimDialog,
        Some(DIALOG_CONTEXT),
    )]);
}

pub(super) struct ReclaimDialogState {
    /// Invalidates scan results still in flight for a replaced dialog.
    generation: u64,
    /// One-shot that auto-opens the indicator's dropdown the first frame
    /// the trigger can anchor to.
    reveal: bool,
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
    /// The session workspace the worktree scan ran against, in the
    /// daemon's path spelling — also the `cwd` the reclaimable-paths
    /// request goes back to. `None` when the row only offers the task's
    /// scratch pool.
    cwd: Option<PathBuf>,
    paths: Vec<PathBuf>,
    /// The task scratch pool this row also offers. It deletes through a
    /// session-keyed request — the daemon re-resolves the owning root
    /// task — never through the worktree path set.
    scratch: Option<PathBuf>,
    /// Counted directory names — "node_modules ×3, target" — so the row
    /// says what would go, not just how much.
    names: String,
    bytes: u64,
    selected: bool,
    focus: FocusHandle,
}

/// Totals for one confirm's background purges. The result card lands
/// when the last request does; `generation` drops results of a batch a
/// newer confirm superseded, and `total` lets the header popover say how
/// far along the batch is.
#[derive(Default)]
pub(super) struct ReclaimBatch {
    generation: u64,
    pending: usize,
    total: usize,
    reclaimed_bytes: u64,
    failures: usize,
}

/// A finished batch awaiting acknowledgment. The header indicator keeps
/// its done state until the card's confirm clears this; `reveal` is the
/// one-shot that auto-opens the popover on completion.
pub(super) struct ReclaimResult {
    reclaimed_bytes: u64,
    failures: usize,
    reveal: bool,
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

    /// Either reclaim surface: regenerable worktree directories or the
    /// task's scratch pool. Gates the palette command and the sidebar
    /// menu item — the dialog's own scan decides which entries exist.
    pub(super) fn session_reclaimable(&self, session: &AgentSession) -> bool {
        self.session_reclaim_eligible(session) || self.session_scratch_reclaim_eligible(session)
    }

    /// The scratch counterpart of `session_reclaim_eligible`: a
    /// top-level, idle task — side chats share the parent's pool, so only
    /// the root row offers it — whose whole side-chat family is quiet.
    /// Any workspace kind qualifies: the pool lives outside the worktree.
    pub(super) fn session_scratch_reclaim_eligible(&self, session: &AgentSession) -> bool {
        session.has_started()
            && session.archived_at.is_none()
            && !session.is_busy()
            && session.side_chat_of.is_none()
            && Some(session.id) != self.state.selected_session
            && unix_time().saturating_sub(session.updated_at) >= RECLAIM_IDLE_SECS
            && !self.task_family_busy(session.id)
    }

    /// Whether the session or a task in its side-chat family is busy —
    /// the shared scratch pool must never be reclaimed under a writer.
    fn task_family_busy(&self, session_id: Uuid) -> bool {
        self.state.sessions.iter().any(|candidate| {
            candidate.is_busy() && self.task_family_contains(session_id, candidate.id)
        })
    }

    /// Whether `descendant`'s side-chat ancestry reaches `root` — the
    /// root itself counts, so this is a contains-not-strict-descends test.
    fn task_family_contains(&self, root: Uuid, descendant: Uuid) -> bool {
        let mut id = descendant;
        for _ in 0..32 {
            if id == root {
                return true;
            }
            match self
                .state
                .sessions
                .iter()
                .find(|session| session.id == id)
                .and_then(|session| session.side_chat_of)
            {
                Some(parent) => id = parent,
                None => return false,
            }
        }
        false
    }

    /// One scan target per eligible session — a worktree path plus
    /// whether the task's scratch pool is also in scope. Worktree rows
    /// dedupe by path since side chats share their parent's; scratch
    /// rides the top-level task row only.
    fn reclaim_candidates(&self) -> Vec<(Uuid, String, Option<PathBuf>, bool)> {
        let mut seen = HashSet::new();
        self.state
            .sessions
            .iter()
            .filter_map(|session| {
                let worktree = match &session.workspace {
                    SessionWorkspace::Worktree { path, .. }
                        if self.session_reclaim_eligible(session) =>
                    {
                        seen.insert(path.clone()).then(|| path.clone())
                    }
                    _ => None,
                };
                let scratch = self.session_scratch_reclaim_eligible(session);
                (worktree.is_some() || scratch).then(|| {
                    (
                        session.id,
                        session.display_title().to_owned(),
                        worktree,
                        scratch,
                    )
                })
            })
            .collect()
    }

    pub(super) fn open_reclaim_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A fresh invocation retires a result still awaiting
        // acknowledgment — the indicator reads from the new scan.
        self.reclaim_result = None;
        let generation = self.reclaim_dialog_generation.wrapping_add(1);
        self.reclaim_dialog_generation = generation;
        let reclaim_focus = cx.focus_handle();
        self.reclaim_dialog = Some(ReclaimDialogState {
            generation,
            reveal: true,
            pending_scans: 0,
            rows: Vec::new(),
            scroll: ScrollHandle::new(),
            cancel_focus: cx.focus_handle(),
            reclaim_focus: reclaim_focus.clone(),
        });
        let pending = self
            .reclaim_candidates()
            .into_iter()
            .filter_map(|(session_id, title, cwd, scratch)| {
                self.workspace_client_for_session(session_id)
                    .map(|client| (session_id, title, cwd, scratch, client))
            })
            .collect::<Vec<_>>();
        if let Some(dialog) = self.reclaim_dialog.as_mut() {
            dialog.pending_scans = pending.len();
        }
        // Each RPC blocks a background worker until the daemon answers.
        // Keep one in flight so large worktree sets cannot occupy the entire
        // executor or flood the daemon's shared heavy-request pool.
        cx.spawn(async move |waku, cx| {
            for (session_id, title, cwd, scan_scratch, client) in pending {
                if !waku
                    .read_with(cx, |waku, _| {
                        waku.reclaim_dialog
                            .as_ref()
                            .is_some_and(|dialog| dialog.generation == generation)
                    })
                    .is_ok_and(|current| current)
                {
                    break;
                }
                let (paths, scratch) = cx
                    .background_executor()
                    .spawn({
                        let cwd = cwd.clone();
                        async move {
                            let paths = match &cwd {
                                Some(cwd) => match client.request(
                                    waku_client::WorkspaceOperation::InspectReclaimable {
                                        cwd: cwd.clone(),
                                    },
                                ) {
                                    Ok(waku_client::WorkspaceResult::Reclaimable { entries }) => {
                                        entries
                                    }
                                    _ => Vec::new(),
                                },
                                None => Vec::new(),
                            };
                            let scratch = if scan_scratch {
                                match client.request(
                                    waku_client::WorkspaceOperation::InspectTaskScratch {
                                        session_id,
                                    },
                                ) {
                                    Ok(waku_client::WorkspaceResult::TaskScratch {
                                        path,
                                        bytes,
                                    }) if bytes > 0 => Some((path, bytes)),
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            (paths, scratch)
                        }
                    })
                    .await;
                if !waku
                    .update(cx, |waku, cx| {
                        let Some(dialog) = waku.reclaim_dialog.as_mut() else {
                            return false;
                        };
                        if dialog.generation != generation {
                            return false;
                        }
                        dialog.pending_scans = dialog.pending_scans.saturating_sub(1);
                        let scratch_bytes = scratch.as_ref().map(|(_, bytes)| *bytes).unwrap_or(0);
                        let bytes =
                            paths.iter().map(|entry| entry.bytes).sum::<u64>() + scratch_bytes;
                        if bytes > 0 {
                            let mut names = reclaim_names(
                                &paths
                                    .iter()
                                    .map(|entry| entry.path.clone())
                                    .collect::<Vec<_>>(),
                            );
                            if scratch.is_some() {
                                if !names.is_empty() {
                                    names.push_str(", ");
                                }
                                names.push_str("scratch");
                            }
                            dialog.rows.push(ReclaimDialogRow {
                                session_id,
                                title,
                                cwd,
                                names,
                                paths: paths.into_iter().map(|entry| entry.path).collect(),
                                scratch: scratch.map(|(path, _)| path),
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
                        true
                    })
                    .is_ok_and(|current| current)
                {
                    break;
                }
            }
        })
        .detach();
        // Like the other deferred surfaces, focus lands two frames after
        // the card joins the dispatch tree.
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
        let pending = dialog
            .rows
            .into_iter()
            .filter(|row| row.selected)
            .collect::<Vec<_>>();
        let total = pending.len();
        self.reclaim_result = None;
        self.reclaim_batch = Some(ReclaimBatch {
            generation,
            pending: total,
            total,
            ..Default::default()
        });
        // Serialize deletions as well as scans. Resolve the client and safety
        // check just before dispatch: a queued session may have woken up.
        cx.spawn(async move |waku, cx| {
            for row in pending {
                let Ok(Some(client)) = waku.read_with(cx, |waku, _| {
                    if !waku
                        .reclaim_batch
                        .as_ref()
                        .is_some_and(|batch| batch.generation == generation)
                    {
                        return None;
                    }
                    let still_safe = waku.state.sessions.iter().any(|session| {
                        session.id == row.session_id
                            && !session.is_busy()
                            && Some(session.id) != waku.state.selected_session
                            && match (&row.cwd, &session.workspace) {
                                // A worktree row re-verifies the same
                                // checkout still backs the task before its
                                // paths delete; scratch-only rows carry no
                                // `cwd` to recheck.
                                (Some(cwd), SessionWorkspace::Worktree { path, .. }) => {
                                    *path == *cwd
                                }
                                (None, _) => true,
                                (Some(_), _) => false,
                            }
                            // A scratch row also re-verifies no side-chat
                            // descendant woke up writing to the shared
                            // pool since the scan.
                            && (row.scratch.is_none() || !waku.task_family_busy(session.id))
                    });
                    Some(
                        still_safe
                            .then(|| waku.workspace_client_for_session(row.session_id))
                            .flatten(),
                    )
                }) else {
                    break;
                };
                let (reclaimed_bytes, failures) = if let Some(client) = client {
                    cx.background_executor()
                        .spawn(async move {
                            let mut reclaimed_bytes = 0;
                            let mut failures = 0;
                            if let Some(cwd) = row.cwd
                                && !row.paths.is_empty()
                            {
                                match client.request(
                                    waku_client::WorkspaceOperation::ReclaimPaths {
                                        cwd,
                                        paths: row.paths,
                                    },
                                ) {
                                    Ok(waku_client::WorkspaceResult::Reclaim {
                                        reclaimed_bytes: freed,
                                        failures: failed,
                                    }) => {
                                        reclaimed_bytes += freed;
                                        failures += failed.len();
                                    }
                                    // The request itself failed — one
                                    // failure for the batch, nothing freed.
                                    _ => failures += 1,
                                }
                            }
                            if row.scratch.is_some() {
                                match client.request(
                                    waku_client::WorkspaceOperation::ReclaimTaskScratch {
                                        session_id: row.session_id,
                                    },
                                ) {
                                    Ok(waku_client::WorkspaceResult::Reclaim {
                                        reclaimed_bytes: freed,
                                        failures: failed,
                                    }) => {
                                        reclaimed_bytes += freed;
                                        failures += failed.len();
                                    }
                                    _ => failures += 1,
                                }
                            }
                            (reclaimed_bytes, failures)
                        })
                        .await
                } else {
                    // Unsafe or offline tasks still complete their slot.
                    (0, 1)
                };
                if !waku
                    .update(cx, |waku, cx| {
                        let Some(batch) = waku.reclaim_batch.as_mut() else {
                            return false;
                        };
                        if batch.generation != generation {
                            return false;
                        }
                        batch.reclaimed_bytes += reclaimed_bytes;
                        batch.failures += failures;
                        batch.pending -= 1;
                        if batch.pending == 0 {
                            let result = ReclaimResult {
                                reclaimed_bytes: batch.reclaimed_bytes,
                                failures: batch.failures,
                                reveal: true,
                            };
                            waku.reclaim_batch = None;
                            waku.reclaim_result = Some(result);
                        }
                        cx.notify();
                        true
                    })
                    .is_ok_and(|current| current)
                {
                    break;
                }
            }
        })
        .detach();
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The discovery card's cancel — the flow's only abort: the state and
    /// the indicator retire together, and the dropdown closes with them.
    fn close_reclaim_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reclaim_dialog.take().is_none() {
            return;
        }
        if let Some(handle) = self.menus.borrow().get(RECLAIM_PROGRESS_MENU_ID).cloned() {
            handle.close(window, cx);
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The result card's confirm: clears the result, which retires the
    /// header toggle — the popover closes with the state it renders.
    fn acknowledge_reclaim_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reclaim_result = None;
        if let Some(handle) = self.menus.borrow().get(RECLAIM_PROGRESS_MENU_ID).cloned() {
            handle.close(window, cx);
        }
        cx.notify();
    }

    /// The header's reclaim toggle: a discovery card while the user picks
    /// worktrees, a live indicator while a confirmed batch purges, a done
    /// state once it lands — gone once the discovery's cancel aborts or
    /// the user confirms the result card. `None` when no phase of the
    /// flow is in flight.
    pub(super) fn render_reclaim_indicator(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.reclaim_dialog.as_ref();
        let batch = self.reclaim_batch.as_ref();
        let result = self.reclaim_result.as_ref();
        if dialog.is_none() && batch.is_none() && result.is_none() {
            return None;
        }
        let theme = Theme::current(cx);
        let done = result.is_some();
        // The pulse reads as work in flight: a batch purging or a
        // discovery still scanning. Awaiting the user's pick is quiet.
        let working = batch.is_some() || dialog.is_some_and(|dialog| dialog.pending_scans > 0);
        // Neither a command invocation nor a batch completion can open
        // the popover itself — the former has no trigger to anchor to yet
        // and the latter's spawn has no window — so `reveal` is honored
        // from the render path, where defer_in lands after the entity
        // lease releases.
        if dialog.is_some_and(|dialog| dialog.reveal) || result.is_some_and(|result| result.reveal)
        {
            cx.defer_in(window, |this, window, _cx| {
                if let Some(result) = this.reclaim_result.as_mut() {
                    result.reveal = false;
                }
                if let Some(dialog) = this.reclaim_dialog.as_mut() {
                    dialog.reveal = false;
                }
                let Some(handle) = this.menus.borrow().get(RECLAIM_PROGRESS_MENU_ID).cloned()
                else {
                    return;
                };
                // The anchor reads the trigger's painted bounds, so the
                // open waits one frame — a fast phase can land on the
                // indicator's first-ever render.
                window.on_next_frame(move |window, cx| {
                    // Already open means the user is watching; the card
                    // flips to its current phase in place.
                    if !handle.is_open() {
                        crate::ui::menu::toggle_popover(&handle, MenuAlign::BelowRight, window, cx);
                    }
                });
            });
        }

        let handle = self.menu_handle(RECLAIM_PROGRESS_MENU_ID, cx);
        let trigger = div()
            .id("reclaim-progress-trigger")
            .size(px(28.0))
            .relative()
            .rounded(px(9.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .when(handle.is_open(), |style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text(if done {
                tr!("reclaim.finished_tooltip")
            } else if batch.is_some() {
                tr!("reclaim.running")
            } else if working {
                tr!("reclaim.scanning")
            } else {
                tr!("reclaim.title")
            }))
            .child(if done {
                icon("icons/check.svg", 15.0, theme.success)
            } else {
                icon("icons/container.svg", 15.0, theme.text_tertiary)
            })
            .when(!done && working, |trigger| {
                trigger.child(
                    div()
                        .absolute()
                        .top(px(4.0))
                        .right(px(4.0))
                        .child(pulse_dot(5.0, theme.accent)),
                )
            });

        let progress = batch.map(|batch| {
            (
                batch.total - batch.pending,
                batch.total,
                batch.reclaimed_bytes,
            )
        });
        let outcome = result.map(|result| (result.reclaimed_bytes, result.failures));
        // The popover content renders while `Waku` is still leased by
        // this render, so the discovery card reads a by-value snapshot
        // instead of borrowing the entity again.
        let discovery = dialog.map(|dialog| ReclaimDiscovery {
            scanning: dialog.pending_scans > 0,
            selected_bytes: dialog
                .rows
                .iter()
                .filter(|row| row.selected)
                .map(|row| row.bytes)
                .sum(),
            scroll: dialog.scroll.clone(),
            reclaim_focus: dialog.reclaim_focus.clone(),
            cancel_focus: dialog.cancel_focus.clone(),
            rows: dialog
                .rows
                .iter()
                .map(|row| ReclaimDiscoveryRow {
                    session_id: row.session_id,
                    title: row.title.clone(),
                    names: row.names.clone(),
                    bytes: row.bytes,
                    selected: row.selected,
                    focus: row.focus.clone(),
                })
                .collect(),
        });
        let done_focus = self.transcript_control_focus(RECLAIM_RESULT_DONE_FOCUS, cx);
        let weak = cx.entity().downgrade();
        Some(popover(
            trigger,
            &handle,
            MenuAlign::BelowRight,
            move |handle, _, cx| {
                if let Some(discovery) = &discovery {
                    render_reclaim_discovery_card(handle, discovery, weak.clone(), cx)
                } else {
                    render_reclaim_progress_card(
                        handle,
                        progress,
                        outcome,
                        done_focus.clone(),
                        weak.clone(),
                        cx,
                    )
                }
            },
        ))
    }
}

/// A frame's snapshot of the discovery phase, built under the render
/// lease so the popover's content closure never borrows `Waku` itself.
struct ReclaimDiscovery {
    scanning: bool,
    selected_bytes: u64,
    scroll: ScrollHandle,
    reclaim_focus: FocusHandle,
    cancel_focus: FocusHandle,
    rows: Vec<ReclaimDiscoveryRow>,
}

struct ReclaimDiscoveryRow {
    session_id: Uuid,
    title: String,
    names: String,
    bytes: u64,
    selected: bool,
    focus: FocusHandle,
}

/// The dropdown's discovery phase: the scanning state, the selectable
/// worktree rows, and the confirm/cancel pair. Hiding the card leaves
/// the state — and its scans — untouched; only the cancel row aborts.
fn render_reclaim_discovery_card(
    handle: &ContextMenuHandle,
    discovery: &ReclaimDiscovery,
    weak: WeakEntity<Waku>,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::current(cx);
    let body: AnyElement = if discovery.scanning {
        div()
            .h(px(40.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .text_size(sp(12.5))
            .text_color(theme.text_secondary)
            .child(tr!("reclaim.scanning"))
            .into_any_element()
    } else if discovery.rows.is_empty() {
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
            .children(discovery.rows.iter().map(|row| {
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
        &discovery.reclaim_focus,
        "icons/container.svg",
        if discovery.selected_bytes > 0 {
            tr!(
                "reclaim.confirm",
                size = format_reclaim_bytes(discovery.selected_bytes)
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
        &discovery.cancel_focus,
        "icons/x.svg",
        tr!("common.cancel"),
        weak.clone(),
        &theme,
        |waku, window, cx| waku.close_reclaim_dialog(window, cx),
    );

    div()
        .id("reclaim-discovery-card")
        .key_context(DIALOG_CONTEXT)
        .track_focus(handle.focus_handle())
        .on_action(move |_: &ConfirmReclaimDialog, window, cx| {
            let _ = weak.update(cx, |waku, cx| waku.confirm_reclaim_dialog(window, cx));
        })
        .tab_group()
        .tab_stop(false)
        .w(px(DISCOVERY_CARD_WIDTH))
        .rounded(px(15.0))
        .border(hairline())
        .border_color(theme.border_subtle)
        .overflow_hidden()
        .bg(theme.raised)
        .shadow_lg()
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
                .track_scroll(&discovery.scroll)
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
        )
        .into_any_element()
}

/// The header popover's card: request-count progress while a batch runs,
/// the reclaimed total and an explicit confirm once it lands.
fn render_reclaim_progress_card(
    handle: &ContextMenuHandle,
    progress: Option<(usize, usize, u64)>,
    outcome: Option<(u64, usize)>,
    done_focus: FocusHandle,
    weak: WeakEntity<Waku>,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::current(cx);
    let body: AnyElement = if let Some((done, total, freed)) = progress {
        let fraction = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        div()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(sp(12.5))
                    .text_color(theme.text)
                    .child(tr!("reclaim.running")),
            )
            .child(
                div()
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!(
                        "reclaim.progress",
                        done = done,
                        total = total,
                        size = format_reclaim_bytes(freed)
                    )),
            )
            .child(
                div()
                    .h(px(3.0))
                    .w_full()
                    .rounded_full()
                    .bg(theme.overlay_strong)
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction))
                            .rounded_full()
                            .bg(theme.accent),
                    ),
            )
            .into_any_element()
    } else if let Some((reclaimed_bytes, failures)) = outcome {
        div()
            .p(px(8.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .px(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().text_size(sp(12.5)).text_color(theme.text).child(tr!(
                        "reclaim.completed",
                        size = format_reclaim_bytes(reclaimed_bytes)
                    )))
                    .when(failures > 0, |element| {
                        element.child(
                            div()
                                .text_size(sp(11.0))
                                .text_color(theme.text_tertiary)
                                .child(tr!("reclaim.failures", count = failures)),
                        )
                    }),
            )
            .child(div().h(hairline()).bg(theme.separator))
            .child(archive_dialog::render_archive_action_row(
                "reclaim-result-done",
                &done_focus,
                "icons/check.svg",
                tr!("reclaim.done"),
                weak,
                &theme,
                |waku, window, cx| waku.acknowledge_reclaim_result(window, cx),
            ))
            .into_any_element()
    } else {
        div().into_any_element()
    };
    div()
        .id("reclaim-progress-card")
        .track_focus(handle.focus_handle())
        .w(px(280.0))
        .rounded(px(15.0))
        .border(hairline())
        .border_color(theme.border_subtle)
        .overflow_hidden()
        .bg(theme.raised)
        .shadow_lg()
        .child(body)
        .into_any_element()
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
