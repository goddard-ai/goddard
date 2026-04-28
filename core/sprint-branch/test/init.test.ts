import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { sprintStateFileName } from "../src"
import {
  branchExists,
  cleanupTestRepos,
  createBaseRepo,
  diagnosticCodes,
  git,
  readState,
  runCli,
  type MutationOutput,
} from "./support"

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

  // Dry-run is the agent's chance to inspect branch-moving commands before they happen.
  // The important contract is that even state-file creation is deferred until real execution.
  test("dry-run reports scaffold operations without creating branches or state", async () => {
    const repo = await createBaseRepo("example")

    const result = await runCli(repo, [
      "init",
      "--sprint",
      "example",
      "--base",
      "main",
      "--dry-run",
      "--json",
    ])
    const init = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(0)
    expect(init.dryRun).toBe(true)
    expect(init.executed).toBe(false)
    expect(init.gitOperations).toEqual([
      "git branch sprint/example/approved main",
      "git branch sprint/example/review sprint/example/approved",
    ])
    expect(await branchExists(repo, "sprint/example/approved")).toBe(false)
    expect(await branchExists(repo, "sprint/example/review")).toBe(false)
    expect(await pathExists(path.join(repo, "sprints", "example", sprintStateFileName))).toBe(false)
  })

  // A bare sprint/<name> branch collides with the namespace used for role branches.
  // Refusing it prevents Git ref layout problems before any scaffold branch is created.
  test("refuses an existing bare sprint namespace branch", async () => {
    const repo = await createBaseRepo("example")
    await git(repo, ["branch", "sprint/example", "main"])

    const result = await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])
    const init = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(init)).toContain("bare_sprint_branch_exists")
    expect(await branchExists(repo, "sprint/example/approved")).toBe(false)
    expect(await pathExists(path.join(repo, "sprints", "example", sprintStateFileName))).toBe(false)
  })
})

async function pathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (
      typeof error === "object" &&
      error !== null &&
      "code" in error &&
      (error as { code?: unknown }).code === "ENOENT"
    ) {
      return false
    }
    throw error
  }
}
