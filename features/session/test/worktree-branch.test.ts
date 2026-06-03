import { spawnSync } from "node:child_process"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import {
  createWorktreeBranchReadableId,
  resolveAvailableWorktreeBranchName,
  resolveWorktreeBranchName,
  resolveWorktreeBranchPrefix,
} from "../src/daemon/worktree-branch.ts"

const cleanupDirs: string[] = []
const originalHome = process.env.HOME

afterEach(async () => {
  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  while (cleanupDirs.length > 0) {
    await rm(cleanupDirs.pop()!, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 })
  }
})

test("resolveWorktreeBranchPrefix defaults to a local-user branch prefix", () => {
  expect(resolveWorktreeBranchPrefix()).toMatch(/^[a-z0-9._-]+(?:\/[a-z0-9._-]+)*$/)
})

test("createWorktreeBranchReadableId creates an easy-to-type word id", () => {
  expect(createWorktreeBranchReadableId()).toMatch(/^[a-z]+-[a-z]+-[a-z]+$/)
})

test("resolveWorktreeBranchName joins the configured branch prefix with a readable id", () => {
  expect(resolveWorktreeBranchName({ readableId: "Cape Town", branchPrefix: "agent" })).toBe(
    "agent/cape-town",
  )
})

test("resolveWorktreeBranchName uses host-scoped pull request branch names", () => {
  expect(
    resolveWorktreeBranchName({
      readableId: "quito",
      repository: "github.com/acme/widgets",
      prNumber: 123,
      branchPrefix: "agent",
    }),
  ).toBe("github.com/pr/123")
})

test("resolveWorktreeBranchName defaults pull request branches to GitHub host", () => {
  expect(
    resolveWorktreeBranchName({
      readableId: "quito",
      repository: "acme/widgets",
      prNumber: 123,
    }),
  ).toBe("github.com/pr/123")
})

test("resolveAvailableWorktreeBranchName skips existing generated branches", async () => {
  const repoDir = await createRepoFixture()

  const firstBranch = await resolveAvailableWorktreeBranchName({
    cwd: repoDir,
    branchPrefix: "agent",
  })
  runGit(repoDir, ["branch", firstBranch])

  const secondBranch = await resolveAvailableWorktreeBranchName({
    cwd: repoDir,
    branchPrefix: "agent",
  })

  expect(secondBranch).toMatch(/^agent\/[a-z]+-[a-z]+-[a-z]+$/)
  expect(secondBranch).not.toBe(firstBranch)
})

async function createRepoFixture() {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-worktree-branch-repo-"))
  cleanupDirs.push(repoDir)

  await writeFile(join(repoDir, "package.json"), JSON.stringify({ name: "repo" }), "utf-8")

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "."])
  runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })

  expect(result.status).toBe(0)
}
