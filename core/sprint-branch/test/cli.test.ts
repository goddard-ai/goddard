import * as fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { getExpectedBranches, sprintStateFileName, type SprintBranchState } from "../src"

const cliPath = path.join(import.meta.dir, "..", "src", "main.ts")
const tempRepos: string[] = []

describe("sprint-branch CLI", () => {
  afterEach(async () => {
    await Promise.all(
      tempRepos.splice(0).map((repo) => fs.rm(repo, { recursive: true, force: true })),
    )
  })

  test("prints JSON status for the inferred sprint", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["status", "--json"])

    expect(result.exitCode).toBe(0)
    const status = JSON.parse(result.stdout) as {
      ok: boolean
      sprint: string
      branches: { review: { exists: boolean }; approved: { exists: boolean } }
      blocked: { review: boolean }
    }
    expect(status.ok).toBe(true)
    expect(status.sprint).toBe("example")
    expect(status.branches.review.exists).toBe(true)
    expect(status.branches.approved.exists).toBe(true)
    expect(status.blocked.review).toBe(true)
  })

  test("infers the sprint from a sprint branch", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])

    const result = await runCli(repo, ["status", "--json"])
    const status = JSON.parse(result.stdout) as {
      ok: boolean
      currentBranch: string
      inferredFrom: string
    }

    expect(result.exitCode).toBe(0)
    expect(status.ok).toBe(true)
    expect(status.currentBranch).toBe("sprint/example/review")
    expect(status.inferredFrom).toContain("current branch")
  })

  test("prints the expected review diff command", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["diff"])

    expect(result.exitCode).toBe(0)
    expect(result.stdout.trim()).toBe("git diff sprint/example/approved...sprint/example/review")
  })

  test("doctor reports missing next branch when next has a recorded task", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: "020-task-name",
      approved: [],
      finishedUnreviewed: [],
    })

    const result = await runCli(repo, ["doctor", "--json"])
    const doctor = JSON.parse(result.stdout) as {
      ok: boolean
      diagnostics: Array<{ code: string }>
    }

    expect(result.exitCode).toBe(1)
    expect(doctor.ok).toBe(false)
    expect(doctor.diagnostics.map((diagnostic) => diagnostic.code)).toContain("next_branch_missing")
  })

  test("init creates state and review scaffold", async () => {
    const repo = await createBaseRepo("example")
    const result = await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])

    expect(result.exitCode).toBe(0)
    const report = JSON.parse(result.stdout) as { ok: boolean; executed: boolean }
    expect(report.ok).toBe(true)
    expect(report.executed).toBe(true)
    expect(await branchExists(repo, "sprint/example/approved")).toBe(true)
    expect(await branchExists(repo, "sprint/example/review")).toBe(true)
    expect(await branchExists(repo, "sprint/example/next")).toBe(false)
    expect((await readState(repo, "example")).tasks.review).toBeNull()
  })

  test("start places the first task on review", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["start", "--task", "010-task-name", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).tasks.review).toBe("010-task-name")
  })

  test("start creates next work when review is occupied", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["start", "--task", "020-task-name", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await branchExists(repo, "sprint/example/next")).toBe(true)
    expect((await readState(repo, "example")).tasks.next).toBe("020-task-name")
  })

  test("feedback stashes dirty next work and checks out review", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "scratch.txt"), "interrupted\n")

    const result = await runCli(repo, ["feedback", "--include-untracked", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).activeStashes[0]?.sourceBranch).toBe(
      "sprint/example/next",
    )
    expect(await stashList(repo)).toContain("sprint-branch:example:020-task-name:feedback")
  })

  test("resume rebases next and applies a recorded feedback stash", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "scratch.txt"), "interrupted\n")
    expect((await runCli(repo, ["feedback", "--include-untracked"])).exitCode).toBe(0)
    await commitAll(repo, "record feedback transition")

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await fs.readFile(path.join(repo, "scratch.txt"), "utf-8")).toBe("interrupted\n")
    expect((await readState(repo, "example")).activeStashes).toEqual([])
  })

  test("resume records conflict state when rebase stops", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "next\n")
    await commitAll(repo, "add next conflict")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(1)
    const report = JSON.parse(result.stdout) as { ok: boolean }
    const state = await readState(repo, "example")
    expect(report.ok).toBe(false)
    expect(state.conflict?.command).toBe("resume")
    expect(state.conflict?.branch).toBe("sprint/example/next")
  })

  test("approve promotes review into approved without next work", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["approve", "--verified", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(state.tasks.review).toBeNull()
    expect(state.tasks.approved).toEqual(["010-task-name"])
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/approved"),
    )
  })

  test("approve promotes next work onto review", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "review.txt"), "reviewed\n")
    await commitAll(repo, "add review work")
    await git(repo, ["checkout", "sprint/example/next"])
    await git(repo, ["rebase", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "next.txt"), "ahead\n")
    await commitAll(repo, "add next work")
    await git(repo, ["checkout", "sprint/example/review"])

    const result = await runCli(repo, ["approve", "--verified", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
    expect(state.tasks.review).toBe("020-task-name")
    expect(state.tasks.next).toBeNull()
    expect(state.tasks.approved).toEqual(["010-task-name"])
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/next"),
    )
  })

  test("finalize leaves review checked out for the human merge", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["finalize", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).baseBranch).toBe("main")
  })
})

async function createBaseRepo(sprint: string) {
  const repo = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-branch-"))
  tempRepos.push(repo)

  await git(repo, ["init"])
  await git(repo, ["checkout", "-b", "main"])
  await fs.writeFile(path.join(repo, "README.md"), "# Test\n")
  await fs.mkdir(path.join(repo, "sprints", sprint), { recursive: true })
  await fs.writeFile(path.join(repo, "sprints", sprint, "010-task-name.md"), "# Task 010\n")
  await fs.writeFile(path.join(repo, "sprints", sprint, "020-task-name.md"), "# Task 020\n")
  await commitAll(repo, "init")

  return repo
}

async function createSprintRepo(
  sprint: string,
  tasks: {
    review: string | null
    next: string | null
    approved: string[]
    finishedUnreviewed: string[]
  },
  options: { createNextBranch?: boolean } = {},
) {
  const repo = await createBaseRepo(sprint)

  await writeSprintState(repo, sprint, tasks)
  await writeIndex(repo, sprint, [tasks.review, tasks.next, ...tasks.approved].filter(Boolean))
  await commitAll(repo, "add sprint state")
  await git(repo, ["branch", `sprint/${sprint}/approved`])
  await git(repo, ["branch", `sprint/${sprint}/review`])
  if (options.createNextBranch) {
    await git(repo, ["branch", `sprint/${sprint}/next`, `sprint/${sprint}/review`])
  }

  return repo
}

async function writeSprintState(
  repo: string,
  sprint: string,
  tasks: {
    review: string | null
    next: string | null
    approved: string[]
    finishedUnreviewed: string[]
  },
) {
  const branches = getExpectedBranches(sprint)
  await fs.writeFile(
    path.join(repo, "sprints", sprint, sprintStateFileName),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        sprint,
        baseBranch: "main",
        branches,
        tasks,
        activeStashes: [],
        lock: null,
        conflict: null,
      },
      null,
      2,
    )}\n`,
  )
}

async function writeIndex(repo: string, sprint: string, tasks: Array<string | null>) {
  const branches = getExpectedBranches(sprint)
  await fs.writeFile(
    path.join(repo, "sprints", sprint, "000-index.md"),
    [
      `# Sprint ${sprint}`,
      "",
      `Review branch: ${branches.review}`,
      `Approved branch: ${branches.approved}`,
      `Next branch: ${branches.next}`,
      "",
      ...tasks.filter(Boolean).map((task) => `Task: ${task}`),
      "",
    ].join("\n"),
  )
}

async function readState(repo: string, sprint: string) {
  return JSON.parse(
    await fs.readFile(path.join(repo, "sprints", sprint, sprintStateFileName), "utf-8"),
  ) as SprintBranchState
}

async function runCli(cwd: string, args: string[]) {
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

async function git(cwd: string, args: string[]) {
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

async function commitAll(repo: string, message: string) {
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

async function branchExists(repo: string, branch: string) {
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

async function currentBranch(repo: string) {
  return (await git(repo, ["branch", "--show-current"])).trim()
}

async function branchHead(repo: string, branch: string) {
  return (await git(repo, ["rev-parse", branch])).trim()
}

async function stashList(repo: string) {
  return git(repo, ["stash", "list"])
}
