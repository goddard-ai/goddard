import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { delimiter, dirname, join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { prepareFreshWorktree } from "../src/daemon/worktrees/bootstrap.ts"
import { createWorktree, deleteWorktree } from "../src/daemon/worktrees/index.ts"

const cleanup: string[] = []
const originalPath = process.env.PATH

afterEach(async () => {
  if (originalPath === undefined) {
    delete process.env.PATH
  } else {
    process.env.PATH = originalPath
  }

  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

test("same-head preparation seeds the default untracked paths and skips bootstrap without a manager", async () => {
  const repoDir = await createRepoFixture()
  await mkdir(join(repoDir, "node_modules", "pkg"), { recursive: true })
  await writeFile(
    join(repoDir, "node_modules", "pkg", "index.js"),
    "export const dep = 1\n",
    "utf-8",
  )
  await mkdir(join(repoDir, "dist"), { recursive: true })
  await writeFile(join(repoDir, "dist", "index.js"), "export const dist = true\n", "utf-8")
  await mkdir(join(repoDir, ".turbo"), { recursive: true })
  await writeFile(join(repoDir, ".turbo", "cache.json"), '{"ok":true}\n', "utf-8")
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-seed-defaults",
  })

  const prepared = await prepareFreshWorktree({
    repoRoot: repoDir,
    worktreeDir: created.worktreeDir,
  })

  expect(prepared.bootstrapRan).toBe(false)
  expect(prepared.packageManager).toBeNull()
  expect(prepared.seededPaths).toEqual([".turbo", "dist", "node_modules"])
  expect(
    await readFile(join(created.worktreeDir, "node_modules", "pkg", "index.js"), "utf-8"),
  ).toBe("export const dep = 1\n")
  expect(await readFile(join(created.worktreeDir, "dist", "index.js"), "utf-8")).toBe(
    "export const dist = true\n",
  )
  expect(await readFile(join(created.worktreeDir, ".turbo", "cache.json"), "utf-8")).toBe(
    '{"ok":true}\n',
  )

  await cleanupWorktree(repoDir, created)
})

test("preparation skips seeding when the source checkout head no longer matches the worktree", async () => {
  const repoDir = await createRepoFixture()
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-head-mismatch",
  })

  await mkdir(join(repoDir, "node_modules", "pkg"), { recursive: true })
  await writeFile(
    join(repoDir, "node_modules", "pkg", "index.js"),
    "export const dep = 1\n",
    "utf-8",
  )
  await writeFile(join(repoDir, "tracked.txt"), "advance head\n", "utf-8")
  await runGit(repoDir, ["add", "tracked.txt"])
  await runGit(repoDir, ["commit", "-m", "advance"])

  const prepared = await prepareFreshWorktree({
    repoRoot: repoDir,
    worktreeDir: created.worktreeDir,
  })

  expect(prepared.seededPaths).toEqual([])
  expect(existsSync(join(created.worktreeDir, "node_modules", "pkg", "index.js"))).toBe(false)

  await cleanupWorktree(repoDir, created)
})

test("explicit seed paths can copy nested untracked files from an untracked directory", async () => {
  const repoDir = await createRepoFixture()
  await mkdir(join(repoDir, "cache", "nested"), { recursive: true })
  await writeFile(join(repoDir, "cache", "nested", "artifact.txt"), "artifact\n", "utf-8")
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-explicit-path",
  })

  const prepared = await prepareFreshWorktree({
    repoRoot: repoDir,
    worktreeDir: created.worktreeDir,
    config: {
      seedNames: [],
      seedPaths: ["cache/nested/artifact.txt"],
    },
  })

  expect(prepared.seededPaths).toEqual(["cache/nested/artifact.txt"])
  expect(
    await readFile(join(created.worktreeDir, "cache", "nested", "artifact.txt"), "utf-8"),
  ).toBe("artifact\n")

  await cleanupWorktree(repoDir, created)
})

test(".worktreeinclude copies matching gitignored files only", async () => {
  const repoDir = await createRepoFixture({
    trackedFiles: {
      ".gitignore": ".env\n.env.local\nconfig/secrets.json\nsecrets/\ntracked-secret.txt\n",
      ".worktreeinclude":
        ".env\n.env.local\nconfig/secrets.json\nsecrets/\nnot-ignored.txt\ntracked-secret.txt\n",
    },
  })
  await writeFile(join(repoDir, "tracked-secret.txt"), "tracked\n", "utf-8")
  await runGit(repoDir, ["add", "-f", "tracked-secret.txt"])
  await runGit(repoDir, ["commit", "-m", "track ignored file"])
  await writeFile(join(repoDir, ".env"), "ENV=1\n", "utf-8")
  await writeFile(join(repoDir, ".env.local"), "LOCAL=1\n", "utf-8")
  await mkdir(join(repoDir, "config"), { recursive: true })
  await writeFile(join(repoDir, "config", "secrets.json"), '{"secret":true}\n', "utf-8")
  await mkdir(join(repoDir, "secrets"), { recursive: true })
  await writeFile(join(repoDir, "secrets", "token.txt"), "token\n", "utf-8")
  await writeFile(join(repoDir, "not-ignored.txt"), "skip\n", "utf-8")
  await writeFile(join(repoDir, "tracked-secret.txt"), "changed\n", "utf-8")
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-worktreeinclude",
  })

  const prepared = await prepareFreshWorktree({
    repoRoot: repoDir,
    worktreeDir: created.worktreeDir,
    config: {
      seedNames: [],
      seedPaths: [],
    },
  })

  expect(prepared.seededPaths).toEqual([".env", ".env.local", "config/secrets.json", "secrets"])
  expect(await readFile(join(created.worktreeDir, ".env"), "utf-8")).toBe("ENV=1\n")
  expect(await readFile(join(created.worktreeDir, ".env.local"), "utf-8")).toBe("LOCAL=1\n")
  expect(await readFile(join(created.worktreeDir, "config", "secrets.json"), "utf-8")).toBe(
    '{"secret":true}\n',
  )
  expect(await readFile(join(created.worktreeDir, "secrets", "token.txt"), "utf-8")).toBe("token\n")
  expect(existsSync(join(created.worktreeDir, "not-ignored.txt"))).toBe(false)
  expect(await readFile(join(created.worktreeDir, "tracked-secret.txt"), "utf-8")).toBe("tracked\n")

  await cleanupWorktree(repoDir, created)
})

test("tracked and ignored paths are not seeded into the fresh worktree", async () => {
  const repoDir = await createRepoFixture({
    trackedFiles: {
      ".gitignore": ".turbo\n",
      "dist/tracked.js": "export const tracked = 'base'\n",
    },
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-ignore-tracked",
  })

  await writeFile(
    join(repoDir, "dist", "tracked.js"),
    "export const tracked = 'changed'\n",
    "utf-8",
  )
  await mkdir(join(repoDir, ".turbo"), { recursive: true })
  await writeFile(join(repoDir, ".turbo", "cache.json"), '{"ignored":true}\n', "utf-8")

  const prepared = await prepareFreshWorktree({
    repoRoot: repoDir,
    worktreeDir: created.worktreeDir,
  })

  expect(prepared.seededPaths).toEqual([])
  expect(await readFile(join(created.worktreeDir, "dist", "tracked.js"), "utf-8")).toBe(
    "export const tracked = 'base'\n",
  )
  expect(existsSync(join(created.worktreeDir, ".turbo", "cache.json"))).toBe(false)

  await cleanupWorktree(repoDir, created)
})

test("preparation infers the package manager from package.json and runs install args", async () => {
  const repoDir = await createRepoFixture({
    packageJson: {
      name: "repo",
      private: true,
      packageManager: "bun@1.3.11",
    },
  })
  const binDir = await createFakePackageManager("bun", {
    exitCode: 0,
    outputFile: ".bootstrap-marker",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-bootstrap-bun",
  })

  process.env.PATH = `${binDir}${delimiter}${originalPath ?? ""}`

  const prepared = await prepareFreshWorktree({
    repoRoot: repoDir,
    worktreeDir: created.worktreeDir,
    config: {
      seedEnabled: false,
      installArgs: ["--frozen-lockfile"],
    },
  })

  expect(prepared.packageManager).toBe("bun")
  expect(prepared.bootstrapRan).toBe(true)
  expect(
    normalizeLineEndings(await readFile(join(created.worktreeDir, ".bootstrap-marker"), "utf-8")),
  ).toBe("install\n--frozen-lockfile\n")

  await cleanupWorktree(repoDir, created)
})

test("ambiguous lockfiles skip inferred bootstrap", async () => {
  const repoDir = await createRepoFixture()
  await writeFile(join(repoDir, "bun.lock"), "", "utf-8")
  await writeFile(join(repoDir, "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n", "utf-8")
  const binDir = await createFakePackageManager("bun", {
    exitCode: 0,
    outputFile: ".bootstrap-marker",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-ambiguous-lockfiles",
  })

  process.env.PATH = `${binDir}${delimiter}${originalPath ?? ""}`

  const prepared = await prepareFreshWorktree({
    repoRoot: repoDir,
    worktreeDir: created.worktreeDir,
    config: {
      seedEnabled: false,
    },
  })

  expect(prepared.packageManager).toBeNull()
  expect(prepared.bootstrapRan).toBe(false)
  expect(existsSync(join(created.worktreeDir, ".bootstrap-marker"))).toBe(false)

  await cleanupWorktree(repoDir, created)
})

async function createRepoFixture(
  options: {
    packageJson?: Record<string, unknown>
    trackedFiles?: Record<string, string>
  } = {},
) {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-worktree-bootstrap-repo-"))
  cleanup.push(repoDir)

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify(options.packageJson ?? { name: "repo", private: true }, null, 2),
    "utf-8",
  )

  for (const [relativePath, content] of Object.entries(options.trackedFiles ?? {})) {
    await mkdir(join(repoDir, pathDir(relativePath)), { recursive: true })
    await writeFile(join(repoDir, relativePath), content, "utf-8")
  }

  await runGit(repoDir, ["init"])
  await runGit(repoDir, ["config", "core.autocrlf", "false"])
  await runGit(repoDir, ["config", "user.email", "bot@example.com"])
  await runGit(repoDir, ["config", "user.name", "Bot"])
  await runGit(repoDir, ["add", "."])
  await runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

async function cleanupWorktree(
  repoDir: string,
  created: Awaited<ReturnType<typeof createWorktree>>,
) {
  await deleteWorktree({
    cwd: repoDir,
    worktreeDir: created.worktreeDir,
    branchName: created.branchName,
    poweredBy: created.poweredBy,
  })
}

async function createFakePackageManager(
  name: string,
  options: {
    exitCode: number
    outputFile: string
  },
) {
  const binDir = await mkdtemp(join(tmpdir(), `goddard-${name}-bin-`))
  cleanup.push(binDir)

  const scriptPath = join(binDir, process.platform === "win32" ? `${name}.cmd` : name)
  await writeFile(scriptPath, createFakePackageManagerScript(options), "utf-8")
  await chmod(scriptPath, 0o755)

  return binDir
}

async function runGit(cwd: string, args: string[]) {
  const result = await new Promise<{ status: number | null }>((resolvePromise, reject) => {
    const child = spawn("git", args, {
      cwd,
      stdio: "ignore",
    })

    child.on("error", reject)
    child.on("close", (status) => {
      resolvePromise({ status })
    })
  })

  expect(result.status).toBe(0)
}

function pathDir(relativePath: string) {
  const parentDir = dirname(relativePath)
  return parentDir.length > 0 ? parentDir : "."
}

function normalizeLineEndings(value: string) {
  return value.replace(/\r\n/g, "\n")
}

function createFakePackageManagerScript(options: { exitCode: number; outputFile: string }) {
  if (process.platform === "win32") {
    return [
      "@echo off",
      "(",
      "for %%A in (%*) do echo %%~A",
      `) > "${options.outputFile}"`,
      `exit /b ${options.exitCode}`,
      "",
    ].join("\r\n")
  }

  return [
    "#!/bin/sh",
    `printf '%s\\n' "$@" > "${options.outputFile}"`,
    `exit ${options.exitCode}`,
    "",
  ].join("\n")
}
