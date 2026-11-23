//! Daemon-owned, user-editable configuration.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use parking_lot::{Mutex, RwLock};
use uuid::Uuid;
pub use waku_protocol::settings::DaemonSettings;

use crate::request_timing::phase;

pub struct DaemonSettingsStore {
    path: PathBuf,
    snapshot: RwLock<DaemonSettings>,
    writer: Mutex<()>,
}

impl DaemonSettingsStore {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        Self::open_with_legacy(path, std::iter::empty())
    }

    /// Open the daemon document, importing the first old combined settings
    /// file only when the new path does not exist. The source is left intact
    /// so the desktop can independently migrate its app-owned fields.
    pub fn open_with_legacy(
        path: PathBuf,
        legacy_paths: impl IntoIterator<Item = PathBuf>,
    ) -> io::Result<Self> {
        let (mut settings, write_current) = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(settings) => (settings, false),
                Err(error) => {
                    let backup = quarantine_corrupt_settings(&path)?;
                    eprintln!(
                        "Goddard daemon moved invalid settings to {}: {error}",
                        backup.display()
                    );
                    (DaemonSettings::default(), true)
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut restored: Option<DaemonSettings> = None;
                for legacy_path in legacy_paths {
                    if legacy_path == path {
                        continue;
                    }
                    match fs::read(legacy_path) {
                        Ok(bytes) => {
                            if let Ok(settings) = serde_json::from_slice(&bytes) {
                                restored = Some(settings);
                                break;
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
                let migrated = restored.is_some();
                (restored.unwrap_or_default(), migrated)
            }
            Err(error) => return Err(error),
        };
        let removed_project_map_setting = settings.extra.contains_key("project_map_enabled");
        settings.discard_legacy_app_keys();
        let write_current = write_current
            || removed_project_map_setting
            || waku_protocol::auto_prompts::refresh_untouched_shipped_rules(
                &mut settings.auto_prompts,
            );
        if write_current {
            write_atomic(&path, &settings)?;
        }
        Ok(Self {
            path,
            snapshot: RwLock::new(settings),
            writer: Mutex::new(()),
        })
    }

    pub fn get(&self) -> DaemonSettings {
        phase("settings.readLock", || self.snapshot.read()).clone()
    }

    pub fn replace(&self, mut settings: DaemonSettings) -> io::Result<()> {
        settings.discard_legacy_app_keys();
        // Keep persistence and publication in the same order across writers.
        let _writer = phase("settings.writeLock", || self.writer.lock());
        phase("settings.write", || write_atomic(&self.path, &settings))?;
        *phase("settings.publishLock", || self.snapshot.write()) = settings;
        Ok(())
    }
}

fn quarantine_corrupt_settings(path: &Path) -> io::Result<PathBuf> {
    let extension = format!("json.corrupt-{}", Uuid::new_v4().simple());
    let backup = path.with_extension(extension);
    fs::rename(path, &backup)?;
    Ok(backup)
}

fn write_atomic(path: &Path, settings: &DaemonSettings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(settings).map_err(to_io_error)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, data)?;
    fs::rename(temporary, path)
}

fn to_io_error(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use uuid::Uuid;
    use waku_protocol::model::ProviderKind;

    #[cfg(unix)]
    #[test]
    fn reads_keep_the_persisted_snapshot_during_a_failed_write() {
        use std::ffi::CString;
        use std::io::Read as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        use std::sync::mpsc;
        use std::thread;
        use std::time::{Duration, Instant};

        let directory = std::env::temp_dir().join(format!("waku-settings-{}", Uuid::new_v4()));
        let path = directory.join("settings.json");
        let store = DaemonSettingsStore::open(path.clone()).unwrap();
        let mut persisted = store.get();
        persisted.disabled_providers = vec![ProviderKind::Claude];
        store.replace(persisted.clone()).unwrap();

        let temporary = path.with_extension("json.tmp");
        let fifo_path = CString::new(temporary.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: the FIFO is inside this test's unique settings directory.
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        let mut fifo = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&temporary)
            .unwrap();

        let (write_started, observed, write_result) = thread::scope(|scope| {
            let mut rejected = persisted.clone();
            // Exceed the pipe buffer so the save stays in progress until the reader closes.
            rejected.extra.insert(
                "test_payload".into(),
                Value::from("x".repeat(2 * 1024 * 1024)),
            );
            let store = &store;
            let writer = scope.spawn(move || store.replace(rejected));

            let timeout = Duration::from_secs(5);
            let deadline = Instant::now() + timeout;
            let write_started = loop {
                match fifo.read(&mut [0]) {
                    Ok(1) => break true,
                    Ok(_) => (),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                    Err(error) => panic!("could not read the settings test pipe: {error}"),
                }
                if Instant::now() >= deadline {
                    break false;
                }
                thread::sleep(Duration::from_millis(2));
            };
            let (send, receive) = mpsc::channel();
            scope.spawn(move || send.send(store.get()).ok());
            let observed = receive.recv_timeout(timeout);

            fs::remove_file(&temporary).unwrap();
            drop(fifo);
            (write_started, observed, writer.join().unwrap())
        });

        let current = store.get();
        let reopened = DaemonSettingsStore::open(path).unwrap().get();
        fs::remove_dir_all(directory).unwrap();

        assert!(write_started, "the writer did not reach the settings file");
        assert_eq!(
            observed.expect("settings read waited for persistence"),
            persisted
        );
        assert!(write_result.is_err());
        assert_eq!(current, persisted);
        assert_eq!(reopened, persisted);
    }

    #[test]
    fn concurrent_writes_leave_the_snapshot_and_file_in_agreement() {
        use std::sync::Barrier;
        use std::thread;

        let directory = std::env::temp_dir().join(format!("waku-settings-{}", Uuid::new_v4()));
        let path = directory.join("settings.json");
        let store = DaemonSettingsStore::open(path.clone()).unwrap();
        let days = [7, 14, 21, 28];
        let start = Barrier::new(days.len());

        thread::scope(|scope| {
            for days in days {
                let mut settings = store.get();
                settings.employee_chat_search_days = days;
                let (start, store) = (&start, &store);
                scope.spawn(move || {
                    start.wait();
                    store.replace(settings).unwrap();
                });
            }
        });

        let stored: DaemonSettings = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(days.contains(&stored.employee_chat_search_days));
        assert_eq!(store.get(), stored);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_combined_settings_keep_only_daemon_fields() {
        let path = std::env::temp_dir().join(format!("waku-settings-{}.json", Uuid::new_v4()));
        fs::write(
            &path,
            r#"{"theme":"dark","analytics_enabled":false,"computer_use_enabled":true,"project_map_enabled":false,"boss_rotation_enabled":false,"future":42}"#,
        )
        .unwrap();

        let store = DaemonSettingsStore::open(path.clone()).unwrap();
        let settings = store.get();
        assert!(settings.computer_use_enabled);
        assert_eq!(settings.extra.get("future"), Some(&Value::from(42)));
        assert!(!settings.extra.contains_key("project_map_enabled"));
        assert!(!settings.extra.contains_key("boss_rotation_enabled"));
        assert!(!settings.extra.contains_key("theme"));
        let migrated: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(migrated.get("project_map_enabled").is_none());
        assert!(migrated.get("boss_rotation_enabled").is_none());
        store.replace(settings).unwrap();

        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(value.get("theme").is_none());
        assert!(value.get("analytics_enabled").is_none());
        assert!(value.get("boss_rotation_enabled").is_none());
        assert_eq!(value["future"], 42);
        fs::remove_file(path).ok();
    }

    #[test]
    fn imports_daemon_fields_from_a_debug_combined_settings_file() {
        let directory = std::env::temp_dir().join(format!("waku-settings-{}", Uuid::new_v4()));
        let path = directory.join("home/settings.json");
        let legacy = directory.join("checkout/temp/settings.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(
            &legacy,
            r#"{"theme":"dark","disabled_providers":["claude"]}"#,
        )
        .unwrap();

        let store = DaemonSettingsStore::open_with_legacy(path.clone(), [legacy.clone()]).unwrap();

        assert_eq!(store.get().disabled_providers, vec![ProviderKind::Claude]);
        let migrated: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(migrated["disabled_providers"][0], "claude");
        assert!(migrated.get("theme").is_none());
        assert!(legacy.exists());
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn absent_auto_prompts_seed_the_shipped_defaults() {
        let path = std::env::temp_dir().join(format!("waku-settings-{}.json", Uuid::new_v4()));
        fs::write(&path, r#"{"computer_use_enabled":true}"#).unwrap();

        let store = DaemonSettingsStore::open(path.clone()).unwrap();
        let settings = store.get();
        assert_eq!(
            settings.auto_prompts,
            waku_protocol::auto_prompts::default_rules()
        );
        store.replace(settings).unwrap();

        // Once written, the seeded rule survives reloads.
        let store = DaemonSettingsStore::open(path.clone()).unwrap();
        assert_eq!(
            store.get().auto_prompts,
            waku_protocol::auto_prompts::default_rules()
        );
        fs::remove_file(path).ok();
    }

    #[test]
    fn explicit_empty_auto_prompts_stay_empty() {
        let path = std::env::temp_dir().join(format!("waku-settings-{}.json", Uuid::new_v4()));
        fs::write(&path, r#"{"auto_prompts":[]}"#).unwrap();

        let store = DaemonSettingsStore::open(path.clone()).unwrap();
        assert!(store.get().auto_prompts.is_empty());
        store.replace(store.get()).unwrap();

        // An emptied list serializes explicitly, so reloads must not reseed.
        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["auto_prompts"], Value::Array(Vec::new()));
        let store = DaemonSettingsStore::open(path.clone()).unwrap();
        assert!(store.get().auto_prompts.is_empty());
        fs::remove_file(path).ok();
    }

    #[test]
    fn corrupt_current_settings_are_quarantined_and_replaced() {
        let directory = std::env::temp_dir().join(format!("waku-settings-{}", Uuid::new_v4()));
        let path = directory.join("settings.json");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&path, b"{ definitely not json").unwrap();

        let store = DaemonSettingsStore::open(path.clone()).unwrap();

        assert_eq!(store.get(), DaemonSettings::default());
        assert!(serde_json::from_slice::<DaemonSettings>(&fs::read(&path).unwrap()).is_ok());
        assert!(fs::read_dir(&directory).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("settings.json.corrupt-")
        }));
        fs::remove_dir_all(directory).ok();
    }
}
