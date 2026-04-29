import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  git,
  readState,
  runCli,
} from "./support"

type JsonCommandOutput = {
  ok: boolean
  diagnostics: Array<{ severity?: string; code: string }>
}

type StatusOutput = JsonCommandOutput & {
  state: {
    tasks: {
      review: string | null
      next: string | null
      approved: string[]
      finishedUnreviewed: string[]
    }
  }
  workingTree: {
    clean: boolean
  }
}

type DoctorOutput = JsonCommandOutput & {
  status: StatusOutput
}

describe("sprint-branch three-task happy path", () => {
  afterEach(cleanupTestRepos)

  // This is a cross-command contract test rather than another edge-case test.
  // It catches drift where each command passes in isolation, but their combined
  // state/branch handoff no longer supports a complete sprint.
  test("runs rolling starts, approvals, finalize, and landing dry-run", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: [],
        finishedUnreviewed: [],
      },
      { extraTaskStems: ["030-task-name"] },
    )

    await startAndCommitTask(repo, "010-task-name", "one")
    await startAndCommitTask(repo, "020-task-name", "two")

    await runJson<JsonCommandOutput>(repo, ["approve", "--json"])
    await commitAll(repo, "record approval 010")

    await startAndCommitTask(repo, "030-task-name", "three")

    await runJson<JsonCommandOutput>(repo, ["approve", "--json"])
    await commitAll(repo, "record approval 020")

    await runJson<JsonCommandOutput>(repo, ["approve", "--json"])
    await commitAll(repo, "record approval 030")

    await runJson<JsonCommandOutput>(repo, ["finalize", "--json"])
    await commitAll(repo, "record sprint finalize")

    const status = await runJson<StatusOutput>(repo, ["status", "--json"])
    const doctor = await runJson<DoctorOutput>(repo, ["doctor", "--json"])
    const land = await runJson<JsonCommandOutput>(repo, [
      "land",
      "main",
      "example",
      "--dry-run",
      "--json",
    ])

    expect(status.ok).toBe(true)
    expect(status.workingTree.clean).toBe(true)
    expect(status.state.tasks).toEqual({
      review: null,
      next: null,
      approved: ["010-task-name", "020-task-name", "030-task-name"],
      finishedUnreviewed: [],
    })
    expect(doctor.ok).toBe(true)
    expect(doctor.diagnostics.filter((diagnostic) => diagnostic.severity === "error")).toEqual([])
    expect(land.ok).toBe(true)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(await changedFiles(repo, "sprint/example/approved", "sprint/example/review")).toEqual(
      expect.arrayContaining(["sprints/example/001-handoff.md"]),
    )
    expect(
      (await changedFiles(repo, "sprint/example/approved", "sprint/example/review")).every((file) =>
        file.startsWith("sprints/example/"),
      ),
    ).toBe(true)
    expect(
      (await changedFiles(repo, "sprint/example/next", "sprint/example/review")).every((file) =>
        file.startsWith("sprints/example/"),
      ),
    ).toBe(true)
    expect((await readState(repo, "example")).tasks.approved).toEqual([
      "010-task-name",
      "020-task-name",
      "030-task-name",
    ])
  })
})

async function startAndCommitTask(repo: string, task: string, content: string) {
  await runJson<JsonCommandOutput>(repo, ["start", "--task", task, "--json"])
  await fs.writeFile(path.join(repo, `${task}.txt`), `${content}\n`)
  await commitAll(repo, `complete ${task}`)
}

async function runJson<T extends JsonCommandOutput>(repo: string, args: string[]) {
  const result = await runCli(repo, args)
  if (result.exitCode !== 0) {
    throw new Error(
      [
        `sprint-branch ${args.join(" ")} failed with exit code ${result.exitCode}`,
        result.stdout,
        result.stderr,
      ].join("\n"),
    )
  }

  expect(result.exitCode).toBe(0)
  expect(result.stderr).toBe("")

  const output = JSON.parse(result.stdout) as T
  expect(output.ok).toBe(true)
  return output
}

async function changedFiles(repo: string, leftRef: string, rightRef: string) {
  return (await git(repo, ["diff", "--name-only", leftRef, rightRef])).split("\n").filter(Boolean)
}
