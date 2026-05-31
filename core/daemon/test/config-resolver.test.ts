import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { getGlobalConfigPath, getLocalConfigPath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"

import { readMergedRootConfig } from "../src/resolvers/config.ts"

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

test("allows repository-local worktree bootstrap config and replaces inherited arrays", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeGlobalRootConfig({
    worktrees: {
      bootstrap: {
        enabled: true,
        packageManager: "bun",
        installArgs: ["--global-flag"],
        seedNames: ["node_modules", "dist"],
        seedPaths: ["global/path"],
      },
    },
  })

  await writeLocalRootConfig(repoDir, {
    worktrees: {
      bootstrap: {
        installArgs: ["--local-flag"],
        seedNames: [".turbo"],
        seedPaths: ["local/path"],
      },
    },
  })

  await expect(readMergedRootConfig(repoDir)).resolves.toMatchObject({
    config: {
      worktrees: {
        bootstrap: {
          enabled: true,
          packageManager: "bun",
          installArgs: ["--local-flag"],
          seedNames: [".turbo"],
          seedPaths: ["local/path"],
        },
      },
    },
  })
})

test("merges agents.default from repository-local config", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeGlobalRootConfig({
    agents: {
      default: "global-agent",
    },
  })

  await writeLocalRootConfig(repoDir, {
    agents: {
      default: "local-agent",
    },
  })

  await expect(readMergedRootConfig(repoDir)).resolves.toMatchObject({
    config: {
      agents: {
        default: "local-agent",
      },
    },
  })
})

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-config-resolver-home-"))
  process.env.HOME = homeDir
  cleanup.push(() => rm(homeDir, { recursive: true, force: true }))
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
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-config-resolver-repo-"))
  cleanup.push(() => rm(repoDir, { recursive: true, force: true }))

  return repoDir
}
