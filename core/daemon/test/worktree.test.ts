import { afterEach, expect, test } from "bun:test"
import { spawn } from "node:child_process"
import { existsSync, realpathSync } from "node:fs"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"

import { createWorktree, deleteWorktree, resolveWorktreePlugin } from "../src/worktrees/index.ts"
import { defaultPlugin } from "../src/worktrees/plugins/default.ts"

const cleanup: string[] = []
const originalHome = process.env.HOME

afterEach(async () => {
  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

test("requires the cwd to be a git repository", async () => {
  expect(resolveWorktreePlugin({ cwd: "/tmp/not-a-repo" })).rejects.toThrow("Not a git repository")
})

test("default worktree setup creates and cleans up a workspace inside an explicit directory", async () => {
  const repoDir = await createRepoFixture()
  const normalizedRepoDir = realpathSync.native(repoDir)

  const created = await createWorktree({
    cwd: repoDir,
    defaultPluginDirName: ".custom-dir",
    branchName: "feature-1",
  })

  expect(created.repoRoot).toBe(normalizedRepoDir)
  expect(created.requestedCwd).toBe(normalizedRepoDir)
  expect(created.effectiveCwd).toBe(created.worktreeDir)
  expect(created.worktreeDir).toMatch(/\.custom-dir\/feature-1-\d+$/)
  expect(existsSync(created.worktreeDir)).toBe(true)
  expect(created.poweredBy).toBe(defaultPlugin.name)
  expect(await resolveGitCommonDir(created.repoRoot)).toBe(
    await resolveGitCommonDir(created.worktreeDir),
  )

  expect(
    await deleteWorktree({
      cwd: created.repoRoot,
      worktreeDir: created.worktreeDir,
      branchName: created.branchName,
      poweredBy: created.poweredBy,
    }),
  ).toBe(true)
  expect(existsSync(created.worktreeDir)).toBe(false)
})

test("default plugin uses the global worktree directory when the repository has no local folder", async () => {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-worktree-home-"))
  cleanup.push(homeDir)
  process.env.HOME = homeDir

  const repoDir = await createRepoFixture()
  const created = await defaultPlugin.setup({
    cwd: repoDir,
    branchName: "feature-1",
  })

  expect(created).toMatch(new RegExp(`${escapeRegExp(join(homeDir, ".goddard", "worktrees"))}`))
  expect(existsSync(created!)).toBe(true)
  expect(await resolveGitCommonDir(repoDir)).toBe(await resolveGitCommonDir(created!))

  expect(await defaultPlugin.cleanup(created!, "feature-1")).toBe(true)
  expect(existsSync(created!)).toBe(false)
})

test("worktree setup maps repository subdirectories into the created workspace", async () => {
  const repoDir = await createRepoFixture({ includeSrc: true })
  const requestedCwd = realpathSync.native(join(repoDir, "src"))
  const created = await createWorktree({
    cwd: repoDir,
    requestedCwd,
    branchName: "feature-1",
  })
  const effectiveCwd = join(created.worktreeDir, "src")

  expect(created.requestedCwd).toBe(requestedCwd)
  expect(created.effectiveCwd).toBe(effectiveCwd)
  expect(existsSync(effectiveCwd)).toBe(true)
  expect(await resolveGitCommonDir(created.repoRoot)).toBe(
    await resolveGitCommonDir(created.worktreeDir),
  )

  expect(
    await deleteWorktree({
      cwd: created.repoRoot,
      worktreeDir: created.worktreeDir,
      branchName: created.branchName,
      poweredBy: created.poweredBy,
    }),
  ).toBe(true)
})

async function createRepoFixture(options: { includeSrc?: boolean } = {}) {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-worktree-repo-"))
  cleanup.push(repoDir)

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "repo", private: true }, null, 2),
    "utf-8",
  )

  if (options.includeSrc) {
    await mkdir(join(repoDir, "src"), { recursive: true })
    await writeFile(join(repoDir, "src", "index.ts"), "export const ready = true\n", "utf-8")
  }

  await runGit(repoDir, ["init"])
  await runGit(repoDir, ["config", "user.email", "bot@example.com"])
  await runGit(repoDir, ["config", "user.name", "Bot"])
  await runGit(repoDir, ["add", "."])
  await runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

async function runGit(cwd: string, args: string[]) {
  const result = await new Promise<{ status: number | null }>((resolve, reject) => {
    const child = spawn("git", args, {
      cwd,
      stdio: "ignore",
    })

    child.on("error", reject)
    child.on("close", (status: number | null) => {
      resolve({ status })
    })
  })

  expect(result.status).toBe(0)
}

async function resolveGitCommonDir(cwd: string) {
  return await new Promise<string>((resolve, reject) => {
    const child = spawn("git", ["rev-parse", "--git-common-dir"], {
      cwd,
      stdio: ["ignore", "pipe", "ignore"],
    })

    if (!child.stdout) {
      reject(new Error("Failed to capture git common dir output"))
      return
    }

    let stdout = ""
    child.stdout.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    child.on("error", reject)
    child.on("close", (status: number | null) => {
      if (status !== 0) {
        reject(new Error(`git rev-parse failed for ${cwd}`))
        return
      }

      resolve(resolvePath(cwd, stdout.trim()))
    })
  })
}

function resolvePath(cwd: string, value: string) {
  return realpathSync.native(resolve(cwd, value))
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}
