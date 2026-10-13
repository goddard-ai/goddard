//! Read-only, demand-loaded predecessors of the live Boss chat.
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;

#[derive(Default)]
pub(super) struct BossChatHistory {
    pub sessions: HashMap<Uuid, BossHistorySession>,
    pub loading: HashSet<Uuid>,
    pub errors: HashMap<Uuid, String>,
    pub revision: u64,
}

pub(super) struct BossHistorySession {
    pub session: AgentSession,
    pub expanded_blocks: HashMap<usize, bool>,
    predecessor: Option<Uuid>,
    pub reference_contexts: HashMap<Uuid, crate::model::ReferenceContext>,
    rows: RefCell<Option<BossHistoryRows>>,
    footers: HashMap<usize, (Option<SharedString>, Option<u64>)>,
}

struct BossHistoryRows {
    signature: u64,
    kinds: Arc<Vec<TranscriptRowKind>>,
    indexes: HashMap<TranscriptRowKind, usize>,
    navigation: Arc<Vec<TranscriptNavigationTurn>>,
}

impl BossHistorySession {
    fn new(session: AgentSession) -> Self {
        let footers = session
            .messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                (message.role == MessageRole::Assistant).then(|| {
                    (
                        index,
                        (
                            assistant_response_footer(&session, index).map(SharedString::from),
                            assistant_response_footer_time(&session, index),
                        ),
                    )
                })
            })
            .collect();
        let predecessor = boss_chat_predecessor(&session);
        let mut seen = HashSet::new();
        let mut reference_contexts = HashMap::new();
        for prompt in session
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::User)
        {
            if let Some(turn) = prompt.turn_id
                && seen.insert(turn)
            {
                if let Some(context) = prompt
                    .reference_context
                    .as_ref()
                    .or_else(|| prompt.report_trigger.as_ref()?.reference_context.as_ref())
                {
                    reference_contexts.insert(turn, context.clone());
                }
            }
        }
        Self {
            session,
            predecessor,
            reference_contexts,
            expanded_blocks: HashMap::new(),
            rows: RefCell::new(None),
            footers,
        }
    }

    pub fn rows(&self, expanded: &HashSet<Uuid>) -> Arc<Vec<TranscriptRowKind>> {
        // Archived content is immutable. Only disclosures can change its rows;
        // streaming the live session must not rescan all historical messages.
        let signature = expanded
            .iter()
            .fold(0u64, |hash, id| hash.wrapping_add(mix_uuid(0, *id)));
        let mut cached = self.rows.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|rows| rows.signature != signature)
        {
            let kinds = Arc::new(folded_transcript_row_kinds(&self.session, expanded, None));
            let indexes = kinds
                .iter()
                .copied()
                .enumerate()
                .map(|(index, kind)| (kind, index))
                .collect();
            let navigation = Arc::new(transcript_navigation_turns(&self.session, &kinds));
            *cached = Some(BossHistoryRows {
                signature,
                kinds,
                indexes,
                navigation,
            });
        }
        cached.as_ref().expect("rows initialized").kinds.clone()
    }

    pub fn cached_rows(&self) -> Arc<Vec<TranscriptRowKind>> {
        self.rows
            .borrow()
            .as_ref()
            .expect("history folded before rendering")
            .kinds
            .clone()
    }

    pub fn content_index(&self, row: BossHistoryRow) -> Option<usize> {
        self.rows
            .borrow()
            .as_ref()?
            .indexes
            .get(&row.content())
            .copied()
    }

    pub fn navigation(&self) -> Arc<Vec<TranscriptNavigationTurn>> {
        self.rows
            .borrow()
            .as_ref()
            .expect("history folded before navigation")
            .navigation
            .clone()
    }
}

/// The page a scroll to the transcript's top should fetch: the boundary has
/// to be the first row with the scroll anchor parked inside it. A
/// tail-following list reports row `count` rather than a real position, so
/// `is_scrolled` keeps resting at the tail from qualifying.
fn top_history_boundary(
    kinds: &[TranscriptRowKind],
    is_scrolled: bool,
    scroll_top: ListOffset,
) -> Option<Uuid> {
    if !is_scrolled || scroll_top.item_ix != 0 {
        return None;
    }
    match kinds.first() {
        Some(&TranscriptRowKind::BossHistoryBoundary(id)) => Some(id),
        _ => None,
    }
}

/// The daemon writes this marker into both sessions. Only the destination's
/// marker is a predecessor link; the source's outgoing marker must be ignored.
fn boss_chat_predecessor(session: &AgentSession) -> Option<Uuid> {
    session.messages.iter().find_map(|message| {
        if message.role != MessageRole::System {
            return None;
        }
        let old = if message.hidden {
            message
                .content
                .strip_prefix("<boss-rotation-link:")?
                .strip_suffix('>')?
        } else {
            // Read existing transcripts written before rotation links became
            // hidden metadata.
            let (_, ids) = message.content.split_once(". Old session: ")?;
            let (old, rest) = ids.split_once(". New session: ")?;
            let (new, _) = rest.split_once('.')?;
            if Uuid::parse_str(new).ok()? != session.id {
                return None;
            }
            old
        };
        let old = Uuid::parse_str(old).ok()?;
        (old != session.id).then_some(old)
    })
}

/// Preserve the same content row and its intra-row offset as the live rows
/// acquire a historical owner and predecessor pages are inserted above them.
fn refresh_scroll_offset(
    previous: &[TranscriptRowKind],
    next: &[TranscriptRowKind],
    scroll: ListOffset,
) -> ListOffset {
    let item_ix = previous
        .get(scroll.item_ix)
        .and_then(|row| next.iter().position(|candidate| candidate == row))
        .unwrap_or(scroll.item_ix.min(next.len().saturating_sub(1)));
    ListOffset {
        item_ix,
        offset_in_item: scroll.offset_in_item,
    }
}

impl BossChatHistory {
    /// Newest first. Stop at the first unloaded predecessor, never skip a gap.
    fn chain(&self, live: &AgentSession) -> (Vec<Uuid>, Option<Uuid>) {
        let mut loaded = Vec::new();
        let mut seen = HashSet::from([live.id]);
        let mut next = boss_chat_predecessor(live);
        while let Some(id) = next {
            if !seen.insert(id) {
                return (loaded, None);
            }
            let Some(entry) = self.sessions.get(&id) else {
                return (loaded, Some(id));
            };
            loaded.push(id);
            next = entry.predecessor;
        }
        (loaded, None)
    }
    /// Commit a hydrated successor without a visit. The mounted transcript
    /// becomes history before selection changes, so there is never an empty
    /// conversation between the two generations.
    fn swap_live_session(
        &mut self,
        sessions: &[AgentSession],
        selected: &mut Option<Uuid>,
        successor: Uuid,
    ) -> bool {
        let Some(old) = sessions.iter().find(|entry| Some(entry.id) == *selected) else {
            return false;
        };
        let Some(next) = sessions.iter().find(|entry| entry.id == successor) else {
            return false;
        };
        let (loaded, boundary) = self.chain(next);
        if old.id == next.id
            || !old.detail_loaded
            || !next.detail_loaded
            || !old.boss_managed
            || !next.boss_managed
            || old.planning.is_some()
            || next.planning.is_some()
            || old.project_id != next.project_id
            || next.archived_at.is_some()
            || !(loaded.contains(&old.id) || boundary == Some(old.id))
        {
            return false;
        }
        self.finish_load(
            old.id,
            Some(old.project_id),
            Ok(BossHistorySession::new(old.clone())),
        );
        *selected = Some(next.id);
        true
    }

    fn begin_load(&mut self, id: Uuid) -> bool {
        if self.sessions.contains_key(&id) || !self.loading.insert(id) {
            return false;
        }
        self.errors.remove(&id);
        true
    }

    fn finish_load(
        &mut self,
        id: Uuid,
        identity: Option<Uuid>,
        result: anyhow::Result<BossHistorySession>,
    ) {
        self.loading.remove(&id);
        match result {
            Ok(entry) if Some(entry.session.project_id) == identity => {
                self.sessions.insert(id, entry);
                self.revision = self.revision.wrapping_add(1);
            }
            Ok(_) => {
                self.errors
                    .insert(id, "The conversation belongs to another Boss.".into());
            }
            Err(error) => {
                self.errors.insert(id, error.to_string());
            }
        }
    }

    pub(super) fn transcript_rows(
        &self,
        live: &AgentSession,
        expanded: &HashSet<Uuid>,
        pending: Option<Uuid>,
    ) -> Vec<TranscriptRowKind> {
        let (history, boundary) = self.chain(live);
        let mut rows = Vec::new();
        if let Some(id) = boundary {
            rows.push(TranscriptRowKind::BossHistoryBoundary(id));
        }
        for id in history.into_iter().rev() {
            let entry = &self.sessions[&id];
            rows.extend(entry.rows(expanded).iter().filter_map(|row| {
                Some(TranscriptRowKind::BossHistory(
                    id,
                    BossHistoryRow::from_content(*row)?,
                ))
            }));
        }
        rows.extend(folded_transcript_row_kinds(live, expanded, pending));
        rows
    }

    pub(super) fn navigation_turns(
        &self,
        live: &AgentSession,
        kinds: &[TranscriptRowKind],
    ) -> Vec<TranscriptNavigationTurn> {
        let (history, _) = self.chain(live);
        // One pass, rather than searching the entire flattened document once
        // for each historical session.
        let mut offsets = HashMap::new();
        for (index, kind) in kinds.iter().enumerate() {
            if let TranscriptRowKind::BossHistory(id, _) = kind {
                offsets.entry(*id).or_insert(index);
            }
        }
        let mut turns = Vec::new();
        for id in history.into_iter().rev() {
            let Some(offset) = offsets.get(&id) else {
                continue;
            };
            turns.extend(self.sessions[&id].navigation().iter().map(|turn| {
                TranscriptNavigationTurn {
                    message_id: turn.message_id,
                    message_index: turn.message_index,
                    row_index: turn.row_index + offset,
                    prompt: turn.prompt.clone(),
                    response: turn.response.clone(),
                }
            }));
        }
        turns.extend(transcript_navigation_turns(live, kinds));
        turns
    }
}

impl Waku {
    pub(super) fn boss_history_session(&self, id: Uuid) -> Option<&BossHistorySession> {
        self.boss_ui
            .chat_history
            .values()
            .find_map(|history| history.sessions.get(&id))
    }

    pub(super) fn transcript_session_for_turn(&self, turn_id: Uuid) -> Option<&AgentSession> {
        self.selected_session()
            .filter(|session| session.turns.iter().any(|turn| turn.id == turn_id))
            .or_else(|| {
                let key = self.boss_chat_key()?;
                self.boss_ui
                    .chat_history
                    .get(&key)?
                    .sessions
                    .values()
                    .map(|entry| &entry.session)
                    .find(|session| session.turns.iter().any(|turn| turn.id == turn_id))
            })
    }

    pub(super) fn transcript_display_session(&self) -> Option<&AgentSession> {
        self.boss_ui
            .history_render_session
            .get()
            .and_then(|id| self.boss_history_session(id).map(|entry| &entry.session))
            .or_else(|| self.selected_session())
    }

    pub(super) fn boss_history_chain(&self) -> (Vec<Uuid>, Option<Uuid>) {
        let Some(key) = self.boss_chat_key() else {
            return (Vec::new(), None);
        };
        let Some(live) = self.selected_session() else {
            return (Vec::new(), None);
        };
        self.boss_ui.chat_history.get(&key).map_or_else(
            || (Vec::new(), boss_chat_predecessor(live)),
            |history| history.chain(live),
        )
    }

    /// A refresh changes the backing session, never the current surface.
    /// Recheck on both document/catalog arrivals and hydration completion.
    pub(super) fn reconcile_boss_chat_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(from) = self.state.selected_session else {
            return;
        };
        let Some(to) = self.live_boss_chat_target(from) else {
            return;
        };
        let Some(key) = self.boss_chat_key() else {
            return;
        };
        let Some(next) = self.state.sessions.iter().find(|entry| entry.id == to) else {
            return;
        };
        if !next.detail_loaded {
            self.ensure_session_loaded(to, cx);
            return;
        }
        // Several swaps can overtake hydration. Fill any intervening link
        // before moving the backing id, retaining the mounted old transcript.
        let history = self.boss_ui.chat_history.entry(key).or_default();
        let (loaded, boundary) = history.chain(next);
        if !loaded.contains(&from) && boundary != Some(from) {
            if let Some(id) = boundary.filter(|id| !history.errors.contains_key(id)) {
                self.load_boss_history(key, id, cx);
            }
            return;
        }
        // Catalog delivery may precede the Boss pointer push. Carry the
        // id-keyed draft state now as well; a later push is idempotent.
        if self.boss_ui.rotated_chat_drafts.get(&from) != Some(&to) {
            self.migrate_voice_scratchpad(from, to);
            self.migrate_boss_composer_draft(from, to, cx);
            self.migrate_boss_queue_drain(from, to);
        }
        // Resolve GPUI's implicit bottom anchor into the actual viewport
        // position before appending the successor; otherwise a tail anchor
        // would jump to the new generation rather than stay on the old text.
        self.pin_transcript_for_disclosure();
        let previous = self.transcript_row_kinds.borrow().clone();
        let scroll = self.active_transcript_rows().logical_scroll_top();
        let previous: Vec<_> = previous
            .into_iter()
            .map(|row| {
                BossHistoryRow::from_content(row)
                    .map_or(row, |content| TranscriptRowKind::BossHistory(from, content))
            })
            .collect();
        let history = self.boss_ui.chat_history.entry(key).or_default();
        if !history.swap_live_session(&self.state.sessions, &mut self.state.selected_session, to) {
            return;
        }
        if let Some(retired) = history.sessions.get_mut(&from) {
            retired.expanded_blocks = self.activities_expanded.clone();
        }
        // Block indexes are local to a session. Keep the mounted disclosure
        // state on its historical owner, rather than applying it to the
        // successor's unrelated blocks.
        self.activities_expanded.clear();
        // No activate_session: it resets pages, composer, focus, expansion,
        // navigation and landing. Only splice transcript data and keep its
        // exact logical anchor, including the pixel offset within the row.
        self.splice_transcript_rows_after_visibility_change(&previous);
        let next = self.transcript_row_kinds.borrow();
        let scroll = refresh_scroll_offset(&previous, &next, scroll);
        drop(next);
        self.active_transcript_rows().scroll_to(scroll);
        if let Some(mut position) = self.transcript_scroll_positions.remove(&from) {
            position.offset = scroll;
            self.transcript_scroll_positions.insert(to, position);
        }
        self.ensure_recent_boss_history(cx);
        self.sync_composer_placeholder(cx);
        cx.notify();
    }

    /// Called on activation, not from a row builder. Only the immediate
    /// predecessor is eager; subsequent links require the history control.
    pub(super) fn ensure_recent_boss_history(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.boss_chat_key() else {
            return;
        };
        let predecessor = self.selected_session().and_then(boss_chat_predecessor);
        if let Some(id) = predecessor {
            self.load_boss_history(key, id, cx);
        }
    }

    /// Fetch the pending page when the reader's scroll reaches the boundary
    /// row. Called from `render_transcript`, so it has to stay cheap on every
    /// other frame and fire once per arrival: a finished load moves the scroll
    /// anchor below the boundary's replacement, an in-flight or failed load
    /// leaves the row's loading/retry state to its own control, and a missing
    /// supervisor skips the request rather than re-arming its toast every
    /// frame.
    pub(super) fn load_boss_history_at_top(&mut self, cx: &mut Context<Self>) {
        let Some(id) = top_history_boundary(
            &self.transcript_row_kinds.borrow(),
            self.transcript_is_scrolled.get(),
            self.active_transcript_rows().logical_scroll_top(),
        ) else {
            return;
        };
        let Some(key) = self.boss_chat_key() else {
            return;
        };
        let loadable = self.boss_ui.chat_history.get(&key).is_none_or(|history| {
            !history.loading.contains(&id) && !history.errors.contains_key(&id)
        });
        if !loadable || self.daemons.supervisor(key).is_none() {
            return;
        }
        self.load_boss_history(key, id, cx);
    }

    pub(super) fn load_boss_history(&mut self, key: DaemonKey, id: Uuid, cx: &mut Context<Self>) {
        let Some(client) = self.daemons.supervisor(key) else {
            self.show_toast(tr!("boss.unreachable"));
            return;
        };
        let history = self.boss_ui.chat_history.entry(key).or_default();
        if !history.begin_load(id) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let session = waku_client::persistence::hydrate_session(&client, id)?
                        .ok_or_else(|| {
                            anyhow::anyhow!("the previous Boss conversation no longer exists")
                        })?;
                    anyhow::ensure!(
                        session.id == id
                            && session.boss_managed
                            && session.planning.is_none(),
                        "not a retired Boss conversation"
                    );
                    Ok::<_, anyhow::Error>(BossHistorySession::new(session))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // A request may finish after selection moved. Cache it for its
                // owner, but never splice another chat's list or move its scroll.
                let selected = this.boss_chat_key() == Some(key);
                let before = selected.then(|| {
                    let following = !this.transcript_is_scrolled.get();
                    if !following { this.pin_transcript_for_disclosure(); }
                    (
                        this.active_transcript_rows().logical_scroll_top(),
                        this.transcript_row_kinds.borrow().clone(),
                        following,
                    )
                });
                let identity = this.boss_ui.states.get(&key).map(|state| state.identity.id);
                let history = this.boss_ui.chat_history.entry(key).or_default();
                history.finish_load(id, identity, result);
                this.daemons.claim_session(id, key);
                if let Some((scroll, previous, following)) = before {
                    this.splice_transcript_rows_after_visibility_change(&previous);
                    // Splicing preserves the logical position. If the reader
                    // activated the replaced boundary, land on its replacement.
                    if following {
                        this.pin_transcript_to_tail();
                    } else if matches!(previous.get(scroll.item_ix), Some(TranscriptRowKind::BossHistoryBoundary(_))) {
                        let first_loaded = this.transcript_row_kinds.borrow().iter().position(|row|
                            matches!(row, TranscriptRowKind::BossHistory(owner, _) if *owner == id));
                        this.active_transcript_rows().scroll_to(ListOffset {
                            item_ix: first_loaded.unwrap_or(scroll.item_ix),
                            offset_in_item: scroll.offset_in_item,
                        });
                    }
                    if let Some(boundary) = this.transcript_row_kinds.borrow().iter().position(|row|
                        *row == TranscriptRowKind::BossHistoryBoundary(id)) {
                        this.remeasure_transcript_rows(boundary..boundary + 1);
                    }
                }
                this.reconcile_boss_chat_refresh(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn historical_footer(
        &self,
        index: usize,
    ) -> Option<(Option<SharedString>, Option<u64>)> {
        let id = self.boss_ui.history_render_session.get()?;
        Some(
            self.boss_history_session(id)?
                .footers
                .get(&index)
                .cloned()
                .unwrap_or((None, None)),
        )
    }

    pub(super) fn render_boss_history_boundary(
        &self,
        id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(key) = self.boss_chat_key() else {
            return div().into_any_element();
        };
        let history = self.boss_ui.chat_history.get(&key);
        let loading = history.is_some_and(|history| history.loading.contains(&id));
        let error = history.and_then(|history| history.errors.get(&id));
        let label = if loading {
            tr!("boss.history_loading")
        } else if error.is_some() {
            tr!("boss.history_retry")
        } else {
            tr!("boss.history_load")
        };
        let theme = Theme::current(cx);
        let focus = self.transcript_control_focus(format!("boss-history-{id}"), cx);
        div()
            .w_full()
            .py(px(12.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .child(
                div()
                    .id(SharedString::from(format!("boss-history-{id}")))
                    .track_focus(&focus)
                    .tab_index(0)
                    .rounded(px(6.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_secondary)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .hover(|style| style.bg(theme.overlay))
                    .child(label)
                    .on_activation(cx, move |this, _, cx| {
                        if !loading {
                            this.load_boss_history(key, id, cx);
                        }
                    }),
            )
            .children(error.map(|error| {
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(error.clone())
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(project: Uuid, prompt: &str) -> AgentSession {
        let mut session = AgentSession::new(project, ProviderKind::Codex);
        session.boss_managed = true;
        session.begin_turn(prompt);
        session.push_message(MessageRole::Assistant, format!("Answer to {prompt}"));
        session.finish_active_turn(TurnStatus::Completed);
        session
    }

    fn rotate(old: &mut AgentSession, next: &mut AgentSession) {
        let marker = "Boss session refreshed; active work preserved.";
        old.push_message(MessageRole::System, marker);
        let mut link = Message::new(
            MessageRole::System,
            format!("<boss-rotation-link:{}>", old.id),
        );
        link.hidden = true;
        old.messages.push(link);
        old.archived_at = Some(10);
        // The destination marker precedes its first turn in the daemon.
        let mut marker_session = AgentSession::new(next.project_id, next.provider);
        marker_session.push_message(MessageRole::System, marker);
        next.messages.insert(0, marker_session.messages.remove(0));
        let mut link = Message::new(
            MessageRole::System,
            format!("<boss-rotation-link:{}>", old.id),
        );
        link.hidden = true;
        next.messages.insert(1, link);
        for block in &mut next.transcript_blocks {
            block.after_message += 1;
        }
    }

    #[test]
    fn refresh_keeps_mounted_scrollback_until_successor_is_hydrated() {
        let project = Uuid::new_v4();
        let mut old = chat(project, "Earlier conversation");
        let mut next = chat(project, "After refresh");
        rotate(&mut old, &mut next);
        let mut history = BossChatHistory::default();
        let mut selected = Some(old.id);
        let projection = next.list_projection();
        // The old Open/activation path lands on a projection with no link:
        // there is no earlier content to display until hydration finishes.
        assert!(
            history
                .transcript_rows(&projection, &HashSet::new(), None)
                .iter()
                .all(|row| !matches!(row, TranscriptRowKind::BossHistory(..)))
        );
        assert!(!history.swap_live_session(&[old.clone(), projection], &mut selected, next.id));
        assert_eq!(selected, Some(old.id));
        assert!(history.swap_live_session(&[old.clone(), next.clone()], &mut selected, next.id));
        assert_eq!(selected, Some(next.id));
        assert_eq!(
            history.sessions[&old.id].session.messages[0].content,
            old.messages[0].content
        );
        let rows = history.transcript_rows(&next, &HashSet::new(), None);
        assert!(rows.contains(&TranscriptRowKind::BossHistory(
            old.id,
            BossHistoryRow::Message(0)
        )));
        assert!(
            rows.iter()
                .any(|row| matches!(row, TranscriptRowKind::Message(_)))
        );
        assert!(!history.swap_live_session(&[old, next.clone()], &mut selected, next.id));
        assert_eq!(history.sessions.len(), 1);
        // An older Open reply arriving after the swap is a projection. It
        // must not erase the successor's hydrated predecessor link.
        let mut catalog = vec![next.clone()];
        super::super::runtime::merge_remote_session_catalog(
            &mut catalog,
            vec![next.list_projection()],
            |_| false,
            |_| false,
            false,
        );
        assert!(catalog[0].detail_loaded);
        assert_eq!(
            boss_chat_predecessor(&catalog[0]),
            Some(history.chain(&next).0[0])
        );
    }

    #[test]
    fn refresh_preserves_reading_position_across_chained_swaps() {
        let project = Uuid::new_v4();
        let mut old = chat(project, "Reading here");
        let mut middle = chat(project, "Intermediate");
        let mut latest = chat(project, "Latest");
        rotate(&mut old, &mut middle);
        rotate(&mut middle, &mut latest);
        let mut selected = Some(old.id);
        let mut history = BossChatHistory::default();
        let sessions = [old.clone(), latest.clone()];
        // A newer refresh overtakes the successor load. Do not skip a gap.
        assert!(!history.swap_live_session(&sessions, &mut selected, latest.id));
        assert_eq!(selected, Some(old.id));
        history.finish_load(
            middle.id,
            Some(project),
            Ok(BossHistorySession::new(middle)),
        );
        let previous: Vec<_> = history
            .transcript_rows(&old, &HashSet::new(), None)
            .into_iter()
            .map(|row| {
                BossHistoryRow::from_content(row).map_or(row, |content| {
                    TranscriptRowKind::BossHistory(old.id, content)
                })
            })
            .collect();
        assert!(history.swap_live_session(&sessions, &mut selected, latest.id));
        let next = history.transcript_rows(&latest, &HashSet::new(), None);
        let scroll = ListOffset {
            item_ix: 1,
            offset_in_item: px(17.5),
        };
        let kept = refresh_scroll_offset(&previous, &next, scroll);
        assert_eq!(next[kept.item_ix], previous[scroll.item_ix]);
        assert_eq!(kept.offset_in_item, px(17.5));
        assert!(history.chain(&latest).0.contains(&old.id));
    }

    #[test]
    fn refresh_completion_cannot_select_boss_after_the_reader_leaves() {
        let project = Uuid::new_v4();
        let mut old = chat(project, "Earlier");
        let mut next = chat(project, "Successor");
        rotate(&mut old, &mut next);
        let another = chat(Uuid::new_v4(), "Other page");
        let mut history = BossChatHistory::default();
        let sessions = [old, next.clone(), another.clone()];
        for mut selected in [Some(another.id), None] {
            let before = selected;
            assert!(!history.swap_live_session(&sessions, &mut selected, next.id));
            assert_eq!(selected, before);
            assert!(history.sessions.is_empty());
        }
    }

    #[test]
    fn rotation_history_is_lazy_and_retries_failed_loads_without_duplicates() {
        let project = Uuid::new_v4();
        let mut oldest = chat(project, "Oldest");
        let mut recent = chat(project, "Recent");
        let mut live = chat(project, "Live");
        rotate(&mut oldest, &mut recent);
        rotate(&mut recent, &mut live);
        let mut history = BossChatHistory::default();
        assert_eq!(boss_chat_predecessor(&recent), Some(oldest.id));
        assert_eq!(history.chain(&live), (vec![], Some(recent.id)));
        assert!(history.begin_load(recent.id));
        assert!(!history.begin_load(recent.id));
        history.finish_load(
            recent.id,
            Some(project),
            Ok(BossHistorySession::new(recent.clone())),
        );
        assert!(!history.begin_load(recent.id));
        assert_eq!(history.chain(&live), (vec![recent.id], Some(oldest.id)));
        assert!(!history.loading.contains(&oldest.id));
        let rows = history.transcript_rows(&live, &HashSet::new(), None);
        assert_eq!(rows[0], TranscriptRowKind::BossHistoryBoundary(oldest.id));
        assert!(
            !rows.iter().any(
                |row| matches!(row, TranscriptRowKind::BossHistory(id, _) if *id == oldest.id)
            )
        );
        assert!(history.begin_load(oldest.id));
        history.finish_load(oldest.id, Some(project), Err(anyhow::anyhow!("offline")));
        assert!(history.errors.contains_key(&oldest.id));
        assert_eq!(history.chain(&live), (vec![recent.id], Some(oldest.id)));
        assert!(history.begin_load(oldest.id));
        assert!(!history.errors.contains_key(&oldest.id));
        history.finish_load(
            oldest.id,
            Some(project),
            Ok(BossHistorySession::new(oldest.clone())),
        );
        assert_eq!(history.chain(&live), (vec![recent.id, oldest.id], None));
        assert_eq!(history.sessions.len(), 2);
    }

    #[test]
    fn scrolling_to_the_boundary_targets_its_page_once() {
        let project = Uuid::new_v4();
        let mut older = chat(project, "Older");
        let mut recent = chat(project, "Recent");
        let mut live = chat(project, "Live");
        rotate(&mut older, &mut recent);
        rotate(&mut recent, &mut live);
        let mut history = BossChatHistory::default();
        history
            .sessions
            .insert(recent.id, BossHistorySession::new(recent));
        let rows = history.transcript_rows(&live, &HashSet::new(), None);
        assert_eq!(rows[0], TranscriptRowKind::BossHistoryBoundary(older.id));
        let top = ListOffset {
            item_ix: 0,
            offset_in_item: px(4.0),
        };
        // The tail-follow position is not a real scroll into the boundary.
        assert_eq!(
            top_history_boundary(
                &rows,
                false,
                ListOffset {
                    item_ix: 0,
                    offset_in_item: Pixels::ZERO
                }
            ),
            None
        );
        // Parking just below the boundary leaves it alone too.
        assert_eq!(
            top_history_boundary(
                &rows,
                true,
                ListOffset {
                    item_ix: 1,
                    offset_in_item: Pixels::ZERO
                }
            ),
            None
        );
        assert_eq!(top_history_boundary(&rows, true, top), Some(older.id));
        // In-flight and failed loads hold their state on the row instead of
        // re-issuing the request every frame.
        assert!(history.begin_load(older.id));
        history.finish_load(older.id, Some(project), Err(anyhow::anyhow!("offline")));
        assert!(history.errors.contains_key(&older.id));
        assert!(!history.loading.contains(&older.id));
    }

    #[test]
    fn turn_navigation_crosses_loaded_session_boundaries_and_returns_to_live_tail() {
        let project = Uuid::new_v4();
        let mut first = chat(project, "First");
        first.begin_turn("Second");
        first.push_message(MessageRole::Assistant, "Second answer");
        first.finish_active_turn(TurnStatus::Completed);
        let mut second = chat(project, "Third");
        let mut live = chat(project, "Fourth");
        rotate(&mut first, &mut second);
        rotate(&mut second, &mut live);
        let mut history = BossChatHistory::default();
        for session in [first.clone(), second.clone()] {
            history
                .sessions
                .insert(session.id, BossHistorySession::new(session));
        }
        let rows = history.transcript_rows(&live, &HashSet::new(), None);
        let turns = history.navigation_turns(&live, &rows);
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn.prompt.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second", "Third", "Fourth"]
        );
        let turn_rows = turns.iter().map(|turn| turn.row_index).collect::<Vec<_>>();
        assert!(turn_rows.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            previous_navigation_turn_index(&turn_rows, turns[3].row_index, true),
            Some(2)
        );
        assert_eq!(
            next_navigation_turn_index(&turn_rows, turns[1].row_index),
            Some(2)
        );
        assert_eq!(
            next_navigation_turn_index(&turn_rows, turns[2].row_index),
            Some(3)
        );
        // The existing next-turn action pins the shared list to its tail on None.
        assert_eq!(
            next_navigation_turn_index(&turn_rows, turns[3].row_index),
            None
        );
        assert_eq!(
            rows[turns[0].row_index],
            TranscriptRowKind::BossHistory(first.id, BossHistoryRow::Message(0))
        );
        assert_eq!(
            rows[turns[2].row_index],
            TranscriptRowKind::BossHistory(second.id, BossHistoryRow::Message(2))
        );
        assert_eq!(rows[turns[3].row_index], TranscriptRowKind::Message(2));
        assert_eq!(turns[2].response, "Answer to Third");
        assert_eq!(turns[3].response, "Answer to Fourth");
    }

    #[test]
    fn paging_prepends_rows_without_replacing_the_visible_session_suffix() {
        let project = Uuid::new_v4();
        let mut older = chat(project, "Older");
        let mut recent = chat(project, "Recent");
        let mut live = chat(project, "Live");
        rotate(&mut older, &mut recent);
        rotate(&mut recent, &mut live);
        let mut history = BossChatHistory::default();
        history
            .sessions
            .insert(recent.id, BossHistorySession::new(recent));
        let before = history.transcript_rows(&live, &HashSet::new(), None);
        history
            .sessions
            .insert(older.id, BossHistorySession::new(older));
        let after = history.transcript_rows(&live, &HashSet::new(), None);
        let (replaced, inserted) = transcript_row_splice(&before, &after).unwrap();
        assert_eq!(replaced, 0..1);
        assert_eq!(&before[1..], &after[inserted..]);
        // Native list splicing preserves an explicit logical scroll anchor.
        let list = ListState::new(before.len(), ListAlignment::Top, px(240.0));
        let live_row = before
            .iter()
            .position(|row| *row == TranscriptRowKind::Message(2))
            .unwrap();
        list.scroll_to(ListOffset {
            item_ix: live_row,
            offset_in_item: px(7.0),
        });
        list.splice(replaced, inserted);
        let scroll = list.logical_scroll_top();
        assert_eq!(after[scroll.item_ix], TranscriptRowKind::Message(2));
        assert_eq!(scroll.offset_in_item, px(7.0));
    }

    #[test]
    fn historical_rows_and_navigation_stay_cached_while_the_live_chat_streams() {
        let project = Uuid::new_v4();
        let mut old = chat(project, "Old");
        let mut live = chat(project, "Live");
        rotate(&mut old, &mut live);
        let mut history = BossChatHistory::default();
        history
            .sessions
            .insert(old.id, BossHistorySession::new(old.clone()));
        history.transcript_rows(&live, &HashSet::new(), None);
        let entry = &history.sessions[&old.id];
        let cached = entry.cached_rows();
        let navigation = entry.navigation();
        live.begin_turn("Still live");
        live.push_message(MessageRole::Assistant, "Streaming");
        history.transcript_rows(&live, &HashSet::new(), None);
        assert!(Arc::ptr_eq(&cached, &entry.cached_rows()));
        assert!(Arc::ptr_eq(&navigation, &entry.navigation()));
        assert_eq!(entry.content_index(BossHistoryRow::Message(0)), Some(0));
        assert_eq!(
            entry.footers[&1].0.as_ref().map(|text| text.as_ref()),
            Some("Answer to Old")
        );
    }

    #[test]
    fn historical_loads_cannot_enter_another_boss_chat_or_follow_a_cycle() {
        let project = Uuid::new_v4();
        let mut old = chat(project, "Old");
        let mut live = chat(project, "Live");
        rotate(&mut old, &mut live);
        let mut history = BossChatHistory::default();
        history.begin_load(old.id);
        history.finish_load(
            old.id,
            Some(Uuid::new_v4()),
            Ok(BossHistorySession::new(old.clone())),
        );
        assert!(history.sessions.is_empty());
        assert!(history.errors.contains_key(&old.id));
        history.begin_load(old.id);
        history.finish_load(
            old.id,
            Some(project),
            Ok(BossHistorySession::new(old.clone())),
        );
        let unrelated = chat(project, "Another chat");
        assert_eq!(history.chain(&unrelated), (vec![], None));
        // A malformed historic link cannot loop or duplicate the live session.
        history.sessions.get_mut(&old.id).unwrap().predecessor = Some(live.id);
        assert_eq!(history.chain(&live), (vec![old.id], None));
    }
}
