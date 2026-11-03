//! Process-local admission fence for owner-requested, non-interrupting shutdown.
//! The owner must account for work before exposing this gate on a transport.
use parking_lot::Mutex;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

const LEASE: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct IdleShutdownGate {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    work: usize,
    lease: Option<Lease>,
    committed: bool,
}

struct Lease {
    owner: Uuid,
    request: Uuid,
    token: Uuid,
    deadline: Instant,
}

/// Hold through the last effect/teardown of admitted work, including detached workers.
pub struct WorkPermit(Arc<IdleShutdownGate>);

impl Drop for WorkPermit {
    fn drop(&mut self) {
        let mut state = self.0.state.lock();
        state.work -= 1;
    }
}

impl State {
    fn expire(&mut self, now: Instant) {
        if !self.committed
            && self
                .lease
                .as_ref()
                .is_some_and(|lease| now >= lease.deadline)
        {
            self.lease = None;
        }
    }
}

impl IdleShutdownGate {
    pub fn enter(self: &Arc<Self>) -> anyhow::Result<WorkPermit> {
        let mut state = self.state.lock();
        state.expire(Instant::now());
        anyhow::ensure!(
            !state.committed && state.lease.is_none(),
            "daemon replacement admission fenced; retry after reconnect"
        );
        state.work = state
            .work
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("daemon work accounting exhausted"))?;
        Ok(WorkPermit(self.clone()))
    }

    /// Busy leaves intake open. The inventory is read under the admission lock;
    /// it must not itself admit work or acquire a permit.
    pub fn prepare(&self, owner: Uuid, request: Uuid, idle: impl FnOnce() -> bool) -> Option<Uuid> {
        self.prepare_at(owner, request, Instant::now(), idle)
    }

    fn prepare_at(
        &self,
        owner: Uuid,
        request: Uuid,
        now: Instant,
        idle: impl FnOnce() -> bool,
    ) -> Option<Uuid> {
        let mut state = self.state.lock();
        state.expire(now);
        if state.committed {
            return None;
        }
        if let Some(lease) = &state.lease {
            return (lease.owner == owner && lease.request == request).then_some(lease.token);
        }
        if state.work != 0 || !idle() {
            return None;
        }
        let token = Uuid::new_v4();
        state.lease = Some(Lease {
            owner,
            request,
            token,
            deadline: now + LEASE,
        });
        Some(token)
    }

    /// Commit and signal the transport while holding the very same gate. No
    /// expiration, disconnect or new admission can interleave with the signal.
    pub fn commit(
        &self,
        owner: Uuid,
        token: Uuid,
        idle: impl FnOnce() -> bool,
        signal: impl FnOnce(),
    ) -> bool {
        let mut state = self.state.lock();
        state.expire(Instant::now());
        if state.committed {
            return state
                .lease
                .as_ref()
                .is_some_and(|l| l.owner == owner && l.token == token);
        }
        if !state
            .lease
            .as_ref()
            .is_some_and(|l| l.owner == owner && l.token == token)
        {
            return false;
        }
        if state.work != 0 || !idle() {
            state.lease = None;
            return false;
        }
        state.committed = true;
        signal();
        true
    }

    pub fn release(&self, owner: Uuid, token: Uuid) {
        let mut state = self.state.lock();
        if !state.committed
            && state
                .lease
                .as_ref()
                .is_some_and(|l| l.owner == owner && l.token == token)
        {
            state.lease = None;
        }
    }

    pub fn disconnect(&self, owner: Uuid) {
        let mut state = self.state.lock();
        if !state.committed && state.lease.as_ref().is_some_and(|l| l.owner == owner) {
            state.lease = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_startup_and_teardown_hold_shutdown_busy_across_all_sessions() {
        let gate = Arc::new(IdleShutdownGate::default());
        let owner = Uuid::new_v4();
        let mut work = (0..75).map(|_| gate.enter().unwrap()).collect::<Vec<_>>();
        work.drain(..74);
        assert!(gate.prepare(owner, Uuid::new_v4(), || true).is_none());
        drop(work);
        let token = gate.prepare(owner, Uuid::new_v4(), || true).unwrap();
        assert!(gate.enter().is_err());
        let mut signaled = false;
        assert!(gate.commit(owner, token, || true, || signaled = true));
        assert!(signaled);
        assert!(gate.commit(
            owner,
            token,
            || panic!("duplicate inventory"),
            || panic!("duplicate shutdown")
        ));
        gate.disconnect(owner);
        assert!(gate.enter().is_err());
    }

    #[test]
    fn disconnect_timeout_stale_duplicate_and_foreign_leases_preserve_admission() {
        let gate = Arc::new(IdleShutdownGate::default());
        let owner = Uuid::new_v4();
        let request = Uuid::new_v4();
        let now = Instant::now();
        let old = gate.prepare_at(owner, request, now, || true).unwrap();
        assert_eq!(gate.prepare_at(owner, request, now, || true), Some(old));
        assert!(gate.prepare(Uuid::new_v4(), request, || true).is_none());
        assert!(!gate.commit(Uuid::new_v4(), old, || true, || panic!("foreign shutdown")));
        let new = gate
            .prepare_at(owner, request, now + LEASE, || true)
            .unwrap();
        assert_ne!(old, new);
        assert!(!gate.commit(owner, old, || true, || panic!("stale shutdown")));
        gate.release(owner, old);
        assert!(gate.enter().is_err());
        gate.disconnect(owner);
        drop(gate.enter().unwrap());
        let token = gate.prepare(owner, request, || true).unwrap();
        assert!(!gate.commit(owner, token, || false, || panic!("busy shutdown")));
        drop(gate.enter().unwrap());
    }
}
