import assert from "node:assert/strict"
import * as fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"

import { executeCleanupOperations } from "../src/landing"
import type { AssociatedWorktree } from "../src/landing/types"
import type { SprintBranchState } from "../src/types"

type JsonOutput = {
  ok: boolean
  detached?: boolean
  sprint?: string
  reviewCommit?: string | null
  output?: string
  diagnostics: Array<{ severity?: string; code: string }>
  branchesToDelete?: string[]
  worktreesToRemove?: AssociatedWorktree[]
}

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const packageRoot = path.resolve(scriptDir, "..")
const cliPath = path.join(packageRoot, "src", "main.ts")
const tempPaths: string[] = []

await main().catch(async (error) => {
  await cleanupTempPaths()
  console.error(error instanceof Error ? error.stack || error.message : String(error))
  process.exit(1)
})

async function main() {
  try {
    const primary = await createBaseRepo("example")
    await fs.writeFile(path.join(primary, "sprints", "example", "030-task-name.md"), "# Task 030\n")
    await commitAll(primary, "add third sprint task")
    const agent = await createLinkedWorktree(primary)

    // Start from the same shape people will use day to day: an agent worktree
    // does the sprint work, and the primary worktree acts like the human's repo.
    // TECHNICAL NOTE: This verifies state is written to the shared Git metadata
    // directory, not to files in either worktree.
    await expectOk(agent, ["init", "--sprint", "example", "--base", "main", "--json"])
    assert.equal(await stateFileExists(primary, "example"), true)
    assert.equal(await workingTreePorcelain(primary), "")
    assert.equal(await workingTreePorcelain(agent), "")

    // A human or agent should be able to inspect the sprint from the primary
    // worktree right after the agent initializes it.
    // TECHNICAL NOTE: This covers inference from the Git-private state file,
    // without relying on the current branch or current directory.
    const initialStatus = await expectOk<JsonOutput>(primary, ["status", "--json"])
    const initialDoctor = await expectOk<JsonOutput>(primary, ["doctor", "--json"])
    assert.equal(initialStatus.sprint, "example")
    assert.equal(initialDoctor.ok, true)

    // Starting the first task should put the agent on the review branch without
    // asking it to choose a branch role.
    // TECHNICAL NOTE: The clean worktree check guards against state writes
    // showing up as tracked bookkeeping changes.
    await expectOk(agent, ["start", "--task", "010-task-name", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/review")
    assert.equal(await workingTreePorcelain(agent), "")
    await fs.writeFile(path.join(agent, "feature-one.txt"), "one\n")
    await commitAll(agent, "complete first task")

    // The review diff should show the first task's work.
    // TECHNICAL NOTE: This verifies the CLI is still diffing approved to review
    // after the branch-moving command chose the branch roles.
    const diff = await expectOk<JsonOutput>(primary, ["diff", "--json"])
    assert.match(diff.output ?? "", /feature-one\.txt/)

    // Starting another task while review is busy should create work-ahead.
    // TECHNICAL NOTE: That work-ahead branch must be based on review, not on
    // approved, so feedback can be folded in before approval.
    await expectOk(agent, ["start", "--task", "020-task-name", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/next")
    await fs.writeFile(path.join(agent, "feature-two.txt"), "two\n")
    await commitAll(agent, "complete second task")
    const withNextDoctor = await expectOk<JsonOutput>(primary, ["doctor", "--json"])
    assert.equal(withNextDoctor.ok, true)

    // Feedback should pause the in-progress work and move the agent back to
    // review, where feedback edits belong.
    // TECHNICAL NOTE: The scratch file is untracked on purpose. This checks that
    // feedback stashes untracked files and records a sprint-specific stash.
    await fs.writeFile(path.join(agent, "scratch-note.txt"), "carry this through feedback\n")
    await expectOk(agent, ["feedback", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/review")
    assert.equal(await workingTreePorcelain(agent), "")
    assert.match(await stashList(agent), /sprint-branch:example:020-task-name:feedback/)
    assert.equal((await readState(primary, "example")).activeStashes.length, 1)

    // Resume should bring the paused work forward after feedback is applied.
    // TECHNICAL NOTE: This rebases next onto the updated review branch, reapplies
    // the recorded stash, then commits the restored scratch file before approval.
    await fs.writeFile(path.join(agent, "feature-one.txt"), "one\nreview feedback\n")
    await commitAll(agent, "apply review feedback")
    await expectOk(agent, ["resume", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/next")
    assert.equal(
      await fs.readFile(path.join(agent, "scratch-note.txt"), "utf-8"),
      "carry this through feedback\n",
    )
    await commitAll(agent, "commit resumed scratch work")

    // Approving the first task should move the review window forward.
    // TECHNICAL NOTE: Approved fast-forwards to the old review branch, then the
    // rebased next branch becomes the new review branch.
    await expectOk(agent, ["approve", "--json"])
    let state = await readState(primary, "example")
    assert.deepEqual(
      {
        review: state.tasks.review,
        next: state.tasks.next,
        approved: state.tasks.approved,
      },
      {
        review: "020-task-name",
        next: null,
        approved: ["010-task-name"],
      },
    )
    assert.equal(
      await branchHead(primary, "sprint/example/review"),
      await branchHead(primary, "sprint/example/next"),
    )

    // After promotion, the agent can start one more work-ahead task.
    // TECHNICAL NOTE: This protects the two-slot model: one review task and at
    // most one next task.
    await expectOk(agent, ["start", "--task", "030-task-name", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/next")
    await fs.writeFile(path.join(agent, "feature-three.txt"), "three\n")
    await commitAll(agent, "complete third task")

    // Approving twice more should finish the sprint's task queue.
    // TECHNICAL NOTE: The first approval promotes task three into review. The
    // second leaves no review, next, or finished-unreviewed work.
    await expectOk(agent, ["approve", "--json"])
    state = await readState(primary, "example")
    assert.deepEqual(
      {
        review: state.tasks.review,
        next: state.tasks.next,
        approved: state.tasks.approved,
      },
      {
        review: "030-task-name",
        next: null,
        approved: ["010-task-name", "020-task-name"],
      },
    )

    await expectOk(agent, ["approve", "--json"])
    state = await readState(primary, "example")
    assert.deepEqual(state.tasks, {
      review: null,
      next: null,
      approved: ["010-task-name", "020-task-name", "030-task-name"],
      finishedUnreviewed: [],
    })

    // Finalize should prepare the review branch for the human's final merge.
    // TECHNICAL NOTE: Review, approved, and next should all point at the same
    // finalized content, while state changes stay out of the worktree.
    await expectOk(agent, ["finalize", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/review")
    assert.equal(await workingTreePorcelain(agent), "")
    const reviewHead = await branchHead(primary, "sprint/example/review")
    assert.equal(await branchHead(primary, "sprint/example/approved"), reviewHead)
    assert.equal(await branchHead(primary, "sprint/example/next"), reviewHead)

    // Human checkout should create a safe review snapshot in the primary repo.
    // TECHNICAL NOTE: It detaches HEAD so the human does not take ownership of
    // the live review branch.
    const checkout = await expectOk<JsonOutput>(primary, ["checkout", "example", "--json"])
    assert.equal(checkout.detached, true)
    assert.equal((await git(primary, ["rev-parse", "--abbrev-ref", "HEAD"])).trim(), "HEAD")
    assert.equal(await branchHead(primary, "HEAD"), reviewHead)

    // Landing is a human-only action, so this script plans it but does not bypass
    // the interactive confirmation rule.
    // TECHNICAL NOTE: The script performs the equivalent fast-forward with Git
    // afterward so cleanup has a landed target to verify.
    const land = await expectOk<JsonOutput>(primary, [
      "land",
      "main",
      "example",
      "--dry-run",
      "--json",
    ])
    assert.equal(land.reviewCommit, reviewHead)
    await git(primary, ["checkout", "main"])
    await git(primary, ["merge", "--ff-only", "sprint/example/review"])
    assert.equal(await branchHead(primary, "main"), reviewHead)

    // Cleanup should find the sprint branches and the agent worktree, then remove
    // the sprint's leftover state after landing.
    // TECHNICAL NOTE: We call the confirmed cleanup operation directly to avoid
    // weakening the CLI's interactive prompt requirement.
    const cleanup = await expectOk<JsonOutput>(primary, [
      "cleanup",
      "main",
      "example",
      "--dry-run",
      "--json",
    ])
    assert.deepEqual(cleanup.branchesToDelete, [
      "sprint/example/review",
      "sprint/example/approved",
      "sprint/example/next",
    ])
    const cleanupWorktrees = cleanup.worktreesToRemove ?? []
    assert.ok(
      (await realpaths(cleanupWorktrees.map((worktree) => worktree.path))).includes(
        await fs.realpath(agent),
      ),
    )
    await executeCleanupOperations(
      primary,
      await readState(primary, "example"),
      cleanup.branchesToDelete ?? [],
      cleanupWorktrees,
    )
    assert.equal(await stateFileExists(primary, "example"), false)
    assert.equal(await branchExists(primary, "sprint/example/review"), false)
    assert.equal(await branchExists(primary, "sprint/example/approved"), false)
    assert.equal(await branchExists(primary, "sprint/example/next"), false)

    console.log("sprint-branch smoke passed")
  } finally {
    await cleanupTempPaths()
  }
}

async function expectOk<T extends JsonOutput = JsonOutput>(cwd: string, args: string[]) {
  const result = await runCli(cwd, args)
  if (result.exitCode !== 0) {
    throw new Error(
      [
        `sprint-branch ${args.join(" ")} failed with exit code ${result.exitCode}`,
        result.stdout,
        result.stderr,
      ].join("\n"),
    )
  }

  assert.equal(result.exitCode, 0)
  assert.equal(result.stderr, "")

  const output = JSON.parse(result.stdout) as T
  assert.equal(output.ok, true)
  return output
}

async function createBaseRepo(sprint: string) {
  const repo = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-branch-smoke-"))
  tempPaths.push(repo)

  await git(repo, ["init"])
  await git(repo, ["checkout", "-b", "main"])
  await fs.writeFile(path.join(repo, "README.md"), "# Test\n")
  await fs.mkdir(path.join(repo, "sprints", sprint), { recursive: true })
  for (const task of ["010-task-name", "020-task-name"]) {
    await fs.writeFile(path.join(repo, "sprints", sprint, `${task}.md`), `# ${task}\n`)
  }
  await commitAll(repo, "init")

  return repo
}

async function createLinkedWorktree(repo: string, ref = "main") {
  const worktree = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-branch-smoke-worktree-"))
  tempPaths.push(worktree)
  await fs.rm(worktree, { recursive: true, force: true })
  await git(repo, ["worktree", "add", "--detach", worktree, ref])
  return worktree
}

async function readState(repo: string, sprint: string) {
  return JSON.parse(
    await fs.readFile(await sprintStatePath(repo, sprint), "utf-8"),
  ) as SprintBranchState
}

async function stateFileExists(repo: string, sprint: string) {
  return pathExists(await sprintStatePath(repo, sprint))
}

async function sprintStatePath(repo: string, sprint: string) {
  return path.join(await gitCommonDir(repo), "sprint-branch", sprint, "state.json")
}

async function gitCommonDir(repo: string) {
  const resolved = (await git(repo, ["rev-parse", "--git-common-dir"])).trim()
  return path.isAbsolute(resolved) ? resolved : path.join(repo, resolved)
}

async function runCli(cwd: string, args: string[]) {
  return runProcess(cwd, process.execPath, [cliPath, ...args])
}

async function git(cwd: string, args: string[]) {
  const result = await runProcess(cwd, "git", args)
  if (result.exitCode !== 0) {
    throw new Error(`git ${args.join(" ")} failed\n${result.stdout}\n${result.stderr}`)
  }
  return result.stdout
}

async function commitAll(repo: string, message: string) {
  await git(repo, ["add", "."])
  await git(repo, [
    "-c",
    "user.name=Smoke Test",
    "-c",
    "user.email=smoke@example.com",
    "commit",
    "--allow-empty",
    "-m",
    message,
  ])
}

async function branchExists(repo: string, branch: string) {
  const result = await runProcess(repo, "git", [
    "rev-parse",
    "--verify",
    "--quiet",
    `refs/heads/${branch}`,
  ])
  return result.exitCode === 0
}

async function currentBranch(repo: string) {
  return (await git(repo, ["branch", "--show-current"])).trim()
}

async function branchHead(repo: string, branch: string) {
  return (await git(repo, ["rev-parse", branch])).trim()
}

async function stashList(repo: string) {
  return git(repo, ["stash", "list"])
}

async function workingTreePorcelain(repo: string) {
  return git(repo, ["status", "--porcelain"])
}

async function realpaths(paths: string[]) {
  return Promise.all(paths.map((entry) => fs.realpath(entry)))
}

async function pathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

async function runProcess(cwd: string, command: string, args: string[]) {
  const subprocess = Bun.spawn([command, ...args], {
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

async function cleanupTempPaths() {
  await Promise.all(
    tempPaths.splice(0).map((entry) => fs.rm(entry, { recursive: true, force: true })),
  )
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}
