import { afterEach, describe, expect, test } from "bun:test"

import {
  branchExists,
  cleanupTestRepos,
  createBaseRepo,
  createLinkedWorktree,
  runCli,
  stateFileExists,
  workingTreePorcelain,
} from "./support"

type JsonOutput = {
  ok: boolean
  sprint: string
  stateRelativePath?: string
  reviewBranch?: string | null
}

describe("sprint-branch linked worktree state", () => {
  afterEach(cleanupTestRepos)

  // Agent and human worktrees share Git refs but have separate working trees.
  // Sprint state must therefore live in the common Git directory, otherwise the
  // primary human checkout cannot see state initialized from the agent worktree.
  test("shares sprint state initialized from a linked worktree", async () => {
    const repo = await createBaseRepo("example")
    const agentWorktree = await createLinkedWorktree(repo)

    const initResult = await runCli(agentWorktree, [
      "init",
      "--sprint",
      "example",
      "--base",
      "main",
      "--json",
    ])
    const init = JSON.parse(initResult.stdout) as JsonOutput

    expect(initResult.exitCode).toBe(0)
    expect(init.ok).toBe(true)
    expect(await stateFileExists(repo, "example")).toBe(true)
    expect(await branchExists(repo, "sprint/example/review")).toBe(true)
    expect(await workingTreePorcelain(repo)).toBe("")
    expect(await workingTreePorcelain(agentWorktree)).toBe("")

    const statusResult = await runCli(repo, ["status", "--sprint", "example", "--json"])
    const status = JSON.parse(statusResult.stdout) as JsonOutput

    expect(statusResult.exitCode).toBe(0)
    expect(status.ok).toBe(true)
    expect(status.sprint).toBe("example")
    expect(status.stateRelativePath).toBe(".git/sprint-branch/example/state.json")

    const checkoutResult = await runCli(repo, ["checkout", "example", "--dry-run", "--json"])
    const checkout = JSON.parse(checkoutResult.stdout) as JsonOutput

    expect(checkoutResult.exitCode).toBe(0)
    expect(checkout.ok).toBe(true)
    expect(checkout.reviewBranch).toBe("sprint/example/review")

    const landResult = await runCli(repo, ["land", "main", "example", "--dry-run", "--json"])
    const land = JSON.parse(landResult.stdout) as JsonOutput

    expect(landResult.exitCode).toBe(0)
    expect(land.ok).toBe(true)
  })
})
