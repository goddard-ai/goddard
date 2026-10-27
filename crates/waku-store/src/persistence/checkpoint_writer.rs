//! Physical checkpoint attempts survive backend replacement independently of
//! checkpoint persistence debt. A later capture cannot confirm an older one.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointWriterOrigin {
    pub origin_id: Uuid,
    pub session_id: Uuid,
    pub turn_count: usize,
    pub workspace: PathBuf,
    pub identity: Option<String>,
}

impl CheckpointWriterOrigin {
    fn validate(&self) -> io::Result<()> {
        if self.origin_id.is_nil()
            || i64::try_from(self.turn_count).is_err()
            || !self.workspace.is_absolute()
            || self.workspace.to_str().is_none_or(|path| path.len() > 8192)
            || self
                .identity
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 128)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid checkpoint writer origin",
            ));
        }
        Ok(())
    }
}

impl StateStore {
    /// Commit before the first Git capture operation. Deduped checkpoint reads
    /// create no attempt, and neither catalog removal nor turn reuse clears it.
    pub fn record_checkpoint_writer(&self, origin: &CheckpointWriterOrigin) -> io::Result<()> {
        origin.validate()?;
        self.open()?
            .execute(
                "INSERT INTO checkpoint_workspace_writers
             (origin_id, session_id, turn_count, workspace, identity, confirmed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, unixepoch())",
                params![
                    origin.origin_id.to_string(),
                    origin.session_id.to_string(),
                    i64::try_from(origin.turn_count).map_err(to_io_error)?,
                    origin.workspace.to_str(),
                    origin.identity
                ],
            )
            .map_err(to_io_error)?;
        Ok(())
    }

    /// Exact-attempt physical completion only; checkpoint persistence is a
    /// separate obligation and must never erase another attempt's uncertainty.
    pub fn confirm_checkpoint_writer(&self, origin_id: Uuid) -> io::Result<()> {
        let changed = self
            .open()?
            .execute(
                "UPDATE checkpoint_workspace_writers SET confirmed = 1
             WHERE origin_id = ?1 AND confirmed IN (0, 1)",
                params![origin_id.to_string()],
            )
            .map_err(to_io_error)?;
        if changed != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "checkpoint writer confirmation has no matching origin",
            ));
        }
        Ok(())
    }

    /// Bounded metadata-only inventory. 65 rows are an incomplete safety scan;
    /// malformed confirmation values must be returned and rejected, not hidden.
    pub fn checkpoint_writer_origins(&self) -> io::Result<Vec<CheckpointWriterOrigin>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT substr(origin_id,1,37), substr(session_id,1,37), turn_count,
             substr(workspace,1,8193), substr(identity,1,129), confirmed
             FROM checkpoint_workspace_writers WHERE confirmed IS NOT 1 ORDER BY origin_id LIMIT 65",
        ).map_err(to_io_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(to_io_error)?;
        rows.map(|row| {
            let (id, session, turn_count, workspace, identity, confirmed) =
                row.map_err(to_io_error)?;
            if confirmed != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid checkpoint writer confirmation",
                ));
            }
            let origin = CheckpointWriterOrigin {
                origin_id: Uuid::parse_str(&id).map_err(to_io_error)?,
                session_id: Uuid::parse_str(&session).map_err(to_io_error)?,
                turn_count: usize::try_from(turn_count).map_err(to_io_error)?,
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
    fn checkpoint_attempts_survive_removal_and_reopen_without_cross_confirming() {
        let root = std::env::temp_dir().join(format!("checkpoint-origins-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        store.save(&mut state).unwrap();
        let first = CheckpointWriterOrigin {
            origin_id: Uuid::new_v4(),
            session_id: state.sessions[0].id,
            turn_count: 1,
            workspace: root.join("repo"),
            identity: Some("unix:1:2".into()),
        };
        let next = CheckpointWriterOrigin {
            origin_id: Uuid::new_v4(),
            ..first.clone()
        };
        store.record_checkpoint_writer(&first).unwrap();
        store.record_checkpoint_writer(&next).unwrap();
        assert!(store.record_checkpoint_writer(&first).is_err());
        state.sessions.clear();
        state.projects.clear();
        store.save(&mut state).unwrap();
        drop(store);
        let reopened = StateStore::daemon(root.join("app.db"));
        assert_eq!(reopened.checkpoint_writer_origins().unwrap().len(), 2);
        reopened.confirm_checkpoint_writer(first.origin_id).unwrap();
        reopened.confirm_checkpoint_writer(first.origin_id).unwrap();
        assert_eq!(
            reopened.checkpoint_writer_origins().unwrap(),
            vec![next.clone()]
        );
        assert!(reopened.record_checkpoint_writer(&first).is_err());
        assert!(reopened.confirm_checkpoint_writer(Uuid::new_v4()).is_err());
        reopened.confirm_checkpoint_writer(next.origin_id).unwrap();
        assert!(reopened.checkpoint_writer_origins().unwrap().is_empty());
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checkpoint_inventory_is_bounded_indexed_and_rejects_corruption() {
        let root = std::env::temp_dir().join(format!("checkpoint-origin-page-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        for _ in 0..70 {
            store
                .record_checkpoint_writer(&CheckpointWriterOrigin {
                    origin_id: Uuid::new_v4(),
                    session_id: Uuid::new_v4(),
                    turn_count: 1,
                    workspace: root.join("repo"),
                    identity: None,
                })
                .unwrap();
        }
        let page = store.checkpoint_writer_origins().unwrap();
        assert_eq!(page.len(), 65);
        let connection = store.open().unwrap();
        let query_plan: String = connection
            .query_row(
                "EXPLAIN QUERY PLAN SELECT origin_id FROM checkpoint_workspace_writers
             WHERE confirmed IS NOT 1 ORDER BY origin_id LIMIT 65",
                [],
                |row| row.get(3),
            )
            .unwrap();
        assert!(
            query_plan.contains("checkpoint_unconfirmed_writers"),
            "{query_plan}"
        );
        for (column, value) in [("confirmed", 7), ("turn_count", -1)] {
            connection
                .execute(
                    &format!(
                        "UPDATE checkpoint_workspace_writers SET {column} = ?1 WHERE origin_id = ?2"
                    ),
                    params![value, page[0].origin_id.to_string()],
                )
                .unwrap();
            assert!(store.checkpoint_writer_origins().is_err());
            connection
                .execute(
                    &format!(
                        "UPDATE checkpoint_workspace_writers SET {column} = ?1 WHERE origin_id = ?2"
                    ),
                    params![
                        if column == "confirmed" { 0 } else { 1 },
                        page[0].origin_id.to_string()
                    ],
                )
                .unwrap();
        }
        drop(connection);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
