import { git, GitNotRepositoryError } from "@goddard-ai/libgit2"

import { runGitCommand } from "./command.ts"

export async function hasGitHead(workspaceRoot: string) {
  try {
    return (await git.history.resolveHead(workspaceRoot)) !== null
  } catch (error) {
    if (error instanceof GitNotRepositoryError) {
      return false
    }
    throw error
  }
}

export async function buildTrackedAndUntrackedDiff(workspaceRoot: string) {
  const trackedDiff = await readGitText(workspaceRoot, [
    "diff",
    "--no-ext-diff",
    "--binary",
    "--full-index",
    "HEAD",
    "--",
  ])
  const untrackedPaths = (await git.status.listUntracked(workspaceRoot)).map((entry) => entry.path)
  const sections = [trackedDiff]

  for (const path of untrackedPaths) {
    sections.push(await readAddedFileDiff(workspaceRoot, path))
  }

  return joinDiffSections(sections)
}

export async function buildInitialWorkspaceDiff(workspaceRoot: string) {
  const trackedAndUntrackedPaths = [
    ...new Set([
      ...(await git.index.listPaths(workspaceRoot)),
      ...(await git.status.listUntracked(workspaceRoot)).map((entry) => entry.path),
    ]),
  ].sort()
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

async function runGit(
  workspaceRoot: string,
  args: string[],
  options: {
    allowedExitCodes?: ReadonlySet<number>
  } = {},
) {
  const result = await runGitCommand(workspaceRoot, ["-c", "core.quotepath=false", ...args], {
    stdin: "ignore",
  })

  if ((options.allowedExitCodes ?? new Set([0])).has(result.status)) {
    return { exitCode: result.status, stdout: result.stdout, stderr: result.stderr }
  }

  const command = ["git", ...args].join(" ")
  throw new Error(result.stderr.trim() || `${command} failed in ${workspaceRoot}`)
}

function joinDiffSections(sections: string[]) {
  const nonEmptySections = sections.map((section) => section.trimEnd()).filter(Boolean)

  return nonEmptySections.length === 0 ? "" : `${nonEmptySections.join("\n")}\n`
}
