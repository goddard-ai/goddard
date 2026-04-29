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

    // The smoke path starts from a linked agent worktree because that is the
    // operationally risky shape: the agent will move live sprint branches, while
    // the human primary worktree must still discover the same Git-private state.
    // Initializing here verifies state is written to the common Git directory,
    // not to either worktree's files.
    await expectOk(agent, ["init", "--sprint", "example", "--base", "main", "--json"])
    assert.equal(await stateFileExists(primary, "example"), true)
    assert.equal(await workingTreePorcelain(primary), "")
    assert.equal(await workingTreePorcelain(agent), "")

    // Status and doctor are the commands an agent should use before deciding
    // its next transition. Running them from the primary worktree immediately
    // after agent-side init proves inference works without being on a sprint
    // branch or inside sprints/<name>.
    const initialStatus = await expectOk<JsonOutput>(primary, ["status", "--json"])
    const initialDoctor = await expectOk<JsonOutput>(primary, ["doctor", "--json"])
    assert.equal(initialStatus.sprint, "example")
    assert.equal(initialDoctor.ok, true)

    // The first start must choose the review branch automatically. This is the
    // oldest unreviewed task, and the agent should not have to name the branch
    // role. The clean status assertion protects the Git-metadata state storage
    // contract from regressing into tracked bookkeeping files.
    await expectOk(agent, ["start", "--task", "010-task-name", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/review")
    assert.equal(await workingTreePorcelain(agent), "")
    await fs.writeFile(path.join(agent, "feature-one.txt"), "one\n")
    await commitAll(agent, "complete first task")

    // Diff is the review surface for humans and agents. The critical property
    // is not just that git diff runs, but that the CLI preserves the approved to
    // review range after the branch-moving commands have chosen the branch roles.
    const diff = await expectOk<JsonOutput>(primary, ["diff", "--json"])
    assert.match(diff.output ?? "", /feature-one\.txt/)

    // A second start while review is occupied must create work-ahead on next.
    // This branch depends on review, so later feedback can force a rebase
    // without exposing unapproved work on approved.
    await expectOk(agent, ["start", "--task", "020-task-name", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/next")
    await fs.writeFile(path.join(agent, "feature-two.txt"), "two\n")
    await commitAll(agent, "complete second task")
    const withNextDoctor = await expectOk<JsonOutput>(primary, ["doctor", "--json"])
    assert.equal(withNextDoctor.ok, true)

    // Human feedback interrupts work-ahead. The untracked scratch file is
    // intentional: Git's default stash would leave it behind, so the workflow
    // relies on feedback stashing untracked files and recording a resumable
    // sprint-specific stash entry before checking out review.
    await fs.writeFile(path.join(agent, "scratch-note.txt"), "carry this through feedback\n")
    await expectOk(agent, ["feedback", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/review")
    assert.equal(await workingTreePorcelain(agent), "")
    assert.match(await stashList(agent), /sprint-branch:example:020-task-name:feedback/)
    assert.equal((await readState(primary, "example")).activeStashes.length, 1)

    // Feedback changes the review branch underneath next. Resume must rebase
    // next onto the updated review branch and then reapply the recorded stash.
    // Committing the restored scratch file turns the dirty resumed work into
    // actual second-task content so approve can safely run afterward.
    await fs.writeFile(path.join(agent, "feature-one.txt"), "one\nreview feedback\n")
    await commitAll(agent, "apply review feedback")
    await expectOk(agent, ["resume", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/next")
    assert.equal(
      await fs.readFile(path.join(agent, "scratch-note.txt"), "utf-8"),
      "carry this through feedback\n",
    )
    await commitAll(agent, "commit resumed scratch work")

    // Approving the first task is the workflow's central promotion step. It
    // must fast-forward approved to review, then promote the already-rebased
    // next branch into the new review branch without requiring the agent to
    // manually reset either branch.
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

    // Starting the third task after promotion should recreate next from the new
    // review branch. This protects the rolling two-slot model: review carries
    // the oldest unapproved task, and next carries at most one work-ahead task.
    await expectOk(agent, ["start", "--task", "030-task-name", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/next")
    await fs.writeFile(path.join(agent, "feature-three.txt"), "three\n")
    await commitAll(agent, "complete third task")

    // These two approvals drain the remaining review window. The first rolls
    // the third task from next into review; the second leaves no unreviewed task
    // recorded, which is the precondition for finalize and human landing.
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

    // Finalize is the last agent-owned branch rewrite. It must leave the review
    // branch checked out for the agent, make review and approved represent the
    // same finalized content, and keep state-only updates out of the worktree.
    await expectOk(agent, ["finalize", "--json"])
    assert.equal(await currentBranch(agent), "sprint/example/review")
    assert.equal(await workingTreePorcelain(agent), "")
    const reviewHead = await branchHead(primary, "sprint/example/review")
    assert.equal(await branchHead(primary, "sprint/example/approved"), reviewHead)
    assert.equal(await branchHead(primary, "sprint/example/next"), reviewHead)

    // Human checkout intentionally detaches HEAD in the primary worktree. The
    // live review branch remains agent-owned, and the human can refresh their
    // snapshot by rerunning checkout if the agent makes more edits.
    const checkout = await expectOk<JsonOutput>(primary, ["checkout", "example", "--json"])
    assert.equal(checkout.detached, true)
    assert.equal((await git(primary, ["rev-parse", "--abbrev-ref", "HEAD"])).trim(), "HEAD")
    assert.equal(await branchHead(primary, "HEAD"), reviewHead)

    // Land is human-confirmed in production, so the smoke script exercises the
    // non-interactive dry-run contract and then performs the equivalent
    // fast-forward with Git. That gives cleanup a landed target without
    // weakening the CLI's TTY-only mutation rule.
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

    // Cleanup has two layers worth checking: the CLI planning path must identify
    // the checked-out agent worktree before deleting branches, and the confirmed
    // operation must remove both refs and Git-private state. Passing the agent
    // worktree to executeCleanupOperations mirrors what the interactive command
    // would do after human confirmation.
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
