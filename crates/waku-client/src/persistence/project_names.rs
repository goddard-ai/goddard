//! Collect and apply project labels cooperatively; resolve collisions on a worker.
use super::*;
use std::collections::LinkedList;

struct ProjectNameInput {
    id: Uuid,
    name: String,
    path: PathBuf,
}

pub struct ProjectNameSnapshot {
    revision: u64,
    // Fixed-size chunks avoid growing/copying one catalog-sized Vec on the UI.
    chunks: LinkedList<Vec<ProjectNameInput>>,
    collected: usize,
}

impl Drop for ProjectNameSnapshot {
    fn drop(&mut self) {
        if !self.chunks.is_empty() {
            super::task_writer::retire(std::mem::take(&mut self.chunks));
        }
    }
}

pub struct ProjectNameLabels {
    revision: u64,
    rows: Vec<(Uuid, Option<String>)>,
    next: usize,
}

impl Drop for ProjectNameLabels {
    fn drop(&mut self) {
        if self.rows.capacity() != 0 {
            super::task_writer::retire(std::mem::take(&mut self.rows));
        }
    }
}

impl ProjectNameSnapshot {
    /// Run on a worker. Use the shared resolver so collision/path semantics
    /// remain identical to other clients; no bookmarks or other metadata travel.
    pub fn resolve(mut self) -> ProjectNameLabels {
        let chunks = std::mem::take(&mut self.chunks);
        let mut projects = chunks
            .into_iter()
            .flatten()
            .map(|input| {
                let mut project = Project::from_path(input.path);
                project.id = input.id;
                project.name = input.name;
                project
            })
            .collect::<Vec<_>>();
        Project::resolve_display_names(&mut projects);
        ProjectNameLabels {
            revision: self.revision,
            rows: projects
                .into_iter()
                .map(|project| (project.id, project.resolved_name))
                .collect(),
            next: 0,
        }
    }
}

#[derive(Default)]
pub struct ProjectNameRefresh {
    applied_revision: Option<u64>,
    collecting: Option<ProjectNameSnapshot>,
    in_flight: Option<u64>,
    labels: Option<ProjectNameLabels>,
}

#[derive(Default)]
pub struct ProjectNameStep {
    pub changed: bool,
    pub processed: usize,
    pub snapshot: Option<ProjectNameSnapshot>,
}

impl ProjectNameRefresh {
    /// A running resolver wakes the owner on completion; do not poll it at the
    /// UI work cadence while there is no collection/application work to do.
    pub fn needs_tick(&self, state: &PersistedState) -> bool {
        self.in_flight.is_none() && self.applied_revision != Some(state.project_names_revision)
    }

    /// Only one resolver may run at a time, including when inputs change.
    pub fn accept(&mut self, labels: ProjectNameLabels) {
        if self.in_flight == Some(labels.revision) {
            self.in_flight = None;
            self.labels = Some(labels);
        }
    }

    pub fn step(&mut self, state: &mut PersistedState, budget: usize) -> ProjectNameStep {
        let mut step = ProjectNameStep::default();
        let revision = state.project_names_revision;
        if let Some(mut labels) = self.labels.take() {
            if labels.revision == revision {
                let end = labels.next.saturating_add(budget).min(labels.rows.len());
                for index in labels.next..end {
                    let (id, label) = &mut labels.rows[index];
                    let Some(project) = state
                        .projects
                        .get_mut(index)
                        .filter(|project| project.id == *id)
                    else {
                        // Defend against a missed direct-mutation hook. Never
                        // apply positional labels to a replacement identity.
                        state.note_project_topology_change();
                        return step;
                    };
                    step.processed += 1;
                    if project.resolved_name != *label {
                        project.resolved_name = label.take();
                        step.changed = true;
                    }
                }
                labels.next = end;
                if end == labels.rows.len() {
                    self.applied_revision = Some(revision);
                } else {
                    self.labels = Some(labels);
                }
                return step;
            }
        }
        if self.applied_revision == Some(revision) || self.in_flight.is_some() {
            return step;
        }
        if self
            .collecting
            .as_ref()
            .is_none_or(|snapshot| snapshot.revision != revision)
        {
            self.collecting = Some(ProjectNameSnapshot {
                revision,
                chunks: LinkedList::new(),
                collected: 0,
            });
        }
        let snapshot = self.collecting.as_mut().expect("name collection exists");
        let end = snapshot
            .collected
            .saturating_add(budget)
            .min(state.projects.len());
        let chunk = state.projects[snapshot.collected..end]
            .iter()
            .map(|project| ProjectNameInput {
                id: project.id,
                name: project.name.clone(),
                path: project.path.clone(),
            })
            .collect::<Vec<_>>();
        step.processed = end - snapshot.collected;
        if !chunk.is_empty() {
            snapshot.chunks.push_back(chunk);
        }
        snapshot.collected = end;
        if end == state.projects.len() {
            if end == 0 {
                self.collecting = None;
                self.applied_revision = Some(revision);
            } else {
                self.in_flight = Some(revision);
                step.snapshot = self.collecting.take();
            }
        }
        step
    }
}

impl PersistedState {
    /// Call after changing a project's name or path directly. Membership and
    /// ordering hooks already invalidate names; other metadata must not do so.
    pub fn note_project_names_change(&mut self) {
        self.project_names_revision = self.project_names_revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(path: &str) -> Project {
        Project::from_path(PathBuf::from(path))
    }

    fn snapshot(
        refresh: &mut ProjectNameRefresh,
        state: &mut PersistedState,
        budget: usize,
    ) -> ProjectNameSnapshot {
        for _ in 0..4096 {
            let step = refresh.step(state, budget);
            assert!(step.processed <= budget);
            if let Some(snapshot) = step.snapshot {
                return snapshot;
            }
        }
        panic!("name collection failed to converge");
    }

    fn finish(refresh: &mut ProjectNameRefresh, state: &mut PersistedState, budget: usize) {
        for _ in 0..4096 {
            if !refresh.needs_tick(state) {
                return;
            }
            let step = refresh.step(state, budget);
            assert!(step.processed <= budget);
            if let Some(snapshot) = step.snapshot {
                refresh.accept(snapshot.resolve());
            }
        }
        panic!("name application failed to converge");
    }

    fn assert_shared_labels(state: &PersistedState) {
        let mut expected = state.projects.clone();
        Project::resolve_display_names(&mut expected);
        assert_eq!(
            state
                .projects
                .iter()
                .map(|row| (&row.id, &row.resolved_name))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|row| (&row.id, &row.resolved_name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn bounded_name_collection_and_application_match_shared_collision_semantics() {
        let mut state = PersistedState::empty();
        state.projects = (0..512)
            .map(|id| project(&format!("/work/{id}/widget")))
            .collect();
        // Duplicate registrations share a label. A renamed project preserves
        // its explicit name; the default name need not match the basename.
        state.projects.push(project("/work/0/widget"));
        let mut renamed = project("/other/source");
        renamed.name = "widget".into();
        state.projects.push(renamed);
        state.projects.push(project("/work/unique"));
        let mut refresh = ProjectNameRefresh::default();
        let first = refresh.step(&mut state, 7);
        assert_eq!(first.processed, 7);
        assert!(first.snapshot.is_none());
        let snapshot = snapshot(&mut refresh, &mut state, 7);
        assert!(
            !refresh.needs_tick(&state),
            "running name work should not require UI polling"
        );
        let idle = refresh.step(&mut state, 7);
        assert_eq!(idle.processed, 0);
        assert!(idle.snapshot.is_none());
        refresh.accept(snapshot.resolve());
        let first_labels = refresh.step(&mut state, 7);
        assert_eq!(first_labels.processed, 7);
        assert!(first_labels.changed);
        assert!(refresh.needs_tick(&state));
        finish(&mut refresh, &mut state, 7);
        assert_shared_labels(&state);
        assert!(!refresh.step(&mut state, 7).changed);
    }

    #[test]
    fn name_changes_revoke_collection_running_results_and_partially_applied_labels() {
        let mut state = PersistedState::empty();
        state.projects = vec![
            project("/one/widget"),
            project("/two/widget"),
            project("/three/widget"),
        ];
        let mut refresh = ProjectNameRefresh::default();
        assert_eq!(refresh.step(&mut state, 1).processed, 1);
        state.projects[0].path = PathBuf::from("/relocated/widget");
        state.note_project_names_change();
        let stale = snapshot(&mut refresh, &mut state, 1);
        state.projects[1].name = "different".into();
        state.note_project_names_change();
        // Only one worker runs even when newer inputs arrive.
        assert!(!refresh.needs_tick(&state));
        assert!(refresh.step(&mut state, 1).snapshot.is_none());
        refresh.accept(stale.resolve());
        let discarded = refresh.step(&mut state, 1);
        assert!(
            !discarded.changed,
            "obsolete labels escaped the revision fence"
        );
        let current = snapshot(&mut refresh, &mut state, 1);
        refresh.accept(current.resolve());
        assert!(refresh.step(&mut state, 1).changed);
        // Replace and reorder after a prefix was applied. Old suffixes cannot
        // rename replacement identities; the new plan must revisit the prefix.
        state.projects.swap_remove(1);
        state.push_project(project("/new/widget"));
        let replacement_id = state.projects[2].id;
        finish(&mut refresh, &mut state, 1);
        assert_eq!(state.projects[2].id, replacement_id);
        assert_shared_labels(&state);
        state.projects.clear();
        state.note_project_topology_change();
        finish(&mut refresh, &mut state, 1);
        assert!(!refresh.needs_tick(&state));
    }

    #[test]
    fn metadata_only_catalog_refreshes_preserve_name_work_and_derived_labels() {
        let mut state = PersistedState::empty();
        state.projects = vec![project("/one/widget"), project("/two/widget")];
        let mut refresh = ProjectNameRefresh::default();
        let running = snapshot(&mut refresh, &mut state, 1);
        let revision = state.project_names_revision;
        let mut catalog = TaskCatalog {
            projects: state.projects.clone(),
            sessions: Vec::new(),
        };
        catalog.projects[0].starred = true;
        let order = CatalogSessionOrder::prepare(&catalog);
        let mut progress = CatalogProjectProgress::start(&order, None);
        for _ in 0..20 {
            progress.step(
                &mut state,
                &catalog,
                &order,
                DaemonKey::Local,
                std::time::Instant::now(),
                8,
                2,
                false,
                |_| true,
            );
        }
        assert_eq!(state.project_names_revision, revision);
        refresh.accept(running.resolve());
        finish(&mut refresh, &mut state, 1);
        assert_shared_labels(&state);
        assert!(state.projects[0].starred);
        let label = state.projects[0].resolved_name.clone();
        let mut retired = Vec::new();
        assert!(state.apply_catalog_project(
            &catalog,
            &order,
            DaemonKey::Local,
            catalog.projects[0].id,
            &mut retired
        ));
        assert_eq!(state.projects[0].resolved_name, label);
        assert!(!refresh.needs_tick(&state));
        // A real path/name change from that same catalog does invalidate names.
        catalog.projects[0].path = PathBuf::from("/moved/widget");
        assert!(state.apply_catalog_project(
            &catalog,
            &order,
            DaemonKey::Local,
            catalog.projects[0].id,
            &mut retired
        ));
        assert!(refresh.needs_tick(&state));
        finish(&mut refresh, &mut state, 1);
        assert_shared_labels(&state);
    }
}
