import { execFileSync } from "node:child_process"
import { readFileSync, rmSync } from "node:fs"

type PendingRestore = {
  stashSha?: string
}

/** Run a command and return its stdout as text. */
const read = (command: string, args: string[]) =>
  execFileSync(command, args, {
    encoding: "utf8",
  })

/** Run a command while streaming its output to the terminal. */
const run = (command: string, args: string[]) => {
  execFileSync(command, args, { stdio: "inherit" })
}

/** Resolve a file path inside the current worktree's Git metadata directory. */
const gitPath = (path: string) => read("git", ["rev-parse", "--git-path", path]).trim()

/** Return true when applying a stash would overwrite local work. */
const hasWorktreeChanges = () =>
  read("git", ["status", "--porcelain=v1", "--untracked-files=all"]).length > 0

/** Look up the current stash ref for a previously recorded stash commit. */
const findStashRef = (stashSha: string) => {
  const stashList = read("git", ["stash", "list", "--format=%gd%x00%H"])

  for (const entry of stashList.split("\n")) {
    if (!entry) {
      continue
    }

    const [stashRef, currentSha] = entry.split("\0")

    if (currentSha === stashSha) {
      return stashRef
    }
  }

  return null
}

/** Clear the pending restore marker when the target stash entry is already gone. */
const skipMissingStashRestore = (pendingRestoreFile: string, stashSha: string) => {
  rmSync(pendingRestoreFile, { force: true })
  console.warn(
    `Skipping post-commit stash restore because ${stashSha} is no longer in the stash list.`,
  )
}

/** Remove files written by a failed stash apply attempt. */
const rollbackFailedRestore = () => {
  run("git", ["reset", "--hard", "--quiet", "HEAD"])
  run("git", ["clean", "-fd", "--quiet"])
}

const pendingRestoreFile = gitPath("goddard/pre-commit-stash.json")

let pendingRestore: PendingRestore

try {
  pendingRestore = JSON.parse(readFileSync(pendingRestoreFile, "utf8")) as PendingRestore
} catch (error) {
  if (error instanceof Error && (error as NodeJS.ErrnoException).code === "ENOENT") {
    process.exit(0)
  }

  throw error
}

if (!pendingRestore.stashSha) {
  rmSync(pendingRestoreFile, { force: true })
  process.exit(0)
}

const stashRef = findStashRef(pendingRestore.stashSha)

if (!stashRef) {
  skipMissingStashRestore(pendingRestoreFile, pendingRestore.stashSha)
  process.exit(0)
}

if (hasWorktreeChanges()) {
  console.error(
    `Refusing to restore ${stashRef} because the index or worktree already has local changes. Commit, stash, or discard those changes, then rerun \`pnpm run postcommit:restore-stash\`.`,
  )
  process.exit(1)
}

try {
  run("git", ["stash", "apply", "--quiet", stashRef])
  run("git", ["stash", "drop", "--quiet", stashRef])
  rmSync(pendingRestoreFile, { force: true })
} catch {
  if (!findStashRef(pendingRestore.stashSha)) {
    skipMissingStashRestore(pendingRestoreFile, pendingRestore.stashSha)
    process.exit(0)
  }

  try {
    rollbackFailedRestore()
  } catch {
    console.error(
      `Failed to restore the hidden unstaged changes from ${stashRef}, then failed to roll back the conflicted restore attempt. Run \`git reset --hard HEAD\` and \`git clean -fd\` to clear the failed restore before retrying.`,
    )
    process.exit(1)
  }

  console.error(
    `Failed to restore the hidden unstaged changes from ${stashRef}. The failed restore was rolled back, so the stash changes were not left in the index or worktree. Git kept the stash entry intact; resolve the conflict manually and rerun \`pnpm run postcommit:restore-stash\` if needed.\n\nTo stop the post-commit hook from retrying this stash without deleting it, run:\n  rm "$(git rev-parse --git-path goddard/pre-commit-stash.json)"`,
  )
  process.exit(1)
}
