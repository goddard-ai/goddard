//! Keep cleanup authority tied to committed archive/workspace state. These
//! records do not authorize filesystem deletion: execution must separately
//! prove current ownership, preservation, and writer quiescence.

use super::*;

/// Read the committed row, never the incoming client projection: metadata
/// fencing may have preserved a newer archive flag or workspace descriptor.
/// Backfill runs separately in bounded, idle-admitted pages.
pub(super) fn sync(connection: &Connection, session_id: Uuid) -> io::Result<()> {
    let id = session_id.to_string();
    connection
        .execute(
            "DELETE FROM workspace_cleanup_intents
         WHERE session_id = ?1
           AND NOT EXISTS (SELECT 1 FROM sessions
                           WHERE sessions.id = workspace_cleanup_intents.session_id
                             AND sessions.archived_at IS NOT NULL)",
            [&id],
        )
        .map_err(to_io_error)?;
    connection
        .execute(
            "INSERT INTO workspace_cleanup_intents
            (session_id, generation, project_id, project_path, workspace, archived_at)
         SELECT sessions.id, lower(hex(randomblob(16))), sessions.project_id,
                projects.path, sessions.workspace, sessions.archived_at
         FROM sessions LEFT JOIN projects ON projects.id = sessions.project_id
         WHERE sessions.id = ?1 AND sessions.archived_at IS NOT NULL
         ON CONFLICT(session_id) DO UPDATE SET
            generation = excluded.generation,
            project_id = excluded.project_id,
            project_path = excluded.project_path,
            workspace = excluded.workspace,
            archived_at = excluded.archived_at,
            phase = 'pending', retry_at = 0, failures = 0, last_error = NULL
         WHERE workspace_cleanup_intents.project_id IS NOT excluded.project_id
            OR workspace_cleanup_intents.project_path IS NOT excluded.project_path
            OR workspace_cleanup_intents.workspace IS NOT excluded.workspace
            OR workspace_cleanup_intents.archived_at IS NOT excluded.archived_at",
            [&id],
        )
        .map_err(to_io_error)?;
    Ok(())
}

/// Project paths can change without dirtying each session. Reconcile only
/// existing intents whose captured path changed, rather than walking histories.
pub(super) fn sync_project_paths(connection: &Connection) -> io::Result<()> {
    let mut statement = connection
        .prepare(
            "SELECT intent.session_id FROM workspace_cleanup_intents AS intent
         LEFT JOIN projects ON projects.id = intent.project_id
         WHERE intent.project_path IS NOT projects.path",
        )
        .map_err(to_io_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(to_io_error)?;
    let ids = rows
        .map(|row| Uuid::parse_str(&row.map_err(to_io_error)?).map_err(to_io_error))
        .collect::<io::Result<Vec<_>>>()?;
    drop(statement);
    for id in ids {
        sync(connection, id)?;
    }
    Ok(())
}

impl StateStore {
    /// Committed archive identity for fencing delayed maintenance effects.
    /// This token alone is never permission to remove a workspace.
    pub fn workspace_cleanup_generation(&self, session_id: Uuid) -> io::Result<Option<Uuid>> {
        let guard = self.storage.lock();
        let storage = guard.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "task store is not loaded")
        })?;
        let generation: Option<String> = storage
            .connection
            .query_row(
                "SELECT generation FROM workspace_cleanup_intents WHERE session_id = ?1",
                [session_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(to_io_error)?;
        generation
            .map(|value| Uuid::parse_str(&value).map_err(to_io_error))
            .transpose()
    }

    /// Backfill at most 128 narrow catalog rows during an admitted maintenance
    /// unit. The keyset cursor advances only after commit. New archive writes
    /// already record intent atomically, including rows behind this cursor.
    pub fn recover_workspace_cleanup_intents(
        &self,
        cursor: &mut Option<Uuid>,
        should_continue: impl Fn() -> bool,
    ) -> io::Result<bool> {
        if !should_continue() {
            return Ok(true);
        }
        let mut guard = self.storage.lock();
        let storage = guard.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "task store is not loaded")
        })?;
        let transaction = storage
            .connection
            .unchecked_transaction()
            .map_err(to_io_error)?;
        let mut statement = transaction
            .prepare("SELECT id FROM sessions WHERE id > ?1 ORDER BY id LIMIT 128")
            .map_err(to_io_error)?;
        let rows = statement
            .query_map(
                [cursor.map(|id| id.to_string()).unwrap_or_default()],
                |row| row.get::<_, String>(0),
            )
            .map_err(to_io_error)?;
        let ids = rows
            .map(|row| Uuid::parse_str(&row.map_err(to_io_error)?).map_err(to_io_error))
            .collect::<io::Result<Vec<_>>>()?;
        drop(statement);
        let mut completed = 0;
        for &id in &ids {
            if !should_continue() {
                break;
            }
            sync(&transaction, id)?;
            completed += 1;
        }
        transaction.commit().map_err(to_io_error)?;
        if completed > 0 {
            *cursor = Some(ids[completed - 1]);
        }
        Ok(completed < ids.len() || ids.len() == 128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(store: &StateStore, id: Uuid) -> Option<(String, String)> {
        store
            .open()
            .unwrap()
            .query_row(
                "SELECT generation, phase FROM workspace_cleanup_intents WHERE session_id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .unwrap()
    }

    #[test]
    fn cleanup_intent_survives_reload_and_tracks_only_committed_archive_authority() {
        let root = std::env::temp_dir().join(format!("cleanup-authority-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        let id = state.sessions[0].id;
        state.sessions[0].begin_turn("preserve this workspace");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].workspace = SessionWorkspace::Worktree {
            path: root.join("worktree"),
            name: "worktree".into(),
            branch: None,
            base_branch: None,
            adopted_by: None,
            borrowed: false,
        };
        store.save(&mut state).unwrap();
        assert!(intent(&store, id).is_none());
        state.mark_session_dirty(id);
        let stale = store.save_batch(&mut state);
        let mut metadata = state.sessions[0].list_projection();
        metadata.archived_at = Some(1234);
        metadata.metadata_revision = 1;
        metadata.metadata_mutation_id = Some(Uuid::new_v4());
        store
            .write_session_metadata(&metadata, 0, &HashSet::new())
            .unwrap();
        let first = intent(&store, id).unwrap();
        assert_eq!(first.1, "pending");
        // A delayed full transcript cannot replace newer cleanup authority.
        store.write_batch(&stale).unwrap();
        assert_eq!(intent(&store, id).unwrap(), first);
        store.open().unwrap().execute(
            "UPDATE workspace_cleanup_intents SET phase = 'verified', retry_at = 42, failures = 3 WHERE session_id = ?1",
            [id.to_string()],
        ).unwrap();
        metadata.title = "accepted rename".into();
        metadata.metadata_revision = 2;
        metadata.metadata_mutation_id = Some(Uuid::new_v4());
        store
            .write_session_metadata(&metadata, 1, &HashSet::new())
            .unwrap();
        assert_eq!(
            intent(&store, id).unwrap(),
            (first.0.clone(), "verified".into())
        );
        drop(store);
        let store = StateStore::daemon(root.join("app.db"));
        let mut reloaded = store.load().unwrap();
        assert_eq!(
            intent(&store, id).unwrap(),
            (first.0.clone(), "verified".into())
        );
        // A project move without dirtying the task revokes the old descriptor.
        reloaded.projects[0].path = root.join("moved-repository");
        store.save(&mut reloaded).unwrap();
        let moved = intent(&store, id).unwrap();
        assert_ne!(moved.0, first.0);
        assert_eq!(moved.1, "pending");
        metadata.archived_at = None;
        metadata.metadata_revision = 3;
        store
            .write_session_metadata(&metadata, 2, &HashSet::new())
            .unwrap();
        assert!(intent(&store, id).is_none());
        // Re-archive within the same timestamp creates a new authority token.
        metadata.archived_at = Some(1234);
        metadata.metadata_revision = 4;
        store
            .write_session_metadata(&metadata, 3, &HashSet::new())
            .unwrap();
        assert_ne!(intent(&store, id).unwrap().0, moved.0);
        reloaded.sessions.clear();
        store.save(&mut reloaded).unwrap();
        assert!(intent(&store, id).is_none());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_cleanup_intent_write_rolls_back_archive_and_recovery_never_hydrates_history() {
        let root = std::env::temp_dir().join(format!("cleanup-atomic-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        let id = state.sessions[0].id;
        state.sessions[0].begin_turn("archive atomically");
        store.save(&mut state).unwrap();
        let connection = store.open().unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER deny_cleanup BEFORE INSERT ON workspace_cleanup_intents
             BEGIN SELECT RAISE(ABORT, 'controlled intent write failure'); END;",
            )
            .unwrap();
        let mut metadata = state.sessions[0].list_projection();
        metadata.archived_at = Some(1234);
        metadata.metadata_revision = 1;
        assert!(
            store
                .write_session_metadata(&metadata, 0, &HashSet::new())
                .is_err()
        );
        assert!(
            store
                .load_session_detail(id)
                .unwrap()
                .unwrap()
                .archived_at
                .is_none()
        );
        assert!(intent(&store, id).is_none());
        connection
            .execute_batch("DROP TRIGGER deny_cleanup")
            .unwrap();
        // Emulate an older archived row lacking an intent. A malformed large
        // detail blob must not be touched while recovering narrow list rows.
        connection
            .execute(
                "UPDATE sessions SET archived_at = 1234 WHERE id = ?1",
                [id.to_string()],
            )
            .unwrap();
        connection.execute("UPDATE session_details SET data = 'malformed detail deliberately not read' WHERE session_id = ?1", [id.to_string()]).unwrap();
        drop(connection);
        drop(store);
        let store = StateStore::daemon(root.join("app.db"));
        let loaded = store.load().unwrap();
        assert!(!loaded.sessions[0].detail_loaded);
        assert!(
            intent(&store, id).is_none(),
            "loading does not run optional backfill"
        );
        let mut cursor = None;
        assert!(
            store
                .recover_workspace_cleanup_intents(&mut cursor, || false)
                .unwrap()
        );
        assert!(intent(&store, id).is_none());
        assert!(
            !store
                .recover_workspace_cleanup_intents(&mut cursor, || true)
                .unwrap()
        );
        assert_eq!(intent(&store, id).unwrap().1, "pending");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_recovery_bounds_pages_and_never_advances_past_a_failed_commit() {
        let root = std::env::temp_dir().join(format!("cleanup-pages-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        state.sessions[0].begin_turn("legacy archive");
        state.sessions[0].archived_at = Some(1234);
        for _ in 1..130 {
            let mut session = AgentSession::new(state.projects[0].id, ProviderKind::Codex);
            session.begin_turn("legacy archive");
            session.archived_at = Some(1234);
            state.push_session(session);
        }
        store.save(&mut state).unwrap();
        let connection = store.open().unwrap();
        connection
            .execute("DELETE FROM workspace_cleanup_intents", [])
            .unwrap();
        let count = || {
            connection
                .query_row(
                    "SELECT count(*) FROM workspace_cleanup_intents",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .unwrap()
        };
        let mut cursor = None;
        assert!(
            store
                .recover_workspace_cleanup_intents(&mut cursor, || true)
                .unwrap()
        );
        assert_eq!(count(), 128);
        let committed_cursor = cursor;
        assert!(
            store
                .recover_workspace_cleanup_intents(&mut cursor, || false)
                .unwrap()
        );
        assert_eq!(cursor, committed_cursor);
        connection
            .execute_batch(
                "CREATE TRIGGER deny_backfill BEFORE INSERT ON workspace_cleanup_intents
             BEGIN SELECT RAISE(ABORT, 'controlled backfill failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .recover_workspace_cleanup_intents(&mut cursor, || true)
                .is_err()
        );
        assert_eq!(cursor, committed_cursor);
        assert_eq!(count(), 128);
        connection
            .execute_batch("DROP TRIGGER deny_backfill")
            .unwrap();
        assert!(
            !store
                .recover_workspace_cleanup_intents(&mut cursor, || true)
                .unwrap()
        );
        assert_eq!(count(), 130);
        drop(connection);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
