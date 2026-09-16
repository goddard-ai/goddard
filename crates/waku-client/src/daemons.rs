//! Which daemon owns which catalog row.
//!
//! The desktop holds one [`DaemonSupervisor`] per connected daemon — the
//! local daemon plus one per saved remote host — and routes every catalog
//! read or write by the row's owner. Ownership is tracked here rather than on
//! [`crate::Project`]/[`crate::AgentSession`]: project and session ids are
//! random UUIDs, so an absent claim unambiguously means local.

use std::collections::{BTreeMap, HashMap, HashSet};

use parking_lot::RwLock;
use uuid::Uuid;

use crate::process::DaemonSupervisor;

/// The daemon a catalog row belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DaemonKey {
    /// The supervisor the app spawned or was configured to use as primary.
    Local,
    /// A saved remote host, keyed by its [`crate::persistence::RemoteHost`] id.
    Remote(Uuid),
}

impl DaemonKey {
    pub fn is_remote(self) -> bool {
        matches!(self, Self::Remote(_))
    }
}

/// Shared registry of daemon connections and catalog ownership.
///
/// Clone-cheap like [`DaemonSupervisor`]: `StateStore`, `ComposerDraftStore`,
/// and the app entity all share one map so routing stays consistent no
/// matter which layer asks.
#[derive(Clone)]
pub struct DaemonMap {
    inner: std::sync::Arc<DaemonMapInner>,
}

struct DaemonMapInner {
    local: DaemonSupervisor,
    /// Connected remote supervisors, keyed by host record id. A configured
    /// but unreachable host is absent here while its cached catalog still
    /// claims its rows.
    remotes: RwLock<BTreeMap<Uuid, DaemonSupervisor>>,
    session_origins: RwLock<HashMap<Uuid, Uuid>>,
    project_origins: RwLock<HashMap<Uuid, Uuid>>,
    /// Remote hosts whose catalog snapshot has been applied — live or seeded
    /// from the offline cache. `SaveTaskState` may only be partitioned to a
    /// host that has answered (or cached) authoritative state, otherwise the
    /// per-daemon `live_session_ids` would delete sessions we never saw.
    loaded_remotes: RwLock<HashSet<Uuid>>,
}

impl DaemonMap {
    pub fn new(local: DaemonSupervisor) -> Self {
        Self {
            inner: std::sync::Arc::new(DaemonMapInner {
                local,
                remotes: RwLock::new(BTreeMap::new()),
                session_origins: RwLock::new(HashMap::new()),
                project_origins: RwLock::new(HashMap::new()),
                loaded_remotes: RwLock::new(HashSet::new()),
            }),
        }
    }

    pub fn local(&self) -> DaemonSupervisor {
        self.inner.local.clone()
    }

    /// Every connected supervisor, local first then remotes in host-id order.
    pub fn connected(&self) -> Vec<(DaemonKey, DaemonSupervisor)> {
        let mut all = Vec::with_capacity(1 + self.inner.remotes.read().len());
        all.push((DaemonKey::Local, self.inner.local.clone()));
        all.extend(
            self.inner
                .remotes
                .read()
                .iter()
                .map(|(host, supervisor)| (DaemonKey::Remote(*host), supervisor.clone())),
        );
        all
    }

    pub fn supervisor(&self, key: DaemonKey) -> Option<DaemonSupervisor> {
        match key {
            DaemonKey::Local => Some(self.inner.local.clone()),
            DaemonKey::Remote(host) => self.inner.remotes.read().get(&host).cloned(),
        }
    }

    pub fn add_remote(&self, host: Uuid, supervisor: DaemonSupervisor) {
        self.inner.remotes.write().insert(host, supervisor);
    }

    /// Detach a host's supervisor. Dropping the last clone stops its monitor
    /// and closes subscriber channels, which ends its task-state sync.
    pub fn remove_remote(&self, host: Uuid) -> Option<DaemonSupervisor> {
        let removed = self.inner.remotes.write().remove(&host);
        self.inner.loaded_remotes.write().remove(&host);
        self.inner
            .session_origins
            .write()
            .retain(|_, owner| *owner != host);
        self.inner
            .project_origins
            .write()
            .retain(|_, owner| *owner != host);
        removed
    }

    pub fn session_owner(&self, session: Uuid) -> DaemonKey {
        self.inner
            .session_origins
            .read()
            .get(&session)
            .map(|host| DaemonKey::Remote(*host))
            .unwrap_or(DaemonKey::Local)
    }

    pub fn project_owner(&self, project: Uuid) -> DaemonKey {
        self.inner
            .project_origins
            .read()
            .get(&project)
            .map(|host| DaemonKey::Remote(*host))
            .unwrap_or(DaemonKey::Local)
    }

    pub fn claim_session(&self, session: Uuid, key: DaemonKey) {
        match key {
            DaemonKey::Local => self.inner.session_origins.write().remove(&session),
            DaemonKey::Remote(host) => self.inner.session_origins.write().insert(session, host),
        };
    }

    pub fn claim_project(&self, project: Uuid, key: DaemonKey) {
        match key {
            DaemonKey::Local => self.inner.project_origins.write().remove(&project),
            DaemonKey::Remote(host) => self.inner.project_origins.write().insert(project, host),
        };
    }

    /// Re-key a host's ownership after its catalog snapshot — live or cached —
    /// was applied: every id it reports is claimed to it, and claims for ids
    /// it no longer reports are dropped so other daemons' rows are untouched.
    pub fn replace_remote_catalog(&self, host: Uuid, projects: &[Uuid], sessions: &[Uuid]) {
        let reported_projects: HashSet<Uuid> = projects.iter().copied().collect();
        let reported_sessions: HashSet<Uuid> = sessions.iter().copied().collect();
        self.inner
            .project_origins
            .write()
            .retain(|id, owner| *owner != host || reported_projects.contains(id));
        self.inner
            .session_origins
            .write()
            .retain(|id, owner| *owner != host || reported_sessions.contains(id));
        self.inner
            .project_origins
            .write()
            .extend(reported_projects.iter().map(|id| (*id, host)));
        self.inner
            .session_origins
            .write()
            .extend(reported_sessions.iter().map(|id| (*id, host)));
        self.inner.loaded_remotes.write().insert(host);
    }

    /// Forget one row's ownership — the row itself is being deleted, so any
    /// later claim for the same id must come from a fresh snapshot.
    pub fn drop_session(&self, session: Uuid) {
        self.inner.session_origins.write().remove(&session);
    }

    pub fn drop_project(&self, project: Uuid) {
        self.inner.project_origins.write().remove(&project);
    }

    /// Whether a host's catalog may be written back. Local is always loaded;
    /// a remote is loaded once its first snapshot or cached catalog applied.
    pub fn catalog_loaded(&self, key: DaemonKey) -> bool {
        match key {
            DaemonKey::Local => true,
            DaemonKey::Remote(host) => self.inner.loaded_remotes.read().contains(&host),
        }
    }

    /// The supervisor that owns a session, `None` when its host is configured
    /// but currently unreachable or was removed.
    pub fn daemon_for_session(&self, session: Uuid) -> Option<DaemonSupervisor> {
        self.supervisor(self.session_owner(session))
    }

    pub fn daemon_for_project(&self, project: Uuid) -> Option<DaemonSupervisor> {
        self.supervisor(self.project_owner(project))
    }

    pub fn is_remote_session(&self, session: Uuid) -> bool {
        self.session_owner(session).is_remote()
    }

    pub fn is_remote_project(&self, project: Uuid) -> bool {
        self.project_owner(project).is_remote()
    }
}
