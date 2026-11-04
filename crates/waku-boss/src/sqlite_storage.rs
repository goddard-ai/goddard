//! Transactional Boss persistence. JSON is bounded to individual domain records;
//! collections and histories have independent rows, and large immutable payloads
//! are shared by digest. The protocol snapshot remains an in-memory projection.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use waku_base::persistence_metrics::WriteMeasurement;

use anyhow::{Context, ensure};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use waku_protocol::boss::BossState;

// Keep this layout in db/schema.ts; the generated migration is also usable by
// standalone Boss hosts, which do not own the task store's migration runner.
const SCHEMA: &str = include_str!("../../../db/migrations/0033_clever_nitro.sql");
const VERSION: i64 = 1;

struct Collection {
    field: &'static str,
    children: &'static [Collection],
    payloads: &'static [&'static str],
}

const COLLECTIONS: &[Collection] = &[
    Collection {
        field: "chains",
        payloads: &["request"],
        children: &[
            Collection {
                field: "steps",
                payloads: &["definition"],
                children: &[
                    Collection {
                        field: "attempts",
                        payloads: &["sealed", "provisionalResult", "terminal"],
                        children: &[],
                    },
                    Collection {
                        field: "approvals",
                        payloads: &[],
                        children: &[],
                    },
                ],
            },
            Collection {
                field: "history",
                payloads: &[],
                children: &[],
            },
        ],
    },
    Collection {
        field: "outcomes",
        payloads: &[],
        children: &[
            Collection {
                field: "assignments",
                payloads: &[],
                children: &[],
            },
            Collection {
                field: "handoffs",
                payloads: &[],
                children: &[],
            },
            Collection {
                field: "history",
                payloads: &[],
                children: &[],
            },
        ],
    },
    Collection {
        field: "planning",
        payloads: &[],
        children: &[
            Collection {
                field: "approvedRevisions",
                payloads: &["content"],
                children: &[],
            },
            Collection {
                field: "items",
                payloads: &[],
                children: &[],
            },
            Collection {
                field: "history",
                payloads: &[],
                children: &[],
            },
        ],
    },
    Collection {
        field: "personas",
        payloads: &["markdown"],
        children: &[],
    },
    Collection {
        field: "employees",
        payloads: &["requestFingerprint"],
        children: &[],
    },
    Collection {
        field: "retiredEmployees",
        payloads: &["requestFingerprint"],
        children: &[],
    },
    Collection {
        field: "deliverables",
        payloads: &[],
        children: &[],
    },
    Collection {
        field: "instructionPins",
        payloads: &[],
        children: &[],
    },
    Collection {
        field: "completionReports",
        payloads: &[],
        children: &[],
    },
    Collection {
        field: "outbox",
        payloads: &[],
        children: &[],
    },
    Collection {
        field: "waves",
        payloads: &[],
        children: &[],
    },
    Collection {
        field: "waveOutbox",
        payloads: &[],
        children: &[],
    },
];

#[derive(Default, PartialEq)]
struct Snapshot {
    coordinator: String,
    // (collection, parent, identity) -> (position, lifecycle, bounded JSON)
    rows: BTreeMap<(String, String, String), (i64, Option<String>, String)>,
    payloads: BTreeMap<String, String>,
}

fn extract(
    value: &mut Value,
    parent: &str,
    prefix: &str,
    specs: &[Collection],
    snapshot: &mut Snapshot,
) -> anyhow::Result<()> {
    for spec in specs {
        let Some(array) = value.get_mut(spec.field).and_then(Value::as_array_mut) else {
            continue;
        };
        let collection = format!("{prefix}{}", spec.field);
        for (position, mut row) in std::mem::take(array).into_iter().enumerate() {
            let identity = ["id", "sessionId", "eventId"]
                .iter()
                .find_map(|key| row.get(key))
                .map(Value::to_string)
                .unwrap_or_else(|| position.to_string());
            // JSON encodes the tuple unambiguously even for user-supplied IDs.
            let path = serde_json::to_string(&(&collection, parent, &identity))?;
            extract(
                &mut row,
                &path,
                &format!("{collection}."),
                spec.children,
                snapshot,
            )?;
            for field in spec.payloads {
                if let Some(payload) = row.get_mut(*field).filter(|v| !v.is_null()) {
                    let data = serde_json::to_string(payload)?;
                    let digest = format!("{:x}", Sha256::digest(data.as_bytes()));
                    snapshot.payloads.insert(digest.clone(), data);
                    *payload = json!({"payload": digest});
                }
            }
            let lifecycle = row
                .get("status")
                .or_else(|| row.get("state"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let key = (collection.clone(), parent.to_owned(), identity);
            ensure!(
                snapshot
                    .rows
                    .insert(
                        key,
                        (position as i64, lifecycle, serde_json::to_string(&row)?)
                    )
                    .is_none(),
                "duplicate Boss record identity"
            );
        }
    }
    Ok(())
}

fn encode(state: &BossState) -> anyhow::Result<Snapshot> {
    let mut value = serde_json::to_value(state)?;
    let mut snapshot = Snapshot::default();
    extract(&mut value, "", "", COLLECTIONS, &mut snapshot)?;
    snapshot.coordinator = serde_json::to_string(&value)?;
    Ok(snapshot)
}

fn restore(
    value: &mut Value,
    parent: &str,
    prefix: &str,
    specs: &[Collection],
    snapshot: &Snapshot,
    visited: &mut BTreeSet<(String, String, String)>,
) -> anyhow::Result<()> {
    for spec in specs {
        let Some(array) = value.get_mut(spec.field).and_then(Value::as_array_mut) else {
            continue;
        };
        let collection = format!("{prefix}{}", spec.field);
        let mut rows = snapshot
            .rows
            .range((collection.clone(), parent.to_owned(), String::new())..)
            .take_while(|((kind, owner, _), _)| kind == &collection && owner == parent)
            .collect::<Vec<_>>();
        rows.sort_by_key(|(_, (position, _, _))| *position);
        for (expected, (key, (position, _, data))) in rows.into_iter().enumerate() {
            ensure!(*position == expected as i64, "invalid Boss record ordering");
            visited.insert(key.clone());
            let mut row: Value = serde_json::from_str(data)?;
            for field in spec.payloads {
                if let Some(reference) = row.get_mut(*field).filter(|v| !v.is_null()) {
                    let digest = reference
                        .get("payload")
                        .and_then(Value::as_str)
                        .context("invalid Boss payload reference")?;
                    let data = snapshot
                        .payloads
                        .get(digest)
                        .context("missing Boss payload")?;
                    ensure!(
                        format!("{:x}", Sha256::digest(data.as_bytes())) == digest,
                        "Boss payload digest mismatch"
                    );
                    *reference = serde_json::from_str(data)?;
                }
            }
            let path = serde_json::to_string(&(&key.0, &key.1, &key.2))?;
            restore(
                &mut row,
                &path,
                &format!("{collection}."),
                spec.children,
                snapshot,
                visited,
            )?;
            array.push(row);
        }
    }
    Ok(())
}

struct Saved {
    revision: Option<i64>,
    snapshot: Snapshot,
}

/// The committed revision moved past this host's baseline — another writer
/// owns the latest state. Detectable through context layers so the service
/// can reload instead of rejecting every later save.
#[derive(Debug)]
pub(super) struct RevisionConflict {
    /// The revision this host last committed — what the write expected.
    pub expected: Option<i64>,
    /// The revision another host committed meanwhile.
    pub observed: Option<i64>,
}

impl std::fmt::Display for RevisionConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Boss database changed in another host; reload before saving")
    }
}

impl std::error::Error for RevisionConflict {}

pub(super) fn revision_conflict(error: &anyhow::Error) -> Option<&RevisionConflict> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<RevisionConflict>())
}

pub(super) struct SqliteStorage {
    path: PathBuf,
    saved: Mutex<Option<Saved>>,
}

impl SqliteStorage {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            saved: Mutex::new(None),
        }
    }
    pub fn clear(&self) {
        *self.saved.lock() = None;
    }

    fn open(&self) -> anyhow::Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        // Approval and admission commits need durable acknowledgement, unlike
        // the task store's per-second UI projection saves.
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        connection.execute_batch(SCHEMA)?;
        Ok(connection)
    }

    pub fn load(&self) -> anyhow::Result<Option<BossState>> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        let header: Option<(i64, i64, String)> = transaction
            .query_row(
                "SELECT schema_version, revision, data FROM boss_coordinator WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((version, revision, coordinator)) = header else {
            let count: i64 =
                transaction.query_row("SELECT count(*) FROM boss_records", [], |r| r.get(0))?;
            ensure!(count == 0, "Boss records exist without a coordinator");
            *self.saved.lock() = Some(Saved {
                revision: None,
                snapshot: Snapshot::default(),
            });
            return Ok(None);
        };
        ensure!(
            version == VERSION,
            "unsupported Boss database schema {version}"
        );
        let mut snapshot = Snapshot {
            coordinator,
            ..Default::default()
        };
        {
            let mut statement = transaction.prepare(
                "SELECT collection, parent_id, id, position, lifecycle, data FROM boss_records",
            )?;
            for row in statement.query_map([], |r| {
                Ok((
                    (r.get(0)?, r.get(1)?, r.get(2)?),
                    (r.get(3)?, r.get(4)?, r.get(5)?),
                ))
            })? {
                let (key, value) = row?;
                snapshot.rows.insert(key, value);
            }
            let mut statement = transaction.prepare("SELECT digest, data FROM boss_payloads")?;
            for row in statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))? {
                let (key, value) = row?;
                snapshot.payloads.insert(key, value);
            }
        }
        transaction.commit()?;
        let mut value: Value = serde_json::from_str(&snapshot.coordinator)?;
        let mut visited = BTreeSet::new();
        restore(&mut value, "", "", COLLECTIONS, &snapshot, &mut visited)?;
        ensure!(
            visited.len() == snapshot.rows.len(),
            "unreachable Boss records"
        );
        let state: BossState =
            serde_json::from_value(value).context("invalid Boss database records")?;
        *self.saved.lock() = Some(Saved {
            revision: Some(revision),
            snapshot,
        });
        Ok(Some(state))
    }

    pub fn unchanged(&self, state: &BossState) -> anyhow::Result<bool> {
        let snapshot = encode(state)?;
        Ok(self
            .saved
            .lock()
            .as_ref()
            .is_some_and(|saved| saved.revision.is_some() && saved.snapshot == snapshot))
    }

    pub fn save(&self, state: &BossState) -> anyhow::Result<()> {
        self.save_with(state, None)
    }

    pub fn save_with(
        &self,
        state: &BossState,
        host: Option<&mut dyn crate::SessionCommit>,
    ) -> anyhow::Result<()> {
        let mut measurement = WriteMeasurement::new(if host.is_some() {
            "boss_shared_prepare"
        } else {
            "boss"
        });
        let snapshot = encode(state)?;
        let mut saved = self.saved.lock();
        let previous = saved
            .as_ref()
            .context("Boss database must be loaded before saving")?;
        // An identical snapshot must not pay a revision bump — open and
        // activate save unconditionally, and a gratuitous commit would race
        // other hosts on this database. A shared commit still owns its
        // session rows, so the host transaction runs with an empty write.
        if snapshot == previous.snapshot {
            if let Some(host) = host {
                ensure!(
                    host.database_path() == self.path,
                    "Boss and sessions must use the same database"
                );
                host.commit(&mut |_: &rusqlite::Transaction<'_>| Ok(()))?;
            }
            measurement.sample.success = true;
            return Ok(());
        }
        // Compute the entire diff before acquiring SQLite's single writer lock.
        let payloads = snapshot
            .payloads
            .iter()
            .filter(|(key, _)| !previous.snapshot.payloads.contains_key(*key))
            .collect::<Vec<_>>();
        let rows = snapshot
            .rows
            .iter()
            .filter(|(key, value)| previous.snapshot.rows.get(*key) != Some(*value))
            .collect::<Vec<_>>();
        let removed = previous
            .snapshot
            .rows
            .keys()
            .filter(|key| !snapshot.rows.contains_key(*key))
            .collect::<Vec<_>>();
        let removed_payloads = previous
            .snapshot
            .payloads
            .keys()
            .filter(|key| !snapshot.payloads.contains_key(*key))
            .collect::<Vec<_>>();
        let next_revision = previous
            .revision
            .unwrap_or(0)
            .checked_add(1)
            .context("Boss storage revision exhausted")?;
        let mut calls = 0;
        let mut write = |transaction: &rusqlite::Transaction<'_>| -> anyhow::Result<()> {
            calls += 1;
            ensure!(calls == 1, "Boss write invoked more than once");
            let revision: Option<i64> = transaction
                .query_row(
                    "SELECT revision FROM boss_coordinator WHERE id=1",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            if revision != previous.revision {
                return Err(RevisionConflict {
                    expected: previous.revision,
                    observed: revision,
                }
                .into());
            }
            for (digest, data) in &payloads {
                transaction.execute(
                    "INSERT OR IGNORE INTO boss_payloads(digest,data) VALUES(?1,?2)",
                    params![digest, data],
                )?;
            }
            for ((collection, parent, id), (position, lifecycle, data)) in &rows {
                transaction.execute("INSERT INTO boss_records(collection,parent_id,id,position,lifecycle,data) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(collection,parent_id,id) DO UPDATE SET position=excluded.position,lifecycle=excluded.lifecycle,data=excluded.data", params![collection,parent,id,position,lifecycle,data])?;
            }
            for (collection, parent, id) in &removed {
                transaction.execute(
                    "DELETE FROM boss_records WHERE collection=?1 AND parent_id=?2 AND id=?3",
                    params![collection, parent, id],
                )?;
            }
            for digest in &removed_payloads {
                transaction.execute("DELETE FROM boss_payloads WHERE digest=?1", [digest])?;
            }
            transaction.execute("INSERT INTO boss_coordinator(id,schema_version,revision,data) VALUES(1,?1,?2,?3) ON CONFLICT(id) DO UPDATE SET schema_version=excluded.schema_version,revision=excluded.revision,data=excluded.data",params![VERSION,next_revision,snapshot.coordinator])?;
            Ok(())
        };
        measurement.sample.rows_changed =
            (payloads.len() + rows.len() + removed.len() + removed_payloads.len() + 1) as u64;
        measurement.prepared();
        if let Some(host) = host {
            ensure!(
                host.database_path() == self.path,
                "Boss and sessions must use the same database"
            );
            host.commit(&mut write)?;
        } else {
            let mut connection = self.open()?;
            measurement.prepared();
            let started = Instant::now();
            let result = connection.transaction_with_behavior(TransactionBehavior::Immediate);
            measurement.waited(started.elapsed());
            let transaction = result?;
            let started = Instant::now();
            let result = write(&transaction);
            measurement.worked(started.elapsed());
            result?;
            let started = Instant::now();
            let result = transaction.commit();
            measurement.committed(started.elapsed());
            result?;
        }
        ensure!(calls == 1, "Host did not write Boss records");
        *saved = Some(Saved {
            revision: Some(next_revision),
            snapshot,
        });
        measurement.sample.success = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BossService;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn import_is_atomic_preserves_backup_and_database_is_authoritative() {
        let root = std::env::temp_dir().join(format!("boss-sqlite-{}", Uuid::new_v4()));
        let boss_root = root.join("boss");
        let database = root.join("app.db");
        let legacy = BossService::open(boss_root.clone()).unwrap();
        legacy
            .update(|state| {
                state.next_sequence = 42;
                Ok(())
            })
            .unwrap();
        let original = fs::read(boss_root.join("boss.json")).unwrap();
        let connection = SqliteStorage::new(database.clone()).open().unwrap();
        connection.execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON boss_coordinator BEGIN SELECT RAISE(ABORT, 'disk failure'); END;").unwrap();
        assert!(BossService::open_database(boss_root.clone(), database.clone()).is_err());
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM boss_records", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(fs::read(boss_root.join("boss.json")).unwrap(), original);
        connection
            .execute_batch("DROP TRIGGER reject_import;")
            .unwrap();
        let service = BossService::open_database(boss_root.clone(), database.clone()).unwrap();
        assert_eq!(service.document().next_sequence, 42);
        assert_eq!(fs::read(boss_root.join("boss.json")).unwrap(), original);
        // Once imported, damaged or stale legacy backups cannot replace DB state.
        fs::write(boss_root.join("boss.json"), "invalid backup").unwrap();
        let coordinator_revision = || {
            connection
                .query_row(
                    "SELECT revision FROM boss_coordinator WHERE id=1",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        // An idle open must not write: a gratuitous revision bump on every
        // second host's boot would poison the live host's baseline without
        // changing state. The same applies to deactivate + activate.
        let other = BossService::open_database(boss_root.clone(), database.clone()).unwrap();
        assert_eq!(other.document().next_sequence, 42);
        assert_eq!(coordinator_revision(), 1);
        other.deactivate();
        other.activate().unwrap();
        assert_eq!(coordinator_revision(), 1);
        // A late write from a second host must not overwrite a newer commit.
        other
            .update(|state| {
                state.next_sequence = 43;
                Ok(())
            })
            .unwrap();
        assert!(
            service
                .update(|state| {
                    state.next_sequence = 99;
                    Ok(())
                })
                .is_err()
        );
        // The conflicted host reloads the committed state instead of
        // rejecting every later save; its next write lands on the new base.
        assert_eq!(service.document().next_sequence, 43);
        service
            .update(|state| {
                state.next_sequence = 44;
                Ok(())
            })
            .unwrap();
        // `other` is the stale host now — its first save reloads too.
        assert!(
            other
                .update(|state| {
                    state.next_sequence = 45;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(other.document().next_sequence, 44);
        let before = other.document().revision;
        connection.execute_batch("CREATE TRIGGER reject_commit BEFORE UPDATE ON boss_coordinator BEGIN SELECT RAISE(ABORT, 'disk failure'); END;").unwrap();
        assert!(
            other
                .update(|state| {
                    state.next_sequence = 45;
                    state.personas.clear();
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(other.document().revision, before);
        connection
            .execute_batch("DROP TRIGGER reject_commit;")
            .unwrap();
        // A coordinator-only edit must not rewrite unrelated domain records.
        connection.execute_batch("CREATE TRIGGER reject_record_rewrite BEFORE UPDATE ON boss_records BEGIN SELECT RAISE(ABORT, 'unrelated record rewritten'); END;").unwrap();
        other
            .update(|state| {
                state.next_sequence = 45;
                Ok(())
            })
            .unwrap();
        let reopened = BossService::open_database(boss_root.clone(), database.clone()).unwrap();
        assert_eq!(reopened.document().next_sequence, 45);
        assert!(!reopened.document().personas.is_empty());
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nested_history_and_shared_payloads_round_trip_without_marker_collisions() {
        let definition =
            json!({"prompt": "a long instruction", "permissions": {"payload": "user data"}});
        let mut value = json!({"chains": [{"id":"chain", "request": definition, "steps":[{"definition":definition,"attempts":[{"id":"attempt","sealed":{"payload":"user evidence"}}],"approvals":[]}], "history":[{"eventId":1,"event":"created"}]}]});
        let original = value.clone();
        let mut snapshot = Snapshot::default();
        extract(&mut value, "", "", COLLECTIONS, &mut snapshot).unwrap();
        assert_eq!(
            snapshot.payloads.len(),
            2,
            "request and definition share content"
        );
        let mut visited = BTreeSet::new();
        restore(&mut value, "", "", COLLECTIONS, &snapshot, &mut visited).unwrap();
        assert_eq!(value, original);
        assert_eq!(visited.len(), snapshot.rows.len());
    }
}
