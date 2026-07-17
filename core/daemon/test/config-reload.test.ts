import { mkdir, mkdtemp, readFile, rename, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { getGlobalConfigPath, getLocalConfigPath } from "@goddard-ai/paths/node"
import { REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED } from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import type { DaemonSession } from "@goddard-ai/session/schema"
import { afterEach, expect, test } from "bun:test"

import { settleWindowsHandles } from "../../test-support/windows-fixtures.ts"
import type { BackendClient } from "../src/backend.ts"
import { createConfigManager } from "../src/config-manager.ts"
import { createDaemonRuntime, startDaemonServer, type DaemonServer } from "../src/ipc.ts"
import type { DaemonRuntime } from "../src/runtime.ts"
import { createWrappedNodeAgent } from "./acp-fixture.ts"
import { resetComposedDaemonStore, type ComposedDaemonStore } from "./support/store.ts"
import { removeTemporaryPath } from "./support/temp.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const AGENT_LAUNCH_TEST_TIMEOUT_MS = 20_000
const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"
const fastFixtureAgentPath = fileURLToPath(
  new URL("./fixtures/fast-acp-agent.mjs", import.meta.url),
)
let db: ComposedDaemonStore = resetComposedDaemonStore({ filename: ":memory:" })

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
  db = resetComposedDaemonStore({ filename: ":memory:" })

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

test("config manager promotes valid root config edits and preserves the last good snapshot after invalid edits", async () => {
  await useTempHome()
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-config-manager-repo-"))
  cleanup.push(() => removeTemporaryPath(repoDir))
  const reloadFailedEvents: Array<{
    name: string
    payload: {
      watchScope?: string
      localConfigPath?: string
      errorMessage?: string
    }
  }> = []
  const configManager = createConfigManager({
    onReloadFailed: (payload) => {
      reloadFailedEvents.push({
        name: "config.reload.failed",
        payload,
      })
    },
  })
  cleanup.push(() => closeConfigManager(configManager))

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

  const previousLocalFailureCount = countLocalReloadFailures(reloadFailedEvents)
  await writeFile(localConfigPath, "{ invalid json\n", "utf-8")

  await waitFor(() => {
    return countLocalReloadFailures(reloadFailedEvents) > previousLocalFailureCount
  })
  expect(reloadFailedEvents.at(-1)).toMatchObject({
    name: "config.reload.failed",
    payload: {
      watchScope: "local",
      localConfigPath,
    },
  })

  const fallbackSnapshot = await configManager.getRootConfig(repoDir)
  // Some filesystems report duplicate valid-write events before the final invalid edit.
  expect(fallbackSnapshot.version).toBeGreaterThanOrEqual(recoveredSnapshot!.version)
  expect(fallbackSnapshot.config.session?.agent).toBe("claude-acp")
  expect(fallbackSnapshot.config.actions?.session?.agent).toBe("gemini-acp")
})

test("config manager serializes atomic global updates and preserves unrelated config", async () => {
  await useTempHome()
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-config-writer-repo-"))
  cleanup.push(() => removeTemporaryPath(repoDir))
  await writeGlobalRootConfig({
    agents: {
      default: "codex-acp",
    },
  })

  const configManager = createConfigManager()
  cleanup.push(() => closeConfigManager(configManager))
  await configManager.getRootConfig(repoDir)

  await Promise.all([
    configManager.updateGlobalConfig((config) => ({
      ...config,
      sessionProfiles: {
        "codex-acp": {
          routine: {
            model: "gpt-5.4-mini-low",
            thoughtLevel: "low",
            approvalMode: "default",
          },
        },
      },
    })),
    configManager.updateGlobalConfig((config) => ({
      ...config,
      security: {
        pullRequests: {
          submit: "deny",
        },
      },
    })),
  ])

  const persisted = JSON.parse(await readFile(getGlobalConfigPath(), "utf8"))
  expect(persisted).toMatchObject({
    $schema: rootConfigSchemaUrl,
    agents: {
      default: "codex-acp",
    },
    security: {
      pullRequests: {
        submit: "deny",
      },
    },
    sessionProfiles: {
      "codex-acp": {
        routine: {
          model: "gpt-5.4-mini-low",
        },
      },
    },
  })
  expect(configManager.getLastKnownRootConfig(repoDir)?.config).toMatchObject({
    agents: {
      default: "codex-acp",
    },
    security: {
      pullRequests: {
        submit: "deny",
      },
    },
    sessionProfiles: {
      "codex-acp": {
        routine: {
          model: "gpt-5.4-mini-low",
        },
      },
    },
  })
})

test("session profile IPC manages fixed profiles in global config", async () => {
  await useTempHome()
  await writeGlobalRootConfig({
    agents: {
      default: "codex-acp",
    },
  })

  const configManager = createConfigManager()
  cleanup.push(() => closeConfigManager(configManager))
  const daemon = await startServer(configManager)
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  await expect(client.session.profile.list({})).resolves.toEqual({ profiles: {} })

  const configured = await client.session.profile.set({
    agentId: "codex-acp",
    profileId: "debug",
    profile: {
      model: "gpt-5.4-medium",
      thoughtLevel: "medium",
      approvalMode: "default",
    },
  })
  expect(configured.profiles).toEqual({
    "codex-acp": {
      debug: {
        model: "gpt-5.4-medium",
        thoughtLevel: "medium",
        approvalMode: "default",
      },
    },
  })

  const persisted = JSON.parse(await readFile(getGlobalConfigPath(), "utf8"))
  expect(persisted.agents).toEqual({ default: "codex-acp" })
  expect(persisted.sessionProfiles).toEqual(configured.profiles)

  await expect(
    client.session.profile.remove({
      agentId: "codex-acp",
      profileId: "debug",
    }),
  ).resolves.toEqual({ profiles: {} })

  const removed = JSON.parse(await readFile(getGlobalConfigPath(), "utf8"))
  expect(removed.agents).toEqual({ default: "codex-acp" })
  expect(removed.sessionProfiles).toBeUndefined()
})

test(
  "action.run picks up updated root-config agent defaults without restarting the daemon",
  async () => {
    await useTempHome()
    const repoDir = await mkdtemp(join(tmpdir(), "goddard-action-reload-repo-"))
    cleanup.push(() => removeTemporaryPath(repoDir))

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
    cleanup.push(() => closeConfigManager(configManager))
    const daemon = await startServer(configManager)
    const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

    const firstRun = await client.action.run({
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

    const secondRun = await client.action.run({
      actionName: "review",
      cwd: repoDir,
    })
    expect(secondRun.session.agentName).toBe("Node Agent B")

    await client.session.shutdown({ id: firstRun.session.id })
    await client.session.shutdown({ id: secondRun.session.id })
  },
  AGENT_LAUNCH_TEST_TIMEOUT_MS,
)

test(
  "pull request feedback handler picks up updated root-config agent defaults without restarting the daemon",
  async () => {
    await useTempHome()
    const repoDir = await mkdtemp(join(tmpdir(), "goddard-pr-feedback-reload-repo-"))
    cleanup.push(() => removeTemporaryPath(repoDir))

    const agentA = createFixtureAgent("Node Agent A")
    const agentB = createFixtureAgent("Node Agent B")
    await writeGlobalRootConfig({
      session: {
        agent: agentA,
      },
    })

    const configManager = createConfigManager()
    cleanup.push(() => closeConfigManager(configManager))
    const daemon = await startServer(configManager)
    const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
    const feedbackFinishedEvents: Array<{
      name?: string
      payload?: { repository?: string; prNumber?: number; feedbackType?: string; exitCode?: number }
    }> = []
    const feedbackFailedEvents: Array<{
      name?: string
      payload?: { repository?: string; prNumber?: number; feedbackType?: string; phase?: string }
    }> = []
    const abortController = new AbortController()
    const eventStream = await client.events.stream(
      {
        names: ["pull_request.feedback.finished", "pull_request.feedback.failed"],
      },
      {
        signal: abortController.signal,
      },
    )
    const eventsDone = (async () => {
      for await (const event of eventStream) {
        if (event && typeof event === "object" && "name" in event) {
          if ((event as { name?: string }).name === "pull_request.feedback.finished") {
            feedbackFinishedEvents.push(event as (typeof feedbackFinishedEvents)[number])
            continue
          }
          if ((event as { name?: string }).name === "pull_request.feedback.failed") {
            feedbackFailedEvents.push(event as (typeof feedbackFailedEvents)[number])
          }
        }
      }
    })()
    const unsubscribeEvents = () => {
      abortController.abort()
      return eventsDone.catch(() => {})
    }
    const feedbackHandler = daemon.backendEventHandlers.find(
      (handler) => handler.name === "pull-request.feedback",
    )
    expect(feedbackHandler).toBeDefined()
    db.pullRequests.putByUnique(
      {
        host: "github",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
      },
      {
        host: "github",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        cwd: repoDir,
      },
    )

    try {
      await feedbackHandler?.handle(createFeedbackBackendEvent())

      await waitFor(async () => {
        if (feedbackFinishedEvents.length !== 1) {
          return false
        }
        const listed = await client.session.list({ limit: 50 })
        return listed.sessions.length === 1
      })

      const firstListed = await client.session.list({ limit: 50 })
      const firstSessionIds = new Set(
        firstListed.sessions.map((session: DaemonSession) => session.id),
      )
      const firstSession = await client.session.get({ id: firstListed.sessions[0].id })
      expect(firstSession.session.agentName).toBe("Node Agent A")

      await writeGlobalRootConfig({
        session: {
          agent: agentB,
        },
      })

      await waitFor(() => {
        const agent = configManager.getLastKnownRootConfig(repoDir)?.config.session?.agent
        return typeof agent === "object" && agent?.name === "Node Agent B"
      })

      await feedbackHandler?.handle(createFeedbackBackendEvent())

      await waitFor(async () => {
        if (feedbackFinishedEvents.length !== 2) {
          return false
        }
        const listed = await client.session.list({ limit: 50 })
        return listed.sessions.length === 2
      })

      const secondListed = await client.session.list({ limit: 50 })
      const secondSessionSummary = secondListed.sessions.find(
        (session: DaemonSession) => firstSessionIds.has(session.id) === false,
      )
      expect(secondSessionSummary).toBeTruthy()
      const secondSession = await client.session.get({ id: secondSessionSummary!.id })
      expect(secondSession.session.agentName).toBe("Node Agent B")
      expect(feedbackFailedEvents).toHaveLength(0)
      expect(
        feedbackFinishedEvents.map(
          (event) =>
            `${event.payload?.repository}#${event.payload?.prNumber}:${event.payload?.feedbackType}:${event.payload?.exitCode}`,
        ),
      ).toEqual(["acme/widgets#12:comment:0", "acme/widgets#12:comment:0"])

      for (const sessionId of secondListed.sessions.map((session: DaemonSession) => session.id)) {
        await client.session.shutdown({ id: sessionId })
      }
    } finally {
      await Promise.resolve(unsubscribeEvents()).catch(() => {})
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

function createFeedbackEvent(): Extract<RepoEvent, { type: "comment" }> {
  return {
    type: "comment",
    provider: "github",
    owner: "acme",
    repo: "widgets",
    prNumber: 12,
    author: "alice",
    body: "Please update this.",
    reactionAdded: "eyes",
    createdAt: new Date().toISOString(),
  }
}

function createFeedbackBackendEvent() {
  return {
    name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
    payload: createFeedbackEvent(),
  }
}

async function startServer(configManager: ReturnType<typeof createConfigManager>) {
  const daemonClient = createTestBackendClient()
  const daemonRuntime = await createDaemonRuntime({
    backendClient: daemonClient,
    configManager,
    port: 0,
    store: db,
  })
  const daemon = await startDaemonServer(daemonRuntime)
  const closeDaemonServer = daemon.close
  const server = Object.assign(daemon, {
    backendEventHandlers: daemonRuntime.backendEventHandlers,
    close: () => closeServerAndRuntime(closeDaemonServer, daemonRuntime),
  })
  cleanup.push(async () => {
    await server.close()
  })
  return server
}

async function closeServerAndRuntime(
  closeDaemonServer: DaemonServer["close"],
  runtime: DaemonRuntime,
) {
  await closeDaemonServer()
  await runtime.close()
}

function createTestBackendClient(): BackendClient {
  return {
    auth: {
      device: {
        start: async () => ({
          deviceCode: "dev_1",
          userCode: "ABCD-1234",
          verificationUri: "https://github.com/login/device",
          expiresIn: 900,
          interval: 5,
        }),
        complete: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
      },
      session: {
        current: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
      },
    },
    pullRequests: {
      create: async () => ({ number: 1, url: "https://example.com/pr/1" }),
      managed: async () => ({ managed: true }),
      comments: {
        create: async () => ({ success: true }),
      },
    },
    webhooks: {
      github: async () => ({ type: "noop" }),
    },
    events: {
      stream: async () => emptyBackendEvents(),
    },
  } as unknown as BackendClient
}

async function* emptyBackendEvents(): AsyncIterable<never> {}

async function closeConfigManager(configManager: ReturnType<typeof createConfigManager>) {
  await configManager.close()
  await settleWindowsHandles(250)
}

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-config-reload-home-"))
  process.env.HOME = homeDir
  db = resetComposedDaemonStore()
  cleanup.push(() => removeTemporaryPath(homeDir))
  cleanup.push(async () => {
    db.close()
    db = resetComposedDaemonStore({ filename: ":memory:" })
  })
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

function countLocalReloadFailures(
  events: Array<{ name: string; payload: { watchScope?: string } }>,
) {
  return events.filter(
    (event) => event.name === "config.reload.failed" && event.payload.watchScope === "local",
  ).length
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
