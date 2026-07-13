import { access, mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { removeTemporaryPath } from "../../test-support/windows-fixtures.ts"
import {
  createReviewSyncGitHost,
  normalizePath,
  resetReviewSyncGitHostForTests,
} from "../src/git.ts"
import { runGit } from "./support.ts"

const originalGitHost = process.env.REVIEW_SYNC_GIT_HOST
const originalLibgit2Path = process.env.LIBGIT2_PATH
const tempRoots: string[] = []

afterEach(async () => {
  restoreEnv("REVIEW_SYNC_GIT_HOST", originalGitHost)
  restoreEnv("LIBGIT2_PATH", originalLibgit2Path)
  resetReviewSyncGitHostForTests()
  while (tempRoots.length > 0) {
    await removeTemporaryPath(tempRoots.pop()!)
  }
})

test("review-sync libgit2 host uses a valid libgit2 candidate when available", async () => {
  const libgit2Path = await findLocalLibgit2Path()
  if (!libgit2Path) {
    expect(libgit2Path).toBeNull()
    return
  }

  const repoDir = await createRepo()
  process.env.LIBGIT2_PATH = libgit2Path
  const host = createReviewSyncGitHost()

  await expect(host.resolveRequiredRepoRoot(repoDir)).resolves.toBe(await normalizePath(repoDir))
  await expect(host.isWorktreeClean(repoDir)).resolves.toBe(true)
  await writeFile(join(repoDir, "untracked.txt"), "untracked\n")
  await expect(host.isWorktreeClean(repoDir)).resolves.toBe(false)

  await runGit(repoDir, ["add", "untracked.txt"])
  await runGit(repoDir, ["config", "user.email", "review-sync@example.com"])
  await runGit(repoDir, ["config", "user.name", "Review Sync"])
  await runGit(repoDir, ["commit", "-m", "init"])
  const head = (await runGit(repoDir, ["rev-parse", "HEAD"])).stdout.trim()
  await host.updateRef(repoDir, "refs/review-sync/test", head)
  await expect(host.resolveRef(repoDir, "refs/review-sync/test")).resolves.toBe(head)
  await host.deleteRef(repoDir, "refs/review-sync/test")
  await expect(host.resolveRef(repoDir, "refs/review-sync/test")).resolves.toBeNull()
})

async function createRepo() {
  const rootDir = await mkdtemp(join(tmpdir(), "review-sync-git-host-test-"))
  tempRoots.push(rootDir)
  const repoDir = join(rootDir, "repo")
  await mkdir(repoDir, { recursive: true })
  await runGit(repoDir, ["init", "-b", "main"])
  return repoDir
}

async function findLocalLibgit2Path() {
  const candidates = [
    process.env.LIBGIT2_TEST_PATH,
    "/opt/homebrew/lib/libgit2.dylib",
    "/usr/local/lib/libgit2.dylib",
  ].filter((path) => typeof path === "string")

  for (const candidate of candidates) {
    if (
      await access(candidate).then(
        () => true,
        () => false,
      )
    ) {
      return candidate
    }
  }

  return null
}

function restoreEnv(key: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[key]
    return
  }

  process.env[key] = value
}
