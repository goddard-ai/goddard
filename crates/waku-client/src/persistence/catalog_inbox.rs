//! Coalesce catalog results before they reach the UI. Producers own disposal
//! of replaced snapshots; the UI consumes one host per apply in arrival order.

use super::TaskCatalog;
use crate::DaemonKey;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default)]
pub struct CatalogUpdate {
    pub catalog: Option<TaskCatalog>,
    pub error: Option<String>,
    /// Reconnection must survive a later same-connection update. Incognito
    /// history uses this distinction to recover after a daemon restart.
    pub fresh: bool,
}

#[derive(Default)]
struct Slots {
    latest: HashMap<DaemonKey, CatalogUpdate>,
    ready: VecDeque<DaemonKey>,
    sources: HashMap<DaemonKey, Uuid>,
}

type Pending = Arc<Mutex<Slots>>;

#[derive(Clone)]
pub struct CatalogInboxSender {
    pending: Pending,
    wake: Sender<()>,
}

/// One registered polling worker. Replacing or removing a host revokes its
/// old publisher, including a delayed result still in flight over the network.
#[derive(Clone)]
pub struct CatalogSource {
    pending: Pending,
    wake: Sender<()>,
    key: DaemonKey,
    source: Uuid,
}

pub struct CatalogInboxReceiver {
    pending: Pending,
    wake: Receiver<()>,
}

pub fn catalog_inbox() -> (CatalogInboxSender, CatalogInboxReceiver) {
    let pending = Arc::new(Mutex::new(Slots::default()));
    let (sender, receiver) = crossbeam_channel::bounded(1);
    (
        CatalogInboxSender {
            pending: Arc::clone(&pending),
            wake: sender,
        },
        CatalogInboxReceiver {
            pending,
            wake: receiver,
        },
    )
}

impl CatalogInboxSender {
    /// Register on the owning UI before starting a worker. Return superseded
    /// plain data so the owner can reclaim it on its background executor.
    pub fn bind(&self, key: DaemonKey) -> (CatalogSource, Option<CatalogUpdate>) {
        let source = Uuid::new_v4();
        let retired = {
            let mut pending = self.pending.lock();
            pending.sources.insert(key, source);
            pending.ready.retain(|queued| *queued != key);
            pending.latest.remove(&key)
        };
        (
            CatalogSource {
                pending: Arc::clone(&self.pending),
                wake: self.wake.clone(),
                key,
                source,
            },
            retired,
        )
    }
}

impl CatalogSource {
    /// Call on the catalog worker: replaced plain-data snapshots are dropped
    /// here, outside the lock, rather than by the UI's channel drain.
    pub fn send(
        &self,
        (result, fresh): (Result<TaskCatalog, String>, bool),
    ) -> Result<(), crossbeam_channel::SendError<()>> {
        let retired = {
            let mut pending = self.pending.lock();
            if pending.sources.get(&self.key) != Some(&self.source) {
                return Err(crossbeam_channel::SendError(()));
            }
            if !pending.latest.contains_key(&self.key) {
                pending.ready.push_back(self.key);
            }
            let slot = pending.latest.entry(self.key).or_default();
            slot.fresh |= fresh;
            match result {
                Ok(catalog) => {
                    slot.error = None;
                    slot.catalog.replace(catalog)
                }
                Err(error) => {
                    slot.error = Some(error);
                    None
                }
            }
        };
        drop(retired);
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => Err(crossbeam_channel::SendError(())),
        }
    }
}

impl CatalogInboxReceiver {
    /// Revoke before removing a host's visible rows. The caller reclaims
    /// returned plain data on a background executor.
    pub fn revoke(&self, key: DaemonKey) -> Option<CatalogUpdate> {
        let mut pending = self.pending.lock();
        pending.sources.remove(&key);
        pending.ready.retain(|queued| *queued != key);
        pending.latest.remove(&key)
    }

    /// One host per UI apply. A repeatedly updated host keeps its original
    /// queue position, then rejoins at the back after consumption.
    pub fn take_next(&self) -> Option<(DaemonKey, CatalogUpdate)> {
        let _ = self.wake.try_recv();
        let mut pending = self.pending.lock();
        let key = pending.ready.pop_front()?;
        pending.latest.remove(&key).map(|update| (key, update))
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.lock().ready.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use waku_protocol::model::{AgentSession, ProviderKind};

    fn catalog(title: &str) -> TaskCatalog {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.title = title.into();
        TaskCatalog {
            projects: Vec::new(),
            sessions: vec![session],
        }
    }

    fn drain(receiver: &CatalogInboxReceiver) -> HashMap<DaemonKey, CatalogUpdate> {
        std::iter::from_fn(|| receiver.take_next()).collect()
    }

    #[test]
    fn delayed_consumer_keeps_latest_per_host_and_reconnect_marker_across_errors() {
        let (sender, receiver) = catalog_inbox();
        let remote = DaemonKey::Remote(Uuid::new_v4());
        let (local_source, _) = sender.bind(DaemonKey::Local);
        let (remote_source, _) = sender.bind(remote);
        local_source.send((Ok(catalog("first")), true)).unwrap();
        remote_source.send((Ok(catalog("remote")), false)).unwrap();
        for _ in 0..1000 {
            local_source.send((Ok(catalog("latest")), false)).unwrap();
        }
        local_source.send((Err("offline".into()), false)).unwrap();
        assert_eq!(sender.wake.len(), 1);
        let mut latest = drain(&receiver);
        assert_eq!(latest.len(), 2);
        let local = latest.remove(&DaemonKey::Local).unwrap();
        assert!(local.fresh);
        assert_eq!(local.catalog.unwrap().sessions[0].title, "latest");
        assert_eq!(local.error.as_deref(), Some("offline"));
        assert_eq!(
            latest.remove(&remote).unwrap().catalog.unwrap().sessions[0].title,
            "remote"
        );
        assert!(drain(&receiver).is_empty());
        local_source
            .send((Ok(catalog("recovered")), false))
            .unwrap();
        let recovered = drain(&receiver).remove(&DaemonKey::Local).unwrap();
        assert!(!recovered.fresh);
        assert!(recovered.error.is_none());
        assert_eq!(recovered.catalog.unwrap().sessions[0].title, "recovered");
        drop(receiver);
        assert!(remote_source.send((Ok(catalog("closed")), false)).is_err());
    }

    #[test]
    fn replaced_or_removed_host_cannot_publish_late_catalogs() {
        let (sender, receiver) = catalog_inbox();
        let host = DaemonKey::Remote(Uuid::new_v4());
        let (old, _) = sender.bind(host);
        old.send((Ok(catalog("old")), true)).unwrap();
        let (new, retired) = sender.bind(host);
        assert_eq!(retired.unwrap().catalog.unwrap().sessions[0].title, "old");
        assert!(old.send((Ok(catalog("late old result")), false)).is_err());
        new.send((Ok(catalog("replacement")), true)).unwrap();
        let latest = drain(&receiver).remove(&host).unwrap();
        assert_eq!(latest.catalog.unwrap().sessions[0].title, "replacement");
        new.send((Ok(catalog("pending removal")), false)).unwrap();
        assert!(receiver.revoke(host).is_some());
        assert!(
            new.send((Ok(catalog("late removed result")), false))
                .is_err()
        );
        assert!(drain(&receiver).is_empty());
        let (readded, _) = sender.bind(host);
        readded.send((Ok(catalog("readded")), true)).unwrap();
        assert!(drain(&receiver).remove(&host).unwrap().fresh);
    }

    #[test]
    fn busy_host_cannot_starve_another_hosts_pending_catalog() {
        let (sender, receiver) = catalog_inbox();
        let (busy, _) = sender.bind(DaemonKey::Local);
        let remote = DaemonKey::Remote(Uuid::new_v4());
        let (quiet, _) = sender.bind(remote);
        busy.send((Ok(catalog("busy first")), false)).unwrap();
        quiet.send((Ok(catalog("quiet")), false)).unwrap();
        busy.send((Ok(catalog("busy latest")), false)).unwrap();
        assert_eq!(receiver.take_next().unwrap().0, DaemonKey::Local);
        busy.send((Ok(catalog("busy again")), false)).unwrap();
        assert!(receiver.has_pending());
        let (host, update) = receiver.take_next().unwrap();
        assert_eq!(host, remote);
        assert_eq!(update.catalog.unwrap().sessions[0].title, "quiet");
        assert_eq!(receiver.take_next().unwrap().0, DaemonKey::Local);
        assert!(!receiver.has_pending());
    }
}
