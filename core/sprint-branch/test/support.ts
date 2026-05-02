import { rmSync } from "node:fs"
import * as fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"

import { sprintStatePath, type SprintBranchState } from "../src"

const cliPath = path.join(import.meta.dir, "..", "src", "main.ts")
const tempRepos: string[] = []
const templateRepos: string[] = []
const baseRepoTemplatePromises = new Map<string, Promise<string>>()
let templateCleanupRegistered = false

/** Raw sprint state as it appears in state.json test fixtures. */
export type SprintStoredState = Omit<SprintBranchState, "branches">

export type SprintTestTasks = {
  review: string | null
  next: string | null
  approved: string[]
  finishedUnreviewed?: string[]
}

export type DiagnosticOutput = {
  diagnostics: Array<{ code: string }>
}

export type MutationOutput = DiagnosticOutput & {
  ok: boolean
  dryRun: boolean
  executed: boolean
  gitOperations: string[]
}

export function diagnosticCodes(output: DiagnosticOutput) {
  return output.diagnostics.map((diagnostic) => diagnostic.code)
}

export async function cleanupTestRepos() {
  await Promise.all(
    tempRepos.splice(0).map((repo) => fs.rm(repo, { recursive: true, force: true })),
  )
}

export async function createBaseRepo(sprint: string) {
  const repo = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-branch-"))
  tempRepos.push(repo)
  await fs.rm(repo, { recursive: true, force: true })
  await git(path.dirname(repo), [
    "clone",
    "--local",
    "--quiet",
    await baseRepoTemplate(sprint),
    repo,
  ])

  return repo
}

export async function createLinkedWorktree(repo: string, ref = "main") {
  const worktree = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-branch-worktree-"))
  tempRepos.push(worktree)
  await fs.rm(worktree, { recursive: true, force: true })
  await git(repo, ["worktree", "add", "--detach", worktree, ref])
  return worktree
}

export async function createSprintRepo(
  sprint: string,
  tasks: SprintTestTasks,
  options: { createNextBranch?: boolean; extraTaskStems?: string[] } = {},
) {
  const repo = await createBaseRepo(sprint)

  for (const task of options.extraTaskStems ?? []) {
    await fs.writeFile(path.join(repo, "sprints", sprint, `${task}.md`), `# ${task}\n`)
  }
  if (options.extraTaskStems?.length) {
    await commitAll(repo, "add extra sprint tasks")
  }

  await writeSprintState(repo, sprint, tasks)
  await git(repo, ["branch", `sprint/${sprint}/approved`])
  await git(repo, ["branch", `sprint/${sprint}/review`])
  if (options.createNextBranch) {
    await git(repo, ["branch", `sprint/${sprint}/next`, `sprint/${sprint}/review`])
  }

  return repo
}

export async function readState(repo: string, sprint: string) {
  return JSON.parse(
    await fs.readFile(await sprintStatePath(repo, sprint), "utf-8"),
  ) as SprintStoredState
}

export async function writeState(repo: string, sprint: string, state: unknown) {
  const statePath = await sprintStatePath(repo, sprint)
  await fs.mkdir(path.dirname(statePath), { recursive: true })
  await fs.writeFile(statePath, `${JSON.stringify(state, null, 2)}\n`)
}

export async function stateFileExists(repo: string, sprint: string) {
  return pathExists(await sprintStatePath(repo, sprint))
}

export async function workingTreePorcelain(repo: string) {
  return git(repo, ["status", "--porcelain"])
}

export async function runCli(cwd: string, args: string[]) {
  const subprocess = Bun.spawn([process.execPath, cliPath, ...args], {
    cwd,
    stdout: "pipe",
    stderr: "pipe",
    env: {
      ...process.env,
      NO_COLOR: "1",
    },
  })
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(subprocess.stdout).text(),
    new Response(subprocess.stderr).text(),
    subprocess.exited,
  ])

  return { stdout, stderr, exitCode }
}

export async function git(cwd: string, args: string[]) {
  const subprocess = Bun.spawn(["git", ...args], {
    cwd,
    stdout: "pipe",
    stderr: "pipe",
  })
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(subprocess.stdout).text(),
    new Response(subprocess.stderr).text(),
    subprocess.exited,
  ])

  if (exitCode !== 0) {
    throw new Error(`git ${args.join(" ")} failed\n${stdout}\n${stderr}`)
  }

  return stdout
}

export async function commitAll(repo: string, message: string) {
  await git(repo, ["add", "."])
  await git(repo, [
    "-c",
    "user.name=Test",
    "-c",
    "user.email=test@example.com",
    "commit",
    "--allow-empty",
    "-m",
    message,
  ])
}

export async function branchExists(repo: string, branch: string) {
  const subprocess = Bun.spawn(
    ["git", "rev-parse", "--verify", "--quiet", `refs/heads/${branch}`],
    {
      cwd: repo,
      stdout: "pipe",
      stderr: "pipe",
    },
  )
  return (await subprocess.exited) === 0
}

export async function currentBranch(repo: string) {
  return (await git(repo, ["branch", "--show-current"])).trim()
}

export async function branchHead(repo: string, branch: string) {
  return (await git(repo, ["rev-parse", branch])).trim()
}

export async function isAncestor(repo: string, ancestor: string, descendant: string) {
  const subprocess = Bun.spawn(["git", "merge-base", "--is-ancestor", ancestor, descendant], {
    cwd: repo,
    stdout: "pipe",
    stderr: "pipe",
  })
  return (await subprocess.exited) === 0
}

export async function stashList(repo: string) {
  return git(repo, ["stash", "list"])
}

async function writeSprintState(repo: string, sprint: string, tasks: SprintTestTasks) {
  const statePath = await sprintStatePath(repo, sprint)
  await fs.mkdir(path.dirname(statePath), { recursive: true })
  await fs.writeFile(
    statePath,
    `${JSON.stringify(
      {
        sprint,
        baseBranch: "main",
        visibility: "active",
        tasks,
        activeStashes: [],
        conflict: null,
      },
      null,
      2,
    )}\n`,
  )
}

/** Creates the immutable seed repository cloned by sprint-branch integration tests. */
async function baseRepoTemplate(sprint: string) {
  let template = baseRepoTemplatePromises.get(sprint)

  if (!template) {
    template = createBaseRepoTemplate(sprint)
    baseRepoTemplatePromises.set(sprint, template)
  }

  return await template
}

/** Initializes one reusable base repository for a sprint name. */
async function createBaseRepoTemplate(sprint: string) {
  registerTemplateCleanup()
  const repo = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-branch-template-"))
  templateRepos.push(repo)

  await git(repo, ["init"])
  await git(repo, ["config", "user.name", "Test"])
  await git(repo, ["config", "user.email", "test@example.com"])
  await git(repo, ["checkout", "-b", "main"])
  await fs.writeFile(path.join(repo, "README.md"), "# Test\n")
  await fs.mkdir(path.join(repo, "sprints", sprint), { recursive: true })
  for (const task of ["010-task-name", "020-task-name"]) {
    await fs.writeFile(path.join(repo, "sprints", sprint, `${task}.md`), `# ${task}\n`)
  }
  await commitAll(repo, "init")

  return repo
}

/** Registers synchronous cleanup because process exit hooks cannot await temp directory removal. */
function registerTemplateCleanup() {
  if (templateCleanupRegistered) {
    return
  }

  templateCleanupRegistered = true
  process.once("exit", () => {
    for (const repo of templateRepos) {
      rmSync(repo, { recursive: true, force: true })
    }
  })
}

async function pathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (
      typeof error === "object" &&
      error !== null &&
      "code" in error &&
      (error as { code?: unknown }).code === "ENOENT"
    ) {
      return false
    }
    throw error
  }
}
