import { afterEach, describe, expect, test } from "bun:test"

import { branchExists, cleanupTestRepos, createBaseRepo, readState, runCli } from "./support"

describe("sprint-branch init", () => {
  afterEach(cleanupTestRepos)

  test("creates state and review scaffold", async () => {
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
})
