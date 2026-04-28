import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchHead,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  diagnosticCodes,
  git,
  readState,
  runCli,
  type MutationOutput,
} from "./support"

describe("sprint-branch finalize", () => {
  afterEach(cleanupTestRepos)

  test("leaves review checked out for the human merge", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["finalize", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).baseBranch).toBe("main")
  })

  test("refuses to finalize while unreviewed work is recorded", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["finalize", "--json"])
    const finalize = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(finalize)).toContain("unreviewed_work_exists")
    expect(await currentBranch(repo)).toBe("main")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  test("refuses when an unrecorded next branch still differs from review", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: ["010-task-name"],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "next.txt"), "ahead\n")
    await commitAll(repo, "add next work")

    const result = await runCli(repo, ["finalize", "--json"])
    const finalize = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(finalize)).toContain("active_next_branch_exists")
    expect(await currentBranch(repo)).toBe("sprint/example/next")
  })

  test("records conflict state when final rebase stops", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")
    await git(repo, ["branch", "-f", "sprint/example/approved", "sprint/example/review"])
    await git(repo, ["checkout", "main"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "main\n")
    await commitAll(repo, "add main conflict")
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/approved"),
    )

    const result = await runCli(repo, ["finalize", "--json"])
    const finalize = JSON.parse(result.stdout) as MutationOutput
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(1)
    expect(finalize.ok).toBe(false)
    expect(state.conflict?.command).toBe("finalize")
    expect(state.conflict?.branch).toBe("sprint/example/review")
  })
})
