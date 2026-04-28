import * as fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { getExpectedBranches, sprintStateFileName } from "../src"

const cliPath = path.join(import.meta.dir, "..", "src", "main.ts")
const tempRepos: string[] = []

describe("sprint-branch CLI", () => {
  afterEach(async () => {
    await Promise.all(
      tempRepos.splice(0).map((repo) => fs.rm(repo, { recursive: true, force: true })),
    )
  })

  test("prints JSON status for the inferred sprint", async () => {
    const repo = await createSprintRepo("example")
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
    const repo = await createSprintRepo("example")
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
    const repo = await createSprintRepo("example")
    const result = await runCli(repo, ["diff"])

    expect(result.exitCode).toBe(0)
    expect(result.stdout.trim()).toBe("git diff sprint/example/approved...sprint/example/review")
  })

  test("doctor reports missing next branch when next has a recorded task", async () => {
    const repo = await createSprintRepo("example")
    await writeSprintState(repo, "example", {
      review: "010-task-name",
      next: "020-task-name",
      approved: [],
      finishedUnreviewed: [],
    })
    await fs.writeFile(path.join(repo, "sprints", "example", "020-task-name.md"), "# Task\n")
    await writeIndex(repo, "example", ["010-task-name", "020-task-name"])

    const result = await runCli(repo, ["doctor", "--json"])
    const doctor = JSON.parse(result.stdout) as {
      ok: boolean
      diagnostics: Array<{ code: string }>
    }

    expect(result.exitCode).toBe(1)
    expect(doctor.ok).toBe(false)
    expect(doctor.diagnostics.map((diagnostic) => diagnostic.code)).toContain("next_branch_missing")
  })
})

async function createSprintRepo(sprint: string) {
  const repo = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-branch-"))
  tempRepos.push(repo)

  await git(repo, ["init"])
  await git(repo, ["checkout", "-b", "main"])
  await fs.writeFile(path.join(repo, "README.md"), "# Test\n")
  await git(repo, ["add", "README.md"])
  await git(repo, [
    "-c",
    "user.name=Test",
    "-c",
    "user.email=test@example.com",
    "commit",
    "-m",
    "init",
  ])

  await fs.mkdir(path.join(repo, "sprints", sprint), { recursive: true })
  await fs.writeFile(path.join(repo, "sprints", sprint, "010-task-name.md"), "# Task\n")
  await writeSprintState(repo, sprint, {
    review: "010-task-name",
    next: null,
    approved: [],
    finishedUnreviewed: [],
  })
  await writeIndex(repo, sprint, ["010-task-name"])
  await git(repo, ["add", "sprints"])
  await git(repo, [
    "-c",
    "user.name=Test",
    "-c",
    "user.email=test@example.com",
    "commit",
    "-m",
    "add sprint state",
  ])
  await git(repo, ["branch", `sprint/${sprint}/approved`])
  await git(repo, ["branch", `sprint/${sprint}/review`])

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

async function writeIndex(repo: string, sprint: string, tasks: string[]) {
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
      ...tasks.map((task) => `Task: ${task}`),
      "",
    ].join("\n"),
  )
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
}
