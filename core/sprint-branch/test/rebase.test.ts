import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchHead,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  git,
  isAncestor,
  readState,
  runCli,
  type MutationOutput,
} from "./support"

describe("sprint-branch rebase", () => {
  afterEach(cleanupTestRepos)

  test("rebases the sprint branch stack onto a target ref", async () => {
    const repo = await createStackedSprintRepo()
    await git(repo, ["checkout", "main"])
    await fs.writeFile(path.join(repo, "base.txt"), "new base\n")
    await commitAll(repo, "advance base")
    await git(repo, ["tag", "new-base", "main"])
    const approvedBefore = await branchHead(repo, "sprint/example/approved")

    const result = await runCli(repo, ["rebase", "new-base", "--sprint", "example", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("main")
    expect((await readState(repo, "example")).baseBranch).toBe("new-base")
    expect(await branchHead(repo, "sprint/example/approved")).not.toBe(approvedBefore)
    expect(await isAncestor(repo, "new-base", "sprint/example/approved")).toBe(true)
    expect(await isAncestor(repo, "sprint/example/approved", "sprint/example/review")).toBe(true)
    expect(await isAncestor(repo, "sprint/example/review", "sprint/example/next")).toBe(true)
  })

  test("dry-run reports the Git rebases without moving branches", async () => {
    const repo = await createStackedSprintRepo()
    await git(repo, ["checkout", "main"])
    await fs.writeFile(path.join(repo, "base.txt"), "new base\n")
    await commitAll(repo, "advance base")
    await git(repo, ["tag", "new-base", "main"])
    const approvedBefore = await branchHead(repo, "sprint/example/approved")

    const result = await runCli(repo, [
      "rebase",
      "new-base",
      "--sprint",
      "example",
      "--dry-run",
      "--json",
    ])
    const rebase = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(0)
    expect(rebase.dryRun).toBe(true)
    expect(rebase.executed).toBe(false)
    expect(rebase.gitOperations).toContain("git checkout sprint/example/approved")
    expect(
      rebase.gitOperations.some((operation) => operation.startsWith("git rebase --onto new-base ")),
    ).toBe(true)
    expect(await branchHead(repo, "sprint/example/approved")).toBe(approvedBefore)
    expect((await readState(repo, "example")).baseBranch).toBe("main")
  })
})

async function createStackedSprintRepo() {
  const repo = await createSprintRepo(
    "example",
    {
      review: "010-task-name",
      next: "020-task-name",
      approved: [],
    },
    { createNextBranch: true },
  )

  await git(repo, ["checkout", "sprint/example/approved"])
  await fs.writeFile(path.join(repo, "approved.txt"), "approved\n")
  await commitAll(repo, "add approved work")

  await git(repo, ["checkout", "sprint/example/review"])
  await git(repo, ["rebase", "sprint/example/approved"])
  await fs.writeFile(path.join(repo, "review.txt"), "review\n")
  await commitAll(repo, "add review work")

  await git(repo, ["checkout", "sprint/example/next"])
  await git(repo, ["rebase", "sprint/example/review"])
  await fs.writeFile(path.join(repo, "next.txt"), "next\n")
  await commitAll(repo, "add next work")

  return repo
}
