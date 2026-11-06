use super::*;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};
use waku_protocol::boss::{BossOperation, BossResult};
use waku_protocol::sidebar_intent::{SidebarEdit, SidebarIntent, SidebarIntentResult};

#[derive(Deserialize, Serialize)]
enum ReceiptResult {
    Applied,
    Blocked { reason: String },
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    intent: SidebarIntent,
    result: Option<ReceiptResult>,
}

/// Keep one high-water mark per client and field, including uncertain writes.
/// An interrupted apply is never automatically executed a second time.
pub(super) struct Receipts {
    path: PathBuf,
    rows: Vec<Receipt>,
    load_failed: bool,
}
impl Receipts {
    pub(super) fn open(path: PathBuf) -> anyhow::Result<Self> {
        let loaded = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        };
        let load_failed = loaded.is_err();
        Ok(Self {
            path,
            rows: loaded.unwrap_or_default(),
            load_failed,
        })
    }
    fn save(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.load_failed, "sidebar receipts need repair");
        let temporary = self.path.with_extension("tmp");
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(&self.rows)?)?;
        file.sync_all()?;
        fs::rename(temporary, &self.path)?;
        #[cfg(unix)]
        if let Some(parent) = self.path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}
fn same_value(left: &SidebarEdit, right: &SidebarEdit) -> bool {
    use waku_protocol::task_metadata::TaskMetadataEdit;
    match (left, right) {
        (
            SidebarEdit::Task {
                id: left_id,
                edit: left,
            },
            SidebarEdit::Task {
                id: right_id,
                edit: right,
            },
        ) if left_id == right_id => match (left, right) {
            (
                TaskMetadataEdit::Archive { archived_at: left },
                TaskMetadataEdit::Archive { archived_at: right },
            ) => left.is_some() == right.is_some(),
            (
                TaskMetadataEdit::Placement {
                    dormant_at: ld,
                    pinned_at: lp,
                    dormant_exempt_until: le,
                },
                TaskMetadataEdit::Placement {
                    dormant_at: rd,
                    pinned_at: rp,
                    dormant_exempt_until: re,
                },
            ) => ld.is_some() == rd.is_some() && lp.is_some() == rp.is_some() && le == re,
            (
                TaskMetadataEdit::Rename { title: left },
                TaskMetadataEdit::Rename { title: right },
            ) => left.trim() == right.trim(),
            _ => false,
        },
        _ => left == right,
    }
}

impl WakuBackend {
    pub(super) fn apply_sidebar_intent(
        &self,
        intent: SidebarIntent,
        events: EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        anyhow::ensure!(
            !intent.client_id.is_nil() && !intent.operation_id.is_nil() && intent.generation > 0,
            "invalid sidebar intent identity"
        );
        let mut receipts = self.sidebar_intents.lock();
        if receipts.load_failed {
            return Ok(ResponsePayload::SidebarIntent { result: SidebarIntentResult::Blocked { reason: "Sidebar synchronization receipts could not be loaded. Repair the receipt file before retrying.".into() } });
        }
        let key = intent.edit.key();
        let position = receipts
            .rows
            .iter()
            .position(|r| r.intent.client_id == intent.client_id && r.intent.edit.key() == key);
        if let Some(position) = position {
            let previous = &receipts.rows[position];
            if previous.intent.generation > intent.generation {
                return Ok(ResponsePayload::SidebarIntent {
                    result: SidebarIntentResult::Superseded,
                });
            }
            if previous.intent.generation == intent.generation {
                anyhow::ensure!(
                    previous.intent == intent,
                    "sidebar intent identity conflict"
                );
                let result = match &previous.result {
                    Some(ReceiptResult::Applied) => self.sidebar_current_result(&intent.edit)?,
                    Some(ReceiptResult::Blocked { reason }) => SidebarIntentResult::Blocked { reason: reason.clone() },
                    None => SidebarIntentResult::Blocked { reason: "An earlier save was interrupted; refresh and explicitly retry this change.".into() },
                };
                return Ok(ResponsePayload::SidebarIntent { result });
            }
        } else {
            anyhow::ensure!(
                receipts.rows.len() < 100_000,
                "sidebar receipt capacity exhausted"
            );
        }
        let predecessor = position.and_then(|index| {
            matches!(receipts.rows[index].result, Some(ReceiptResult::Applied))
                .then(|| receipts.rows[index].intent.edit.clone())
        });
        // Fence before effects. If either save fails, retain the fence in memory
        // as well: a transport retry cannot repeat an uncertain side effect.
        let receipt = Receipt {
            intent: intent.clone(),
            result: None,
        };
        let position = if let Some(position) = position {
            receipts.rows[position] = receipt;
            position
        } else {
            receipts.rows.push(receipt);
            receipts.rows.len() - 1
        };
        receipts.save()?;
        let result = self
            .execute_sidebar_edit(&intent, predecessor.as_ref(), events)
            .unwrap_or_else(|_| SidebarIntentResult::Blocked {
                reason: "The daemon could not save this change. Refresh its state before retrying."
                    .into(),
            });
        receipts.rows[position].result = Some(match &result {
            SidebarIntentResult::Task { .. }
            | SidebarIntentResult::Deliverables { .. }
            | SidebarIntentResult::Removed => ReceiptResult::Applied,
            SidebarIntentResult::Blocked { reason } => ReceiptResult::Blocked {
                reason: reason.clone(),
            },
            SidebarIntentResult::Superseded => unreachable!("new intent cannot be superseded"),
        });
        if let Err(error) = receipts.save() {
            receipts.rows[position].result = None;
            return Err(error);
        }
        Ok(ResponsePayload::SidebarIntent { result })
    }

    fn sidebar_current_result(&self, edit: &SidebarEdit) -> anyhow::Result<SidebarIntentResult> {
        match edit {
            SidebarEdit::Task { id, .. } => Ok(self
                .task_state
                .lock()
                .sessions
                .iter()
                .find(|s| s.id == *id)
                .map(|s| SidebarIntentResult::Task {
                    session: Box::new(s.list_projection()),
                })
                .unwrap_or(SidebarIntentResult::Removed)),
            _ => Ok(SidebarIntentResult::Deliverables {
                state: Box::new(self.boss.document()),
            }),
        }
    }

    fn execute_sidebar_edit(
        &self,
        intent: &SidebarIntent,
        predecessor: Option<&SidebarEdit>,
        events: EventSink,
    ) -> anyhow::Result<SidebarIntentResult> {
        if !matches!(&intent.edit, SidebarEdit::Task { .. })
            && !self.settings.get().boss_experiment_enabled
        {
            return Ok(SidebarIntentResult::Blocked {
                reason: "Enable Boss on the owning host before synchronizing deliverable changes."
                    .into(),
            });
        }
        let current = match &intent.edit {
            SidebarEdit::Task { id, edit } => self
                .task_state
                .lock()
                .sessions
                .iter()
                .find(|s| s.id == *id)
                .map(|s| SidebarEdit::Task {
                    id: *id,
                    edit: waku_protocol::task_metadata::TaskMetadataEdit::capture(edit.field(), s),
                }),
            edit => {
                let state = self.boss.document();
                match edit {
                    SidebarEdit::DeliverableArchive { id, .. } => {
                        state.deliverables.iter().find(|d| d.id == *id).map(|d| {
                            SidebarEdit::DeliverableArchive {
                                id: *id,
                                archived: d.archived_at.is_some(),
                            }
                        })
                    }
                    SidebarEdit::DeliverablePin { id, .. } => {
                        state.deliverables.iter().find(|d| d.id == *id).map(|d| {
                            SidebarEdit::DeliverablePin {
                                id: *id,
                                pinned: d.pinned_at.is_some(),
                            }
                        })
                    }
                    SidebarEdit::DeliverableDormant { id, .. } => {
                        state.deliverables.iter().find(|d| d.id == *id).map(|d| {
                            SidebarEdit::DeliverableDormant {
                                id: *id,
                                dormant: d.dormant_at.is_some(),
                            }
                        })
                    }
                    _ => None,
                }
            }
        };
        if current.is_none() {
            return Ok(SidebarIntentResult::Removed);
        }
        if !matches!(
            &intent.edit,
            SidebarEdit::Task {
                edit: waku_protocol::task_metadata::TaskMetadataEdit::Rename { .. },
                ..
            }
        ) && current
            .as_ref()
            .is_some_and(|current| same_value(current, &intent.edit))
        {
            return self.sidebar_current_result(&intent.edit);
        }
        if let Some(expected) = &intent.expected
            && !current
                .as_ref()
                .is_some_and(|current| same_value(current, expected))
            && !current
                .as_ref()
                .is_some_and(|current| same_value(current, &intent.edit))
            && !current
                .as_ref()
                .zip(predecessor)
                .is_some_and(|(current, previous)| same_value(current, previous))
        {
            return Ok(SidebarIntentResult::Blocked { reason: "This field changed elsewhere. Use daemon state, or explicitly retry to apply your change.".into() });
        }
        let command = match &intent.edit {
            SidebarEdit::Task { id, edit } => {
                let state = self.task_state.lock();
                let Some(session) = state.sessions.iter().find(|s| s.id == *id) else {
                    return Ok(SidebarIntentResult::Blocked {
                        reason: "This task no longer exists.".into(),
                    });
                };
                if intent
                    .expected_revision
                    .is_some_and(|r| r != session.metadata_revision)
                {
                    return Ok(SidebarIntentResult::Blocked {
                        reason: "This task changed elsewhere. Refresh before applying your change."
                            .into(),
                    });
                }
                Command::UpdateTaskMetadata {
                    updates: vec![waku_protocol::task_metadata::TaskMetadataUpdate {
                        session_id: *id,
                        edit: edit.clone(),
                    }],
                }
            }
            edit => {
                if intent
                    .expected_revision
                    .is_some_and(|r| r != self.boss.document().revision)
                {
                    return Ok(SidebarIntentResult::Blocked {
                        reason:
                            "Deliverables changed elsewhere. Refresh before applying your change."
                                .into(),
                    });
                }
                let operation = match edit {
                    SidebarEdit::DeliverableArchive { id, archived } => {
                        BossOperation::ArchiveDeliverable {
                            id: *id,
                            archived: *archived,
                        }
                    }
                    SidebarEdit::DeliverablePin { id, pinned } => BossOperation::PinDeliverable {
                        id: *id,
                        pinned: *pinned,
                    },
                    SidebarEdit::DeliverableDormant { id, dormant } => {
                        BossOperation::SweepDeliverable {
                            id: *id,
                            dormant: *dormant,
                        }
                    }
                    SidebarEdit::Task { .. } => unreachable!(),
                };
                Command::Boss { operation }
            }
        };
        let response = self.handle(
            Request {
                request_id: intent.operation_id,
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command,
            },
            events,
            None,
        )?;
        match response {
            ResponsePayload::TaskStateSaved { sessions } => {
                let session = sessions
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("missing sidebar acknowledgement"))?;
                Ok(SidebarIntentResult::Task {
                    session: Box::new(session),
                })
            }
            ResponsePayload::Boss {
                result: BossResult::Saved,
            } => Ok(SidebarIntentResult::Deliverables {
                state: Box::new(self.boss.document()),
            }),
            _ => bail!("invalid sidebar acknowledgement"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::task_metadata::TaskMetadataEdit;
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("sidebar-intents-{}", Uuid::new_v4())))
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn edit(client: Uuid, generation: u64, id: Uuid, archived: bool) -> SidebarIntent {
        SidebarIntent {
            client_id: client,
            generation,
            operation_id: Uuid::new_v4(),
            expected_revision: None,
            expected: None,
            edit: SidebarEdit::Task {
                id,
                edit: TaskMetadataEdit::Archive {
                    archived_at: archived.then_some(123),
                },
            },
        }
    }
    fn apply(backend: &WakuBackend, intent: SidebarIntent) -> SidebarIntentResult {
        let ResponsePayload::SidebarIntent { result } = backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command: Command::ApplySidebarIntent { intent },
                },
                EventSink::detached(),
                None,
            )
            .unwrap()
        else {
            panic!("missing receipt")
        };
        result
    }
    #[test]
    fn sidebar_receipts_reject_old_archive_after_restore_and_daemon_replacement() {
        let root = Root::new();
        let (backend, id) = super::super::tests::surface_test_backend(&root.0);
        let client = Uuid::new_v4();
        let archive = edit(client, 1, id, true);
        let first = apply(&backend, archive.clone());
        let SidebarIntentResult::Task { session } = first else {
            panic!("archive not saved")
        };
        let revision = session.metadata_revision;
        let SidebarIntentResult::Task { session } = apply(&backend, archive.clone()) else {
            panic!("duplicate not acknowledged")
        };
        assert_eq!(
            session.metadata_revision, revision,
            "lost replies must not reapply"
        );
        assert!(matches!(
            apply(&backend, edit(client, 2, id, false)),
            SidebarIntentResult::Task { .. }
        ));
        drop(backend);
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.0.join("settings.json")).unwrap(),
            StateStore::daemon(root.0.join("app.db")),
        )
        .unwrap();
        assert!(matches!(
            apply(&backend, archive),
            SidebarIntentResult::Superseded
        ));
        assert!(
            backend
                .task_store
                .load()
                .unwrap()
                .sessions
                .into_iter()
                .find(|s| s.id == id)
                .unwrap()
                .archived_at
                .is_none()
        );
    }
    #[test]
    fn sidebar_conflicts_are_per_field_and_do_not_overwrite_another_client() {
        let root = Root::new();
        let (backend, id) = super::super::tests::surface_test_backend(&root.0);
        let original = backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .title
            .clone();
        let title = |text: &str| SidebarEdit::Task {
            id,
            edit: TaskMetadataEdit::Rename { title: text.into() },
        };
        let mut first = edit(Uuid::new_v4(), 1, id, false);
        first.edit = title("Changed elsewhere");
        first.expected = Some(title(&original));
        assert!(matches!(
            apply(&backend, first),
            SidebarIntentResult::Task { .. }
        ));
        let mut stale = edit(Uuid::new_v4(), 1, id, false);
        stale.edit = title("Offline edit");
        stale.expected = Some(title(&original));
        assert!(matches!(
            apply(&backend, stale),
            SidebarIntentResult::Blocked { .. }
        ));
        let mut archive = edit(Uuid::new_v4(), 1, id, true);
        archive.expected = Some(SidebarEdit::Task {
            id,
            edit: TaskMetadataEdit::Archive { archived_at: None },
        });
        assert!(
            matches!(apply(&backend, archive), SidebarIntentResult::Task { .. }),
            "an independent title edit must not conflict with archive"
        );
        assert_eq!(
            backend
                .task_store
                .load()
                .unwrap()
                .sessions
                .into_iter()
                .find(|s| s.id == id)
                .unwrap()
                .title,
            "Changed elsewhere"
        );
    }
    #[test]
    fn sidebar_missing_targets_settle_without_recreating_deleted_tasks() {
        let root = Root::new();
        let (backend, _) = super::super::tests::surface_test_backend(&root.0);
        let missing = Uuid::new_v4();
        let intent = edit(Uuid::new_v4(), 1, missing, true);
        assert!(matches!(
            apply(&backend, intent.clone()),
            SidebarIntentResult::Removed
        ));
        assert!(matches!(
            apply(&backend, intent),
            SidebarIntentResult::Removed
        ));
        assert!(
            !backend
                .task_state
                .lock()
                .sessions
                .iter()
                .any(|session| session.id == missing)
        );
    }
    #[test]
    fn sidebar_interrupted_apply_remains_fenced_and_payload_reuse_is_rejected() {
        let root = Root::new();
        let (backend, id) = super::super::tests::surface_test_backend(&root.0);
        let intent = edit(Uuid::new_v4(), 1, id, true);
        {
            let mut receipts = backend.sidebar_intents.lock();
            receipts.rows.push(Receipt {
                intent: intent.clone(),
                result: None,
            });
            receipts.save().unwrap();
        }
        assert!(matches!(
            apply(&backend, intent.clone()),
            SidebarIntentResult::Blocked { .. }
        ));
        assert!(
            backend
                .task_state
                .lock()
                .sessions
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .archived_at
                .is_none()
        );
        let mut changed = intent;
        changed.edit = SidebarEdit::Task {
            id,
            edit: TaskMetadataEdit::Archive { archived_at: None },
        };
        assert!(
            backend
                .apply_sidebar_intent(changed, EventSink::detached())
                .is_err()
        );
    }
}
