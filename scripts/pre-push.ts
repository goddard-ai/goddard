#!/usr/bin/env bun
/**
 * Runs the main-branch pre-push guard from TypeScript so the Husky hook stays
 * thin and the repo-check rules live in a discoverable place.
 */
import { execFileSync, spawnSync } from "node:child_process"
import { existsSync, readdirSync, readFileSync } from "node:fs"
import * as path from "node:path"
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

const CHECK_SCRIPTS = ["typecheck", "lint", "test"] as const

/** Names one workspace package script lane included in the pre-push matrix. */
type CheckScript = (typeof CHECK_SCRIPTS)[number]

/** Minimal package.json shape needed by the pre-push runner. */
type PackageJson = {
  name?: string
  scripts?: Record<string, string>
  workspaces?: string[] | { packages?: string[] }
}

/**
 * Describes one ref update streamed to the pre-push hook on stdin.
 */
type PushUpdate = {
  localRef: string
  localSha: string
  remoteRef: string
  remoteSha: string
}

/** Describes one package discovered from the root workspace list. */
type WorkspacePackage = {
  name: string
  packagePath: string
  relativePath: string
  scripts: Record<string, string>
}

/** Describes one concrete workspace check process. */
type WorkspaceCheckTask = {
  packageName: string
  packagePath: string
  relativePath: string
  script: CheckScript
}

/** Captures one completed workspace check process. */
type WorkspaceCheckResult = {
  task: WorkspaceCheckTask
  status: number | null
  elapsedMilliseconds: number
  stdout: string
  stderr: string
  error?: Error
}

/** Compiles one forward-slash filepath glob into the regex used for Git diff paths. */
function compileFileFilterGlob(fileGlob: string) {
  const compiled = globrex(fileGlob, {
    filepath: true,
    globstar: true,
  })

  return compiled.path.regex as RegExp
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

/** Reads a package manifest from disk. */
function readPackageJson(packageJsonPath: string) {
  return JSON.parse(readFileSync(packageJsonPath, "utf8")) as PackageJson
}

/** Formats a path relative to the repository root with stable separators. */
function toRepoPath(repoRoot: string, targetPath: string) {
  const relativePath = path.relative(repoRoot, targetPath)
  return relativePath.split(path.sep).join("/") || "."
}

/** Returns the workspace globs declared by the root package manifest. */
function getWorkspacePatterns(repoRoot: string) {
  const workspaces = readPackageJson(path.join(repoRoot, "package.json")).workspaces

  if (Array.isArray(workspaces)) {
    return workspaces
  }

  return workspaces?.packages ?? []
}

/** Expands one simple package-manager workspace glob into candidate directories. */
function expandWorkspacePattern(repoRoot: string, workspacePattern: string) {
  const segments = workspacePattern.split("/").filter(Boolean)
  let candidatePaths = [repoRoot]

  for (const segment of segments) {
    candidatePaths = candidatePaths.flatMap((candidatePath) => {
      if (!segment.includes("*")) {
        const nextPath = path.join(candidatePath, segment)
        return existsSync(nextPath) ? [nextPath] : []
      }

      if (!existsSync(candidatePath)) {
        return []
      }

      const segmentPattern = globrex(segment).regex as RegExp
      return readdirSync(candidatePath, { withFileTypes: true })
        .filter((entry) => entry.isDirectory() && segmentPattern.test(entry.name))
        .map((entry) => path.join(candidatePath, entry.name))
    })
  }

  return candidatePaths
}

/** Resolves root workspaces into package manifests with scripts. */
function getWorkspacePackages(repoRoot: string) {
  const packagesByPath = new Map<string, WorkspacePackage>()

  for (const workspacePattern of getWorkspacePatterns(repoRoot)) {
    for (const packagePath of expandWorkspacePattern(repoRoot, workspacePattern)) {
      const packageJsonPath = path.join(packagePath, "package.json")

      if (!existsSync(packageJsonPath)) {
        continue
      }

      const packageJson = readPackageJson(packageJsonPath)
      const relativePath = toRepoPath(repoRoot, packagePath)
      packagesByPath.set(packagePath, {
        name: packageJson.name ?? relativePath,
        packagePath,
        relativePath,
        scripts: packageJson.scripts ?? {},
      })
    }
  }

  return Array.from(packagesByPath.values()).sort((left, right) =>
    left.relativePath.localeCompare(right.relativePath),
  )
}

/** Builds the full package/script matrix for pre-push checks. */
function getWorkspaceCheckTasks(repoRoot: string) {
  return getWorkspacePackages(repoRoot).flatMap((workspacePackage) =>
    CHECK_SCRIPTS.filter((script) => workspacePackage.scripts[script]).map(
      (script) =>
        ({
          packageName: workspacePackage.name,
          packagePath: workspacePackage.packagePath,
          relativePath: workspacePackage.relativePath,
          script,
        }) satisfies WorkspaceCheckTask,
    ),
  )
}

/** Labels one task in hook output. */
function formatTaskLabel(task: WorkspaceCheckTask) {
  return `${task.packageName} ${task.script}`
}

/** Formats task duration only when it is long enough to help scan hook output. */
function formatElapsedTime(elapsedMilliseconds: number) {
  if (elapsedMilliseconds < 1000) {
    return undefined
  }

  const elapsedSeconds = elapsedMilliseconds / 1000
  return elapsedSeconds < 10 ? `${elapsedSeconds.toFixed(1)}s` : `${Math.round(elapsedSeconds)}s`
}

/** Runs one workspace package script through Turbo while capturing output for grouped failures. */
function runWorkspaceCheckTask(repoRoot: string, task: WorkspaceCheckTask) {
  return new Promise<WorkspaceCheckResult>((resolve) => {
    const startedAt = performance.now()
    let childProcess: ReturnType<typeof Bun.spawn>

    try {
      childProcess = Bun.spawn(
        [
          process.execPath,
          "run",
          "turbo",
          "--ui=stream",
          "run",
          task.script,
          `--filter=${task.packageName}`,
          "--only",
          "--output-logs=errors-only",
        ],
        {
          cwd: repoRoot,
          stdout: "pipe",
          stderr: "pipe",
        },
      )
    } catch (error) {
      resolve({
        task,
        status: null,
        elapsedMilliseconds: performance.now() - startedAt,
        stdout: "",
        stderr: "",
        error: error instanceof Error ? error : new Error(String(error)),
      })
      return
    }

    Promise.all([
      new Response(childProcess.stdout).text(),
      new Response(childProcess.stderr).text(),
      childProcess.exited,
    ]).then(
      ([stdout, stderr, status]) => {
        resolve({
          task,
          status,
          elapsedMilliseconds: performance.now() - startedAt,
          stdout,
          stderr,
        })
      },
      (error) => {
        resolve({
          task,
          status: null,
          elapsedMilliseconds: performance.now() - startedAt,
          stdout: "",
          stderr: "",
          error: error instanceof Error ? error : new Error(String(error)),
        })
      },
    )
  })
}

/** Prints captured process output for failed checks. */
function printWorkspaceCheckFailures(failures: WorkspaceCheckResult[]) {
  for (const failure of failures) {
    const output = `${failure.stdout}${failure.stderr}`.trimEnd()
    const statusLabel = failure.status === null ? "spawn failed" : `exit ${failure.status}`
    const elapsedTime = formatElapsedTime(failure.elapsedMilliseconds)

    process.stderr.write(
      `\npre-push: ${formatTaskLabel(failure.task)} failed (${[statusLabel, elapsedTime]
        .filter(Boolean)
        .join(", ")}) in ${failure.task.relativePath}\n`,
    )

    if (failure.error) {
      process.stderr.write(`${failure.error.stack ?? failure.error.message}\n`)
    }

    if (output) {
      process.stderr.write(`${output}\n`)
    }
  }
}

/** Runs all check lanes across all workspace packages while each task can use Turbo cache. */
async function runWorkspaceChecks(repoRoot: string) {
  const tasks = getWorkspaceCheckTasks(repoRoot)
  const taskCounts = CHECK_SCRIPTS.map(
    (script) => `${script}: ${tasks.filter((task) => task.script === script).length}`,
  ).join(", ")

  process.stdout.write(
    `pre-push: running ${tasks.length} workspace checks in parallel through Turbo (${taskCounts})\n`,
  )

  const results = await Promise.all(
    tasks.map(async (task) => {
      const result = await runWorkspaceCheckTask(repoRoot, task)

      if (result.status === 0) {
        const elapsedTime = formatElapsedTime(result.elapsedMilliseconds)
        process.stdout.write(
          `pre-push: passed ${formatTaskLabel(task)}${elapsedTime ? ` (${elapsedTime})` : ""}\n`,
        )
      }

      return result
    }),
  )
  const failures = results.filter((result) => result.status !== 0)

  if (failures.length > 0) {
    printWorkspaceCheckFailures(failures)
    return false
  }

  return true
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
async function runRepoCheck(repoRoot: string) {
  return runBun(repoRoot, ["install", "--frozen-lockfile"]) && (await runWorkspaceChecks(repoRoot))
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
    return (await runRepoCheck(repoRoot)) ? 0 : 1
  }

  const branchPoint = getMergeBase(repoRoot, pushedSha)

  if (!branchPoint) {
    return (await runRepoCheck(repoRoot)) ? 0 : 1
  }

  if (
    shouldRunRepoCheck(getChangedFiles(repoRoot, branchPoint, pushedSha)) &&
    !(await runRepoCheck(repoRoot))
  ) {
    return 1
  }

  return 0
}

if (import.meta.main) {
  process.exit(await main())
}
