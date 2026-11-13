use super::*;
use waku_protocol::scratch::{ScratchEntry, ScratchOperation};
use waku_protocol::workspace::ReclaimFailure;

/// How many named directories one task's scratch pool holds — bounds the
/// directory a sweep or `list` ever walks.
const MAX_SCRATCH_ENTRIES: usize = 64;

/// Characters a scratch name may carry — no separators or parent hops, so
/// a name can never address outside the task's pool.
fn valid_scratch_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name.bytes().all(
            |byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'_' | b'-'),
        )
}

/// Mark `scratch/` out of Time Machine's reach — the pool is reproducible
/// output, so backing it up buys nothing. Best-effort like every cleanup
/// here; an unsupported filesystem just keeps its backups.
#[cfg(target_os = "macos")]
fn exclude_scratch_from_backup(root: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = CString::new(root.as_os_str().as_bytes()) else {
        return;
    };
    const NAME: &[u8] = b"com.apple.metadata:com_apple_backup_excludeItem\0";
    const VALUE: &[u8] = b"com.apple.backupd";
    // SAFETY: both C strings are NUL-terminated by construction and
    // outlive the call; `value` is only read.
    unsafe {
        libc::setxattr(
            path.as_ptr(),
            NAME.as_ptr().cast(),
            VALUE.as_ptr().cast(),
            VALUE.len(),
            0,
            0,
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn exclude_scratch_from_backup(_root: &Path) {}

fn scratch_entries(pool: &Path) -> Vec<ScratchEntry> {
    let mut entries: Vec<ScratchEntry> = match std::fs::read_dir(pool) {
        Ok(read) => read
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| {
                entry.file_name().to_str().map(|name| ScratchEntry {
                    name: name.to_owned(),
                    path: entry.path(),
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

impl WakuBackend {
    /// `<data dir>/scratch` — every task's pool lives under it keyed by
    /// root task id, so a sweep attributes directories without stored
    /// state.
    fn scratch_root(&self) -> PathBuf {
        self.data_dir.join("scratch")
    }

    /// The task a scratch pool belongs to: the calling session's root
    /// ancestor, so side chats share their parent's pool. `Err` when the
    /// session is unknown, its ancestry is broken, or the root task is
    /// archived — a swept pool must not silently reappear under a dead
    /// owner.
    fn scratch_task(&self, session_id: Uuid) -> anyhow::Result<Uuid> {
        let state = self.task_state.lock();
        let mut id = session_id;
        for _ in 0..32 {
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == id)
                .ok_or_else(|| anyhow!("unknown task {id}"))?;
            match session.side_chat_of {
                Some(parent) => id = parent,
                None if session.archived_at.is_some() => {
                    bail!("task {id} is archived")
                }
                None => return Ok(id),
            }
        }
        bail!("task {session_id} has a cyclic side-chat ancestry")
    }

    /// `goddard-agent scratch` — ensure or list the calling task's
    /// scratch directories. Creation is the agent's declaration that the
    /// contents are reproducible, which is why archive and removal may
    /// drop the pool without the worktree's dirty-work check.
    pub(super) fn agent_scratch(
        &self,
        agent: Option<Uuid>,
        session_id: Uuid,
        operation: ScratchOperation,
    ) -> anyhow::Result<ResponsePayload> {
        let caller = agent
            .or_else(|| {
                (!session_id.is_nil() && self.known_session(session_id)).then_some(session_id)
            })
            .ok_or_else(|| anyhow!("scratch requires a scoped task credential"))?;
        let pool = self
            .scratch_root()
            .join(self.scratch_task(caller)?.to_string());
        match operation {
            ScratchOperation::Get { name } => {
                let path = match &name {
                    Some(name) => {
                        if !valid_scratch_name(name) {
                            bail!("invalid scratch name `{name}` — use 1–64 of [A-Za-z0-9._-]");
                        }
                        let entries = scratch_entries(&pool);
                        if entries.len() >= MAX_SCRATCH_ENTRIES
                            && !entries.iter().any(|entry| &entry.name == name)
                        {
                            bail!("task scratch pool is full ({MAX_SCRATCH_ENTRIES} entries)");
                        }
                        pool.join(name)
                    }
                    None => pool.clone(),
                };
                std::fs::create_dir_all(&path).with_context(|| {
                    format!("could not create scratch directory {}", path.display())
                })?;
                exclude_scratch_from_backup(&self.scratch_root());
                Ok(ResponsePayload::AgentScratch {
                    path,
                    entries: Vec::new(),
                })
            }
            ScratchOperation::List => Ok(ResponsePayload::AgentScratch {
                entries: scratch_entries(&pool),
                path: pool,
            }),
        }
    }

    /// Drop the scratch pools of `session_ids` — best-effort like the
    /// projectless workspace's removal: a busy or unwritable directory
    /// never blocks task lifecycle, and the reconcile sweep takes what's
    /// left.
    pub(super) fn drop_scratch(&self, session_ids: &[Uuid]) {
        for id in session_ids {
            let path = self.scratch_root().join(id.to_string());
            if let Err(error) = std::fs::remove_dir_all(&path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "could not remove task scratch {}: {error:#}",
                    path.display()
                );
            }
        }
    }

    /// The session's pool size for `InspectTaskScratch`, or `None` when
    /// the task no longer holds one — unknown or archived owners are
    /// swept, not offered for reclaim.
    pub(super) fn inspect_task_scratch(&self, session_id: Uuid) -> WorkspaceResult {
        let path = match self.scratch_task(session_id) {
            Ok(root) => self.scratch_root().join(root.to_string()),
            Err(_) => {
                return WorkspaceResult::TaskScratch {
                    path: PathBuf::new(),
                    bytes: 0,
                };
            }
        };
        WorkspaceResult::TaskScratch {
            bytes: crate::workspace::directory_size(&path),
            path,
        }
    }

    /// `ReclaimTaskScratch` — resolve the session to its pool server-side
    /// and delete it, the scratch counterpart of `ReclaimPaths`'
    /// re-derived safe set. A task archived or removed between scan and
    /// confirm reaps nothing here: the sweep already took its pool.
    pub(super) fn reclaim_task_scratch(&self, session_id: Uuid) -> WorkspaceResult {
        let mut reclaimed_bytes = 0;
        let mut failures = Vec::new();
        if let Ok(root) = self.scratch_task(session_id) {
            let path = self.scratch_root().join(root.to_string());
            let bytes = crate::workspace::directory_size(&path);
            match std::fs::remove_dir_all(&path) {
                Ok(()) => reclaimed_bytes = bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => failures.push(ReclaimFailure {
                    path,
                    error: error.to_string(),
                }),
            }
        }
        WorkspaceResult::Reclaim {
            reclaimed_bytes,
            failures,
        }
    }

    /// Delete scratch pools whose owner no longer qualifies — archived,
    /// removed, or never known. Runs with the archive purge (daemon
    /// startup and every `LoadTaskState`), so retention needs no timer
    /// and a crash between the state write and the unlink heals on the
    /// next pass. Only UUID-named directories are in scope — anything
    /// else under `scratch/` is left alone.
    pub(super) fn sweep_scratch(&self) {
        let keep: HashSet<Uuid> = {
            let state = self.task_state.lock();
            state
                .sessions
                .iter()
                .filter(|session| session.archived_at.is_none())
                .map(|session| {
                    let mut id = session.id;
                    for _ in 0..32 {
                        match state
                            .sessions
                            .iter()
                            .find(|parent| parent.id == id)
                            .and_then(|parent| parent.side_chat_of)
                        {
                            Some(parent) => id = parent,
                            None => break,
                        }
                    }
                    id
                })
                .collect()
        };
        let root = self.scratch_root();
        let Ok(entries) = std::fs::read_dir(&root) else {
            return;
        };
        for entry in entries.flatten() {
            let owned = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<Uuid>().ok())
                .is_some_and(|id| !keep.contains(&id));
            if owned
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
                && let Err(error) = std::fs::remove_dir_all(entry.path())
            {
                eprintln!(
                    "could not sweep task scratch {}: {error:#}",
                    entry.path().display()
                );
            }
        }
    }
}
