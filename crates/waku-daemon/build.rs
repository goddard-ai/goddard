//! Platform build metadata: the checked-out commit, republished so the
//! daemon's hello handshake can report what it was built from.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    export_commit_sha();
}

/// Republish the checked-out commit as `GODDARD_COMMIT_SHA` so development
/// builds can report the exact source each binary was built from. Unset when
/// the build host has no git checkout — a tarball build simply omits it.
///
/// A `-dirty` suffix marks tracked worktree changes: a build almost always
/// carries uncommitted edits, so a bare hash would overstate what is running.
fn export_commit_sha() {
    let Some(mut sha) = git(&["rev-parse", "--short", "HEAD"]) else {
        return;
    };
    if git(&["status", "--porcelain"]).is_some_and(|status| !status.is_empty()) {
        sha.push_str("-dirty");
    }
    println!("cargo:rustc-env=GODDARD_COMMIT_SHA={sha}");

    // Re-resolve on checkout movement: HEAD covers branch switches and
    // detached commits, the ref it names covers new commits on a branch, and
    // the index covers staging so the dirty marker stays honest. The emitted
    // env only rebuilds dependents when the sha actually changes.
    // The package's own inputs are watched too: once any rerun-if-changed is
    // emitted, Cargo drops its everything-changes default, and without these
    // a rebuild caused by a plain source edit would reuse a stale flag.
    println!("cargo:rerun-if-changed=src");
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let git_dir = std::path::PathBuf::from(git_dir);
    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
    println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
    let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD")) else {
        return;
    };
    let Some(reference) = head.trim().strip_prefix("ref: ") else {
        return;
    };
    // Branch refs live in the common dir every worktree shares; packed-refs
    // covers refs stored packed instead of loose.
    let Some(common) = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]) else {
        return;
    };
    let common = std::path::PathBuf::from(common);
    println!(
        "cargo:rerun-if-changed={}",
        common.join("packed-refs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        common.join(reference).display()
    );
}

/// `git <arguments>` trimmed to a single line, `None` when git is missing or
/// the command fails.
fn git(arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|output| !output.is_empty())
}
