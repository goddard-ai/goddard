//! Immutable workspace origins recorded before stable provider admission.
//! No task foreign key: deleting a task does not prove its writer stopped.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StableWriterOrigin {
    pub session_id: Uuid,
    pub root: PathBuf,
    pub runtime_instance: Uuid,
    pub runtime_session_id: Uuid,
    pub generation: u64,
    pub workspace: PathBuf,
}

impl StableWriterOrigin {
    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.runtime_instance, self.runtime_session_id, self.generation
        )
    }
    fn validate(&self) -> io::Result<()> {
        for path in [&self.root, &self.workspace] {
            if !path.is_absolute() || path.to_str().is_none_or(|text| text.len() > 8192) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stable writer requires a bounded absolute path",
                ));
            }
        }
        if self.generation == 0 || self.generation > i64::MAX as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid stable writer generation",
            ));
        }
        Ok(())
    }
}

impl StateStore {
    pub fn stable_writer_begin(&self, origin: &StableWriterOrigin) -> io::Result<()> {
        origin.validate()?;
        let connection = self.open()?;
        let values = params![
            origin.key(),
            origin.session_id.to_string(),
            origin.root.to_str(),
            origin.runtime_instance.to_string(),
            origin.runtime_session_id.to_string(),
            origin.generation as i64,
            origin.workspace.to_str()
        ];
        connection.execute("INSERT INTO stable_runtime_writers
            (writer_key, session_id, root, runtime_instance, runtime_session_id, generation, workspace, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch()) ON CONFLICT(writer_key) DO NOTHING", values).map_err(to_io_error)?;
        let matches: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM stable_runtime_writers
            WHERE writer_key = ?1 AND session_id = ?2 AND root = ?3 AND runtime_instance = ?4
            AND runtime_session_id = ?5 AND generation = ?6 AND workspace = ?7)",
                values,
                |row| row.get(0),
            )
            .map_err(to_io_error)?;
        if !matches {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stable writer origin changed",
            ));
        }
        Ok(())
    }

    /// Bounded metadata-only keyset page. Corruption defers cleanup rather
    /// than silently skipping a possible workspace writer.
    pub fn stable_writer_page(&self, after: Option<&str>) -> io::Result<Vec<StableWriterOrigin>> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare(
                "SELECT substr(writer_key,1,100), substr(session_id,1,37),
            substr(root,1,8193), substr(runtime_instance,1,37), substr(runtime_session_id,1,37),
            generation, substr(workspace,1,8193) FROM stable_runtime_writers
            WHERE writer_key > ?1 ORDER BY writer_key LIMIT 64",
            )
            .map_err(to_io_error)?;
        let rows = statement
            .query_map(params![after.unwrap_or("")], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(to_io_error)?;
        rows.map(|row| {
            let (key, task, root, instance, session, generation, workspace) =
                row.map_err(to_io_error)?;
            let origin = StableWriterOrigin {
                session_id: Uuid::parse_str(&task).map_err(to_io_error)?,
                root: root.into(),
                runtime_instance: Uuid::parse_str(&instance).map_err(to_io_error)?,
                runtime_session_id: Uuid::parse_str(&session).map_err(to_io_error)?,
                generation: u64::try_from(generation).map_err(to_io_error)?,
                workspace: workspace.into(),
            };
            origin.validate()?;
            if key != origin.key() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid stable writer key",
                ));
            }
            Ok(origin)
        })
        .collect()
    }

    /// Legacy links without a known origin cannot be treated as quiet merely
    /// because a replacement daemon has not attached a driver yet.
    pub fn stable_writers_have_unknown_links(&self) -> io::Result<bool> {
        self.open()?.query_row("SELECT EXISTS(SELECT 1 FROM stable_runtime_links l
            WHERE NOT EXISTS(SELECT 1 FROM stable_runtime_writers w
                WHERE w.writer_key = l.runtime_instance || '/' || l.runtime_session_id || '/' || CAST(l.generation AS TEXT)
                AND w.runtime_instance = l.runtime_instance AND w.runtime_session_id = l.runtime_session_id
                AND w.generation = l.generation))", [], |row| row.get(0)).map_err(to_io_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_writer_origins_are_immutable_paged_and_survive_link_replacement() {
        let root = std::env::temp_dir().join(format!("stable-origins-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("workspace"));
        state.sessions[0].begin_turn("seed");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        store.save(&mut state).unwrap();
        let origin = StableWriterOrigin {
            session_id: state.sessions[0].id,
            root: root.join("host"),
            runtime_instance: Uuid::new_v4(),
            runtime_session_id: Uuid::new_v4(),
            generation: 1,
            workspace: root.join("workspace"),
        };
        store.stable_writer_begin(&origin).unwrap();
        store.stable_writer_begin(&origin).unwrap();
        for field in 0..3 {
            let mut replacement = origin.clone();
            match field {
                0 => replacement.workspace = root.join("other"),
                1 => replacement.root = root.join("other-host"),
                _ => replacement.session_id = Uuid::new_v4(),
            }
            assert!(store.stable_writer_begin(&replacement).is_err());
        }
        let mut relative = origin.clone();
        relative.workspace = "relative".into();
        assert!(store.stable_writer_begin(&relative).is_err());
        let link = StableLinkRow::admit(
            "goddard",
            origin.session_id,
            origin.runtime_session_id,
            origin.runtime_instance,
        );
        store.stable_link_save(&link).unwrap();
        assert!(!store.stable_writers_have_unknown_links().unwrap());
        let mut newer = link.clone();
        newer.generation += 1;
        store.stable_link_save(&newer).unwrap();
        assert!(store.stable_writers_have_unknown_links().unwrap());
        let mut new_origin = origin.clone();
        new_origin.generation += 1;
        store.stable_writer_begin(&new_origin).unwrap();
        assert!(!store.stable_writers_have_unknown_links().unwrap());
        store
            .stable_link_clear(
                newer.session_id,
                newer.runtime_instance,
                newer.runtime_session_id,
                newer.generation,
            )
            .unwrap();
        for _ in 0..65 {
            let mut other = origin.clone();
            other.runtime_session_id = Uuid::new_v4();
            store.stable_writer_begin(&other).unwrap();
        }
        let removed = store
            .open()
            .unwrap()
            .execute(
                "DELETE FROM sessions WHERE id = ?1",
                params![origin.session_id.to_string()],
            )
            .unwrap();
        assert_eq!(removed, 1);
        drop(store);
        let store = StateStore::daemon(root.join("app.db"));
        let first = store.stable_writer_page(None).unwrap();
        assert_eq!(first.len(), 64);
        let second = store
            .stable_writer_page(Some(&first.last().unwrap().key()))
            .unwrap();
        assert_eq!(second.len(), 3);
        assert!(first.iter().chain(&second).any(|record| record == &origin));
        assert!(
            first
                .iter()
                .chain(&second)
                .any(|record| record == &new_origin)
        );
        store
            .open()
            .unwrap()
            .execute(
                "UPDATE stable_runtime_writers SET workspace = 'relative' WHERE writer_key = ?1",
                params![origin.key()],
            )
            .unwrap();
        assert!(
            store.stable_writer_page(None).is_err()
                || store
                    .stable_writer_page(Some(&first.last().unwrap().key()))
                    .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
