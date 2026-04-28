import { afterEach, describe, expect, test } from "bun:test"

import {
  branchExists,
  cleanupTestRepos,
  createSprintRepo,
  currentBranch,
  readState,
  runCli,
} from "./support"

describe("sprint-branch start", () => {
  afterEach(cleanupTestRepos)

  test("places the first task on review", async () => {
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

  test("creates next work when review is occupied", async () => {
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
})
