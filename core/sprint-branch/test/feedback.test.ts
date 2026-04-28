import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  cleanupTestRepos,
  createSprintRepo,
  currentBranch,
  git,
  readState,
  runCli,
  stashList,
} from "./support"

describe("sprint-branch feedback", () => {
  afterEach(cleanupTestRepos)

  test("stashes dirty next work and checks out review", async () => {
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

    const result = await runCli(repo, ["feedback", "--include-untracked", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).activeStashes[0]?.sourceBranch).toBe(
      "sprint/example/next",
    )
    expect(await stashList(repo)).toContain("sprint-branch:example:020-task-name:feedback")
  })
})
