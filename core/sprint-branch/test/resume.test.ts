import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  git,
  readState,
  runCli,
} from "./support"

describe("sprint-branch resume", () => {
  afterEach(cleanupTestRepos)

  test("rebases next and applies a recorded feedback stash", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "scratch.txt"), "interrupted\n")
    expect((await runCli(repo, ["feedback", "--include-untracked"])).exitCode).toBe(0)
    await commitAll(repo, "record feedback transition")

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await fs.readFile(path.join(repo, "scratch.txt"), "utf-8")).toBe("interrupted\n")
    expect((await readState(repo, "example")).activeStashes).toEqual([])
  })

  test("records conflict state when rebase stops", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "next\n")
    await commitAll(repo, "add next conflict")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(1)
    const report = JSON.parse(result.stdout) as { ok: boolean }
    const state = await readState(repo, "example")
    expect(report.ok).toBe(false)
    expect(state.conflict?.command).toBe("resume")
    expect(state.conflict?.branch).toBe("sprint/example/next")
  })
})
