//! Daemon-owned verified plan delivery. The exact approved revision and the
//! withheld briefs reach provider input together; the adapter's turn boundary
//! is the acceptance evidence. This withholds briefs, not native tools.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use waku_protocol::boss::{
    ApprovedPlanRevision, PlanAssignmentKind, PlanContractError, PlanFailureKind, PlanReadiness,
    PlanReadinessPhase as Phase,
};

// Match the existing 256 KiB Boss approval capture limit. Supported adapters
// stream the complete execution input; this is not a process argument bound.
const COMPLETE_INPUT_BYTES: usize = 256 * 1024;
const SECTION_BYTES: usize = 8 * 1024;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    assignment: Uuid,
    execution_project: Uuid,
    execution_workspace: PathBuf,
    grants: Value,
    attempt: Uuid,
    runtime_generation: Uuid,
    revision: ApprovedPlanRevision,
    artifact: PathBuf,
    section_digests: Vec<String>,
    delivery_mode: String,
    phase: Phase,
    failure: Option<String>,
    acknowledged: bool,
    #[serde(default)]
    delivered: bool,
}

impl Receipt {
    fn blocker_instruction(&self) -> String {
        format!(
            "If required-plan access or CLI blocker reporting fails, report through this owned runtime by emitting a standalone line: PLAN-BLOCKER {} {} runtimeAuthentication (or runtimeUnavailable, artifactInaccessible, integrity, accessDenied). Never include secrets or private paths. This controller records and retries supervisor delivery independently of the CLI bearer.",
            self.attempt, self.runtime_generation
        )
    }

    fn reported_failure(&self, response: &str) -> Option<anyhow::Error> {
        let prefix = format!("PLAN-BLOCKER {} {} ", self.attempt, self.runtime_generation);
        response
            .lines()
            .find_map(|line| match line.trim().strip_prefix(&prefix)? {
                "runtimeAuthentication" => Some(anyhow!(
                    "plan runtime authentication failed: employee report"
                )),
                "runtimeUnavailable" => Some(anyhow!("plan runtime unavailable: employee report")),
                "artifactInaccessible" => {
                    Some(anyhow!("plan artifact inaccessible: employee report"))
                }
                "integrity" => Some(anyhow!("plan integrity failure: employee report")),
                "accessDenied" => Some(anyhow!("plan access denied: employee report")),
                _ => None,
            })
    }

    fn readiness(&self) -> PlanReadiness {
        PlanReadiness {
            binding: waku_protocol::boss::PlanBinding {
                owner_id: self.revision.review.owner_id,
                plan_id: self.revision.review.plan_id,
                revision_id: Some(self.revision.id),
                content_digest: Some(self.revision.review.content_digest.clone()),
            },
            attempt: self.attempt,
            runtime_generation: self.runtime_generation,
            phase: self.phase,
            delivered: self.delivered,
            acknowledged: self.acknowledged,
            failure: self
                .failure
                .as_ref()
                .map(|error| classify_plan_failure(&anyhow!(error.clone()))),
        }
    }
}

/// Publish body-free diagnostics; raw I/O errors may contain private host paths.
pub(super) fn classify_plan_failure(error: &anyhow::Error) -> PlanContractError {
    if let Some(contract) = error.downcast_ref::<PlanContractError>() {
        return contract.clone();
    }
    let text = format!("{error:#}");
    let io_kind = error.chain().find_map(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(std::io::Error::kind)
    });
    let (kind, detail) = if text.contains("authentication") {
        (
            PlanFailureKind::RuntimeAuthentication,
            "Implementation blocked: runtime authentication failed; plan location and permission were not evaluated. Recover the runtime, then retry this binding.",
        )
    } else if text.contains("access denied")
        || io_kind == Some(std::io::ErrorKind::PermissionDenied)
    {
        (
            PlanFailureKind::AccessDenied,
            "Implementation blocked: required material access denied after authentication. Restore legitimate access or explicitly share the material, then retry.",
        )
    } else if io_kind == Some(std::io::ErrorKind::NotFound) {
        (
            PlanFailureKind::MissingPlan,
            "Implementation blocked: the exact approved artifact is missing. Restore the approved copy or explicitly correct the binding, then retry.",
        )
    } else if text.contains("unsupported") {
        if text.contains("exceeds the qualified complete-input delivery bound")
            || text.contains("exceeds the qualified inline delivery bound")
        {
            (
                PlanFailureKind::DeliveryUnsupported,
                "Implementation blocked: the complete approved plan exceeds the qualified delivery bound. Reduce the approved content or split the assignment, then retry.",
            )
        } else {
            (
                PlanFailureKind::DeliveryUnsupported,
                "Implementation blocked: this runtime cannot accept the complete approved plan. Choose a supported runtime, then retry.",
            )
        }
    } else if text.contains("acknowledgement") {
        // Historical acknowledgement-contract failure; new deliveries never
        // produce it. Keep the evidence readable on old receipts.
        (
            PlanFailureKind::Acknowledgement,
            "Implementation blocked: a complete matching revision acknowledgement was not received. Retry; the implementation brief is retained.",
        )
    } else if text.contains("integrity") || text.contains("differs") {
        (
            PlanFailureKind::Integrity,
            "Implementation blocked: the approved revision could not be verified. Restore the approved copy, then retry.",
        )
    } else if text.contains("authority changed") || text.contains("identity changed") {
        (
            PlanFailureKind::BindingChanged,
            "Implementation blocked: assignment authority changed. Resolve the binding or grants, then retry.",
        )
    } else if text.contains("runtime") {
        (
            PlanFailureKind::RuntimeUnavailable,
            "Implementation blocked: plan delivery could not be confirmed. Recover the runtime, then retry.",
        )
    } else {
        (
            PlanFailureKind::ArtifactInaccessible,
            "Implementation blocked: the complete approved content could not be delivered or recorded. Repair delivery, then retry the same binding.",
        )
    };
    PlanContractError {
        kind,
        detail: detail.into(),
    }
}

#[derive(Deserialize)]
struct StoredDelivery {
    receipt: Receipt,
    withheld: Vec<(String, Vec<crate::model::MessageAttachment>)>,
}

struct Review {
    record_path: PathBuf,
    receipt: Receipt,
    pending: Vec<(String, Vec<crate::model::MessageAttachment>)>,
    /// How many `pending` entries the in-flight complete input carries. The
    /// input was submitted through the adapter but acceptance is unconfirmed
    /// while this is nonzero — retry must not resend those briefs.
    submitted: usize,
    response: String,
}

type AuthorizedDispatch = Box<
    dyn Fn(&DriverHandle, String, Vec<crate::model::MessageAttachment>) -> anyhow::Result<()>
        + Send
        + Sync,
>;

/// Owns the exact artifact and all outbound briefs for one runtime generation.
/// The validation closure checks current principal, binding, grants and project.
pub(super) struct PlanGate {
    review: Mutex<Review>,
    validate: Box<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    dispatch: AuthorizedDispatch,
    publish: Box<dyn Fn(&Receipt, bool) -> anyhow::Result<()> + Send + Sync>,
    driver: DriverHandle,
    events: EventSink,
    lifecycle: Arc<ReentrantMutex<std::cell::Cell<Option<Uuid>>>>,
    runtime_id: Uuid,
}

impl Review {
    fn save(&self) -> anyhow::Result<()> {
        // The task-owned recovery record includes withheld briefs. It is never
        // telemetry and never exposes the plan owner's files root.
        let path = &self.record_path;
        let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
        let value = serde_json::json!({"receipt": self.receipt, "withheld": self.pending});
        private_write(&temp, &serde_json::to_vec(&value)?)?;
        fs::rename(temp, path).context("could not persist plan delivery state")
    }

    /// The one verified delivery boundary: the exact approved bytes and the
    /// withheld briefs reach provider input together. A locator or hash alone
    /// is never delivery, and the immutable artifact stays the exact-version
    /// reread surface after compaction.
    fn execution_input(&self, brief: &str) -> String {
        let revision = &self.receipt.revision;
        format!(
            "{}\nApproved plan delivery — the complete approved revision is the authoritative specification for this assignment. Exact-version reread artifact: {}. Execution workspace: {}. Work only in this current assigned workspace; earlier workspace locators are historical.\nOwner: {}; plan: {}; approved revision: {}; approval actor: human; approved at: {}; SHA-256: {}; bytes: {}.\nAssignment: {}; attempt: {}; runtime generation: {}.\n\nComplete approved text (authoritative; identical to the bound artifact):\n{}\n\nImplementation assignment:\n{}",
            self.receipt.blocker_instruction(),
            self.receipt.artifact.display(),
            self.receipt.execution_workspace.display(),
            revision.review.owner_id,
            revision.review.plan_id,
            revision.id,
            revision.approved_at,
            revision.review.content_digest,
            revision.content.len(),
            self.receipt.assignment,
            self.receipt.attempt,
            self.receipt.runtime_generation,
            revision.content,
            brief
        )
    }

    /// A follow-up inside an already-delivered attempt keeps the reread
    /// surface addressable without resending the approved bytes.
    fn followup_input(&self, prompt: &str) -> String {
        format!(
            "{}\nApproved plan reread artifact: {} (revision {}, SHA-256 {}).\n\n{}",
            self.receipt.blocker_instruction(),
            self.receipt.artifact.display(),
            self.receipt.revision.id,
            self.receipt.revision.review.content_digest,
            prompt
        )
    }

    fn check_artifact(&self) -> anyhow::Result<()> {
        // Deliver every capturable approved revision in full. Keep a bounded
        // artifact read and reject oversized/corrupt persisted revisions; no
        // summary, truncation or omitted-section fallback exists.
        anyhow::ensure!(
            self.receipt.revision.content.len() <= COMPLETE_INPUT_BYTES,
            "plan delivery unsupported: the complete approved plan exceeds the qualified complete-input delivery bound"
        );
        for ancestor in self.receipt.artifact.ancestors() {
            anyhow::ensure!(
                !fs::symlink_metadata(ancestor)?.file_type().is_symlink(),
                "plan delivery invalid location: artifact ancestor was substituted"
            );
        }
        let metadata = fs::symlink_metadata(&self.receipt.artifact)
            .context("plan delivery missing: restore this attempt's exact artifact")?;
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "plan integrity failure: artifact is not a regular file"
        );
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        options
            .open(&self.receipt.artifact)?
            .take((COMPLETE_INPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes == self.receipt.revision.content.as_bytes(),
            "plan integrity failure: artifact differs from the bound approved revision"
        );
        Ok(())
    }
}

fn private_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

impl PlanGate {
    fn check(&self, review: &Review) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.lifecycle.lock().get() == Some(self.runtime_id),
            "plan runtime unavailable: runtime generation changed"
        );
        (self.validate)()?;
        review.check_artifact()
    }

    fn save(&self, review: &Review) -> anyhow::Result<()> {
        review.save()?;
        (self.publish)(&review.receipt, false)
    }

    fn block(&self, review: &mut Review, error: anyhow::Error) {
        review.receipt.phase = Phase::Blocked;
        review.receipt.failure = Some(format!("{error:#}"));
        self.driver.cancel();
        let persisted = self.save(review);
        let detail = match persisted {
            Ok(()) => format!(
                "required-plan execution withheld: {error:#}; restart this attempt after repairing the cause"
            ),
            Err(save) => format!(
                "required-plan execution withheld: {error:#}; delivery state persistence failed: {save:#}"
            ),
        };
        let _ = self.events.send(
            event_to_wire(DriverEvent::Error(detail))
                .unwrap_or_else(|_| WireDriverEvent::new("error", Value::Null)),
        );
    }

    /// The adapter contract reports input acceptance as a real turn boundary:
    /// `TurnStarted` (or a settling turn) for the submitted complete input.
    /// `Ready` stays accepted so receipts written by the acknowledgement
    /// contract keep their delivered meaning on recovery.
    fn delivered_phase(phase: Phase) -> bool {
        matches!(phase, Phase::Delivered | Phase::Ready)
    }

    /// Submit the withheld briefs as the single complete execution input, or
    /// as ordinary follow-ups once this attempt's input is already delivered.
    /// Returns early when a submission is in flight — its acceptance lands
    /// through `observe`, and late briefs drain as follow-ups at that point.
    fn drive(&self, review: &mut Review) {
        if review.receipt.phase == Phase::Blocked || review.submitted != 0 {
            return;
        }
        if let Err(error) = self.check(review) {
            self.block(review, error);
            return;
        }
        if Self::delivered_phase(review.receipt.phase) {
            let pending = review.pending.clone();
            for (prompt, attachments) in pending {
                let sent = (self.dispatch)(
                    &self.driver,
                    review.followup_input(&prompt),
                    attachments,
                );
                if let Err(error) = sent {
                    self.block(review, error);
                    return;
                }
            }
            review.pending.clear();
            if let Err(error) = self.save(review) {
                self.block(review, error);
            }
            return;
        }
        let brief = review
            .pending
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let attachments = review
            .pending
            .iter()
            .flat_map(|(_, files)| files.clone())
            .collect();
        let input = review.execution_input(&brief);
        if let Err(error) = (self.dispatch)(&self.driver, input, attachments) {
            // Acceptance is uncertain — the provider may hold the input.
            // Hold the withheld briefs; retry excludes overlap through a
            // fresh attempt on a runtime that cannot keep executing.
            self.block(
                review,
                anyhow!("plan delivery unconfirmed: execution input submission failed through the runtime: {error:#}"),
            );
            return;
        }
        review.submitted = review.pending.len();
    }

    /// Acceptance confirmed for the submitted complete input: mark delivery
    /// durably, then drain later-arriving briefs as ordinary follow-ups.
    fn confirm(&self, review: &mut Review) {
        // Revalidate authority at the execution boundary, not just dispatch.
        if let Err(error) = self.check(review) {
            self.block(review, error);
            return;
        }
        review.receipt.delivered = true;
        review.receipt.phase = Phase::Delivered;
        review.pending.drain(..review.submitted);
        review.submitted = 0;
        if let Err(error) = self.save(review) {
            self.block(review, error);
            return;
        }
        self.drive(review);
    }

    fn submit(&self, prompt: String, attachments: Vec<crate::model::MessageAttachment>) {
        let _owner = self.lifecycle.lock();
        let mut review = self.review.lock();
        // Persist each withheld command before checking authority, so failures
        // retain the brief and attachments rather than consuming user work.
        review.pending.push((prompt, attachments));
        if let Err(error) = self.save(&review) {
            self.block(&mut review, error);
            return;
        }
        if review.receipt.phase == Phase::Blocked {
            return;
        }
        self.drive(&mut review);
    }

    /// A recovered attempt keeps withheld briefs durable; release them through
    /// the same verified boundary without waiting on another prompt.
    pub(super) fn resume_pending(&self) {
        let _owner = self.lifecycle.lock();
        let mut review = self.review.lock();
        if review.pending.is_empty() {
            return;
        }
        self.drive(&mut review);
    }

    /// An intentional interrupt mid-submission leaves acceptance uncertain —
    /// hold rather than risk an overlapping redispatch of the same brief.
    pub(super) fn interrupt_delivery(&self) {
        let mut review = self.review.lock();
        if review.submitted != 0 && !Self::delivered_phase(review.receipt.phase) {
            self.block(
                &mut review,
                anyhow!(
                    "plan delivery unconfirmed: interrupted while acceptance was uncertain; restart this attempt's runtime"
                ),
            );
        }
    }

    #[cfg(test)]
    pub(super) fn delivered(&self) -> bool {
        Self::delivered_phase(self.review.lock().receipt.phase)
    }

    fn authorized_control(&self, action: impl FnOnce(&DriverHandle)) {
        let _owner = self.lifecycle.lock();
        let mut review = self.review.lock();
        if !Self::delivered_phase(review.receipt.phase) {
            return;
        }
        if let Err(error) = self.check(&review) {
            self.block(&mut review, error);
            return;
        }
        action(&self.driver);
    }

    /// True consumes an event the delivery contract owns — a settle from a
    /// blocked attempt's cancelled turn must never expire the employee or
    /// drain follow-ups as though implementation had completed.
    pub(super) fn observe(&self, event: &DriverEvent) -> bool {
        let mut review = self.review.lock();
        if let DriverEvent::TextDelta(text) = event {
            // Reports can be long; retain a bounded tail so a trailing
            // control notice stays observable in every phase.
            review.response.push_str(text);
            if review.response.len() > 64 * 1024 {
                let mut cut = review.response.len() - 64 * 1024;
                while !review.response.is_char_boundary(cut) {
                    cut += 1;
                }
                review.response.drain(..cut);
            }
            if review.response.ends_with('\n')
                && let Some(error) = review.receipt.reported_failure(&review.response)
            {
                self.block(&mut review, error);
            }
        }
        if matches!(event, DriverEvent::TurnFinished { .. } | DriverEvent::TurnStarted)
            && let Some(error) = review.receipt.reported_failure(&review.response)
        {
            self.block(&mut review, error);
        }
        if Self::delivered_phase(review.receipt.phase) {
            if matches!(event, DriverEvent::ProcessExited) {
                self.block(
                    &mut review,
                    anyhow!("plan runtime unavailable: owned runtime exited"),
                );
            }
            if matches!(
                event,
                DriverEvent::TurnFinished { .. } | DriverEvent::TurnStarted
            ) {
                review.response.clear();
            }
            return false;
        }
        if review.receipt.phase == Phase::Blocked {
            return matches!(event, DriverEvent::TurnFinished { .. });
        }
        match event {
            DriverEvent::Error(_)
            | DriverEvent::LocalizedError { .. }
            | DriverEvent::ProcessExited => {
                self.block(
                    &mut review,
                    anyhow!(
                        "plan delivery unconfirmed: runtime transport failed before acceptance"
                    ),
                );
            }
            // The adapter began the submitted input's turn — its contract
            // establishes complete input acceptance for this attempt.
            DriverEvent::TurnStarted if review.submitted != 0 => {
                self.confirm(&mut review);
            }
            DriverEvent::TurnFinished { success, .. } if review.submitted != 0 => {
                if *success {
                    // A settled turn on the submitted input is acceptance
                    // evidence even when the adapter skips TurnStarted.
                    self.confirm(&mut review);
                } else {
                    self.block(
                        &mut review,
                        anyhow!(
                            "plan delivery unconfirmed: the submitted input's turn failed before acceptance could be verified"
                        ),
                    );
                }
            }
            _ => {}
        }
        false
    }
}

impl WakuBackend {
    /// Generated context follows task retention, never runtime idle eviction.
    pub(super) fn cleanup_plan_delivery(&self, removed: &[Uuid]) {
        let Some(root) = self.task_store.path().parent() else {
            return;
        };
        let Ok(root) = dunce::canonicalize(root) else {
            return;
        };
        for id in removed {
            for directory in [
                root.join("plan-delivery").join(id.to_string()),
                root.join("agent")
                    .join(id.to_string())
                    .join("approved-plans"),
            ] {
                if directory
                    .ancestors()
                    .take_while(|path| *path != root)
                    .any(|ancestor| {
                        fs::symlink_metadata(ancestor)
                            .is_ok_and(|meta| meta.file_type().is_symlink())
                    })
                {
                    continue;
                }
                if let Err(error) = fs::remove_dir_all(&directory)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    eprintln!(
                        "could not remove generated approved-plan delivery for {id}: {error}"
                    );
                }
            }
        }
    }

    pub(super) fn sweep_retired_plan_delivery(&self) {
        let Some(root) = self.task_store.path().parent() else {
            return;
        };
        let directory = root.join("plan-delivery");
        if fs::symlink_metadata(&directory).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let retained: HashSet<_> = self
            .task_state
            .lock()
            .sessions
            .iter()
            .map(|session| session.id)
            .collect();
        let removed: Vec<_> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse::<Uuid>().ok())
            .filter(|id| !retained.contains(id))
            .collect();
        self.cleanup_plan_delivery(&removed);
    }

    /// A newly bound live employee cannot keep an ungated driver. Grant/item
    /// changes invalidate the old generation rather than retagging its receipt.
    pub(super) fn retire_plan_runtime(
        &self,
        session_id: Uuid,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let _owner = lifecycle.lock();
        let gate = self
            .sessions
            .lock()
            .get(&session_id)
            .and_then(|entry| entry.plan_gate.clone());
        if let Some(gate) = gate {
            let mut review = gate.review.lock();
            // An already-blocked receipt keeps its original failure — the
            // recorded cause, not the retirement, is the actionable blocker.
            if review.receipt.phase != Phase::Blocked {
                gate.block(
                    &mut review,
                    anyhow!(
                        "plan authority changed: restart with fresh verified delivery of the bound revision"
                    ),
                );
            }
        }
        if let Some(entry) = self.remove_runtime(session_id, None, false) {
            entry.driver.cancel();
            events
                .for_session(session_id, entry.runtime_id)
                .notify_runtime_ended();
            events
                .for_session(session_id, entry.runtime_id)
                .end_session_runtime();
            drop_detached(Some(entry));
            record_daemon_interrupt(
                &self.task_state,
                &self.task_store,
                session_id,
                Some(
                    "Required-plan authority changed; existing work is retained. The next attempt redelivers the complete approved revision.",
                ),
            )?;
        }
        Ok(())
    }

    pub(super) fn prepare_plan_delivery(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        provider: ProviderKind,
        options: &DriverStartOptions,
        snapshot: Option<&waku_protocol::boss::BossEmployee>,
    ) -> anyhow::Result<Option<PreparedPlan>> {
        let Some(employee) = snapshot.cloned() else {
            return Ok(None);
        };
        self.boss.validate_plan_binding(&employee)?;
        if employee.assignment_kind != PlanAssignmentKind::Implementation
            || employee.plan_binding.is_none()
        {
            return Ok(None);
        }
        anyhow::ensure!(
            matches!(provider, ProviderKind::Codex | ProviderKind::Devin),
            "plan delivery unsupported: this adapter has no verified complete-input delivery contract"
        );
        let environment = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.environment())
            .ok_or_else(|| anyhow!("plan runtime unavailable: session missing"))?;
        anyhow::ensure!(
            !environment.is_sandbox() && !environment.is_cloud(),
            "plan delivery unsupported: guest/cloud runtime lacks readable artifact and scoped control transport"
        );
        let launch = options.agent.as_ref().ok_or_else(|| {
            anyhow!("plan runtime authentication unavailable: scoped agent surface missing")
        })?;
        anyhow::ensure!(
            launch.task_id == session_id && self.agent.resolve(&launch.token) == Some(session_id),
            "plan runtime authentication failed: scoped principal mismatch"
        );
        let binding = employee
            .plan_binding
            .clone()
            .ok_or_else(|| anyhow!("required plan binding missing"))?;
        let revision = self.boss.approved_plan_revision(&binding)?;
        let project = {
            let state = self.task_state.lock();
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("plan runtime session missing"))?;
            state
                .projects
                .iter()
                .find(|project| project.id == session.project_id)
                .cloned()
                .ok_or_else(|| anyhow!("plan execution project missing"))?
        };
        if let Some(ticket) = &employee.ticket {
            anyhow::ensure!(
                dunce::canonicalize(&ticket.project)? == dunce::canonicalize(&project.path)?,
                "plan execution project mismatch"
            );
        }
        let cwd = dunce::canonicalize(&options.cwd)?;
        let execution_project = project.id;
        let execution_workspace = cwd.clone();
        let token = launch.token.clone();
        let permissions = serde_json::to_value(&employee.permissions)?;
        let grant_snapshot = permissions.clone();
        let assignment = serde_json::to_value(&employee.assignment)?;
        let dispatch_boss = self.boss.clone();
        let dispatch_employee = employee.clone();
        let dispatch: AuthorizedDispatch = Box::new(move |driver, prompt, attachments| {
            dispatch_boss.with_validated_plan_dispatch(&dispatch_employee, || {
                driver.prompt_with_attachments(prompt, attachments)
            })?;
            Ok(())
        });
        let boss = self.boss.clone();
        let agent = self.agent.clone();
        let state = self.task_state.clone();
        let shutting_down = self.runtime_lifecycles.clone();
        let validate = Box::new(move || {
            anyhow::ensure!(
                !shutting_down
                    .shutting_down
                    .load(std::sync::atomic::Ordering::Acquire),
                "plan runtime unavailable: daemon stopping"
            );
            anyhow::ensure!(
                agent.resolve(&token) == Some(session_id),
                "plan runtime authentication failed: credential revoked"
            );
            boss.require_active(session_id)?;
            let current = boss
                .employee(session_id)
                .ok_or_else(|| anyhow!("plan principal missing"))?;
            boss.validate_plan_binding(&current)?;
            anyhow::ensure!(
                current.plan_binding.as_ref() == Some(&binding)
                    && current.assignment_kind == PlanAssignmentKind::Implementation
                    && current.supervisor_id == employee.supervisor_id
                    && current.item_id == employee.item_id
                    && serde_json::to_value(&current.permissions)? == permissions
                    && serde_json::to_value(&current.assignment)? == assignment
                    && !current.workspace_transition,
                "plan authority changed: binding, principal, assignment or grants differ"
            );
            let state = state.lock();
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("plan runtime session missing"))?;
            let current_project = state
                .projects
                .iter()
                .find(|entry| entry.id == session.project_id)
                .ok_or_else(|| anyhow!("plan execution project missing"))?;
            let current_cwd = session.workspace.path().unwrap_or(&current_project.path);
            anyhow::ensure!(
                current_project.id == project.id
                    && current_project.path == project.path
                    && dunce::canonicalize(current_cwd)? == cwd
                    && session.provider == provider
                    && !session.environment().is_sandbox()
                    && !session.environment().is_cloud(),
                "plan runtime identity changed: execution project, workspace or provider differ"
            );
            Ok(())
        });
        validate()?;
        // Resolve trusted host aliases (macOS /var -> /private/var) once.
        // Only generated descendants must be free of symlink substitutions.
        let storage_root = dunce::canonicalize(
            self.task_store
                .path()
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )?;
        let directory = storage_root
            .join("plan-delivery")
            .join(session_id.to_string());
        fs::create_dir_all(&directory)?;
        for ancestor in directory
            .ancestors()
            .take_while(|path| *path != storage_root)
        {
            anyhow::ensure!(
                !fs::symlink_metadata(ancestor)?.file_type().is_symlink(),
                "plan delivery invalid location: symlink ancestor"
            );
        }
        let previous = directory.join("current-attempt");
        let mut pending = Vec::new();
        if previous.exists() {
            let previous_id: Uuid = fs::read_to_string(&previous)?
                .trim()
                .parse()
                .context("plan recovery failed: invalid attempt identity")?;
            let record = directory
                .join(previous_id.to_string())
                .join("delivery.json");
            use std::io::Read;
            let mut bytes = Vec::new();
            fs::File::open(record)?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() <= 4 * 1024 * 1024,
                "plan recovery failed: delivery record exceeds supported size"
            );
            let stored: StoredDelivery = serde_json::from_slice(&bytes).context(
                "plan recovery failed: delivery record is corrupt; preserve the prior attempt",
            )?;
            if stored.receipt.assignment == session_id && stored.receipt.phase == Phase::Blocked {
                let readiness = stored.receipt.readiness();
                if let Some(error) = readiness.failure {
                    self.boss.retain_plan_blocker(
                        session_id,
                        waku_protocol::boss::PlanBlockerNotice {
                            id: readiness.attempt,
                            attempt: readiness.attempt,
                            runtime_generation: readiness.runtime_generation,
                            binding: readiness.binding,
                            error,
                            recorded_at: crate::model::unix_time(),
                            supervisor_notified_at: None,
                        },
                    )?;
                }
            }
            // Withheld briefs recover only while the prior attempt never
            // reached verified delivery — Ready and Delivered both mark the
            // input accepted, so their leftovers must not redispatch.
            if stored.receipt.assignment == session_id
                && stored.receipt.execution_project == execution_project
                && stored.receipt.revision.id == revision.id
                && stored.receipt.revision.review == revision.review
                && stored.receipt.grants == grant_snapshot
                && !PlanGate::delivered_phase(stored.receipt.phase)
            {
                pending = stored.withheld;
            }
        }
        let attempt = Uuid::new_v4();
        let record_directory = directory.join(attempt.to_string());
        fs::create_dir(&record_directory)?;
        let artifact_directory = storage_root
            .join("agent")
            .join(session_id.to_string())
            .join("approved-plans")
            .join(attempt.to_string());
        fs::create_dir_all(&artifact_directory)?;
        for ancestor in artifact_directory
            .ancestors()
            .take_while(|path| *path != storage_root)
        {
            anyhow::ensure!(
                !fs::symlink_metadata(ancestor)?.file_type().is_symlink(),
                "plan delivery invalid location: symlink ancestor"
            );
        }
        let artifact = artifact_directory.join("approved-plan.md");
        private_write(&artifact, revision.content.as_bytes())
            .context("plan artifact delivery failed")?;
        let mut permissions = fs::metadata(&artifact)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&artifact, permissions)?;
        // The complete input supports the entire approval capture range.
        // Reject anything beyond it rather than substituting a locator.
        anyhow::ensure!(
            revision.content.len() <= COMPLETE_INPUT_BYTES,
            "plan delivery unsupported: the complete approved plan exceeds the qualified complete-input delivery bound"
        );
        let delivery_mode = "complete-input-inline".to_owned();
        let mut section_digests: Vec<String> = revision
            .content
            .as_bytes()
            .chunks(SECTION_BYTES)
            .map(digest)
            .collect();
        if section_digests.is_empty() {
            section_digests.push(digest(&[]));
        }
        let review = Review {
            record_path: record_directory.join("delivery.json"),
            receipt: Receipt {
                assignment: session_id,
                execution_project,
                execution_workspace,
                grants: grant_snapshot,
                attempt,
                runtime_generation: runtime_id,
                revision,
                artifact,
                section_digests,
                delivery_mode,
                phase: Phase::Preparing,
                failure: None,
                acknowledged: false,
                delivered: false,
            },
            pending,
            submitted: 0,
            response: String::new(),
        };
        review.check_artifact()?;
        review.save()?;
        let projection_boss = self.boss.clone();
        let publish: Box<dyn Fn(&Receipt, bool) -> anyhow::Result<()> + Send + Sync> =
            Box::new(move |receipt, start| {
                projection_boss.record_plan_readiness(session_id, receipt.readiness(), start)
            });
        publish(&review.receipt, true)?;
        let pointer = previous.with_extension(format!("{}.tmp", Uuid::new_v4()));
        private_write(&pointer, attempt.to_string().as_bytes())?;
        fs::rename(pointer, previous)
            .context("plan recovery failed: could not retain current attempt")?;
        Ok(Some(PreparedPlan {
            review,
            validate,
            dispatch,
            publish,
        }))
    }
}

pub(super) struct PreparedPlan {
    review: Review,
    validate: Box<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    dispatch: AuthorizedDispatch,
    publish: Box<dyn Fn(&Receipt, bool) -> anyhow::Result<()> + Send + Sync>,
}

impl PreparedPlan {
    pub(super) fn attach(
        self,
        driver: DriverHandle,
        events: EventSink,
        lifecycle: Arc<ReentrantMutex<std::cell::Cell<Option<Uuid>>>>,
        runtime_id: Uuid,
    ) -> Arc<PlanGate> {
        Arc::new(PlanGate {
            review: Mutex::new(self.review),
            validate: self.validate,
            dispatch: self.dispatch,
            publish: self.publish,
            driver,
            events,
            lifecycle,
            runtime_id,
        })
    }
}

impl driver::DriverControl for PlanGate {
    fn prompt(&self, prompt: String) {
        self.submit(prompt, Vec::new());
    }
    fn prompt_with_attachments(
        &self,
        prompt: String,
        attachments: Vec<crate::model::MessageAttachment>,
    ) {
        self.submit(prompt, attachments);
    }
    // Queue follow-ups throughout required-plan execution. Hidden context
    // steers must not bypass the verified delivery boundary.
    fn supports_steer(&self) -> bool {
        false
    }
    fn steer(&self, prompt: String) {
        let event = DriverEvent::SteerRejected {
            message: prompt, reason: "Required-plan assignments queue follow-ups; steering is unavailable during verified plan delivery.".into(),
            reason_i18n: None, hidden: false,
        };
        if let Ok(wire) = event_to_wire(event) {
            let _ = self.events.send(wire);
        }
    }
    fn agent_surface_delivery(&self) -> driver::AgentSurfaceDelivery {
        self.driver.agent_surface_delivery()
    }
    fn cancel(&self) {
        self.driver.cancel();
    }
    fn begin_shutdown(&self) {
        self.driver.begin_shutdown();
    }
    fn respond(&self, request_id: String, option_id: String) {
        self.driver.respond(request_id, option_id);
    }
    fn computer_use_available(&self) -> bool {
        self.driver.computer_use_available()
    }
    fn cancel_computer_use(&self) {
        self.driver.cancel_computer_use();
    }
    fn refresh_background_work(&self) {
        self.driver.refresh_background_work();
    }
    fn stop_background_work(&self, key: crate::model::BackgroundWorkKey, control_id: String) {
        self.driver.stop_background_work(key, control_id);
    }
    fn respond_user_input(&self, request_id: String, answers: Vec<crate::model::UserInputAnswer>) {
        self.driver.respond_user_input(request_id, answers);
    }
    fn supports_user_input_actions(&self) -> bool {
        self.driver.supports_user_input_actions()
    }
    fn clarify_user_input(&self, request_id: String, content: String) {
        self.driver.clarify_user_input(request_id, content);
    }
    fn cancel_user_input(&self, request_id: String) {
        self.driver.cancel_user_input(request_id);
    }
    fn goal(&self, operation: crate::model::GoalOperation) {
        self.authorized_control(|driver| driver.goal(operation));
    }
    fn compact(&self) {
        self.authorized_control(|driver| driver.compact());
    }
    fn run_computer_tool(&self, request: crate::computer_use::ComputerToolRequest) {
        self.authorized_control(|driver| driver.run_computer_tool(request));
    }
    fn reject_computer_tool(
        &self,
        request: crate::computer_use::ComputerToolRequest,
        reason: String,
    ) {
        self.driver.reject_computer_tool(request, reason);
    }
    fn rollback(&self, _turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        bail!("required-plan runtime must restart after rollback")
    }
    fn apply_options(&self, _options: SessionOptions) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use waku_protocol::boss::{PlanActor, PlanReview};

    #[derive(Default)]
    struct Capture(Mutex<Vec<String>>);
    impl driver::DriverControl for Capture {
        fn prompt(&self, text: String) {
            self.0.lock().push(text);
        }
        fn cancel(&self) {}
        fn respond(&self, _: String, _: String) {}
        fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
            Ok(None)
        }
    }

    struct Fixture {
        root: PathBuf,
        gate: Arc<PlanGate>,
        capture: Arc<Capture>,
        authorized: Arc<AtomicBool>,
    }
    impl Fixture {
        fn new(content: &str) -> Self {
            let root = std::env::temp_dir().join(format!("plan-delivery-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let artifact = root.join("approved-plan.md");
            private_write(&artifact, content.as_bytes()).unwrap();
            let mut perms = fs::metadata(&artifact).unwrap().permissions();
            perms.set_readonly(true);
            fs::set_permissions(&artifact, perms).unwrap();
            let runtime_id = Uuid::new_v4();
            let receipt = Receipt {
                assignment: Uuid::new_v4(),
                execution_project: Uuid::new_v4(),
                execution_workspace: root.clone(),
                grants: Value::Null,
                attempt: Uuid::new_v4(),
                runtime_generation: runtime_id,
                revision: ApprovedPlanRevision {
                    id: Uuid::new_v4(),
                    review: PlanReview {
                        owner_id: Uuid::new_v4(),
                        plan_id: Uuid::new_v4(),
                        content_digest: digest(content.as_bytes()),
                    },
                    content: content.into(),
                    actor: PlanActor::User,
                    approved_at: 1,
                },
                artifact: dunce::canonicalize(artifact).unwrap(),
                section_digests: content
                    .as_bytes()
                    .chunks(SECTION_BYTES)
                    .map(digest)
                    .collect(),
                delivery_mode: "fixture".into(),
                phase: Phase::Preparing,
                failure: None,
                acknowledged: false,
                delivered: false,
            };
            let authorized = Arc::new(AtomicBool::new(true));
            let current = authorized.clone();
            let prepared = PreparedPlan {
                review: Review {
                    record_path: root.join("delivery.json"),
                    receipt,
                    pending: Vec::new(),
                    submitted: 0,
                    response: String::new(),
                },
                publish: Box::new(|_, _| Ok(())),
                dispatch: Box::new(|driver, prompt, attachments| {
                    driver.prompt_with_attachments(prompt, attachments);
                    Ok(())
                }),
                validate: Box::new(move || {
                    anyhow::ensure!(
                        current.load(Ordering::Acquire),
                        "plan runtime authentication failed: credential revoked"
                    );
                    Ok(())
                }),
            };
            let capture = Arc::new(Capture::default());
            let gate = prepared.attach(
                DriverHandle::from_control(capture.clone()),
                EventSink::detached(),
                Arc::new(ReentrantMutex::new(std::cell::Cell::new(Some(runtime_id)))),
                runtime_id,
            );
            Self {
                root,
                gate,
                capture,
                authorized,
            }
        }
        /// The adapter contract's acceptance signal for the submitted
        /// complete input — a real turn boundary, never a returned receipt.
        fn accept(&self) {
            self.gate.observe(&DriverEvent::TurnStarted);
        }
        fn stored(&self) -> StoredDelivery {
            serde_json::from_slice(&fs::read(self.root.join("delivery.json")).unwrap()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    mod acceptance;

    #[test]
    fn plan_blocker_runtime_report_survives_cli_auth_failure_and_rejects_old_provenance() {
        let fixture = Fixture::new("approved bytes");
        fixture.gate.submit("implementation".into(), Vec::new());
        fixture.accept();
        assert!(fixture.gate.delivered());
        fixture.gate.observe(&DriverEvent::TextDelta(format!(
            "PLAN-BLOCKER {} {} runtimeAuthentication",
            Uuid::new_v4(),
            Uuid::new_v4()
        )));
        assert!(fixture.gate.delivered());
        fixture.gate.observe(&DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        });
        let (attempt, runtime) = {
            let review = fixture.gate.review.lock();
            (review.receipt.attempt, review.receipt.runtime_generation)
        };
        fixture.authorized.store(false, Ordering::Release);
        fixture.gate.observe(&DriverEvent::TextDelta(format!(
            "{}\n",
            "長".repeat(30_000)
        )));
        fixture.gate.observe(&DriverEvent::TextDelta(format!(
            "PLAN-BLOCKER {attempt} {runtime} runtimeAuthentication\n"
        )));
        assert!(!fixture.gate.delivered());
        assert!(fixture.gate.observe(&DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None
        }));
        let stored = fixture.stored();
        assert_eq!(stored.receipt.phase, Phase::Blocked);
        assert_eq!(
            stored.receipt.readiness().failure.unwrap().kind,
            PlanFailureKind::RuntimeAuthentication
        );
        fixture.gate.submit("followup".into(), Vec::new());
        assert_eq!(fixture.capture.0.lock().len(), 1);
    }

    #[test]
    fn plan_delivery_runtime_loss_cannot_keep_delivered_receipt() {
        let fixture = Fixture::new("approved bytes");
        fixture.gate.submit("implementation".into(), Vec::new());
        fixture.accept();
        assert!(fixture.gate.delivered());
        fixture.gate.observe(&DriverEvent::ProcessExited);
        assert!(!fixture.gate.delivered());
        let stored = fixture.stored();
        assert_eq!(
            stored.receipt.readiness().failure.unwrap().kind,
            PlanFailureKind::RuntimeUnavailable
        );
    }

    #[test]
    fn plan_delivery_releases_exact_approved_bytes_with_the_brief() {
        let fixture = Fixture::new("# Exact approved text\nα\n");
        fixture.gate.submit("IMPLEMENT_SENTINEL".into(), Vec::new());
        // One verified delivery boundary: the complete approved bytes and the
        // brief reach provider input together — no review turn, no locator.
        assert_eq!(fixture.capture.0.lock().len(), 1);
        let input = fixture.capture.0.lock()[0].clone();
        assert!(input.contains("# Exact approved text\nα\n"));
        assert!(input.contains("IMPLEMENT_SENTINEL"));
        assert!(!input.contains("PLAN-ACK"));
        assert_eq!(
            fs::read(fixture.root.join("approved-plan.md")).unwrap(),
            "# Exact approved text\nα\n".as_bytes()
        );
        // Submission is not acceptance: withheld work stays durable until the
        // adapter reports the input's turn.
        assert!(!fixture.gate.delivered());
        let stored = fixture.stored();
        assert_eq!(stored.receipt.phase, Phase::Preparing);
        assert_eq!(stored.withheld[0].0, "IMPLEMENT_SENTINEL");
        fixture.accept();
        assert!(fixture.gate.delivered());
        let receipt: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("delivery.json")).unwrap()).unwrap();
        assert_eq!(receipt["receipt"]["phase"], "delivered");
        assert_eq!(receipt["receipt"]["delivered"], true);
        assert_eq!(receipt["receipt"]["acknowledged"], false);
        assert!(receipt["withheld"].as_array().unwrap().is_empty());
    }

    #[test]
    fn plan_delivery_commentary_streaming_and_stray_ack_have_no_effect() {
        // Interim provider messages and even a forged PLAN-ACK line neither
        // confirm nor disturb delivery — acceptance is the adapter's turn.
        let fixture = Fixture::new("approved bytes");
        fixture.gate.submit("IMPLEMENT_SENTINEL".into(), Vec::new());
        for delta in [
            "I will read the plan and start work.",
            "\n\nPLAN-ACK 1 2 3 4 5 6 7 8\n\n",
            "Progress update.\n",
        ] {
            fixture
                .gate
                .observe(&DriverEvent::TextDelta(delta.to_owned()));
        }
        assert!(!fixture.gate.delivered());
        fixture.accept();
        assert!(fixture.gate.delivered());
        assert_eq!(fixture.capture.0.lock().len(), 1);
    }

    #[test]
    fn plan_delivery_holds_briefs_and_blocks_on_delivery_failures() {
        for fault in [
            "generation",
            "auth",
            "tamper",
            "missing",
            "persistence",
            "transport",
            "dispatch",
            "turn-failure",
        ] {
            let fixture = Fixture::new(&"section text\n".repeat(4000));
            match fault {
                "generation" | "auth" | "tamper" | "missing" => {}
                _ => fixture.gate.submit("IMPLEMENT_SENTINEL".into(), Vec::new()),
            }
            match fault {
                "generation" => fixture.gate.lifecycle.lock().set(Some(Uuid::new_v4())),
                "auth" => fixture.authorized.store(false, Ordering::Release),
                "tamper" => {
                    fs::remove_file(fixture.root.join("approved-plan.md")).unwrap();
                    fs::write(
                        fixture.root.join("approved-plan.md"),
                        "lookalike mutable content",
                    )
                    .unwrap();
                }
                "missing" => {
                    fs::remove_file(fixture.root.join("approved-plan.md")).unwrap();
                }
                "persistence" => {
                    fs::remove_file(fixture.root.join("delivery.json")).unwrap();
                    fs::create_dir(fixture.root.join("delivery.json")).unwrap();
                    fixture.accept();
                }
                "transport" => {
                    fixture
                        .gate
                        .observe(&DriverEvent::Error("connection lost".into()));
                }
                "dispatch" => {
                    // Authority revoked between submission and acceptance.
                    fixture.authorized.store(false, Ordering::Release);
                    fixture.accept();
                }
                "turn-failure" => {
                    fixture.gate.observe(&DriverEvent::TurnFinished {
                        success: false,
                        summary: None,
                        summary_i18n: None,
                    });
                }
                _ => unreachable!(),
            }
            if matches!(fault, "generation" | "auth" | "tamper" | "missing") {
                fixture.gate.submit("IMPLEMENT_SENTINEL".into(), Vec::new());
            }
            assert!(!fixture.gate.delivered(), "{fault}");
            match fault {
                // Authority/integrity faults block before any submission.
                "generation" | "auth" | "tamper" | "missing" => {
                    assert_eq!(fixture.capture.0.lock().len(), 0, "{fault}");
                }
                _ => {
                    assert_eq!(fixture.capture.0.lock().len(), 1, "{fault}");
                }
            }
            assert_eq!(
                fixture.gate.review.lock().receipt.phase,
                Phase::Blocked,
                "{fault}"
            );
            if fault != "persistence" {
                // The withheld brief survives every failure durably —
                // the persistence fault holds it in the last durable save.
                assert_eq!(
                    fixture.stored().receipt.phase,
                    Phase::Blocked,
                    "{fault}"
                );
                assert_eq!(
                    fixture.gate.review.lock().pending[0].0,
                    "IMPLEMENT_SENTINEL",
                    "{fault}"
                );
            }
            fixture.gate.submit("FOLLOWUP_SENTINEL".into(), Vec::new());
            assert!(!fixture.gate.delivered(), "{fault}");
        }
    }

    #[test]
    fn plan_delivery_revalidates_delivered_runtime_before_each_followup() {
        let fixture = Fixture::new("approved");
        fixture
            .gate
            .submit("initial implementation".into(), Vec::new());
        fixture.accept();
        assert!(fixture.gate.delivered());
        fixture.authorized.store(false, Ordering::Release);
        fixture.gate.submit("FOLLOWUP_SENTINEL".into(), Vec::new());
        assert_eq!(fixture.capture.0.lock().len(), 1);
        assert!(!fixture.gate.delivered());
        let state: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("delivery.json")).unwrap()).unwrap();
        assert_eq!(state["withheld"][0][0], "FOLLOWUP_SENTINEL");
        assert!(
            state["receipt"]["failure"]
                .as_str()
                .unwrap()
                .contains("authentication failed")
        );
    }
}
