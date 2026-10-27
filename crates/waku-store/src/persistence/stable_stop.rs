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
        let descriptor_owner = intent
            .descriptor_owner
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(to_io_error)?;
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
        connection
            .query_row(
                "SELECT confirmed_at IS NOT NULL FROM stable_runtime_stops
             WHERE command_id = ?1 AND session_id = ?2 AND namespace = ?3 AND root = ?4
               AND runtime_instance = ?5 AND runtime_session_id = ?6 AND generation = ?7
               AND descriptor_owner IS ?8",
                values,
                |row| row.get(0),
            )
            .optional()
            .map_err(to_io_error)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stable-runtime stop intent owner changed",
                )
            })
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
        let descriptor_owner = intent
            .descriptor_owner
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(to_io_error)?;
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
        assert!(!store.stable_stop_begin(&legacy).unwrap());
        let mut qualified = legacy.clone();
        qualified.descriptor_owner = intent.descriptor_owner.clone();
        assert!(!store.stable_stop_begin(&qualified).unwrap());
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
