use super::*;
use waku_protocol::task_metadata::{TaskMetadataEdit, TaskMetadataField};

#[derive(Clone)]
pub(super) struct MetadataRequest {
    session_id: Uuid,
    sequence: u64,
    edited_at: u64,
    edit: TaskMetadataEdit,
    observed: Option<(u64, TaskMetadataEdit)>,
}

#[derive(Default)]
pub(super) struct PendingTaskMetadata {
    sequence: u64,
    pub(super) edits: HashMap<(Uuid, TaskMetadataField), MetadataRequest>,
}

impl PendingTaskMetadata {
    fn record(&mut self, session: &AgentSession, field: TaskMetadataField) -> MetadataRequest {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("metadata edit sequence exhausted");
        let mut edit = TaskMetadataEdit::capture(field, session);
        if let TaskMetadataEdit::Placement { dormant_at, .. } = &mut edit {
            // Expired stamps are history, not an intent to make the task dormant.
            *dormant_at = dormant_at.filter(|at| *at >= session.updated_at);
        }
        let request = MetadataRequest {
            session_id: session.id,
            sequence: self.sequence,
            edited_at: session.updated_at,
            edit,
            observed: None,
        };
        self.edits.insert((session.id, field), request.clone());
        request
    }

    fn current(&self, request: &MetadataRequest) -> bool {
        self.edits
            .get(&(request.session_id, request.edit.field()))
            .is_some_and(|current| current.sequence == request.sequence)
    }

    pub(super) fn remove(&mut self, id: &Uuid) {
        self.edits.retain(|(session_id, _), _| session_id != id);
    }

    pub(super) fn is_archiving(&self, id: &Uuid) -> bool {
        self.edits
            .get(&(*id, TaskMetadataField::Archive))
            .is_some_and(|request| {
                matches!(
                    request.edit,
                    TaskMetadataEdit::Archive {
                        archived_at: Some(_)
                    }
                )
            })
    }

    pub(super) fn archiving_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.edits.iter().filter_map(|((id, _), request)| {
            matches!(
                request.edit,
                TaskMetadataEdit::Archive {
                    archived_at: Some(_)
                }
            )
            .then_some(*id)
        })
    }

    pub(super) fn observe_raw(&mut self, session: &AgentSession) {
        for field in [
            TaskMetadataField::Title,
            TaskMetadataField::Placement,
            TaskMetadataField::Archive,
        ] {
            if let Some(request) = self.edits.get_mut(&(session.id, field))
                && request
                    .observed
                    .as_ref()
                    .is_none_or(|(revision, _)| *revision <= session.metadata_revision)
            {
                request.observed = Some((
                    session.metadata_revision,
                    TaskMetadataEdit::capture(field, session),
                ));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn observe(&mut self, session: &mut AgentSession) {
        self.observe_raw(session);
        self.overlay(session);
    }

    pub(super) fn overlay(&self, session: &mut AgentSession) {
        // Pinning and dormancy share one atomic edit, so a later pin also
        // supersedes a failed dormant transition's unpin on retry.
        for field in [
            TaskMetadataField::Title,
            TaskMetadataField::Placement,
            TaskMetadataField::Archive,
        ] {
            if let Some(request) = self.edits.get(&(session.id, field)) {
                request.edit.apply(session);
                session.updated_at = session.updated_at.max(request.edited_at);
                if let TaskMetadataEdit::Placement {
                    dormant_at: Some(at),
                    ..
                } = request.edit
                {
                    session.dormant_at = Some(at.max(session.updated_at));
                }
            }
        }
    }

    pub(super) fn acknowledge(
        &mut self,
        request: &MetadataRequest,
        saved: &AgentSession,
        local: &mut AgentSession,
    ) {
        local.metadata_revision = local.metadata_revision.max(saved.metadata_revision);
        if self.current(request) {
            let pending = self
                .edits
                .remove(&(request.session_id, request.edit.field()))
                .expect("current metadata edit");
            // A catalog may have observed another client's later edit while
            // this acknowledgement was queued. Release to that newer value.
            let (revision, confirmed) = pending
                .observed
                .filter(|(revision, _)| *revision > saved.metadata_revision)
                .unwrap_or_else(|| {
                    (
                        saved.metadata_revision,
                        TaskMetadataEdit::capture(request.edit.field(), saved),
                    )
                });
            local.metadata_revision = local.metadata_revision.max(revision);
            confirmed.apply(local);
        }
        self.overlay(local);
    }
}

impl Waku {
    pub(super) fn record_task_metadata(&mut self, id: Uuid, field: TaskMetadataField) {
        let Some(session) = self.state.sessions.iter().find(|session| session.id == id) else {
            return;
        };
        // Draft names remain local until the first prompt creates a daemon row.
        if !session.has_started() && !session.boss_managed {
            return;
        }
        let request = self.pending_task_metadata.record(session, field);
        let key = self.daemons.session_owner(id);
        let edit = request.edit;
        self.sidebar_intents.record(
            key,
            waku_protocol::sidebar_intent::SidebarEdit::Task { id, edit },
            None,
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::runtime::merge_remote_session_catalog;

    #[test]
    fn task_metadata_waits_for_the_latest_acknowledgement_and_rejects_old_catalogs() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Task");
        let stale = session.list_projection();
        let mut pending = PendingTaskMetadata::default();
        // Pin -> unpin -> pin within one clock tick. A snapshot matching an
        // intermediate value must never be mistaken for an acknowledgement.
        session.pinned_at = Some(1);
        let first = pending.record(&session, TaskMetadataField::Placement);
        session.pinned_at = None;
        let second = pending.record(&session, TaskMetadataField::Placement);
        session.pinned_at = Some(2);
        let third = pending.record(&session, TaskMetadataField::Placement);
        let mut remote = stale.clone();
        pending.observe(&mut remote);
        assert_eq!(remote.pinned_at, Some(2));
        assert!(pending.current(&third));

        for (request, pin, revision) in [
            (&first, Some(1), 1),
            (&second, None, 2),
            (&third, Some(2), 3),
        ] {
            let mut saved = stale.clone();
            saved.pinned_at = pin;
            saved.metadata_revision = revision;
            pending.acknowledge(request, &saved, &mut session);
            assert_eq!(session.pinned_at, Some(2));
        }
        assert!(pending.edits.is_empty());
        let mut catalog = vec![session];
        merge_remote_session_catalog(
            &mut catalog,
            vec![stale.clone()],
            |_| true,
            |_| false,
            false,
            &HashSet::new(),
        );
        assert_eq!(catalog[0].pinned_at, Some(2));

        let mut changed_elsewhere = stale;
        changed_elsewhere.metadata_revision = 4;
        merge_remote_session_catalog(
            &mut catalog,
            vec![changed_elsewhere],
            |_| true,
            |_| false,
            false,
            &HashSet::new(),
        );
        assert_eq!(catalog[0].pinned_at, None);
    }

    #[test]
    fn task_metadata_keeps_independent_edits_and_supersedes_dormancy_retries() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Task");
        let mut pending = PendingTaskMetadata::default();
        session.set_title_from_user("Renamed");
        let rename = pending.record(&session, TaskMetadataField::Title);
        session.archived_at = Some(1);
        pending.record(&session, TaskMetadataField::Archive);
        assert!(pending.is_archiving(&session.id));
        let mut unarchived_snapshot = session.list_projection();
        unarchived_snapshot.archived_at = None;
        pending.observe(&mut unarchived_snapshot);
        assert_eq!(unarchived_snapshot.archived_at, Some(1));
        session.archived_at = None;
        pending.record(&session, TaskMetadataField::Archive);
        assert!(!pending.is_archiving(&session.id));
        session.dormant_at = Some(session.updated_at);
        session.pinned_at = None;
        let dormant = pending.record(&session, TaskMetadataField::Placement);
        session.pinned_at = Some(2);
        pending.record(&session, TaskMetadataField::Placement);
        assert!(!pending.current(&dormant));

        let mut remote = session.list_projection();
        remote.title = "Old name".into();
        remote.archived_at = Some(1);
        remote.pinned_at = None;
        pending.observe(&mut remote);
        assert_eq!(remote.title, "Renamed");
        assert_eq!(remote.archived_at, None);
        assert_eq!(remote.pinned_at, Some(2));

        // A later external rename can arrive before the older save reply.
        let mut newer = session.list_projection();
        newer.metadata_revision = 5;
        newer.title = "Changed elsewhere".into();
        pending.observe(&mut newer);
        let mut saved = session.list_projection();
        saved.metadata_revision = 1;
        pending.acknowledge(&rename, &saved, &mut session);
        assert_eq!(session.title, "Changed elsewhere");
        assert_eq!(session.metadata_revision, 5);
        assert_eq!(session.pinned_at, Some(2));
        // A superseded request's failure/retry is ignored.
        assert!(!pending.current(&rename));
    }
}
