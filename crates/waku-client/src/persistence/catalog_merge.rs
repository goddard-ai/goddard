//! Cached UUID lookup for list-only catalog reconciliation. Session identity
//! and order changes invalidate the cache; metadata updates leave it usable.
use super::*;

#[derive(Debug, Default)]
pub(super) struct SessionCatalogIndex {
    pub(super) revision: Option<u64>,
    pub(super) indexed_rows: usize,
    pub(super) positions: BTreeMap<Uuid, usize>,
    pub(super) project_counts: BTreeMap<Uuid, usize>,
}
// A state snapshot must not copy a potentially large disposable lookup cache.
impl Clone for SessionCatalogIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Drop for SessionCatalogIndex {
    fn drop(&mut self) {
        if self.positions.is_empty() && self.project_counts.is_empty() {
            return;
        }
        let retired = (
            std::mem::take(&mut self.positions),
            std::mem::take(&mut self.project_counts),
        );
        super::task_writer::retire(retired);
    }
}

impl PersistedState {
    /// Call after changing session identity, membership, order or project directly.
    /// Prefer push_session for ordinary new sessions. Metadata edits do not
    /// change topology and must not continuously invalidate catalog lookup.
    pub fn note_session_topology_change(&mut self) {
        self.session_topology_revision = self.session_topology_revision.wrapping_add(1);
        self.session_catalog_index.revision = None;
    }

    pub fn note_local_session_removed(&mut self, id: Uuid, key: DaemonKey) {
        self.locally_removed_sessions
            .entry(key)
            .or_default()
            .insert(id, std::time::Instant::now());
    }

    pub fn revoke_catalog_removals(
        &mut self,
        key: DaemonKey,
    ) -> Option<BTreeMap<Uuid, std::time::Instant>> {
        self.locally_removed_sessions.remove(&key)
    }

    pub fn note_session_removed(&mut self, id: Uuid) {
        self.unreported_sessions.remove(&id);
        self.note_session_topology_change();
    }

    pub fn merge_session_catalog(
        &mut self,
        mut remote: Vec<AgentSession>,
        owns: impl Fn(Uuid) -> bool,
        has_local_runtime: impl Fn(Uuid) -> bool,
        fresh: bool,
    ) -> Vec<AgentSession> {
        let valid = self.session_catalog_index.revision == Some(self.session_topology_revision)
            && self.session_catalog_index.indexed_rows == self.sessions.len();
        remote.retain(|row| {
            !self
                .locally_removed_sessions
                .values()
                .any(|removed| removed.contains_key(&row.id))
        });
        for row in &remote {
            self.unreported_sessions.remove(&row.id);
        }
        let unreported = &self.unreported_sessions;
        let previous_len = self.sessions.len();
        let removed = merge_indexed(
            &mut self.sessions,
            remote,
            |id| owns(id) && !unreported.contains(&id),
            has_local_runtime,
            fresh,
            &mut self.session_catalog_index.positions,
            &mut self.session_catalog_index.project_counts,
            valid,
        );
        if !removed.is_empty() || self.sessions.len() != previous_len {
            self.session_topology_revision = self.session_topology_revision.wrapping_add(1);
        }
        self.session_catalog_index.revision = Some(self.session_topology_revision);
        self.session_catalog_index.indexed_rows = self.sessions.len();
        removed
    }
}

pub fn merge_remote_session_catalog(
    local: &mut Vec<AgentSession>,
    remote: Vec<AgentSession>,
    owns: impl Fn(Uuid) -> bool,
    has_local_runtime: impl Fn(Uuid) -> bool,
    fresh: bool,
) -> Vec<AgentSession> {
    merge_indexed(
        local,
        remote,
        owns,
        has_local_runtime,
        fresh,
        &mut BTreeMap::new(),
        &mut BTreeMap::new(),
        false,
    )
}

fn merge_indexed(
    local: &mut Vec<AgentSession>,
    remote: Vec<AgentSession>,
    owns: impl Fn(Uuid) -> bool,
    has_local_runtime: impl Fn(Uuid) -> bool,
    fresh: bool,
    positions: &mut BTreeMap<Uuid, usize>,
    project_counts: &mut BTreeMap<Uuid, usize>,
    valid: bool,
) -> Vec<AgentSession> {
    let remote_ids = remote
        .iter()
        .map(|session| session.id)
        .collect::<HashSet<_>>();
    // Re-registration needs the transcript, so only detail-loaded copies
    // survive; an incognito skeleton learned from another client comes back
    // through the catalog when its owner re-pushes it.
    let survives_restart =
        |session: &AgentSession| fresh && session.incognito && session.detail_loaded;
    // Return removed records without dropping their potentially large
    // transcripts. UI callers transfer them to a reclamation worker.
    let removed = local
        .extract_if(.., |session| {
            session.has_started()
                && owns(session.id)
                && !remote_ids.contains(&session.id)
                && !survives_restart(session)
        })
        .collect::<Vec<_>>();

    // Build once after retention: finding every remote row by a fresh scan
    // made each catalog refresh quadratic in the number of tasks.
    if !valid || !removed.is_empty() {
        positions.clear();
        project_counts.clear();
        for (index, session) in local.iter().enumerate() {
            positions.entry(session.id).or_insert(index);
            *project_counts.entry(session.project_id).or_default() += 1;
        }
    }
    for remote in remote {
        if let Some(&index) = positions.get(&remote.id) {
            let local = &mut local[index];
            let previous_project = local.project_id;
            merge_session_catalog_row(local, remote, has_local_runtime(local.id));
            if local.project_id != previous_project {
                decrement_project_reference(project_counts, previous_project);
                *project_counts.entry(local.project_id).or_default() += 1;
            }
        } else {
            positions.insert(remote.id, local.len());
            *project_counts.entry(remote.project_id).or_default() += 1;
            local.push(remote);
        }
    }

    removed
}

pub(super) fn decrement_project_reference(counts: &mut BTreeMap<Uuid, usize>, id: Uuid) {
    if let Some(count) = counts.get_mut(&id) {
        *count -= 1;
        if *count == 0 {
            counts.remove(&id);
        }
    }
}

pub fn merge_session_catalog_row(
    local: &mut AgentSession,
    remote: AgentSession,
    has_local_runtime: bool,
) {
    if remote.metadata_revision >= local.metadata_revision {
        local.title = remote.title;
        local.archived_at = remote.archived_at;
        local.pinned_at = remote.pinned_at;
        local.metadata_revision = remote.metadata_revision;
        local.metadata_mutation_id = remote.metadata_mutation_id;
    }
    local.auto_title = remote.auto_title;
    local.project_id = remote.project_id;
    // A snapshot that predates the confirmed activation cannot own
    // the effective fields — its provider/model/effort are
    // pre-activation requested state. The mark itself merges
    // forward only, so the snapshot cannot lower the local
    // high-water and reopen stale redeliveries either.
    if remote.config_activation >= local.config_activation {
        local.provider = remote.provider;
        local.model = remote.model;
        local.reasoning_effort = remote.reasoning_effort;
    }
    local.config_activation = local.config_activation.max(remote.config_activation);
    local.created_at = remote.created_at;
    local.last_reply_at = remote.last_reply_at;
    local.dormant_at = remote.dormant_at;
    local.dormant_exempt_until = remote.dormant_exempt_until;
    local.side_chat_of = remote.side_chat_of;
    // A hydrated session's workspace is at least as fresh as the
    // stored column the projection carries — and may hold an unsaved
    // move — so only skeletons adopt it.
    if !local.detail_loaded {
        local.workspace = remote.workspace;
    }
    if !has_local_runtime {
        local.status = remote.status;
        local.updated_at = remote.updated_at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::model::{
        ConfigActivation, Message, MessageRole, SessionStatus, SessionWorkspace,
    };
    #[test]
    fn remote_task_catalog_adds_web_tasks_without_replacing_hydrated_detail() {
        let project_id = Uuid::new_v4();
        let mut local = AgentSession::new(project_id, ProviderKind::Codex);
        local.title = "Local title".into();
        local
            .messages
            .push(Message::new(MessageRole::User, "keep this transcript"));
        let local_id = local.id;

        let mut local_projection = local.list_projection();
        local_projection.title = "Renamed elsewhere".into();
        local_projection.status = SessionStatus::Waiting;
        local_projection.updated_at += 10;

        let mut web_task = AgentSession::new(project_id, ProviderKind::Claude).list_projection();
        web_task.title = "Created in Web".into();
        let web_task_id = web_task.id;

        let mut catalog = vec![local];
        let removed = merge_remote_session_catalog(
            &mut catalog,
            vec![local_projection, web_task],
            |_| false,
            |_| false,
            false,
        );

        assert!(removed.is_empty());
        assert_eq!(catalog.len(), 2);
        let merged_local = catalog
            .iter()
            .find(|session| session.id == local_id)
            .unwrap();
        assert_eq!(merged_local.title, "Renamed elsewhere");
        assert_eq!(merged_local.status, SessionStatus::Waiting);
        assert_eq!(merged_local.messages.len(), 1);
        assert_eq!(merged_local.messages[0].content, "keep this transcript");
        assert!(catalog.iter().any(|session| session.id == web_task_id));
    }

    #[test]
    fn employee_catalog_selection_updates_hydrated_live_and_reconnected_clients() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Devin);
        session.boss_managed = true;
        session.begin_turn("Keep the employee transcript");
        let mut client = vec![session.clone()];
        session.provider = ProviderKind::Codex;
        session.model = Some("gpt-6-luna".into());
        session.reasoning_effort = Some("max".into());
        for (fresh, live, effort) in [(false, true, Some("max")), (true, false, None)] {
            session.reasoning_effort = effort.map(str::to_owned);
            merge_remote_session_catalog(
                &mut client,
                vec![session.list_projection()],
                |_| true,
                |_| live,
                fresh,
            );
            let reflected = &client[0];
            assert_eq!(reflected.provider, session.provider);
            assert_eq!(reflected.model, session.model);
            assert_eq!(reflected.reasoning_effort, session.reasoning_effort);
            assert!(reflected.detail_loaded);
            assert_eq!(reflected.messages.len(), session.messages.len());
            assert_eq!(reflected.messages[0].content, session.messages[0].content);
        }
    }

    #[test]
    fn catalog_merge_carries_the_confirmed_activation_mark_forward() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.model = Some("gpt-6".into());
        session.reasoning_effort = Some("low".into());
        session.config_activation = Some(ConfigActivation {
            generation: 1,
            revision: 5,
        });
        let mut client = vec![session.clone()];

        // A snapshot built before the activation owns no effective fields and
        // cannot lower the mark — the stale redeliveries it would unlock stay
        // rejected.
        let mut stale_projection = session.list_projection();
        stale_projection.model = Some("gpt-5.5".into());
        stale_projection.reasoning_effort = Some("high".into());
        stale_projection.config_activation = Some(ConfigActivation {
            generation: 1,
            revision: 2,
        });
        merge_remote_session_catalog(
            &mut client,
            vec![stale_projection],
            |_| true,
            |_| false,
            true,
        );
        assert_eq!(client[0].model.as_deref(), Some("gpt-6"));
        assert_eq!(client[0].reasoning_effort.as_deref(), Some("low"));
        assert_eq!(
            client[0].config_activation,
            Some(ConfigActivation {
                generation: 1,
                revision: 5
            })
        );

        // A snapshot carrying a newer activation adopts it together with the
        // confirmed fields.
        let mut newer = session.clone();
        newer.model = Some("gpt-6.1".into());
        newer.reasoning_effort = Some("high".into());
        newer.config_activation = Some(ConfigActivation {
            generation: 1,
            revision: 6,
        });
        merge_remote_session_catalog(
            &mut client,
            vec![newer.list_projection()],
            |_| true,
            |_| false,
            true,
        );
        assert_eq!(client[0].model.as_deref(), Some("gpt-6.1"));
        assert_eq!(client[0].reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            client[0].config_activation,
            Some(ConfigActivation {
                generation: 1,
                revision: 6
            })
        );
    }

    #[test]
    fn remote_task_catalog_adopts_workspace_for_skeletons_only() {
        let project_id = Uuid::new_v4();
        let worktree = SessionWorkspace::Worktree {
            path: std::path::PathBuf::from("/tmp/worktrees/task"),
            name: "task".into(),
            branch: Some("waku/task".into()),
            base_branch: None,
            adopted_by: None,
            borrowed: false,
        };

        // A skeleton row has no workspace of its own beyond what the daemon
        // stored — the projection's column is authoritative for it.
        let skeleton = AgentSession::new(project_id, ProviderKind::Codex).list_projection();
        assert_eq!(skeleton.workspace, SessionWorkspace::Local);
        let skeleton_id = skeleton.id;
        let mut remote = skeleton.clone();
        remote.workspace = worktree.clone();

        // A hydrated session may hold an unsaved move; the projection must not
        // clobber it.
        let mut hydrated = AgentSession::new(project_id, ProviderKind::Codex);
        hydrated.workspace = worktree.clone();
        let hydrated_id = hydrated.id;
        let mut stale_remote = hydrated.list_projection();
        stale_remote.workspace = SessionWorkspace::Local;

        let mut catalog = vec![skeleton, hydrated];
        merge_remote_session_catalog(
            &mut catalog,
            vec![remote, stale_remote],
            |_| false,
            |_| false,
            false,
        );

        assert_eq!(
            catalog
                .iter()
                .find(|session| session.id == skeleton_id)
                .map(|session| &session.workspace),
            Some(&worktree)
        );
        assert_eq!(
            catalog
                .iter()
                .find(|session| session.id == hydrated_id)
                .map(|session| &session.workspace),
            Some(&worktree)
        );
    }

    #[test]
    fn fresh_catalog_keeps_incognito_sessions_but_still_removes_ordinary_ones() {
        let project_id = Uuid::new_v4();
        // A daemon restart: the fresh catalog knows only what the store held,
        // so both of these rows are absent from it.
        let mut incognito = AgentSession::new(project_id, ProviderKind::Codex);
        incognito.incognito = true;
        incognito.detail_loaded = true;
        incognito.begin_turn("off the record");
        let incognito_id = incognito.id;

        let mut ordinary = AgentSession::new(project_id, ProviderKind::Codex);
        ordinary.detail_loaded = true;
        ordinary.begin_turn("on the record");
        let ordinary_id = ordinary.id;

        // The incognito skeleton another client learned about is not kept — it
        // lacks the transcript a re-push needs and returns with its owner's.
        let mut skeleton = incognito.list_projection();
        skeleton.id = Uuid::new_v4();
        let skeleton_id = skeleton.id;

        let mut catalog = vec![incognito, ordinary, skeleton];
        let removed =
            merge_remote_session_catalog(&mut catalog, Vec::new(), |_| true, |_| false, true);

        assert_eq!(
            removed.iter().map(|session| session.id).collect::<Vec<_>>(),
            vec![ordinary_id, skeleton_id]
        );
        assert_eq!(removed[0].messages[0].content, "on the record");
        assert_eq!(catalog.len(), 1);
        assert!(catalog.iter().any(|session| session.id == incognito_id));
    }

    #[test]
    fn non_fresh_catalog_removes_incognito_sessions_like_any_other() {
        let project_id = Uuid::new_v4();
        // Same-daemon revision: a deleted incognito task stays deleted — the
        // restart survival is not a resurrection.
        let mut incognito = AgentSession::new(project_id, ProviderKind::Codex);
        incognito.incognito = true;
        incognito.detail_loaded = true;
        incognito.begin_turn("off the record");
        let incognito_id = incognito.id;

        let mut catalog = vec![incognito];
        let removed =
            merge_remote_session_catalog(&mut catalog, Vec::new(), |_| true, |_| false, false);

        assert_eq!(
            removed.iter().map(|session| session.id).collect::<Vec<_>>(),
            vec![incognito_id]
        );
        assert!(catalog.is_empty());
    }

    #[test]
    fn cached_catalog_lookup_tracks_same_length_local_replacement_and_remote_removal() {
        let project = Uuid::new_v4();
        let mut state = PersistedState::empty();
        let mut first = AgentSession::new(project, ProviderKind::Codex);
        first.begin_turn("first");
        let mut second = AgentSession::new(project, ProviderKind::Codex);
        second.begin_turn("keep resident history");
        let second_id = second.id;
        state.push_session(first);
        state.push_session(second);
        let original = state.sessions[1].list_projection();
        state.merge_session_catalog(vec![original.clone()], |_| false, |_| false, false);
        // Keep the length unchanged while shifting every positional lookup.
        let retired = state.sessions.remove(0);
        let mut replacement = AgentSession::new(project, ProviderKind::Claude);
        replacement.title = "replacement".into();
        let replacement_id = replacement.id;
        state.push_session(replacement);
        drop(retired);
        let expected_status = state.sessions[0].status;
        let expected_updated_at = state.sessions[0].updated_at;
        let mut incoming = original;
        incoming.title = "renamed remote".into();
        incoming.status = SessionStatus::Waiting;
        incoming.updated_at += 100;
        state.merge_session_catalog(vec![incoming], |_| false, |id| id == second_id, false);
        assert_eq!(state.sessions[0].id, second_id);
        assert_eq!(state.sessions[0].title, "renamed remote");
        assert_eq!(state.sessions[0].status, expected_status);
        assert_eq!(state.sessions[0].updated_at, expected_updated_at);
        assert_eq!(
            state.sessions[0].messages[0].content,
            "keep resident history"
        );
        assert_eq!(state.sessions[1].id, replacement_id);
        assert_eq!(state.sessions[1].title, "replacement");
        let mut projection = state.sessions[1].list_projection();
        projection.title = "replacement renamed".into();
        let removed =
            state.merge_session_catalog(vec![projection], |id| id == second_id, |_| false, false);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, second_id);
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].id, replacement_id);
        assert_eq!(state.sessions[0].title, "replacement renamed");
        let mut later = state.sessions[0].list_projection();
        later.title = "latest".into();
        state.merge_session_catalog(vec![later], |_| false, |_| false, false);
        assert_eq!(state.sessions[0].title, "latest");
    }
}

/// Preserve resident detail and newer list edits; abandoned detail and the
/// replaced skeleton retain background ownership rather than dropping on UI.
pub fn merge_hydrated_session(
    existing: &mut AgentSession,
    hydrated: BackgroundOwned<AgentSession>,
) -> bool {
    if existing.detail_loaded {
        return false;
    }
    let columns = existing.list_projection();
    let mut hydrated = hydrated.into_inner();
    merge_session_catalog_row(&mut hydrated, columns, false);
    let retired = std::mem::replace(existing, hydrated);
    drop(BackgroundOwned::new(retired));
    true
}

#[cfg(test)]
mod hydration_tests {
    use super::*;
    use waku_protocol::model::ConfigActivation;
    #[test]
    fn startup_hydration_keeps_newer_list_edits_and_live_transcript_detail() {
        let mut stored = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        stored.begin_turn("Stored prompt");
        stored.config_activation = Some(ConfigActivation {
            generation: 1,
            revision: 1,
        });
        let mut current = stored.list_projection();
        let current_project = Uuid::new_v4();
        current.project_id = current_project;
        current.metadata_revision = 10;
        current.title = "Renamed while loading".into();
        current.archived_at = Some(42);
        current.provider = ProviderKind::Claude;
        current.model = Some("newer selection".into());
        current.config_activation = Some(ConfigActivation {
            generation: 1,
            revision: 2,
        });

        merge_hydrated_session(&mut current, BackgroundOwned::new(stored.clone()));
        assert!(current.detail_loaded);
        assert_eq!(current.title, "Renamed while loading");
        assert_eq!(current.project_id, current_project);
        assert_eq!(current.metadata_revision, 10);
        assert_eq!(current.archived_at, Some(42));
        assert_eq!(current.provider, ProviderKind::Claude);
        assert_eq!(current.model.as_deref(), Some("newer selection"));
        assert_eq!(current.messages[0].content, "Stored prompt");

        current.begin_turn("Newer live prompt");
        let before = serde_json::to_value(&current).unwrap();
        merge_hydrated_session(&mut current, BackgroundOwned::new(stored));
        assert_eq!(serde_json::to_value(&current).unwrap(), before);
    }
}
