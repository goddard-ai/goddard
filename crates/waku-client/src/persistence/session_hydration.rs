//! Indexed navigation/runtime detail queue: selected reads overtake warmup.
use super::*;

#[derive(Default)]
pub struct SessionHydrations {
    queued: BTreeMap<i128, Uuid>,
    positions: BTreeMap<Uuid, i128>,
    running: HashSet<Uuid>,
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
        self.running.contains(&session_id) || self.positions.contains_key(&session_id)
    }

    pub fn enqueue(&mut self, session_id: Uuid, visible: bool) {
        if self.running.contains(&session_id) {
            return;
        }
        if let Some(&previous) = self.positions.get(&session_id) {
            if !visible
                || self
                    .queued
                    .first_key_value()
                    .is_some_and(|(_, id)| *id == session_id)
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
        self.queued.insert(order, session_id);
        self.positions.insert(session_id, order);
    }

    pub fn has_ready(&self) -> bool {
        self.running.len() < 2 && !self.queued.is_empty()
    }

    pub fn next(&mut self) -> Option<Uuid> {
        if self.running.len() >= 2 {
            return None;
        }
        let (_, id) = self.queued.pop_first()?;
        self.positions.remove(&id);
        self.running.insert(id);
        Some(id)
    }

    pub fn finished(&mut self, session_id: Uuid) {
        self.running.remove(&session_id);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Uuid> {
        self.running.iter().chain(self.queued.values())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_hydration_prioritizes_selection_without_duplicate_or_unbounded_reads() {
        let ids = [
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        let mut queue = SessionHydrations::default();
        for id in ids {
            queue.enqueue(id, false);
        }
        assert_eq!(queue.next(), Some(ids[0]));
        assert_eq!(queue.next(), Some(ids[1]));
        assert_eq!(queue.next(), None);
        assert!(!queue.has_ready());

        // A click overtakes queued warmup. Repeated clicks on active and
        // queued tasks still produce exactly one read per task.
        queue.enqueue(ids[3], true);
        queue.enqueue(ids[3], true);
        queue.enqueue(ids[0], true);
        queue.running.remove(&ids[0]);
        assert!(queue.has_ready());
        assert_eq!(queue.next(), Some(ids[3]));
        assert_eq!(queue.next(), None);
        queue.running.remove(&ids[1]);
        assert_eq!(queue.next(), Some(ids[2]));
        queue.running.clear();
        assert_eq!(queue.next(), None);
        assert!(!queue.contains(ids[0]));

        // A completed or failed request can be explicitly retried.
        queue.enqueue(ids[0], true);
        assert_eq!(queue.next(), Some(ids[0]));
    }

    #[test]
    fn large_hydration_queue_keeps_fifo_and_selection_order_across_duplicates_and_retries() {
        let mut queue = SessionHydrations::default();
        let ids = (1..=4096).map(Uuid::from_u128).collect::<Vec<_>>();
        for &id in &ids {
            queue.enqueue(id, false);
        }
        for &id in &ids {
            queue.enqueue(id, false);
        }
        assert_eq!(queue.iter().count(), ids.len());
        assert_eq!(queue.next(), Some(ids[0]));
        assert_eq!(queue.next(), Some(ids[1]));
        for &id in &ids[4000..] {
            queue.enqueue(id, true);
            queue.enqueue(id, true);
        }
        assert_eq!(queue.next(), None);
        let mut dispatched = vec![ids[0], ids[1]];
        queue.finished(ids[0]);
        queue.finished(ids[1]);
        for expected in ids[4000..].iter().rev().chain(ids[2..4000].iter()) {
            let actual = queue.next().unwrap();
            assert_eq!(actual, *expected);
            assert!(queue.contains(actual));
            dispatched.push(actual);
            queue.finished(actual);
            assert!(!queue.contains(actual));
        }
        assert_eq!(
            dispatched.iter().copied().collect::<HashSet<_>>().len(),
            ids.len()
        );
        assert_eq!(queue.next(), None);
        assert_eq!(queue.iter().count(), 0);
        queue.enqueue(ids[2000], true);
        queue.enqueue(ids[2000], false);
        assert_eq!(queue.next(), Some(ids[2000]));
    }
}
