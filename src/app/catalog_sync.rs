//! Apply session contributions cooperatively. Plain snapshot/index reclamation
//! stays on the background executor; entity-affine cleanup stays on the UI.
use super::*;
use waku_client::DaemonKey;
use waku_client::persistence::{
    CatalogSessionEffect, CatalogSessionOrder, CatalogSessionProgress, CatalogUpdate,
    CatalogVersion,
};

const INDEX_ROWS_PER_TICK: usize = 2048;
const SESSION_ROWS_PER_TICK: usize = 128;

pub(super) struct CatalogApplication {
    executor: BackgroundExecutor,
    version: CatalogVersion,
    started_at: std::time::Instant,
    snapshot: RemoteTaskStateSnapshot,
    order: CatalogSessionOrder,
    progress: CatalogSessionProgress,
    fresh: bool,
    reregistered: HashSet<Uuid>,
    project_removals: Vec<Uuid>,
}

impl Drop for CatalogApplication {
    fn drop(&mut self) {
        let retired = (
            std::mem::take(&mut self.snapshot),
            std::mem::take(&mut self.order),
            std::mem::take(&mut self.reregistered),
            std::mem::take(&mut self.project_removals),
        );
        self.executor
            .spawn(async move {
                drop(retired);
            })
            .detach();
    }
}

pub(super) use waku_client::persistence::CatalogSelectionFallback;
use waku_client::persistence::CatalogSelectionStep;

impl Waku {
    pub(super) fn queue_catalog_application(
        &mut self,
        key: DaemonKey,
        mut update: CatalogUpdate,
        _cx: &mut Context<Self>,
    ) {
        let _timing = waku_client::diagnostics::measure(waku_client::diagnostics::Work::Catalog);
        if let Some(error) = update.error.take() {
            eprintln!("could not refresh daemon task state: {error}");
        }
        let Some(version) = update
            .version
            .filter(|version| self.task_state_sync_events.is_current(*version))
        else {
            self.background_executor
                .spawn(async move { drop(update) })
                .detach();
            return;
        };
        let Some(started_at) = update.started_at else {
            self.background_executor
                .spawn(async move {
                    drop(update);
                })
                .detach();
            return;
        };
        if update.catalog.is_none() || update.session_order.is_none() {
            self.background_executor
                .spawn(async move { drop(update) })
                .detach();
            return;
        }
        let mut snapshot = update.catalog.take().expect("catalog checked above");
        let order = update
            .session_order
            .take()
            .expect("catalog ordering checked above");
        // Project batching is separate from the bounded session path. Applying
        // projects first ensures every arriving task can resolve its owner.
        let project_removals = self
            .state
            .projects
            .iter()
            .filter(|project| {
                self.daemons.project_owner(project.id) == key && !order.contains_project(project.id)
            })
            .map(|project| project.id)
            .collect();
        runtime::merge_remote_project_catalog(
            &mut self.state.projects,
            std::mem::take(&mut snapshot.projects),
            |_| false,
        );
        Project::resolve_display_names(&mut self.state.projects);
        if let DaemonKey::Remote(host) = key {
            self.daemons.note_remote_catalog_loaded(host);
            for id in order.project_ids() {
                self.daemons.claim_project(id, key);
            }
            for project in self.boss_ui.projects.values() {
                if self
                    .boss_ui
                    .states
                    .get(&key)
                    .is_some_and(|state| state.identity.id == project.id)
                {
                    self.daemons.claim_project(project.id, key);
                }
            }
            if let Some(cache) = update.cache_catalog.take() {
                self.update_remote_catalog_cache(
                    host,
                    waku_client::persistence::CatalogCacheUpdate::Replace(cache),
                );
            }
        }
        let mut previous = self.catalog_applications.remove(&key);
        let same_source = previous
            .as_ref()
            .is_some_and(|old| version.same_source(old.version));
        let progress = CatalogSessionProgress::start(
            &order,
            previous
                .as_ref()
                .filter(|_| same_source)
                .map(|old| &old.progress),
        );
        let reregistered = if same_source {
            std::mem::take(&mut previous.as_mut().expect("same source exists").reregistered)
        } else {
            HashSet::new()
        };
        let fresh = update.fresh;
        self.background_executor
            .spawn(async move {
                drop((previous, update));
            })
            .detach();
        self.catalog_applications.insert(
            key,
            CatalogApplication {
                executor: self.background_executor.clone(),
                version,
                started_at,
                snapshot,
                order,
                progress,
                fresh,
                reregistered,
                project_removals,
            },
        );
        if !self.catalog_apply_queue.contains(&key) {
            self.catalog_apply_queue.push_back(key);
        }
    }

    pub(super) fn revoke_catalog_application(&mut self, key: DaemonKey, _cx: &mut Context<Self>) {
        self.catalog_apply_queue.retain(|queued| *queued != key);
        let retired = self.catalog_applications.remove(&key);
        let removals = self.state.revoke_catalog_removals(key);
        self.background_executor
            .spawn(async move {
                drop((retired, removals));
            })
            .detach();
    }

    pub(super) fn apply_catalog_session_batch(&mut self, cx: &mut Context<Self>) -> bool {
        let _timing = (!self.catalog_apply_queue.is_empty()
            || self.catalog_selection_fallback.is_some())
        .then(|| waku_client::diagnostics::measure(waku_client::diagnostics::Work::Catalog));
        let mut changed = false;
        if let Some(key) = self.catalog_apply_queue.pop_front() {
            if let Some(mut application) = self.catalog_applications.remove(&key) {
                if !self.task_state_sync_events.is_current(application.version) {
                    // Keep only its small progress until the queued successor
                    // arrives. Never apply an obsolete batch or consume fresh.
                    if self
                        .task_state_sync_events
                        .source_is_current(application.version)
                    {
                        self.catalog_applications.insert(key, application);
                        self.catalog_apply_queue.push_back(key);
                    } else {
                        self.background_executor
                            .spawn(async move {
                                drop(application);
                            })
                            .detach();
                    }
                } else {
                    let mut batch = application.progress.step(
                        &mut self.state,
                        &application.snapshot,
                        &application.order,
                        application.fresh,
                        key,
                        application.started_at,
                        INDEX_ROWS_PER_TICK,
                        SESSION_ROWS_PER_TICK,
                        |id| {
                            self.daemons.session_owner(id) == key
                                && !self.friend_sessions.contains_key(&id)
                        },
                        |id| self.runtimes.contains_key(&id),
                        |id| self.pending_session_metadata.contains_key(&id),
                    );
                    changed = !batch.effects.is_empty();
                    let mut retired = Vec::new();
                    let mut save = false;
                    for effect in batch.effects {
                        match effect {
                            CatalogSessionEffect::Applied { id, added } => {
                                if matches!(key, DaemonKey::Remote(_)) {
                                    self.daemons.claim_session(id, key);
                                }
                                if let Some(session) = self.state.catalog_session(id) {
                                    if added {
                                        self.track_task_created(session, "external");
                                    }
                                    if session.status.is_busy()
                                        || (self.state.selected_session == Some(id)
                                            && session.has_started())
                                    {
                                        self.start_runtime_attachment(id, cx);
                                    }
                                }
                            }
                            CatalogSessionEffect::Removed(session) => {
                                self.daemons.drop_session(session.id);
                                self.retire_catalog_session_surface(session.id, cx);
                                retired.push(session);
                            }
                            CatalogSessionEffect::Reregister(id) => {
                                if application.reregistered.insert(id) {
                                    self.state.mark_session_dirty(id);
                                    save = true;
                                }
                            }
                        }
                    }
                    if save {
                        // Reuse mandatory save coalescing rather than scan the
                        // whole state once for every small catalog batch.
                        self.stream_state_dirty = true;
                    }
                    let old_index = batch.retired_index.take();
                    self.background_executor
                        .spawn(async move {
                            drop((retired, old_index));
                        })
                        .detach();
                    if batch.complete
                        && self.state.catalog_index_ready()
                        && self.task_state_sync_events.is_current(application.version)
                    {
                        self.finish_catalog_project_removals(key, &application, cx);
                        self.task_state_sync_events.acknowledge(application.version);
                        self.background_executor
                            .spawn(async move {
                                drop(application);
                            })
                            .detach();
                    } else {
                        self.catalog_applications.insert(key, application);
                        self.catalog_apply_queue.push_back(key);
                    }
                }
            }
        }
        self.advance_catalog_selection_fallback(cx) || changed
    }

    fn finish_catalog_project_removals(
        &mut self,
        key: DaemonKey,
        application: &CatalogApplication,
        _cx: &mut Context<Self>,
    ) {
        let removed_ids = application
            .project_removals
            .iter()
            .copied()
            .filter(|id| {
                self.daemons.project_owner(*id) == key
                    && !application.order.contains_project(*id)
                    && !self.state.catalog_project_has_sessions(*id)
            })
            .collect::<HashSet<_>>();
        if removed_ids.is_empty() {
            return;
        }
        let retired = self
            .state
            .projects
            .extract_if(.., |project| removed_ids.contains(&project.id))
            .collect::<Vec<_>>();
        for id in &removed_ids {
            self.daemons.drop_project(*id);
        }
        Project::resolve_display_names(&mut self.state.projects);
        self.background_executor
            .spawn(async move {
                drop((retired, removed_ids));
            })
            .detach();
    }

    fn retire_catalog_session_surface(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.session_metadata_saves.remove(&id);
        self.pending_session_metadata.remove(&id);
        self.runtime_attach_pending.remove(&id);
        self.pending_session_cancels.remove(&id);
        self.runtime_attach_misses.remove(&id);
        if let Some(runtime) = self.runtimes.remove(&id) {
            runtime.retire_payloads(&self.background_executor);
        }
        self.background_work.remove(&id);
        self.remove_right_panel_session_state(id, cx);
        self.remove_side_chat_surface(id, cx);
        self.side_chat_views.remove(&id);
        self.side_chat_composers.remove(&id);
        self.task_switcher.remove(id);
        self.project_switcher.session_removed(id);
        self.transcript_scroll_positions.remove(&id);
        if self.state.selected_session == Some(id) {
            self.state.selected_session = None;
            self.sync_voice_scratchpad_capture(cx);
            self.sync_right_panel_owner(cx);
            self.catalog_selection_fallback =
                Some(CatalogSelectionFallback::new(self.state.selected_project));
        }
    }

    fn advance_catalog_selection_fallback(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(mut fallback) = self.catalog_selection_fallback.take() else {
            return false;
        };
        match fallback.step(
            &self.state.sessions,
            self.state.selected_project,
            self.state.selected_session,
            INDEX_ROWS_PER_TICK,
        ) {
            CatalogSelectionStep::Pending => {
                self.catalog_selection_fallback = Some(fallback);
                return false;
            }
            CatalogSelectionStep::Cancelled => return false,
            CatalogSelectionStep::Select(id) => self.select_session(id, cx),
            CatalogSelectionStep::Empty => {
                if !self.catalog_applications.is_empty() {
                    self.catalog_selection_fallback = Some(fallback);
                    return false;
                }
                if let Some(project) = self
                    .state
                    .selected_project
                    .filter(|id| self.state.projects.iter().any(|project| project.id == *id))
                    .or_else(|| self.state.projects.first().map(|project| project.id))
                {
                    self.state.selected_project = Some(project);
                    self.create_session_for(project, self.state.last_provider, cx);
                }
            }
        }
        self.sync_right_panel_owner(cx);
        true
    }
}
