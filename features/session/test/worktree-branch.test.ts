import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import {
  resolveAvailableWorktreeBranchName,
  resolvePullRequestWorktreeBranchName,
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

test("resolveAvailableWorktreeBranchName creates a generated branch under the configured prefix", async () => {
  const repoDir = await createRepoFixture()

  await expect(
    resolveAvailableWorktreeBranchName({
      cwd: repoDir,
      branchPrefix: "agent",
    }),
  ).resolves.toMatch(/^agent\/[a-z]+-[a-z]+-[a-z]+$/)
})

test("resolvePullRequestWorktreeBranchName uses host-scoped pull request branch names", () => {
  expect(
    resolvePullRequestWorktreeBranchName({
      repository: "github.com/acme/widgets",
      prNumber: 123,
    }),
  ).toBe("github.com/pr/123")
})

test("resolvePullRequestWorktreeBranchName defaults pull request branches to GitHub host", () => {
  expect(
    resolvePullRequestWorktreeBranchName({
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
  await runGit(repoDir, ["branch", firstBranch])

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

  await runGit(repoDir, ["init"])
  await runGit(repoDir, ["config", "user.email", "bot@example.com"])
  await runGit(repoDir, ["config", "user.name", "Bot"])
  await runGit(repoDir, ["add", "."])
  await runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

async function runGit(cwd: string, args: string[]) {
  const subprocess = Bun.spawn(["git", ...args], {
    cwd,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  })
  const exitCode = await subprocess.exited

  expect(exitCode).toBe(0)
}
