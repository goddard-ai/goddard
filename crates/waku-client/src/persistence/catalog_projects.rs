//! Cooperative project reconciliation with session-reference-safe retirement.
use super::*;

#[derive(Debug, Default)]
pub(super) struct ProjectCatalogIndex {
    revision: Option<u64>,
    indexed_rows: usize,
    positions: BTreeMap<Uuid, usize>,
}

impl Clone for ProjectCatalogIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Drop for ProjectCatalogIndex {
    fn drop(&mut self) {
        if !self.positions.is_empty() {
            super::task_writer::retire(std::mem::take(&mut self.positions));
        }
    }
}

#[derive(Default)]
pub struct CatalogProjectProgress {
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

#[derive(Default)]
pub struct CatalogProjectBatch {
    pub applied: Vec<Uuid>,
    pub retired: Vec<Project>,
    pub removed: Vec<Uuid>,
    pub retired_index: Option<BTreeMap<Uuid, usize>>,
    pub processed: usize,
    pub complete: bool,
}

impl CatalogProjectProgress {
    pub fn start(order: &CatalogSessionOrder, previous: Option<&Self>) -> Self {
        let last_remote = previous.and_then(|previous| previous.last_remote);
        let local_next = previous.map_or(0, |previous| previous.local_next);
        Self {
            last_remote,
            remote_next: last_remote.map_or(0, |id| {
                order.project_rows.partition_point(|row| row.0 <= id)
            }),
            remote_remaining: order.project_rows.len(),
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
        key: DaemonKey,
        started_at: std::time::Instant,
        index_budget: usize,
        row_budget: usize,
        allow_removals: bool,
        owns: impl Fn(Uuid) -> bool,
    ) -> CatalogProjectBatch {
        let mut batch = CatalogProjectBatch::default();
        if state.project_catalog_index.revision != Some(state.project_topology_revision)
            || state.project_catalog_index.indexed_rows > state.projects.len()
        {
            batch.retired_index = Some(std::mem::take(&mut state.project_catalog_index.positions));
            state.project_catalog_index.indexed_rows = 0;
            state.project_catalog_index.revision = Some(state.project_topology_revision);
        }
        let start = state.project_catalog_index.indexed_rows;
        let end = start.saturating_add(index_budget).min(state.projects.len());
        for index in start..end {
            state
                .project_catalog_index
                .positions
                .entry(state.projects[index].id)
                .or_insert(index);
        }
        state.project_catalog_index.indexed_rows = end;
        batch.processed = end - start;
        if end != state.projects.len() {
            return batch;
        }
        if self.topology != Some(state.project_topology_revision) {
            self.local_complete = false;
            self.local_wrap |= self.local_next != 0;
        }
        for operation in 0..row_budget {
            if !self.removals_complete && (operation % 3 == 0 || self.remote_remaining == 0) {
                use std::ops::Bound::{Excluded, Unbounded};
                let next = state
                    .locally_removed_projects
                    .get(&key)
                    .and_then(|removed| {
                        removed
                            .range((self.last_removed.map_or(Unbounded, Excluded), Unbounded))
                            .next()
                            .map(|(id, removed_at)| (*id, *removed_at))
                    });
                if let Some((id, removed_at)) = next {
                    if started_at > removed_at && !order.contains_project(id) {
                        state
                            .locally_removed_projects
                            .get_mut(&key)
                            .expect("removal exists")
                            .remove(&id);
                    }
                    self.last_removed = Some(id);
                    batch.processed += 1;
                } else {
                    self.removals_complete = true;
                }
            } else if self.remote_remaining != 0 {
                self.remote_next %= order.project_rows.len();
                let (id, _) = order.project_rows[self.remote_next];
                if state.apply_catalog_project(catalog, order, key, id, &mut batch.retired) {
                    batch.applied.push(id);
                }
                self.last_remote = Some(id);
                self.remote_next += 1;
                self.remote_remaining -= 1;
                batch.processed += 1;
            } else if allow_removals && !self.local_complete {
                if self.local_next >= state.projects.len() {
                    if self.local_wrap {
                        self.local_next = 0;
                        self.local_wrap = false;
                    } else {
                        self.local_complete = true;
                        continue;
                    }
                }
                if self.local_next >= state.projects.len() {
                    self.local_complete = true;
                    continue;
                }
                let id = state.projects[self.local_next].id;
                if owns(id)
                    && !order.contains_project(id)
                    && !state.unreported_projects.contains(&id)
                    && !state.catalog_project_has_sessions(id)
                {
                    let removed = state.projects.swap_remove(self.local_next);
                    state.project_catalog_index.positions.remove(&id);
                    if let Some(moved) = state.projects.get(self.local_next) {
                        state
                            .project_catalog_index
                            .positions
                            .insert(moved.id, self.local_next);
                    }
                    state.finish_indexed_project_mutation();
                    batch.removed.push(id);
                    batch.retired.push(removed);
                } else {
                    self.local_next += 1;
                }
                batch.processed += 1;
            } else {
                break;
            }
        }
        self.topology = Some(state.project_topology_revision);
        batch.complete =
            self.remote_remaining == 0 && self.local_complete && self.removals_complete;
        batch
    }
}

impl PersistedState {
    /// Call after direct project membership, identity or order changes.
    pub fn note_project_topology_change(&mut self) {
        self.project_topology_revision = self.project_topology_revision.wrapping_add(1);
        self.project_catalog_index.revision = None;
        self.note_project_names_change();
    }

    pub fn push_project(&mut self, project: Project) {
        self.unreported_projects.insert(project.id);
        for removed in self.locally_removed_projects.values_mut() {
            removed.remove(&project.id);
        }
        self.projects.push(project);
        self.note_project_topology_change();
    }

    pub fn note_local_project_removed(&mut self, id: Uuid, key: DaemonKey) {
        self.unreported_projects.remove(&id);
        self.locally_removed_projects
            .entry(key)
            .or_default()
            .insert(id, std::time::Instant::now());
        self.note_project_topology_change();
    }

    pub fn revoke_project_removals(
        &mut self,
        key: DaemonKey,
    ) -> Option<BTreeMap<Uuid, std::time::Instant>> {
        self.locally_removed_projects.remove(&key)
    }

    fn finish_indexed_project_mutation(&mut self) {
        self.note_project_names_change();
        self.project_topology_revision = self.project_topology_revision.wrapping_add(1);
        self.project_catalog_index.revision = Some(self.project_topology_revision);
        self.project_catalog_index.indexed_rows = self.projects.len();
    }

    pub fn project_catalog_index_ready(&self) -> bool {
        self.project_catalog_index.revision == Some(self.project_topology_revision)
            && self.project_catalog_index.indexed_rows == self.projects.len()
    }

    /// Apply a referenced project before exposing a session contribution. The
    /// caller has prepared the index; this never builds or scans one on demand.
    pub fn apply_catalog_project(
        &mut self,
        catalog: &TaskCatalog,
        order: &CatalogSessionOrder,
        key: DaemonKey,
        id: Uuid,
        retired: &mut Vec<Project>,
    ) -> bool {
        if !self.project_catalog_index_ready()
            || self
                .locally_removed_projects
                .get(&key)
                .is_some_and(|removed| removed.contains_key(&id))
        {
            return false;
        }
        let Ok(remote) = order.project_rows.binary_search_by_key(&id, |row| row.0) else {
            return false;
        };
        let mut project = catalog.projects[order.project_rows[remote].1].clone();
        self.unreported_projects.remove(&id);
        if let Some(&position) = self.project_catalog_index.positions.get(&id) {
            let existing = &self.projects[position];
            if existing.name == project.name && existing.path == project.path {
                project.resolved_name.clone_from(&existing.resolved_name);
            } else {
                project.resolved_name = None;
                self.note_project_names_change();
            }
            retired.push(std::mem::replace(&mut self.projects[position], project));
        } else {
            self.project_catalog_index
                .positions
                .insert(id, self.projects.len());
            self.projects.push(project);
            self.finish_indexed_project_mutation();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn project(id: u128) -> Project {
        let mut project = Project::from_path(PathBuf::from(format!("/work/{id}")));
        project.id = Uuid::from_u128(id);
        project
    }

    fn finish(
        state: &mut PersistedState,
        catalog: &TaskCatalog,
        started_at: Instant,
        owns: impl Fn(Uuid) -> bool,
    ) {
        let order = CatalogSessionOrder::prepare(catalog);
        let mut progress = CatalogProjectProgress::start(&order, None);
        for _ in 0..4096 {
            let batch = progress.step(
                state,
                catalog,
                &order,
                DaemonKey::Local,
                started_at,
                8,
                5,
                true,
                &owns,
            );
            assert!(batch.processed <= 13);
            if batch.complete {
                return;
            }
        }
        panic!("project reconciliation did not converge");
    }

    #[test]
    fn bounded_project_batches_reconcile_by_id_and_preserve_other_owners_references_and_local_creations()
     {
        let mut state = PersistedState::empty();
        state.projects = (1..=64).map(project).collect();
        state.projects[0].starred = true;
        // A referenced project missing from the remote snapshot must survive.
        let referenced = state.projects[63].id;
        state
            .sessions
            .push(AgentSession::new(referenced, ProviderKind::Codex));
        state.merge_session_catalog(Vec::new(), |_| false, |_| false, false);
        let foreign = project(900);
        let foreign_id = foreign.id;
        state.projects.push(foreign);
        let catalog = TaskCatalog {
            projects: (1..=512).filter(|id| *id != 64).map(project).collect(),
            sessions: Vec::new(),
        };
        let order = CatalogSessionOrder::prepare(&catalog);
        let mut progress = CatalogProjectProgress::start(&order, None);
        let first = progress.step(
            &mut state,
            &catalog,
            &order,
            DaemonKey::Local,
            Instant::now(),
            8,
            5,
            true,
            |_| true,
        );
        assert!(!first.complete);
        assert!(first.processed <= 13);
        assert!(!state.project_catalog_index_ready());
        // Same-length replacement during partial indexing must rebuild safely.
        state.projects.remove(10);
        let local = project(1000);
        let local_id = local.id;
        state.push_project(local);
        let mut referenced_early = false;
        for _ in 0..4096 {
            let mut batch = progress.step(
                &mut state,
                &catalog,
                &order,
                DaemonKey::Local,
                Instant::now(),
                8,
                5,
                true,
                |id| id != foreign_id,
            );
            assert!(batch.processed <= 13);
            if state.project_catalog_index_ready() && !referenced_early {
                // A task can depend on a project far ahead of the fair upsert
                // cursor. The UI installs it before using that task row.
                assert!(state.apply_catalog_project(
                    &catalog,
                    &order,
                    DaemonKey::Local,
                    Uuid::from_u128(512),
                    &mut batch.retired
                ));
                assert_eq!(
                    state.projects[state.project_catalog_index.positions[&Uuid::from_u128(512)]].id,
                    Uuid::from_u128(512)
                );
                referenced_early = true;
            }
            if batch.complete {
                break;
            }
        }
        assert!(referenced_early);
        assert!(state.project_catalog_index_ready());
        assert_eq!(state.projects.len(), 514);
        assert_eq!(
            state
                .projects
                .iter()
                .map(|row| row.id)
                .collect::<HashSet<_>>()
                .len(),
            514
        );
        assert!(state.projects.iter().any(|row| row.id == referenced));
        assert!(state.projects.iter().any(|row| row.id == foreign_id));
        assert!(state.projects.iter().any(|row| row.id == local_id));
        assert!(
            !state
                .projects
                .iter()
                .find(|row| row.id == Uuid::from_u128(1))
                .unwrap()
                .starred
        );
        // A later complete snapshot can remove unreferenced owned rows, while
        // foreign and unreported local rows remain protected.
        state.sessions.clear();
        state.note_session_topology_change();
        state.merge_session_catalog(Vec::new(), |_| false, |_| false, false);
        finish(&mut state, &TaskCatalog::default(), Instant::now(), |id| {
            id != foreign_id
        });
        assert_eq!(
            state
                .projects
                .iter()
                .map(|row| row.id)
                .collect::<HashSet<_>>(),
            HashSet::from([foreign_id, local_id])
        );
    }

    #[test]
    fn interleaved_session_refresh_installs_dependencies_before_retiring_old_projects() {
        let mut state = PersistedState::empty();
        let old = project(1);
        let new = project(128);
        let mut task = AgentSession::new(old.id, ProviderKind::Codex);
        task.title = "resident".into();
        let id = task.id;
        state.projects.push(old);
        state.sessions.push(task.clone());
        task.project_id = new.id;
        let catalog = TaskCatalog {
            projects: (2..=128).map(project).collect(),
            sessions: vec![task.list_projection()],
        };
        let order = CatalogSessionOrder::prepare(&catalog);
        let mut projects = CatalogProjectProgress::start(&order, None);
        let mut sessions = CatalogSessionProgress::start(&order, None);
        let mut sessions_complete = false;
        let mut complete = false;
        for _ in 0..1024 {
            let mut batch = projects.step(
                &mut state,
                &catalog,
                &order,
                DaemonKey::Local,
                Instant::now(),
                4,
                2,
                sessions_complete,
                |_| true,
            );
            assert!(batch.processed <= 6);
            if !sessions_complete {
                assert!(
                    state
                        .projects
                        .iter()
                        .any(|row| row.id == Uuid::from_u128(1))
                );
            }
            if state.project_catalog_index_ready() {
                let session_batch = sessions.step(
                    &mut state,
                    &catalog,
                    &order,
                    false,
                    DaemonKey::Local,
                    Instant::now(),
                    4,
                    2,
                    |_| true,
                    |_| false,
                    |_| false,
                );
                sessions_complete = session_batch.complete;
                for effect in session_batch.effects {
                    if let CatalogSessionEffect::Applied { id, .. } = effect {
                        let project_id = state.catalog_session(id).unwrap().project_id;
                        assert!(state.apply_catalog_project(
                            &catalog,
                            &order,
                            DaemonKey::Local,
                            project_id,
                            &mut batch.retired
                        ));
                        assert!(state.projects.iter().any(|row| row.id == project_id));
                    }
                }
                if sessions_complete && batch.complete {
                    complete = true;
                    break;
                }
            }
        }
        assert!(complete);
        assert_eq!(state.catalog_session(id).unwrap().project_id, new.id);
        assert_eq!(state.projects.len(), 127);
        assert!(
            !state
                .projects
                .iter()
                .any(|row| row.id == Uuid::from_u128(1))
        );
    }

    #[test]
    fn project_cursor_survives_repeated_supersession_and_latest_snapshot_converges() {
        let mut state = PersistedState::empty();
        state.merge_session_catalog(Vec::new(), |_| false, |_| false, false);
        let catalog = TaskCatalog {
            projects: (1..=128).map(project).collect(),
            sessions: Vec::new(),
        };
        let order = CatalogSessionOrder::prepare(&catalog);
        let mut previous = None;
        for _ in 0..256 {
            let mut progress = CatalogProjectProgress::start(&order, previous.as_ref());
            let batch = progress.step(
                &mut state,
                &catalog,
                &order,
                DaemonKey::Local,
                Instant::now(),
                4,
                2,
                false,
                |_| true,
            );
            assert!(batch.processed <= 6);
            previous = Some(progress);
        }
        assert_eq!(
            state.projects.len(),
            128,
            "supersession starved later projects"
        );
        let mut latest = catalog;
        latest.projects.retain(|row| row.id != Uuid::from_u128(1));
        latest.projects[0].name = "latest".into();
        // Last duplicate id wins, as in the original bulk merger.
        let mut duplicate = latest.projects[0].clone();
        duplicate.name = "last duplicate".into();
        latest.projects.push(duplicate);
        finish(&mut state, &latest, Instant::now(), |_| true);
        assert_eq!(state.projects.len(), 127);
        assert_eq!(
            state
                .projects
                .iter()
                .find(|row| row.id == Uuid::from_u128(2))
                .unwrap()
                .name,
            "last duplicate"
        );
    }

    #[test]
    fn local_project_removal_rejects_delayed_results_until_causal_absence_and_preserves_pending_creation()
     {
        let mut state = PersistedState::empty();
        state.merge_session_catalog(Vec::new(), |_| false, |_| false, false);
        let remote = project(1);
        let id = remote.id;
        state.projects.push(remote.clone());
        state.projects.clear();
        state.note_local_project_removed(id, DaemonKey::Local);
        let removed_at = state.locally_removed_projects[&DaemonKey::Local][&id];
        let before = removed_at - Duration::from_secs(1);
        let after = removed_at + Duration::from_secs(1);
        let catalog = TaskCatalog {
            projects: vec![remote.clone()],
            sessions: Vec::new(),
        };
        finish(&mut state, &TaskCatalog::default(), before, |_| true);
        finish(&mut state, &catalog, before, |_| true);
        assert!(state.projects.is_empty());
        finish(&mut state, &TaskCatalog::default(), after, |_| true);
        finish(&mut state, &catalog, after, |_| true);
        assert_eq!(state.projects.len(), 1);
        let pending = project(2);
        let pending_id = pending.id;
        state.push_project(pending.clone());
        finish(&mut state, &catalog, after, |_| true);
        assert!(state.projects.iter().any(|row| row.id == pending_id));
        let adopted = TaskCatalog {
            projects: vec![remote, pending],
            sessions: Vec::new(),
        };
        finish(&mut state, &adopted, after, |_| true);
        finish(&mut state, &catalog, after, |_| true);
        assert!(!state.projects.iter().any(|row| row.id == pending_id));
    }
}
