#!/usr/bin/env bun
/**
 * Runs the main-branch pre-push guard from TypeScript so the Husky hook stays
 * thin and the repo-check rules live in a discoverable place.
 */
import { execFileSync, spawnSync } from "node:child_process"
import globrex from "globrex"

const CHECKED_SOURCE_FILE_EXTENSIONS = ["ts", "tsrx", "mts", "cts", "js", "jsx", "mjs", "cjs"]

const FULL_CHECK_FILE_GLOBS = [
  ...CHECKED_SOURCE_FILE_EXTENSIONS.flatMap((extension) => [
    `src/**/*.${extension}`,
    `scripts/**/*.${extension}`,
    `*.test.${extension}`,
  ]),
  "tsconfig.json",
  "tsconfig.*.json",
  "package.json",
  "tsdown.config.ts",
]

const FULL_CHECK_FILE_PATTERNS = FULL_CHECK_FILE_GLOBS.flatMap(compileFileFilterGlobs)

/**
 * Describes one ref update streamed to the pre-push hook on stdin.
 */
type PushUpdate = {
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
function parsePushUpdates(stdinText: string) {
  return stdinText
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [localRef = "", localSha = "", remoteRef = "", remoteSha = ""] = line.split(/\s+/, 4)
      return { localRef, localSha, remoteRef, remoteSha } satisfies PushUpdate
    })
}

/** Selects the pushed commit only when origin/main is being updated by a non-delete ref. */
function findMainPushSha(remoteName: string, updates: PushUpdate[]) {
  if (remoteName !== "origin") {
    return undefined
  }

  return updates.find(
    ({ localRef, remoteRef }) => remoteRef === "refs/heads/main" && localRef !== "(delete)",
  )?.localSha
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

/** Lists files changed between the branch point and the pushed commit. */
function getChangedFiles(repoRoot: string, branchPoint: string, pushedSha: string) {
  const result = spawnSync("git", ["diff", "--name-only", `${branchPoint}..${pushedSha}`], {
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

/** Runs one Bun command and returns whether it succeeded. */
function runBun(repoRoot: string, args: string[]) {
  const result = spawnSync(process.execPath, args, {
    cwd: repoRoot,
    stdio: "inherit",
  })

  return result.status === 0
}

/** Installs dependencies and runs the full repo check when the push requires it. */
function runRepoCheck(repoRoot: string) {
  return runBun(repoRoot, ["install", "--frozen-lockfile"]) && runBun(repoRoot, ["run", "check"])
}

/** Runs the pre-push guard and returns a process exit code. */
async function main(argv = process.argv.slice(2)) {
  const [remoteName = ""] = argv
  const pushedSha = findMainPushSha(remoteName, parsePushUpdates(await readStdinText()))

  if (!pushedSha) {
    return 0
  }

  const repoRoot = getRepoRoot()

  if (!hasOriginMain(repoRoot)) {
    return runRepoCheck(repoRoot) ? 0 : 1
  }

  const branchPoint = getMergeBase(repoRoot, pushedSha)

  if (!branchPoint) {
    return runRepoCheck(repoRoot) ? 0 : 1
  }

  if (
    shouldRunRepoCheck(getChangedFiles(repoRoot, branchPoint, pushedSha)) &&
    !runRepoCheck(repoRoot)
  ) {
    return 1
  }

  return 0
}

if (import.meta.main) {
  process.exit(await main())
}
