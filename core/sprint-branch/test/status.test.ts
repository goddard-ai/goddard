import { afterEach, describe, expect, test } from "bun:test"

import { cleanupTestRepos, createSprintRepo, git, runCli } from "./support"

describe("sprint-branch status", () => {
  afterEach(cleanupTestRepos)

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
})
