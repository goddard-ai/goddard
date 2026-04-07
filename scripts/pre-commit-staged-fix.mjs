import { execFileSync } from "node:child_process"
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { dirname } from "node:path"

/** Run a command and return its stdout as text. */
const read = (command, args) =>
  execFileSync(command, args, {
    encoding: "utf8",
  })

/** Run a command while streaming its output to the terminal. */
const run = (command, args) => {
  execFileSync(command, args, { stdio: "inherit" })
}

/** Resolve a file path inside the current worktree's Git metadata directory. */
const gitPath = (path) => read("git", ["rev-parse", "--git-path", path]).trim()

/** Return the staged files that can be fixed in place before commit. */
const getStagedFiles = () =>
  read("git", ["diff", "--name-only", "--cached", "--diff-filter=ACMR", "-z"])
    .split("\0")
    .filter(Boolean)

/** Return true when there is unstaged tracked or untracked work to hide. */
const hasUnstagedChanges = () => {
  const unstagedFiles = read("git", ["diff", "--name-only", "-z"])
  const untrackedFiles = read("git", ["ls-files", "--others", "--exclude-standard", "-z"])

  return unstagedFiles.length > 0 || untrackedFiles.length > 0
}

/** Persist the stash commit that should be restored after the commit finishes. */
const writePendingRestore = (stateFile, stashSha) => {
  mkdirSync(dirname(stateFile), { recursive: true })
  writeFileSync(stateFile, JSON.stringify({ stashSha }), "utf8")
}

/** Remove any pending restore marker after the stash is restored or discarded. */
const clearPendingRestore = (stateFile) => {
  rmSync(stateFile, { force: true })
}

/** Abort when a previous commit left hidden changes waiting to be restored. */
const assertNoPendingRestore = (stateFile) => {
  try {
    const pendingRestore = JSON.parse(readFileSync(stateFile, "utf8"))

    if (pendingRestore.stashSha) {
      throw new Error(
        `A previous pre-commit stash is still pending restore (${pendingRestore.stashSha}). Run \`bun run postcommit:restore-stash\` before committing again.`,
      )
    }
  } catch (error) {
    if (error.code === "ENOENT") {
      return
    }

    throw error
  }
}

/** Stash unstaged work so fixers only operate on the staged snapshot. */
const stashUnstagedChanges = (stateFile) => {
  run("git", [
    "stash",
    "push",
    "--keep-index",
    "--include-untracked",
    "--message",
    "goddard pre-commit backup",
  ])

  const stashSha = read("git", ["rev-parse", "--verify", "refs/stash"]).trim()
  writePendingRestore(stateFile, stashSha)

  return "stash@{0}"
}

/** Restore the hidden unstaged work when the pre-commit fix flow fails. */
const restoreStashAfterFailure = (stateFile, stashRef) => {
  try {
    run("git", ["stash", "pop", "--quiet", stashRef])
    clearPendingRestore(stateFile)
  } catch (error) {
    console.error(
      `Failed to restore the hidden unstaged changes from ${stashRef}. Git kept the stash entry so the work can be recovered with \`bun run postcommit:restore-stash\`.`,
    )
    throw error
  }
}

const pendingRestoreFile = gitPath("goddard/pre-commit-stash.json")
assertNoPendingRestore(pendingRestoreFile)

const stagedFiles = getStagedFiles()

if (stagedFiles.length === 0) {
  process.exit(0)
}

let pendingStashRef = null

if (hasUnstagedChanges()) {
  pendingStashRef = stashUnstagedChanges(pendingRestoreFile)
}

try {
  run("bunx", ["oxfmt", "--no-error-on-unmatched-pattern", ...stagedFiles])
  run("bunx", ["oxlint", "--fix", "--quiet", ...stagedFiles])
  run("git", ["add", "--", ...stagedFiles])
} catch (error) {
  if (pendingStashRef) {
    restoreStashAfterFailure(pendingRestoreFile, pendingStashRef)
  }

  throw error
}
