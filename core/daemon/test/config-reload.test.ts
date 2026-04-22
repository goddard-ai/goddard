import { mkdir, mkdtemp, rename, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { getGlobalConfigPath, getLocalConfigPath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"

import { createConfigManager } from "../src/config-manager.ts"
import { resolveRuntimeConfig } from "../src/config.ts"
import { SetupContext } from "../src/context.ts"
import type { FeedbackEvent } from "../src/feedback.ts"
import { startDaemonServer } from "../src/ipc.ts"
import { configureLogging } from "../src/logging.ts"
import { db, resetDb } from "../src/persistence/store.ts"
import { runPrFeedbackFlow } from "../src/pr-feedback-run.ts"
import { createWrappedNodeAgent } from "./acp-fixture.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const AGENT_LAUNCH_TEST_TIMEOUT_MS = 20_000
const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"
const fastFixtureAgentPath = fileURLToPath(
  new URL("./fixtures/fast-acp-agent.mjs", import.meta.url),
)

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }

  resetDb({ filename: ":memory:" })

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

test("config manager promotes valid root config edits and preserves the last good snapshot after invalid edits", async () => {
  await useTempHome()
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-config-manager-repo-"))
  cleanup.push(() => rm(repoDir, { recursive: true, force: true }))

  const output: string[] = []
  const restoreLogging = configureLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
  })
  cleanup.push(async () => {
    restoreLogging()
  })

  const configManager = createConfigManager()
  cleanup.push(() => configManager.close())

  const firstSnapshot = await configManager.getRootConfig(repoDir)
  expect(firstSnapshot.version).toBe(1)

  await writeGlobalRootConfig({
    session: {
      agent: "pi-acp",
    },
  })

  await waitFor(() => {
    return configManager.getLastKnownRootConfig(repoDir)?.config.session?.agent === "pi-acp"
  })

  const globalSnapshot = configManager.getLastKnownRootConfig(repoDir)
  expect(globalSnapshot).toBeTruthy()
  expect(globalSnapshot!.version).toBe(2)

  await writeLocalRootConfig(repoDir, {
    actions: {
      session: {
        agent: "codex-acp",
      },
    },
  })

  await waitFor(() => {
    return (
      configManager.getLastKnownRootConfig(repoDir)?.config.actions?.session?.agent === "codex-acp"
    )
  })

  const localSnapshot = configManager.getLastKnownRootConfig(repoDir)
  expect(localSnapshot).toBeTruthy()
  expect(localSnapshot!.version).toBe(3)

  await replaceRootConfigAtomically(getGlobalConfigPath(), {
    session: {
      agent: "claude-acp",
    },
  })

  await waitFor(() => {
    const snapshot = configManager.getLastKnownRootConfig(repoDir)
    return (
      snapshot?.config.session?.agent === "claude-acp" &&
      snapshot?.config.actions?.session?.agent === "codex-acp"
    )
  })

  const renamedSnapshot = configManager.getLastKnownRootConfig(repoDir)
  expect(renamedSnapshot).toBeTruthy()

  const localConfigPath = getLocalConfigPath(repoDir)
  const recoveredWrite = Bun.sleep(75).then(() =>
    writeLocalRootConfig(repoDir, {
      actions: {
        session: {
          agent: "gemini-acp",
        },
      },
    }),
  )
  await writeFile(localConfigPath, "{ invalid json\n", "utf-8")

  await waitFor(() => {
    return (
      configManager.getLastKnownRootConfig(repoDir)?.config.actions?.session?.agent === "gemini-acp"
    )
  })
  await recoveredWrite

  const recoveredSnapshot = configManager.getLastKnownRootConfig(repoDir)
  expect(recoveredSnapshot).toBeTruthy()
  expect(recoveredSnapshot!.version).toBeGreaterThan(renamedSnapshot!.version)

  await writeFile(localConfigPath, "{ invalid json\n", "utf-8")

  await waitFor(() => {
    return readLogs(output).some(
      (entry) => entry.event === "config.reload_failed" && entry.watchScope === "local",
    )
  })

  const fallbackSnapshot = await configManager.getRootConfig(repoDir)
  expect(fallbackSnapshot.version).toBe(recoveredSnapshot!.version)
  expect(fallbackSnapshot.config.session?.agent).toBe("claude-acp")
  expect(fallbackSnapshot.config.actions?.session?.agent).toBe("gemini-acp")
})

test(
  "actionRun picks up updated root-config agent defaults without restarting the daemon",
  async () => {
    await useTempHome()
    const repoDir = await mkdtemp(join(tmpdir(), "goddard-action-reload-repo-"))
    cleanup.push(() => rm(repoDir, { recursive: true, force: true }))

    const agentA = createFixtureAgent("Node Agent A")
    const agentB = createFixtureAgent("Node Agent B")
    await writeGlobalRootConfig({
      session: {
        agent: agentA,
      },
      actions: {
        session: {
          agent: agentA,
        },
      },
    })
    await writePromptOnlyAction(repoDir, "review", "Say hello in one sentence.")

    const configManager = createConfigManager()
    cleanup.push(() => configManager.close())
    const daemon = await startServer(configManager)
    const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

    const firstRun = await client.send("actionRun", {
      actionName: "review",
      cwd: repoDir,
    })
    expect(firstRun.session.agentName).toBe("Node Agent A")

    await writeGlobalRootConfig({
      session: {
        agent: agentB,
      },
      actions: {
        session: {
          agent: agentB,
        },
      },
    })

    await waitFor(() => {
      const agent = configManager.getLastKnownRootConfig(repoDir)?.config.actions?.session?.agent
      return typeof agent === "object" && agent?.name === "Node Agent B"
    })

    const secondRun = await client.send("actionRun", {
      actionName: "review",
      cwd: repoDir,
    })
    expect(secondRun.session.agentName).toBe("Node Agent B")

    await client.send("sessionShutdown", { id: firstRun.session.id })
    await client.send("sessionShutdown", { id: secondRun.session.id })
  },
  AGENT_LAUNCH_TEST_TIMEOUT_MS,
)

test(
  "runPrFeedbackFlow picks up updated root-config agent defaults without restarting the daemon",
  async () => {
    await useTempHome()
    const repoDir = await mkdtemp(join(tmpdir(), "goddard-pr-feedback-reload-repo-"))
    cleanup.push(() => rm(repoDir, { recursive: true, force: true }))

    const agentA = createFixtureAgent("Node Agent A")
    const agentB = createFixtureAgent("Node Agent B")
    await writeGlobalRootConfig({
      session: {
        agent: agentA,
      },
    })

    const configManager = createConfigManager()
    cleanup.push(() => configManager.close())
    const daemon = await startServer(configManager)
    const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

    const firstExitCode = await runPrFeedbackFlow({
      event: createFeedbackEvent(),
      prompt: "Reply briefly.",
      daemonUrl: daemon.daemonUrl,
      agentBinDir: fileURLToPath(new URL("../agent-bin", import.meta.url)),
      configManager,
      resolveProjectDir: () => repoDir,
    })
    expect(firstExitCode).toBe(0)

    const firstSessions = db.sessions.findMany()
    const firstSessionIds = new Set(firstSessions.map((session) => session.id))
    expect(firstSessions.map((session) => session.agentName)).toEqual(["Node Agent A"])

    await writeGlobalRootConfig({
      session: {
        agent: agentB,
      },
    })

    await waitFor(() => {
      const agent = configManager.getLastKnownRootConfig(repoDir)?.config.session?.agent
      return typeof agent === "object" && agent?.name === "Node Agent B"
    })

    const secondExitCode = await runPrFeedbackFlow({
      event: createFeedbackEvent(),
      prompt: "Reply briefly.",
      daemonUrl: daemon.daemonUrl,
      agentBinDir: fileURLToPath(new URL("../agent-bin", import.meta.url)),
      configManager,
      resolveProjectDir: () => repoDir,
    })
    expect(secondExitCode).toBe(0)

    const secondSession = db.sessions
      .findMany()
      .find((session) => firstSessionIds.has(session.id) === false)
    expect(secondSession?.agentName).toBe("Node Agent B")

    for (const sessionId of [...firstSessionIds, secondSession?.id].filter(
      (value) => value != null,
    )) {
      await client.send("sessionShutdown", { id: sessionId })
    }
  },
  AGENT_LAUNCH_TEST_TIMEOUT_MS,
)

function createFixtureAgent(name: string) {
  return {
    ...createWrappedNodeAgent(fastFixtureAgentPath),
    id: "fast-node-agent",
    name,
  }
}

function createFeedbackEvent(): FeedbackEvent {
  return {
    type: "comment",
    owner: "acme",
    repo: "widgets",
    prNumber: 12,
    author: "alice",
    body: "Please update this.",
    reactionAdded: "eyes",
    createdAt: new Date().toISOString(),
  }
}

async function startServer(configManager: ReturnType<typeof createConfigManager>) {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-config-reload-daemon-"))
  const runtime = resolveRuntimeConfig({
    socketPath: join(socketDir, "daemon.sock"),
  })
  const daemonClient = {
    auth: {
      startDeviceFlow: async () => ({
        deviceCode: "dev_1",
        userCode: "ABCD-1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      }),
      completeDeviceFlow: async () => ({
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      }),
      whoami: async () => ({
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      }),
      logout: async () => {},
    },
    pr: {
      create: async () => ({ number: 1, url: "https://example.com/pr/1" }),
      reply: async () => ({ success: true }),
    },
  }
  const daemon = await SetupContext.run({ runtime, configManager }, () =>
    startDaemonServer(daemonClient, {
      socketPath: runtime.socketPath,
      agentBinDir: runtime.agentBinDir,
    }),
  )
  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })
  return daemon
}

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-config-reload-home-"))
  process.env.HOME = homeDir
  resetDb()
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

async function replaceRootConfigAtomically(configPath: string, config: Record<string, unknown>) {
  const tempPath = `${configPath}.tmp`
  await writeRootConfig(tempPath, config)
  await rename(tempPath, configPath)
}

async function writePromptOnlyAction(repoDir: string, actionName: string, prompt: string) {
  const actionsDir = join(repoDir, ".goddard", "actions")
  await mkdir(actionsDir, { recursive: true })
  await writeFile(join(actionsDir, `${actionName}.md`), `${prompt}\n`, "utf-8")
}

function readLogs(lines: string[]) {
  return lines
    .flatMap((chunk) => chunk.split("\n"))
    .filter((line) => line.trim().length > 0)
    .map((line) => JSON.parse(line) as Record<string, unknown>)
}

async function waitFor(
  predicate: () => boolean | Promise<boolean>,
  { timeoutMs = 2_000, intervalMs = 25 } = {},
) {
  const deadline = Date.now() + timeoutMs

  while (true) {
    if (await predicate()) {
      return
    }

    if (Date.now() >= deadline) {
      throw new Error("Timed out waiting for condition")
    }

    await Bun.sleep(intervalMs)
  }
}
