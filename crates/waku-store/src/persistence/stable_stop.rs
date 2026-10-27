//! Persist explicit stop authority before contacting a surviving provider host.
//! Confirmation is historical evidence for this exact owner, not authority to
//! remove a workspace or retire any replacement attachment.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StableIngressOwner {
    pub path: PathBuf,
    pub grant_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StableStopIntent {
    pub command_id: Uuid,
    pub session_id: Uuid,
    pub namespace: String,
    pub root: PathBuf,
    pub runtime_instance: Uuid,
    pub runtime_session_id: Uuid,
    pub generation: u64,
    pub descriptor_owner: Option<StableIngressOwner>,
    /// Distinguishes a qualified absent surface from missing legacy evidence.
    pub descriptor_qualified: bool,
}

fn descriptor_evidence(intent: &StableStopIntent) -> io::Result<Option<String>> {
    if intent
        .descriptor_owner
        .as_ref()
        .is_some_and(|owner| !owner.path.is_absolute())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ingress retirement path must be absolute",
        ));
    }
    if intent.descriptor_qualified {
        serde_json::to_string(&intent.descriptor_owner)
            .map(Some)
            .map_err(to_io_error)
    } else if intent.descriptor_owner.is_none() {
        Ok(None)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unqualified ingress owner",
        ))
    }
}

fn confirmed_timestamp(value: Option<i64>) -> io::Result<bool> {
    match value {
        None => Ok(false),
        Some(value) if value >= 0 => Ok(true),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid stop confirmation timestamp",
        )),
    }
}

impl StateStore {
    /// Commit before provider contact. Repeating the exact intent returns
    /// whether termination was already confirmed. Changed ownership fails
    /// closed; it never replaces an unresolved command or its original id.
    pub fn stable_stop_begin(&self, intent: &StableStopIntent) -> io::Result<bool> {
        let connection = self.open()?;
        let root = intent.root.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "stable-runtime root is not UTF-8",
            )
        })?;
        let generation = i64::try_from(intent.generation).map_err(to_io_error)?;
        let descriptor_owner = descriptor_evidence(intent)?;
        let values = params![
            intent.command_id.to_string(),
            intent.session_id.to_string(),
            intent.namespace,
            root,
            intent.runtime_instance.to_string(),
            intent.runtime_session_id.to_string(),
            generation,
            descriptor_owner,
        ];
        connection
            .execute(
                "INSERT INTO stable_runtime_stops
             (command_id, session_id, namespace, root, runtime_instance,
              runtime_session_id, generation, descriptor_owner, requested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())
             ON CONFLICT(command_id) DO NOTHING",
                values,
            )
            .map_err(to_io_error)?;
        // Older records predate descriptor ownership. Only the exact target's
        // qualified current link may fill this missing evidence; once bound,
        // neither a new grant nor an older caller can replace or erase it.
        connection
            .execute(
                "UPDATE stable_runtime_stops SET descriptor_owner = ?8
             WHERE command_id = ?1 AND session_id = ?2 AND namespace = ?3 AND root = ?4
               AND runtime_instance = ?5 AND runtime_session_id = ?6 AND generation = ?7
               AND descriptor_owner IS NULL AND ?8 IS NOT NULL",
                values,
            )
            .map_err(to_io_error)?;
        let confirmed = connection
            .query_row(
                "SELECT confirmed_at FROM stable_runtime_stops
             WHERE command_id = ?1 AND session_id = ?2 AND namespace = ?3 AND root = ?4
               AND runtime_instance = ?5 AND runtime_session_id = ?6 AND generation = ?7
               AND descriptor_owner IS ?8",
                values,
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(to_io_error)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stable-runtime stop intent owner changed",
                )
            })?;
        confirmed_timestamp(confirmed)
    }

    /// Record only a positive host acknowledgement. Keep the original intent
    /// available even after link replacement or task removal, so an uncertain
    /// or interrupted local retirement never manufactures a new stop command.
    pub fn stable_stop_confirm(&self, intent: &StableStopIntent) -> io::Result<()> {
        let root = intent.root.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "stable-runtime root is not UTF-8",
            )
        })?;
        let generation = i64::try_from(intent.generation).map_err(to_io_error)?;
        let descriptor_owner = descriptor_evidence(intent)?;
        let changed = self
            .open()?
            .execute(
                "UPDATE stable_runtime_stops SET confirmed_at = COALESCE(confirmed_at, unixepoch())
             WHERE command_id = ?1 AND session_id = ?2 AND namespace = ?3 AND root = ?4
               AND runtime_instance = ?5 AND runtime_session_id = ?6 AND generation = ?7
               AND descriptor_owner IS ?8",
                params![
                    intent.command_id.to_string(),
                    intent.session_id.to_string(),
                    intent.namespace,
                    root,
                    intent.runtime_instance.to_string(),
                    intent.runtime_session_id.to_string(),
                    generation,
                    descriptor_owner,
                ],
            )
            .map_err(to_io_error)?;
        if changed != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stable-runtime stop confirmation owner changed",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_authority_survives_reopen_and_refuses_identity_or_owner_changes() {
        let root = std::env::temp_dir().join(format!("stable-stop-{}", Uuid::new_v4()));
        let path = root.join("app.db");
        let store = StateStore::daemon(path.clone());
        let mut state = PersistedState::fresh(root.join("repo"));
        store.save(&mut state).unwrap();
        let intent = StableStopIntent {
            command_id: Uuid::new_v4(),
            session_id: state.sessions[0].id,
            namespace: "fixture".into(),
            root: root.join("host"),
            runtime_instance: Uuid::new_v4(),
            runtime_session_id: Uuid::new_v4(),
            generation: 1,
            descriptor_owner: Some(StableIngressOwner {
                path: root.join("ingress.json"),
                grant_id: Uuid::new_v4(),
            }),
            descriptor_qualified: true,
        };
        assert!(!store.stable_stop_begin(&intent).unwrap());
        drop(store);
        let store = StateStore::daemon(path);
        assert!(!store.stable_stop_begin(&intent).unwrap());
        let mut foreign = intent.clone();
        foreign.root = root.join("other-host");
        assert!(store.stable_stop_begin(&foreign).is_err());
        assert!(store.stable_stop_confirm(&foreign).is_err());
        foreign = intent.clone();
        foreign.descriptor_owner.as_mut().unwrap().grant_id = Uuid::new_v4();
        assert!(store.stable_stop_begin(&foreign).is_err());
        assert!(store.stable_stop_confirm(&foreign).is_err());
        foreign = intent.clone();
        foreign.descriptor_owner.as_mut().unwrap().path = PathBuf::from("relative-ingress.json");
        assert!(store.stable_stop_begin(&foreign).is_err());
        foreign = intent.clone();
        foreign.command_id = Uuid::new_v4();
        assert!(
            store.stable_stop_begin(&foreign).is_err(),
            "stop identity was replaced"
        );
        assert!(!store.stable_stop_begin(&intent).unwrap());
        store.stable_stop_confirm(&intent).unwrap();
        assert!(store.stable_stop_begin(&intent).unwrap());
        // Task/catalog deletion cannot erase stop evidence for a surviving host.
        store
            .open()
            .unwrap()
            .execute("DELETE FROM sessions", [])
            .unwrap();
        assert!(store.stable_stop_begin(&intent).unwrap());
        let mut replacement = intent.clone();
        replacement.command_id = Uuid::new_v4();
        replacement.generation += 1;
        assert!(!store.stable_stop_begin(&replacement).unwrap());
        assert!(store.stable_stop_begin(&intent).unwrap());
        let mut legacy = replacement.clone();
        legacy.command_id = Uuid::new_v4();
        legacy.generation += 1;
        legacy.descriptor_owner = None;
        legacy.descriptor_qualified = false;
        assert!(!store.stable_stop_begin(&legacy).unwrap());
        store.stable_stop_confirm(&legacy).unwrap();
        assert!(
            store.stable_stop_retired(&legacy).is_err(),
            "unknown credential scope was retired"
        );
        let mut qualified = legacy.clone();
        qualified.descriptor_owner = intent.descriptor_owner.clone();
        qualified.descriptor_qualified = true;
        assert!(store.stable_stop_begin(&qualified).unwrap());
        assert!(
            store.stable_stop_begin(&legacy).is_err(),
            "qualified ownership was erased"
        );
        store.stable_stop_confirm(&qualified).unwrap();
        assert!(store.stable_stop_begin(&qualified).unwrap());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// A bounded control-work descriptor. Recovery never loads a transcript.
#[derive(Clone, Debug)]
pub struct StableStopRecord {
    pub intent: StableStopIntent,
    pub confirmed: bool,
}

impl StateStore {
    pub fn stable_stops_pending(&self) -> io::Result<bool> {
        self.open()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM stable_runtime_stops WHERE retired_at IS NULL)",
                [],
                |row| row.get(0),
            )
            .map_err(to_io_error)
    }

    pub fn stable_stop_blocks_attachment(
        &self,
        task: Uuid,
        instance: Uuid,
        session: Uuid,
        generation: u64,
    ) -> io::Result<bool> {
        self.open()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM stable_runtime_stops WHERE session_id = ?1
             AND runtime_instance = ?2 AND runtime_session_id = ?3 AND generation = ?4 AND retired_at IS NULL)",
            params![task.to_string(), instance.to_string(), session.to_string(), i64::try_from(generation).map_err(to_io_error)?],
            |row| row.get(0),
        ).map_err(to_io_error)
    }

    /// Read at most eight due descriptors. Malformed rows are retained and
    /// backed off independently so one corrupt intent cannot block its peers.
    pub fn stable_stops_due(&self, now: u64) -> io::Result<Vec<StableStopRecord>> {
        let connection = self.open()?;
        let now = i64::try_from(now).map_err(to_io_error)?;
        let mut statement = connection.prepare(
            "SELECT rowid, substr(command_id,1,37), substr(session_id,1,37), substr(namespace,1,128),
                    substr(root,1,8193), substr(runtime_instance,1,37), substr(runtime_session_id,1,37),
                    generation, CASE WHEN length(descriptor_owner) <= 65536 THEN descriptor_owner
                       WHEN descriptor_owner IS NULL THEN NULL ELSE 'oversized owner' END,
                    confirmed_at
             FROM stable_runtime_stops WHERE retired_at IS NULL AND retry_at <= ?1
             ORDER BY retry_at, command_id LIMIT 8"
        ).map_err(to_io_error)?;
        let rows = statement
            .query_map([now], |row| {
                let id: i64 = row.get(0)?;
                let parsed = (|| -> io::Result<StableStopRecord> {
                    let uuid = |index| -> io::Result<Uuid> {
                        Uuid::parse_str(&row.get::<_, String>(index).map_err(to_io_error)?)
                            .map_err(to_io_error)
                    };
                    let root = PathBuf::from(row.get::<_, String>(4).map_err(to_io_error)?);
                    if !root.is_absolute() || root.as_os_str().len() > 8192 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid stable host root",
                        ));
                    }
                    let descriptor: Option<String> = row.get(8).map_err(to_io_error)?;
                    Ok(StableStopRecord {
                        intent: StableStopIntent {
                            command_id: uuid(1)?,
                            session_id: uuid(2)?,
                            namespace: row.get(3).map_err(to_io_error)?,
                            root,
                            runtime_instance: uuid(5)?,
                            runtime_session_id: uuid(6)?,
                            generation: u64::try_from(row.get::<_, i64>(7).map_err(to_io_error)?)
                                .map_err(to_io_error)?,
                            descriptor_qualified: descriptor.is_some(),
                            descriptor_owner: descriptor
                                .map(|value| {
                                    serde_json::from_str::<Option<StableIngressOwner>>(&value)
                                        .map_err(to_io_error)
                                })
                                .transpose()?
                                .flatten(),
                        },
                        confirmed: confirmed_timestamp(row.get(9).map_err(to_io_error)?)?,
                    })
                })();
                Ok((id, parsed))
            })
            .map_err(to_io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_io_error)?;
        drop(statement);
        let mut ready = Vec::new();
        for (rowid, parsed) in rows {
            match parsed {
                Ok(record) => ready.push(record),
                Err(_) => {
                    connection.execute("UPDATE stable_runtime_stops SET retry_at = ?1 + 900,
                        failures = MIN(failures + 1, 30), last_error = 'invalid stop evidence; reconciliation required'
                        WHERE rowid = ?2 AND retired_at IS NULL", params![now, rowid]).map_err(to_io_error)?;
                }
            }
        }
        Ok(ready)
    }

    pub fn stable_stop_retry(
        &self,
        intent: &StableStopIntent,
        now: u64,
        error: &str,
    ) -> io::Result<()> {
        let error: String = error.chars().take(512).collect();
        self.open()?.execute(
            "UPDATE stable_runtime_stops SET retry_at = ?1 + MIN(900, 30 * (1 << MIN(failures, 5))),
             failures = MIN(failures + 1, 30), last_error = ?2
             WHERE command_id = ?3 AND retired_at IS NULL",
            params![i64::try_from(now).map_err(to_io_error)?, error, intent.command_id.to_string()],
        ).map_err(to_io_error)?;
        Ok(())
    }

    pub fn stable_stop_retired(&self, intent: &StableStopIntent) -> io::Result<()> {
        if !intent.descriptor_qualified {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ingress retirement scope is unqualified",
            ));
        }
        // Revalidate the immutable owner before acknowledging local retirement.
        if !self.stable_stop_begin(intent)? {
            return Err(io::Error::other("stable provider stop is not confirmed"));
        }
        self.open()?
            .execute(
                "UPDATE stable_runtime_stops SET retired_at = COALESCE(retired_at, unixepoch()),
            last_error = NULL WHERE command_id = ?1 AND confirmed_at IS NOT NULL",
                [intent.command_id.to_string()],
            )
            .map_err(to_io_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn due_stop_pages_isolate_corruption_back_off_and_retire_without_histories() {
        let root = std::env::temp_dir().join(format!("stop-pages-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        store.save(&mut state).unwrap();
        let mut intents = Vec::new();
        for _ in 0..11 {
            let intent = StableStopIntent {
                command_id: Uuid::new_v4(),
                session_id: state.sessions[0].id,
                namespace: "goddard".into(),
                root: root.join("host"),
                runtime_instance: Uuid::new_v4(),
                runtime_session_id: Uuid::new_v4(),
                generation: 1,
                descriptor_owner: None,
                descriptor_qualified: true,
            };
            assert!(!store.stable_stop_begin(&intent).unwrap());
            intents.push(intent);
        }
        store.open().unwrap().execute("UPDATE stable_runtime_stops SET descriptor_owner = 'invalid JSON' WHERE command_id = ?1",
            [intents[0].command_id.to_string()]).unwrap();
        store.open().unwrap().execute("UPDATE stable_runtime_stops SET confirmed_at = 'invalid timestamp' WHERE command_id = ?1",
            [intents[10].command_id.to_string()]).unwrap();
        assert!(store.stable_stop_begin(&intents[10]).is_err());
        let now = 1000;
        let mut processed = HashSet::new();
        for _ in 0..4 {
            let page = store.stable_stops_due(now).unwrap();
            assert!(page.len() <= 8);
            for record in page {
                assert_ne!(record.intent.command_id, intents[0].command_id);
                assert!(processed.insert(record.intent.command_id));
                assert!(
                    store
                        .stable_stop_blocks_attachment(
                            record.intent.session_id,
                            record.intent.runtime_instance,
                            record.intent.runtime_session_id,
                            record.intent.generation
                        )
                        .unwrap()
                );
                assert!(store.stable_stop_retired(&record.intent).is_err());
                store.stable_stop_confirm(&record.intent).unwrap();
                store.stable_stop_retired(&record.intent).unwrap();
                // A late failure cannot revive completed recovery work.
                store
                    .stable_stop_retry(&record.intent, now, "late failure")
                    .unwrap();
            }
        }
        assert_eq!(processed.len(), 9);
        assert!(store.stable_stops_due(now).unwrap().is_empty());
        assert!(
            store.stable_stops_pending().unwrap(),
            "corrupt evidence was discarded"
        );
        let mut retry = intents[1].clone();
        retry.command_id = Uuid::new_v4();
        retry.generation += 1;
        store.stable_stop_begin(&retry).unwrap();
        store
            .stable_stop_retry(&retry, now, "temporarily unavailable")
            .unwrap();
        assert!(store.stable_stops_due(now + 29).unwrap().is_empty());
        assert_eq!(store.stable_stops_due(now + 30).unwrap().len(), 1);
        for _ in 0..20 {
            store
                .stable_stop_retry(&retry, now, &"x".repeat(1024))
                .unwrap();
        }
        assert!(store.stable_stops_due(now + 899).unwrap().is_empty());
        assert!(
            store
                .stable_stops_due(now + 900)
                .unwrap()
                .iter()
                .any(|record| record.intent.command_id == retry.command_id)
        );
        let length: i64 = store
            .open()
            .unwrap()
            .query_row(
                "SELECT length(last_error) FROM stable_runtime_stops WHERE command_id = ?1",
                [retry.command_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(length, 512);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
