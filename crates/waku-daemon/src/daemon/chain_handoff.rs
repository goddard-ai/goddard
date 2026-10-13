//! Chain execution seam: what a successor attempt actually receives.
//!
//! The execution core (admission, sealing, quiet routing) calls into this
//! module for the two halves the Phase 1 reducer cannot perform inside a
//! document update: real filesystem and Git checks behind
//! `HandoffVerification`, and the workspace plus brief the next summon is
//! built from. Every check is read-only — an unverifiable input leaves its
//! field unset, which fails the corresponding gate and pauses the chain.
//! Nothing here widens access or fabricates evidence.
use std::path::{Path, PathBuf};

use anyhow::{Context as _, anyhow, ensure};
use uuid::Uuid;
use waku_boss::chains::HandoffVerification;
use waku_protocol::AgentWorkspace;
use waku_protocol::boss::BossState;
use waku_protocol::chains::*;

use crate::model::{Project, SessionWorkspace};
use crate::persistence::PersistedState;

use super::WakuBackend;

/// Artifact references a chain result may carry. Only these forms resolve —
/// anything else is unreachable by construction, which pauses safely.
///
/// - `file:<rel>` — a Boss files-root document (`plans/…`, persona-pinned)
/// - `transcript:<session>` — an employee transcript; only its supervisor
///   lineage may read it, so a chain successor (a sibling) never can
/// - `memory:<bucket>` or `memory:<bucket>:<seq>` — a memory bucket grant
/// - `deliverable:<id>` — a published deliverable record plus its snapshot
/// - `commit:<sha>` — a commit object in the successor's repository
/// - `worktree:<abs>` — a path inside an adopted or own checkout
///
/// Bare references degrade predictably: absolute paths behave as `worktree:`,
/// full hex commit ids as `commit:`, and relative paths as `file:`.
enum ArtifactRef<'a> {
    File(&'a str),
    Transcript(Uuid),
    Memory(&'a str),
    Deliverable(Uuid),
    Commit(&'a str),
    CheckoutPath(&'a Path),
    Unknown,
}

fn classify(reference: &str) -> ArtifactRef<'_> {
    if let Some((scheme, value)) = reference.split_once(':')
        && !value.is_empty()
    {
        match scheme.to_ascii_lowercase().as_str() {
            "file" => return ArtifactRef::File(value),
            "transcript" => {
                return value
                    .parse()
                    .map(ArtifactRef::Transcript)
                    .unwrap_or(ArtifactRef::Unknown);
            }
            "memory" | "bucket" => return ArtifactRef::Memory(value),
            "deliverable" => {
                return value
                    .parse()
                    .map(ArtifactRef::Deliverable)
                    .unwrap_or(ArtifactRef::Unknown);
            }
            "commit" => return ArtifactRef::Commit(value),
            "worktree" => return ArtifactRef::CheckoutPath(Path::new(value)),
            _ => {}
        }
    }
    let path = Path::new(reference);
    if path.is_absolute() {
        ArtifactRef::CheckoutPath(path)
    } else if (reference.len() == 40 || reference.len() == 64)
        && reference.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        ArtifactRef::Commit(reference)
    } else {
        ArtifactRef::File(reference)
    }
}

/// Whose grants gate the references — the next step's employee, or the
/// chain's supervisor when evidence terminates at the last step.
enum HandoffSubject<'a> {
    /// The supervisor/boss reads every transcript, file, bucket, and
    /// deliverable the daemon stores; only existence needs checking.
    Supervisor,
    Successor {
        pinned_files: &'a [String],
        persona_id: Option<Uuid>,
        bucket_ids: &'a [String],
        project_path: &'a Path,
    },
}

/// Whether `relative` names an existing document under the files root —
/// lexical traversal (`..`) is rejected and a symlinked component can never
/// canonicalize back inside the root.
fn boss_file(files_root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let candidate = files_root.join(relative);
    let resolved = dunce::canonicalize(&candidate).ok()?;
    let root = dunce::canonicalize(files_root).ok()?;
    resolved.starts_with(&root).then_some(resolved)
}

/// Whether `relative` is covered by the successor's file grants — `plans/`
/// is shared, the rest must sit under a pinned file or be the persona's own
/// document, mirroring `authorize_file`'s live-employee rule.
fn file_granted(relative: &str, pinned_files: &[String], persona_id: Option<Uuid>) -> bool {
    relative == "plans"
        || relative.starts_with("plans/")
        || (!relative.starts_with("memory/")
            && (pinned_files
                .iter()
                .any(|pinned| relative == pinned || relative.starts_with(&format!("{pinned}/")))
                || persona_id.is_some_and(|id| relative == format!("personas/{id}/PERSONA.md"))))
}

fn commit_available(repo: &Path, revision: &str) -> bool {
    (revision.len() == 40 || revision.len() == 64)
        && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        && std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "--verify", "--end-of-options"])
            .arg(format!("{revision}^{{commit}}"))
            .output()
            .is_ok_and(|output| output.status.success())
}

/// Paths a successor may legitimately read: its adopted checkout and its own
/// project's registered root (which covers every worktree of that repo).
fn successor_path_root(adopt_root: Option<&Path>, project_path: &Path, candidate: &Path) -> bool {
    adopt_root.is_some_and(|root| candidate.starts_with(root))
        || dunce::canonicalize(project_path).is_ok_and(|root| candidate.starts_with(root))
}

fn transcript_exists(state: &BossState, session: Uuid) -> bool {
    state
        .employees
        .iter()
        .chain(state.retired_employees.iter())
        .any(|employee| employee.session_id == session)
}

fn deliverable_accessible(
    state: &BossState,
    id: Uuid,
    subject: &HandoffSubject<'_>,
    adopt_root: Option<&Path>,
) -> bool {
    let Some(deliverable) = state
        .deliverables
        .iter()
        .find(|deliverable| deliverable.id == id)
    else {
        return false;
    };
    let path = Path::new(&deliverable.path);
    // A copied deliverable's snapshot lives in the daemon store; a reference
    // deliverable's path is a live source that dies with its workspace.
    if deliverable.source_path.is_some() {
        return path.exists();
    }
    if !path.exists() {
        return false;
    }
    match subject {
        HandoffSubject::Supervisor => true,
        HandoffSubject::Successor { project_path, .. } => dunce::canonicalize(path)
            .is_ok_and(|resolved| successor_path_root(adopt_root, project_path, &resolved)),
    }
}

fn artifact_accessible(
    state: &BossState,
    files_root: &Path,
    known_buckets: &[String],
    reference: &str,
    subject: &HandoffSubject<'_>,
    producer_repo: &Path,
    adopt_root: Option<&Path>,
) -> bool {
    let reference = reference.trim();
    match (classify(reference), subject) {
        (ArtifactRef::File(relative), HandoffSubject::Supervisor) => {
            boss_file(files_root, relative).is_some()
        }
        (
            ArtifactRef::File(relative),
            HandoffSubject::Successor {
                pinned_files,
                persona_id,
                ..
            },
        ) => {
            file_granted(relative, pinned_files, *persona_id)
                && boss_file(files_root, relative).is_some()
        }
        (ArtifactRef::Transcript(session), HandoffSubject::Supervisor) => {
            transcript_exists(state, session)
        }
        // A chain successor is the producer's sibling — transcript grants
        // follow supervisor lineage only.
        (ArtifactRef::Transcript(_), HandoffSubject::Successor { .. }) => false,
        (ArtifactRef::Memory(value), HandoffSubject::Supervisor) => {
            let bucket = value.split(':').next().unwrap_or_default();
            known_buckets.iter().any(|known| known == bucket)
        }
        (
            ArtifactRef::Memory(value),
            HandoffSubject::Successor {
                bucket_ids,
                project_path,
                ..
            },
        ) => {
            let bucket = value.split(':').next().unwrap_or_default();
            let own = crate::boss::project_bucket_id(project_path);
            bucket == own || bucket_ids.iter().any(|granted| granted == bucket)
        }
        (ArtifactRef::Deliverable(id), _) => deliverable_accessible(state, id, subject, adopt_root),
        (ArtifactRef::Commit(revision), HandoffSubject::Supervisor) => {
            commit_available(producer_repo, revision)
        }
        (ArtifactRef::Commit(revision), HandoffSubject::Successor { project_path, .. }) => {
            let repo = adopt_root.unwrap_or(project_path);
            commit_available(repo, revision)
        }
        (ArtifactRef::CheckoutPath(path), HandoffSubject::Supervisor) => {
            dunce::canonicalize(path).is_ok()
        }
        (ArtifactRef::CheckoutPath(path), HandoffSubject::Successor { project_path, .. }) => {
            dunce::canonicalize(path)
                .is_ok_and(|resolved| successor_path_root(adopt_root, project_path, &resolved))
        }
        (ArtifactRef::Unknown, _) => false,
    }
}

/// The session still owning `workspace`, when it is an unadopted daemon
/// worktree — the exclusive-adoption precondition.
fn unadopted_owner(state: &PersistedState, workspace: &Path) -> Option<Uuid> {
    state
        .sessions
        .iter()
        .find(|session| {
            matches!(
                &session.workspace,
                SessionWorkspace::Worktree { path, adopted_by, .. }
                    if adopted_by.is_none()
                        && dunce::canonicalize(path).is_ok_and(|path| path == workspace)
            )
        })
        .map(|session| session.id)
}

fn git_revision(cwd: &Path, reference: &str) -> anyhow::Result<String> {
    crate::agent_merge::git_text(
        cwd,
        &["rev-parse", "--verify", "--end-of-options", reference],
    )
    .map(|revision| revision.trim().to_owned())
}

/// The committed-workspace gate: registered daemon worktree of the step's
/// project, clean under merge-submit rules, committed at the reported
/// revision atop the recorded source, and still owned by its released
/// producer — the exclusive-adoption precondition. Returns the canonical
/// workspace path plus the observed HEAD on success.
fn clean_committed_workspace(
    project: &Project,
    code: &ChainCodeEvidence,
    submissions_enabled: bool,
    unadopted: bool,
) -> anyhow::Result<(PathBuf, String)> {
    ensure!(
        submissions_enabled,
        "committed-workspace handoff requires project submissions enabled"
    );
    let workspace = dunce::canonicalize(Path::new(&code.workspace))
        .with_context(|| format!("workspace {} is unavailable", code.workspace))?;
    ensure!(
        crate::worktree::is_worktree_of(&project.path, &workspace),
        "{} is not a managed worktree of {}",
        workspace.display(),
        project.path.display()
    );
    ensure!(
        unadopted,
        "{} is not held by an unadopted employee session",
        workspace.display()
    );
    crate::agent_merge::no_operation(&workspace)?;
    crate::agent_merge::clean_status(&workspace, "all", "handoff worktree")?;
    let head = git_revision(&workspace, "HEAD")?;
    let committed = code
        .committed_revision
        .as_deref()
        .ok_or_else(|| anyhow!("committed revision is unreported"))?;
    ensure!(
        head == committed,
        "workspace HEAD moved past the reported unit"
    );
    ensure!(
        crate::agent_merge::is_ancestor(&workspace, &code.source_revision, &head)?,
        "the committed unit does not descend from its recorded source"
    );
    Ok((workspace, head))
}

/// The landed gate: the reported revision is reachable on the project's
/// configured QA branch. The submission id is the employee's claim — the
/// branch ancestry is what the host confirms.
fn confirmed_landing(
    project: &Project,
    configured_branch: &str,
    code: &ChainCodeEvidence,
) -> anyhow::Result<(String, String, String)> {
    let landed = code
        .landed_revision
        .as_deref()
        .ok_or_else(|| anyhow!("landed revision is unreported"))?;
    let claimed_branch = code
        .qa_branch
        .as_deref()
        .ok_or_else(|| anyhow!("QA branch is unreported"))?;
    let branch = crate::review::qa_branch_name(configured_branch);
    ensure!(
        claimed_branch == branch,
        "reported QA branch {claimed_branch:?} is not the configured {branch:?}"
    );
    let submission = code
        .submission_id
        .clone()
        .ok_or_else(|| anyhow!("submission identity is unreported"))?;
    let tip = format!("refs/heads/{branch}");
    ensure!(
        git_revision(&project.path, &tip).is_ok(),
        "QA branch {branch:?} is not checked out locally"
    );
    ensure!(
        crate::agent_merge::is_ancestor(&project.path, landed, &tip)?,
        "the reported revision is not reachable on {branch:?}"
    );
    Ok((landed.to_owned(), branch, submission))
}

/// The resolved inputs a chain dispatch feeds the next attempt's summon:
/// the workspace the verified handoff dictates plus the successor brief.
/// Recomputed on demand — a stale check can only pause, never launch on
/// dead references.
#[allow(dead_code)] // Consumed by the execution core's dispatch path.
pub(crate) struct ChainSuccessorInputs {
    /// Index of the step these inputs belong to.
    pub step: usize,
    /// Summon workspace selection after handoff resolution.
    pub workspace: AgentWorkspace,
    /// `Adopt` target for a committed-workspace handoff.
    pub adopt_worktree: Option<PathBuf>,
    /// Fresh-worktree base: the verified landed revision for `Landed`, the
    /// reconciled retry revision for a retried attempt, else the step's own.
    pub base_revision: Option<String>,
    /// The revision the successor checkout must start on when the handoff
    /// pins one — `None` for `Local` and `ArtifactsReady` handoffs. Dispatch
    /// records the observed value as the attempt's `starting_revision`.
    pub expected_starting_revision: Option<String>,
    /// The sealed handoff brief for the successor's assignment prompt —
    /// `None` for the first step, which has no predecessor.
    pub brief: Option<String>,
}

fn chain<'a>(state: &'a BossState, chain_id: Uuid) -> anyhow::Result<&'a EmployeeChain> {
    state
        .chains
        .iter()
        .find(|chain| chain.id == chain_id)
        .ok_or_else(|| anyhow!("unknown chain {chain_id}"))
}

fn bound_attempt<'a>(
    chain: &'a EmployeeChain,
    step: usize,
    attempt_id: Uuid,
    definition_version: u64,
) -> anyhow::Result<&'a ChainAttempt> {
    let attempt = chain.steps[step]
        .attempts
        .last()
        .ok_or_else(|| anyhow!("step has no attempt"))?;
    ensure!(
        attempt.id == attempt_id && attempt.definition_version == definition_version,
        "stale chain attempt"
    );
    Ok(attempt)
}

fn brief(chain: &EmployeeChain, step: usize, handoff: &ChainHandoff) -> String {
    let producer = &chain.steps[step].definition;
    let mut text = format!(
        "Chain handoff — step {} of {} complete: \"{}\".\n\n{}",
        step + 1,
        chain.steps.len(),
        producer.title,
        handoff.terminal_message
    );
    let result = &handoff.result;
    if !result.checks.is_empty() {
        text.push_str("\n\nChecks:");
        for check in &result.checks {
            let status = match check.status {
                CheckStatus::Pass => "pass",
                CheckStatus::Fail => "fail",
                CheckStatus::NotRun => "not run",
            };
            text.push_str(&format!(
                "\n- {} — {status}: {}",
                check.name, check.evidence
            ));
        }
    }
    if !result.artifacts.is_empty() {
        text.push_str("\n\nArtifacts:");
        for artifact in &result.artifacts {
            text.push_str(&format!("\n- {} — {}", artifact.name, artifact.reference));
        }
    }
    if let Some(code) = &result.code {
        text.push_str("\n\nCode:");
        text.push_str(&format!("\n- source revision {}", code.source_revision));
        if let Some(revision) = &code.committed_revision {
            text.push_str(&format!("\n- committed revision {revision}"));
        }
        if let Some(revision) = &code.landed_revision {
            let branch = code.qa_branch.as_deref().unwrap_or("QA");
            text.push_str(&format!("\n- landed revision {revision} on {branch}"));
        }
        text.push_str(&format!("\n- workspace {}", code.workspace));
    }
    if !result.limitations.is_empty() {
        text.push_str("\n\nLimitations:");
        for limitation in &result.limitations {
            text.push_str(&format!("\n- {limitation}"));
        }
    }
    if !producer.successor_brief.trim().is_empty() {
        text.push_str(&format!(
            "\n\nSuccessor brief: {}",
            producer.successor_brief
        ));
    }
    text
}

impl WakuBackend {
    /// Project-policy checks the reducer cannot see. The reducer already
    /// proved a committed-workspace run ends at a same-project Landed
    /// submitter; the daemon confirms that submitter's project opts in to
    /// employee submissions. `Revise` passes its post-edit definition list.
    pub(crate) fn validate_chain_projects(
        &self,
        steps: &[&ChainStepDefinition],
    ) -> anyhow::Result<()> {
        let task_state = self.task_state.lock();
        for (index, step) in steps.iter().enumerate() {
            if step.boundary != HandoffBoundary::CommittedWorkspace {
                continue;
            }
            let Some(submitter) = steps[index + 1..].iter().find(|next| {
                next.boundary == HandoffBoundary::Landed && next.project_id == step.project_id
            }) else {
                continue;
            };
            let project = task_state
                .projects
                .iter()
                .find(|project| project.id == submitter.project_id)
                .ok_or_else(|| anyhow!("unknown chain project"))?;
            ensure!(
                project.submissions_enabled,
                "committed-workspace handoff requires submissions enabled for project {}",
                project.name
            );
        }
        Ok(())
    }

    /// The host facts `seal_chain_handoff`/`recheck_chain_handoff` consume.
    /// Reads attempt-recorded state — the provisional result before sealing,
    /// the sealed packet after — and performs the filesystem and Git checks
    /// the reducer cannot. Unverifiable inputs leave their fields unset,
    /// which fails the corresponding gate and pauses the chain.
    #[allow(dead_code)] // Consumed by the execution core's sealing path.
    pub(crate) fn verify_chain_handoff(
        &self,
        chain_id: Uuid,
        attempt_id: Uuid,
        definition_version: u64,
    ) -> anyhow::Result<HandoffVerification> {
        let document = self.boss.document();
        let chain = chain(&document, chain_id)?;
        let attempt = bound_attempt(chain, chain.current_step, attempt_id, definition_version)?;
        self.verify_attempt(&document, chain, chain.current_step, attempt)
    }

    /// Verify `step_index`'s attempt against evidence sealed or provisional
    /// on it — shared by the seal-time check and the dispatch-time recheck,
    /// where the producer is a prior step.
    fn verify_attempt(
        &self,
        document: &BossState,
        chain: &EmployeeChain,
        step_index: usize,
        attempt: &ChainAttempt,
    ) -> anyhow::Result<HandoffVerification> {
        let result = attempt
            .sealed
            .as_ref()
            .map(|handoff| &handoff.result)
            .or(attempt.provisional_result.as_ref());
        let Some(result) = result else {
            return Ok(HandoffVerification::default());
        };
        let producer = &chain.steps[step_index].definition;
        let files_root = self.boss.files_root();
        let known_buckets = self.boss.known_bucket_ids().unwrap_or_default();
        // Snapshot the session catalog under the lock; every filesystem and
        // Git check below runs without it.
        let (producer_project, next_project, unadopted) = {
            let task_state = self.task_state.lock();
            let producer_project = task_state
                .projects
                .iter()
                .find(|project| project.id == producer.project_id)
                .cloned();
            let unadopted = result
                .code
                .as_ref()
                .and_then(|code| dunce::canonicalize(Path::new(&code.workspace)).ok())
                .is_some_and(|workspace| unadopted_owner(&task_state, &workspace).is_some());
            let next_project = chain.steps.get(step_index + 1).and_then(|step| {
                task_state
                    .projects
                    .iter()
                    .find(|project| project.id == step.definition.project_id)
                    .cloned()
            });
            (producer_project, next_project, unadopted)
        };

        // Code evidence first — the verified workspace is what path-based
        // artifact references resolve against for an adopting successor.
        let mut verification = HandoffVerification::default();
        let producer_repo = producer_project
            .as_ref()
            .map(|project| project.path.clone())
            .or_else(|| attempt.workspace.as_deref().map(PathBuf::from))
            .unwrap_or_default();
        let mut adopt_root = None;
        if producer.boundary != HandoffBoundary::ArtifactsReady
            && let (Some(code), Some(project)) = (&result.code, &producer_project)
        {
            if commit_available(&project.path, &code.source_revision) {
                verification.source_revision = Some(code.source_revision.clone());
            }
            match producer.boundary {
                HandoffBoundary::CommittedWorkspace => {
                    if let Ok((workspace, head)) = clean_committed_workspace(
                        project,
                        code,
                        project.submissions_enabled,
                        unadopted,
                    ) {
                        adopt_root = Some(workspace);
                        // The gate binds this tuple to the sealed evidence
                        // verbatim — echo the reported path, not its
                        // canonicalization.
                        verification.clean_committed_workspace =
                            Some((code.workspace.clone(), head));
                    }
                }
                HandoffBoundary::Landed => {
                    let global = self.settings.get().qa_branch;
                    let configured = project
                        .qa_branch
                        .as_deref()
                        .filter(|branch| !branch.trim().is_empty())
                        .unwrap_or(global.as_str());
                    if let Ok(landing) = confirmed_landing(project, configured, code) {
                        verification.confirmed_landing = Some(landing);
                    }
                }
                HandoffBoundary::ArtifactsReady => {}
            }
        }

        let successor = chain.steps.get(step_index + 1);
        // Owned grant context for the `Successor` arm — hoisted so the enum
        // borrows locals, not temporaries.
        let successor_pins: Vec<String>;
        let successor_project: PathBuf;
        let (subject, successor_access) = match successor {
            None => (HandoffSubject::Supervisor, true),
            Some(step) => {
                let definition = &step.definition;
                let project = next_project;
                let persona = match definition.persona_id {
                    Some(id) => document.personas.iter().find(|p| p.id == id),
                    None => document
                        .personas
                        .iter()
                        .find(|p| Some(p.id) == document.employee_persona_id),
                };
                let mut access = project
                    .as_ref()
                    .is_some_and(|project| project.path.is_dir())
                    && persona.is_some()
                    && definition
                        .persona_id
                        .is_none_or(|id| id != document.persona_id)
                    && self
                        .boss
                        .validate_bucket_ids(&definition.permissions.bucket_ids)
                        .is_ok();
                successor_pins = persona
                    .map(|persona| persona.pinned_files.clone())
                    .unwrap_or_default();
                access = access
                    && successor_pins
                        .iter()
                        .all(|file| boss_file(&files_root, file).is_some());
                if access && matches!(producer.boundary, HandoffBoundary::CommittedWorkspace) {
                    // The designated submitter runs merge submit — the
                    // project must still opt in at handoff time.
                    access = producer_project
                        .as_ref()
                        .is_some_and(|project| project.submissions_enabled);
                }
                successor_project = project
                    .as_ref()
                    .map(|project| project.path.clone())
                    .unwrap_or_default();
                (
                    HandoffSubject::Successor {
                        pinned_files: &successor_pins,
                        persona_id: definition.persona_id.or(document.employee_persona_id),
                        bucket_ids: &definition.permissions.bucket_ids,
                        project_path: &successor_project,
                    },
                    access,
                )
            }
        };
        verification.successor_access = successor_access;
        verification.accessible_artifacts = result
            .artifacts
            .iter()
            .filter(|artifact| {
                artifact_accessible(
                    &document,
                    &files_root,
                    &known_buckets,
                    &artifact.reference,
                    &subject,
                    &producer_repo,
                    adopt_root.as_deref(),
                )
            })
            .map(|artifact| artifact.reference.clone())
            .collect();
        Ok(verification)
    }

    /// Resolve what the next attempt's summon receives after an accepted
    /// handoff: adoption of the verified committed workspace, a fresh
    /// worktree at the recorded landed revision, or the step's declared
    /// workspace. The sealed evidence is re-verified here — drift between
    /// acceptance and dispatch surfaces as an error for the execution core
    /// to pause on, never as a dispatch on dead references.
    #[allow(dead_code)] // Consumed by the execution core's dispatch path.
    pub(crate) fn chain_successor_inputs(
        &self,
        chain_id: Uuid,
    ) -> anyhow::Result<ChainSuccessorInputs> {
        let document = self.boss.document();
        let chain = chain(&document, chain_id)?;
        ensure!(
            matches!(chain.status, ChainStatus::Waiting | ChainStatus::Queued),
            "chain is not awaiting dispatch"
        );
        let step = &chain.steps[chain.current_step];
        let definition = &step.definition;
        let mut inputs = ChainSuccessorInputs {
            step: chain.current_step,
            workspace: definition.workspace,
            adopt_worktree: None,
            base_revision: definition.base_revision.clone(),
            expected_starting_revision: None,
            brief: None,
        };
        // A reconciled retry starts fresh at its recorded source revision.
        if let Some(revision) = step
            .attempts
            .last()
            .and_then(|attempt| attempt.starting_revision.clone())
        {
            ensure!(
                matches!(definition.workspace, AgentWorkspace::Worktree),
                "a reconciled retry requires a managed worktree"
            );
            inputs.base_revision = Some(revision.clone());
            inputs.expected_starting_revision = Some(revision.clone());
            return Ok(inputs);
        }
        if chain.current_step == 0 {
            if matches!(definition.workspace, AgentWorkspace::Worktree) {
                inputs.expected_starting_revision = definition.base_revision.clone();
            }
            return Ok(inputs);
        }
        let producer_index = chain.current_step - 1;
        let producer_step = &chain.steps[producer_index];
        ensure!(
            producer_step.status == StepStatus::Succeeded,
            "the previous step is not accepted"
        );
        let producer_attempt = producer_step
            .attempts
            .last()
            .ok_or_else(|| anyhow!("the previous step has no attempt"))?;
        let handoff = producer_attempt
            .sealed
            .as_ref()
            .filter(|handoff| handoff.accepted)
            .ok_or_else(|| anyhow!("the previous handoff is not accepted"))?;
        let verification =
            self.verify_attempt(&document, chain, producer_index, producer_attempt)?;
        for artifact in &handoff.result.artifacts {
            ensure!(
                verification
                    .accessible_artifacts
                    .contains(&artifact.reference),
                "artifact is no longer reachable: {}",
                artifact.name
            );
        }
        let code = handoff.result.code.as_ref();
        match producer_step.definition.boundary {
            HandoffBoundary::CommittedWorkspace => {
                let (workspace, head) = verification
                    .clean_committed_workspace
                    .clone()
                    .ok_or_else(|| anyhow!("the committed workspace is no longer clean"))?;
                let reported = code
                    .and_then(|code| code.committed_revision.as_deref())
                    .ok_or_else(|| anyhow!("committed revision is unreported"))?;
                ensure!(head == reported, "the committed workspace moved");
                inputs.workspace = AgentWorkspace::Adopt;
                inputs.adopt_worktree = Some(PathBuf::from(&workspace));
                inputs.base_revision = None;
                inputs.expected_starting_revision = Some(head);
            }
            HandoffBoundary::Landed => {
                let (revision, ..) = verification
                    .confirmed_landing
                    .clone()
                    .ok_or_else(|| anyhow!("the QA landing is no longer confirmed"))?;
                if matches!(definition.workspace, AgentWorkspace::Worktree) {
                    inputs.base_revision = Some(revision.clone());
                    inputs.expected_starting_revision = Some(revision);
                }
            }
            HandoffBoundary::ArtifactsReady => {
                if matches!(definition.workspace, AgentWorkspace::Worktree) {
                    inputs.expected_starting_revision = definition.base_revision.clone();
                }
            }
        }
        inputs.brief = Some(brief(chain, producer_index, handoff));
        Ok(inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    use waku_boss::chains::ChainObservation;
    use waku_protocol::boss::PersonaPermissions;
    use waku_protocol::model::ProviderKind;
    use waku_protocol::resources::ResourceSet;

    use crate::EventSink;

    use crate::persistence::{PersistedState, StateStore};
    use crate::settings::DaemonSettingsStore;

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    struct Fixture {
        root: PathBuf,
        backend: WakuBackend,
        project_id: Uuid,
        repo: PathBuf,
        outcome_id: Uuid,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("chain-handoff-{}", Uuid::new_v4()));
            let repo = root.join("repo");
            fs::create_dir_all(&repo).unwrap();
            git(&repo, &["init", "-b", "main"]);
            git(&repo, &["config", "user.email", "test@goddard"]);
            git(&repo, &["config", "user.name", "Goddard Test"]);
            fs::write(repo.join("seed.txt"), "seed").unwrap();
            git(&repo, &["add", "seed.txt"]);
            git(&repo, &["commit", "-m", "seed"]);
            let store = StateStore::daemon(root.join("app.db"));
            let mut state = PersistedState::fresh(repo.clone());
            state.projects[0].submissions_enabled = true;
            let project_id = state.projects[0].id;
            store.save(&mut state).unwrap();
            let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
            let mut daemon = settings.get();
            daemon.boss_experiment_enabled = true;
            settings.replace(daemon).unwrap();
            let backend = WakuBackend::new(settings, store).unwrap();
            let outcome_id = backend
                .boss
                .create_outcome(None, "handoff test", "verified")
                .unwrap()
                .id;
            Self {
                root,
                backend,
                project_id,
                repo,
                outcome_id,
            }
        }

        fn step(&self, boundary: HandoffBoundary) -> ChainStepDefinition {
            ChainStepDefinition {
                purpose: Some(waku_protocol::boss::EmployeePurpose::Helping),
                title: "step".into(),
                prompt: "do the work".into(),
                project_id: self.project_id,
                persona_id: None,
                provider: ProviderKind::Codex,
                model: "gpt-5.5".into(),
                reasoning_effort: None,
                permissions: PersonaPermissions::default(),
                resources: ResourceSet::default(),
                workspace: AgentWorkspace::Worktree,
                base_revision: Some(git(&self.repo, &["rev-parse", "HEAD"])),
                boundary,
                required_checks: vec![],
                required_artifacts: vec![],
                required_approvals: vec![],
                max_retries: 1,
                plan_id: None,
                item_id: None,
                successor_brief: String::new(),
            }
        }

        fn create(&self, steps: Vec<ChainStepDefinition>) -> EmployeeChain {
            self.backend
                .boss
                .chain_operation(
                    None,
                    ChainOperation::Create {
                        request_id: Uuid::new_v4(),
                        definition: ChainDefinition {
                            outcome_id: self.outcome_id,
                            finishes_outcome: false,
                            steps,
                        },
                    },
                )
                .unwrap()
        }

        fn show(&self, id: Uuid) -> EmployeeChain {
            self.backend
                .boss
                .chain_operation(None, ChainOperation::Show { chain_id: id })
                .unwrap()
        }

        fn observe(&self, chain: &EmployeeChain, observation: ChainObservation) {
            let attempt = chain.steps[chain.current_step].attempts.last().unwrap();
            self.backend
                .boss
                .observe_chain(
                    chain.id,
                    attempt.id,
                    attempt.definition_version,
                    Uuid::new_v4(),
                    observation,
                )
                .unwrap();
        }

        /// Drive the current attempt to released-terminal state with the
        /// given recorded workspace/revision, ready for verify + seal.
        fn finish_attempt(
            &self,
            chain: &EmployeeChain,
            workspace: &Path,
            result: ChainResult,
        ) -> EmployeeChain {
            self.observe(
                chain,
                ChainObservation::Admitted {
                    employee_id: Uuid::new_v4(),
                },
            );
            self.observe(chain, ChainObservation::Starting);
            self.observe(
                chain,
                ChainObservation::Working {
                    starting_revision: git(&self.repo, &["rev-parse", "HEAD"]),
                    workspace: workspace.display().to_string(),
                },
            );
            let attempt = chain.steps[chain.current_step].attempts.last().unwrap();
            self.backend
                .boss
                .chain_operation(
                    None,
                    ChainOperation::RecordResult {
                        chain_id: chain.id,
                        attempt_id: attempt.id,
                        definition_version: attempt.definition_version,
                        result,
                    },
                )
                .unwrap();
            self.observe(
                chain,
                ChainObservation::Terminal {
                    terminal: ChainTerminal {
                        message: "Exact final\n  ```code```\n".into(),
                        disposition: ResultDisposition::Success,
                    },
                },
            );
            self.observe(
                chain,
                ChainObservation::Released {
                    execution: true,
                    workspace: true,
                },
            );
            self.show(chain.id)
        }

        /// Verify and seal the current attempt; returns the chain.
        fn seal(&self, chain: &EmployeeChain) -> EmployeeChain {
            let attempt = chain.steps[chain.current_step].attempts.last().unwrap();
            let verification = self
                .backend
                .verify_chain_handoff(chain.id, attempt.id, attempt.definition_version)
                .unwrap();
            self.backend
                .boss
                .seal_chain_handoff(
                    chain.id,
                    attempt.id,
                    attempt.definition_version,
                    &verification,
                )
                .unwrap();
            self.show(chain.id)
        }

        fn verification(&self, chain: &EmployeeChain) -> HandoffVerification {
            let attempt = chain.steps[chain.current_step].attempts.last().unwrap();
            self.backend
                .verify_chain_handoff(chain.id, attempt.id, attempt.definition_version)
                .unwrap()
        }

        /// A linked worktree at `base` plus the session record claiming it.
        fn managed_worktree(&self, base: &str) -> PathBuf {
            let created =
                crate::worktree::create(&self.repo, None, Some(base), false, &[]).unwrap();
            let mut session = crate::model::AgentSession::new(self.project_id, ProviderKind::Codex);
            session.workspace = SessionWorkspace::Worktree {
                path: created.path.clone(),
                name: created.name,
                branch: None,
                base_branch: None,
                adopted_by: None,
            };
            let mut state = self.backend.task_state.lock();
            state.sessions.push(session);
            created.path
        }

        fn commit(&self, cwd: &Path, file: &str, contents: &str) -> String {
            fs::write(cwd.join(file), contents).unwrap();
            git(cwd, &["add", file]);
            git(cwd, &["commit", "-m", file]);
            git(cwd, &["rev-parse", "HEAD"])
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }

    fn success_result() -> ChainResult {
        ChainResult {
            disposition: ResultDisposition::Success,
            checks: vec![ChainCheck {
                name: "repro".into(),
                status: CheckStatus::Pass,
                evidence: "reproduced".into(),
            }],
            artifacts: vec![],
            code: None,
            limitations: vec![],
        }
    }

    #[test]
    fn artifact_references_gate_on_the_successors_grants() {
        let fixture = Fixture::new();
        let files = fixture.backend.boss.files_root();
        fs::create_dir_all(files.join("plans")).unwrap();
        fs::write(files.join("plans/design.md"), "# design").unwrap();
        fs::create_dir_all(files.join("docs")).unwrap();
        fs::write(files.join("docs/private.md"), "private").unwrap();
        // A copied deliverable's stored snapshot is durable; an unknown id
        // and a live source outside the successor's roots are not.
        let stored = fixture.root.join("stored-report.md");
        fs::write(&stored, "report").unwrap();
        fixture
            .backend
            .handle_boss_operation(
                None,
                waku_protocol::boss::BossOperation::PublishDeliverable {
                    path: stored.display().to_string(),
                    name: Some("report".into()),
                    reference: false,
                },
                &EventSink::detached(),
            )
            .unwrap();
        let deliverable = fixture.backend.boss.document().deliverables[0].id;
        let own_bucket = crate::boss::project_bucket_id(&fixture.repo);
        let head = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let inside = fixture.repo.join("seed.txt").canonicalize().unwrap();
        let accessible = [
            ChainArtifact {
                name: "plans".into(),
                reference: "plans/design.md".into(),
            },
            ChainArtifact {
                name: "own-bucket".into(),
                reference: format!("memory:{own_bucket}"),
            },
            ChainArtifact {
                name: "commit".into(),
                reference: head.clone(),
            },
            ChainArtifact {
                name: "inside".into(),
                reference: inside.display().to_string(),
            },
            ChainArtifact {
                name: "deliverable".into(),
                reference: format!("deliverable:{deliverable}"),
            },
        ];
        let make_chain = |required: &[&str]| {
            fixture.create(vec![
                {
                    let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
                    step.required_artifacts =
                        required.iter().map(|name| (*name).to_owned()).collect();
                    step
                },
                fixture.step(HandoffBoundary::ArtifactsReady),
            ])
        };
        // A packet carrying any unreachable reference pauses — the gate
        // checks every supplied reference, not just the required ones.
        let mut mixed = success_result();
        mixed.artifacts = accessible.to_vec();
        mixed.artifacts.extend([
            ChainArtifact {
                name: "hidden".into(),
                reference: "docs/private.md".into(),
            },
            ChainArtifact {
                name: "missing".into(),
                reference: "plans/absent.md".into(),
            },
            ChainArtifact {
                name: "other-bucket".into(),
                reference: "memory:custom-bucket".into(),
            },
            ChainArtifact {
                name: "sibling".into(),
                reference: format!("transcript:{}", Uuid::new_v4()),
            },
            ChainArtifact {
                name: "unknown".into(),
                reference: "deadbeef".repeat(5),
            },
            ChainArtifact {
                name: "outside".into(),
                reference: "/etc/hostname".into(),
            },
            ChainArtifact {
                name: "gone".into(),
                reference: format!("deliverable:{}", Uuid::new_v4()),
            },
        ]);
        let chain = fixture.finish_attempt(
            &make_chain(&["plans", "commit", "deliverable"]),
            &fixture.repo,
            mixed,
        );
        let verification = fixture.verification(&chain);
        assert!(verification.successor_access);
        assert_eq!(
            verification.accessible_artifacts,
            vec![
                "plans/design.md".to_owned(),
                format!("memory:{own_bucket}"),
                head.clone(),
                inside.display().to_string(),
                format!("deliverable:{deliverable}"),
            ]
        );
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Paused);
        assert_eq!(
            chain.reason.as_deref(),
            Some("artifact reference is inaccessible")
        );
        // Only reachable references — required or not — carry through.
        let mut clean = success_result();
        clean.artifacts = accessible.to_vec();
        let chain = fixture.finish_attempt(
            &make_chain(&["plans", "commit", "deliverable"]),
            &fixture.repo,
            clean,
        );
        let chain = fixture.seal(&chain);
        assert_eq!(
            chain.status,
            ChainStatus::Waiting,
            "reason: {:?}",
            chain.reason
        );
        assert_eq!(chain.steps[0].status, StepStatus::Succeeded);
        assert!(chain.steps[0].attempts[0].handoff_accepted);
    }

    #[test]
    fn missing_or_inaccessible_artifacts_pause_without_widening() {
        let fixture = Fixture::new();
        let mut result = success_result();
        result.artifacts = vec![ChainArtifact {
            name: "report".into(),
            reference: "docs/nowhere.md".into(),
        }];
        let chain = fixture.create(vec![
            {
                let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
                step.required_artifacts = vec!["report".into()];
                step
            },
            fixture.step(HandoffBoundary::ArtifactsReady),
        ]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Paused);
        assert!(chain.reason.as_deref().unwrap().contains("artifact"));
        assert!(!chain.steps[0].attempts[0].handoff_accepted);
        // The same reference under a still-ungranted path stays inaccessible
        // even after the document appears — existence is not a grant.
        let files = fixture.backend.boss.files_root();
        fs::create_dir_all(files.join("docs")).unwrap();
        fs::write(files.join("docs/nowhere.md"), "recovered").unwrap();
        let verification = fixture.verification(&chain);
        assert!(verification.accessible_artifacts.is_empty());
    }

    #[test]
    fn committed_workspace_transfers_through_exclusive_adoption() {
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let worktree = fixture.managed_worktree(&base);
        let committed = fixture.commit(&worktree, "unit.txt", "work");
        let mut result = success_result();
        result.code = Some(ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(committed.clone()),
            landed_revision: None,
            qa_branch: None,
            submission_id: None,
        });
        let chain = fixture.create(vec![
            {
                let mut step = fixture.step(HandoffBoundary::CommittedWorkspace);
                step.title = "produce".into();
                step
            },
            {
                // The successor's base comes from the verified handoff, so
                // its definition needs none.
                let mut step = fixture.step(HandoffBoundary::Landed);
                step.base_revision = None;
                step.title = "ship".into();
                step
            },
        ]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        let verification = fixture.verification(&chain);
        assert_eq!(
            verification.clean_committed_workspace,
            Some((worktree.display().to_string(), committed.clone()))
        );
        let chain = fixture.seal(&chain);
        assert_eq!(
            chain.status,
            ChainStatus::Waiting,
            "reason: {:?}",
            chain.reason
        );
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.workspace, AgentWorkspace::Adopt);
        assert_eq!(
            inputs.adopt_worktree.as_deref(),
            Some(dunce::canonicalize(&worktree).unwrap().as_path())
        );
        assert_eq!(
            inputs.expected_starting_revision.as_deref(),
            Some(committed.as_str())
        );
        let brief = inputs.brief.unwrap();
        assert!(brief.contains("Exact final"));
        assert!(brief.contains(&committed));
    }

    #[test]
    fn dirty_or_unowned_workspaces_fail_the_committed_gate() {
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let worktree = fixture.managed_worktree(&base);
        let committed = fixture.commit(&worktree, "unit.txt", "work");
        let code = |workspace: &Path| ChainCodeEvidence {
            workspace: workspace.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(committed.clone()),
            landed_revision: None,
            qa_branch: None,
            submission_id: None,
        };
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::CommittedWorkspace),
            fixture.step(HandoffBoundary::Landed),
        ]);
        // Uncommitted work never transfers.
        fs::write(worktree.join("dirty.txt"), "dirty").unwrap();
        let mut result = success_result();
        result.code = Some(code(&worktree));
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        assert!(
            fixture
                .verification(&chain)
                .clean_committed_workspace
                .is_none()
        );
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Paused);
        fs::remove_file(worktree.join("dirty.txt")).unwrap();
        // An adopted-away workspace cannot be handed over a second time.
        let mut state = fixture.backend.task_state.lock();
        let owner = state
            .sessions
            .iter_mut()
            .find(|session| {
                matches!(
                    &session.workspace,
                    SessionWorkspace::Worktree { path, .. }
                        if dunce::canonicalize(path).unwrap() == dunce::canonicalize(&worktree).unwrap()
                )
            })
            .unwrap();
        if let SessionWorkspace::Worktree { adopted_by, .. } = &mut owner.workspace {
            *adopted_by = Some(Uuid::new_v4());
        }
        drop(state);
        assert!(
            fixture
                .verification(&chain)
                .clean_committed_workspace
                .is_none()
        );
    }

    #[test]
    fn landed_handoff_starts_fresh_at_the_recorded_revision() {
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let worktree = fixture.managed_worktree(&base);
        let landed = fixture.commit(&worktree, "unit.txt", "work");
        // The QA branch tracks the landed unit, then moves past it — the
        // successor still starts at the recorded revision, never latest.
        git(&fixture.repo, &["branch", "dev", &landed]);
        fixture.commit(&fixture.repo, "later.txt", "unrelated");
        let mut result = success_result();
        result.code = Some(ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(landed.clone()),
            landed_revision: Some(landed.clone()),
            qa_branch: Some("dev".into()),
            submission_id: Some("sub-1".into()),
        });
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::Landed),
            fixture.step(HandoffBoundary::ArtifactsReady),
        ]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        let verification = fixture.verification(&chain);
        assert_eq!(
            verification.confirmed_landing,
            Some((landed.clone(), "dev".into(), "sub-1".into()))
        );
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Waiting);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.workspace, AgentWorkspace::Worktree);
        assert!(inputs.adopt_worktree.is_none());
        assert_eq!(inputs.base_revision.as_deref(), Some(landed.as_str()));
        assert_eq!(
            inputs.expected_starting_revision.as_deref(),
            Some(landed.as_str())
        );
        assert!(inputs.brief.unwrap().contains("Exact final"));
        // A revision the branch never contained is not a landing.
        let other = fixture.repo.join("other");
        fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-b", "main"]);
        git(&other, &["config", "user.email", "test@goddard"]);
        git(&other, &["config", "user.name", "Goddard Test"]);
        let foreign = fixture.commit(&other, "foreign.txt", "foreign");
        assert!(crate::agent_merge::is_ancestor(&fixture.repo, &foreign, "dev").is_err());
    }

    #[test]
    fn successor_inputs_hold_the_first_step_and_reject_unaccepted_handoffs() {
        let fixture = Fixture::new();
        let chain = fixture.create(vec![fixture.step(HandoffBoundary::ArtifactsReady)]);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert!(inputs.brief.is_none());
        assert_eq!(inputs.workspace, AgentWorkspace::Worktree);
        assert_eq!(
            inputs.expected_starting_revision.as_deref(),
            fixture
                .step(HandoffBoundary::ArtifactsReady)
                .base_revision
                .as_deref()
        );
        assert!(
            fixture
                .backend
                .chain_successor_inputs(Uuid::new_v4())
                .is_err()
        );
    }
}
