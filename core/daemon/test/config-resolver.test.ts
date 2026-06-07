import { mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { getGlobalConfigPath, getLocalConfigPath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"

import { readMergedRootConfig } from "../src/resolvers/config.ts"
import { removeTemporaryPath } from "./support/temp.ts"

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

test("merges session idle-shutdown duration from repository-local config", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeGlobalRootConfig({
    sessions: {
      idleShutdown: "15m",
    },
  })

  await writeLocalRootConfig(repoDir, {
    sessions: {
      idleShutdown: "30s",
    },
  })

  await expect(readMergedRootConfig(repoDir)).resolves.toMatchObject({
    config: {
      sessions: {
        idleShutdown: "30s",
      },
    },
  })
})

test("rejects disabled session idle-shutdown config", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeLocalRootConfig(repoDir, {
    sessions: {
      idleShutdown: "0m",
    },
  })

  await expect(readMergedRootConfig(repoDir)).rejects.toThrow(
    "Use a positive duration like `15m`, `1h`, `30s`, or `500ms`.",
  )
})

test("merges session env policy restrictions with global fixed env", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeGlobalRootConfig({
    sessions: {
      envPolicy: {
        inherit: true,
        allow: ["PATH", "GITHUB_TOKEN"],
        set: {
          GITHUB_TOKEN: "global-token",
        },
      },
    },
  })

  await writeLocalRootConfig(repoDir, {
    sessions: {
      envPolicy: {
        inherit: false,
        block: ["GITHUB_TOKEN"],
      },
    },
  })

  await expect(readMergedRootConfig(repoDir)).resolves.toMatchObject({
    config: {
      sessions: {
        envPolicy: {
          inherit: false,
          allow: ["PATH", "GITHUB_TOKEN"],
          block: ["GITHUB_TOKEN"],
          set: {
            GITHUB_TOKEN: "global-token",
          },
        },
      },
    },
  })
})

test("rejects fixed session env injection in repository-local config", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeLocalRootConfig(repoDir, {
    sessions: {
      envPolicy: {
        set: {
          GITHUB_TOKEN: "local-token",
        },
      },
    },
  })

  await expect(readMergedRootConfig(repoDir)).rejects.toThrow(
    "`sessions.envPolicy.set` is only supported in the global Goddard config",
  )
})

test("repository-local security policy can tighten pull request operations", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeGlobalRootConfig({
    security: {
      pullRequests: {
        submit: "allow",
        reply: "allow",
      },
    },
  })

  await writeLocalRootConfig(repoDir, {
    security: {
      pullRequests: {
        submit: "deny",
      },
    },
  })

  await expect(readMergedRootConfig(repoDir)).resolves.toMatchObject({
    config: {
      security: {
        pullRequests: {
          submit: "deny",
          reply: "allow",
        },
      },
    },
  })
})

test("rejects repository-local security policy that loosens pull request operations", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()

  await writeLocalRootConfig(repoDir, {
    security: {
      pullRequests: {
        reply: "allow",
      },
    },
  })

  await expect(readMergedRootConfig(repoDir)).rejects.toThrow(
    '`security.pullRequests.reply` cannot be set to "allow" in repository-local config',
  )
})

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-config-resolver-home-"))
  process.env.HOME = homeDir
  cleanup.push(() => removeTemporaryPath(homeDir))
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
  cleanup.push(() => removeTemporaryPath(repoDir))

  return repoDir
}
