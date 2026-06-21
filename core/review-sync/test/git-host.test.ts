import { access, mkdir, mkdtemp } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { removeTemporaryPath } from "../../test-support/windows-fixtures.ts"
import {
  createLibgit2GitHost,
  createReviewSyncGitHost,
  normalizePath,
  resetReviewSyncGitHostForTests,
  resolveReviewSyncGitHostMode,
  type GitHost,
} from "../src/git.ts"
import { runGit } from "./support.ts"

const originalGitHost = process.env.REVIEW_SYNC_GIT_HOST
const originalGoddardGitHost = process.env.GODDARD_GIT_HOST
const originalLibgit2Path = process.env.LIBGIT2_PATH
const originalReviewSyncLibgit2Path = process.env.REVIEW_SYNC_LIBGIT2_PATH
const tempRoots: string[] = []

afterEach(async () => {
  restoreEnv("REVIEW_SYNC_GIT_HOST", originalGitHost)
  restoreEnv("GODDARD_GIT_HOST", originalGoddardGitHost)
  restoreEnv("LIBGIT2_PATH", originalLibgit2Path)
  restoreEnv("REVIEW_SYNC_LIBGIT2_PATH", originalReviewSyncLibgit2Path)
  resetReviewSyncGitHostForTests()
  while (tempRoots.length > 0) {
    await removeTemporaryPath(tempRoots.pop()!)
  }
})

test("review-sync Git host mode defaults to auto and honors explicit modes", () => {
  delete process.env.GODDARD_GIT_HOST
  expect(resolveReviewSyncGitHostMode()).toBe("auto")

  process.env.GODDARD_GIT_HOST = "cli"
  expect(resolveReviewSyncGitHostMode()).toBe("cli")

  process.env.GODDARD_GIT_HOST = "libgit2"
  expect(resolveReviewSyncGitHostMode()).toBe("libgit2")
})

test("review-sync Git host forced CLI mode does not require libgit2", async () => {
  process.env.GODDARD_GIT_HOST = "cli"
  process.env.LIBGIT2_PATH = "/missing/libgit2.dylib"
  const repoDir = await createRepo()

  const host = createReviewSyncGitHost({
    libgit2PathCandidates: ["/missing/libgit2.dylib"],
  })

  await expect(host.resolveRequiredRepoRoot(repoDir)).resolves.toBe(await normalizePath(repoDir))
})

test("review-sync Git host auto mode falls back to CLI when libgit2 cannot load", async () => {
  delete process.env.GODDARD_GIT_HOST
  const repoDir = await createRepo()

  const host = createReviewSyncGitHost({
    libgit2PathCandidates: ["/missing/libgit2.dylib"],
  })

  await expect(host.resolveRequiredRepoRoot(repoDir)).resolves.toBe(await normalizePath(repoDir))
})

test("review-sync Git host forced libgit2 mode fails when libgit2 cannot load", () => {
  process.env.GODDARD_GIT_HOST = "libgit2"

  expect(() =>
    createReviewSyncGitHost({
      libgit2PathCandidates: ["/missing/libgit2.dylib"],
    }),
  ).toThrow("Unable to load libgit2")
})

test("review-sync libgit2 host uses a valid libgit2 candidate when available", async () => {
  const libgit2Path = await findLocalLibgit2Path()
  if (!libgit2Path) {
    expect(libgit2Path).toBeNull()
    return
  }

  const repoDir = await createRepo()
  const host = createLibgit2GitHost(createThrowingGitHost(), {
    libgit2PathCandidates: [libgit2Path],
  })

  await expect(host.resolveRequiredRepoRoot(repoDir)).resolves.toBe(await normalizePath(repoDir))
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

function createThrowingGitHost(): GitHost {
  const fail = async () => {
    throw new Error("fallback should not be used")
  }

  return {
    run: fail,
    resolveRequiredRepoRoot: fail,
    resolveRequiredGitCommonDir: fail,
    resolveRequiredGitDir: fail,
    resolveCurrentBranch: fail,
    branchExists: fail,
    isWorktreeClean: fail,
    resolveRef: fail,
    updateRef: fail,
    deleteRef: fail,
    listWorktrees: fail,
  }
}

function restoreEnv(key: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[key]
    return
  }

  process.env[key] = value
}
