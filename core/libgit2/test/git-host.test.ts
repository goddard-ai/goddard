import { spawn } from "node:child_process"
import { access, mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { removeTemporaryPath } from "../../test-support/windows-fixtures.ts"
import { git, normalizePath, resetGitForTests, validateLibgit2Runtime } from "../src/index.ts"
import { nativeLibgit2PathCandidates } from "../src/libgit2/native-artifact.ts"
import { createFakeGitApi } from "../src/testing.ts"

const originalGitLibgit2Path = process.env.GODDARD_GIT_LIBGIT2_PATH
const originalLibgit2Path = process.env.LIBGIT2_PATH
const tempRoots: string[] = []

afterEach(async () => {
  restoreEnv("GODDARD_GIT_LIBGIT2_PATH", originalGitLibgit2Path)
  restoreEnv("LIBGIT2_PATH", originalLibgit2Path)
  resetGitForTests()
  while (tempRoots.length > 0) {
    await removeTemporaryPath(tempRoots.pop()!)
  }
})

test("libgit2 runtime validation fails when libgit2 cannot load", () => {
  expect(() =>
    validateLibgit2Runtime({
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
  process.env.LIBGIT2_PATH = libgit2Path

  await expect(git.repository.resolveRoot(repoDir)).resolves.toBe(await normalizePath(repoDir))
  await expect(git.repository.resolveGitDir(repoDir)).resolves.toEndWith(".git")
  await expect(git.repository.resolveCommonDir(repoDir)).resolves.toEndWith(".git")
  await expect(git.refs.getCurrentBranch(repoDir)).resolves.toBe("main")
  await expect(git.refs.branchExists(repoDir, "main")).resolves.toBe(true)
  await expect(git.refs.resolve(repoDir, "HEAD")).resolves.toBe(branchHead)
  await expect(git.refs.getBranchHead(repoDir, "main")).resolves.toBe(branchHead)
  await expect(git.history.resolveHead(repoDir)).resolves.toBe(branchHead)
  await expect(git.history.isAncestor(repoDir, branchHead, featureHead)).resolves.toBe(true)
  await expect(git.history.getMergeBase(repoDir, "main", "feature")).resolves.toBe(branchHead)
  await expect(git.repository.isBareRepository(repoDir)).resolves.toBe(false)
  await expect(git.repository.resolveGitPath(repoDir, "info/exclude")).resolves.toEndWith(
    ".git/info/exclude",
  )
  await expect(git.status.isWorktreeClean(repoDir)).resolves.toBe(true)

  await writeFile(join(repoDir, "untracked.txt"), "untracked\n")
  await expect(git.status.getWorkingTreeStatus(repoDir)).resolves.toEqual({
    clean: false,
    entries: ["?? untracked.txt"],
  })
  await runGit(repoDir, ["add", "untracked.txt"])
  await writeFile(join(repoDir, "README.md"), "modified\n")
  const changedStatus = await git.status.getWorkingTreeStatus(repoDir)
  expect(changedStatus.clean).toBe(false)
  expect(changedStatus.entries).toContain("A  untracked.txt")
  expect(changedStatus.entries).toContain(" M README.md")

  await git.refs.update(repoDir, "refs/goddard/test", featureHead)
  await expect(git.refs.resolve(repoDir, "refs/goddard/test")).resolves.toBe(featureHead)
  await git.refs.delete(repoDir, "refs/goddard/test")
  await expect(git.refs.resolve(repoDir, "refs/goddard/test")).resolves.toBeNull()
  await expect(git.refs.delete(repoDir, "refs/goddard/missing")).resolves.toBeUndefined()

  const bareRepoDir = join(repoDir, "..", "bare.git")
  await runGit(repoDir, ["clone", "--bare", repoDir, bareRepoDir])
  await expect(git.repository.isBareRepository(bareRepoDir)).resolves.toBe(true)

  const linkedWorktreeDir = join(repoDir, "..", "linked")
  await runGit(repoDir, ["worktree", "add", linkedWorktreeDir, "feature"])
  const expectedInfoPath = (
    await runGit(linkedWorktreeDir, [
      "rev-parse",
      "--path-format=absolute",
      "--git-path",
      "info/exclude",
    ])
  ).stdout.trim()
  const expectedOperationPath = (
    await runGit(linkedWorktreeDir, [
      "rev-parse",
      "--path-format=absolute",
      "--git-path",
      "MERGE_HEAD",
    ])
  ).stdout.trim()
  await expect(git.repository.resolveGitPath(linkedWorktreeDir, "info/exclude")).resolves.toBe(
    await normalizePath(expectedInfoPath),
  )
  await expect(git.repository.resolveGitPath(linkedWorktreeDir, "MERGE_HEAD")).resolves.toBe(
    expectedOperationPath,
  )
})

test("fake Git API exposes deterministic method overrides", async () => {
  const fakeGit = createFakeGitApi({
    refs: {
      branchExists: async (_cwd, branch) => branch === "main",
    },
  })

  await expect(fakeGit.refs.branchExists("/repo", "main")).resolves.toBe(true)
  await expect(fakeGit.refs.branchExists("/repo", "other")).resolves.toBe(false)
  await expect(fakeGit.refs.resolve("/repo", "HEAD")).rejects.toThrow(
    "Fake Git API method was not implemented",
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
