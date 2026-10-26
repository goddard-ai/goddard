//! Derived catalog cache: one owner, one pending update per host, one writer.
//! Callers transfer catalogs instead of cloning them for every disk write.

use super::*;

pub enum CatalogCacheUpdate {
    Replace(TaskCatalog),
    Remove,
    RemoveProject(Uuid),
}

#[derive(Default)]
struct HostUpdate {
    // None keeps the previous catalog; Some(None) removes the host.
    replacement: Option<Option<TaskCatalog>>,
    removed_projects: HashSet<Uuid>,
}

pub struct CatalogCacheWriter {
    pending: Arc<Mutex<HashMap<Uuid, HostUpdate>>>,
    wake: crossbeam_channel::Sender<()>,
}

impl CatalogCacheWriter {
    pub fn start(path: PathBuf, mut catalogs: HashMap<Uuid, TaskCatalog>) -> io::Result<Self> {
        let pending = Arc::new(Mutex::new(HashMap::<Uuid, HostUpdate>::new()));
        let shared = Arc::clone(&pending);
        let (wake, receiver) = crossbeam_channel::bounded(1);
        std::thread::Builder::new()
            .name("waku-catalog-cache".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    loop {
                        let updates = std::mem::take(&mut *shared.lock());
                        if updates.is_empty() {
                            break;
                        }
                        for (host, update) in updates {
                            if let Some(replacement) = update.replacement {
                                if let Some(catalog) = replacement {
                                    catalogs.insert(host, catalog);
                                } else {
                                    catalogs.remove(&host);
                                }
                            }
                            if let Some(catalog) = catalogs.get_mut(&host) {
                                catalog.projects.retain(|project| {
                                    !update.removed_projects.contains(&project.id)
                                });
                                catalog.sessions.retain(|session| {
                                    !session.incognito
                                        && !update.removed_projects.contains(&session.project_id)
                                });
                            }
                        }
                        // Sanitize startup caches too, including hosts untouched
                        // by this batch. Incognito data never reaches disk.
                        for catalog in catalogs.values_mut() {
                            catalog.sessions.retain(|session| !session.incognito);
                        }
                        if let Err(error) = write_cache(&path, &catalogs) {
                            eprintln!("remote catalog cache write failed: {error}");
                        }
                    }
                }
            })?;
        Ok(Self { pending, wake })
    }

    /// Returns a superseded pending catalog, if any. UI callers must reclaim
    /// that plain data on their background executor. No payload is cloned or
    /// destroyed under the queue lock.
    pub fn queue(&self, host: Uuid, update: CatalogCacheUpdate) -> Option<TaskCatalog> {
        let retired = {
            let mut pending = self.pending.lock();
            let entry = pending.entry(host).or_default();
            match update {
                CatalogCacheUpdate::Replace(catalog) => {
                    entry.removed_projects.clear();
                    entry.replacement.replace(Some(catalog)).flatten()
                }
                CatalogCacheUpdate::Remove => {
                    entry.removed_projects.clear();
                    entry.replacement.replace(None).flatten()
                }
                CatalogCacheUpdate::RemoveProject(project) => {
                    entry.removed_projects.insert(project);
                    None
                }
            }
        };
        // A full wake channel already guarantees a drain of the latest slot.
        // The worker owns no sender and exits after the last client drops.
        let _ = self.wake.try_send(());
        retired
    }
}

fn write_cache(path: &Path, catalogs: &HashMap<Uuid, TaskCatalog>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut writer = io::BufWriter::new(options.open(&temporary)?);
        serde_json::to_writer(&mut writer, catalogs).map_err(to_io_error)?;
        writer.flush()?;
        drop(writer);
        // This is a reconstructible cache; atomic replacement matters, but
        // it does not need the durability barrier of user-authored state.
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn catalog_cache_keeps_latest_updates_removals_and_incognito_privacy() {
        let root = std::env::temp_dir().join(format!("waku-catalog-cache-{}", Uuid::new_v4()));
        let path = root.join("catalog.json");
        let host = Uuid::new_v4();
        let removed_host = Uuid::new_v4();
        let project = Project::from_path(root.join("project"));
        let removed_project = Project::from_path(root.join("removed"));
        let ordinary = AgentSession::new(project.id, ProviderKind::Codex);
        let mut private = ordinary.clone();
        private.id = Uuid::new_v4();
        private.incognito = true;
        private.title = "must never be written".into();
        let removed = AgentSession::new(removed_project.id, ProviderKind::Codex);
        let writer = CatalogCacheWriter::start(path.clone(), HashMap::new()).unwrap();
        for version in 0..200 {
            let mut ordinary = ordinary.clone();
            ordinary.title = format!("version {version}");
            writer.queue(
                host,
                CatalogCacheUpdate::Replace(TaskCatalog {
                    projects: vec![project.clone(), removed_project.clone()],
                    sessions: vec![ordinary, private.clone(), removed.clone()],
                }),
            );
        }
        writer.queue(host, CatalogCacheUpdate::RemoveProject(removed_project.id));
        writer.queue(
            removed_host,
            CatalogCacheUpdate::Replace(TaskCatalog::default()),
        );
        writer.queue(removed_host, CatalogCacheUpdate::Remove);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(bytes) = fs::read(&path)
                && let Ok(catalogs) = serde_json::from_slice::<HashMap<Uuid, TaskCatalog>>(&bytes)
                && let Some(catalog) = catalogs.get(&host)
                && catalog.sessions.len() == 1
                && catalog.sessions[0].title == "version 199"
                && catalog.projects.len() == 1
                && !catalogs.contains_key(&removed_host)
            {
                assert!(
                    !String::from_utf8(bytes)
                        .unwrap()
                        .contains("must never be written")
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "cache did not settle to its latest state"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let worker = Arc::downgrade(&writer.pending);
        drop(writer);
        while worker.upgrade().is_some() {
            assert!(Instant::now() < deadline, "cache worker did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        let catalogs: HashMap<Uuid, TaskCatalog> =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(catalogs[&host].sessions.len(), 1);
        assert_eq!(catalogs[&host].sessions[0].title, "version 199");
        assert_eq!(catalogs[&host].projects.len(), 1);
        assert!(!catalogs.contains_key(&removed_host));
        fs::remove_dir_all(root).unwrap();
    }
}
