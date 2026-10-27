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

/// Narrow catalog state; never reads or deserializes a transcript.
#[derive(PartialEq, Eq)]
pub(super) struct CatalogWorkspace {
    project_id: String,
    project_path: Option<String>,
    workspace: Option<String>,
    archived_at: Option<i64>,
}

pub(super) fn catalog_workspace(
    connection: &Connection,
    session_id: Uuid,
) -> io::Result<Option<CatalogWorkspace>> {
    connection.query_row(
        "SELECT sessions.project_id, projects.path, sessions.workspace, sessions.archived_at
         FROM sessions LEFT JOIN projects ON projects.id = sessions.project_id WHERE sessions.id = ?1",
        [session_id.to_string()],
        |row| Ok(CatalogWorkspace { project_id: row.get(0)?, project_path: row.get(1)?, workspace: row.get(2)?, archived_at: row.get(3)? }),
    ).optional().map_err(to_io_error)
}

impl CatalogWorkspace {
    fn path(&self) -> io::Result<Option<PathBuf>> {
        let workspace = self
            .workspace
            .as_deref()
            .map(serde_json::from_str::<SessionWorkspace>)
            .transpose()
            .map_err(to_io_error)?
            .unwrap_or_default();
        Ok(workspace
            .path()
            .map(Path::to_owned)
            .or_else(|| self.project_path.as_ref().map(PathBuf::from)))
    }
}

/// Compare the actual promoted rows after SQL metadata fencing, so a stale
/// incoming save cannot unnecessarily block persistence during retirement.
pub(super) fn changed_catalog_paths(
    previous: Option<CatalogWorkspace>,
    next: Option<CatalogWorkspace>,
    paths: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let changed = match (&previous, &next) {
        (Some(before), Some(after)) => {
            before.project_id != after.project_id
                || before.project_path != after.project_path
                || before.workspace != after.workspace
                || (before.archived_at.is_some() && before.archived_at != after.archived_at)
        }
        (None, None) => false,
        _ => true,
    };
    if changed {
        for row in previous.iter().chain(next.iter()) {
            if let Some(path) = row.path()? {
                paths.push(path);
            }
        }
    }
    Ok(())
}

/// Project moves can invalidate cleanup authority without dirtying a session.
/// Compare only path columns when the project snapshot itself changed.
pub(super) fn changed_project_paths(
    connection: &Connection,
    projects: &[Project],
) -> io::Result<Vec<PathBuf>> {
    let mut statement = connection
        .prepare("SELECT id, path FROM projects")
        .map_err(to_io_error)?;
    let mut old = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(to_io_error)?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(to_io_error)?;
    let mut paths = Vec::new();
    for project in projects {
        let previous = old.remove(&project.id.to_string());
        if previous.as_deref() != project.path.to_str() {
            paths.extend(previous.map(PathBuf::from));
            paths.push(project.path.clone());
            project_workspaces(connection, &project.id.to_string(), &mut paths)?;
        }
    }
    for (id, path) in old {
        paths.push(PathBuf::from(path));
        project_workspaces(connection, &id, &mut paths)?;
    }
    Ok(paths)
}

fn project_workspaces(
    connection: &Connection,
    project_id: &str,
    paths: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let mut statement = connection
        .prepare("SELECT workspace FROM sessions WHERE project_id = ?1 AND workspace IS NOT NULL")
        .map_err(to_io_error)?;
    let rows = statement
        .query_map([project_id], |row| row.get::<_, String>(0))
        .map_err(to_io_error)?;
    for row in rows {
        let workspace: SessionWorkspace =
            serde_json::from_str(&row.map_err(to_io_error)?).map_err(to_io_error)?;
        if let Some(path) = workspace.path() {
            paths.push(path.to_owned());
        }
    }
    Ok(())
}

pub(super) fn guard_catalog_paths(
    store: &Path,
    paths: &[PathBuf],
) -> io::Result<Option<waku_base::workspace_admission::WorkspaceAdmission>> {
    waku_base::workspace_admission::WorkspaceAdmission::catalog(store, paths).map_err(|error| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("workspace catalog commit deferred: {error:#}"),
        )
    })
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

    #[cfg(unix)]
    #[test]
    fn catalog_changes_cannot_revoke_or_add_ownership_during_retirement() {
        use waku_base::workspace_admission::WorkspaceAdmission;
        let root = std::env::temp_dir().join(format!("catalog-retirement-{}", Uuid::new_v4()));
        let workspace = root.join("task-worktree");
        fs::create_dir_all(&workspace).unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&workspace, &alias).unwrap();
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("project"));
        let id = state.sessions[0].id;
        state.sessions[0].workspace = SessionWorkspace::Worktree {
            path: workspace.clone(),
            name: "task".into(),
            branch: None,
            base_branch: None,
            adopted_by: None,
            borrowed: false,
        };
        state.sessions[0].begin_turn("retained history");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].archived_at = Some(1234);
        store.save(&mut state).unwrap();
        state.mark_session_dirty(id);
        let delayed = store.save_batch(&mut state);
        let generation = store.workspace_cleanup_generation(id).unwrap();
        let peer = StateStore::daemon(store.path().to_owned());
        let mut peer_state = peer.load().unwrap();
        let retirement = WorkspaceAdmission::retirement(store.path(), &workspace).unwrap();

        let mut metadata = state.sessions[0].list_projection();
        metadata.archived_at = None;
        metadata.metadata_revision = 1;
        metadata.metadata_mutation_id = Some(Uuid::new_v4());
        let error = peer
            .write_session_metadata(&metadata, 0, &HashSet::new())
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(
            peer.load_session_detail(id).unwrap().unwrap().archived_at,
            Some(1234)
        );
        assert_eq!(peer.workspace_cleanup_generation(id).unwrap(), generation);

        // Renames and old saves which SQL metadata fencing keeps archived do
        // not add an owner or revoke cleanup authority, and must remain prompt.
        metadata.archived_at = Some(1234);
        metadata.title = "renamed while retired".into();
        peer.write_session_metadata(&metadata, 0, &HashSet::new())
            .unwrap();
        store.write_batch(&delayed).unwrap();
        assert_eq!(
            store
                .load()
                .unwrap()
                .sessions
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .title,
            metadata.title
        );
        assert_eq!(store.workspace_cleanup_generation(id).unwrap(), generation);

        let mut sibling = AgentSession::new(peer_state.projects[0].id, ProviderKind::Codex);
        sibling.workspace = SessionWorkspace::Worktree {
            path: alias,
            name: "shared".into(),
            branch: None,
            base_branch: None,
            adopted_by: None,
            borrowed: true,
        };
        sibling.begin_turn("shared workspace owner");
        sibling.finish_active_turn(crate::model::TurnStatus::Completed);
        let sibling_id = sibling.id;
        peer_state.push_session(sibling);
        let shared_batch = peer.save_batch(&mut peer_state);
        assert_eq!(
            peer.write_batch(&shared_batch).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(peer.load_session_detail(sibling_id).unwrap().is_none());

        let mut moved_owner = peer.load().unwrap();
        moved_owner.session_mut(id).unwrap().workspace = SessionWorkspace::Worktree {
            path: root.join("replacement-worktree"),
            name: "replacement".into(),
            branch: None,
            base_branch: None,
            adopted_by: None,
            borrowed: false,
        };
        moved_owner.mark_session_dirty(id);
        let moved_owner_batch = peer.save_batch(&mut moved_owner);
        assert_eq!(
            peer.write_batch(&moved_owner_batch).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            peer.load()
                .unwrap()
                .sessions
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .workspace
                .path(),
            Some(workspace.as_path())
        );

        // Project moves must guard materialized worktrees outside the ordinary
        // project directory, even without dirtying any session.
        let mut moved_project = peer.load().unwrap();
        moved_project.projects[0].path = root.join("new-project");
        let moved_batch = peer.save_batch(&mut moved_project);
        assert_eq!(
            peer.write_batch(&moved_batch).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(peer.load().unwrap().projects[0].path, root.join("project"));

        // Explicit removal must not revoke a live cleanup's authority mid-unit.
        let mut removed = peer.load().unwrap();
        removed.sessions.clear();
        let removed_batch = peer.save_batch(&mut removed);
        assert_eq!(
            peer.write_batch(&removed_batch).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(peer.load_session_detail(id).unwrap().is_some());
        drop(retirement);

        // A normal retry succeeds after the operation's final boundary and
        // revokes the original archive generation atomically.
        metadata.archived_at = None;
        metadata.metadata_revision = 2;
        peer.write_session_metadata(&metadata, 1, &HashSet::new())
            .unwrap();
        assert!(peer.workspace_cleanup_generation(id).unwrap().is_none());
        peer.write_batch(&shared_batch).unwrap();
        assert!(peer.load_session_detail(sibling_id).unwrap().is_some());
        drop(peer);
        drop(store);
        fs::remove_dir_all(root).unwrap();
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
