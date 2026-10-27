//! Active Boss state and bounded, separately persisted employee history.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, ensure};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use waku_protocol::boss::{BossEmployee, BossState};

#[derive(Clone, Deserialize, Serialize)]
struct HistoryReference {
    slot: u8,
    digest: String,
}

impl HistoryReference {
    fn path(&self, root: &Path) -> anyhow::Result<PathBuf> {
        ensure!(self.slot < 2, "invalid Boss employee history slot");
        Ok(root.join(format!("retired-employees-{}.json", self.slot)))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredState {
    #[serde(flatten)]
    state: BossState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retired_history: Option<HistoryReference>,
}

struct SavedState {
    retired: Arc<Vec<BossEmployee>>,
    history: Option<HistoryReference>,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct StateStorage {
    saved: Mutex<Option<SavedState>>,
}

impl StateStorage {
    pub fn clear(&self) {
        *self.saved.lock() = None;
    }

    pub fn load(&self, root: &Path) -> anyhow::Result<Option<BossState>> {
        self.clear();
        let bytes = match fs::read(root.join("boss.json")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut stored: StoredState =
            serde_json::from_slice(&bytes).context("invalid Boss document")?;
        if let Some(history) = &mut stored.retired_history {
            ensure!(
                stored.state.retired_employees.is_empty(),
                "Boss employee history has two sources"
            );
            let history_bytes =
                fs::read(history.path(root)?).context("reading Boss employee history")?;
            stored.state.retired_employees =
                serde_json::from_slice(&history_bytes).context("invalid Boss employee history")?;
            let digest = format!("{:x}", Sha256::digest(&history_bytes));
            if digest != history.digest {
                // A byte mismatch alone need not make valid history unavailable.
                // Refuse empty or inconsistent rosters rather than blessing data loss.
                let mut identities = std::collections::HashSet::new();
                ensure!(
                    !stored.state.retired_employees.is_empty()
                        && stored
                            .state
                            .retired_employees
                            .iter()
                            .all(|employee| employee.expired()
                                && identities.insert(employee.session_id)),
                    "Boss employee history digest mismatch with an invalid retired roster"
                );
                let backup = root.join(format!("history-recovery-{}", uuid::Uuid::new_v4()));
                fs::create_dir(&backup).context("preserving mismatched Boss history")?;
                super::atomic_write(&backup.join("boss.json"), &bytes)?;
                super::atomic_write(&backup.join("history.json"), &history_bytes)?;
                sync_directory(&backup)?;
                sync_directory(root)?;
                eprintln!(
                    "Boss employee history digest mismatch; recovering valid history; original files preserved in {}",
                    backup.display()
                );
                history.digest = digest;
            }
        }
        // A legacy inline history is migrated by the next successful save.
        if stored.retired_history.is_some() || stored.state.retired_employees.is_empty() {
            *self.saved.lock() = Some(SavedState {
                retired: stored.state.retired_employees.clone(),
                bytes: Self::encode(&stored.state, stored.retired_history.clone())?,
                history: stored.retired_history,
            });
        }
        Ok(Some(stored.state))
    }

    pub fn unchanged(&self, state: &BossState) -> anyhow::Result<bool> {
        let saved = self.saved.lock();
        let Some(saved) = saved.as_ref() else {
            return Ok(false);
        };
        if !Self::same_history(saved, state)? {
            return Ok(false);
        }
        Ok(Self::encode(state, saved.history.clone())? == saved.bytes)
    }

    pub fn save(&self, root: &Path, state: &BossState) -> anyhow::Result<()> {
        let mut saved = self.saved.lock();
        let same_history = saved
            .as_ref()
            .map(|previous| Self::same_history(previous, state))
            .transpose()?
            .unwrap_or(false);
        let history = if same_history {
            saved.as_ref().and_then(|previous| previous.history.clone())
        } else if state.retired_employees.is_empty() {
            None
        } else {
            // Before reusing the inactive slot, make the previous manifest's
            // rename durable. Never overwrite the history it still references.
            sync_directory(root)?;
            let bytes = serde_json::to_vec(&state.retired_employees)?;
            let history = HistoryReference {
                slot: saved
                    .as_ref()
                    .and_then(|previous| previous.history.as_ref())
                    .map_or(0, |previous| 1 - previous.slot),
                digest: format!("{:x}", Sha256::digest(&bytes)),
            };
            super::atomic_write(&history.path(root)?, &bytes)?;
            sync_directory(root)?;
            Some(history)
        };
        let bytes = Self::encode(state, history.clone())?;
        super::atomic_write(&root.join("boss.json"), &bytes)?;
        // Publish the cache only after the active document commits. A failed
        // commit leaves both the in-memory state and its history slot intact.
        *saved = Some(SavedState {
            retired: state.retired_employees.clone(),
            history,
            bytes,
        });
        Ok(())
    }

    fn encode(state: &BossState, history: Option<HistoryReference>) -> anyhow::Result<Vec<u8>> {
        let mut active = state.clone();
        active.retired_employees = Arc::default();
        Ok(serde_json::to_vec(&StoredState {
            state: active,
            retired_history: history,
        })?)
    }

    fn same_history(saved: &SavedState, state: &BossState) -> anyhow::Result<bool> {
        if Arc::ptr_eq(&saved.retired, &state.retired_employees) {
            return Ok(true);
        }
        let Some(history) = &saved.history else {
            return Ok(state.retired_employees.is_empty());
        };
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&state.retired_employees)?)
        ) == history.digest)
    }
}

fn sync_directory(root: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    fs::File::open(root)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::BossService;
    use super::*;
    use uuid::Uuid;
    use waku_protocol::boss::{EmployeeGoal, EmployeeLifecycle};

    fn legacy_history(root: &Path) -> Uuid {
        let service = BossService::open(root.to_path_buf()).unwrap();
        let boss = Uuid::new_v4();
        service.set_session_id(boss).unwrap();
        let mut employee = service
            .prepare_employee(
                boss,
                None,
                "Historical assignment".into(),
                None,
                false,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap();
        employee.set_lifecycle(EmployeeLifecycle::Expired, 100);
        let id = employee.session_id;
        let mut ticket = super::super::memory_op_tests::test_ticket(None);
        ticket.prompt = "retained assignment context ".repeat(50_000);
        employee.ticket = Some(ticket);
        let mut state = service.document();
        state.retired_employees = Arc::new(vec![employee]);
        drop(service);
        fs::write(root.join("boss.json"), serde_json::to_vec(&state).unwrap()).unwrap();
        id
    }

    #[test]
    fn legacy_history_migrates_and_active_updates_leave_it_untouched() {
        let root = std::env::temp_dir().join(format!("boss-history-{}", Uuid::new_v4()));
        let id = legacy_history(&root);
        let service = BossService::open(root.clone()).unwrap();
        let stored: StoredState =
            serde_json::from_slice(&fs::read(root.join("boss.json")).unwrap()).unwrap();
        assert!(stored.state.retired_employees.is_empty());
        let history = stored.retired_history.unwrap().path(&root).unwrap();
        assert!(
            fs::metadata(root.join("boss.json")).unwrap().len()
                < fs::metadata(&history).unwrap().len()
        );
        let original_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(100);
        fs::File::options()
            .write(true)
            .open(&history)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(original_time))
            .unwrap();
        for _ in 0..3 {
            service.set_session_id(Uuid::new_v4()).unwrap();
        }
        assert_eq!(
            fs::metadata(&history).unwrap().modified().unwrap(),
            original_time
        );
        let revision = service.document().revision;
        service
            .update(|state| {
                // Merely obtaining mutable history must not publish a change.
                let _ = Arc::make_mut(&mut state.retired_employees);
                Ok(())
            })
            .unwrap();
        assert_eq!(service.document().revision, revision);
        drop(service);
        let reopened = BossService::open(root.clone()).unwrap();
        assert_eq!(reopened.document().retired_employees[0].session_id, id);
        assert!(
            reopened.document().retired_employees[0]
                .ticket
                .as_ref()
                .unwrap()
                .prompt
                .len()
                > 1_000_000
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_history_publication_preserves_the_committed_snapshot() {
        let root = std::env::temp_dir().join(format!("boss-history-failure-{}", Uuid::new_v4()));
        legacy_history(&root);
        let service = BossService::open(root.clone()).unwrap();
        let path = root.join("boss.json");
        let committed = fs::read(&path).unwrap();
        let revision = service.document().revision;
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(
            service
                .update(|state| {
                    Arc::make_mut(&mut state.retired_employees)[0].job_title = "uncommitted".into();
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(service.document().revision, revision);
        assert_eq!(
            service.document().retired_employees[0].job_title,
            "Historical assignment"
        );
        fs::remove_dir(&path).unwrap();
        fs::write(&path, committed).unwrap();
        drop(service);
        let reopened = BossService::open(root.clone()).unwrap();
        assert_eq!(
            reopened.document().retired_employees[0].job_title,
            "Historical assignment"
        );
        for index in 0..4 {
            reopened
                .update(|state| {
                    Arc::make_mut(&mut state.retired_employees)[0].job_title =
                        format!("revision {index}");
                    Ok(())
                })
                .unwrap();
        }
        drop(reopened);
        let reopened = BossService::open(root.clone()).unwrap();
        assert_eq!(
            reopened.document().retired_employees[0].job_title,
            "revision 3"
        );
        let history_files = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("retired-employees-")
            })
            .count();
        assert_eq!(
            history_files, 2,
            "history updates must not accumulate full snapshots"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mismatched_valid_history_recovers_once_and_preserves_original_files() {
        let root = std::env::temp_dir().join(format!("boss-history-recovery-{}", Uuid::new_v4()));
        let id = legacy_history(&root);
        drop(BossService::open(root.clone()).unwrap());
        let manifest = root.join("boss.json");
        let original = fs::read(&manifest).unwrap();
        let stored: StoredState = serde_json::from_slice(&original).unwrap();
        let history = stored.retired_history.unwrap().path(&root).unwrap();
        let mut employees: Vec<BossEmployee> =
            serde_json::from_slice(&fs::read(&history).unwrap()).unwrap();
        employees[0].job_title = "Recovered assignment".into();
        let changed = serde_json::to_vec_pretty(&employees).unwrap();
        fs::write(&history, &changed).unwrap();

        let service = BossService::open(root.clone()).unwrap();
        assert_eq!(service.document().retired_employees[0].session_id, id);
        assert_eq!(
            service.document().retired_employees[0].job_title,
            "Recovered assignment"
        );
        assert_eq!(fs::read(&history).unwrap(), changed);
        let repaired: StoredState = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(
            repaired.retired_history.unwrap().digest,
            format!("{:x}", Sha256::digest(&changed))
        );
        let backups = || {
            fs::read_dir(&root)
                .unwrap()
                .map(Result::unwrap)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("history-recovery-")
                })
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        };
        let preserved = backups();
        assert_eq!(preserved.len(), 1);
        assert_eq!(fs::read(preserved[0].join("boss.json")).unwrap(), original);
        assert_eq!(
            fs::read(preserved[0].join("history.json")).unwrap(),
            changed
        );
        drop(service);
        let reopened = BossService::open(root.clone()).unwrap();
        assert_eq!(reopened.document().retired_employees[0].session_id, id);
        assert_eq!(backups(), preserved);
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_or_corrupt_history_never_becomes_an_empty_roster() {
        for missing in [true, false] {
            let root =
                std::env::temp_dir().join(format!("boss-history-corrupt-{}", Uuid::new_v4()));
            legacy_history(&root);
            drop(BossService::open(root.clone()).unwrap());
            let committed = fs::read(root.join("boss.json")).unwrap();
            let stored: StoredState = serde_json::from_slice(&committed).unwrap();
            let history = stored.retired_history.unwrap().path(&root).unwrap();
            if missing {
                fs::remove_file(&history).unwrap();
            } else {
                fs::write(&history, "[]").unwrap();
            }
            assert!(BossService::open(root.clone()).is_err());
            assert_eq!(fs::read(root.join("boss.json")).unwrap(), committed);
            fs::remove_dir_all(root).unwrap();
        }
    }
}
