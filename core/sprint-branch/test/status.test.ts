import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  diagnosticCodes,
  git,
  runCli,
} from "./support"

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

  // Approved is supposed to contain only work that humans have already accepted.
  // Dirty edits there are especially dangerous because they bypass review branch boundaries.
  test("reports dirty work on the approved branch as unsafe", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/approved"])
    await fs.writeFile(path.join(repo, "README.md"), "# Test\ndirty approved\n")

    const result = await runCli(repo, ["status", "--json"])
    const status = JSON.parse(result.stdout) as {
      ok: boolean
      diagnostics: Array<{ code: string }>
      workingTree: { clean: boolean }
    }

    expect(result.exitCode).toBe(1)
    expect(status.ok).toBe(false)
    expect(status.workingTree.clean).toBe(false)
    expect(diagnosticCodes(status)).toContain("dirty_approved_worktree")
  })

  // Branch state is canonical, but task files are the human-readable recovery surface.
  // Missing files should not block read-only status, but they must be visible to the agent.
  test("warns when a recorded task file is missing", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await fs.rm(path.join(repo, "sprints", "example", "010-task-name.md"))
    await commitAll(repo, "remove recorded task file")

    const result = await runCli(repo, ["status", "--json"])
    const status = JSON.parse(result.stdout) as {
      ok: boolean
      diagnostics: Array<{ code: string }>
    }

    expect(result.exitCode).toBe(0)
    expect(status.ok).toBe(true)
    expect(diagnosticCodes(status)).toContain("task_file_missing")
  })
})
