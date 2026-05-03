import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchExists,
  cleanupTestRepos,
  createBaseRepo,
  diagnosticCodes,
  git,
  readState,
  runCli,
  stateFileExists,
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
    expect(await git(repo, ["status", "--porcelain"])).toBe("")
    await expectGitInfoExcludeLines(repo, ["sprints/"])
    const state = await readState(repo, "example")
    expect(Object.keys(state)).toEqual([
      "sprint",
      "baseBranch",
      "visibility",
      "tasks",
      "activeStashes",
      "conflict",
    ])
    expect(state.visibility).toBe("active")
    expect(state.tasks.review).toBeNull()
    await expect(pathExists(path.join(repo, "sprints", "example", "000-index.md"))).resolves.toBe(
      false,
    )
    await expect(pathExists(path.join(repo, "sprints", "example", "001-handoff.md"))).resolves.toBe(
      false,
    )
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
    expect(await stateFileExists(repo, "example")).toBe(false)
    await expectGitInfoExcludeLines(repo, [])
  })

  test("does not duplicate an existing sprints exclude entry", async () => {
    const repo = await createBaseRepo("example")
    await fs.writeFile(path.join(repo, ".git", "info", "exclude"), "# local excludes\nsprints/\n")

    const result = await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])

    expect(result.exitCode).toBe(0)
    await expectGitInfoExcludeLines(repo, ["sprints/"])
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
    expect(await stateFileExists(repo, "example")).toBe(false)
  })
})

async function expectGitInfoExcludeLines(repo: string, expected: string[]) {
  const exclude = await fs.readFile(path.join(repo, ".git", "info", "exclude"), "utf-8")
  expect(exclude.split(/\r?\n/).filter((line) => line.trim() === "sprints/")).toEqual(expected)
}

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
