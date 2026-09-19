//! Project sharing state — which repositories each side offers, the sync
//! links the local user opted into, and sync alerts awaiting a decision.
//! Persisted as `share.json` under the share dir, atomically like
//! `friends.json`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};

/// One repo a friend shares with us — the `ShareProjects` payload entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedRepo {
    /// Sender's project name — display only.
    pub name: String,
    /// Fetch URL of its `origin` remote; matching normalizes it.
    pub origin_url: String,
    /// The sender lets us watch this project's sessions — read-only,
    /// live. `false` when the share predates session sharing.
    #[serde(default)]
    pub share_sessions: bool,
}

/// A repo we share with `peer`. `repo_path` lets the sync loop find the
/// checkout without asking the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingShare {
    pub peer: EndpointId,
    /// Our project name — display only.
    pub name: String,
    pub origin_url: String,
    pub repo_path: PathBuf,
    /// The peer told us they enabled sync on this share.
    #[serde(default)]
    pub peer_sync_enabled: bool,
    /// We let the peer watch this project's sessions — read-only, live.
    /// Independent of sync; `false` for shares predating the flag.
    #[serde(default)]
    pub share_sessions: bool,
    pub shared_at_ms: u64,
}

/// A repo `peer` shares with us.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingShare {
    pub peer: EndpointId,
    /// Self-reported display name at offer time.
    pub peer_name: String,
    /// Their project name — display only.
    pub name: String,
    pub origin_url: String,
    /// Our matching project, resolved daemon-side when the share or the
    /// project list last changed. `None` means we don't have this repo.
    #[serde(default)]
    pub matched_path: Option<PathBuf>,
    #[serde(default)]
    pub matched_name: Option<String>,
    /// The peer lets us watch this project's sessions — learned from the
    /// `share_sessions` flag on their `SharedRepo` entries.
    #[serde(default)]
    pub share_sessions: bool,
    pub received_at_ms: u64,
}

/// A sync relationship we opted into — manually on a friend's share, or
/// auto-created when a friend enabled sync on one of ours. Links are
/// mutual: either side tearing down removes both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncLink {
    /// Local identifier (uuid string) for commands and alerts.
    pub id: String,
    pub peer: EndpointId,
    pub origin_url: String,
    pub repo_path: PathBuf,
    /// Push commits that land on enabled branches. On unless disabled.
    #[serde(default = "default_auto_push")]
    pub auto_push: bool,
    /// Branches we integrate when the peer pushes — the repo's default
    /// branch starts enabled; the rest are opt-in.
    #[serde(default)]
    pub enabled_branches: BTreeSet<String>,
    /// Enabled branches paused by an abort — a manual sync re-arms them.
    #[serde(default)]
    pub paused_branches: BTreeSet<String>,
    /// The peer acknowledged the link — their `SyncEnabled` answer (or
    /// their originating message) landed. Until then we keep re-sending
    /// ours on the slow sync cadence.
    #[serde(default)]
    pub peer_sync_enabled: bool,
    pub created_at_ms: u64,
}

fn default_auto_push() -> bool {
    true
}

/// Which integration owns a stopped checkout — decides `--abort` and the
/// "merge instead" retry.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Integration {
    Rebase,
    Merge,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncAlertKind {
    /// The integration stopped on conflicts; the user picks
    /// resolve-in-chat / merge-instead / abort.
    Conflict,
    /// Sync refused to touch a dirty checkout — retry once it's clean.
    RefusedDirtyWorktree,
}

/// A sync decision the user hasn't answered yet. Persisted because the
/// stopped rebase it describes survives a daemon restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAlert {
    pub id: String,
    pub link_id: String,
    pub branch: String,
    pub kind: SyncAlertKind,
    /// Set while a rebase/merge is stopped on conflicts.
    #[serde(default)]
    pub in_progress: Option<Integration>,
    /// Working-tree paths still carrying conflict markers.
    #[serde(default)]
    pub files: Vec<String>,
    /// Where the integration is stopped — the repo root, or the temp
    /// worktree created for a branch checked out nowhere.
    pub worktree_path: PathBuf,
    /// A temp worktree we created and own — removed on abort/resolution.
    #[serde(default)]
    pub temp_worktree: bool,
    pub at_ms: u64,
}

/// Persisted share document — `share.json` under the share dir.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ShareStore {
    #[serde(default)]
    pub outgoing: Vec<OutgoingShare>,
    #[serde(default)]
    pub incoming: Vec<IncomingShare>,
    #[serde(default)]
    pub links: Vec<SyncLink>,
    #[serde(default)]
    pub alerts: Vec<SyncAlert>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl ShareStore {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join("share.json");
        let mut store = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        store.path = Some(path);
        Ok(store)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// The link between `peer` and `origin_url` (normalized), if any.
    pub fn link_for(&self, peer: &EndpointId, origin_url: &str) -> Option<&SyncLink> {
        self.links.iter().find(|l| {
            l.peer == *peer && normalize_origin(&l.origin_url) == normalize_origin(origin_url)
        })
    }

    pub fn link_for_mut(
        &mut self,
        peer: &EndpointId,
        origin_url: &str,
    ) -> Option<&mut SyncLink> {
        self.links.iter_mut().find(|l| {
            l.peer == *peer && normalize_origin(&l.origin_url) == normalize_origin(origin_url)
        })
    }
}

/// Canonical compare key for remote URLs: `git@host:org/repo.git`,
/// `ssh://git@host/org/repo`, and `https://host/org/repo.git` all collapse
/// to `host/org/repo`.
pub fn normalize_origin(url: &str) -> String {
    let mut s = url.trim().trim_end_matches('/').to_string();
    for prefix in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
            break;
        }
    }
    // `git@host:org/repo` and `ssh://git@host/org/repo` → `host/org/repo`.
    if let Some(rest) = s.strip_prefix("git@") {
        s = rest.replacen(':', "/", 1);
    }
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    #[test]
    fn normalize_origin_collapses_equivalents() {
        let expected = "github.com/org/repo";
        for url in [
            "git@github.com:org/repo.git",
            "git@github.com:org/repo",
            "https://github.com/org/repo.git",
            "https://github.com/org/repo",
            "https://github.com/org/repo/",
            "ssh://git@github.com/org/repo.git",
            "https://GitHub.com/Org/Repo.git",
        ] {
            assert_eq!(normalize_origin(url), expected, "{url}");
        }
    }

    #[test]
    fn store_round_trips() {
        let dir = std::env::temp_dir().join(format!("waku-share-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = ShareStore::load(&dir).unwrap();
        let peer = SecretKey::generate().public();
        store.outgoing.push(OutgoingShare {
            peer,
            name: "proj".into(),
            origin_url: "git@github.com:org/repo.git".into(),
            repo_path: PathBuf::from("/tmp/proj"),
            peer_sync_enabled: false,
            share_sessions: false,
            shared_at_ms: now_ms(),
        });
        store.save().unwrap();
        let loaded = ShareStore::load(&dir).unwrap();
        assert_eq!(loaded.outgoing.len(), 1);
        assert_eq!(loaded.outgoing[0].name, "proj");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
