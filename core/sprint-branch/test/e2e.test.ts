import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchHead,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  readState,
  runCli,
  writeCompleteReviewReport,
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
  // branch and state transitions no longer support a complete sprint.
  test("runs rolling starts, approvals, finalize, and landing dry-run", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: [],
      },
      { extraTaskStems: ["030-task-name"] },
    )

    await startAndCommitTask(repo, "010-task-name", "one")
    await startAndCommitTask(repo, "020-task-name", "two")

    await runJson<JsonCommandOutput>(repo, ["approve", "--sprint", "example", "--json"])

    await startAndCommitTask(repo, "030-task-name", "three")

    await runJson<JsonCommandOutput>(repo, ["approve", "--sprint", "example", "--json"])

    await runJson<JsonCommandOutput>(repo, ["approve", "--sprint", "example", "--json"])

    await runJson<JsonCommandOutput>(repo, ["finalize", "--sprint", "example", "--json"])

    const status = await runJson<StatusOutput>(repo, ["status", "--sprint", "example", "--json"])
    const doctor = await runJson<DoctorOutput>(repo, ["doctor", "--sprint", "example", "--json"])
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
    expect(await branchHead(repo, "sprint/example/approved")).toBe(
      await branchHead(repo, "sprint/example/review"),
    )
    expect(await branchHead(repo, "sprint/example/next")).toBe(
      await branchHead(repo, "sprint/example/review"),
    )
    expect((await readState(repo, "example")).tasks.approved).toEqual([
      "010-task-name",
      "020-task-name",
      "030-task-name",
    ])
  })
})

async function startAndCommitTask(repo: string, task: string, content: string) {
  await runJson<JsonCommandOutput>(repo, ["start", "--sprint", "example", "--task", task, "--json"])
  await fs.writeFile(path.join(repo, `${task}.txt`), `${content}\n`)
  await writeCompleteReviewReport(repo, "example", task)
  await commitAll(repo, `complete ${task}`)
  await runJson<JsonCommandOutput>(repo, [
    "finish",
    "--sprint",
    "example",
    "--task",
    task,
    "--json",
  ])
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
