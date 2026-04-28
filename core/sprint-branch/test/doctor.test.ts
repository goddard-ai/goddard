import { afterEach, describe, expect, test } from "bun:test"

import { cleanupTestRepos, createSprintRepo, runCli } from "./support"

describe("sprint-branch doctor", () => {
  afterEach(cleanupTestRepos)

  test("reports missing next branch when next has a recorded task", async () => {
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
})
