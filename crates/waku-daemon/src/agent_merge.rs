//! Serialized submission of an employee's daemon-managed worktree to QA.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, bail};

#[derive(Default)]
struct SubmissionQueue {
    next: u64,
    serving: u64,
}

struct SubmissionPermit {
    queue: &'static parking_lot::Mutex<SubmissionQueue>,
    wake: &'static parking_lot::Condvar,
}

impl Drop for SubmissionPermit {
    fn drop(&mut self) {
        let mut queue = self.queue.lock();
        queue.serving = queue.serving.wrapping_add(1);
        self.wake.notify_all();
    }
}

fn submission_slot() -> SubmissionPermit {
    static QUEUE: std::sync::OnceLock<(parking_lot::Mutex<SubmissionQueue>, parking_lot::Condvar)> =
        std::sync::OnceLock::new();
    let (queue, wake) = QUEUE.get_or_init(|| {
        (
            parking_lot::Mutex::new(SubmissionQueue::default()),
            parking_lot::Condvar::new(),
        )
    });
    let mut state = queue.lock();
    let ticket = state.next;
    state.next = state.next.wrapping_add(1);
    while state.serving != ticket {
        wake.wait(&mut state);
    }
    drop(state);
    SubmissionPermit { queue, wake }
}

pub(crate) fn submit(
    project: &Path,
    source: &Path,
    recorded_base: Option<&str>,
    branch: &str,
) -> anyhow::Result<String> {
    let _permit = submission_slot();
    let root = git_text(project, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root.trim());
    let target = git_text(
        &root,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )?;
    clean(source)?;
    no_operation(source)?;
    // A submitting worktree attached to the QA branch would drive it
    // directly — its rebase and commit move the branch without any landing
    // checkout — so it must submit from a detached head like everyone else.
    let source_branch = git_text(source, &["branch", "--show-current"])?;
    if source_branch.trim() == branch {
        bail!(
            "the submitting worktree is itself checked out on the QA branch {branch:?}; detach it before submitting"
        );
    }
    let source_head = git_text(source, &["rev-parse", "--verify", "HEAD"])?;
    let source_head = source_head.trim().to_owned();
    let target = target.trim().to_owned();

    if source_head == target || is_ancestor(source, &source_head, &target)? {
        return Ok(target);
    }
    if !is_ancestor(source, &target, &source_head)? {
        let fork_base = recorded_base
            .and_then(|base| git_status(source, &["merge-base", "HEAD", base]).ok())
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|base| base.trim().to_owned());
        let output = match fork_base {
            Some(base) => git_capture(source, &["rebase", "--onto", &target, &base])?,
            None => git_capture(source, &["rebase", &target])?,
        };
        if !output.status.success() {
            bail!("{}", command_error(&output));
        }
    }

    squash_unit(source, &target)?;

    let rebased_head = git_text(source, &["rev-parse", "--verify", "HEAD"])?;
    let rebased_head = rebased_head.trim().to_owned();
    if rebased_head == target {
        return Ok(target);
    }
    verify(source, &target, &rebased_head)?;
    let final_source_head = git_text(source, &["rev-parse", "--verify", "HEAD"])?;
    if final_source_head.trim() != rebased_head {
        bail!("the submitting worktree changed during verification; QA branch was not advanced");
    }
    clean(source).context("the submitting worktree changed during verification")?;
    no_operation(source)?;

    let dev_worktrees = find_branch_worktrees(&root, branch)?;
    let [dev_worktree] = dev_worktrees.as_slice() else {
        if dev_worktrees.is_empty() {
            bail!("no checked-out worktree owns the configured QA branch {branch:?}");
        }
        // Two checkouts on the QA branch means the first `worktree list`
        // match is arbitrary — landings fast-forward whichever Git happens
        // to name and strand the other's index at an older tip, where its
        // staged state becomes a revert of everything landed since. Fail
        // closed rather than pick one.
        bail!(
            "the QA branch {branch:?} is checked out in multiple worktrees; \
             reconcile it to a single landing checkout before submitting:\n{}",
            dev_worktrees
                .iter()
                .map(|worktree| worktree.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    };
    // Unrelated untracked files do not prevent a fast-forward. Git itself
    // rejects any that would be overwritten, preserving the landing checkout.
    clean_status(dev_worktree, "no", "QA landing checkout")?;
    no_operation(dev_worktree)?;
    let actual_branch = git_text(dev_worktree, &["branch", "--show-current"])?;
    let actual_head = git_text(dev_worktree, &["rev-parse", "--verify", "HEAD"])?;
    if actual_branch.trim() != branch || actual_head.trim() != target {
        bail!("the QA checkout changed during submission; nothing was landed, retry merge submit");
    }
    let merge = git_status(dev_worktree, &["merge", "--ff-only", &rebased_head])?;
    if !merge.status.success() {
        bail!(
            "could not fast-forward the QA landing checkout at {}: {}",
            dev_worktree.display(),
            command_error(&merge)
        );
    }
    let landed_head = git_text(dev_worktree, &["rev-parse", "--verify", "HEAD"])?;
    if landed_head.trim() != rebased_head {
        bail!(
            "the QA landing checkout at {} did not fast-forward to the submitted commit",
            dev_worktree.display()
        );
    }
    Ok(rebased_head)
}

/// Goddard's QA branch keeps one commit per submitted employee unit. Rebase
/// first so patch-equivalent commits have already disappeared, then preserve
/// each original message and carry every Test-Plan trailer onto the unit.
fn squash_unit(source: &Path, target: &str) -> anyhow::Result<()> {
    let output = git_text(
        source,
        &[
            "log",
            "--format=%B%x00",
            "--reverse",
            &format!("{target}..HEAD"),
        ],
    )?;
    let messages = output
        .split('\0')
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .collect::<Vec<_>>();
    if messages.len() <= 1 {
        return Ok(());
    }
    let subject = messages[0]
        .lines()
        .find(|line| !line.trim().is_empty())
        .context("submitted commits have no subject")?;
    let plans = messages
        .iter()
        .flat_map(|message| message.lines())
        .map(str::trim)
        .filter(|line| line.starts_with("Test-Plan:"))
        .collect::<Vec<_>>();
    let first_body = messages[0].lines().skip(1).collect::<Vec<_>>().join("\n");
    let mut message = subject.to_owned();
    if !first_body.trim().is_empty() {
        message.push_str("\n\n");
        message.push_str(first_body.trim());
    }
    for note in messages.iter().skip(1) {
        message.push_str("\n\n---\n\n");
        message.push_str(note);
    }
    if !plans.is_empty() {
        message.push_str("\n\n");
        message.push_str(&plans.join("\n"));
        message.push('\n');
    }
    let reset = git_capture(source, &["reset", "--soft", target])?;
    if !reset.status.success() {
        bail!(
            "could not stage the submitted unit for squash: {}",
            command_error(&reset)
        );
    }
    let mut child = Command::new("git")
        .args(["commit", "--cleanup=verbatim", "-F", "-"])
        .current_dir(source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start the unit squash commit")?;
    std::io::Write::write_all(
        child.stdin.as_mut().expect("piped stdin"),
        message.as_bytes(),
    )?;
    drop(child.stdin.take());
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "could not create the submitted unit commit: {}",
            command_error(&output)
        );
    }
    Ok(())
}

fn verify(source: &Path, base: &str, head: &str) -> anyhow::Result<()> {
    git_success(source, &["diff", "--check", &format!("{base}..{head}")])?;
    let configured = git_capture(
        source,
        &["config", "--null", "--get-all", "agent-merge.verify"],
    )?;
    if configured.status.code() != Some(0) && configured.status.code() != Some(1) {
        bail!(
            "could not read agent-merge.verify: {}",
            command_error(&configured)
        );
    }
    let configured =
        String::from_utf8(configured.stdout).context("agent-merge.verify is not UTF-8")?;
    for command in configured.split('\0').filter(|command| !command.is_empty()) {
        let result = run_verification_command(source, command)?;
        if !result.status.success() {
            bail!(
                "verification failed ({command:?}): {}",
                output_text(&result.stdout, &result.stderr)
            );
        }
        let actual_head = git_text(source, &["rev-parse", "--verify", "HEAD"])?;
        if actual_head.trim() != head {
            bail!("verification changed HEAD; QA branch was not advanced");
        }
        clean(source).context("verification changed the submitting worktree")?;
        no_operation(source)?;
    }
    Ok(())
}

#[cfg(unix)]
fn run_verification_command(cwd: &Path, command: &str) -> anyhow::Result<std::process::Output> {
    Command::new("sh")
        .args(["-c", command])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("could not run verification command {command:?}"))
}

#[cfg(windows)]
fn run_verification_command(cwd: &Path, command: &str) -> anyhow::Result<std::process::Output> {
    Command::new("cmd")
        .args(["/C", command])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("could not run verification command {command:?}"))
}

fn find_branch_worktrees(root: &Path, branch: &str) -> anyhow::Result<Vec<PathBuf>> {
    let listing = git_text(root, &["worktree", "list", "--porcelain", "-z"])?;
    let mut worktrees = Vec::new();
    let mut path = None;
    for field in listing.split('\0') {
        if let Some(worktree) = field.strip_prefix("worktree ") {
            path = Some(PathBuf::from(worktree));
        } else if field == format!("branch refs/heads/{branch}")
            && let Some(path) = path.take()
        {
            worktrees.push(path);
        }
    }
    Ok(worktrees)
}

pub(crate) fn is_ancestor(cwd: &Path, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
    let output = git_capture(cwd, &["merge-base", "--is-ancestor", ancestor, descendant])?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "could not compare submitted history: {}",
            command_error(&output)
        ),
    }
}

fn clean(cwd: &Path) -> anyhow::Result<()> {
    clean_status(cwd, "all", "submitting worktree")
}

pub(crate) fn clean_status(
    cwd: &Path,
    untracked_files: &str,
    checkout: &str,
) -> anyhow::Result<()> {
    let status = git_text(
        cwd,
        &[
            "status",
            "--porcelain=v1",
            &format!("--untracked-files={untracked_files}"),
        ],
    )?;
    if !status.trim().is_empty() {
        bail!(
            "merge submit requires a clean {checkout} at {}; commit or clean these changes first:\n{}",
            cwd.display(),
            status.trim_end()
        );
    }
    Ok(())
}

pub(crate) fn no_operation(cwd: &Path) -> anyhow::Result<()> {
    for marker in [
        "rebase-merge",
        "rebase-apply",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
    ] {
        let path = git_text(cwd, &["rev-parse", "--git-path", marker])?;
        if cwd.join(path.trim()).exists() {
            bail!(
                "a Git operation is in progress in checkout {} ({marker}); resolve or abort it before submitting",
                cwd.display()
            );
        }
    }
    Ok(())
}

fn git_status(cwd: &Path, args: &[&str]) -> anyhow::Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("could not run git {}", args.join(" ")))
}

fn git_capture(cwd: &Path, args: &[&str]) -> anyhow::Result<std::process::Output> {
    let output = git_status(cwd, args)?;
    if output.status.success() || output.status.code() == Some(1) {
        Ok(output)
    } else {
        bail!("git {} failed: {}", args.join(" "), command_error(&output));
    }
}

pub(crate) fn git_text(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = git_status(cwd, args)?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), command_error(&output));
    }
    String::from_utf8(output.stdout).context("git returned non-UTF-8 output")
}

fn git_success(cwd: &Path, args: &[&str]) -> anyhow::Result<()> {
    let output = git_status(cwd, args)?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), command_error(&output));
    }
    Ok(())
}

pub(crate) fn command_error(output: &std::process::Output) -> String {
    output_text(&output.stdout, &output.stderr)
}

pub(crate) fn output_text(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
    match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("{stderr}\n{stdout}"),
        (false, true) => stdout,
        (true, false) => stderr,
        (true, true) => "command exited unsuccessfully".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    struct TempRepo(PathBuf);

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Path text the way submission errors render it: git's porcelain
    /// output uses forward slashes even on Windows.
    fn slashed(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            command_error(&output)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn commit(cwd: &Path, file: &str, contents: &str, message: &str) -> String {
        std::fs::write(cwd.join(file), contents).unwrap();
        git(cwd, &["add", file]);
        git(cwd, &["commit", "-m", message]);
        git(cwd, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn landing_fixture() -> (TempRepo, PathBuf, PathBuf, String) {
        // CI's %TEMP% resolves through an 8.3 name (RUNNER~1) while git
        // reports the canonical path — normalize once so error text matches.
        let root_dir = std::env::temp_dir().join(format!("goddard-merge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root_dir).unwrap();
        let root = TempRepo(dunce::canonicalize(&root_dir).unwrap());
        let project = root.0.join("project");
        let employee = root.0.join("employee");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "--initial-branch=dev"]);
        git(&project, &["config", "core.autocrlf", "false"]);
        git(&project, &["config", "user.name", "Goddard Test"]);
        git(&project, &["config", "user.email", "test@goddard.local"]);
        let base = commit(&project, "base.txt", "base\n", "base");
        git(
            &project,
            &[
                "worktree",
                "add",
                "--detach",
                employee.to_str().unwrap(),
                &base,
            ],
        );
        commit(&employee, "unit.txt", "unit\n", "fix: employee unit");
        (root, project, employee, base)
    }

    #[test]
    fn submission_preserves_unrelated_untracked_landing_files() {
        let (_root, project, employee, base) = landing_fixture();
        std::fs::create_dir_all(project.join(".ns-vite-build")).unwrap();
        std::fs::write(project.join(".ns-vite-build/output.js"), "build\n").unwrap();
        std::fs::write(project.join("build.aar"), "android\n").unwrap();
        assert!(git(&employee, &["status", "--porcelain"]).is_empty());

        let landed = submit(&project, &employee, Some(&base), "dev").unwrap();

        assert_eq!(git(&project, &["rev-parse", "HEAD"]).trim(), landed);
        assert_eq!(
            std::fs::read_to_string(project.join("unit.txt")).unwrap(),
            "unit\n"
        );
        assert_eq!(
            std::fs::read_to_string(project.join(".ns-vite-build/output.js")).unwrap(),
            "build\n"
        );
        assert_eq!(
            std::fs::read_to_string(project.join("build.aar")).unwrap(),
            "android\n"
        );
    }

    #[test]
    fn submission_rejects_tracked_landing_changes_with_checkout_and_files() {
        let (_root, project, employee, base) = landing_fixture();
        std::fs::write(project.join("base.txt"), "local work\n").unwrap();
        for staged in [false, true] {
            if staged {
                git(&project, &["add", "base.txt"]);
            }
            let error = submit(&project, &employee, Some(&base), "dev")
                .unwrap_err()
                .to_string()
                .replace('\\', "/");
            assert!(error.contains("QA landing checkout"), "{error}");
            assert!(error.contains(&slashed(&project)), "{error}");
            assert!(error.contains("base.txt"), "{error}");
            assert_eq!(git(&project, &["rev-parse", "HEAD"]).trim(), base);
            assert_eq!(
                std::fs::read_to_string(project.join("base.txt")).unwrap(),
                "local work\n"
            );
        }
    }

    #[test]
    fn submission_still_rejects_untracked_employee_files() {
        let (_root, project, employee, base) = landing_fixture();
        std::fs::write(employee.join("unfinished.txt"), "local work\n").unwrap();

        let error = submit(&project, &employee, Some(&base), "dev")
            .unwrap_err()
            .to_string()
            .replace('\\', "/");

        assert!(error.contains(&slashed(&employee)), "{error}");
        assert!(error.contains("unfinished.txt"), "{error}");
        assert_eq!(git(&project, &["rev-parse", "HEAD"]).trim(), base);
    }

    #[test]
    fn submission_names_checkout_with_git_operation_in_progress() {
        let (_root, project, employee, base) = landing_fixture();
        for checkout in [&employee, &project] {
            let marker = git(checkout, &["rev-parse", "--git-path", "MERGE_HEAD"]);
            let marker = checkout.join(marker.trim());
            std::fs::write(&marker, format!("{base}\n")).unwrap();
            assert!(git(checkout, &["status", "--porcelain"]).is_empty());

            let error = submit(&project, &employee, Some(&base), "dev")
                .unwrap_err()
                .to_string()
                .replace('\\', "/");

            assert!(error.contains(&slashed(&checkout)), "{error}");
            assert!(error.contains("MERGE_HEAD"), "{error}");
            assert_eq!(git(&project, &["rev-parse", "HEAD"]).trim(), base);
            std::fs::remove_file(marker).unwrap();
        }
    }

    #[test]
    fn submission_preserves_untracked_files_that_would_be_overwritten() {
        let (_root, project, employee, base) = landing_fixture();
        std::fs::write(project.join("unit.txt"), "local work\n").unwrap();

        let error = submit(&project, &employee, Some(&base), "dev")
            .unwrap_err()
            .to_string()
            .replace('\\', "/");

        assert!(error.contains(&slashed(&project)), "{error}");
        assert!(error.contains("unit.txt"), "{error}");
        assert_eq!(git(&project, &["rev-parse", "HEAD"]).trim(), base);
        assert_eq!(
            std::fs::read_to_string(project.join("unit.txt")).unwrap(),
            "local work\n"
        );
    }

    #[test]
    fn submission_refuses_a_second_qa_checkout() {
        let (_root, project, employee, base) = landing_fixture();
        // `symbolic-ref` bypasses the checked-out-branch guard, leaving two
        // worktrees that both claim `dev` — landing in the first `worktree
        // list` match would strand the other's index as a staged revert.
        let dupe = project.with_file_name("dupe");
        git(
            &project,
            &["worktree", "add", "--detach", dupe.to_str().unwrap(), "dev"],
        );
        git(&dupe, &["symbolic-ref", "HEAD", "refs/heads/dev"]);
        assert!(git(&dupe, &["status", "--porcelain"]).is_empty());

        let error = submit(&project, &employee, Some(&base), "dev")
            .unwrap_err()
            .to_string()
            .replace('\\', "/");

        assert!(error.contains("multiple worktrees"), "{error}");
        assert!(error.contains(&slashed(&project)), "{error}");
        assert!(error.contains(&slashed(&dupe)), "{error}");
        assert_eq!(git(&project, &["rev-parse", "dev"]).trim(), base);
        assert!(employee.join("unit.txt").exists());
    }

    #[test]
    fn submission_refuses_a_submitting_worktree_on_the_qa_branch() {
        let (_root, project, employee, base) = landing_fixture();
        // Attached to the QA branch, the worktree's own commits and rebases
        // drive it directly — no landing checkout, no verification gate.
        git(&employee, &["symbolic-ref", "HEAD", "refs/heads/dev"]);
        git(&employee, &["reset", "--hard", "HEAD"]);
        let unit = commit(&employee, "unit2.txt", "unit2\n", "feat: employee unit");
        assert_eq!(git(&project, &["rev-parse", "dev"]).trim(), unit);

        let error = submit(&project, &employee, Some(&base), "dev")
            .unwrap_err()
            .to_string()
            .replace('\\', "/");

        assert!(error.contains("submitting worktree"), "{error}");
        assert!(error.contains("QA branch"), "{error}");
        assert_eq!(git(&project, &["rev-parse", "dev"]).trim(), unit);
    }

    #[test]
    fn concurrent_submissions_serialize_deduplicate_and_preserve_trailers() {
        let root =
            TempRepo(std::env::temp_dir().join(format!("goddard-merge-{}", uuid::Uuid::new_v4())));
        let project = root.0.join("project");
        let dev = root.0.join("dev");
        let employee_a = root.0.join("employee-a");
        let employee_b = root.0.join("employee-b");
        let employee_c = root.0.join("employee-c");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "--initial-branch=main"]);
        git(&project, &["config", "user.name", "Goddard Test"]);
        git(&project, &["config", "user.email", "test@goddard.local"]);
        let base = commit(&project, "base.txt", "base\n", "base");
        git(&project, &["branch", "dev"]);
        git(&project, &["worktree", "add", dev.to_str().unwrap(), "dev"]);
        git(
            &project,
            &[
                "worktree",
                "add",
                "--detach",
                employee_a.to_str().unwrap(),
                &base,
            ],
        );
        git(
            &project,
            &[
                "worktree",
                "add",
                "--detach",
                employee_c.to_str().unwrap(),
                &base,
            ],
        );
        git(
            &project,
            &[
                "worktree",
                "add",
                "--detach",
                employee_b.to_str().unwrap(),
                &base,
            ],
        );
        commit(&employee_a, "a.txt", "a\n", "feat: employee A");
        commit(
            &employee_a,
            "a-notes.txt",
            "notes\n",
            "docs: employee A details\n\nTest-Plan: inspect A on dev",
        );
        commit(&employee_b, "b.txt", "b\n", "feat: employee B");

        // The primary checkout is not the landing checkout and must be left alone.
        std::fs::write(project.join("base.txt"), "primary local work\n").unwrap();
        std::fs::write(project.join("build.aar"), "android\n").unwrap();
        std::fs::write(project.join(".git/MERGE_HEAD"), format!("{base}\n")).unwrap();

        let start = Arc::new(Barrier::new(3));
        let jobs = [employee_a.clone(), employee_b.clone()].map(|source| {
            let start = start.clone();
            let project = project.clone();
            let base = base.clone();
            std::thread::spawn(move || {
                start.wait();
                submit(&project, &source, Some(&base), "dev").unwrap()
            })
        });
        start.wait();
        for job in jobs {
            job.join().unwrap();
        }

        assert!(dev.join("a.txt").exists());
        assert!(dev.join("b.txt").exists());
        assert_eq!(
            std::fs::read_to_string(project.join("base.txt")).unwrap(),
            "primary local work\n"
        );
        assert!(project.join(".git/MERGE_HEAD").exists());
        assert_eq!(
            git(&dev, &["rev-list", "--count", &format!("{base}..HEAD")]).trim(),
            "2"
        );
        let message = git(&dev, &["log", "--format=%B", "--", "a.txt"]);
        assert!(message.contains("Test-Plan: inspect A on dev"));

        let before_retry = git(&dev, &["rev-parse", "HEAD"]);
        let retry = submit(&project, &employee_a, Some(&base), "dev").unwrap();
        assert_eq!(retry, before_retry.trim());
        assert_eq!(git(&dev, &["rev-parse", "HEAD"]), before_retry);

        commit(&employee_c, "c.txt", "c\n", "feat: employee C");
        git(
            &project,
            &["config", "--local", "agent-merge.verify", "false"],
        );
        let before_failed_verification = git(&dev, &["rev-parse", "HEAD"]);
        assert!(submit(&project, &employee_c, Some(&base), "dev").is_err());
        assert_eq!(
            git(&dev, &["rev-parse", "HEAD"]),
            before_failed_verification
        );
    }
}
