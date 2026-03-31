import { existsSync } from "node:fs"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { spawn } from "node:child_process"
import { afterEach, expect, test } from "vitest"
import { Worktree } from "../src/index.ts"
import { defaultPlugin } from "../src/default-plugin.ts"

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

test("requires the cwd to be a git repository", () => {
  expect(() => new Worktree({ cwd: "/tmp/not-a-repo" })).toThrow("Not a git repository")
})

test("default worktree setup creates and cleans up a workspace inside an explicit directory", async () => {
  const repoDir = await createRepoFixture()

  const worktree = new Worktree({
    cwd: repoDir,
    defaultPluginDirName: ".custom-dir",
  })

  const created = await worktree.setup("feature-1")

  expect(created.worktreeDir).toMatch(/\.custom-dir\/feature-1-\d+$/)
  expect(existsSync(created.worktreeDir)).toBe(true)

  expect(await worktree.cleanup(created.worktreeDir, created.branchName)).toBe(true)
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

  expect(await defaultPlugin.cleanup(created!, "feature-1")).toBe(true)
  expect(existsSync(created!)).toBe(false)
})

test("worktree setup maps repository subdirectories into the created workspace", async () => {
  const repoDir = await createRepoFixture({ includeSrc: true })
  const requestedCwd = join(repoDir, "src")

  const worktree = new Worktree({ cwd: repoDir })
  const created = await worktree.setup("feature-1")
  const effectiveCwd = join(created.worktreeDir, "src")

  expect(existsSync(effectiveCwd)).toBe(true)

  expect(await worktree.cleanup(created.worktreeDir, created.branchName)).toBe(true)
})

async function createRepoFixture(options: { includeSrc?: boolean } = {}): Promise<string> {
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

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}
