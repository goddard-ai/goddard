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
    pub version: Option<CatalogVersion>,
    pub catalog: Option<TaskCatalog>,
    /// Unmodified remote-host cache input, copied by the polling worker so
    /// optimistic UI overlays never enter the offline cache.
    pub cache_catalog: Option<TaskCatalog>,
    pub error: Option<String>,
    /// Reconnection must survive a later same-connection update. Incognito
    /// history uses this distinction to recover after a daemon restart.
    pub fresh: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogVersion {
    key: DaemonKey,
    source: Uuid,
    generation: Uuid,
}

struct SourceState {
    source: Uuid,
    generation: Option<Uuid>,
    fresh: bool,
}

#[derive(Default)]
struct Slots {
    latest: HashMap<DaemonKey, CatalogUpdate>,
    ready: VecDeque<DaemonKey>,
    sources: HashMap<DaemonKey, SourceState>,
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
            pending.sources.insert(
                key,
                SourceState {
                    source,
                    generation: None,
                    fresh: false,
                },
            );
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
        // Copy outside the shared lock and on the producer, never in the
        // UI's apply path. Local catalogs do not need an offline host copy.
        let cache_catalog = match (&self.key, &result) {
            (DaemonKey::Remote(_), Ok(catalog)) => Some(catalog.clone()),
            _ => None,
        };
        let retired = {
            let mut pending = self.pending.lock();
            let Some(state) = pending
                .sources
                .get_mut(&self.key)
                .filter(|state| state.source == self.source)
            else {
                return Err(crossbeam_channel::SendError(()));
            };
            state.fresh |= fresh;
            if result.is_ok() {
                state.generation = Some(Uuid::new_v4());
            }
            let fresh = state.fresh;
            let version = state.generation.map(|generation| CatalogVersion {
                key: self.key,
                source: self.source,
                generation,
            });
            if !pending.latest.contains_key(&self.key) {
                pending.ready.push_back(self.key);
            }
            let slot = pending.latest.entry(self.key).or_default();
            slot.fresh |= fresh;
            match result {
                Ok(catalog) => {
                    slot.version = version;
                    slot.error = None;
                    (
                        slot.catalog.replace(catalog),
                        std::mem::replace(&mut slot.cache_catalog, cache_catalog),
                    )
                }
                Err(error) => {
                    slot.error = Some(error);
                    (None, None)
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
    /// Check before applying each batch, including after background preparation.
    pub fn is_current(&self, version: CatalogVersion) -> bool {
        self.pending
            .lock()
            .sources
            .get(&version.key)
            .is_some_and(|state| {
                state.source == version.source && state.generation == Some(version.generation)
            })
    }

    /// Only completion of the current catalog consumes reconnection evidence.
    /// Superseded or revoked work cannot acknowledge its successor's source.
    pub fn acknowledge(&self, version: CatalogVersion) -> bool {
        let mut pending = self.pending.lock();
        let Some(state) = pending.sources.get_mut(&version.key).filter(|state| {
            state.source == version.source && state.generation == Some(version.generation)
        }) else {
            return false;
        };
        state.fresh = false;
        true
    }
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
        pending.latest.remove(&key).map(|mut update| {
            update.fresh = pending.sources.get(&key).is_some_and(|state| state.fresh);
            (key, update)
        })
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
        std::iter::from_fn(|| receiver.take_next())
            .map(|(key, update)| {
                if let Some(version) = update.version {
                    assert!(receiver.acknowledge(version));
                }
                (key, update)
            })
            .collect()
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
        assert!(local.cache_catalog.is_none());
        assert_eq!(local.catalog.unwrap().sessions[0].title, "latest");
        assert_eq!(local.error.as_deref(), Some("offline"));
        let mut remote = latest.remove(&remote).unwrap();
        assert_eq!(remote.catalog.as_ref().unwrap().sessions[0].title, "remote");
        // Applying optimistic fields to the UI must never cache an edit the
        // owning daemon has not acknowledged.
        remote.catalog.as_mut().unwrap().sessions[0].title = "optimistic edit".into();
        assert_eq!(remote.cache_catalog.unwrap().sessions[0].title, "remote");
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
    fn taken_catalogs_are_revoked_and_freshness_survives_until_current_completion() {
        let (sender, receiver) = catalog_inbox();
        let host = DaemonKey::Remote(Uuid::new_v4());
        let (source, _) = sender.bind(host);
        source.send((Ok(catalog("first")), true)).unwrap();
        let first = receiver.take_next().unwrap().1;
        let old = first.version.unwrap();
        assert!(receiver.is_current(old));
        source
            .send((Err("temporarily unavailable".into()), false))
            .unwrap();
        assert!(
            receiver.is_current(old),
            "an error must not invalidate accepted catalog data"
        );
        source.send((Ok(catalog("successor")), false)).unwrap();
        assert!(!receiver.is_current(old));
        assert!(!receiver.acknowledge(old));
        let next = receiver.take_next().unwrap().1;
        assert!(next.fresh, "supersession lost incognito recovery authority");
        let current = next.version.unwrap();
        assert!(receiver.acknowledge(current));
        source
            .send((Ok(catalog("ordinary update")), false))
            .unwrap();
        let ordinary = receiver.take_next().unwrap().1;
        assert!(!ordinary.fresh);
        let token = ordinary.version.unwrap();
        receiver.revoke(host);
        assert!(!receiver.is_current(token));
        assert!(!receiver.acknowledge(token));
        let (replacement, _) = sender.bind(host);
        replacement
            .send((Ok(catalog("replacement")), true))
            .unwrap();
        assert!(!receiver.acknowledge(token));
        assert!(receiver.take_next().unwrap().1.fresh);
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
