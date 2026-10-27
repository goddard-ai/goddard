//! Unqualified terminal writers are independent of catalog membership. A shell
//! exit is not proof that every descendant has stopped writing its workspace.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalWriterOrigin {
    pub workspace: PathBuf,
    pub identity: String,
}

impl TerminalWriterOrigin {
    fn validate(&self) -> io::Result<()> {
        if !self.workspace.is_absolute()
            || self.workspace.to_str().is_none_or(|path| path.len() > 8192)
            || self.identity.is_empty()
            || self.identity.len() > 128
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid terminal writer origin",
            ));
        }
        Ok(())
    }
}

impl StateStore {
    /// Must commit before the PTY factory can create a child. Repeated opens
    /// deduplicate, while a reused directory path retains both identities.
    pub fn record_terminal_writer(&self, origin: &TerminalWriterOrigin) -> io::Result<()> {
        origin.validate()?;
        self.open()?
            .execute(
                "INSERT INTO terminal_workspace_writers (workspace, identity, created_at)
             VALUES (?1, ?2, unixepoch()) ON CONFLICT(workspace, identity) DO NOTHING",
                params![origin.workspace.to_str(), origin.identity],
            )
            .map_err(to_io_error)?;
        Ok(())
    }

    /// Bounded, metadata-only safety scan. A caller receiving 65 rows must
    /// defer rather than infer quiet from this incomplete inventory.
    pub fn terminal_writer_origins(&self) -> io::Result<Vec<TerminalWriterOrigin>> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare(
                "SELECT substr(workspace,1,8193), substr(identity,1,129)
             FROM terminal_workspace_writers ORDER BY workspace, identity LIMIT 65",
            )
            .map_err(to_io_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(TerminalWriterOrigin {
                    workspace: PathBuf::from(row.get::<_, String>(0)?),
                    identity: row.get(1)?,
                })
            })
            .map_err(to_io_error)?;
        rows.map(|row| {
            let origin = row.map_err(to_io_error)?;
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
    fn terminal_origins_survive_catalog_removal_reload_and_path_reuse() {
        let root = std::env::temp_dir().join(format!("terminal-origin-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        store.save(&mut state).unwrap();
        let mut origin = TerminalWriterOrigin {
            workspace: root.join("workspace"),
            identity: "unix:1:2".into(),
        };
        store.record_terminal_writer(&origin).unwrap();
        store.record_terminal_writer(&origin).unwrap();
        origin.identity = "unix:1:3".into();
        store.record_terminal_writer(&origin).unwrap();
        state.sessions.clear();
        state.projects.clear();
        store.save(&mut state).unwrap();
        assert_eq!(store.terminal_writer_origins().unwrap().len(), 2);
        drop(store);
        let reopened = StateStore::daemon(root.join("app.db"));
        assert_eq!(reopened.terminal_writer_origins().unwrap().len(), 2);
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_origin_inventory_is_bounded_and_rejects_corruption() {
        let root = std::env::temp_dir().join(format!("terminal-origin-page-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        for index in 0..100 {
            store
                .record_terminal_writer(&TerminalWriterOrigin {
                    workspace: root.join(format!("workspace-{index:03}")),
                    identity: "unix:1:2".into(),
                })
                .unwrap();
        }
        assert_eq!(store.terminal_writer_origins().unwrap().len(), 65);
        store
            .open()
            .unwrap()
            .execute(
                "UPDATE terminal_workspace_writers SET identity = ?1 WHERE workspace = ?2",
                params!["x".repeat(129), root.join("workspace-000").to_str()],
            )
            .unwrap();
        assert!(store.terminal_writer_origins().is_err());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
