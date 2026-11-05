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

fn deliverable_unreachable(
    state: &BossState,
    id: Uuid,
    subject: &HandoffSubject<'_>,
    adopt_root: Option<&Path>,
) -> Option<String> {
    let Some(deliverable) = state
        .deliverables
        .iter()
        .find(|deliverable| deliverable.id == id)
    else {
        return Some(format!("unknown deliverable {id}"));
    };
    let path = Path::new(&deliverable.path);
    // A copied deliverable's snapshot lives in the daemon store; a reference
    // deliverable's path is a live source that dies with its workspace.
    if deliverable.source_path.is_some() {
        return (!path.exists())
            .then(|| format!("deliverable {id} source {} is gone", deliverable.path));
    }
    if !path.exists() {
        return Some(format!(
            "deliverable {id} snapshot {} is missing",
            deliverable.path
        ));
    }
    match subject {
        HandoffSubject::Supervisor => None,
        HandoffSubject::Successor { project_path, .. } => (!dunce::canonicalize(path)
            .is_ok_and(|resolved| successor_path_root(adopt_root, project_path, &resolved)))
        .then(|| format!("deliverable {id} is outside the successor's roots")),
    }
}

/// Why `reference` cannot resolve to content for `subject` — `None` when
/// the artifact is reachable. The reason names the failed resolution so a
/// gate pause or dispatch warning points at the fix, not the symptom.
fn artifact_unreachable(
    state: &BossState,
    files_root: &Path,
    known_buckets: &[String],
    reference: &str,
    subject: &HandoffSubject<'_>,
    producer_repo: &Path,
    adopt_root: Option<&Path>,
) -> Option<String> {
    let reference = reference.trim();
    Some(match (classify(reference), subject) {
        (ArtifactRef::File(relative), HandoffSubject::Supervisor) => {
            if boss_file(files_root, relative).is_some() {
                return None;
            }
            format!("boss file {relative} does not exist")
        }
        (
            ArtifactRef::File(relative),
            HandoffSubject::Successor {
                pinned_files,
                persona_id,
                ..
            },
        ) => {
            if !file_granted(relative, pinned_files, *persona_id) {
                format!("file {relative} is outside the successor's grants")
            } else if boss_file(files_root, relative).is_none() {
                format!("boss file {relative} does not exist")
            } else {
                return None;
            }
        }
        (ArtifactRef::Transcript(session), HandoffSubject::Supervisor) => {
            if transcript_exists(state, session) {
                return None;
            }
            format!("unknown employee session {session}")
        }
        // A chain successor is the producer's sibling — transcript grants
        // follow supervisor lineage only.
        (ArtifactRef::Transcript(_), HandoffSubject::Successor { .. }) => {
            "a sibling step cannot read another employee's transcript".to_owned()
        }
        (ArtifactRef::Memory(value), HandoffSubject::Supervisor) => {
            let bucket = value.split(':').next().unwrap_or_default();
            if known_buckets.iter().any(|known| known == bucket) {
                return None;
            }
            format!("unknown memory bucket {bucket}")
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
            if bucket == own || bucket_ids.iter().any(|granted| granted == bucket) {
                return None;
            }
            format!("memory bucket {bucket} is not granted to the successor")
        }
        (ArtifactRef::Deliverable(id), _) => {
            return deliverable_unreachable(state, id, subject, adopt_root);
        }
        (ArtifactRef::Commit(revision), HandoffSubject::Supervisor) => {
            if commit_available(producer_repo, revision) {
                return None;
            }
            format!(
                "commit {revision} is not an object in {}",
                producer_repo.display()
            )
        }
        (ArtifactRef::Commit(revision), HandoffSubject::Successor { project_path, .. }) => {
            let repo = adopt_root.unwrap_or(project_path);
            if commit_available(repo, revision) {
                return None;
            }
            format!(
                "commit {revision} is not an object in the successor's repository {}",
                repo.display()
            )
        }
        (ArtifactRef::CheckoutPath(path), HandoffSubject::Supervisor) => {
            if dunce::canonicalize(path).is_ok() {
                return None;
            }
            format!("path {} does not exist", path.display())
        }
        (ArtifactRef::CheckoutPath(path), HandoffSubject::Successor { project_path, .. }) => {
            if dunce::canonicalize(path)
                .is_ok_and(|resolved| successor_path_root(adopt_root, project_path, &resolved))
            {
                return None;
            }
            format!(
                "path {} does not resolve inside the successor's roots",
                path.display()
            )
        }
        (ArtifactRef::Unknown, _) => format!("unrecognized artifact reference {reference:?}"),
    })
}

/// The session still owning `workspace`, when it is an unadopted daemon
/// worktree — the exclusive-adoption precondition.
fn unadopted_owner(state: &PersistedState, workspace: &Path) -> Option<Uuid> {
    let mut owners = state
        .sessions
        .iter()
        .filter(|session| {
            matches!(
                &session.workspace,
                SessionWorkspace::Worktree { path, adopted_by, .. }
                    if adopted_by.is_none()
                        && dunce::canonicalize(path).is_ok_and(|path| path == workspace)
            )
        })
        .map(|session| session.id);
    let owner = owners.next()?;
    owners.next().is_none().then_some(owner)
}

fn git_revision(cwd: &Path, reference: &str) -> anyhow::Result<String> {
    crate::agent_merge::git_text(
        cwd,
        &["rev-parse", "--verify", "--end-of-options", reference],
    )
    .map(|revision| revision.trim().to_owned())
}

/// A declared worktree base resolved to a commit id. Bare text that is a
/// full commit id passes through pinned; any other bare text resolves now
/// as a ref — admission-time resolution is what lets a chain name a moving
/// branch. A kinded source takes the previous step's accepted landing,
/// verified in the repo the worktree will fork from.
fn resolve_base_source(
    repo: &Path,
    source: &ChainBaseSource,
    landed: Option<&str>,
) -> anyhow::Result<String> {
    match source {
        ChainBaseSource::Bare(text) => {
            let text = text.trim();
            ensure!(!text.is_empty(), "chain source revision is empty");
            if (text.len() == 40 || text.len() == 64)
                && text.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Ok(text.to_owned());
            }
            git_revision(repo, &format!("{text}^{{commit}}"))
                .with_context(|| format!("chain source ref {text:?} is unavailable"))
        }
        ChainBaseSource::Kinded(ChainBaseKind::PreviousStepLanded) => {
            let landed = landed
                .map(str::trim)
                .filter(|landed| !landed.is_empty())
                .ok_or_else(|| anyhow!("the previous step reported no landed revision"))?;
            git_revision(repo, &format!("{landed}^{{commit}}"))
                .context("the previous step's landed revision is unavailable")
        }
    }
}

/// The landed revision a prior step's accepted handoff recorded, if any.
fn accepted_landing(chain: &EmployeeChain, step_index: usize) -> Option<String> {
    chain
        .steps
        .get(step_index)?
        .attempts
        .last()
        .filter(|attempt| attempt.handoff_accepted)
        .and_then(|attempt| attempt.sealed.as_ref())
        .and_then(|handoff| handoff.result.code.as_ref())
        .and_then(|code| code.landed_revision.clone())
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
/// configured QA branch. The submission id and QA branch are the
/// employee's claims — `merge submit` reports neither — so each is
/// echoed for traceability when present and never required; the branch
/// ancestry is what the host confirms.
fn confirmed_landing(
    project: &Project,
    configured_branch: &str,
    code: &ChainCodeEvidence,
) -> anyhow::Result<(String, String, Option<String>)> {
    let landed = code
        .landed_revision
        .as_deref()
        .ok_or_else(|| anyhow!("landed revision is unreported"))?;
    let branch = crate::review::qa_branch_name(configured_branch);
    if let Some(claimed_branch) = code.qa_branch.as_deref() {
        ensure!(
            claimed_branch == branch,
            "reported QA branch {claimed_branch:?} is not the configured {branch:?}"
        );
    }
    let tip = format!("refs/heads/{branch}");
    ensure!(
        git_revision(&project.path, &tip).is_ok(),
        "QA branch {branch:?} is not checked out locally"
    );
    ensure!(
        crate::agent_merge::is_ancestor(&project.path, landed, &tip)?,
        "the reported revision is not reachable on {branch:?}"
    );
    Ok((landed.to_owned(), branch, code.submission_id.clone()))
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
        for artifact in &result.artifacts {
            match artifact_unreachable(
                document,
                &files_root,
                &known_buckets,
                &artifact.reference,
                &subject,
                &producer_repo,
                adopt_root.as_deref(),
            ) {
                None => verification
                    .accessible_artifacts
                    .push(artifact.reference.clone()),
                Some(reason) => verification
                    .artifact_diagnostics
                    .push((artifact.reference.clone(), reason)),
            }
        }
        Ok(verification)
    }

    /// Resolve what the next attempt's summon receives after an accepted
    /// handoff: adoption of the verified committed workspace, a fresh
    /// worktree at the recorded landed revision, or the step's declared
    /// workspace. Dispatch inputs are re-verified here — drift between
    /// acceptance and dispatch surfaces as an error for the execution core
    /// to pause on, never as a dispatch on a dead workspace or landing.
    /// Artifact references were already the seal's verdict, so drift there
    /// rides the successor brief as a warning instead of re-pausing.
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
            base_revision: None,
            expected_starting_revision: None,
            brief: None,
        };
        // A reconciled retry preserves committed-workspace chains; other
        // boundaries start fresh at the recorded source revision.
        if let Some(revision) = step
            .attempts
            .last()
            .and_then(|attempt| attempt.starting_revision.clone())
        {
            if definition.workspace == AgentWorkspace::Local
                && definition.boundary == HandoffBoundary::ArtifactsReady
            {
                self.verify_local_retry_source(
                    step,
                    &step.attempts[..step.attempts.len() - 1],
                    &revision,
                )?;
                // Local execution uses the existing checkout; never pass a
                // checkout/reset base to the workspace machinery.
                inputs.expected_starting_revision = Some(revision);
                return Ok(inputs);
            }
            ensure!(
                matches!(definition.workspace, AgentWorkspace::Worktree),
                "a reconciled retry requires a managed worktree"
            );
            if definition.boundary == HandoffBoundary::CommittedWorkspace
                && let Some((prior, path)) = step
                    .attempts
                    .iter()
                    .find_map(|attempt| attempt.workspace.as_ref().map(|path| (attempt, path)))
            {
                // This boundary promises one workspace across employees. A
                // retry must not silently fork away its committed or dirty work.
                ensure!(
                    prior.execution_released && prior.workspace_released,
                    "original retry workspace has not been released"
                );
                let workspace = dunce::canonicalize(path)?;
                let state = self.task_state.lock();
                let project = state
                    .projects
                    .iter()
                    .find(|project| project.id == definition.project_id)
                    .ok_or_else(|| anyhow!("retry project is unavailable"))?;
                ensure!(
                    crate::worktree::is_worktree_of(&project.path, &workspace),
                    "retry workspace is not a managed worktree of the accepted project"
                );
                let owner = unadopted_owner(&state, &workspace)
                    .ok_or_else(|| anyhow!("original retry workspace ownership changed"))?;
                if let Some(expected) = step
                    .attempts
                    .last()
                    .and_then(|attempt| attempt.retry_workspace_lineage.as_ref())
                {
                    ensure!(
                        expected.owners.last() == Some(&owner)
                            && self.retry_repair_lineage(chain, &state, &workspace, owner)?
                                == *expected,
                        "repair ownership lineage changed since retry"
                    );
                    crate::agent_merge::clean_status(&workspace, "all", "repair retry admission")?;
                    ensure!(
                        git_revision(&workspace, "HEAD")? == revision,
                        "repaired source changed since retry"
                    );
                } else {
                    ensure!(
                        step.attempts.iter().any(|attempt| {
                            attempt.employee_id == Some(owner)
                                && attempt.execution_released
                                && attempt.workspace_released
                                && attempt.workspace.as_ref().is_some_and(|path| {
                                    dunce::canonicalize(path).is_ok_and(|path| path == workspace)
                                })
                        }),
                        "original retry workspace ownership changed"
                    );
                }
                drop(state);
                crate::agent_merge::no_operation(&workspace)?;
                let head = git_revision(&workspace, "HEAD")?;
                ensure!(
                    crate::agent_merge::is_ancestor(&workspace, &revision, &head)?,
                    "retry workspace no longer descends from the reconciled revision"
                );
                inputs.workspace = AgentWorkspace::Adopt;
                inputs.adopt_worktree = Some(workspace);
                inputs.expected_starting_revision = Some(head);
                return Ok(inputs);
            }
            inputs.base_revision = Some(revision.clone());
            inputs.expected_starting_revision = Some(revision.clone());
            return Ok(inputs);
        }
        if chain.current_step == 0 {
            if matches!(definition.workspace, AgentWorkspace::Worktree) {
                // Symbolic bases resolve at admission — the ticketed
                // checkout and the durable record only ever see the
                // commit id.
                let base = self.chain_step_base(chain, 0)?;
                inputs.base_revision = base.clone();
                inputs.expected_starting_revision = base;
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
        // Acceptance is the live verdict, not the seal-time packet — a
        // gate recheck grants it and a rejected approval revokes it after
        // sealing, while `sealed.accepted` keeps the original record.
        let handoff = producer_attempt
            .sealed
            .as_ref()
            .filter(|_| producer_attempt.handoff_accepted)
            .ok_or_else(|| anyhow!("the previous handoff is not accepted"))?;
        let verification =
            self.verify_attempt(&document, chain, producer_index, producer_attempt)?;
        // Artifact reachability is a seal gate, not a dispatch gate — the
        // owner may already have accepted flagged evidence, and a dead
        // reference cannot be re-supplied here, so re-failing admission
        // would only dead-end the chain. What still hard-verifies is the
        // dispatch input itself (workspace, landing, base); unreachable
        // references degrade to a warning in the successor's brief.
        let artifact_warnings: Vec<(&ChainArtifact, &str)> = handoff
            .result
            .artifacts
            .iter()
            .filter_map(|artifact| {
                verification
                    .artifact_diagnostics
                    .iter()
                    .find(|(reference, _)| reference == &artifact.reference)
                    .map(|(_, reason)| (artifact, reason.as_str()))
            })
            .collect();
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
                    let base = self.chain_step_base(chain, chain.current_step)?;
                    inputs.base_revision = base.clone();
                    inputs.expected_starting_revision = base;
                }
            }
        }
        let mut text = brief(chain, producer_index, handoff);
        if !artifact_warnings.is_empty() {
            text.push_str(
                "\n\nUnreachable predecessor artifacts — accepted but not resolvable now:",
            );
            for (artifact, reason) in &artifact_warnings {
                text.push_str(&format!(
                    "\n- {} — {} ({reason})",
                    artifact.name, artifact.reference
                ));
            }
        }
        inputs.brief = Some(text);
        Ok(inputs)
    }

    /// The step's declared base resolved against its own project —
    /// `None` when undeclared. The previous step's accepted landing feeds
    /// `previousStepLanded`.
    pub(crate) fn chain_step_base(
        &self,
        chain: &EmployeeChain,
        step_index: usize,
    ) -> anyhow::Result<Option<String>> {
        let step = &chain.steps[step_index];
        let Some(source) = &step.definition.base_revision else {
            return Ok(None);
        };
        if matches!(source, ChainBaseSource::Kinded(_)) {
            ensure!(
                step_index > 0,
                "previousStepLanded requires a previous step"
            );
        }
        let project = self
            .task_state
            .lock()
            .projects
            .iter()
            .find(|project| project.id == step.definition.project_id)
            .cloned()
            .ok_or_else(|| anyhow!(
                "project_unavailable: accepted project {} is no longer registered; restore that registration or explicitly revise the chain; no replacement was selected",
                step.definition.project_id))?;
        let landed = step_index
            .checked_sub(1)
            .and_then(|prior| accepted_landing(chain, prior));
        resolve_base_source(&project.path, source, landed.as_deref()).map(Some)
    }

    /// Follow only persisted managed-adoption edges rooted in a released attempt.
    /// Repairs must serve this outcome, have no chain binding, and retain neither
    /// execution nor admission claims. No employee-supplied result proves ownership.
    fn retry_repair_lineage(
        &self,
        chain: &EmployeeChain,
        state: &PersistedState,
        workspace: &Path,
        owner: Uuid,
    ) -> anyhow::Result<RetryWorkspaceLineage> {
        use waku_protocol::boss::EmployeeLifecycle;
        let step = &chain.steps[chain.current_step];
        let root = step
            .attempts
            .iter()
            .rev()
            .find(|attempt| {
                attempt.employee_id.is_some()
                    && attempt.execution_released
                    && attempt.workspace_released
                    && attempt.workspace.as_ref().is_some_and(|path| {
                        dunce::canonicalize(path).is_ok_and(|path| path == workspace)
                    })
            })
            .and_then(|attempt| attempt.employee_id)
            .ok_or_else(|| anyhow!("repair lineage has no released attempt root"))?;
        let project = state
            .projects
            .iter()
            .find(|project| project.id == step.definition.project_id)
            .ok_or_else(|| anyhow!("repair project is missing"))?;
        let supervisor = self
            .boss
            .employee_including_retired(root)
            .ok_or_else(|| anyhow!("repair root ticket is missing"))?
            .supervisor_id;
        // The supervisor is a principal, not a session id: a rotated Boss
        // generation and its planning sessions admit repairs under new ids
        // that answer to the same supervisor. Employee-supervised repairs
        // keep exact identity.
        let supervisor_lineage = self.boss_lineage_sessions(supervisor);
        let boss_supervised =
            supervisor_lineage.len() > 1 || self.boss.is_boss_principal(supervisor);
        let same_supervisor = |candidate: Uuid| {
            candidate == supervisor
                || (boss_supervised
                    && (supervisor_lineage.contains(&candidate)
                        || self.boss.is_boss_principal(candidate)))
        };
        ensure!(
            state
                .sessions
                .iter()
                .filter(|session| matches!(&session.workspace,
            SessionWorkspace::Worktree { path, adopted_by: None, .. }
                if dunce::canonicalize(path).is_ok_and(|path| path == workspace)))
                .count()
                <= 1,
            "repair workspace has concurrent ownership claims"
        );
        let mut owners = Vec::new();
        let mut current = root;
        loop {
            ensure!(
                !owners.contains(&current),
                "repair ownership lineage contains a cycle"
            );
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == current)
                .ok_or_else(|| anyhow!("repair ownership session is missing"))?;
            ensure!(
                session.project_id == step.definition.project_id,
                "repair ownership belongs to a foreign project"
            );
            let next = match &session.workspace {
                SessionWorkspace::Worktree {
                    path,
                    adopted_by,
                    borrowed: false,
                    ..
                } if dunce::canonicalize(path).is_ok_and(|path| path == workspace) => *adopted_by,
                _ => anyhow::bail!("repair ownership left the original managed workspace"),
            };
            let employee = self
                .boss
                .employee_including_retired(current)
                .ok_or_else(|| anyhow!("repair ownership ticket is missing"))?;
            let ticket = employee
                .ticket
                .as_ref()
                .ok_or_else(|| anyhow!("repair ownership admission is missing"))?;
            ensure!(
                employee.lifecycle() == EmployeeLifecycle::Expired
                    && !employee.workspace_transition
                    && ticket.reservation.is_none()
                    && ticket.pending_reservation.is_none()
                    && !self.agent.has_open_turn(current)
                    && !self.sessions.lock().contains_key(&current),
                "repair owner still has execution or admission claims"
            );
            if current != root {
                ensure!(
                    employee
                        .assignment
                        .as_ref()
                        .is_some_and(|assignment| assignment.outcome_id == chain.request.outcome_id)
                        && ticket.outcome_id == Some(chain.request.outcome_id),
                    "repair owner is not assigned to this chain outcome"
                );
                ensure!(
                    same_supervisor(employee.supervisor_id),
                    "repair owner answers to a different supervisor"
                );
                ensure!(
                    dunce::canonicalize(&ticket.project)
                        .is_ok_and(|path| dunce::canonicalize(&project.path)
                            .is_ok_and(|project| path == project))
                        && ticket.workspace == Some(AgentWorkspace::Adopt)
                        && ticket
                            .adopt_worktree
                            .as_ref()
                            .is_some_and(|path| dunce::canonicalize(path)
                                .is_ok_and(|path| path == workspace)),
                    "repair owner ticket did not adopt this chain workspace"
                );
                ensure!(
                    self.chain_binding(current).is_none(),
                    "repair owner is bound to a chain attempt"
                );
            }
            owners.push(current);
            if current == owner {
                break;
            }
            current = next.ok_or_else(|| anyhow!("original retry workspace ownership changed"))?;
        }
        ensure!(owners.len() > 1, "repair lineage has no repair owner");
        Ok(RetryWorkspaceLineage { owners })
    }

    /// Check a captured repair lineage again under the adoption's task-state lock,
    /// immediately before its first ownership write. Partial adoption replay may
    /// already credit this exact adopter; any different owner remains an error.
    pub(super) fn verify_retry_repair_adoption(
        &self,
        state: &PersistedState,
        workspace: &Path,
        owner: Uuid,
        adopter: Uuid,
    ) -> anyhow::Result<()> {
        let document = self.boss.document();
        let employee = self.boss.employee_including_retired(adopter);
        for chain in &document.chains {
            let step = &chain.steps[chain.current_step];
            let Some(attempt) = step.attempts.last().filter(|attempt| {
                attempt.employee_id == Some(adopter)
                    || employee
                        .as_ref()
                        .is_some_and(|employee| employee.request_id == Some(attempt.dispatch_id))
            }) else {
                continue;
            };
            let Some(expected) = &attempt.retry_workspace_lineage else {
                return Ok(());
            };
            ensure!(
                expected.owners.last() == Some(&owner),
                "repair workspace owner changed since retry"
            );
            ensure!(
                self.retry_repair_lineage(chain, state, workspace, owner)? == *expected,
                "repair ownership lineage changed since retry"
            );
            crate::agent_merge::no_operation(workspace)?;
            crate::agent_merge::clean_status(workspace, "all", "repair retry adoption")?;
            ensure!(
                Some(git_revision(workspace, "HEAD")?).as_ref()
                    == attempt.starting_revision.as_ref(),
                "repaired source changed since retry"
            );
            return Ok(());
        }
        Ok(())
    }

    /// Local artifact retries retain the shared checkout, including dirty work.
    /// Releases fence prior attempts; HEAD must still match the captured source.
    fn verify_local_retry_source(
        &self,
        step: &ChainStep,
        prior: &[ChainAttempt],
        revision: &str,
    ) -> anyhow::Result<PathBuf> {
        let project = self
            .task_state
            .lock()
            .projects
            .iter()
            .find(|project| project.id == step.definition.project_id)
            .cloned()
            .ok_or_else(|| anyhow!("retry project unavailable"))?;
        let workspace = dunce::canonicalize(&project.path)?;
        for attempt in prior {
            ensure!(
                attempt.execution_released && attempt.workspace_released,
                "original local retry execution or workspace has not been released"
            );
            if let Some(path) = &attempt.workspace {
                ensure!(
                    dunce::canonicalize(path)? == workspace,
                    "local retry workspace differs from the accepted project checkout"
                );
            }
        }
        crate::agent_merge::no_operation(&workspace)?;
        ensure!(
            git_revision(&workspace, "HEAD")? == revision,
            "local retry source must equal current project HEAD; retry explicitly at the current source"
        );
        Ok(workspace)
    }

    /// A queued local retry must not launch at a different source after waiting
    /// for capacity or recovering from restart.
    pub(super) fn verify_local_retry_dispatch(&self, employee_id: Uuid) -> anyhow::Result<()> {
        let document = self.boss.document();
        for chain in &document.chains {
            let step = &chain.steps[chain.current_step];
            let Some((attempt, prior)) = step.attempts.split_last() else {
                continue;
            };
            if step.definition.workspace != AgentWorkspace::Local
                || step.definition.boundary != HandoffBoundary::ArtifactsReady
                || prior.is_empty()
            {
                continue;
            }
            if attempt.employee_id == Some(employee_id) {
                self.verify_local_retry_source(
                    step,
                    prior,
                    attempt
                        .starting_revision
                        .as_deref()
                        .ok_or_else(|| anyhow!("local retry source is missing"))?,
                )?;
            }
        }
        Ok(())
    }

    /// Reconcile committed descendant repairs in the original released workspace.
    /// The operation guard fences chain controls; admission repeats workspace ownership
    /// and ancestry checks before any provider execution.
    pub(crate) fn chain_retry_verification(
        &self,
        chain_id: Uuid,
        revision: &str,
    ) -> anyhow::Result<Option<waku_boss::chains::RetryVerification>> {
        let document = self.boss.document();
        let chain = chain(&document, chain_id)?;
        let step = &chain.steps[chain.current_step];
        let attempt = step
            .attempts
            .last()
            .ok_or_else(|| anyhow!("no retry attempt"))?;
        if (attempt.employee_id.is_none()
            && attempt.workspace.is_none()
            && attempt.starting_revision.is_none())
            || attempt.reconciled_retry_revision.as_deref() == Some(revision)
        {
            return Ok(None);
        }
        ensure!(
            chain.status == ChainStatus::Paused,
            "repair reconciliation requires a paused step"
        );
        ensure!(
            attempt.execution_released
                && attempt.workspace_released
                && attempt
                    .sealed
                    .as_ref()
                    .is_some_and(|handoff| !handoff.accepted),
            "repair reconciliation requires sealed released unsuccessful effects"
        );
        let base = attempt
            .reconciled_retry_revision
            .as_ref()
            .ok_or_else(|| anyhow!("uncertain prior effects must be reconciled before retry; prior effects have no reconciled source"))?;
        if step.definition.workspace == AgentWorkspace::Local
            && step.definition.boundary == HandoffBoundary::ArtifactsReady
        {
            let workspace = self.verify_local_retry_source(step, &step.attempts, revision)?;
            ensure!(
                crate::agent_merge::is_ancestor(&workspace, base, revision)?,
                "local retry source must descend from reconciled effects"
            );
            return Ok(Some(waku_boss::chains::RetryVerification {
                attempt_id: attempt.id,
                previous_revision: base.clone(),
                source_revision: revision.to_owned(),
                workspace_lineage: None,
            }));
        }
        ensure!(
            step.definition.boundary == HandoffBoundary::CommittedWorkspace
                && step.definition.workspace == AgentWorkspace::Worktree,
            "repair reconciliation requires a paused committed-workspace step"
        );
        let original = step
            .attempts
            .iter()
            .find_map(|entry| entry.workspace.as_ref())
            .ok_or_else(|| anyhow!("original retry workspace is missing"))?;
        let workspace = dunce::canonicalize(original)?;
        ensure!(
            attempt
                .workspace
                .as_ref()
                .is_some_and(|path| dunce::canonicalize(path).is_ok_and(|path| path == workspace)),
            "retry attempt left the original workspace"
        );
        let state = self.task_state.lock();
        let project = state
            .projects
            .iter()
            .find(|project| project.id == step.definition.project_id)
            .ok_or_else(|| anyhow!("retry project unavailable"))?;
        ensure!(
            project.submissions_enabled
                && crate::worktree::is_worktree_of(&project.path, &workspace),
            "retry workspace is not managed by the accepted project"
        );
        let owner = unadopted_owner(&state, &workspace)
            .ok_or_else(|| anyhow!("original retry workspace ownership changed"))?;
        let workspace_lineage = if step.attempts.iter().any(|entry| {
            entry.employee_id == Some(owner)
                && entry.execution_released
                && entry.workspace_released
                && entry.workspace.as_ref().is_some_and(|path| {
                    dunce::canonicalize(path).is_ok_and(|path| path == workspace)
                })
        }) {
            None
        } else {
            Some(self.retry_repair_lineage(chain, &state, &workspace, owner)?)
        };
        drop(state);
        crate::agent_merge::no_operation(&workspace)?;
        crate::agent_merge::clean_status(&workspace, "all", "repair reconciliation")?;
        ensure!(
            git_revision(&workspace, "HEAD")? == revision,
            "repaired source must equal original workspace HEAD"
        );
        ensure!(
            crate::agent_merge::is_ancestor(&workspace, base, revision)?,
            "repair source must descend from reconciled effects"
        );
        Ok(Some(waku_boss::chains::RetryVerification {
            attempt_id: attempt.id,
            previous_revision: base.clone(),
            source_revision: revision.to_owned(),
            workspace_lineage,
        }))
    }

    /// A retry control's source resolved to a commit id — the same
    /// grammar as a step's declared base. Refs resolve against the
    /// retried step's project now; the durable attempt record only ever
    /// carries commit ids, so kinded sources resolve before the reducer.
    pub(crate) fn chain_retry_base(
        &self,
        chain_id: Uuid,
        source: &ChainBaseSource,
    ) -> anyhow::Result<String> {
        let document = self.boss.document();
        let chain = chain(&document, chain_id)?;
        if matches!(source, ChainBaseSource::Kinded(_)) {
            ensure!(
                chain.current_step > 0,
                "previousStepLanded requires a previous step"
            );
        }
        let step = &chain.steps[chain.current_step];
        let project = self
            .task_state
            .lock()
            .projects
            .iter()
            .find(|project| project.id == step.definition.project_id)
            .cloned()
            .ok_or_else(|| anyhow!(
                "project_unavailable: accepted project {} is no longer registered; restore that registration or explicitly revise the chain; no replacement was selected",
                step.definition.project_id))?;
        let landed = chain
            .current_step
            .checked_sub(1)
            .and_then(|prior| accepted_landing(chain, prior));
        resolve_base_source(&project.path, source, landed.as_deref())
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
                project: None,
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
                base_revision: Some(ChainBaseSource::Bare(git(
                    &self.repo,
                    &["rev-parse", "HEAD"],
                ))),
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
                borrowed: false,
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
        let published = fixture
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
        // The publish result hands the publisher the exact reference the
        // handoff gate accepts — the employee never reconstructs the id.
        let waku_protocol::boss::BossResult::DeliverablePublished {
            reference: deliverable_reference,
            ..
        } = published
        else {
            panic!("publish must return the stored deliverable record");
        };
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
                reference: deliverable_reference.clone(),
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
                deliverable_reference.clone(),
            ]
        );
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Paused);
        let reason = chain.reason.as_deref().unwrap();
        assert!(
            reason.starts_with("artifact reference is inaccessible: hidden"),
            "reason: {reason}"
        );
        assert!(reason.contains("outside the successor's grants"));
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

    /// A seal rejected on a repairable gate and accepted on recheck must
    /// let the successor launch — dispatch reads the live acceptance
    /// verdict, never the immutable seal-time packet.
    #[test]
    fn recheck_accepted_handoff_launches_the_successor() {
        let fixture = Fixture::new();
        let mut result = success_result();
        result.artifacts = vec![ChainArtifact {
            name: "report".into(),
            reference: "plans/report.md".into(),
        }];
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::ArtifactsReady),
            fixture.step(HandoffBoundary::ArtifactsReady),
        ]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Paused);
        let attempt = &chain.steps[0].attempts[0];
        assert!(!attempt.handoff_accepted);
        assert!(!attempt.sealed.as_ref().unwrap().accepted);
        // Repair the gate — the referenced artifact now exists under the
        // granted plans root — and recheck accepts the same sealed packet.
        let files = fixture.backend.boss.files_root();
        fs::create_dir_all(files.join("plans")).unwrap();
        fs::write(files.join("plans/report.md"), "recovered").unwrap();
        let attempt = chain.steps[0].attempts[0].clone();
        let verification = fixture.verification(&chain);
        fixture
            .backend
            .boss
            .recheck_chain_handoff(
                chain.id,
                attempt.id,
                attempt.definition_version,
                &verification,
            )
            .unwrap();
        let chain = fixture.show(chain.id);
        let attempt = &chain.steps[0].attempts[0];
        assert!(attempt.handoff_accepted);
        assert!(!attempt.sealed.as_ref().unwrap().accepted);
        assert_eq!(chain.steps[0].status, StepStatus::Succeeded);
        assert_eq!(chain.status, ChainStatus::Paused);
        // Resume advances — and the successor's dispatch inputs resolve
        // rather than refusing the seal-time rejection.
        let chain = fixture
            .backend
            .boss
            .chain_operation(
                None,
                ChainOperation::Control {
                    chain_id: chain.id,
                    expected_revision: chain.revision,
                    control: ChainControl::Resume,
                    source_revision: None,
                },
            )
            .unwrap();
        assert_eq!(chain.status, ChainStatus::Waiting);
        assert_eq!(chain.current_step, 1);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.step, 1);
        assert!(inputs.brief.unwrap().contains("Exact final"));
    }

    /// The seal's artifact verdict is final: an owner-accepted reference
    /// that still cannot resolve must not kill the successor's admission.
    /// The dispatch hands off with the dead reference named as a warning —
    /// re-failing here would dead-end the chain on a waived gate.
    #[test]
    fn accepted_unreachable_artifacts_warn_the_successor() {
        let fixture = Fixture::new();
        let bogus = "0".repeat(40);
        let mut result = success_result();
        result.artifacts = vec![ChainArtifact {
            name: "parity report".into(),
            reference: format!("commit:{bogus}"),
        }];
        let chain = fixture.create(vec![
            {
                let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
                step.required_artifacts = vec!["parity report".into()];
                step
            },
            fixture.step(HandoffBoundary::ArtifactsReady),
        ]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Paused);
        let reason = chain.reason.as_deref().unwrap();
        assert!(
            reason.starts_with("required artifact is inaccessible: parity report"),
            "reason: {reason}"
        );
        assert!(reason.contains(&bogus), "reason: {reason}");
        // The owner accepts the gate-failed handoff; resume advances.
        let chain = fixture
            .backend
            .boss
            .chain_operation(
                None,
                ChainOperation::Control {
                    chain_id: chain.id,
                    expected_revision: chain.revision,
                    control: ChainControl::Accept,
                    source_revision: None,
                },
            )
            .unwrap();
        let chain = fixture
            .backend
            .boss
            .chain_operation(
                None,
                ChainOperation::Control {
                    chain_id: chain.id,
                    expected_revision: chain.revision,
                    control: ChainControl::Resume,
                    source_revision: None,
                },
            )
            .unwrap();
        assert_eq!(chain.status, ChainStatus::Waiting);
        assert_eq!(chain.current_step, 1);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        let brief = inputs.brief.unwrap();
        assert!(
            brief.contains("Unreachable predecessor artifacts")
                && brief.contains("parity report")
                && brief.contains(&format!("commit:{bogus}")),
            "brief: {brief}"
        );
    }

    /// Build real managed adoption edges without starting a provider. Tickets and
    /// expired employee records use the same persistence APIs as normal admission.
    fn repair_fixture() -> (Fixture, EmployeeChain, PathBuf, String, Vec<Uuid>) {
        repair_fixture_with(|_| None)
    }

    /// `repair_supervisor` overrides each non-root owner's recorded
    /// supervisor — Boss principal sessions legitimately differ from the
    /// failed attempt's across rotations and planning sessions.
    fn repair_fixture_with(
        repair_supervisor: impl Fn(usize) -> Option<Uuid>,
    ) -> (Fixture, EmployeeChain, PathBuf, String, Vec<Uuid>) {
        use waku_protocol::boss::{Assignment, EmployeeGoal, EmployeeLifecycle, SummonTicket};
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let workspace = fixture.managed_worktree(&base);
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::CommittedWorkspace),
            fixture.step(HandoffBoundary::Landed),
        ]);
        let mut result = success_result();
        result.disposition = ResultDisposition::Blocked;
        let chain = fixture.finish_attempt(&chain, &workspace, result);
        let root = chain.steps[0].attempts[0].employee_id.unwrap();
        fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|session| session.workspace.path() == Some(workspace.as_path()))
            .unwrap()
            .id = root;
        let chain = fixture.seal(&chain);
        fixture.observe(
            &chain,
            ChainObservation::EffectsReconciled {
                source_revision: base,
            },
        );
        let supervisor = Uuid::new_v4();
        fixture.backend.boss.set_session_id(supervisor).unwrap();
        let mut owners = vec![root];
        for index in 0..3 {
            let mut employee = fixture
                .backend
                .boss
                .prepare_employee(
                    supervisor,
                    Some(fixture.backend.boss.document().personas[0].id),
                    "prerequisite repair".into(),
                    None,
                    false,
                    EmployeeGoal::Errand,
                    None,
                )
                .unwrap();
            if index == 0 {
                employee.session_id = root;
                employee.identity.id = root;
            } else if let Some(override_supervisor) = repair_supervisor(index) {
                employee.supervisor_id = override_supervisor;
            }
            employee.set_lifecycle(EmployeeLifecycle::Expired, 1);
            employee.ticket = Some(
                serde_json::from_value::<SummonTicket>(serde_json::json!({
                    "sequence": index, "generation": 1, "provider": "codex", "model": "gpt-6.1-sol",
                    "prompt": "repair prerequisite", "project": fixture.repo,
                    "workspace": "adopt", "adoptWorktree": workspace,
                    "outcomeId": fixture.outcome_id
                }))
                .unwrap(),
            );
            if index > 0 {
                employee.assignment = Some(Assignment {
                    outcome_id: fixture.outcome_id,
                    after_success: "retry original chain".into(),
                    finishes_outcome: false,
                    prerequisites: vec![],
                });
            }
            let id = employee.session_id;
            fixture.backend.boss.add_employee(employee).unwrap();
            if index > 0 {
                let adoption = fixture
                    .backend
                    .resolve_worktree_adoption(&fixture.repo, &workspace, Some(id))
                    .unwrap();
                let mut session =
                    crate::model::AgentSession::new(fixture.project_id, ProviderKind::Codex);
                session.id = id;
                session.workspace = adoption.workspace;
                fixture.backend.task_state.lock().sessions.push(session);
                owners.push(id);
            }
        }
        let repaired = fixture.commit(&workspace, "prerequisite.txt", "repair");
        let chain = fixture.show(chain.id);
        (fixture, chain, workspace, repaired, owners)
    }

    #[test]
    fn retry_repair_lineage_admits_released_out_of_chain_repairs_and_fences_adoption() {
        let (fixture, chain, workspace, repaired, owners) = repair_fixture();
        let original = serde_json::to_value(&chain.steps[0].attempts[0]).unwrap();
        let proof = fixture
            .backend
            .chain_retry_verification(chain.id, &repaired)
            .unwrap()
            .unwrap();
        assert_eq!(proof.workspace_lineage.as_ref().unwrap().owners, owners);
        let retry = fixture
            .backend
            .boss
            .chain_operation_with_retry_preflight(
                None,
                ChainOperation::Control {
                    chain_id: chain.id,
                    expected_revision: chain.revision,
                    control: ChainControl::Retry,
                    source_revision: Some(ChainBaseSource::Bare(repaired.clone())),
                },
                None,
                None,
                Some(proof),
            )
            .unwrap();
        let mut expected_original = original;
        expected_original["reconciledRetryRevision"] = repaired.clone().into();
        assert_eq!(
            serde_json::to_value(&retry.steps[0].attempts[0]).unwrap(),
            expected_original
        );
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(
            inputs.adopt_worktree,
            Some(dunce::canonicalize(&workspace).unwrap())
        );
        assert_eq!(inputs.expected_starting_revision, Some(repaired.clone()));
        // Restart-safe proof and immutable starting/source/result facts.
        let persisted: EmployeeChain =
            serde_json::from_value(serde_json::to_value(&retry).unwrap()).unwrap();
        assert_eq!(
            persisted.steps[0].attempts[1]
                .retry_workspace_lineage
                .as_ref()
                .unwrap()
                .owners,
            owners
        );
        let adopter = Uuid::new_v4();
        fixture.observe(
            &retry,
            ChainObservation::Admitted {
                employee_id: adopter,
            },
        );
        // Clean descendant drift still cannot replace an explicitly captured source.
        let moved = fixture.commit(&workspace, "later.txt", "later");
        assert_ne!(moved, repaired);
        assert!(fixture.backend.chain_successor_inputs(chain.id).is_err());
        assert!(
            fixture
                .backend
                .resolve_worktree_adoption(&fixture.repo, &workspace, Some(adopter))
                .is_err()
        );
        git(&workspace, &["checkout", "--detach", &repaired]);
        // A new, fully released repair is nevertheless a concurrent owner change.
        let last = *owners.last().unwrap();
        if let SessionWorkspace::Worktree { adopted_by, .. } = &mut fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|session| session.id == last)
            .unwrap()
            .workspace
        {
            *adopted_by = Some(Uuid::new_v4());
        }
        assert!(
            fixture
                .backend
                .resolve_worktree_adoption(&fixture.repo, &workspace, Some(adopter))
                .is_err()
        );
        if let SessionWorkspace::Worktree { adopted_by, .. } = &mut fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|session| session.id == last)
            .unwrap()
            .workspace
        {
            *adopted_by = None;
        }
        let adopted = fixture
            .backend
            .resolve_worktree_adoption(&fixture.repo, &workspace, Some(adopter))
            .unwrap();
        assert_eq!(adopted.workspace.path(), Some(workspace.as_path()));
        // Crash replay between adopted_by and the adopter's workspace write.
        fixture
            .backend
            .resolve_worktree_adoption(&fixture.repo, &workspace, Some(adopter))
            .unwrap();
    }

    #[test]
    fn retry_repair_lineage_refuses_unrelated_active_foreign_and_stale_owners() {
        let (fixture, chain, workspace, repaired, owners) = repair_fixture();
        let owner = *owners.last().unwrap();
        let before = serde_json::to_value(fixture.show(chain.id)).unwrap();
        // Ticket claims, outcome, adoption source, and lifecycle are independent guards.
        for fault in ["lease", "pending lease", "outcome", "workspace"] {
            let old = fixture
                .backend
                .boss
                .employee(owner)
                .unwrap()
                .ticket
                .unwrap();
            fixture
                .backend
                .boss
                .update_employee_ticket(owner, |ticket| match fault {
                    "lease" => ticket.reservation = Some(Uuid::new_v4()),
                    "pending lease" => ticket.pending_reservation = Some(Uuid::new_v4()),
                    "outcome" => ticket.outcome_id = Some(Uuid::new_v4()),
                    _ => ticket.adopt_worktree = Some(fixture.repo.clone()),
                })
                .unwrap();
            assert!(
                fixture
                    .backend
                    .chain_retry_verification(chain.id, &repaired)
                    .is_err(),
                "{fault}"
            );
            fixture
                .backend
                .boss
                .update_employee_ticket(owner, |ticket| *ticket = old)
                .unwrap();
        }
        // A live intermediate owner also blocks an otherwise released endpoint.
        fixture
            .backend
            .boss
            .requeue_employee(
                owners[1],
                fixture
                    .backend
                    .boss
                    .employee(owners[1])
                    .unwrap()
                    .ticket
                    .unwrap(),
                |_| {},
            )
            .unwrap();
        assert!(
            fixture
                .backend
                .chain_retry_verification(chain.id, &repaired)
                .is_err()
        );
        fixture.backend.boss.expire(owners[1]).unwrap();
        // Project and graph changes cannot be justified by matching Git ancestry.
        let original_project = fixture.project_id;
        fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|session| session.id == owner)
            .unwrap()
            .project_id = Uuid::new_v4();
        assert!(
            fixture
                .backend
                .chain_retry_verification(chain.id, &repaired)
                .is_err()
        );
        fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|session| session.id == owner)
            .unwrap()
            .project_id = original_project;
        let mut state = fixture.backend.task_state.lock();
        let middle = state
            .sessions
            .iter_mut()
            .find(|session| session.id == owners[1])
            .unwrap();
        let original_workspace = middle.workspace.clone();
        if let SessionWorkspace::Worktree { adopted_by, .. } = &mut middle.workspace {
            *adopted_by = None;
        }
        drop(state);
        assert!(
            fixture
                .backend
                .chain_retry_verification(chain.id, &repaired)
                .is_err()
        );
        fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|session| session.id == owners[1])
            .unwrap()
            .workspace = original_workspace;
        let proof = fixture
            .backend
            .chain_retry_verification(chain.id, &repaired)
            .unwrap()
            .unwrap();
        assert!(
            fixture
                .backend
                .boss
                .chain_operation_with_retry_preflight(
                    None,
                    ChainOperation::Control {
                        chain_id: chain.id,
                        expected_revision: chain.revision - 1,
                        control: ChainControl::Retry,
                        source_revision: Some(ChainBaseSource::Bare(repaired))
                    },
                    None,
                    None,
                    Some(proof)
                )
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(fixture.show(chain.id)).unwrap(),
            before
        );
        assert_eq!(git(&workspace, &["status", "--porcelain"]), "");
    }

    /// A repair admitted by a later Boss generation or a planning session
    /// still answers to the step's supervisor — the recorded supervisor id a
    /// rotation stranded cannot stand in for the principal. Supervisors
    /// outside the Boss principal still refuse.
    #[test]
    fn retry_repair_lineage_admits_repairs_across_boss_principal_sessions() {
        let rotated = Uuid::new_v4();
        let planning = Uuid::new_v4();
        let (fixture, chain, _workspace, repaired, owners) =
            repair_fixture_with(|index| match index {
                1 => Some(rotated),
                _ => Some(planning),
            });
        let root_supervisor = fixture
            .backend
            .boss
            .employee(owners[0])
            .unwrap()
            .supervisor_id;
        let mut journal = waku_boss::boss_rotation::RotationJournal {
            active_session_id: Some(root_supervisor),
            ..Default::default()
        };
        journal
            .begin(Uuid::new_v4(), root_supervisor, rotated, 1)
            .unwrap();
        journal.commit(2).unwrap();
        journal
            .persist(&fixture.backend.boss_journal_path())
            .unwrap();
        fixture
            .backend
            .boss
            .add_plan(waku_protocol::boss::BossPlan {
                id: Uuid::new_v4(),
                session_id: planning,
                plan_file: "plans/repair.md".into(),
                idea: "repair plan".into(),
                finalized_at: None,
                approved_revisions: Vec::new(),
                items: Vec::new(),
                outcome: None,
                history: Vec::new(),
            })
            .unwrap();
        let proof = fixture
            .backend
            .chain_retry_verification(chain.id, &repaired)
            .unwrap()
            .unwrap();
        assert_eq!(proof.workspace_lineage.as_ref().unwrap().owners, owners);

        // A supervisor outside the Boss principal still refuses.
        let (fixture, chain, _workspace, repaired, _owners) =
            repair_fixture_with(|index| (index == 1).then(Uuid::new_v4));
        let error = fixture
            .backend
            .chain_retry_verification(chain.id, &repaired)
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("different supervisor"),
            "error: {error:#}"
        );
    }

    #[test]
    fn local_artifact_retry_reconciles_current_source_without_resetting_checkout() {
        for explicit in [false, true] {
            let fixture = Fixture::new();
            let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
            let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
            step.workspace = AgentWorkspace::Local;
            step.required_checks = vec!["P4 plan exit criteria".into()];
            step.required_artifacts = vec!["P4 phase evidence and successor handoff".into()];
            let chain = fixture.create(vec![step]);
            let mut result = success_result();
            result.disposition = ResultDisposition::Failed;
            let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
            let chain = fixture.seal(&chain);
            fixture.observe(
                &chain,
                ChainObservation::EffectsReconciled {
                    source_revision: base.clone(),
                },
            );
            let mut chain = fixture.show(chain.id);
            if explicit {
                // Reproduce recovery after the old dispatcher failed before
                // binding an employee. Earlier workspace effects still matter.
                chain = fixture
                    .backend
                    .boss
                    .chain_operation(
                        None,
                        ChainOperation::Control {
                            chain_id: chain.id,
                            expected_revision: chain.revision,
                            control: ChainControl::Retry,
                            source_revision: None,
                        },
                    )
                    .unwrap();
                fixture.observe(
                    &chain,
                    ChainObservation::LaunchError {
                        reason: "old local retry admission failed".into(),
                    },
                );
                fixture.observe(
                    &chain,
                    ChainObservation::Released {
                        execution: true,
                        workspace: true,
                    },
                );
                chain = fixture.seal(&fixture.show(chain.id));
                fixture.observe(
                    &chain,
                    ChainObservation::EffectsReconciled {
                        source_revision: base.clone(),
                    },
                );
                chain = fixture.show(chain.id);
                assert!(
                    chain.steps[0]
                        .attempts
                        .last()
                        .unwrap()
                        .employee_id
                        .is_none()
                );
            }
            let source = if explicit {
                fixture.commit(&fixture.repo, "repair.txt", "repair")
            } else {
                base.clone()
            };
            fs::write(fixture.repo.join("unrelated.txt"), "preserve me").unwrap();
            let status = git(&fixture.repo, &["status", "--porcelain"]);
            let mut unreleased = chain.steps[0].attempts.clone();
            unreleased[0].execution_released = false;
            assert!(
                fixture
                    .backend
                    .verify_local_retry_source(&chain.steps[0], &unreleased, &source)
                    .is_err()
            );
            let mut foreign = chain.steps[0].attempts.clone();
            foreign[0].workspace = Some(fixture.root.display().to_string());
            assert!(
                fixture
                    .backend
                    .verify_local_retry_source(&chain.steps[0], &foreign, &source)
                    .is_err()
            );
            let proof = if explicit {
                Some(
                    fixture
                        .backend
                        .chain_retry_verification(chain.id, &source)
                        .unwrap()
                        .unwrap(),
                )
            } else {
                None
            };
            let retry = fixture
                .backend
                .boss
                .chain_operation_with_retry_preflight(
                    None,
                    ChainOperation::Control {
                        chain_id: chain.id,
                        expected_revision: chain.revision,
                        control: ChainControl::Retry,
                        source_revision: explicit.then(|| ChainBaseSource::Bare(source.clone())),
                    },
                    None,
                    None,
                    proof,
                )
                .unwrap();
            assert_eq!(
                serde_json::to_value(&retry.steps[0].definition).unwrap(),
                serde_json::to_value(&chain.steps[0].definition).unwrap()
            );
            assert_eq!(
                retry.steps[0].attempts.len(),
                chain.steps[0].attempts.len() + 1
            );
            assert_ne!(
                retry.steps[0].attempts[0].dispatch_id,
                retry.steps[0].attempts[1].dispatch_id
            );
            let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
            assert_eq!(inputs.workspace, AgentWorkspace::Local);
            assert!(inputs.adopt_worktree.is_none());
            assert!(inputs.base_revision.is_none());
            assert_eq!(inputs.expected_starting_revision, Some(source.clone()));
            assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD"]), source);
            assert_eq!(git(&fixture.repo, &["status", "--porcelain"]), status);
            assert_eq!(
                fs::read_to_string(fixture.repo.join("unrelated.txt")).unwrap(),
                "preserve me"
            );
            fixture.commit(&fixture.repo, "drift.txt", "drift");
            assert!(fixture.backend.chain_successor_inputs(chain.id).is_err());
        }
    }

    #[test]
    fn explicit_retry_reconciles_only_clean_original_descendant_repairs() {
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let workspace = fixture.managed_worktree(&base);
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::CommittedWorkspace),
            fixture.step(HandoffBoundary::Landed),
        ]);
        let mut result = success_result();
        result.disposition = ResultDisposition::Failed;
        let chain = fixture.finish_attempt(&chain, &workspace, result);
        let employee = chain.steps[0].attempts[0].employee_id.unwrap();
        fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|s| s.workspace.path() == Some(workspace.as_path()))
            .unwrap()
            .id = employee;
        let chain = fixture.seal(&chain);
        fixture.observe(
            &chain,
            ChainObservation::EffectsReconciled {
                source_revision: base.clone(),
            },
        );
        let chain = fixture.show(chain.id);
        let repaired = fixture.commit(&workspace, "repair.txt", "repair");
        let action = || ChainOperation::Control {
            chain_id: chain.id,
            expected_revision: chain.revision,
            control: ChainControl::Retry,
            source_revision: Some(ChainBaseSource::Bare(repaired.clone())),
        };
        // The reducer alone still rejects unverified effects.
        assert!(
            fixture
                .backend
                .boss
                .chain_operation(None, action())
                .is_err()
        );
        fs::write(workspace.join("dirty.txt"), "dirty").unwrap();
        assert!(
            fixture
                .backend
                .chain_retry_verification(chain.id, &repaired)
                .is_err()
        );
        fs::remove_file(workspace.join("dirty.txt")).unwrap();
        assert!(
            fixture
                .backend
                .chain_retry_verification(chain.id, &base)
                .unwrap()
                .is_none()
        );
        assert!(
            fixture
                .backend
                .chain_retry_verification(chain.id, &"0".repeat(40))
                .is_err()
        );
        git(&workspace, &["checkout", "--orphan", "unrelated"]);
        git(&workspace, &["commit", "--allow-empty", "-m", "unrelated root"]);
        let unrelated = git(&workspace, &["rev-parse", "HEAD"]);
        assert!(
            fixture.backend.chain_retry_verification(chain.id, &unrelated)
                .err().unwrap().to_string().contains("descend")
        );
        git(&workspace, &["checkout", "--detach", &repaired]);
        let proof = fixture
            .backend
            .chain_retry_verification(chain.id, &repaired)
            .unwrap()
            .unwrap();
        // Changed ownership invalidates verification before control is admitted.
        fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|s| s.id == employee)
            .unwrap()
            .id = Uuid::new_v4();
        assert!(
            fixture
                .backend
                .chain_retry_verification(chain.id, &repaired)
                .is_err()
        );
        fixture
            .backend
            .task_state
            .lock()
            .sessions
            .iter_mut()
            .find(|s| s.workspace.path() == Some(workspace.as_path()))
            .unwrap()
            .id = employee;
        // A stale control cannot consume a previously valid proof.
        let mut stale = action();
        if let ChainOperation::Control {
            expected_revision, ..
        } = &mut stale
        {
            *expected_revision -= 1;
        }
        assert!(
            fixture
                .backend
                .boss
                .chain_operation_with_retry_preflight(None, stale, None, None, Some(proof))
                .is_err()
        );
        fixture
            .backend
            .handle_boss_operation(
                None,
                waku_protocol::boss::BossOperation::Chain { action: action() },
                &crate::EventSink::detached(),
            )
            .unwrap();
        let retried = fixture.show(chain.id);
        assert_eq!(retried.steps[0].attempts.len(), 2);
        assert_eq!(
            retried.steps[0].attempts[0]
                .reconciled_retry_revision
                .as_deref(),
            Some(repaired.as_str())
        );
        assert_eq!(
            retried.steps[0].attempts[1].starting_revision.as_deref(),
            Some(repaired.as_str())
        );
        assert!(
            fixture
                .backend
                .handle_boss_operation(
                    None,
                    waku_protocol::boss::BossOperation::Chain { action: action() },
                    &crate::EventSink::detached()
                )
                .is_err()
        );
        assert_eq!(fixture.show(chain.id).steps[0].attempts.len(), 2);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(
            inputs.adopt_worktree,
            Some(dunce::canonicalize(&workspace).unwrap())
        );
        assert_eq!(inputs.expected_starting_revision, Some(repaired));
    }

    #[test]
    fn committed_workspace_retry_preserves_original_commits_and_dirty_work() {
        let mut fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let workspace = fixture.managed_worktree(&base);
        let committed = fixture.commit(&workspace, "committed.txt", "retained");
        fs::write(workspace.join("dirty.txt"), "uncommitted").unwrap();
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::CommittedWorkspace),
            fixture.step(HandoffBoundary::Landed),
        ]);
        let mut result = success_result();
        result.disposition = ResultDisposition::Failed;
        let chain = fixture.finish_attempt(&chain, &workspace, result);
        let employee = chain.steps[0].attempts[0].employee_id.unwrap();
        {
            let mut state = fixture.backend.task_state.lock();
            state
                .sessions
                .iter_mut()
                .find(|s| s.workspace.path() == Some(workspace.as_path()))
                .unwrap()
                .id = employee;
        }
        let chain = fixture.seal(&chain);
        fixture.observe(
            &chain,
            ChainObservation::EffectsReconciled {
                source_revision: base,
            },
        );
        // Re-open persisted daemon state before the public retry, as after
        // a restart; no production runtime or provider is involved.
        {
            let mut state = fixture.backend.task_state.lock();
            state
                .sessions
                .iter_mut()
                .find(|session| session.id == employee)
                .unwrap()
                .provider_cursor = Some(crate::model::ProviderResumeCursor::Codex {
                thread_id: "fixture-thread".into(),
            });
            state.mark_session_dirty(employee);
            fixture.backend.task_store.save(&mut state).unwrap();
        }
        crate::Backend::shutdown(&fixture.backend);
        fixture.backend = WakuBackend::new(
            DaemonSettingsStore::open(fixture.root.join("settings.json")).unwrap(),
            StateStore::daemon(fixture.root.join("app.db")),
        )
        .unwrap();
        let chain = fixture.show(chain.id);
        fixture
            .backend
            .handle_boss_operation(
                None,
                waku_protocol::boss::BossOperation::Chain {
                    action: ChainOperation::Control {
                        chain_id: chain.id,
                        expected_revision: chain.revision,
                        control: ChainControl::Retry,
                        source_revision: None,
                    },
                },
                &crate::EventSink::detached(),
            )
            .unwrap();
        let retry = fixture.show(chain.id);
        assert_ne!(retry.steps[0].attempts[0].id, retry.steps[0].attempts[1].id);
        assert!(
            fixture
                .backend
                .handle_boss_operation(
                    None,
                    waku_protocol::boss::BossOperation::Chain {
                        action: ChainOperation::RecordResult {
                            chain_id: chain.id,
                            attempt_id: chain.steps[0].attempts[0].id,
                            definition_version: chain.definition_version,
                            result: success_result(),
                        }
                    },
                    &crate::EventSink::detached()
                )
                .is_err()
        );
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.workspace, AgentWorkspace::Adopt);
        assert_eq!(
            inputs.adopt_worktree,
            Some(dunce::canonicalize(&workspace).unwrap())
        );
        assert_eq!(inputs.expected_starting_revision, Some(committed.clone()));
        assert_eq!(git(&workspace, &["rev-parse", "HEAD"]), committed);
        assert_eq!(
            fs::read_to_string(workspace.join("dirty.txt")).unwrap(),
            "uncommitted"
        );
        {
            let mut state = fixture.backend.task_state.lock();
            state
                .sessions
                .iter_mut()
                .find(|s| s.id == employee)
                .unwrap()
                .id = Uuid::new_v4();
        }
        assert!(
            fixture
                .backend
                .chain_successor_inputs(chain.id)
                .err()
                .unwrap()
                .to_string()
                .contains("ownership changed")
        );
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
            Some((landed.clone(), "dev".into(), Some("sub-1".into())))
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

    /// `merge submit` mints no submission id — the production shape — so a
    /// verified landing without one seals. Ancestry on the configured QA
    /// branch and the branch name itself still gate.
    #[test]
    fn landed_gate_needs_ancestry_not_a_submission_id() {
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let worktree = fixture.managed_worktree(&base);
        let landed = fixture.commit(&worktree, "unit.txt", "work");
        git(&fixture.repo, &["branch", "dev", &landed]);
        // A real commit the QA branch never contained.
        let stray = fixture.commit(&fixture.repo, "stray.txt", "stray");
        let code = |revision: &str, branch: &str| ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(landed.clone()),
            landed_revision: Some(revision.to_owned()),
            qa_branch: Some(branch.to_owned()),
            submission_id: None,
        };
        let attempt = |fixture: &Fixture, code: ChainCodeEvidence| {
            let mut result = success_result();
            result.code = Some(code);
            let chain = fixture.create(vec![
                fixture.step(HandoffBoundary::Landed),
                fixture.step(HandoffBoundary::ArtifactsReady),
            ]);
            fixture.finish_attempt(&chain, &fixture.repo, result)
        };
        let chain = attempt(&fixture, code(&landed, "dev"));
        assert_eq!(
            fixture.verification(&chain).confirmed_landing,
            Some((landed.clone(), "dev".into(), None))
        );
        let chain = fixture.seal(&chain);
        assert_eq!(
            chain.status,
            ChainStatus::Waiting,
            "reason: {:?}",
            chain.reason
        );
        for code in [code(&stray, "dev"), code(&landed, "main")] {
            let chain = attempt(&fixture, code);
            assert!(fixture.verification(&chain).confirmed_landing.is_none());
            let chain = fixture.seal(&chain);
            assert_eq!(chain.status, ChainStatus::Paused);
            assert_eq!(chain.reason.as_deref(), Some("QA landing unconfirmed"));
        }
    }

    /// `merge submit` reports no QA branch either — the daemon resolves the
    /// verification target itself — so an unreported branch claims the
    /// resolved one and a verified landing seals. A misreported branch, a
    /// non-ancestor revision, and a missing revision all still gate.
    #[test]
    fn landed_gate_resolves_an_unreported_qa_branch() {
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let worktree = fixture.managed_worktree(&base);
        let landed = fixture.commit(&worktree, "unit.txt", "work");
        git(&fixture.repo, &["branch", "dev", &landed]);
        // A real commit the QA branch never contained.
        let stray = fixture.commit(&fixture.repo, "stray.txt", "stray");
        let code = |revision: Option<&str>, branch: Option<&str>| ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(landed.clone()),
            landed_revision: revision.map(str::to_owned),
            qa_branch: branch.map(str::to_owned),
            submission_id: None,
        };
        let attempt = |fixture: &Fixture, code: ChainCodeEvidence| {
            let mut result = success_result();
            result.code = Some(code);
            let chain = fixture.create(vec![
                fixture.step(HandoffBoundary::Landed),
                fixture.step(HandoffBoundary::ArtifactsReady),
            ]);
            fixture.finish_attempt(&chain, &fixture.repo, result)
        };
        let chain = attempt(&fixture, code(Some(&landed), None));
        assert_eq!(
            fixture.verification(&chain).confirmed_landing,
            Some((landed.clone(), "dev".into(), None))
        );
        let chain = fixture.seal(&chain);
        assert_eq!(
            chain.status,
            ChainStatus::Waiting,
            "reason: {:?}",
            chain.reason
        );
        for code in [
            code(Some(&landed), Some("release")),
            code(Some(&stray), None),
            code(None, None),
        ] {
            let chain = attempt(&fixture, code);
            assert!(fixture.verification(&chain).confirmed_landing.is_none());
            let chain = fixture.seal(&chain);
            assert_eq!(chain.status, ChainStatus::Paused);
            assert_eq!(chain.reason.as_deref(), Some("QA landing unconfirmed"));
        }
    }

    /// A per-project QA override is the branch an unreported claim resolves
    /// to — ancestry on it, not the global fallback, is what seals.
    #[test]
    fn landed_gate_resolves_the_project_qa_override() {
        let fixture = Fixture::new();
        {
            let mut state = fixture.backend.task_state.lock();
            state.projects[0].qa_branch = Some("release".into());
        }
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let worktree = fixture.managed_worktree(&base);
        let landed = fixture.commit(&worktree, "unit.txt", "work");
        // The override carries the unit; the global fallback never did, so
        // resolving "dev" would fail where "release" seals.
        git(&fixture.repo, &["branch", "release", &landed]);
        git(&fixture.repo, &["branch", "dev", &base]);
        let mut result = success_result();
        result.code = Some(ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(landed.clone()),
            landed_revision: Some(landed.clone()),
            qa_branch: None,
            submission_id: None,
        });
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::Landed),
            fixture.step(HandoffBoundary::ArtifactsReady),
        ]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        assert_eq!(
            fixture.verification(&chain).confirmed_landing,
            Some((landed.clone(), "release".into(), None))
        );
        let chain = fixture.seal(&chain);
        assert_eq!(
            chain.status,
            ChainStatus::Waiting,
            "reason: {:?}",
            chain.reason
        );
        // The claim still binds: reporting the global fallback contradicts
        // the override and rejects.
        let mut result = success_result();
        result.code = Some(ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(landed.clone()),
            landed_revision: Some(landed.clone()),
            qa_branch: Some("dev".into()),
            submission_id: None,
        });
        let chain = fixture.create(vec![
            fixture.step(HandoffBoundary::Landed),
            fixture.step(HandoffBoundary::ArtifactsReady),
        ]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        assert!(fixture.verification(&chain).confirmed_landing.is_none());
        let chain = fixture.seal(&chain);
        assert_eq!(chain.status, ChainStatus::Paused);
        assert_eq!(chain.reason.as_deref(), Some("QA landing unconfirmed"));
    }

    #[test]
    fn successor_inputs_hold_the_first_step_and_reject_unaccepted_handoffs() {
        let fixture = Fixture::new();
        let chain = fixture.create(vec![fixture.step(HandoffBoundary::ArtifactsReady)]);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert!(inputs.brief.is_none());
        assert_eq!(inputs.workspace, AgentWorkspace::Worktree);
        let Some(ChainBaseSource::Bare(pinned)) =
            fixture.step(HandoffBoundary::ArtifactsReady).base_revision
        else {
            panic!("fixture declares a bare base")
        };
        assert_eq!(inputs.expected_starting_revision.as_deref(), Some(&*pinned));
        assert!(
            fixture
                .backend
                .chain_successor_inputs(Uuid::new_v4())
                .is_err()
        );
    }

    /// A bare ref resolves against the project repo when the successor
    /// inputs are computed — at admission — so a moving branch always
    /// forks at its current head while a commit id stays pinned.
    #[test]
    fn declared_refs_resolve_at_admission_not_at_create() {
        let fixture = Fixture::new();
        let first = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
        step.base_revision = Some(ChainBaseSource::Bare("main".into()));
        let chain = fixture.create(vec![step]);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.base_revision.as_deref(), Some(first.as_str()));
        assert_eq!(
            inputs.expected_starting_revision.as_deref(),
            Some(first.as_str())
        );
        // A later main commit moves the resolved base — intake pinned
        // nothing, and the stored definition still names the ref.
        let moved = fixture.commit(&fixture.repo, "moved.txt", "moved");
        let chain = fixture.show(chain.id);
        assert_eq!(
            chain.steps[0].definition.base_revision,
            Some(ChainBaseSource::Bare("main".into()))
        );
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.base_revision.as_deref(), Some(moved.as_str()));
        // A ref that resolves to nothing fails admission — the dispatch
        // pauses on the error rather than cutting at a stale guess.
        let mut bogus = fixture.step(HandoffBoundary::ArtifactsReady);
        bogus.base_revision = Some(ChainBaseSource::Bare("no-such-branch".into()));
        let chain = fixture.create(vec![bogus]);
        let error = fixture
            .backend
            .chain_successor_inputs(chain.id)
            .err()
            .expect("an unresolvable ref cannot admit");
        assert!(format!("{error:#}").contains("no-such-branch"));
        // A full commit id stays pinned verbatim.
        let mut pinned = fixture.step(HandoffBoundary::ArtifactsReady);
        pinned.base_revision = Some(ChainBaseSource::Bare(first.clone()));
        let chain = fixture.create(vec![pinned]);
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.base_revision.as_deref(), Some(first.as_str()));
    }

    /// `previousStepLanded` cuts the successor's worktree at the previous
    /// step's accepted landing — carried through the sealed handoff —
    /// rather than a revision pinned at chain create.
    #[test]
    fn previous_step_landed_bases_the_successor_on_the_landing() {
        let fixture = Fixture::new();
        let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let worktree = fixture.managed_worktree(&base);
        let landed = fixture.commit(&worktree, "unit.txt", "work");
        let mut result = success_result();
        result.code = Some(ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(landed.clone()),
            landed_revision: Some(landed.clone()),
            qa_branch: None,
            submission_id: None,
        });
        let chain = fixture.create(vec![fixture.step(HandoffBoundary::ArtifactsReady), {
            let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
            step.base_revision = Some(ChainBaseSource::Kinded(ChainBaseKind::PreviousStepLanded));
            step
        }]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        let chain = fixture.seal(&chain);
        assert_eq!(
            chain.status,
            ChainStatus::Waiting,
            "reason: {:?}",
            chain.reason
        );
        let inputs = fixture.backend.chain_successor_inputs(chain.id).unwrap();
        assert_eq!(inputs.workspace, AgentWorkspace::Worktree);
        assert_eq!(inputs.base_revision.as_deref(), Some(landed.as_str()));
        assert_eq!(
            inputs.expected_starting_revision.as_deref(),
            Some(landed.as_str())
        );
        // A producer that never reported a landing gives the source
        // nothing to resolve — admission errors instead of guessing.
        let mut result = success_result();
        result.code = Some(ChainCodeEvidence {
            workspace: worktree.display().to_string(),
            source_revision: base.clone(),
            committed_revision: Some(landed.clone()),
            landed_revision: None,
            qa_branch: None,
            submission_id: None,
        });
        let chain = fixture.create(vec![fixture.step(HandoffBoundary::ArtifactsReady), {
            let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
            step.base_revision = Some(ChainBaseSource::Kinded(ChainBaseKind::PreviousStepLanded));
            step
        }]);
        let chain = fixture.finish_attempt(&chain, &fixture.repo, result);
        let chain = fixture.seal(&chain);
        let error = fixture
            .backend
            .chain_successor_inputs(chain.id)
            .err()
            .expect("an unreported landing cannot admit");
        assert_eq!(
            format!("{error:#}"),
            "the previous step reported no landed revision"
        );
    }

    /// The kinded source needs a producer to resolve against — a first
    /// step cannot declare it.
    #[test]
    fn previous_step_landed_needs_a_predecessor() {
        let fixture = Fixture::new();
        let mut step = fixture.step(HandoffBoundary::ArtifactsReady);
        step.base_revision = Some(ChainBaseSource::Kinded(ChainBaseKind::PreviousStepLanded));
        let error = fixture
            .backend
            .boss
            .chain_operation(
                None,
                ChainOperation::Create {
                    request_id: Uuid::new_v4(),
                    definition: ChainDefinition {
                        outcome_id: fixture.outcome_id,
                        finishes_outcome: false,
                        steps: vec![step],
                    },
                },
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "previousStepLanded requires a previous step"
        );
    }
}
