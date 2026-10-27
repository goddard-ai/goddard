//! Bounded session reconciliation. Prepare snapshot ordering on a worker;
//! retain index construction across snapshot generations and metadata edits.
use super::*;

#[derive(Default)]
pub struct CatalogSessionOrder {
    rows: Vec<(Uuid, usize)>,
    project_ids: HashSet<Uuid>,
}

impl CatalogSessionOrder {
    pub fn prepare(catalog: &TaskCatalog) -> Self {
        // Keep the last projection for a duplicate id, matching bulk merge.
        let mut rows = BTreeMap::new();
        for (index, session) in catalog.sessions.iter().enumerate() {
            rows.insert(session.id, index);
        }
        Self {
            rows: rows.into_iter().collect(),
            project_ids: catalog.projects.iter().map(|project| project.id).collect(),
        }
    }
    pub fn project_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.project_ids.iter().copied()
    }
    pub fn contains_project(&self, id: Uuid) -> bool {
        self.project_ids.contains(&id)
    }
    fn contains(&self, id: Uuid) -> bool {
        self.rows.binary_search_by_key(&id, |row| row.0).is_ok()
    }
}

#[derive(Default)]
pub struct CatalogSessionProgress {
    last_remote: Option<Uuid>,
    remote_next: usize,
    remote_remaining: usize,
    local_next: usize,
    local_wrap: bool,
    local_complete: bool,
    topology: Option<u64>,
    last_removed: Option<Uuid>,
    removals_complete: bool,
}

pub enum CatalogSessionEffect {
    Applied { id: Uuid, added: bool },
    Removed(Box<AgentSession>),
    Reregister(Uuid),
}

#[derive(Default)]
pub struct CatalogSessionBatch {
    pub effects: Vec<CatalogSessionEffect>,
    /// Move to a reclamation worker; dropping a large tree is not frame work.
    pub retired_index: Option<(BTreeMap<Uuid, usize>, BTreeMap<Uuid, usize>)>,
    pub complete: bool,
    pub processed: usize,
}

pub struct CatalogSelectionFallback {
    project: Option<Uuid>,
    next: usize,
    preferred: Option<(usize, Uuid, u64)>,
    any: Option<(usize, Uuid, u64)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CatalogSelectionStep {
    Pending,
    Cancelled,
    Select(Uuid),
    Empty,
}

impl CatalogSelectionFallback {
    pub fn new(project: Option<Uuid>) -> Self {
        Self {
            project,
            next: 0,
            preferred: None,
            any: None,
        }
    }

    pub fn step(
        &mut self,
        rows: &[AgentSession],
        project: Option<Uuid>,
        selected: Option<Uuid>,
        budget: usize,
    ) -> CatalogSelectionStep {
        if selected.is_some() || project != self.project {
            return CatalogSelectionStep::Cancelled;
        }
        let end = self.next.saturating_add(budget).min(rows.len());
        for (index, row) in rows.iter().enumerate().take(end).skip(self.next) {
            if self.any.is_none_or(|best| row.updated_at >= best.2) {
                self.any = Some((index, row.id, row.updated_at));
            }
            if self.project.is_none_or(|project| row.project_id == project)
                && self.preferred.is_none_or(|best| row.updated_at >= best.2)
            {
                self.preferred = Some((index, row.id, row.updated_at));
            }
        }
        self.next = end;
        if end != rows.len() {
            return CatalogSelectionStep::Pending;
        }
        if let Some((index, id, _)) = self.preferred.or(self.any) {
            if rows.get(index).is_none_or(|row| row.id != id)
                || (self.preferred.is_some()
                    && self
                        .project
                        .is_some_and(|project| rows[index].project_id != project))
            {
                self.next = 0;
                self.preferred = None;
                self.any = None;
                CatalogSelectionStep::Pending
            } else {
                CatalogSelectionStep::Select(id)
            }
        } else {
            CatalogSelectionStep::Empty
        }
    }
}

impl CatalogSessionProgress {
    pub fn start(order: &CatalogSessionOrder, previous: Option<&Self>) -> Self {
        let last_remote = previous.and_then(|previous| previous.last_remote);
        let remote_next = last_remote.map_or(0, |id| order.rows.partition_point(|row| row.0 <= id));
        let local_next = previous.map_or(0, |previous| previous.local_next);
        Self {
            last_remote,
            remote_next,
            remote_remaining: order.rows.len(),
            local_next,
            local_wrap: local_next != 0,
            ..Self::default()
        }
    }

    pub fn step(
        &mut self,
        state: &mut PersistedState,
        catalog: &TaskCatalog,
        order: &CatalogSessionOrder,
        fresh: bool,
        key: DaemonKey,
        started_at: std::time::Instant,
        index_budget: usize,
        row_budget: usize,
        owns: impl Fn(Uuid) -> bool,
        has_runtime: impl Fn(Uuid) -> bool,
        pending_metadata: impl Fn(Uuid) -> bool,
    ) -> CatalogSessionBatch {
        let mut batch = CatalogSessionBatch::default();
        if state.session_catalog_index.revision != Some(state.session_topology_revision)
            || state.session_catalog_index.indexed_rows > state.sessions.len()
        {
            batch.retired_index = Some((
                std::mem::take(&mut state.session_catalog_index.positions),
                std::mem::take(&mut state.session_catalog_index.project_counts),
            ));
            state.session_catalog_index.indexed_rows = 0;
            state.session_catalog_index.revision = Some(state.session_topology_revision);
        }
        let start = state.session_catalog_index.indexed_rows;
        let end = start.saturating_add(index_budget).min(state.sessions.len());
        for index in start..end {
            state
                .session_catalog_index
                .positions
                .entry(state.sessions[index].id)
                .or_insert(index);
            *state
                .session_catalog_index
                .project_counts
                .entry(state.sessions[index].project_id)
                .or_default() += 1;
        }
        state.session_catalog_index.indexed_rows = end;
        batch.processed = end - start;
        if end != state.sessions.len() {
            return batch;
        }
        if self.topology != Some(state.session_topology_revision) {
            self.local_complete = false;
            self.local_wrap |= self.local_next != 0;
        }
        // Alternate retention and incoming rows so a large existing catalog
        // cannot hold all incoming metadata behind a full retention scan.
        for operation in 0..row_budget {
            if !self.removals_complete
                && (operation % 3 == 0 || (self.local_complete && self.remote_remaining == 0))
            {
                use std::ops::Bound::{Excluded, Unbounded};
                let next = state
                    .locally_removed_sessions
                    .get(&key)
                    .and_then(|removed| {
                        removed
                            .range((self.last_removed.map_or(Unbounded, Excluded), Unbounded))
                            .next()
                            .map(|(id, removed_at)| (*id, *removed_at))
                    });
                if let Some((id, removed_at)) = next {
                    if started_at > removed_at && !order.contains(id) {
                        state
                            .locally_removed_sessions
                            .get_mut(&key)
                            .expect("removal exists")
                            .remove(&id);
                    }
                    self.last_removed = Some(id);
                    batch.processed += 1;
                } else {
                    self.removals_complete = true;
                }
            } else if !self.local_complete && (operation % 2 == 0 || self.remote_remaining == 0) {
                if self.local_next >= state.sessions.len() {
                    if self.local_wrap {
                        self.local_next = 0;
                        self.local_wrap = false;
                    } else {
                        self.local_complete = true;
                        continue;
                    }
                }
                if self.local_next >= state.sessions.len() {
                    self.local_complete = true;
                    continue;
                }
                let local = &state.sessions[self.local_next];
                let id = local.id;
                let missing = owns(id) && local.has_started() && !order.contains(id);
                if missing && fresh && local.incognito && local.detail_loaded {
                    // Completion of the catalog is earlier than adoption of
                    // this transcript. Keep it until a later catalog reports it.
                    state.unreported_sessions.insert(id);
                    batch.effects.push(CatalogSessionEffect::Reregister(id));
                    self.local_next += 1;
                } else if missing
                    && !state.unreported_sessions.contains(&id)
                    && !pending_metadata(id)
                    && !state.dirty_sessions.contains(&id)
                    && !state.pending_session_saves.contains_key(&id)
                {
                    let removed = state.sessions.swap_remove(self.local_next);
                    state.session_catalog_index.positions.remove(&id);
                    super::catalog_merge::decrement_project_reference(
                        &mut state.session_catalog_index.project_counts,
                        removed.project_id,
                    );
                    if let Some(moved) = state.sessions.get(self.local_next) {
                        state
                            .session_catalog_index
                            .positions
                            .insert(moved.id, self.local_next);
                    }
                    batch
                        .effects
                        .push(CatalogSessionEffect::Removed(Box::new(removed)));
                    state.finish_indexed_catalog_mutation();
                } else {
                    self.local_next += 1;
                }
                batch.processed += 1;
            } else if self.remote_remaining != 0 {
                self.remote_next %= order.rows.len();
                let (id, index) = order.rows[self.remote_next];
                if state
                    .locally_removed_sessions
                    .get(&key)
                    .is_some_and(|removed| removed.contains_key(&id))
                {
                    self.last_remote = Some(id);
                    self.remote_next += 1;
                    self.remote_remaining -= 1;
                    batch.processed += 1;
                    continue;
                }
                let mut remote = catalog.sessions[index].list_projection();
                state.unreported_sessions.remove(&id);
                let added = if let Some(&position) = state.session_catalog_index.positions.get(&id)
                {
                    let local = &mut state.sessions[position];
                    let previous_project = local.project_id;
                    if pending_metadata(id) {
                        remote.title.clone_from(&local.title);
                        remote.archived_at = local.archived_at;
                        remote.pinned_at = local.pinned_at;
                        remote.agent_rename_allowed = local.agent_rename_allowed;
                        remote.metadata_revision = local.metadata_revision;
                        remote.metadata_mutation_id = local.metadata_mutation_id;
                    }
                    merge_session_catalog_row(local, remote, has_runtime(id));
                    if local.project_id != previous_project {
                        super::catalog_merge::decrement_project_reference(
                            &mut state.session_catalog_index.project_counts,
                            previous_project,
                        );
                        *state
                            .session_catalog_index
                            .project_counts
                            .entry(local.project_id)
                            .or_default() += 1;
                    }
                    false
                } else {
                    state
                        .session_catalog_index
                        .positions
                        .insert(id, state.sessions.len());
                    *state
                        .session_catalog_index
                        .project_counts
                        .entry(remote.project_id)
                        .or_default() += 1;
                    state.sessions.push(remote);
                    state.finish_indexed_catalog_mutation();
                    true
                };
                batch
                    .effects
                    .push(CatalogSessionEffect::Applied { id, added });
                self.last_remote = Some(id);
                self.remote_next += 1;
                self.remote_remaining -= 1;
                batch.processed += 1;
            } else {
                break;
            }
        }
        self.topology = Some(state.session_topology_revision);
        batch.complete =
            self.local_complete && self.remote_remaining == 0 && self.removals_complete;
        batch
    }
}

impl PersistedState {
    fn finish_indexed_catalog_mutation(&mut self) {
        self.session_topology_revision = self.session_topology_revision.wrapping_add(1);
        self.session_catalog_index.revision = Some(self.session_topology_revision);
        self.session_catalog_index.indexed_rows = self.sessions.len();
    }

    pub fn catalog_index_ready(&self) -> bool {
        self.session_catalog_index.revision == Some(self.session_topology_revision)
            && self.session_catalog_index.indexed_rows == self.sessions.len()
    }

    pub fn catalog_project_has_sessions(&self, project: Uuid) -> bool {
        !self.catalog_index_ready()
            || self
                .session_catalog_index
                .project_counts
                .contains_key(&project)
    }

    /// Reuse a completed index without making a render-time cache miss build it.
    pub fn catalog_session(&self, id: Uuid) -> Option<&AgentSession> {
        if self.session_catalog_index.revision == Some(self.session_topology_revision)
            && self.session_catalog_index.indexed_rows == self.sessions.len()
        {
            self.session_catalog_index
                .positions
                .get(&id)
                .and_then(|index| self.sessions.get(*index))
                .filter(|row| row.id == id)
        } else {
            self.sessions.iter().find(|session| session.id == id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn row(id: u128, title: &str) -> AgentSession {
        let mut row = AgentSession::new(Uuid::nil(), ProviderKind::Codex);
        row.id = Uuid::from_u128(id);
        row.title = title.into();
        row.begin_turn("resident history");
        row
    }

    #[test]
    fn bounded_batches_preserve_histories_pending_edits_borrowed_rows_and_new_local_tasks() {
        let mut state = PersistedState::empty();
        state.sessions = (1..=64).map(|id| row(id, "local")).collect();
        state.sessions.push(row(700, "borrowed"));
        state.note_session_topology_change();
        state.sessions[1].title = "optimistic rename".into();
        state.sessions[1].metadata_revision = 9;
        let active_status = state.sessions[0].status;
        let active_updated = state.sessions[0].updated_at;
        let mut catalog = TaskCatalog {
            projects: Vec::new(),
            sessions: (1..=512)
                .map(|id| row(id, "remote").list_projection())
                .collect(),
        };
        catalog.sessions[1].metadata_revision = 10;
        let order = CatalogSessionOrder::prepare(&catalog);
        let started = Instant::now();
        let mut progress = CatalogSessionProgress::start(&order, None);
        let mut complete = false;
        for step in 0..5000 {
            if step == 5 {
                state.push_session(row(900, "created while applying"));
            }
            let batch = progress.step(
                &mut state,
                &catalog,
                &order,
                false,
                DaemonKey::Local,
                started,
                7,
                5,
                |id| id != Uuid::from_u128(700),
                |id| id == Uuid::from_u128(1),
                |id| id == Uuid::from_u128(2),
            );
            assert!(
                batch.processed <= 12,
                "index or row processing exceeded its explicit budget"
            );
            if batch.complete {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(state.sessions.len(), 514);
        let active = state.catalog_session(Uuid::from_u128(1)).unwrap();
        assert_eq!(active.status, active_status);
        assert_eq!(active.updated_at, active_updated);
        assert_eq!(active.messages[0].content, "resident history");
        let pending = state.catalog_session(Uuid::from_u128(2)).unwrap();
        assert_eq!(pending.title, "optimistic rename");
        assert_eq!(pending.metadata_revision, 9);
        assert!(state.catalog_session(Uuid::from_u128(700)).is_some());
        assert!(state.catalog_session(Uuid::from_u128(900)).is_some());
    }

    #[test]
    fn fresh_connection_reregisters_resident_incognito_without_retaining_skeletons() {
        let mut state = PersistedState::empty();
        let mut resident = row(1, "incognito");
        resident.incognito = true;
        state.sessions.push(resident);
        let mut skeleton = row(2, "skeleton").list_projection();
        skeleton.incognito = true;
        state.sessions.push(skeleton);
        state.sessions.push(row(3, "ordinary"));
        state.note_session_topology_change();
        let empty = TaskCatalog::default();
        let order = CatalogSessionOrder::prepare(&empty);
        let mut progress = CatalogSessionProgress::start(&order, None);
        let mut recovered = HashSet::new();
        let mut complete = false;
        for _ in 0..50 {
            let batch = progress.step(
                &mut state,
                &empty,
                &order,
                true,
                DaemonKey::Local,
                Instant::now(),
                2,
                3,
                |_| true,
                |_| false,
                |_| false,
            );
            for effect in batch.effects {
                if let CatalogSessionEffect::Reregister(id) = effect {
                    recovered.insert(id);
                }
            }
            if batch.complete {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(recovered, HashSet::from([Uuid::from_u128(1)]));
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].messages[0].content, "resident history");
        // A same-connection update may race the asynchronous adoption save.
        let mut ordinary_refresh = CatalogSessionProgress::start(&order, None);
        for _ in 0..50 {
            if ordinary_refresh
                .step(
                    &mut state,
                    &empty,
                    &order,
                    false,
                    DaemonKey::Local,
                    Instant::now(),
                    2,
                    3,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        assert_eq!(
            state.sessions.len(),
            1,
            "adoption had not yet been observed in the catalog"
        );
        let observed = TaskCatalog {
            projects: Vec::new(),
            sessions: vec![state.sessions[0].list_projection()],
        };
        let observed_order = CatalogSessionOrder::prepare(&observed);
        let mut reported = CatalogSessionProgress::start(&observed_order, None);
        for _ in 0..50 {
            if reported
                .step(
                    &mut state,
                    &observed,
                    &observed_order,
                    false,
                    DaemonKey::Local,
                    Instant::now(),
                    2,
                    3,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        let mut deleted = CatalogSessionProgress::start(&order, None);
        for _ in 0..50 {
            if deleted
                .step(
                    &mut state,
                    &empty,
                    &order,
                    false,
                    DaemonKey::Local,
                    Instant::now(),
                    2,
                    3,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        assert!(
            state.sessions.is_empty(),
            "a later confirmed deletion must still apply"
        );
    }

    #[test]
    fn project_reference_counts_follow_partial_moves_removals_and_legacy_updates() {
        let first_project = Uuid::new_v4();
        let second_project = Uuid::new_v4();
        let mut first = row(1, "moving");
        first.project_id = first_project;
        let mut second = row(2, "retiring");
        second.project_id = first_project;
        let mut state = PersistedState::empty();
        state.sessions = vec![first, second];
        state.note_session_topology_change();
        let mut moved = state.sessions[0].list_projection();
        moved.project_id = second_project;
        let catalog = TaskCatalog {
            projects: Vec::new(),
            sessions: vec![moved],
        };
        let order = CatalogSessionOrder::prepare(&catalog);
        let mut progress = CatalogSessionProgress::start(&order, None);
        assert!(
            state.catalog_project_has_sessions(first_project),
            "unknown index must preserve the project"
        );
        let mut complete = false;
        for _ in 0..50 {
            let batch = progress.step(
                &mut state,
                &catalog,
                &order,
                false,
                DaemonKey::Local,
                Instant::now(),
                1,
                2,
                |_| true,
                |_| false,
                |_| false,
            );
            if state
                .sessions
                .iter()
                .any(|row| row.project_id == first_project)
            {
                assert!(state.catalog_project_has_sessions(first_project));
            }
            if batch.complete {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert!(!state.catalog_project_has_sessions(first_project));
        assert!(state.catalog_project_has_sessions(second_project));
        let mut moved_again = state.sessions[0].list_projection();
        moved_again.project_id = first_project;
        state.merge_session_catalog(vec![moved_again], |_| false, |_| false, false);
        assert!(state.catalog_project_has_sessions(first_project));
        assert!(!state.catalog_project_has_sessions(second_project));
        let empty = TaskCatalog::default();
        let empty_order = CatalogSessionOrder::prepare(&empty);
        let mut progress = CatalogSessionProgress::start(&empty_order, None);
        for _ in 0..50 {
            if progress
                .step(
                    &mut state,
                    &empty,
                    &empty_order,
                    false,
                    DaemonKey::Local,
                    Instant::now(),
                    1,
                    2,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        assert!(!state.catalog_project_has_sessions(first_project));
    }

    #[test]
    fn repeated_supersession_makes_fair_progress_then_converges() {
        let mut state = PersistedState::empty();
        let mut catalog = TaskCatalog {
            projects: Vec::new(),
            sessions: (1..=128)
                .map(|id| row(id, "initial").list_projection())
                .collect(),
        };
        let order = CatalogSessionOrder::prepare(&catalog);
        let mut progress = CatalogSessionProgress::start(&order, None);
        let mut observed = HashSet::new();
        for generation in 0..128 {
            for row in &mut catalog.sessions {
                row.title = format!("generation {generation}");
            }
            progress = CatalogSessionProgress::start(&order, Some(&progress));
            let batch = progress.step(
                &mut state,
                &catalog,
                &order,
                false,
                DaemonKey::Local,
                Instant::now(),
                16,
                8,
                |_| true,
                |_| false,
                |_| false,
            );
            assert!(batch.processed <= 24);
            for effect in batch.effects {
                if let CatalogSessionEffect::Applied { id, .. } = effect {
                    observed.insert(id);
                }
            }
        }
        assert_eq!(observed.len(), 128, "new generations starved later UUIDs");
        let mut completed = false;
        for _ in 0..1000 {
            let batch = progress.step(
                &mut state,
                &catalog,
                &order,
                false,
                DaemonKey::Local,
                Instant::now(),
                16,
                8,
                |_| true,
                |_| false,
                |_| false,
            );
            if batch.complete {
                completed = true;
                break;
            }
        }
        assert!(completed);
        assert_eq!(state.sessions.len(), 128);
        assert!(
            state
                .sessions
                .iter()
                .all(|row| row.title == "generation 127")
        );
    }

    #[test]
    fn selection_fallback_yields_revalidates_removal_and_preserves_user_selection() {
        let mut rows = (1..=5).map(|id| row(id, "candidate")).collect::<Vec<_>>();
        for (index, row) in rows.iter_mut().enumerate() {
            row.updated_at = (index + 1) as u64;
        }
        let mut cancelled = CatalogSelectionFallback::new(None);
        assert_eq!(
            cancelled.step(&rows, None, None, 1),
            CatalogSelectionStep::Pending
        );
        assert_eq!(
            cancelled.step(&rows, None, Some(rows[0].id), 1),
            CatalogSelectionStep::Cancelled
        );
        let mut fallback = CatalogSelectionFallback::new(None);
        assert_eq!(
            fallback.step(&rows, None, None, 4),
            CatalogSelectionStep::Pending
        );
        rows.remove(4);
        rows.remove(0);
        assert_eq!(
            fallback.step(&rows, None, None, 1),
            CatalogSelectionStep::Pending,
            "the old positional winner no longer exists"
        );
        let mut selected = None;
        for _ in 0..10 {
            if let CatalogSelectionStep::Select(id) = fallback.step(&rows, None, None, 1) {
                selected = Some(id);
                break;
            }
        }
        assert_eq!(selected, Some(Uuid::from_u128(4)));
        let mut project_changed = CatalogSelectionFallback::new(Some(Uuid::nil()));
        assert_eq!(
            project_changed.step(&rows, Some(Uuid::new_v4()), None, 1),
            CatalogSelectionStep::Cancelled
        );
    }

    #[test]
    fn local_removal_waits_for_causal_absence_before_allowing_server_restore() {
        let mut state = PersistedState::empty();
        state.sessions.push(row(1, "original"));
        let catalog = TaskCatalog {
            projects: Vec::new(),
            sessions: vec![state.sessions[0].list_projection()],
        };
        let order = CatalogSessionOrder::prepare(&catalog);
        let before_removal = Instant::now();
        state.sessions.clear();
        state.note_session_removed(Uuid::from_u128(1));
        state.note_local_session_removed(Uuid::from_u128(1), DaemonKey::Local);
        let mut progress = CatalogSessionProgress::start(&order, None);
        for _ in 0..20 {
            if progress
                .step(
                    &mut state,
                    &catalog,
                    &order,
                    false,
                    DaemonKey::Local,
                    before_removal,
                    4,
                    4,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        assert!(
            state.sessions.is_empty(),
            "an older snapshot recreated a deleted local row"
        );
        let empty = TaskCatalog::default();
        let empty_order = CatalogSessionOrder::prepare(&empty);
        let mut absent = CatalogSessionProgress::start(&empty_order, None);
        for _ in 0..20 {
            if absent
                .step(
                    &mut state,
                    &empty,
                    &empty_order,
                    false,
                    DaemonKey::Local,
                    before_removal,
                    4,
                    4,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        assert!(
            state.locally_removed_sessions[&DaemonKey::Local].contains_key(&Uuid::from_u128(1)),
            "pre-removal absence cleared the fence"
        );
        let mut absent = CatalogSessionProgress::start(&empty_order, None);
        let after_removal = Instant::now();
        for _ in 0..20 {
            if absent
                .step(
                    &mut state,
                    &empty,
                    &empty_order,
                    false,
                    DaemonKey::Local,
                    after_removal,
                    4,
                    4,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        let mut restored = CatalogSessionProgress::start(&order, None);
        for _ in 0..20 {
            if restored
                .step(
                    &mut state,
                    &catalog,
                    &order,
                    false,
                    DaemonKey::Local,
                    Instant::now(),
                    4,
                    4,
                    |_| true,
                    |_| false,
                    |_| false,
                )
                .complete
            {
                break;
            }
        }
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].id, Uuid::from_u128(1));
    }
}
