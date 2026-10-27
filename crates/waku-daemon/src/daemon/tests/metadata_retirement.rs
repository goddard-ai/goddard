use super::*;

struct PausedSideChatShutdown {
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}

impl crate::driver::DriverControl for PausedSideChatShutdown {
    fn prompt(&self, _: String) {}
    fn cancel(&self) {}
    fn respond(&self, _: String, _: String) {}
    fn rollback(&self, _: usize) -> anyhow::Result<Option<crate::model::ProviderResumeCursor>> {
        Ok(None)
    }
    fn begin_shutdown(&self) {
        self.entered.send(()).unwrap();
        self.release
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("controlled side-chat teardown was not released");
    }
}

#[test]
fn metadata_archive_stop_rechecks_durable_generation_after_side_chat_teardown() {
    use waku_protocol::SessionMetadataChange;
    for change in ["current", "rename", "unarchive", "rearchive", "corrupt"] {
        let root =
            std::env::temp_dir().join(format!("metadata-retirement-{change}-{}", Uuid::new_v4()));
        let (backend, id) = surface_test_backend(&root);
        let backend = Arc::new(backend);
        let workspace = root.join("repo");
        let child_id = {
            let mut state = backend.task_state.lock();
            let project = state
                .sessions
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .project_id;
            let mut child = AgentSession::new(project, ProviderKind::Codex);
            child.side_chat_of = Some(id);
            child.begin_turn("side chat");
            let child_id = child.id;
            state.push_session(child);
            backend.task_store.save(&mut state).unwrap();
            child_id
        };
        let runtime_id = Uuid::new_v4();
        let parent = Arc::new(CaptureDriver::default());
        backend.sessions.lock().insert(
            id,
            runtime_generation_entry(runtime_id, parent.clone(), &workspace),
        );
        let (entered, paused) = crossbeam_channel::bounded(1);
        let (release, resume) = crossbeam_channel::bounded(1);
        backend.sessions.lock().insert(
            child_id,
            RuntimeEntry {
                plan_gate: None,
                runtime_id: Uuid::new_v4(),
                driver: DriverHandle::from_control(Arc::new(PausedSideChatShutdown {
                    entered,
                    release: resume,
                })),
                stable: None,
                last_active: std::time::Instant::now(),
                resumable: true,
                computer_use_available: false,
                provider: ProviderKind::Codex,
                cwd: workspace.clone(),
            },
        );
        let token = backend.agent.mint(id);
        let mutation = Uuid::new_v4();
        let archiving = {
            let backend = Arc::clone(&backend);
            std::thread::spawn(move || {
                backend.update_session_metadata(
                    id,
                    0,
                    mutation,
                    SessionMetadataChange::Archive { archived: true },
                )
            })
        };
        paused
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("archive did not commit and enter side-chat teardown");
        let generation = backend
            .task_store
            .workspace_cleanup_generation(id)
            .unwrap()
            .unwrap();
        // Independent lifecycle/catalog locks model another daemon owning the
        // same durable store. The first backend deliberately keeps stale rows.
        let peer = WakuBackend::new(
            DaemonSettingsStore::open(root.join("peer-settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        match change {
            "current" => {}
            "rename" => {
                peer.update_session_metadata(
                    id,
                    1,
                    Uuid::new_v4(),
                    SessionMetadataChange::Title {
                        title: "still archived".into(),
                    },
                )
                .unwrap();
                assert_eq!(
                    peer.task_store.workspace_cleanup_generation(id).unwrap(),
                    Some(generation)
                );
            }
            "unarchive" | "rearchive" => {
                peer.update_session_metadata(
                    id,
                    1,
                    Uuid::new_v4(),
                    SessionMetadataChange::Archive { archived: false },
                )
                .unwrap();
                assert!(
                    peer.task_store
                        .workspace_cleanup_generation(id)
                        .unwrap()
                        .is_none()
                );
                if change == "rearchive" {
                    peer.update_session_metadata(
                        id,
                        2,
                        Uuid::new_v4(),
                        SessionMetadataChange::Archive { archived: true },
                    )
                    .unwrap();
                    assert_ne!(
                        peer.task_store.workspace_cleanup_generation(id).unwrap(),
                        Some(generation)
                    );
                }
            }
            "corrupt" => {
                rusqlite::Connection::open(backend.task_store.path()).unwrap().execute(
                    "UPDATE workspace_cleanup_intents SET generation = 'invalid' WHERE session_id = ?1",
                    [id.to_string()],
                ).unwrap();
            }
            _ => unreachable!(),
        }
        release.send(()).unwrap();
        assert!(matches!(
            archiving.join().unwrap().unwrap(),
            ResponsePayload::SessionMetadataUpdated { applied: true, .. }
        ));
        let should_stop = matches!(change, "current" | "rename");
        assert_eq!(*parent.shutdowns.lock(), u32::from(should_stop), "{change}");
        assert_eq!(
            backend.sessions.lock().contains_key(&id),
            !should_stop,
            "{change}"
        );
        assert_eq!(
            backend.agent.resolve(&token).is_none(),
            should_stop,
            "{change}"
        );
        if change == "current" {
            let successor = Arc::new(CaptureDriver::default());
            backend.sessions.lock().insert(
                id,
                runtime_generation_entry(runtime_id, successor.clone(), &workspace),
            );
            backend
                .update_session_metadata(
                    id,
                    0,
                    mutation,
                    SessionMetadataChange::Archive { archived: true },
                )
                .unwrap();
            assert_eq!(
                *successor.shutdowns.lock(),
                0,
                "retry recaptured a newer physical owner"
            );
            assert!(backend.sessions.lock().contains_key(&id));
        }
        drop(peer);
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }
}
