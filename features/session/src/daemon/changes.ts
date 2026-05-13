import { resolveGitRepoRoot, type SessionWorktreeState } from "./worktree.ts"

/**
 * Reads the current git diff snapshot for one daemon session workspace.
 */
export async function readSessionChanges(params: {
  cwd: string
  worktree: SessionWorktreeState | null
}) {
  const workspaceRoot = params.worktree?.worktreeDir ?? (await resolveGitRepoRoot(params.cwd))

  if (!workspaceRoot) {
    return {
      workspaceRoot: null,
      diff: "",
      hasChanges: false,
    }
  }

  const hasHead =
    (
      await runGit(workspaceRoot, ["rev-parse", "--verify", "--quiet", "HEAD"], {
        allowedExitCodes: new Set([0, 1]),
      })
    ).exitCode === 0

  const diff = hasHead
    ? await buildTrackedAndUntrackedDiff(workspaceRoot)
    : await buildInitialWorkspaceDiff(workspaceRoot)

  return {
    workspaceRoot,
    diff,
    hasChanges: diff.length > 0,
  }
}

async function buildTrackedAndUntrackedDiff(workspaceRoot: string) {
  const trackedDiff = await readGitText(workspaceRoot, [
    "diff",
    "--no-ext-diff",
    "--binary",
    "--full-index",
    "HEAD",
    "--",
  ])
  const untrackedPaths = await readGitPathList(workspaceRoot, [
    "ls-files",
    "--others",
    "--exclude-standard",
    "-z",
  ])
  const sections = [trackedDiff]

  // `git diff HEAD` intentionally ignores untracked paths, so add them as no-index file additions.
  for (const path of untrackedPaths) {
    sections.push(await readAddedFileDiff(workspaceRoot, path))
  }

  return joinDiffSections(sections)
}

async function buildInitialWorkspaceDiff(workspaceRoot: string) {
  const trackedAndUntrackedPaths = await readGitPathList(workspaceRoot, [
    "ls-files",
    "--cached",
    "--others",
    "--exclude-standard",
    "-z",
  ])
  const sections: string[] = []

  for (const path of trackedAndUntrackedPaths) {
    sections.push(await readAddedFileDiff(workspaceRoot, path))
  }

  return joinDiffSections(sections)
}

async function readAddedFileDiff(workspaceRoot: string, path: string) {
  return await readGitText(
    workspaceRoot,
    ["diff", "--no-index", "--no-ext-diff", "--binary", "--full-index", "--", "/dev/null", path],
    new Set([0, 1]),
  )
}

async function readGitText(workspaceRoot: string, args: string[], allowedExitCodes = new Set([0])) {
  const { stdout } = await runGit(workspaceRoot, args, { allowedExitCodes })
  return stdout
}

async function readGitPathList(workspaceRoot: string, args: string[]) {
  const { stdout } = await runGit(workspaceRoot, args)
  return stdout.split("\0").filter((path) => path.length > 0)
}

async function runGit(
  workspaceRoot: string,
  args: string[],
  options: {
    allowedExitCodes?: ReadonlySet<number>
  } = {},
) {
  const result = Bun.spawn(["git", "-c", "core.quotepath=false", ...args], {
    cwd: workspaceRoot,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  })
  const exitCodePromise = result.exited
  const stdoutPromise = result.stdout ? new Response(result.stdout).text() : Promise.resolve("")
  const stderrPromise = result.stderr ? new Response(result.stderr).text() : Promise.resolve("")
  const [exitCode, stdout, stderr] = await Promise.all([
    exitCodePromise,
    stdoutPromise,
    stderrPromise,
  ])

  if ((options.allowedExitCodes ?? new Set([0])).has(exitCode)) {
    return { exitCode, stdout, stderr }
  }

  const command = ["git", ...args].join(" ")
  throw new Error(stderr.trim() || `${command} failed in ${workspaceRoot}`)
}

function joinDiffSections(sections: string[]) {
  const nonEmptySections = sections.map((section) => section.trimEnd()).filter(Boolean)

  return nonEmptySections.length === 0 ? "" : `${nonEmptySections.join("\n")}\n`
}
