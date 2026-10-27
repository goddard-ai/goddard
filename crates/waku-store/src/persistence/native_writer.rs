//! Native attempts have independent identities; catalog removal and runtime-id
//! reuse cannot discard an older attempt's unconfirmed physical writer.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeWriterOrigin {
    pub origin_id: Uuid,
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    pub workspace: PathBuf,
    pub identity: Option<String>,
}

impl NativeWriterOrigin {
    fn validate(&self) -> io::Result<()> {
        if self.origin_id.is_nil()
            || !self.workspace.is_absolute()
            || self.workspace.to_str().is_none_or(|path| path.len() > 8192)
            || self
                .identity
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 128)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid native writer origin",
            ));
        }
        Ok(())
    }
}

impl StateStore {
    /// Commit before entering a driver factory, including factories which can
    /// fail after contacting a provider. An attempt id cannot be reused.
    pub fn record_native_writer(&self, origin: &NativeWriterOrigin) -> io::Result<()> {
        origin.validate()?;
        self.open()?
            .execute(
                "INSERT INTO native_workspace_writers
             (origin_id, session_id, runtime_id, workspace, identity, confirmed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, unixepoch())",
                params![
                    origin.origin_id.to_string(),
                    origin.session_id.to_string(),
                    origin.runtime_id.to_string(),
                    origin.workspace.to_str(),
                    origin.identity
                ],
            )
            .map_err(to_io_error)?;
        Ok(())
    }

    /// Only the exact attempt's qualified physical witness may call this.
    /// Keep the immutable origin so its id cannot later admit another writer.
    pub fn confirm_native_writer(&self, origin_id: Uuid) -> io::Result<()> {
        let changed = self
            .open()?
            .execute(
                "UPDATE native_workspace_writers SET confirmed = 1
             WHERE origin_id = ?1 AND confirmed IN (0, 1)",
                params![origin_id.to_string()],
            )
            .map_err(to_io_error)?;
        if changed != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native writer confirmation has no matching origin",
            ));
        }
        Ok(())
    }

    /// Bounded metadata-only inventory. A caller receiving 65 rows must defer;
    /// invalid confirmation values are included and rejected, never skipped.
    pub fn native_writer_origins(&self) -> io::Result<Vec<NativeWriterOrigin>> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare(
                "SELECT substr(origin_id,1,37), substr(session_id,1,37), substr(runtime_id,1,37),
             substr(workspace,1,8193), substr(identity,1,129), confirmed
             FROM native_workspace_writers WHERE confirmed IS NOT 1 ORDER BY origin_id LIMIT 65",
            )
            .map_err(to_io_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(to_io_error)?;
        rows.map(|row| {
            let (id, session, runtime, workspace, identity, confirmed) =
                row.map_err(to_io_error)?;
            if confirmed != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid native writer confirmation",
                ));
            }
            let origin = NativeWriterOrigin {
                origin_id: Uuid::parse_str(&id).map_err(to_io_error)?,
                session_id: Uuid::parse_str(&session).map_err(to_io_error)?,
                runtime_id: Uuid::parse_str(&runtime).map_err(to_io_error)?,
                workspace: workspace.into(),
                identity,
            };
            origin.validate()?;
            Ok(origin)
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_attempts_survive_reopen_and_old_completion_cannot_release_replacement() {
        let root = std::env::temp_dir().join(format!("native-origins-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        store.save(&mut state).unwrap();
        let first = NativeWriterOrigin {
            origin_id: Uuid::new_v4(),
            session_id: state.sessions[0].id,
            runtime_id: Uuid::new_v4(),
            workspace: root.join("repo"),
            identity: Some("unix:1:2".into()),
        };
        store.record_native_writer(&first).unwrap();
        let next = NativeWriterOrigin {
            origin_id: Uuid::new_v4(),
            ..first.clone()
        };
        store.record_native_writer(&next).unwrap();
        assert!(
            store.record_native_writer(&first).is_err(),
            "an attempt id was reused"
        );
        state.sessions.clear();
        state.projects.clear();
        store.save(&mut state).unwrap();
        drop(store);
        let store = StateStore::daemon(root.join("app.db"));
        assert_eq!(store.native_writer_origins().unwrap().len(), 2);
        store.confirm_native_writer(first.origin_id).unwrap();
        store.confirm_native_writer(first.origin_id).unwrap();
        assert_eq!(store.native_writer_origins().unwrap(), vec![next.clone()]);
        assert!(
            store.record_native_writer(&first).is_err(),
            "a confirmed identity was readmitted"
        );
        store.confirm_native_writer(next.origin_id).unwrap();
        assert!(store.native_writer_origins().unwrap().is_empty());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_inventory_is_bounded_and_rejects_invalid_confirmation() {
        let root = std::env::temp_dir().join(format!("native-origin-page-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        for _ in 0..70 {
            store
                .record_native_writer(&NativeWriterOrigin {
                    origin_id: Uuid::new_v4(),
                    session_id: Uuid::new_v4(),
                    runtime_id: Uuid::new_v4(),
                    workspace: root.join("repo"),
                    identity: None,
                })
                .unwrap();
        }
        let page = store.native_writer_origins().unwrap();
        assert_eq!(page.len(), 65);
        store
            .open()
            .unwrap()
            .execute(
                "UPDATE native_workspace_writers SET confirmed = 7
            WHERE origin_id = ?1",
                params![page[0].origin_id.to_string()],
            )
            .unwrap();
        assert!(store.native_writer_origins().is_err());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
