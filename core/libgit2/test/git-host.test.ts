import { spawn } from "node:child_process"
import { access, mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { removeTemporaryPath } from "../../test-support/windows-fixtures.ts"
import {
  createGitHost,
  createLibgit2GitHost,
  normalizePath,
  resetGitHostForTests,
} from "../src/index.ts"
import { nativeLibgit2PathCandidates } from "../src/libgit2/native-artifact.ts"
import { createFakeGitHost } from "../src/testing.ts"

const originalGitLibgit2Path = process.env.GODDARD_GIT_LIBGIT2_PATH
const originalLibgit2Path = process.env.LIBGIT2_PATH
const tempRoots: string[] = []

afterEach(async () => {
  restoreEnv("GODDARD_GIT_LIBGIT2_PATH", originalGitLibgit2Path)
  restoreEnv("LIBGIT2_PATH", originalLibgit2Path)
  resetGitHostForTests()
  while (tempRoots.length > 0) {
    await removeTemporaryPath(tempRoots.pop()!)
  }
})

test("Git host fails when libgit2 cannot load", () => {
  expect(() =>
    createGitHost({
      libgit2PathCandidates: ["/missing/libgit2.dylib"],
    }),
  ).toThrow("Unable to load libgit2")
})

test("native libgit2 candidates include the repo-local artifact for supported targets", () => {
  const candidates = nativeLibgit2PathCandidates({
    platform: "darwin",
    arch: "arm64",
    moduleDir: "/repo/core/libgit2/src/libgit2",
    cwd: "/repo",
  })

  expect(candidates).toEqual([
    "/repo/core/libgit2/vendor/libgit2/dist/darwin-arm64/lib/libgit2.dylib",
  ])
})

test("native libgit2 candidates are empty for unsupported targets", () => {
  expect(
    nativeLibgit2PathCandidates({
      platform: "linux",
      arch: "x64",
      moduleDir: "/repo/core/libgit2/src/libgit2",
      cwd: "/repo",
    }),
  ).toEqual([])
})

test("libgit2 host uses a valid libgit2 candidate for read operations", async () => {
  const libgit2Path = await findLocalLibgit2Path()
  if (!libgit2Path) {
    expect(libgit2Path).toBeNull()
    return
  }

  const { branchHead, featureHead, repoDir } = await createRepo({ withFeatureBranch: true })
  const host = createLibgit2GitHost({
    libgit2PathCandidates: [libgit2Path],
  })

  await expect(host.repository.resolveRoot(repoDir)).resolves.toBe(await normalizePath(repoDir))
  await expect(host.repository.resolveGitDir(repoDir)).resolves.toEndWith(".git")
  await expect(host.repository.resolveCommonDir(repoDir)).resolves.toEndWith(".git")
  await expect(host.refs.getCurrentBranch(repoDir)).resolves.toBe("main")
  await expect(host.refs.branchExists(repoDir, "main")).resolves.toBe(true)
  await expect(host.refs.resolve(repoDir, "HEAD")).resolves.toBe(branchHead)
  await expect(host.refs.getBranchHead(repoDir, "main")).resolves.toBe(branchHead)
  await expect(host.history.resolveHead(repoDir)).resolves.toBe(branchHead)
  await expect(host.history.isAncestor(repoDir, branchHead, featureHead)).resolves.toBe(true)
  await expect(host.history.getMergeBase(repoDir, "main", "feature")).resolves.toBe(branchHead)
  await expect(host.status.isWorktreeClean(repoDir)).rejects.toThrow(
    "libgit2 host does not support status.isWorktreeClean",
  )
})

test("fake Git host exposes deterministic method overrides", async () => {
  const host = createFakeGitHost({
    refs: {
      branchExists: async (_cwd, branch) => branch === "main",
    },
  })

  await expect(host.refs.branchExists("/repo", "main")).resolves.toBe(true)
  await expect(host.refs.branchExists("/repo", "other")).resolves.toBe(false)
  await expect(host.refs.resolve("/repo", "HEAD")).rejects.toThrow(
    "Fake Git host method was not implemented",
  )
})

async function createRepo(options: { withFeatureBranch?: boolean } = {}) {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-git-host-test-"))
  tempRoots.push(rootDir)
  const repoDir = join(rootDir, "repo")
  await mkdir(repoDir, { recursive: true })
  await runGit(repoDir, ["init", "-b", "main"])
  await runGit(repoDir, ["config", "user.email", "git-host@example.com"])
  await runGit(repoDir, ["config", "user.name", "Git Host Test"])
  await writeFile(join(repoDir, "README.md"), "main\n")
  await runGit(repoDir, ["add", "README.md"])
  await runGit(repoDir, ["commit", "-m", "init"])
  const branchHead = (await runGit(repoDir, ["rev-parse", "HEAD"])).stdout.trim()
  let featureHead = branchHead

  if (options.withFeatureBranch) {
    await runGit(repoDir, ["checkout", "-b", "feature"])
    await writeFile(join(repoDir, "feature.txt"), "feature\n")
    await runGit(repoDir, ["add", "feature.txt"])
    await runGit(repoDir, ["commit", "-m", "feature"])
    featureHead = (await runGit(repoDir, ["rev-parse", "HEAD"])).stdout.trim()
    await runGit(repoDir, ["checkout", "main"])
  }

  return { branchHead, featureHead, repoDir }
}

async function findLocalLibgit2Path() {
  const candidates = [
    process.env.LIBGIT2_TEST_PATH,
    ...nativeLibgit2PathCandidates(),
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

async function runGit(cwd: string, args: string[]) {
  const result = await new Promise<{ status: number; stdout: string; stderr: string }>(
    (resolvePromise, rejectPromise) => {
      const child = spawn("git", args, {
        cwd,
        stdio: ["ignore", "pipe", "pipe"],
      })
      let stdout = ""
      let stderr = ""
      child.stdout.setEncoding("utf8")
      child.stderr.setEncoding("utf8")
      child.stdout.on("data", (chunk: string) => {
        stdout += chunk
      })
      child.stderr.on("data", (chunk: string) => {
        stderr += chunk
      })
      child.on("error", rejectPromise)
      child.on("close", (status) => {
        resolvePromise({ status: status ?? 1, stdout, stderr })
      })
    },
  )

  if (result.status !== 0) {
    throw new Error(`git ${args.join(" ")} failed in ${cwd}: ${result.stderr || result.stdout}`)
  }

  return result
}
