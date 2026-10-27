//! A Stop drains a finite, already-received prefix before settling the turn.

use std::collections::VecDeque;

use crossbeam_channel::Receiver;

use crate::driver::DriverEventBatch;
use crate::model::DriverEvent;

pub(super) struct CancelBacklog {
    remaining_batches: usize,
    retain_runtime: bool,
    close_requested: bool,
    follow_up_requested: bool,
}

impl CancelBacklog {
    pub(super) fn new(buffered_batches: usize, retain_runtime: bool) -> Self {
        Self {
            remaining_batches: buffered_batches,
            retain_runtime,
            close_requested: false,
            follow_up_requested: false,
        }
    }

    pub(super) fn collect(
        &mut self,
        events: &Receiver<DriverEventBatch>,
        pending: &mut VecDeque<DriverEvent>,
        limit: usize,
    ) {
        let received = collect(events, pending, limit.min(self.remaining_batches));
        self.remaining_batches -= received;
    }

    pub(super) fn finished(&self, pending: &VecDeque<DriverEvent>) -> bool {
        self.remaining_batches == 0 && pending.is_empty()
    }

    /// Request CloseSession immediately, retaining the local subscription long
    /// enough to drain its received prefix. Return true only for the first
    /// request, so finishing that prefix cannot close a successor runtime.
    pub(super) fn evict(&mut self) -> bool {
        self.retain_runtime = false;
        !std::mem::replace(&mut self.close_requested, true)
    }

    pub(super) fn retains_runtime(&self, still_live: bool) -> bool {
        still_live && self.retain_runtime
    }

    pub(super) fn note_follow_up(&mut self) {
        self.follow_up_requested = true;
    }

    pub(super) fn follow_up_requested(&self) -> bool {
        self.follow_up_requested
    }
}

pub(super) fn collect(
    events: &Receiver<DriverEventBatch>,
    pending: &mut VecDeque<DriverEvent>,
    limit: usize,
) -> usize {
    let mut received = 0;
    for batch in events.try_iter().take(limit) {
        pending.push_back(batch.event);
        if let Some(cursor) = batch.cursor {
            pending.push_back(DriverEvent::RuntimeEventCursorAdvanced(cursor));
        }
        received += 1;
    }
    received
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_preserves_received_order_in_bounded_slices_and_later_events_cannot_extend_it() {
        let (send, events) = crossbeam_channel::unbounded();
        let mut pending = VecDeque::from([DriverEvent::TextDelta("already smoothed;".into())]);
        let runtime_id = uuid::Uuid::new_v4();
        let epoch = uuid::Uuid::new_v4();
        for index in 0..5_000 {
            send.send(DriverEventBatch {
                event: DriverEvent::TextDelta(format!("{index};")),
                cursor: Some(crate::model::RuntimeEventCursor {
                    runtime_id,
                    epoch,
                    sequence: index,
                }),
            })
            .unwrap();
        }
        let mut backlog = CancelBacklog::new(events.len(), true);
        // Archive closes the provider immediately but keeps the received local
        // prefix. Its final retirement must not send a second CloseSession.
        assert!(backlog.evict());
        assert!(!backlog.evict());
        assert!(!backlog.follow_up_requested());
        // Only a new explicit send during this local drain may re-arm the
        // queue. A finish already buffered before Stop must not do that.
        backlog.note_follow_up();
        assert!(!backlog.finished(&pending));
        for index in 5_000..8_000 {
            send.send(DriverEventBatch {
                event: DriverEvent::TextDelta(format!("{index};")),
                cursor: None,
            })
            .unwrap();
        }
        let mut applied = String::new();
        let mut acknowledged = 0;
        let mut slices = 0;
        while !backlog.finished(&pending) {
            if pending.is_empty() {
                let before = events.len();
                backlog.collect(&events, &mut pending, 31);
                assert!(before - events.len() <= 31);
            }
            // Stand in for a pump's finite application allowance. In
            // production the existing time budget governs the same queue.
            for _ in 0..7 {
                let Some(event) = pending.pop_front() else {
                    break;
                };
                match event {
                    DriverEvent::TextDelta(text) => applied.push_str(&text),
                    DriverEvent::RuntimeEventCursorAdvanced(cursor) => {
                        assert_eq!(cursor.runtime_id, runtime_id);
                        assert_eq!(cursor.epoch, epoch);
                        assert_eq!(cursor.sequence, acknowledged);
                        assert!(applied.ends_with(&format!("{acknowledged};")));
                        acknowledged += 1;
                    }
                    _ => panic!("received event order changed"),
                }
            }
            slices += 1;
            assert!(slices < 10_000, "new deliveries extended the Stop frontier");
        }
        let expected = std::iter::once("already smoothed;".to_owned())
            .chain((0..5_000).map(|index| format!("{index};")))
            .collect::<String>();
        assert_eq!(applied, expected);
        assert_eq!(acknowledged, 5_000);
        assert!(slices > 1);
        assert!(backlog.follow_up_requested());
        assert!(!backlog.evict(), "retirement closed a successor runtime");
        assert_eq!(events.len(), 3_000, "post-Stop events crossed the frontier");
        assert!(!backlog.retains_runtime(true));
        assert!(CancelBacklog::new(0, true).retains_runtime(true));
        assert!(!CancelBacklog::new(0, true).retains_runtime(false));
        assert!(!CancelBacklog::new(0, false).retains_runtime(true));
    }
}
