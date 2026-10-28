//! Narrow worker metadata. Tokens and recovery records never authorize a
//! filesystem effect without workspace admission and physical writer evidence.
use super::*;

const MAX_DESCRIPTOR_BYTES: i64 = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanupToken {
    pub session_id: Uuid,
    pub generation: Uuid,
}

pub struct CleanupDescriptor {
    pub project_path: PathBuf,
    pub workspace: SessionWorkspace,
}

pub struct CleanupCatalogRow {
    pub session_id: Uuid,
    pub project_path: PathBuf,
    pub workspace: SessionWorkspace,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupRun {
    pub phase: String,
    pub payload: String,
}

fn workspace(value: Option<String>) -> io::Result<SessionWorkspace> {
    value
        .map(|value| serde_json::from_str(&value).map_err(to_io_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

impl StateStore {
    fn cleanup_connection(&self) -> io::Result<Connection> {
        let connection =
            Connection::open_with_flags(self.path(), OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(to_io_error)?;
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(to_io_error)?;
        // Recovery records precede filesystem publication/retirement. Unlike
        // retry timestamps, losing their latest commit is not acceptable.
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(to_io_error)?;
        Ok(connection)
    }

    /// Load only the exact current promoted descriptor after heavy admission.
    /// A stale or completed token returns None, without reading any history.
    pub fn workspace_cleanup_descriptor(
        &self,
        token: CleanupToken,
    ) -> io::Result<Option<CleanupDescriptor>> {
        let sql = format!(
            "SELECT intent.project_path, intent.workspace
            FROM workspace_cleanup_intents AS intent
            WHERE intent.session_id=?1 AND lower(replace(intent.generation,'-',''))=?2
              AND intent.phase NOT IN ('complete', 'ignored')
              AND length(CAST(intent.project_path AS BLOB)) <= ?3
              AND coalesce(length(CAST(intent.workspace AS BLOB)), 0) <= ?3
              AND EXISTS ({})",
            queue::CURRENT_CATALOG
        );
        let row: Option<(String, Option<String>)> = self
            .cleanup_connection()?
            .query_row(
                &sql,
                params![
                    token.session_id.to_string(),
                    token.generation.simple().to_string(),
                    MAX_DESCRIPTOR_BYTES
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(to_io_error)?;
        row.map(|(path, value)| {
            Ok(CleanupDescriptor {
                project_path: PathBuf::from(path),
                workspace: workspace(value)?,
            })
        })
        .transpose()
    }

    /// Keyset pages keep ownership checks cancellable without hydrating tasks
    /// or holding the store's global connection mutex across filesystem work.
    pub fn workspace_cleanup_catalog_page(
        &self,
        after: Option<Uuid>,
    ) -> io::Result<Vec<CleanupCatalogRow>> {
        let connection = self.cleanup_connection()?;
        let mut statement = connection.prepare("SELECT session.id,
            CASE WHEN length(CAST(project.path AS BLOB)) <= ?2 THEN project.path END,
            CASE WHEN coalesce(length(CAST(session.workspace AS BLOB)), 0) <= ?2 THEN session.workspace END,
            coalesce(length(CAST(session.workspace AS BLOB)), 0) > ?2,
            session.archived_at IS NOT NULL
            FROM sessions AS session LEFT JOIN projects AS project ON project.id=session.project_id
            WHERE session.id > ?1 ORDER BY session.id LIMIT 128").map_err(to_io_error)?;
        let rows = statement
            .query_map(
                params![
                    after.map(|id| id.to_string()).unwrap_or_default(),
                    MAX_DESCRIPTOR_BYTES
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, bool>(4)?,
                    ))
                },
            )
            .map_err(to_io_error)?;
        rows.map(|row| {
            let (id, path, value, oversized, archived) = row.map_err(to_io_error)?;
            if oversized {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized cleanup owner descriptor",
                ));
            }
            Ok(CleanupCatalogRow {
                session_id: Uuid::parse_str(&id).map_err(to_io_error)?,
                project_path: PathBuf::from(path.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "cleanup owner project is unavailable",
                    )
                })?),
                workspace: workspace(value)?,
                archived,
            })
        })
        .collect()
    }

    /// Records deliberately survive unarchive/rearchive and session deletion.
    /// An obsolete run may still name a partial file or published archive.
    pub fn workspace_cleanup_run(&self, token: CleanupToken) -> io::Result<Option<CleanupRun>> {
        let row: Option<(Option<String>, Option<String>)> = self
            .cleanup_connection()?
            .query_row(
                "SELECT CASE WHEN length(CAST(phase AS BLOB)) <= 32 THEN phase END,
             CASE WHEN length(CAST(payload AS BLOB)) <= ?3 THEN payload END
             FROM workspace_cleanup_runs WHERE session_id=?1 AND generation=?2",
                params![
                    token.session_id.to_string(),
                    token.generation.simple().to_string(),
                    MAX_DESCRIPTOR_BYTES
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(to_io_error)?;
        row.map(|(phase, payload)| match (phase, payload) {
            (Some(phase), Some(payload)) => Ok(CleanupRun { phase, payload }),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized cleanup recovery record",
            )),
        })
        .transpose()
    }

    /// Compare phase and actual promoted authority at the commit boundary.
    /// The caller must separately retain workspace admission through its effect.
    pub fn save_workspace_cleanup_run(
        &self,
        token: CleanupToken,
        expected: Option<&str>,
        run: &CleanupRun,
    ) -> io::Result<bool> {
        if run.payload.len() > MAX_DESCRIPTOR_BYTES as usize || run.phase.len() > 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "oversized cleanup recovery record",
            ));
        }
        let connection = self.cleanup_connection()?;
        let sql = format!("INSERT INTO workspace_cleanup_runs (session_id, generation, phase, payload)
            SELECT intent.session_id, ?2, ?3, ?4 FROM workspace_cleanup_intents AS intent
            WHERE intent.session_id=?1 AND lower(replace(intent.generation,'-',''))=?2 AND intent.phase NOT IN ('complete', 'ignored')
              AND EXISTS ({})
              AND (?5 IS NULL OR EXISTS (SELECT 1 FROM workspace_cleanup_runs WHERE session_id=?1 AND generation=?2 AND phase=?5))
            ON CONFLICT(session_id, generation) DO UPDATE SET phase=excluded.phase, payload=excluded.payload
            WHERE workspace_cleanup_runs.phase=?5", queue::CURRENT_CATALOG);
        connection
            .execute(
                &sql,
                params![
                    token.session_id.to_string(),
                    token.generation.simple().to_string(),
                    run.phase,
                    run.payload,
                    expected
                ],
            )
            .map(|count| count == 1)
            .map_err(to_io_error)
    }

    /// Preservation is not completed cleanup. Park it in metadata until the
    /// qualified retirement stage is available, without resubmitting heavy ZIP
    /// work on every dispatch. A new archive/workspace generation resets it.
    pub fn park_workspace_cleanup(&self, token: CleanupToken, now: u64) -> io::Result<bool> {
        let sql = format!("UPDATE workspace_cleanup_intents AS intent
            SET phase='preserved', retry_at=?3, failures=0, last_error='awaiting-qualified-retirement'
            WHERE session_id=?1 AND lower(replace(generation,'-',''))=?2
              AND phase NOT IN ('complete', 'ignored') AND EXISTS ({})
              AND EXISTS (SELECT 1 FROM workspace_cleanup_runs WHERE session_id=?1 AND generation=?2 AND phase='preserved')", queue::CURRENT_CATALOG);
        self.cleanup_connection()?
            .execute(
                &sql,
                params![
                    token.session_id.to_string(),
                    token.generation.simple().to_string(),
                    i64::try_from(now).unwrap_or(i64::MAX).saturating_add(300)
                ],
            )
            .map(|count| count == 1)
            .map_err(to_io_error)
    }

    /// Return an unsuccessful attempt to durable retry, freeing the scarce
    /// scheduler slot. Storage failures may retain that slot until writable.
    pub fn retry_workspace_cleanup(
        &self,
        token: CleanupToken,
        now: u64,
        failed: bool,
    ) -> io::Result<()> {
        let failures = if failed {
            "min(max(failures,0)+1, 8)"
        } else {
            "failures"
        };
        let delay = if failed {
            "min(1 << min(max(failures,0)+1, 8), 300)"
        } else {
            "30"
        };
        let sql = format!(
            "UPDATE workspace_cleanup_intents
            SET failures={failures}, retry_at=min(?3 + {delay}, 9223372036854775807),
                last_error=?4
            WHERE session_id=?1 AND lower(replace(generation,'-',''))=?2
              AND phase NOT IN ('complete', 'ignored', 'preserved')"
        );
        self.cleanup_connection()?
            .execute(
                &sql,
                params![
                    token.session_id.to_string(),
                    token.generation.simple().to_string(),
                    i64::try_from(now).unwrap_or(i64::MAX).min(i64::MAX - 300),
                    if failed {
                        "projectless-preservation-deferred"
                    } else {
                        "projectless-preservation-paused"
                    }
                ],
            )
            .map(|_| ())
            .map_err(to_io_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_commits_fence_phase_and_generation_and_bound_payload_reads() {
        let root = std::env::temp_dir().join(format!("cleanup-record-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("workspace")).unwrap();
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("workspace"));
        state.sessions[0].begin_turn("fixture");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].archived_at = Some(100);
        let id = state.sessions[0].id;
        store.save(&mut state).unwrap();
        let token = CleanupToken {
            session_id: id,
            generation: store.workspace_cleanup_generation(id).unwrap().unwrap(),
        };
        assert!(store.workspace_cleanup_descriptor(token).unwrap().is_some());
        let record = CleanupRun {
            phase: "capture".into(),
            payload: "{}".into(),
        };
        assert!(
            store
                .save_workspace_cleanup_run(token, None, &record)
                .unwrap()
        );
        assert!(
            !store
                .save_workspace_cleanup_run(token, None, &record)
                .unwrap()
        );
        assert!(
            !store
                .save_workspace_cleanup_run(token, Some("verify"), &record)
                .unwrap()
        );
        let preserved = CleanupRun {
            phase: "preserved".into(),
            payload: "{}".into(),
        };
        assert!(
            store
                .save_workspace_cleanup_run(token, Some("capture"), &preserved)
                .unwrap()
        );
        assert!(store.park_workspace_cleanup(token, 1000).unwrap());
        assert!(
            store
                .advance_workspace_cleanup_queue(1300, || true)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.workspace_cleanup_run(token).unwrap(), Some(preserved));
        // A malformed durable descriptor must fail before allocating its body.
        store
            .open()
            .unwrap()
            .execute(
                "UPDATE workspace_cleanup_runs SET payload=zeroblob(70000)",
                [],
            )
            .unwrap();
        assert_eq!(
            store.workspace_cleanup_run(token).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        state.sessions[0].archived_at = None;
        state.mark_session_dirty(id);
        store.save(&mut state).unwrap();
        assert!(store.workspace_cleanup_descriptor(token).unwrap().is_none());
        assert!(
            !store
                .save_workspace_cleanup_run(token, Some("preserved"), &record)
                .unwrap()
        );
        let retained: i64 = store
            .open()
            .unwrap()
            .query_row("SELECT count(*) FROM workspace_cleanup_runs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            retained, 1,
            "revoking intent must retain its recovery record"
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
