//! Serialized submission of an employee's daemon-managed worktree to QA.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, anyhow, bail};

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

    let dev_worktree = find_branch_worktree(&root, branch)?.ok_or_else(|| {
        anyhow!("no checked-out worktree owns the configured QA branch {branch:?}")
    })?;
    clean(&dev_worktree)?;
    no_operation(&dev_worktree)?;
    let actual_branch = git_text(&dev_worktree, &["branch", "--show-current"])?;
    let actual_head = git_text(&dev_worktree, &["rev-parse", "--verify", "HEAD"])?;
    if actual_branch.trim() != branch || actual_head.trim() != target {
        bail!("the QA checkout changed during submission; nothing was landed, retry merge submit");
    }
    let merge = git_capture(&dev_worktree, &["merge", "--ff-only", &rebased_head])?;
    if !merge.status.success() {
        bail!(
            "could not fast-forward the QA worktree: {}",
            command_error(&merge)
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

fn find_branch_worktree(root: &Path, branch: &str) -> anyhow::Result<Option<PathBuf>> {
    let listing = git_text(root, &["worktree", "list", "--porcelain", "-z"])?;
    let mut path = None;
    for field in listing.split('\0') {
        if let Some(worktree) = field.strip_prefix("worktree ") {
            path = Some(PathBuf::from(worktree));
        } else if field == format!("branch refs/heads/{branch}") {
            return Ok(path);
        }
    }
    Ok(None)
}

fn is_ancestor(cwd: &Path, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
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
    let status = git_text(cwd, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    if !status.trim().is_empty() {
        bail!("merge submit requires a clean worktree; commit or clean its changes first");
    }
    Ok(())
}

fn no_operation(cwd: &Path) -> anyhow::Result<()> {
    for marker in [
        "rebase-merge",
        "rebase-apply",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
    ] {
        let path = git_text(cwd, &["rev-parse", "--git-path", marker])?;
        if cwd.join(path.trim()).exists() {
            bail!("a Git operation is in progress; resolve or abort it before submitting");
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

fn git_text(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
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

fn command_error(output: &std::process::Output) -> String {
    output_text(&output.stdout, &output.stderr)
}

fn output_text(stdout: &[u8], stderr: &[u8]) -> String {
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
