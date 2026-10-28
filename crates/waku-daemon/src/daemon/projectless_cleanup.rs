//! Idle-admitted preservation of projectless directories.
//! Recovery records precede effects; source retirement remains a separate gate.
use super::*;
use crate::persistence::{CleanupRun, CleanupToken};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::task::Poll;
use waku_base::maintenance::{Progress, Scheduler, Unit};
use waku_base::projectless::{ArchiveCapture, ArchiveLocations, ArchiveVerification};
use waku_base::workspace_admission::{WorkspaceAdmission, catalog_path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn read(path: &Path, directory: bool) -> anyhow::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            },
            "cleanup artifact has an unexpected file type"
        );
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Recovery {
    version: u8,
    attempt: Uuid,
    root: PathBuf,
    source: PathBuf,
    source_identity: Identity,
    partial_identity: Option<Identity>,
    archive_identity: Option<Identity>,
}

impl WakuBackend {
    pub(super) fn schedule_projectless_cleanup(&self, token: CleanupToken) -> io::Result<()> {
        let weak = self.weak_self();
        let key = format!(
            "workspace-cleanup:{}:{}:{}:{:p}",
            self.task_store.path().display(),
            token.session_id,
            token.generation,
            weak.as_ptr()
        );
        Scheduler::shared()?.submit(key, Unit::Heavy, move |permit| {
            let Some(backend) = weak.upgrade() else {
                return Ok(Progress::Complete);
            };
            let result = match waku_base::projectless::workspace_root() {
                Some(root) => {
                    backend.advance_projectless_cleanup(token, &root, || permit.should_continue())
                }
                None => Ok(Progress::More),
            };
            if !matches!(result, Ok(Progress::Complete)) {
                // Queue full, cancellation, and uncertain artifacts must not
                // pin all scheduler keys. Preserve the durable retry instead.
                backend.task_store.retry_workspace_cleanup(
                    token,
                    crate::model::unix_time(),
                    result.is_err(),
                )?;
            }
            // Scheduler diagnostics must not print paths or ZIP entry names.
            Ok(Progress::Complete)
        })?;
        Ok(())
    }

    fn cleanup_scope(
        &self,
        token: CleanupToken,
        root: &Path,
        source: &Path,
        continue_work: &impl Fn() -> bool,
    ) -> anyhow::Result<bool> {
        if !continue_work() {
            return Ok(false);
        }
        let Some(descriptor) = self.task_store.workspace_cleanup_descriptor(token)? else {
            return Ok(false);
        };
        anyhow::ensure!(
            matches!(
                descriptor.workspace,
                SessionWorkspace::Local | SessionWorkspace::NewWorktree { .. }
            ),
            "cleanup descriptor is not a projectless workspace"
        );
        anyhow::ensure!(
            catalog_path(&descriptor.project_path)? == catalog_path(source)?,
            "cleanup source changed"
        );
        let target = catalog_path(source)?;
        let mut after = None;
        loop {
            if !continue_work() {
                return Ok(false);
            }
            let page = self.task_store.workspace_cleanup_catalog_page(after)?;
            if page.is_empty() {
                return Ok(true);
            }
            after = page.last().map(|row| row.session_id);
            for row in page {
                if !continue_work() {
                    return Ok(false);
                }
                if row.session_id == token.session_id {
                    continue;
                }
                let retained = !row.archived
                    || matches!(
                        row.workspace,
                        SessionWorkspace::Worktree { borrowed: true, .. }
                            | SessionWorkspace::Worktree {
                                adopted_by: Some(_),
                                ..
                            }
                    );
                let path = row.workspace.path().unwrap_or(&row.project_path);
                if matches!(
                    row.workspace,
                    SessionWorkspace::Local | SessionWorkspace::NewWorktree { .. }
                ) {
                    if let Ok(other) = ArchiveLocations::new(root, path, Uuid::nil()) {
                        let own = ArchiveLocations::new(root, source, Uuid::nil())?;
                        anyhow::ensure!(
                            other.archive != own.archive || catalog_path(path)? == target,
                            "cleanup archive destination has another source owner"
                        );
                    }
                }
                if !retained {
                    continue;
                }
                anyhow::ensure!(path.is_absolute(), "cleanup owner path is not absolute");
                let owner = catalog_path(path)?;
                anyhow::ensure!(
                    !(owner.starts_with(&target) || target.starts_with(&owner)),
                    "cleanup workspace has a retained catalog owner"
                );
            }
        }
    }

    /// All heavy state stays inside one admission. A load change cancels a
    /// partial capture before releasing its source lease; a later attempt
    /// restarts from the still-live source rather than trusting a stale ZIP.
    pub(super) fn advance_projectless_cleanup(
        &self,
        token: CleanupToken,
        root: &Path,
        continue_work: impl Fn() -> bool,
    ) -> anyhow::Result<Progress> {
        if !continue_work() {
            return Ok(Progress::More);
        }
        let Some(descriptor) = self.task_store.workspace_cleanup_descriptor(token)? else {
            return Ok(Progress::Complete);
        };
        if !matches!(
            descriptor.workspace,
            SessionWorkspace::Local | SessionWorkspace::NewWorktree { .. }
        ) {
            // Managed worktrees need their separate Git preservation executor.
            return Ok(Progress::Complete);
        }
        let source = descriptor.project_path;
        let mut saved = self.task_store.workspace_cleanup_run(token)?;
        let mut recovery: Recovery = match saved.as_ref() {
            Some(run) => serde_json::from_str(&run.payload)?,
            None => Recovery {
                version: 1,
                attempt: Uuid::new_v4(),
                root: root.to_owned(),
                source: source.clone(),
                source_identity: Identity::read(&source, true)?,
                partial_identity: None,
                archive_identity: None,
            },
        };
        anyhow::ensure!(
            recovery.version == 1 && recovery.root == root && recovery.source == source,
            "cleanup recovery descriptor changed"
        );
        let paths = ArchiveLocations::new(root, &source, recovery.attempt)?;
        if saved.as_ref().is_some_and(|run| run.phase == "preserved") {
            anyhow::ensure!(
                Some(Identity::read(&paths.archive, false)?) == recovery.archive_identity,
                "cleanup preserved archive identity changed"
            );
            self.task_store
                .park_workspace_cleanup(token, crate::model::unix_time())?;
            return Ok(Progress::Complete);
        }
        let _live_admission = self.begin_workspace_retirement(&source)?;
        let _archive_admission =
            WorkspaceAdmission::materialize(self.task_store.path(), &paths.archive)?;
        anyhow::ensure!(
            Identity::read(&source, true)? == recovery.source_identity,
            "cleanup source identity changed"
        );
        // A different process may have advanced the run before we got admission.
        anyhow::ensure!(
            self.task_store.workspace_cleanup_run(token)? == saved,
            "cleanup recovery changed during admission"
        );
        if !self.cleanup_scope(token, root, &source, &continue_work)? {
            return Ok(Progress::More);
        }

        let phase = saved
            .as_ref()
            .map(|run| run.phase.as_str())
            .unwrap_or("capture")
            .to_owned();
        anyhow::ensure!(
            matches!(phase.as_str(), "capture" | "verify" | "publish"),
            "unknown cleanup recovery phase"
        );
        {
            // A previous lease ended before publication. Discard only its known
            // partial file and recapture current contents under this fresh lease.
            if paths.partial.try_exists()? {
                anyhow::ensure!(
                    Some(Identity::read(&paths.partial, false)?) == recovery.partial_identity,
                    "cleanup partial identity is uncertain"
                );
                fs::remove_file(&paths.partial)?;
            }
            // Never replace an archive of unknown provenance. A crash after
            // publication leaves the live source and its prior copy intact.
            if paths.archive.try_exists()? {
                let identity = Identity::read(&paths.archive, false)?;
                let owned = recovery.archive_identity.or_else(|| {
                    (phase == "publish")
                        .then_some(recovery.partial_identity)
                        .flatten()
                });
                anyhow::ensure!(
                    Some(identity) == owned,
                    "cleanup archive publication requires recovery"
                );
                recovery.archive_identity = Some(identity);
            }
            paths.prepare_archive_parent()?;
            recovery.partial_identity = None;
            self.save_cleanup_progress(token, &mut saved, "capture", &recovery)?;
            if !continue_work() {
                return Ok(Progress::More);
            }
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&paths.partial)?;
            recovery.partial_identity = Some(Identity::read(&paths.partial, false)?);
            self.save_cleanup_progress(token, &mut saved, "capture", &recovery)?;
            let mut capture = ArchiveCapture::new(source.clone(), file);
            let file = loop {
                if !continue_work() {
                    return Ok(Progress::More);
                }
                if let Poll::Ready(result) = capture.advance() {
                    break result?;
                }
            };
            drop(capture);
            file.sync_all()?;
            drop(file);
            self.save_cleanup_progress(token, &mut saved, "verify", &recovery)?;
            if !verify(&paths.partial, &continue_work)? {
                return Ok(Progress::More);
            }
            self.save_cleanup_progress(token, &mut saved, "publish", &recovery)?;
            if !self.cleanup_scope(token, root, &source, &continue_work)? {
                return Ok(Progress::More);
            }
            anyhow::ensure!(
                Identity::read(&source, true)? == recovery.source_identity
                    && Some(Identity::read(&paths.partial, false)?) == recovery.partial_identity,
                "cleanup capture identity changed"
            );
            if paths.archive.try_exists()? {
                anyhow::ensure!(
                    Some(Identity::read(&paths.archive, false)?) == recovery.archive_identity,
                    "cleanup previous archive identity changed"
                );
            }
            paths.publish_verified()?;
            recovery.archive_identity = Some(Identity::read(&paths.archive, false)?);
            self.save_cleanup_progress(token, &mut saved, "preserved", &recovery)?;
        }
        self.task_store
            .park_workspace_cleanup(token, crate::model::unix_time())?;
        // This is preservation, not permission to retire the live source. Keep
        // the intent pending until the full qualified retirement executor lands.
        Ok(Progress::Complete)
    }

    fn save_cleanup_progress(
        &self,
        token: CleanupToken,
        saved: &mut Option<CleanupRun>,
        phase: &str,
        recovery: &Recovery,
    ) -> anyhow::Result<()> {
        let next = CleanupRun {
            phase: phase.to_owned(),
            payload: serde_json::to_string(recovery)?,
        };
        anyhow::ensure!(
            self.task_store.save_workspace_cleanup_run(
                token,
                saved.as_ref().map(|run| run.phase.as_str()),
                &next
            )?,
            "cleanup authority changed before recovery commit"
        );
        *saved = Some(next);
        Ok(())
    }
}

fn verify(path: &Path, continue_work: &impl Fn() -> bool) -> anyhow::Result<bool> {
    let mut verification = ArchiveVerification::new(fs::File::open(path)?);
    loop {
        if !continue_work() {
            return Ok(false);
        }
        if let Poll::Ready(result) = verification.advance() {
            result?;
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests;
