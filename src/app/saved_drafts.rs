//! Saved drafts and the Drafts page they live on.
//!
//! "Create draft" parks the composer's current payload — text, attachments,
//! and annotations — as a [`SavedDraft`] in app state, separate from the
//! automatic per-composer draft that silently reappears in its slot. The
//! page lists them as cards: click or ⌘Z-undoable Use puts the payload back
//! in its owning composer and consumes the draft, right-click offers Edit
//! (inline), Delete, Hide, and Use. Hidden drafts leave the list and the
//! composer's count badge; the top bar's dropdown is the way back to them.

use gpui::KeyBinding;

use super::*;
use crate::persistence::{ComposerDraft, ComposerDraftKey, ComposerDraftTarget, SavedDraft};

/// Key context the page declares; Escape peels it one layer at a time.
const DRAFTS_PAGE_CONTEXT: &str = "DraftsPage";
/// Cap on kept undo records — each holds a full draft payload.
const DRAFT_USE_UNDO_CAP: usize = 16;

pub fn init(cx: &mut App) {
    cx.bind_keys([
        // ⌘Z outside a field (or inside one whose own undo history is spent)
        // resolves to the same `Undo` action text fields dispatch — the
        // workspace handler below claims it to restore a used draft.
        KeyBinding::new("secondary-z", Undo, Some("Workspace")),
        KeyBinding::new("escape", DismissDraftsLayer, Some(DRAFTS_PAGE_CONTEXT)),
    ]);
}

/// What one "Use" consumed, kept so ⌘Z can hand it all back: the draft and
/// its slot in the list, plus what the target composer's draft slot held
/// before the payload landed.
pub(super) struct DraftUseUndo {
    kind: DraftUndoKind,
    saved: SavedDraft,
    index: usize,
    key: ComposerDraftKey,
    previous: Option<ComposerDraft>,
}

enum DraftUndoKind {
    Use,
    Create,
}

impl Waku {
    // ── Create ─────────────────────────────────────────────────────────────

    /// "Create draft": park the live composer's payload as a saved draft and
    /// empty the composer. The draft records the slot it came from — the
    /// chat, or the project's new-task draft — so Use lands it back there.
    pub(super) fn create_saved_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.composer_draft_key() else {
            self.show_toast(tr!("drafts.no_composer"));
            cx.notify();
            return;
        };
        let draft = self.current_composer_draft(Some(key), cx);
        if draft.is_empty() {
            self.show_toast(tr!("drafts.nothing_to_save"));
            cx.notify();
            return;
        }
        // The card's context label and the fallback landing spot when the
        // owning task no longer exists.
        let project_id = match key {
            ComposerDraftKey::NewSession(project_id) => Some(project_id),
            ComposerDraftKey::Session(session_id) => self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.project_id),
        }
        .or(self.state.selected_project);
        let Some(project_id) = project_id else {
            self.show_toast(tr!("drafts.no_composer"));
            cx.notify();
            return;
        };
        let saved = SavedDraft {
            id: Uuid::new_v4(),
            target: key.into(),
            project_id,
            draft: draft.clone(),
            created_at: unix_time(),
            hidden: false,
        };
        self.state.saved_drafts.insert(0, saved.clone());
        self.draft_use_undos.push(DraftUseUndo {
            kind: DraftUndoKind::Create,
            saved,
            index: 0,
            key,
            previous: Some(draft),
        });
        if self.draft_use_undos.len() > DRAFT_USE_UNDO_CAP {
            self.draft_use_undos.remove(0);
        }
        // Clear the slot before the composer so the debounced autosave can't
        // file the payload back under the composer key on its next pass.
        self.composer_drafts.remove(key);
        self.apply_composer_draft(Some(key), ComposerDraft::default(), cx);
        self.schedule_composer_draft_save(cx);
        self.save();
        self.show_success_toast(tr!("drafts.saved_toast"));
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Drafts the badge counts: visible only — hidden ones stay parked.
    pub(super) fn visible_saved_draft_count(&self) -> usize {
        self.state
            .saved_drafts
            .iter()
            .filter(|draft| !draft.hidden)
            .count()
    }

    /// A removed project takes its parked drafts with it, including any
    /// ⌘Z-undoable Use records that could still hand one back.
    pub(super) fn remove_project_drafts(&mut self, project_id: Uuid) {
        let mut removed = HashSet::new();
        self.state.saved_drafts.retain(|draft| {
            let keep = draft.project_id != project_id;
            if !keep {
                removed.insert(draft.id);
            }
            keep
        });
        self.draft_use_undos
            .retain(|undo| undo.saved.project_id != project_id);
        if self.drafts_editing.is_some_and(|id| removed.contains(&id)) {
            self.drafts_editing = None;
        }
    }

    // ── Page ───────────────────────────────────────────────────────────────

    /// Open the page as a navigation destination; back returns to whatever
    /// the main column showed before.
    pub(super) fn open_drafts_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session_navigation
            .visit(self.navigation_location(), NavigationLocation::DraftsPage);
        self.show_drafts_page(window, cx);
    }

    /// Put the page on screen. Recording the move is the caller's job, same
    /// split as `show_projects_page`: opens `visit`, restores `go_back` /
    /// `go_forward`.
    pub(super) fn show_drafts_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = None;
        self.projects_page = None;
        self.automations_page = false;
        self.automations_detail = None;
        self.notifications.open = false;
        self.selected_terminal = None;
        // An activation still in flight must not hand the area back once
        // its hydration lands — same guard the Projects page takes.
        self.pending_session_activation = None;
        if self
            .sidebar_collapsed_groups
            .insert(SidebarGroup::Terminals)
        {
            self.sidebar_rows_fingerprint.set(None);
        }
        self.drafts_page = true;
        // The page owns its own strip — whatever was mounted (a session's,
        // a terminal's) parks until it comes back.
        self.sync_right_panel_owner(cx);
        let focus = self.drafts_search.read(cx).focus();
        window.focus(&focus, cx);
        cx.notify();
    }

    fn close_drafts_page(&mut self, cx: &mut Context<Self>) {
        if !self.drafts_page {
            return;
        }
        self.drafts_page = false;
        // Closing is a location change too: the surface underneath comes
        // back, and back returns to the page.
        if let Some(location) = self.navigation_location() {
            self.session_navigation
                .visit(Some(NavigationLocation::DraftsPage), location);
        }
        self.sync_right_panel_owner(cx);
        cx.notify();
    }

    /// Escape peels the innermost layer first — an in-progress edit — then
    /// the page itself.
    pub(super) fn dismiss_drafts_layer_action(
        &mut self,
        _: &DismissDraftsLayer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.drafts_page {
            return;
        }
        if self.drafts_editing.is_some() {
            self.drafts_editing = None;
            cx.notify();
            return;
        }
        self.close_drafts_page(cx);
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    // ── Card actions ───────────────────────────────────────────────────────

    fn begin_draft_edit(&mut self, draft_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self
            .state
            .saved_drafts
            .iter()
            .find(|draft| draft.id == draft_id)
            .map(|draft| draft.draft.text.clone())
        else {
            return;
        };
        self.drafts_editing = Some(draft_id);
        self.drafts_edit_input
            .update(cx, |input, cx| input.set_content(text, cx));
        let focus = self.drafts_edit_input.read(cx).focus();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Write the edit field back into the draft's text. Attachments and
    /// annotations are untouched — the field edits the text only.
    fn commit_draft_edit(&mut self, cx: &mut Context<Self>) {
        let Some(draft_id) = self.drafts_editing.take() else {
            return;
        };
        let text = self.drafts_edit_input.read(cx).content().to_owned();
        if let Some(draft) = self
            .state
            .saved_drafts
            .iter_mut()
            .find(|draft| draft.id == draft_id)
        {
            draft.draft.text = text;
        }
        self.save();
        cx.notify();
    }

    fn set_draft_hidden(&mut self, draft_id: Uuid, hidden: bool, cx: &mut Context<Self>) {
        if let Some(draft) = self
            .state
            .saved_drafts
            .iter_mut()
            .find(|draft| draft.id == draft_id)
        {
            draft.hidden = hidden;
        } else {
            return;
        }
        if self.drafts_editing == Some(draft_id) {
            self.drafts_editing = None;
        }
        self.save();
        cx.notify();
    }

    fn delete_saved_draft(&mut self, draft_id: Uuid, cx: &mut Context<Self>) {
        self.state.saved_drafts.retain(|draft| draft.id != draft_id);
        if self.drafts_editing == Some(draft_id) {
            self.drafts_editing = None;
        }
        // A consumed-then-deleted draft has nothing left for ⌘Z to return.
        self.draft_use_undos
            .retain(|undo| undo.saved.id != draft_id);
        self.save();
        cx.notify();
    }

    // ── Use / undo ─────────────────────────────────────────────────────────

    /// Put the draft's payload into its owning composer and consume it: a
    /// started task's slot, or the project's new-task draft — which is also
    /// the fallback when the task it was written in no longer exists.
    fn use_saved_draft(&mut self, draft_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .state
            .saved_drafts
            .iter()
            .position(|draft| draft.id == draft_id)
        else {
            return;
        };
        let saved = &self.state.saved_drafts[index];
        let session = match saved.target {
            ComposerDraftTarget::Session { session_id }
                if self
                    .state
                    .sessions
                    .iter()
                    .any(|session| session.id == session_id) =>
            {
                Some(session_id)
            }
            _ => None,
        };
        if session.is_none()
            && !self
                .state
                .projects
                .iter()
                .any(|project| project.id == saved.project_id)
        {
            self.show_toast(tr!("drafts.missing_target"));
            cx.notify();
            return;
        }
        let key = match session {
            Some(session_id) => ComposerDraftKey::Session(session_id),
            None => ComposerDraftKey::NewSession(saved.project_id),
        };
        let saved = self.state.saved_drafts.remove(index);
        if self.drafts_editing == Some(draft_id) {
            self.drafts_editing = None;
        }
        self.draft_use_undos.push(DraftUseUndo {
            kind: DraftUndoKind::Use,
            previous: self.composer_drafts.get(key).cloned(),
            saved: saved.clone(),
            index,
            key,
        });
        if self.draft_use_undos.len() > DRAFT_USE_UNDO_CAP {
            self.draft_use_undos.remove(0);
        }
        self.composer_drafts.set(key, saved.draft.clone());
        match session {
            Some(session_id) => self.select_session(session_id, cx),
            None => self.select_project(saved.project_id, cx),
        }
        // Activation restores the slot for us when the session changes;
        // already on that composer (or waiting on a hydration) the payload
        // goes in directly.
        if self.composer_draft_key() == Some(key) {
            self.apply_composer_draft(Some(key), saved.draft.clone(), cx);
        }
        self.schedule_composer_draft_save(cx);
        self.save();
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// ⌘Z claimed at the workspace level once the focused field's own undo
    /// history is spent: return the last consumed draft to the list and go
    /// back to where the page was left.
    pub(super) fn undo_draft_use_action(
        &mut self,
        _: &Undo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(undo) = self.draft_use_undos.pop() else {
            return;
        };
        if matches!(undo.kind, DraftUndoKind::Create) {
            // Restore only if the composer slot is still empty; later typing
            // belongs to the user and must not be overwritten by Undo.
            if self
                .composer_drafts
                .get(undo.key)
                .is_none_or(ComposerDraft::is_empty)
            {
                self.state
                    .saved_drafts
                    .retain(|draft| draft.id != undo.saved.id);
                self.composer_drafts.set(undo.key, undo.saved.draft.clone());
                if self.composer_draft_key() == Some(undo.key) {
                    self.apply_composer_draft(Some(undo.key), undo.saved.draft, cx);
                    let focus = self.composer_focus(cx);
                    window.focus(&focus, cx);
                }
            } else {
                // The slot has newer content, so leave the saved copy intact.
                let index = undo.index.min(self.state.saved_drafts.len());
                self.state.saved_drafts.insert(index, undo.saved);
            }
            self.schedule_composer_draft_save(cx);
            self.save();
            cx.notify();
            return;
        }
        let index = undo.index.min(self.state.saved_drafts.len());
        let applied = undo.saved.draft.clone();
        self.state.saved_drafts.insert(index, undo.saved);
        // Hand the slot back only while it still holds exactly what Use put
        // there — text the user has typed since is theirs, not ours to move.
        if self.composer_drafts.get(undo.key) == Some(&applied) {
            match undo.previous {
                Some(previous) => {
                    self.composer_drafts.set(undo.key, previous);
                }
                None => {
                    self.composer_drafts.remove(undo.key);
                }
            }
            if self.composer_draft_key() == Some(undo.key) {
                self.restore_selected_composer_draft(cx);
            }
        }
        self.schedule_composer_draft_save(cx);
        self.save();
        self.session_navigation
            .visit(self.navigation_location(), NavigationLocation::DraftsPage);
        self.show_drafts_page(window, cx);
    }

    // ── Filtering ──────────────────────────────────────────────────────────

    /// The card's context line: the task the draft belongs to, or the
    /// project whose new-task composer it was written in.
    fn draft_context_label(&self, saved: &SavedDraft) -> String {
        let project_name = || {
            self.state
                .projects
                .iter()
                .find(|project| project.id == saved.project_id)
                .map(|project| project.display_name())
        };
        match saved.target {
            ComposerDraftTarget::Session { session_id } => self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.title.clone())
                .or_else(project_name)
                .unwrap_or_else(|| tr!("drafts.missing_task")),
            ComposerDraftTarget::NewSession { .. } => project_name()
                .map(|name| tr!("drafts.new_task_in", project = name))
                .unwrap_or_else(|| tr!("drafts.missing_project")),
        }
    }

    fn draft_matches(&self, saved: &SavedDraft, query: &str) -> bool {
        saved.draft.text.to_lowercase().contains(query)
            || saved
                .draft
                .attachments
                .iter()
                .any(|attachment| attachment.name.to_lowercase().contains(query))
            || self
                .draft_context_label(saved)
                .to_lowercase()
                .contains(query)
    }

    /// Keep the virtualized list in sync with the freshly filtered rows,
    /// sharing the prefix so scroll position survives edits and keystrokes.
    fn sync_drafts_rows(&self, rows: &[Uuid]) {
        let mut cached = self.drafts_rows.borrow_mut();
        if cached.as_slice() == rows {
            return;
        }
        let prefix = cached
            .iter()
            .zip(rows.iter())
            .take_while(|(cached, fresh)| cached == fresh)
            .count();
        let old_count = cached.len();
        *cached = rows.to_vec();
        if old_count == 0 {
            self.drafts_list_state.reset(rows.len());
        } else {
            self.drafts_list_state
                .splice(prefix..old_count, rows.len() - prefix);
        }
    }

    // ── Render ─────────────────────────────────────────────────────────────

    /// The circular composer badge beside the access control: the visible
    /// draft count, absent entirely at zero.
    pub(super) fn render_drafts_count_button(
        &self,
        controls: &composer::ComposerControls,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let count = self.visible_saved_draft_count();
        if count == 0 {
            return None;
        }
        let theme = Theme::current(cx);
        let focus = self.transcript_control_focus(controls.chip_id("drafts-count").to_string(), cx);
        Some(
            div()
                .id(controls.chip_id("drafts-count"))
                .track_focus(&focus)
                .tab_index(0)
                .tab_stop(true)
                .h(px(18.0))
                .flex_none()
                .when(count >= 10, |element| element.px_1())
                .when(count < 10, |element| element.w(px(18.0)))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .bg(theme.overlay_strong)
                .hover(|element| element.bg(theme.overlay))
                .active(|element| element.opacity(0.8))
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .child(
                    div()
                        .text_size(sp(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_secondary)
                        .child(count.to_string()),
                )
                .tooltip(Tooltip::text(tr!("drafts.view_tooltip")))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_drafts_page(window, cx);
                }))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.open_drafts_page(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .into_any_element(),
        )
    }

    pub(super) fn render_drafts_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let query = self.drafts_search.read(cx).content().trim().to_lowercase();
        let show_hidden = self.drafts_show_hidden;
        let rows: Vec<Uuid> = self
            .state
            .saved_drafts
            .iter()
            .filter(|draft| draft.hidden == show_hidden)
            .filter(|draft| query.is_empty() || self.draft_matches(draft, &query))
            .map(|draft| draft.id)
            .collect();
        self.sync_drafts_rows(&rows);

        let body: AnyElement = if rows.is_empty() {
            let (title, hint) = if !query.is_empty() {
                (tr!("drafts.no_match"), tr!("drafts.no_match_hint"))
            } else if show_hidden {
                (tr!("drafts.empty_hidden"), tr!("drafts.empty_hidden_hint"))
            } else {
                (tr!("drafts.empty_title"), tr!("drafts.empty_hint"))
            };
            drafts_status_row(&theme, title, hint).into_any_element()
        } else {
            let entity = cx.entity().downgrade();
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    list(self.drafts_list_state.clone(), move |index, _window, cx| {
                        entity
                            .upgrade()
                            .map(|entity| entity.update(cx, |this, cx| this.drafts_row(index, cx)))
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .size_full(),
                )
                .child(scrollbar::vertical(
                    &self.drafts_list_state,
                    &self.drafts_scrollbar,
                ))
                .into_any_element()
        };

        div()
            .key_context(DRAFTS_PAGE_CONTEXT)
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::dismiss_drafts_layer_action))
            .child(self.render_drafts_header(&theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH + 48.0))
                    .mx_auto()
                    .px(px(24.0))
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .into_any_element()
    }

    /// The page's top bar: title and count on the left, the shown/hidden
    /// switch and the search field on the right.
    fn render_drafts_header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let show_hidden = self.drafts_show_hidden;
        let hidden_count = self
            .state
            .saved_drafts
            .iter()
            .filter(|draft| draft.hidden)
            .count();
        let handle = self.menu_handle("drafts-view-filter", cx);
        let view_switch = dropdown_menu(
            MenuChip::new("drafts-view-filter")
                .icon(
                    if show_hidden {
                        "icons/eye-off.svg"
                    } else {
                        "icons/compose.svg"
                    },
                    theme.text_tertiary,
                )
                .label(if show_hidden {
                    tr!("drafts.view_hidden")
                } else {
                    tr!("drafts.view_drafts")
                })
                .outlined()
                .background(theme.raised)
                .selected(handle.is_open()),
            "drafts-view-filter-menu",
            &handle,
            MenuAlign::BelowRight,
            move |_| {
                let mut items = Vec::new();
                {
                    let weak = weak.clone();
                    items.push(
                        MenuItem::new(tr!("drafts.view_drafts"), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.drafts_show_hidden = false;
                                cx.notify();
                            });
                        })
                        .icon("icons/compose.svg")
                        .selected(!show_hidden),
                    );
                }
                let label = if hidden_count > 0 {
                    format!("{} · {hidden_count}", tr!("drafts.view_hidden"))
                } else {
                    tr!("drafts.view_hidden")
                };
                let weak = weak.clone();
                items.push(
                    MenuItem::new(label, move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.drafts_show_hidden = true;
                            cx.notify();
                        });
                    })
                    .icon("icons/eye-off.svg")
                    .selected(show_hidden),
                );
                items
            },
        );

        div()
            .flex_none()
            .w_full()
            .px(px(24.0))
            .pt(px(16.0))
            .pb(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .text_size(sp(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .child(tr!("drafts.title")),
            )
            .child(div().flex_1())
            .child(view_switch)
            .child(
                div().w(px(220.0)).flex_none().child(
                    TextField::new("drafts-search-field", self.drafts_search.clone())
                        .icon("icons/search.svg", 13.0)
                        .w_full(),
                ),
            )
            .into_any_element()
    }

    /// One card, built only while visible. Reads the per-frame id cache; a
    /// stale index from a frame racing a mutation renders empty rather than
    /// panicking.
    fn drafts_row(&mut self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let rows = self.drafts_rows.borrow();
        let Some(draft_id) = rows.get(row).copied() else {
            return div().into_any_element();
        };
        drop(rows);
        let Some(saved) = self
            .state
            .saved_drafts
            .iter()
            .find(|draft| draft.id == draft_id)
            .cloned()
        else {
            return div().into_any_element();
        };
        let editing = self.drafts_editing == Some(draft_id);
        let context = self.draft_context_label(&saved);
        let age = format_time_ago(unix_time().saturating_sub(saved.created_at));
        let waku = cx.entity().downgrade();
        let menu = self.menu_handle(format!("draft-{draft_id}"), cx);
        let row_focus = menu.trigger_focus_handle().clone();
        let keyboard_menu = menu.clone();

        let body: AnyElement = if editing {
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .w_full()
                        .rounded(px(8.0))
                        .border(hairline())
                        .border_color(theme.accent)
                        .bg(theme.inset)
                        .px(px(8.0))
                        .py(px(6.0))
                        .child(self.drafts_edit_input.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(6.0))
                        .child(self.draft_edit_button(
                            "draft-edit-cancel",
                            tr!("drafts.edit_cancel"),
                            false,
                            &theme,
                            cx,
                        ))
                        .child(self.draft_edit_button(
                            "draft-edit-save",
                            tr!("drafts.edit_save"),
                            true,
                            &theme,
                            cx,
                        )),
                )
                .into_any_element()
        } else {
            let text = saved.draft.text.trim();
            let attachments = saved.draft.attachments.len();
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .when(!text.is_empty(), |element| {
                    element.child(
                        div()
                            .w_full()
                            .text_size(sp(12.5))
                            .line_height(sp(17.0))
                            .text_color(theme.text)
                            .child(SharedString::from(truncate_draft_text(text))),
                    )
                })
                .when(attachments > 0, |element| {
                    element.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .child(icon("icons/paperclip.svg", 11.0, theme.text_tertiary))
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(tr!("drafts.attachments", count = attachments)),
                            ),
                    )
                })
                .into_any_element()
        };

        let card = div()
            .id(SharedString::from(format!("draft-card-{draft_id}")))
            .w_full()
            .mb(px(8.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(10.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.raised)
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(context)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(11.5))
                            .text_color(theme.text_ghost)
                            .child(age),
                    ),
            )
            .child(body);

        let card = if editing {
            // Clicking away commits the edit, like the sidebar's rename.
            card.on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                if this.drafts_editing == Some(draft_id) {
                    this.commit_draft_edit(cx);
                }
            }))
            .into_any_element()
        } else {
            card.track_focus(&row_focus)
                .tab_index(0)
                .tab_stop(true)
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if matches!(key, "enter" | "space") {
                        this.use_saved_draft(draft_id, window, cx);
                        cx.stop_propagation();
                    } else if key == "f10" && event.keystroke.modifiers.shift {
                        keyboard_menu.open_context_menu(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.use_saved_draft(draft_id, window, cx);
                }))
                .into_any_element()
        };

        context_menu(
            div().w_full().child(card),
            SharedString::from(format!("draft-menu-{draft_id}")),
            &menu,
            move |cx| {
                let Some(saved) = waku
                    .update(cx, |this, _| {
                        this.state
                            .saved_drafts
                            .iter()
                            .find(|draft| draft.id == draft_id)
                            .cloned()
                    })
                    .ok()
                    .flatten()
                else {
                    return Vec::new();
                };
                let edit_waku = waku.clone();
                let hide_waku = waku.clone();
                let use_waku = waku.clone();
                let delete_waku = waku.clone();
                let hidden = saved.hidden;
                vec![
                    MenuItem::new(tr!("drafts.menu.edit"), move |window, cx| {
                        let _ = edit_waku.update(cx, |this, cx| {
                            this.begin_draft_edit(draft_id, window, cx);
                        });
                    })
                    .icon("icons/pencil.svg"),
                    MenuItem::new(
                        if hidden {
                            tr!("drafts.menu.unhide")
                        } else {
                            tr!("drafts.menu.hide")
                        },
                        move |_, cx| {
                            let _ = hide_waku.update(cx, |this, cx| {
                                this.set_draft_hidden(draft_id, !hidden, cx);
                            });
                        },
                    )
                    .icon(if hidden {
                        "icons/eye.svg"
                    } else {
                        "icons/eye-off.svg"
                    }),
                    MenuItem::new(tr!("drafts.menu.use"), move |window, cx| {
                        let _ = use_waku.update(cx, |this, cx| {
                            this.use_saved_draft(draft_id, window, cx);
                        });
                    })
                    .icon("icons/compose.svg"),
                    MenuItem::Separator,
                    MenuItem::new(tr!("drafts.menu.delete"), move |_, cx| {
                        let _ = delete_waku.update(cx, |this, cx| {
                            this.delete_saved_draft(draft_id, cx);
                        });
                    })
                    .icon("icons/trash.svg"),
                ]
            },
        )
    }

    fn draft_edit_button(
        &self,
        id: &'static str,
        label: String,
        primary: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let button = div()
            .id(id)
            .h(px(24.0))
            .px(px(10.0))
            .rounded(px(7.0))
            .flex()
            .items_center()
            .cursor_default()
            .text_size(sp(12.0))
            .font_weight(FontWeight::MEDIUM)
            .child(label);
        if primary {
            button
                .bg(theme.inverse)
                .text_color(theme.on_inverse)
                .hover(|element| element.opacity(0.9))
                .active(|element| element.opacity(0.8))
                .on_click(cx.listener(|this, _, _, cx| this.commit_draft_edit(cx)))
        } else {
            button
                .text_color(theme.text_secondary)
                .hover(|element| element.bg(theme.overlay))
                .active(|element| element.bg(theme.overlay_strong))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.drafts_editing = None;
                    cx.notify();
                }))
        }
    }
}

/// One screen's worth of preview: the first lines, whitespace flattened, so
/// a card reads like a mail list row rather than a document.
fn truncate_draft_text(text: &str) -> String {
    const MAX: usize = 240;
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.len() <= MAX {
        return flattened;
    }
    let mut end = MAX;
    while end > 0 && !flattened.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &flattened[..end])
}

fn drafts_status_row(theme: &Theme, title: String, hint: String) -> Div {
    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .child(icon("icons/compose.svg", 16.0, theme.text_tertiary))
        .child(
            div()
                .text_size(sp(13.0))
                .text_color(theme.text_secondary)
                .child(title),
        )
        .child(
            div()
                .text_size(sp(12.0))
                .text_color(theme.text_tertiary)
                .child(hint),
        )
}
