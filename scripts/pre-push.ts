#!/usr/bin/env bun
/*
 * Runs the pre-push guard from TypeScript so the Husky hook stays thin and the
 * repo-check rules live in a discoverable place.
 */
import { execFileSync, spawnSync } from "node:child_process"
import { mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import globrex from "globrex"

const REBASE_CHECKED_REMOTE_BRANCH_REFS = ["refs/heads/aleclarson"]
const ZERO_SHA = "0000000000000000000000000000000000000000"

const CHECKED_SOURCE_FILE_EXTENSIONS = ["ts", "tsrx", "mts", "cts", "js", "jsx", "mjs", "cjs"]

const FULL_CHECK_FILE_GLOBS = [
  ...CHECKED_SOURCE_FILE_EXTENSIONS.flatMap((extension) => [
    `src/**/*.${extension}`,
    `scripts/**/*.${extension}`,
    `*.test.${extension}`,
  ]),
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "tsconfig.json",
  "tsconfig.*.json",
  "package.json",
  "tsdown.config.ts",
]

const FULL_CHECK_FILE_PATTERNS = FULL_CHECK_FILE_GLOBS.flatMap(compileFileFilterGlobs)

/**
 * Describes one ref update streamed to the pre-push hook on stdin.
 */
export type PushUpdate = {
  localRef: string
  localSha: string
  remoteRef: string
  remoteSha: string
}

/** Compiles one forward-slash filepath glob into the regex used for Git diff paths. */
function compileFileFilterGlob(fileGlob: string) {
  const compiled = globrex(fileGlob, {
    filepath: true,
    globstar: true,
  })

  return compiled.path!.regex
}

/** Compiles one filepath glob for both repo-root and nested package paths. */
function compileFileFilterGlobs(fileGlob: string) {
  return [compileFileFilterGlob(fileGlob), compileFileFilterGlob(`**/${fileGlob}`)]
}

/** Reads the hook's stdin payload so the script can inspect pushed refs. */
async function readStdinText() {
  const chunks: string[] = []
  process.stdin.setEncoding("utf8")

  for await (const chunk of process.stdin) {
    chunks.push(chunk)
  }

  return chunks.join("")
}

/** Parses Git's pre-push stdin format into individual ref updates. */
export function parsePushUpdates(stdinText: string) {
  return stdinText
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [localRef = "", localSha = "", remoteRef = "", remoteSha = ""] = line.split(/\s+/, 4)
      return { localRef, localSha, remoteRef, remoteSha } satisfies PushUpdate
    })
}

/** Selects the pushed commit when origin is being updated by a non-delete ref. */
function findPushedUpdate(remoteName: string, updates: PushUpdate[]) {
  if (remoteName !== "origin") {
    return undefined
  }

  return updates.find(({ localRef, localSha }) => localRef !== "(delete)" && localSha !== ZERO_SHA)
}

/** Selects the pushed commit when a personal branch should prove it can rebase onto main. */
export function findRebaseCheckedBranchPushSha(remoteName: string, updates: PushUpdate[]) {
  if (remoteName !== "origin") {
    return undefined
  }

  return updates.find(
    ({ localRef, localSha, remoteRef }) =>
      REBASE_CHECKED_REMOTE_BRANCH_REFS.includes(remoteRef) &&
      localRef !== "(delete)" &&
      localSha !== ZERO_SHA,
  )?.localSha
}

/** Builds the Turbo affected range from the pushed remote commit, or branch point for new refs. */
export function getTurboAffectedFilterRange(pushedUpdate: PushUpdate, branchPoint?: string) {
  if (pushedUpdate.remoteSha !== ZERO_SHA) {
    return `${pushedUpdate.remoteSha}...${pushedUpdate.localSha}`
  }

  return branchPoint ? `${branchPoint}...${pushedUpdate.localSha}` : undefined
}

/** Resolves the repository root so subprocesses run from a stable location. */
function getRepoRoot() {
  return execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim()
}

/** Checks whether the local repo already knows about origin/main. */
function hasOriginMain(repoRoot: string) {
  return (
    spawnSync("git", ["rev-parse", "--verify", "--quiet", "origin/main"], {
      cwd: repoRoot,
      stdio: "ignore",
    }).status === 0
  )
}

/** Finds the merge base used to scope the incremental pre-push check. */
function getMergeBase(repoRoot: string, pushedSha: string) {
  const result = spawnSync("git", ["merge-base", pushedSha, "origin/main"], {
    cwd: repoRoot,
    encoding: "utf8",
  })

  if (result.status !== 0) {
    return undefined
  }

  return result.stdout.trim() || undefined
}

/** Refreshes origin/main so the local hook checks against the latest known merge base. */
function fetchOriginMain(repoRoot: string) {
  const result = spawnSync(
    "git",
    ["fetch", "--no-tags", "origin", "+main:refs/remotes/origin/main"],
    {
      cwd: repoRoot,
      stdio: "inherit",
    },
  )

  return result.status === 0
}

/** Checks rebaseability in a temporary worktree so the developer's branch is not rewritten. */
function canRebaseOntoOriginMain(repoRoot: string, pushedSha: string) {
  const tempRoot = mkdtempSync(join(tmpdir(), "goddard-pre-push-rebase-"))
  const worktreePath = join(tempRoot, "worktree")

  try {
    const addResult = spawnSync("git", ["worktree", "add", "--detach", worktreePath, pushedSha], {
      cwd: repoRoot,
      stdio: "ignore",
    })

    if (addResult.status !== 0) {
      return false
    }

    const rebaseResult = spawnSync("git", ["rebase", "--quiet", "origin/main"], {
      cwd: worktreePath,
      stdio: "ignore",
    })

    return rebaseResult.status === 0
  } finally {
    const removeResult = spawnSync("git", ["worktree", "remove", "--force", worktreePath], {
      cwd: repoRoot,
      stdio: "ignore",
    })

    if (removeResult.status !== 0) {
      console.error("pre-push: failed to remove temporary rebase-check worktree.")
    }

    rmSync(tempRoot, { force: true, recursive: true })

    spawnSync("git", ["worktree", "prune"], {
      cwd: repoRoot,
      stdio: "ignore",
    })
  }
}

/** Blocks personal branch pushes that CI would be unable to auto-merge with rebase. */
function runRebaseCheck(repoRoot: string, pushedSha: string) {
  if (!fetchOriginMain(repoRoot)) {
    console.error("pre-push: failed to fetch origin/main before checking rebaseability.")
    return false
  }

  if (!canRebaseOntoOriginMain(repoRoot, pushedSha)) {
    console.error("pre-push: pushed changes do not currently rebase cleanly onto origin/main.")
    console.error("pre-push: rebase locally onto origin/main before pushing again.")
    return false
  }

  return true
}

/** Lists files changed between the branch point and the pushed commit. */
function getChangedFiles(repoRoot: string, fromSha: string, toSha: string) {
  const result = spawnSync("git", ["diff", "--name-only", `${fromSha}..${toSha}`], {
    cwd: repoRoot,
    encoding: "utf8",
  })

  return result.stdout
    .split(/\r?\n/)
    .map((file) => file.trim())
    .filter(Boolean)
}

/** Detects whether the pushed diff can affect the full-repo check. */
function shouldRunRepoCheck(changedFiles: string[]) {
  return changedFiles.some((file) => FULL_CHECK_FILE_PATTERNS.some((pattern) => pattern.test(file)))
}

/** Runs one pnpm command and returns whether it succeeded. */
function runPnpm(repoRoot: string, args: string[]) {
  const result = spawnSync("pnpm", args, {
    cwd: repoRoot,
    stdio: "inherit",
  })

  return result.status === 0
}

/** Runs the static pre-push checks, filtering package checks when a push range is available. */
function runRepoCheck(repoRoot: string, changedFiles: string[], filterRange?: string) {
  if (
    changedFiles.includes("pnpm-lock.yaml") &&
    !runPnpm(repoRoot, ["install", "--frozen-lockfile", "--ignore-scripts"])
  ) {
    return false
  }

  if (!runPnpm(repoRoot, ["run", "check:docs"])) {
    return false
  }

  if (!runPnpm(repoRoot, ["exec", "turbo", "run", "codegen"])) {
    return false
  }

  const typecheckLintArgs = [
    "exec",
    "turbo",
    "run",
    "typecheck",
    "lint",
    "--output-logs=errors-only",
    "--continue=always",
  ]

  if (filterRange) {
    typecheckLintArgs.push(`--filter=...[${filterRange}]`)
  }

  return runPnpm(repoRoot, typecheckLintArgs)
}

/** Runs the pre-push guard and returns a process exit code. */
async function main(argv = process.argv.slice(2)) {
  const [remoteName = ""] = argv
  const updates = parsePushUpdates(await readStdinText())
  const pushedUpdate = findPushedUpdate(remoteName, updates)
  const pushedSha = pushedUpdate?.localSha
  const rebaseCheckedSha = findRebaseCheckedBranchPushSha(remoteName, updates)

  if (!pushedSha && !rebaseCheckedSha) {
    return 0
  }

  const repoRoot = getRepoRoot()

  if (rebaseCheckedSha && !runRebaseCheck(repoRoot, rebaseCheckedSha)) {
    return 1
  }

  if (!pushedSha) {
    return 0
  }

  if (pushedUpdate.remoteSha !== ZERO_SHA) {
    const changedFiles = getChangedFiles(repoRoot, pushedUpdate.remoteSha, pushedSha)
    const filterRange = getTurboAffectedFilterRange(pushedUpdate)

    if (shouldRunRepoCheck(changedFiles) && !runRepoCheck(repoRoot, changedFiles, filterRange)) {
      return 1
    }

    return 0
  }

  if (!hasOriginMain(repoRoot)) {
    return runRepoCheck(repoRoot, []) ? 0 : 1
  }

  const branchPoint = getMergeBase(repoRoot, pushedSha)

  if (!branchPoint) {
    return runRepoCheck(repoRoot, []) ? 0 : 1
  }

  const changedFiles = getChangedFiles(repoRoot, branchPoint, pushedSha)
  const filterRange = getTurboAffectedFilterRange(pushedUpdate, branchPoint)

  if (shouldRunRepoCheck(changedFiles) && !runRepoCheck(repoRoot, changedFiles, filterRange)) {
    return 1
  }

  return 0
}

if (import.meta.main) {
  process.exit(await main())
}
