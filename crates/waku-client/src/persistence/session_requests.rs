//! Per-attempt result authority, independent of physical work still in flight.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionRequest {
    session_id: Uuid,
    attempt: Uuid,
    owner: DaemonKey,
    owner_epoch: Uuid,
}
impl SessionRequest {
    pub fn session_id(self) -> Uuid {
        self.session_id
    }
    pub(super) fn session_id_ref(&self) -> &Uuid {
        &self.session_id
    }
    pub fn owner(self) -> DaemonKey {
        self.owner
    }
}

#[derive(Default)]
pub struct PendingSessionRequests {
    pending: BTreeMap<Uuid, SessionRequest>,
    owner_epochs: HashMap<DaemonKey, Uuid>,
}
impl Drop for PendingSessionRequests {
    fn drop(&mut self) {
        if !self.pending.is_empty() {
            super::task_writer::retire(std::mem::take(&mut self.pending));
        }
    }
}
impl PendingSessionRequests {
    pub fn current(&self, session_id: Uuid) -> Option<SessionRequest> {
        self.pending
            .get(&session_id)
            .copied()
            .filter(|request| self.is_current(*request))
    }
    pub fn contains(&self, session_id: Uuid) -> bool {
        self.current(session_id).is_some()
    }
    pub fn is_current(&self, request: SessionRequest) -> bool {
        self.pending.get(&request.session_id) == Some(&request)
            && self.owner_epochs.get(&request.owner) == Some(&request.owner_epoch)
    }
    pub fn begin(&mut self, session_id: Uuid, owner: DaemonKey) -> Option<SessionRequest> {
        if self
            .current(session_id)
            .is_some_and(|request| request.owner == owner)
        {
            return None;
        }
        let owner_epoch = *self.owner_epochs.entry(owner).or_insert_with(Uuid::new_v4);
        let request = SessionRequest {
            session_id,
            attempt: Uuid::new_v4(),
            owner,
            owner_epoch,
        };
        self.pending.insert(session_id, request);
        Some(request)
    }
    pub fn complete(&mut self, request: SessionRequest) -> bool {
        let current = self.is_current(request);
        // An old completion may retire its own obsolete record, never a newer one.
        if self.pending.get(&request.session_id) == Some(&request) {
            self.pending.remove(&request.session_id);
        }
        current
    }
    pub fn cancel(&mut self, session_id: Uuid) {
        self.pending.remove(&session_id);
    }
    /// Revoke one host in constant catalog work without scanning all requests.
    pub fn cancel_owner(&mut self, owner: DaemonKey) {
        self.owner_epochs.insert(owner, Uuid::new_v4());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_completion_cannot_consume_replacement_or_cross_host_revocation() {
        let mut pending = PendingSessionRequests::default();
        let id = Uuid::new_v4();
        let host = DaemonKey::Remote(Uuid::new_v4());
        let other_id = Uuid::new_v4();
        let other = pending.begin(other_id, DaemonKey::Local).unwrap();
        let old = pending.begin(id, host).unwrap();
        assert!(pending.begin(id, host).is_none());
        pending.cancel(id);
        let replacement = pending.begin(id, host).unwrap();
        assert!(!pending.complete(old));
        assert!(pending.is_current(replacement));
        pending.cancel_owner(host);
        assert!(!pending.contains(id));
        assert!(pending.is_current(other));
        let readded = pending.begin(id, host).unwrap();
        assert!(!pending.complete(replacement));
        assert!(pending.is_current(readded));
        assert!(pending.complete(readded));
        assert!(!pending.complete(readded));
        assert!(pending.complete(other));
    }
}
