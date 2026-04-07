import { execFileSync } from "node:child_process"
import { readFileSync, rmSync } from "node:fs"

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

/** Look up the current stash ref for a previously recorded stash commit. */
const findStashRef = (stashSha) => {
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

const pendingRestoreFile = gitPath("goddard/pre-commit-stash.json")

let pendingRestore

try {
  pendingRestore = JSON.parse(readFileSync(pendingRestoreFile, "utf8"))
} catch (error) {
  if (error.code === "ENOENT") {
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
  rmSync(pendingRestoreFile, { force: true })
  console.error(
    `Could not find the pending pre-commit stash ${pendingRestore.stashSha}. It may have already been restored or dropped.`,
  )
  process.exit(1)
}

try {
  run("git", ["stash", "pop", "--quiet", stashRef])
  rmSync(pendingRestoreFile, { force: true })
} catch (error) {
  console.error(
    `Failed to restore the hidden unstaged changes from ${stashRef}. Git kept the stash entry intact; resolve the worktree conflict and rerun \`bun run postcommit:restore-stash\` if needed.`,
  )
  process.exit(1)
}
