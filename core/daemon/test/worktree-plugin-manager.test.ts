import { getGlobalConfigPath, getLocalConfigPath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"
import { spawn } from "node:child_process"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { createConfigManager } from "../src/config-manager.ts"
import { readMergedRootConfig } from "../src/resolvers/config.ts"
import { createWorktree, deleteWorktree } from "../src/worktrees/index.ts"
import { createWorktreePluginManager } from "../src/worktrees/plugin-manager.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

test("loads a configured path plugin from the global config", async () => {
  const homeDir = await useTempHome()
  const repoDir = await createRepoFixture()
  const configManager = createConfigManager()
  cleanup.push(() => configManager.close())

  const pluginPath = join(homeDir, ".goddard", "plugins", "path-plugin.mjs")
  await mkdir(dirname(pluginPath), { recursive: true })
  await writeFile(
    pluginPath,
    [
      'import { mkdir, rm } from "node:fs/promises"',
      'import { join } from "node:path"',
      "",
      "export const plugin = {",
      '  name: "path-plugin",',
      "  isApplicable() {",
      "    return true",
      "  },",
      "  async setup(options) {",
      '    const worktreeDir = join(options.cwd, ".path-plugin", options.branchName)',
      "    await mkdir(worktreeDir, { recursive: true })",
      "    return worktreeDir",
      "  },",
      "  async cleanup(worktreeDir) {",
      "    await rm(worktreeDir, { recursive: true, force: true })",
      "    return true",
      "  },",
      "}",
      "",
    ].join("\n"),
    "utf-8",
  )

  await writeGlobalRootConfig({
    worktrees: {
      plugins: [
        {
          type: "path",
          path: "plugins/path-plugin.mjs",
          export: "plugin",
        },
      ],
    },
  })

  const pluginManager = createWorktreePluginManager({ configManager })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-1",
    plugins: await pluginManager.getPlugins(repoDir),
  })

  expect(created.poweredBy).toBe("path-plugin")
  expect(created.worktreeDir).toBe(join(created.repoRoot, ".path-plugin", "feature-1"))

  await expect(
    deleteWorktree({
      cwd: created.repoRoot,
      worktreeDir: created.worktreeDir,
      branchName: created.branchName,
      poweredBy: created.poweredBy,
      plugins: await pluginManager.getPlugins(repoDir),
    }),
  ).resolves.toBe(true)
})

test("loads a configured package plugin from a resolvable package specifier", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()
  const configManager = createConfigManager()
  cleanup.push(() => configManager.close())
  const packageDir = fileURLToPath(
    new URL("../../../node_modules/@acme/goddard-worktree-plugin", import.meta.url),
  )

  await mkdir(packageDir, { recursive: true })
  cleanup.push(() => rm(packageDir, { recursive: true, force: true }))
  await writeFile(
    join(packageDir, "package.json"),
    JSON.stringify(
      {
        name: "@acme/goddard-worktree-plugin",
        type: "module",
        exports: "./index.mjs",
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(
    join(packageDir, "index.mjs"),
    [
      'import { mkdir, rm } from "node:fs/promises"',
      'import { join } from "node:path"',
      "",
      "export default {",
      '  name: "package-plugin",',
      "  isApplicable() {",
      "    return true",
      "  },",
      "  async setup(options) {",
      '    const worktreeDir = join(options.cwd, ".package-plugin", options.branchName)',
      "    await mkdir(worktreeDir, { recursive: true })",
      "    return worktreeDir",
      "  },",
      "  async cleanup(worktreeDir) {",
      "    await rm(worktreeDir, { recursive: true, force: true })",
      "    return true",
      "  },",
      "}",
      "",
    ].join("\n"),
    "utf-8",
  )

  await writeGlobalRootConfig({
    worktrees: {
      plugins: [
        {
          type: "package",
          package: "@acme/goddard-worktree-plugin",
        },
      ],
    },
  })

  const pluginManager = createWorktreePluginManager({ configManager })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "feature-2",
    plugins: await pluginManager.getPlugins(repoDir),
  })

  expect(created.poweredBy).toBe("package-plugin")
  expect(created.worktreeDir).toBe(join(created.repoRoot, ".package-plugin", "feature-2"))

  await expect(
    deleteWorktree({
      cwd: created.repoRoot,
      worktreeDir: created.worktreeDir,
      branchName: created.branchName,
      poweredBy: created.poweredBy,
      plugins: await pluginManager.getPlugins(repoDir),
    }),
  ).resolves.toBe(true)
})

test("rejects worktree plugin references in repository-local config", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeLocalRootConfig(repoDir, {
    worktrees: {
      plugins: [
        {
          type: "path",
          path: "./plugin.mjs",
        },
      ],
    },
  })

  await expect(readMergedRootConfig(repoDir)).rejects.toThrow(
    "`worktrees.plugins` is only supported in the global Goddard config",
  )
})

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-worktree-plugin-home-"))
  process.env.HOME = homeDir
  cleanup.push(() => rm(homeDir, { recursive: true, force: true }))
  return homeDir
}

async function writeGlobalRootConfig(config: Record<string, unknown>) {
  await writeRootConfig(getGlobalConfigPath(), config)
}

async function writeLocalRootConfig(repoDir: string, config: Record<string, unknown>) {
  await writeRootConfig(getLocalConfigPath(repoDir), config)
}

async function writeRootConfig(configPath: string, config: Record<string, unknown>) {
  await mkdir(dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify({ $schema: rootConfigSchemaUrl, ...config }, null, 2)}\n`,
    "utf-8",
  )
}

async function createRepoFixture() {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-worktree-plugin-repo-"))
  cleanup.push(() => rm(repoDir, { recursive: true, force: true }))

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "repo", private: true }, null, 2),
    "utf-8",
  )

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
