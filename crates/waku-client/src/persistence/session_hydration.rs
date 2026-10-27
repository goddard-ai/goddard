//! Indexed navigation/runtime detail queue: selected reads overtake warmup.
use super::*;

#[derive(Default)]
pub struct SessionHydrations {
    queued: BTreeMap<i128, SessionRequest>,
    positions: BTreeMap<Uuid, i128>,
    running: HashSet<SessionRequest>,
    requests: PendingSessionRequests,
}

impl Drop for SessionHydrations {
    fn drop(&mut self) {
        if !self.queued.is_empty() {
            super::task_writer::retire((
                std::mem::take(&mut self.queued),
                std::mem::take(&mut self.positions),
            ));
        }
    }
}

impl SessionHydrations {
    pub fn contains(&self, session_id: Uuid) -> bool {
        self.requests.contains(session_id)
    }

    pub fn enqueue(&mut self, session_id: Uuid, owner: DaemonKey, visible: bool) {
        let existing = self
            .requests
            .current(session_id)
            .filter(|request| request.owner() == owner);
        if existing.is_some_and(|request| self.running.contains(&request)) {
            return;
        }
        let request = existing.unwrap_or_else(|| {
            self.requests
                .begin(session_id, owner)
                .expect("new hydration attempt")
        });
        if let Some(&previous) = self.positions.get(&session_id) {
            if existing.is_some()
                && (!visible
                    || self
                        .queued
                        .first_key_value()
                        .is_some_and(|(_, id)| id.session_id() == session_id))
            {
                return;
            }
            self.queued.remove(&previous);
        }
        // Wide signed ordinals permit push-front and push-back without
        // reindexing every queued id. Empty queues start at zero again.
        let order = if visible {
            self.queued.first_key_value().map_or(0, |(&first, _)| {
                first
                    .checked_sub(1)
                    .expect("hydration queue front exhausted")
            })
        } else {
            self.queued.last_key_value().map_or(0, |(&last, _)| {
                last.checked_add(1).expect("hydration queue back exhausted")
            })
        };
        self.queued.insert(order, request);
        self.positions.insert(session_id, order);
    }

    pub fn has_ready(&self) -> bool {
        self.running.len() < 2 && !self.queued.is_empty()
    }

    pub fn next(&mut self) -> Option<SessionRequest> {
        if self.running.len() >= 2 {
            return None;
        }
        let (_, request) = self.queued.pop_first()?;
        self.positions.remove(&request.session_id());
        self.running.insert(request);
        Some(request)
    }

    /// Revocation does not release a physical slot. Only completion does.
    pub fn finished(&mut self, request: SessionRequest) -> bool {
        self.running.remove(&request) && self.requests.complete(request)
    }

    pub fn is_current(&self, request: SessionRequest) -> bool {
        self.requests.is_current(request)
    }

    pub fn cancel(&mut self, session_id: Uuid) {
        self.requests.cancel(session_id);
        if let Some(position) = self.positions.remove(&session_id) {
            self.queued.remove(&position);
        }
    }

    pub fn cancel_owner(&mut self, owner: DaemonKey) {
        self.requests.cancel_owner(owner);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Uuid> {
        self.running
            .iter()
            .chain(self.queued.values())
            .map(|request| request.session_id_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_queue_preserves_priority_fifo_duplicates_and_two_read_limit() {
        let mut queue = SessionHydrations::default();
        let ids = (1..=4096).map(Uuid::from_u128).collect::<Vec<_>>();
        for &id in &ids {
            queue.enqueue(id, DaemonKey::Local, false);
        }
        for &id in &ids {
            queue.enqueue(id, DaemonKey::Local, false);
        }
        let first = queue.next().unwrap();
        let second = queue.next().unwrap();
        assert_eq!(first.session_id(), ids[0]);
        assert_eq!(second.session_id(), ids[1]);
        assert!(!queue.has_ready());
        assert!(queue.next().is_none());
        for &id in &ids[4000..] {
            queue.enqueue(id, DaemonKey::Local, true);
            queue.enqueue(id, DaemonKey::Local, true);
        }
        queue.enqueue(first.session_id(), DaemonKey::Local, true);
        assert_eq!(queue.iter().count(), ids.len());
        assert!(queue.finished(first));
        assert!(queue.has_ready());
        let selected = queue.next().unwrap();
        assert_eq!(selected.session_id(), ids[4095]);
        assert!(queue.next().is_none());
        assert!(queue.finished(selected));
        assert!(queue.finished(second));
        let mut dispatched = HashSet::from([
            first.session_id(),
            second.session_id(),
            selected.session_id(),
        ]);
        for expected in ids[4000..4095].iter().rev().chain(ids[2..4000].iter()) {
            let request = queue.next().unwrap();
            assert_eq!(request.session_id(), *expected);
            assert!(queue.contains(*expected));
            assert!(dispatched.insert(*expected));
            assert!(queue.finished(request));
            assert!(!queue.contains(*expected));
        }
        assert_eq!(dispatched.len(), ids.len());
        assert!(queue.next().is_none());
        queue.enqueue(ids[0], DaemonKey::Local, true);
        let retry = queue.next().unwrap();
        assert_eq!(retry.session_id(), ids[0]);
        assert!(!queue.finished(first));
        assert!(queue.finished(retry));
    }

    #[test]
    fn queued_rows_get_new_authority_after_owner_revocation_or_reassignment() {
        let host = DaemonKey::Remote(Uuid::new_v4());
        for (replacement_owner, visible) in [(host, false), (host, true), (DaemonKey::Local, false)]
        {
            let mut queue = SessionHydrations::default();
            let id = Uuid::new_v4();
            queue.enqueue(id, host, false);
            queue.cancel_owner(host);
            queue.enqueue(id, replacement_owner, visible);
            let request = queue.next().unwrap();
            assert!(
                queue.is_current(request),
                "queued replacement kept an old attempt"
            );
            assert_eq!(request.owner(), replacement_owner);
            assert!(queue.finished(request));
            assert!(queue.next().is_none());
            assert!(!queue.contains(id));
        }
    }

    #[test]
    fn revoked_reads_keep_physical_slots_and_cannot_complete_readded_tasks() {
        let mut queue = SessionHydrations::default();
        let host = DaemonKey::Remote(Uuid::new_v4());
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        for id in [a, b, c] {
            queue.enqueue(id, host, false);
        }
        let old_a = queue.next().unwrap();
        let old_b = queue.next().unwrap();
        queue.cancel_owner(host);
        queue.enqueue(a, host, true);
        assert!(
            !queue.has_ready(),
            "revocation released a physical read slot"
        );
        assert!(queue.next().is_none());
        assert!(!queue.finished(old_a));
        let new_a = queue.next().unwrap();
        assert_eq!(new_a.session_id(), a);
        assert!(queue.is_current(new_a));
        assert!(!queue.finished(old_a));
        assert!(queue.is_current(new_a));
        assert!(queue.next().is_none());
        assert!(!queue.finished(old_b));
        let stale_c = queue.next().unwrap();
        assert_eq!(stale_c.session_id(), c);
        assert!(!queue.is_current(stale_c));
        assert!(!queue.finished(stale_c));
        queue.cancel(a);
        queue.enqueue(a, host, true);
        let replacement = queue.next().unwrap();
        assert!(!queue.finished(new_a));
        assert!(queue.is_current(replacement));
        assert!(queue.finished(replacement));
    }
}
