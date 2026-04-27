import { spawnSync } from "node:child_process"
import { randomUUID } from "node:crypto"
import { existsSync } from "node:fs"
import { chmod, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import {
  getAcpRegistryCacheDir,
  getGlobalConfigPath,
  getLocalConfigPath,
} from "@goddard-ai/paths/node"
import { afterAll, afterEach, expect, test } from "bun:test"

import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"
import { db, resetDb } from "../src/persistence/store.ts"
import { matchAcpRequest } from "../src/session/acp.ts"
import { createWrappedNodeAgent } from "./acp-fixture.ts"

const queueAgentPath = fileURLToPath(new URL("./fixtures/queue-agent.mjs", import.meta.url))
const chunkingAgentPath = createRequire(import.meta.url).resolve("./fixtures/chunking-agent.mjs")
const launchPreviewAgentPath = fileURLToPath(
  new URL("./fixtures/launch-preview-agent.mjs", import.meta.url),
)

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const originalPath = process.env.PATH
const fastFixtureAgentPath = createRequire(import.meta.url).resolve("./fixtures/fast-acp-agent.mjs")
const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"
let sharedHomeDir: string | null = null

afterEach(async () => {
  if (originalPath === undefined) {
    delete process.env.PATH
  } else {
    process.env.PATH = originalPath
  }

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

afterAll(async () => {
  resetDb({ filename: ":memory:" })

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  if (sharedHomeDir) {
    await rm(sharedHomeDir, { recursive: true, force: true })
  }
})

test("daemon revokes session tokens when agent processes exit", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const permissions = db.sessions.get(created.session.id)?.permissions ?? null
  const token = db.sessions.get(created.session.id)?.token ?? null
  expect(permissions).toBeTruthy()
  expect(typeof token).toBe("string")

  await client.send("sessionShutdown", { id: created.session.id })

  await waitFor(async () => {
    return db.sessions.get(created.session.id)?.permissions == null
  })

  expect(db.sessions.get(created.session.id)?.permissions ?? null).toBeNull()
})

test("daemon persists repository context into durable session storage", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    repository: "acme/widgets",
    prNumber: 12,
    workforce: { agentId: "reviewer", requestId: "req-1" },
  })

  const storedRecord = db.sessions.get(created.session.id) ?? null
  const workforceRecord =
    db.workforces.first({
      where: { sessionId: created.session.id },
    }) ?? null
  const workforce = await client.send("sessionWorkforce", {
    id: created.session.id,
  })

  expect(created.session.repository).toBe("acme/widgets")
  expect(created.session.prNumber).toBe(12)
  expect(storedRecord).toMatchObject({
    repository: "acme/widgets",
    prNumber: 12,
  })
  expect(storedRecord?.metadata ?? null).toBeNull()
  expect(workforceRecord).toMatchObject({
    sessionId: created.session.id,
    agentId: "reviewer",
    requestId: "req-1",
  })
  expect(created.session.metadata ?? null).toBeNull()
  expect(workforce.workforce).toMatchObject({
    sessionId: created.session.id,
    agentId: "reviewer",
    requestId: "req-1",
  })

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon resolves the default agent for direct session creation", async () => {
  await useTempHome()
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  await writeGlobalRootConfig({
    session: {
      agent: createWrappedNodeAgent(exampleAgentPath),
    },
  })

  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  expect(created.session.agentName).toBe("Node Agent")

  await client.send("sessionShutdown", { id: created.session.id })
})

test("loadable sessions remain reconnectable after shutdown", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionShutdown", { id: created.session.id })
  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)

  const session = await client.send("sessionGet", { id: created.session.id })
  const history = await client.send("sessionHistory", {
    id: created.session.id,
  })
  const promptStarts: string[] = []
  const promptStops: string[] = []
  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      const message = payload.message as {
        method?: string
        params?: { update?: { content?: { text?: string } } }
        result?: { stopReason?: string }
      }

      if (message.method === "session/update") {
        const updateText = message.params?.update?.content?.text ?? ""
        if (updateText.startsWith("prompt_started:")) {
          promptStarts.push(updateText.slice("prompt_started:".length))
        }
      }

      if (message.result?.stopReason) {
        promptStops.push(message.result.stopReason)
      }
    },
  )

  expect(session.session.connectionMode).toBe("live")
  expect(session.session.activeDaemonSession).toBe(false)
  expect(history.connection).toEqual({
    mode: "live",
    reconnectable: true,
    activeDaemonSession: false,
  })

  const reconnected = await client.send("sessionConnect", {
    id: created.session.id,
  })
  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(
      reconnected.session.acpSessionId,
      "prompt-reload-1",
      "after-shutdown",
    ),
  })
  await waitFor(async () => promptStops.includes("end_turn"))
  await Promise.resolve(unsubscribe()).catch(() => {})

  expect(reconnected.session.connectionMode).toBe("live")
  expect(reconnected.session.activeDaemonSession).toBe(true)
  expect(promptStarts).toContain("after-shutdown")

  await client.send("sessionShutdown", { id: created.session.id })
})

test("loadable sessions remain reconnectable after daemon restart", async () => {
  await useTempHome()

  const daemonA = await startServer({ useExistingHome: true })
  const clientA = createDaemonIpcClient({ daemonUrl: daemonA.daemonUrl })
  const created = await clientA.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await daemonA.close()
  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)

  const daemonB = await startServer({ useExistingHome: true })
  const clientB = createDaemonIpcClient({ daemonUrl: daemonB.daemonUrl })
  const reloadedSession = await clientB.send("sessionGet", {
    id: created.session.id,
  })
  const history = await clientB.send("sessionHistory", {
    id: created.session.id,
  })

  expect(reloadedSession.session.connectionMode).toBe("live")
  expect(reloadedSession.session.activeDaemonSession).toBe(false)
  expect(history.connection).toEqual({
    mode: "live",
    reconnectable: true,
    activeDaemonSession: false,
  })

  const connected = await clientB.send("sessionConnect", {
    id: created.session.id,
  })
  expect(connected.session.connectionMode).toBe("live")
  expect(connected.session.activeDaemonSession).toBe(true)

  await clientB.send("sessionShutdown", { id: created.session.id })
})

test("session reconnect fails when the resolved agent no longer supports ACP session/load", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionShutdown", { id: created.session.id })
  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)

  db.sessions.update(created.session.id, {
    agent: createWrappedNodeAgent(chunkingAgentPath),
  })

  await expect(client.send("sessionConnect", { id: created.session.id })).rejects.toThrow(
    /does not support session\/load/i,
  )

  const session = await client.send("sessionGet", { id: created.session.id })
  expect(session.session.connectionMode).toBe("live")
  expect(session.session.activeDaemonSession).toBe(false)
  expect(session.session.acpSessionId).toBe(created.session.acpSessionId)
})

test("daemon persists ACP stop reasons on the session record", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(fastFixtureAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    initialPrompt: "Say hello in one sentence.",
    oneShot: true,
  })

  expect(created.session.stopReason).toBe("end_turn")
  expect(db.sessions.get(created.session.id)?.stopReason).toBe("end_turn")
})

test("daemon coalesces stored agent message chunks while keeping the live stream granular", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(chunkingAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const liveChunks: string[] = []
  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      const message = payload.message as {
        method?: string
        params?: {
          update?: {
            content?: { text?: string }
            sessionUpdate?: string
          }
        }
      }

      if (message.method !== "session/update") {
        return
      }

      if (message.params?.update?.sessionUpdate === "agent_message_chunk") {
        liveChunks.push(message.params.update.content?.text ?? "")
      }
    },
  )

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "Say hello."),
  })

  await waitFor(async () => {
    return db.sessions.get(created.session.id)?.stopReason === "end_turn" && liveChunks.length === 3
  })

  await Promise.resolve(unsubscribe()).catch(() => {})

  expect(liveChunks).toEqual(["Chunked ", "response", "."])

  const history = await client.send("sessionHistory", {
    id: created.session.id,
  })
  const chunkMessages = history.turns
    .flatMap((turn) => turn.messages)
    .filter((message) => {
      return (
        typeof message === "object" &&
        message !== null &&
        "method" in message &&
        message.method === "session/update" &&
        "params" in message &&
        typeof message.params === "object" &&
        message.params !== null &&
        "update" in message.params &&
        typeof message.params.update === "object" &&
        message.params.update !== null &&
        "sessionUpdate" in message.params.update &&
        message.params.update.sessionUpdate === "agent_message_chunk"
      )
    })

  expect(chunkMessages).toHaveLength(1)
  expect(chunkMessages[0]).toMatchObject({
    method: "session/update",
    params: {
      update: {
        sessionUpdate: "agent_message_chunk",
        content: {
          type: "text",
          text: "Chunked response.",
        },
      },
    },
  })

  const turnRecord =
    db.sessionTurns.first({
      where: { sessionId: created.session.id },
    }) ?? null
  expect(history.turns).toHaveLength(1)
  expect(turnRecord?.messages).toEqual(history.turns[0]?.messages)
})

test("daemon creates placeholder session titles before any user prompt is sent", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(fastFixtureAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  expect(created.session.title).toBe("New session")
  expect(created.session.titleState).toBe("placeholder")

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon derives a fallback title immediately when the session starts with an initial prompt", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(fastFixtureAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    initialPrompt: "Review the worktree bootstrap flow for race conditions.",
  })

  expect(created.session.title).toBe("Review the worktree bootstrap flow for")
  expect(created.session.titleState).toBe("fallback")

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon promotes placeholder titles after the first later prompt is accepted", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(
      created.session.acpSessionId,
      "prompt-title-1",
      "Audit the retry policy for loop failures.",
    ),
  })

  await waitFor(async () => db.sessions.get(created.session.id)?.titleState === "fallback")

  expect(db.sessions.get(created.session.id)).toMatchObject({
    title: "Audit the retry policy for loop",
    titleState: "fallback",
  })

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon marks pending title generation as failed when provider config is present but unusable", async () => {
  await useTempHome()
  await writeGlobalRootConfig({
    sessionTitles: {
      generator: {
        provider: "openai",
        model: "gpt-4.1-mini",
      },
    },
  })

  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(fastFixtureAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    initialPrompt: "Summarize the retry failure mode.",
  })

  expect(created.session.title).toBe("Summarize the retry failure mode")
  expect(created.session.titleState).toBe("pending")

  await waitFor(async () => db.sessions.get(created.session.id)?.titleState === "failed")

  expect(db.sessions.get(created.session.id)).toMatchObject({
    title: "Summarize the retry failure mode",
    titleState: "failed",
  })

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon lists adapters through the shared registry service and config default", async () => {
  await useTempHome()
  const repoDir = await createRepoFixture()
  const registryCacheDir = getAcpRegistryCacheDir()
  const adapterDir = join(registryCacheDir, "pi-acp")
  await mkdir(join(registryCacheDir, ".git"), { recursive: true })
  await mkdir(adapterDir, { recursive: true })
  await writeFile(
    join(registryCacheDir, ".goddard-registry-state.json"),
    JSON.stringify({
      lastAttemptedSyncAt: "2026-04-11T00:00:00.000Z",
      lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
      lastError: null,
    }),
    "utf8",
  )
  await writeFile(
    join(adapterDir, "agent.json"),
    JSON.stringify(
      {
        id: "pi-acp",
        name: "pi ACP",
        version: "0.0.25",
        description: "ACP adapter for pi coding agent",
        repository: "https://github.com/svkozak/pi-acp",
        authors: ["Sergii Kozak <svkozak@gmail.com>"],
        license: "MIT",
        distribution: {
          npx: {
            package: "pi-acp@0.0.25",
          },
        },
      },
      null,
      2,
    ),
    "utf8",
  )
  await writeFile(
    getGlobalConfigPath(),
    JSON.stringify({
      session: {
        agent: "pi-acp",
      },
    }),
    "utf8",
  )

  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const response = await client.send("adapterList", { cwd: repoDir })

  expect(response.defaultAdapterId).toBe("pi-acp")
  expect(response.registrySource).toBe("cache")
  expect(response.lastSuccessfulSyncAt).toBe("2026-04-11T00:00:00.000Z")
  expect(response.adapters).toHaveLength(1)
  expect(response.adapters[0]).toMatchObject({
    id: "pi-acp",
    unofficial: true,
    source: "registry",
  })
})

test("daemon reconciles interrupted sessions on restart and leaves archived history readable", async () => {
  await useTempHome()

  const sessionId = db.sessions.newId()
  const acpSessionId = `acp-restart-${randomUUID()}`
  const sessionRecord = {
    acpSessionId,
    status: "active",
    stopReason: null,
    agent: "pi-acp",
    agentName: "node",
    cwd: process.cwd(),
    title: "New session",
    titleState: "placeholder",
    mcpServers: [],
    connectionMode: "live",
    supportsLoadSession: false,
    activeDaemonSession: true,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: "tok-restart-1",
    permissions: {
      owner: "acme",
      repo: "widgets",
      allowedPrNumbers: [12],
    },
    metadata: null,
    models: null,
    availableCommands: [],
  } satisfies Parameters<typeof db.sessions.put>[1]
  db.sessions.put(sessionId, sessionRecord)
  db.sessionTurns.create({
    sessionId,
    turnId: "turn-restart-1",
    sequence: 1,
    promptRequestId: "prompt-restart-1",
    startedAt: "2026-04-14T00:00:00.000Z",
    completedAt: "2026-04-14T00:00:01.000Z",
    completionKind: "result",
    stopReason: "end_turn",
    inboxScope: null,
    inboxHeadline: null,
    messages: [
      {
        jsonrpc: "2.0",
        method: "session/update",
        params: { value: "persisted" },
      },
    ],
  })
  db.sessionDiagnostics.create({
    sessionId,
    events: [],
  })
  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const session = await client.send("sessionGet", { id: sessionId })
  expect(session.session.status).toBe("error")
  expect(session.session.connectionMode).toBe("history")
  expect(session.session.activeDaemonSession).toBe(false)
  expect(session.session.errorMessage ?? "").toMatch(/previous daemon exited unexpectedly/i)

  const history = await client.send("sessionHistory", { id: sessionId })
  expect(history.connection.mode).toBe("history")
  expect(history.turns).toHaveLength(1)

  const diagnostics = await client.send("sessionDiagnostics", {
    id: sessionId,
  })
  expect(
    diagnostics.events.some((event) => event.type === "session_reconciled_after_restart"),
  ).toBe(true)
  await expect(client.send("sessionConnect", { id: sessionId })).rejects.toThrow(/archived/i)
  await expect(client.send("sessionResolveToken", { token: "tok-restart-1" })).rejects.toThrow(
    /invalid session token/i,
  )
})

test("daemon promotes interrupted turn drafts into incomplete turn history on restart", async () => {
  await useTempHome()

  const sessionId = db.sessions.newId()
  const acpSessionId = `acp-draft-${randomUUID()}`
  db.sessions.put(sessionId, {
    acpSessionId,
    status: "active",
    stopReason: null,
    agent: "pi-acp",
    agentName: "node",
    cwd: process.cwd(),
    title: "New session",
    titleState: "placeholder",
    mcpServers: [],
    connectionMode: "live",
    supportsLoadSession: false,
    activeDaemonSession: true,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: "tok-draft-1",
    permissions: {
      owner: "acme",
      repo: "widgets",
      allowedPrNumbers: [42],
    },
    metadata: null,
    models: null,
    availableCommands: [],
  })
  db.sessionTurnDrafts.create({
    sessionId,
    turnId: "turn-draft-1",
    sequence: 1,
    promptRequestId: "prompt-draft-1",
    startedAt: "2026-04-14T00:00:00.000Z",
    updatedAt: "2026-04-14T00:00:00.050Z",
    messages: [
      buildPromptMessage(acpSessionId, "prompt-draft-1", "Continue the review."),
      {
        jsonrpc: "2.0",
        method: "session/update",
        params: {
          sessionId: acpSessionId,
          update: {
            sessionUpdate: "agent_message_chunk",
            content: { type: "text", text: "Partial response" },
          },
        },
      },
    ],
  })
  db.sessionDiagnostics.create({
    sessionId,
    events: [],
  })

  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const history = await client.send("sessionHistory", { id: sessionId })

  expect(history.turns).toHaveLength(1)
  expect(history.turns[0]).toMatchObject({
    turnId: "turn-draft-1",
    sequence: 1,
    promptRequestId: "prompt-draft-1",
    completedAt: null,
    completionKind: null,
    stopReason: null,
  })
  expect(
    db.sessionTurnDrafts.first({
      where: { sessionId },
    }) ?? null,
  ).toBeNull()
  expect(
    db.sessionTurns.first({
      where: { sessionId },
    }),
  ).toMatchObject({
    turnId: "turn-draft-1",
    completedAt: null,
    completionKind: null,
  })
})

test("multiple clients can observe the same live session stream independently", async () => {
  const daemon = await startServer()
  const clientA = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const clientB = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await clientA.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const clientAMessages: unknown[] = []
  const clientBMessages: unknown[] = []
  const unsubscribeA = await clientA.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      clientAMessages.push(payload.message)
    },
  )
  const unsubscribeB = await clientB.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      clientBMessages.push(payload.message)
    },
  )

  await clientA.send("sessionSend", {
    id: created.session.id,
    message: {
      jsonrpc: "2.0",
      id: 1,
      method: "session/prompt",
      params: {
        sessionId: created.session.acpSessionId,
        prompt: [{ type: "text", text: "Say hello in one sentence." }],
      },
    },
  })

  await waitFor(async () => clientAMessages.length > 0 && clientBMessages.length > 0)

  await Promise.resolve(unsubscribeA()).catch(() => {})
  await Promise.resolve(unsubscribeB()).catch(() => {})

  expect(clientAMessages.length).toBeGreaterThan(0)
  expect(clientBMessages.length).toBeGreaterThan(0)
})

test("daemon auto-shuts down idle loadable sessions with no connected clients", async () => {
  const daemon = await startServer({ idleSessionShutdownTimeoutMs: 60 })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)

  const session = await client.send("sessionGet", { id: created.session.id })
  expect(session.session.connectionMode).toBe("live")
  expect(session.session.activeDaemonSession).toBe(false)
  expect(getDiagnosticEventTypes(created.session.id)).toContain(
    "session_idle_shutdown_timer_started",
  )
  expect(getDiagnosticEventTypes(created.session.id)).toContain(
    "session_idle_shutdown_timer_expired",
  )

  const reconnected = await client.send("sessionConnect", {
    id: created.session.id,
  })
  expect(reconnected.session.activeDaemonSession).toBe(true)

  await client.send("sessionShutdown", { id: created.session.id })
})

test("sessionMessage subscribers cancel idle auto-shutdown before expiry", async () => {
  const idleSessionShutdownTimeoutMs = 80
  const daemon = await startServer({ idleSessionShutdownTimeoutMs })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    () => {},
  )
  await waitFor(async () =>
    getDiagnosticEventTypes(created.session.id).includes("session_idle_shutdown_timer_cancelled"),
  )
  await new Promise((resolve) => setTimeout(resolve, idleSessionShutdownTimeoutMs + 40))

  expect(db.sessions.get(created.session.id)?.activeDaemonSession).toBe(true)
  expect(getDiagnosticEventTypes(created.session.id)).not.toContain(
    "session_idle_shutdown_timer_expired",
  )

  await Promise.resolve(unsubscribe()).catch(() => {})
  await client.send("sessionShutdown", { id: created.session.id })
})

test("idle auto-shutdown waits for the last sessionMessage subscriber to disconnect", async () => {
  const idleSessionShutdownTimeoutMs = 70
  const daemon = await startServer({ idleSessionShutdownTimeoutMs })
  const clientA = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const clientB = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await clientA.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const unsubscribeA = await clientA.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    () => {},
  )
  const unsubscribeB = await clientB.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    () => {},
  )

  await Promise.resolve(unsubscribeA()).catch(() => {})
  await new Promise((resolve) => setTimeout(resolve, idleSessionShutdownTimeoutMs + 40))
  expect(db.sessions.get(created.session.id)?.activeDaemonSession).toBe(true)

  await Promise.resolve(unsubscribeB()).catch(() => {})
  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)

  expect(getDiagnosticEventTypes(created.session.id)).toContain(
    "session_idle_shutdown_timer_expired",
  )
})

test("busy loadable sessions do not time out until they become quiescent", async () => {
  const daemon = await startServer({ idleSessionShutdownTimeoutMs: 60 })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "wait:150"),
  })

  await new Promise((resolve) => setTimeout(resolve, 110))
  expect(db.sessions.get(created.session.id)?.activeDaemonSession).toBe(true)

  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)
  expect(getDiagnosticEventTypes(created.session.id)).toContain(
    "session_idle_shutdown_timer_expired",
  )
})

test("sessions waiting on permission responses do not time out until the permission resolves", async () => {
  const idleSessionShutdownTimeoutMs = 60
  const daemon = await startServer({ idleSessionShutdownTimeoutMs })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "permission:approve"),
  })

  await new Promise((resolve) => setTimeout(resolve, idleSessionShutdownTimeoutMs + 40))
  expect(db.sessions.get(created.session.id)?.activeDaemonSession).toBe(true)

  await client.send("sessionSend", {
    id: created.session.id,
    message: {
      jsonrpc: "2.0",
      id: "permission-prompt-1",
      result: {
        outcome: {
          outcome: "allow_once",
        },
      },
    },
  })

  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)
  expect(getDiagnosticEventTypes(created.session.id)).toContain(
    "session_idle_shutdown_timer_expired",
  )
})

test("sessions without session/load support never use idle auto-shutdown", async () => {
  const idleSessionShutdownTimeoutMs = 60
  const daemon = await startServer({ idleSessionShutdownTimeoutMs })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(chunkingAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await new Promise((resolve) => setTimeout(resolve, idleSessionShutdownTimeoutMs + 40))

  expect(db.sessions.get(created.session.id)?.activeDaemonSession).toBe(true)
  expect(getDiagnosticEventTypes(created.session.id)).not.toContain(
    "session_idle_shutdown_timer_started",
  )

  await client.send("sessionShutdown", { id: created.session.id })
})

test("manual session shutdown clears any pending idle auto-shutdown timer", async () => {
  const idleSessionShutdownTimeoutMs = 80
  const daemon = await startServer({ idleSessionShutdownTimeoutMs })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await new Promise((resolve) => setTimeout(resolve, 20))
  await client.send("sessionShutdown", { id: created.session.id })
  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)
  await new Promise((resolve) => setTimeout(resolve, idleSessionShutdownTimeoutMs + 40))

  expect(getDiagnosticEventTypes(created.session.id)).toContain(
    "session_idle_shutdown_timer_cancelled",
  )
  expect(getDiagnosticEventTypes(created.session.id)).not.toContain(
    "session_idle_shutdown_timer_expired",
  )
})

test("daemon shutdown clears pending idle auto-shutdown timers", async () => {
  const idleSessionShutdownTimeoutMs = 80
  const daemon = await startServer({ idleSessionShutdownTimeoutMs })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await new Promise((resolve) => setTimeout(resolve, 20))
  await daemon.close()
  await new Promise((resolve) => setTimeout(resolve, idleSessionShutdownTimeoutMs + 40))

  expect(getDiagnosticEventTypes(created.session.id)).toContain(
    "session_idle_shutdown_timer_cancelled",
  )
  expect(getDiagnosticEventTypes(created.session.id)).not.toContain(
    "session_idle_shutdown_timer_expired",
  )
})

test("agent process exit clears pending idle auto-shutdown timers", async () => {
  const daemon = await startServer({ idleSessionShutdownTimeoutMs: 80 })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "exit-after-turn:20"),
  })

  await waitFor(async () => db.sessions.get(created.session.id)?.activeDaemonSession === false)

  const diagnosticTypes = getDiagnosticEventTypes(created.session.id)
  expect(
    diagnosticTypes.filter((type) => type === "session_idle_shutdown_timer_started").length,
  ).toBeGreaterThanOrEqual(2)
  expect(diagnosticTypes).toContain("session_idle_shutdown_timer_cancelled")
  expect(diagnosticTypes).not.toContain("session_idle_shutdown_timer_expired")
})

test("daemon queues concurrent prompts per session and drains them in arrival order", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const promptStarts: string[] = []
  const promptErrors: string[] = []
  const promptStops: string[] = []
  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      const message = payload.message as {
        method?: string
        params?: { update?: { content?: { text?: string } } }
        error?: { message?: string }
        result?: { stopReason?: string }
      }

      if (message.method === "session/update") {
        const updateText = message.params?.update?.content?.text ?? ""
        if (updateText.startsWith("prompt_started:")) {
          promptStarts.push(updateText.slice("prompt_started:".length))
        }
      }

      if (message.error?.message) {
        promptErrors.push(message.error.message)
      }

      if (message.result?.stopReason) {
        promptStops.push(message.result.stopReason)
      }
    },
  )

  await Promise.all([
    client.send("sessionSend", {
      id: created.session.id,
      message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "wait:40"),
    }),
    client.send("sessionSend", {
      id: created.session.id,
      message: buildPromptMessage(created.session.acpSessionId, "prompt-2", "second"),
    }),
    client.send("sessionSend", {
      id: created.session.id,
      message: buildPromptMessage(created.session.acpSessionId, "prompt-3", "third"),
    }),
  ])

  await waitFor(async () => promptStops.length >= 3)
  await Promise.resolve(unsubscribe()).catch(() => {})

  expect(promptStarts).toEqual(["wait:40", "second", "third"])
  expect(promptErrors).toEqual([])
  expect(promptStops).toEqual(["end_turn", "end_turn", "end_turn"])
})

test("daemon cancel returns queued prompts, emits terminal errors for queued raw prompts, and prevents them from being sent", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const promptStarts: string[] = []
  const promptErrors: Array<{
    code?: number
    id?: string
    message?: string
  }> = []
  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      const message = payload.message as {
        id?: string
        method?: string
        params?: { update?: { content?: { text?: string } } }
        error?: { code?: number; message?: string }
      }

      if (message.method === "session/update") {
        const updateText = message.params?.update?.content?.text ?? ""
        if (updateText.startsWith("prompt_started:")) {
          promptStarts.push(updateText.slice("prompt_started:".length))
        }
      }

      if (message.error) {
        promptErrors.push({
          code: message.error.code,
          id: message.id,
          message: message.error.message,
        })
      }
    },
  )

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "hold:final-only"),
  })
  await waitFor(async () => promptStarts.includes("hold:final-only"))

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-2", "queued-second"),
  })
  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-3", "queued-third"),
  })

  const cancelled = await client.send("sessionCancel", {
    id: created.session.id,
  })
  await waitFor(async () => promptErrors.length >= 2)
  await Promise.resolve(unsubscribe()).catch(() => {})

  expect(cancelled).toEqual({
    id: created.session.id,
    activeTurnCancelled: true,
    abortedQueue: [
      {
        requestId: "prompt-2",
        prompt: [{ type: "text", text: "queued-second" }],
      },
      {
        requestId: "prompt-3",
        prompt: [{ type: "text", text: "queued-third" }],
      },
    ],
  })
  expect(promptErrors).toEqual([
    {
      code: -32800,
      id: "prompt-2",
      message: "Queued prompt aborted before dispatch by session cancellation.",
    },
    {
      code: -32800,
      id: "prompt-3",
      message: "Queued prompt aborted before dispatch by session cancellation.",
    },
  ])
  expect(promptStarts).toEqual(["hold:final-only"])
})

test("daemon steering ignores message chunks and dispatches on tool updates", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const events: string[] = []
  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      const message = payload.message as {
        id?: string
        method?: string
        params?: {
          update?: {
            content?: { text?: string }
            sessionUpdate?: string
            title?: string
          }
        }
        result?: { stopReason?: string }
      }

      if (message.method === "session/update") {
        const update = message.params?.update
        if (update?.sessionUpdate === "agent_message_chunk") {
          events.push(`chunk:${update.content?.text ?? ""}`)
        }
        if (update?.sessionUpdate === "tool_call" || update?.sessionUpdate === "tool_call_update") {
          events.push(`${update.sessionUpdate}:${update.title ?? ""}`)
        }
      } else if (message.result?.stopReason && message.id) {
        events.push(`result:${message.id}:${message.result.stopReason}`)
      }
    },
  )

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "hold:update-boundary"),
  })
  await waitFor(async () => events.includes("chunk:prompt_started:hold:update-boundary"))

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-2", "stale-queued"),
  })

  const steered = await client.send("sessionSteer", {
    id: created.session.id,
    prompt: "replacement",
  })
  await waitFor(async () => events.includes("result:prompt-1:cancelled"))
  await Promise.resolve(unsubscribe()).catch(() => {})

  expect(steered.abortedQueue).toEqual([
    {
      requestId: "prompt-2",
      prompt: [{ type: "text", text: "stale-queued" }],
    },
  ])
  expect(steered.response.stopReason).toBe("end_turn")
  expect(events.indexOf("chunk:cancel_notice:hold:update-boundary")).toBeGreaterThan(-1)
  expect(events.indexOf("tool_call_update:cancel_boundary:hold:update-boundary")).toBeGreaterThan(
    events.indexOf("chunk:cancel_notice:hold:update-boundary"),
  )
  expect(events.indexOf("chunk:prompt_started:replacement")).toBeGreaterThan(
    events.indexOf("tool_call_update:cancel_boundary:hold:update-boundary"),
  )
  expect(events.indexOf("chunk:prompt_started:replacement")).toBeLessThan(
    events.indexOf("result:prompt-1:cancelled"),
  )
})

test("daemon steering falls back to the cancelled prompt response when no tool boundary appears", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const events: string[] = []
  const unsubscribe = await client.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      const message = payload.message as {
        id?: string
        method?: string
        params?: {
          update?: {
            content?: { text?: string }
            sessionUpdate?: string
            title?: string
          }
        }
        result?: { stopReason?: string }
      }

      if (message.method === "session/update") {
        const update = message.params?.update
        if (update?.sessionUpdate === "agent_message_chunk") {
          events.push(`chunk:${update.content?.text ?? ""}`)
        }
        if (update?.sessionUpdate === "tool_call" || update?.sessionUpdate === "tool_call_update") {
          events.push(`${update.sessionUpdate}:${update.title ?? ""}`)
        }
      } else if (message.result?.stopReason && message.id) {
        events.push(`result:${message.id}:${message.result.stopReason}`)
      }
    },
  )

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-1", "hold:final-only"),
  })
  await waitFor(async () => events.includes("chunk:prompt_started:hold:final-only"))

  await client.send("sessionSend", {
    id: created.session.id,
    message: buildPromptMessage(created.session.acpSessionId, "prompt-2", "stale-queued"),
  })

  const steered = await client.send("sessionSteer", {
    id: created.session.id,
    prompt: "replacement",
  })
  await waitFor(async () => events.includes("result:prompt-1:cancelled"))
  await Promise.resolve(unsubscribe()).catch(() => {})

  expect(steered.abortedQueue).toEqual([
    {
      requestId: "prompt-2",
      prompt: [{ type: "text", text: "stale-queued" }],
    },
  ])
  expect(steered.response.stopReason).toBe("end_turn")
  expect(events.indexOf("chunk:cancel_notice:hold:final-only")).toBeGreaterThan(-1)
  expect(events.some((event) => event.startsWith("tool_call:cancel_boundary:"))).toBe(false)
  expect(events.some((event) => event.startsWith("tool_call_update:cancel_boundary:"))).toBe(false)
  expect(events.indexOf("chunk:prompt_started:replacement")).toBeGreaterThan(
    events.indexOf("result:prompt-1:cancelled"),
  )
})

test("session worktree opt-in maps cwd into a real worktree subdirectory", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const repoDir = await createRepoFixture({ includeSrc: true })
  const requestedCwd = join(repoDir, "src")

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: requestedCwd,
    worktree: { enabled: true },
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const fetchedWorktree = await client.send("sessionWorktree", {
    id: created.session.id,
  })
  const worktree = fetchedWorktree.worktree
  expect(worktree).toBeTruthy()
  expect(fetchedWorktree.id).toBe(created.session.id)
  expect(worktree?.requestedCwd.endsWith("/src")).toBe(true)
  expect(worktree?.effectiveCwd).toMatch(/\/src$/)
  expect(worktree?.worktreeDir).not.toBe(repoDir)
  expect(existsSync(worktree!.worktreeDir)).toBe(true)
  expect(existsSync(worktree!.effectiveCwd)).toBe(true)
  await client.send("sessionShutdown", { id: created.session.id })
})

test("sessionChanges reads tracked and untracked diff content from the session workspace root", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const repoDir = await createRepoFixture({ includeSrc: true })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: join(repoDir, "src"),
    worktree: { enabled: true },
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const fetchedWorktree = await client.send("sessionWorktree", {
    id: created.session.id,
  })
  expect(fetchedWorktree.worktree).toBeTruthy()

  await writeFile(
    join(fetchedWorktree.worktree!.worktreeDir, "package.json"),
    JSON.stringify({ name: "repo", private: false }, null, 2) + "\n",
    "utf-8",
  )
  await writeFile(join(fetchedWorktree.worktree!.worktreeDir, "README.md"), "# Session changes\n")

  const changes = await client.send("sessionChanges", {
    id: created.session.id,
  })

  expect(changes.workspaceRoot).toBe(fetchedWorktree.worktree!.worktreeDir)
  expect(changes.hasChanges).toBe(true)
  expect(changes.diff).toContain("diff --git a/package.json b/package.json")
  expect(changes.diff).toContain("diff --git a/README.md b/README.md")

  await client.send("sessionShutdown", { id: created.session.id })
})

test("session worktree launch branches from the selected base branch", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const repoDir = await createRepoFixture()
  const defaultBranch = readGitOutput(repoDir, ["branch", "--show-current"])

  await writeFile(join(repoDir, "branch-source.txt"), "feature-base\n", "utf-8")
  runGit(repoDir, ["checkout", "-b", "feature-base"])
  runGit(repoDir, ["add", "branch-source.txt"])
  runGit(repoDir, ["commit", "-m", "feature-base"])
  const featureHead = readGitOutput(repoDir, ["rev-parse", "HEAD"])
  runGit(repoDir, ["checkout", defaultBranch])

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: repoDir,
    worktree: { enabled: true, baseBranchName: "feature-base" },
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const fetchedWorktree = await client.send("sessionWorktree", {
    id: created.session.id,
  })
  expect(fetchedWorktree.worktree).toBeTruthy()
  expect(
    readGitOutput(fetchedWorktree.worktree!.worktreeDir, ["rev-parse", "--abbrev-ref", "HEAD"]),
  ).toBe(fetchedWorktree.worktree!.branchName)
  expect(readGitOutput(fetchedWorktree.worktree!.worktreeDir, ["rev-parse", "HEAD"])).toBe(
    featureHead,
  )

  await client.send("sessionShutdown", { id: created.session.id })
})

test("sessionComposerSuggestions scopes `@` lookups to the session cwd and skips ignored directories", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const repoDir = await createRepoFixture()
  await mkdir(join(repoDir, "src", "nested"), { recursive: true })
  await mkdir(join(repoDir, "node_modules", "pkg"), { recursive: true })
  await mkdir(join(repoDir, "dist"), { recursive: true })
  await mkdir(join(repoDir, ".git", "objects"), { recursive: true })
  await writeFile(
    join(repoDir, "src", "nested", "match.ts"),
    "export const match = true\n",
    "utf-8",
  )
  await writeFile(join(repoDir, "node_modules", "pkg", "ignore.ts"), "ignored\n", "utf-8")
  await writeFile(join(repoDir, "dist", "ignore.ts"), "ignored\n", "utf-8")

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(fastFixtureAgentPath),
    cwd: repoDir,
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const emptyQuery = await client.send("sessionComposerSuggestions", {
    id: created.session.id,
    trigger: "at",
    query: "",
  })
  const filtered = await client.send("sessionComposerSuggestions", {
    id: created.session.id,
    trigger: "at",
    query: "match",
  })

  const emptyQueryLabels = emptyQuery.suggestions.flatMap((suggestion) =>
    "label" in suggestion ? [suggestion.label] : [],
  )

  expect(emptyQueryLabels).toContain("src")
  expect(emptyQueryLabels).not.toContain(".git")
  expect(emptyQueryLabels).not.toContain("node_modules")
  expect(filtered.suggestions).toEqual([
    {
      type: "file",
      path: join(repoDir, "src", "nested", "match.ts"),
      uri: pathToFileURL(join(repoDir, "src", "nested", "match.ts")).toString(),
      label: "match.ts",
      detail: "./src/nested/match.ts",
    },
  ])

  await client.send("sessionShutdown", { id: created.session.id })
})

test("sessionComposerSuggestions prefers local `$` skills over global duplicates", async () => {
  await useTempHome()

  const repoDir = await createRepoFixture()
  const localSkillDir = join(repoDir, ".agents", "skills", "alpha")
  const globalSkillDir = join(process.env.HOME!, ".agents", "skills")
  await mkdir(localSkillDir, { recursive: true })
  await mkdir(join(globalSkillDir, "alpha"), { recursive: true })
  await mkdir(join(globalSkillDir, "beta"), { recursive: true })
  await writeFile(join(localSkillDir, "SKILL.md"), "# alpha\n", "utf-8")
  await writeFile(join(globalSkillDir, "alpha", "SKILL.md"), "# alpha global\n", "utf-8")
  await writeFile(join(globalSkillDir, "beta", "SKILL.md"), "# beta global\n", "utf-8")

  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(fastFixtureAgentPath),
    cwd: repoDir,
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const suggestions = await client.send("sessionComposerSuggestions", {
    id: created.session.id,
    trigger: "dollar",
    query: "",
  })

  expect(suggestions.suggestions).toEqual([
    {
      type: "skill",
      path: join(localSkillDir, "SKILL.md"),
      uri: pathToFileURL(join(localSkillDir, "SKILL.md")).toString(),
      label: "alpha",
      detail: "./.agents/skills/alpha/SKILL.md",
      source: "local",
    },
    {
      type: "skill",
      path: join(globalSkillDir, "beta", "SKILL.md"),
      uri: pathToFileURL(join(globalSkillDir, "beta", "SKILL.md")).toString(),
      label: "beta",
      detail: "~/.agents/skills/beta/SKILL.md",
      source: "global",
    },
  ])

  await client.send("sessionShutdown", { id: created.session.id })
})

test("sessionComposerSuggestions reads `/` commands from the latest ACP history update", async () => {
  await useTempHome()

  const sessionId = db.sessions.newId()
  const acpSessionId = `acp-history-${randomUUID()}`
  db.sessions.put(sessionId, {
    acpSessionId,
    status: "done",
    stopReason: null,
    agent: "pi-acp",
    agentName: "node",
    cwd: process.cwd(),
    title: "New session",
    titleState: "placeholder",
    mcpServers: [],
    connectionMode: "history",
    supportsLoadSession: false,
    activeDaemonSession: false,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: null,
    permissions: null,
    metadata: null,
    models: null,
    availableCommands: [
      {
        name: "plan",
        description: "Create or revise the plan",
        input: { hint: "What should change?" },
      },
      {
        name: "summarize",
        description: "Summarize the current progress",
      },
    ],
  })

  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const suggestions = await client.send("sessionComposerSuggestions", {
    id: sessionId,
    trigger: "slash",
    query: "plan",
  })

  expect(suggestions.suggestions).toEqual([
    {
      type: "slash_command",
      name: "plan",
      description: "Create or revise the plan",
      inputHint: "What should change?",
    },
  ])
})

test("sessionDraftSuggestions reads launch-dialog `@` and `$` suggestions without a session id", async () => {
  await useTempHome()

  const repoDir = await createRepoFixture()
  const localSkillDir = join(repoDir, ".agents", "skills", "checks")
  await mkdir(localSkillDir, { recursive: true })
  await mkdir(join(repoDir, "src"), { recursive: true })
  await writeFile(join(localSkillDir, "SKILL.md"), "# checks\n", "utf-8")
  await writeFile(join(repoDir, "src", "launch.ts"), "export const launch = true\n", "utf-8")

  const daemon = await startServer({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const atSuggestions = await client.send("sessionDraftSuggestions", {
    cwd: repoDir,
    trigger: "at",
    query: "launch",
  })
  const dollarSuggestions = await client.send("sessionDraftSuggestions", {
    cwd: repoDir,
    trigger: "dollar",
    query: "check",
  })

  expect(atSuggestions.suggestions).toEqual([
    {
      type: "file",
      path: join(repoDir, "src", "launch.ts"),
      uri: pathToFileURL(join(repoDir, "src", "launch.ts")).toString(),
      label: "launch.ts",
      detail: "./src/launch.ts",
    },
  ])
  expect(dollarSuggestions.suggestions).toEqual([
    {
      type: "skill",
      path: join(localSkillDir, "SKILL.md"),
      uri: pathToFileURL(join(localSkillDir, "SKILL.md")).toString(),
      label: "checks",
      detail: "./.agents/skills/checks/SKILL.md",
      source: "local",
    },
  ])
})

test("sessionLaunchPreview loads adapter capabilities and repository branches for the launch dialog", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const repoDir = await createRepoFixture()
  const currentBranch = readGitOutput(repoDir, ["branch", "--show-current"])

  runGit(repoDir, ["branch", "feature-a"])

  const preview = await client.send("sessionLaunchPreview", {
    agent: createWrappedNodeAgent(launchPreviewAgentPath),
    cwd: repoDir,
  })

  expect(preview.repoRoot).toBe(await realpath(repoDir))
  expect(preview.branches).toContainEqual({
    name: currentBranch,
    current: true,
  })
  expect(preview.branches).toContainEqual({
    name: "feature-a",
    current: false,
  })
  expect(preview.models?.currentModelId).toBe("gpt-5.4")
  expect(preview.configOptions).toContainEqual(
    expect.objectContaining({
      id: "thinking",
      category: "thought_level",
      currentValue: "medium",
    }),
  )
  expect(preview.slashCommands).toContainEqual({
    type: "slash_command",
    name: "plan",
    description: "Create or revise the plan",
    inputHint: "What should change?",
  })
})

test("sessionCreate applies initial model and thinking configuration before the first prompt", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const repoDir = await createRepoFixture()

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(launchPreviewAgentPath),
    cwd: repoDir,
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    initialModelId: "gpt-5.4-mini",
    initialConfigOptions: [
      {
        configId: "thinking",
        value: "high",
      },
    ],
    initialPrompt: "Start the session.",
    oneShot: true,
  })

  expect(created.session.models?.currentModelId).toBe("gpt-5.4-mini")

  const history = await client.send("sessionHistory", {
    id: created.session.id,
  })
  expect(
    history.turns.some((turn) =>
      turn.messages.some((message) => {
        const update = matchAcpRequest<{
          update?: {
            sessionUpdate?: string
            content?: { type?: string; text?: string }
          }
        }>(message, "session/update")?.update
        return (
          update?.sessionUpdate === "agent_message_chunk" &&
          update.content?.type === "text" &&
          update.content.text === "model=gpt-5.4-mini;thinking=high"
        )
      }),
    ),
  ).toBe(true)
})

test("sync-enabled worktree launch mounts after bootstrap and mirrors bootstrap output", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const repoDir = await createRepoFixture()
  const binDir = await createFakePackageManager("bun", {
    exitCode: 0,
    outputFile: ".bootstrap-marker",
  })

  process.env.PATH = `${binDir}:${originalPath ?? ""}`
  await writeLocalRootConfig(repoDir, {
    worktrees: {
      bootstrap: {
        packageManager: "bun",
        seedEnabled: false,
      },
    },
  })

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: repoDir,
    worktree: { enabled: true, sync: { enabled: true } },
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const worktree = (await client.send("sessionWorktree", { id: created.session.id })).worktree
  expect(worktree?.sync?.status).toBe("mounted")
  expect(await readFile(join(worktree!.worktreeDir, ".bootstrap-marker"), "utf-8")).toBe(
    "install\n",
  )
  expect(await readFile(join(repoDir, ".bootstrap-marker"), "utf-8")).toBe("install\n")

  await client.send("sessionShutdown", { id: created.session.id })
})

test("session creation fails when fresh worktree bootstrap install exits unsuccessfully", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const repoDir = await createRepoFixture()
  const binDir = await createFakePackageManager("bun", {
    exitCode: 1,
    outputFile: ".bootstrap-marker",
  })

  process.env.PATH = `${binDir}:${originalPath ?? ""}`
  await writeLocalRootConfig(repoDir, {
    worktrees: {
      bootstrap: {
        packageManager: "bun",
        seedEnabled: false,
      },
    },
  })

  const sessionCountBefore = db.sessions.findMany().length
  await expect(
    client.send("sessionCreate", {
      agent: createWrappedNodeAgent(exampleAgentPath),
      cwd: repoDir,
      worktree: { enabled: true },
      mcpServers: [],
      systemPrompt: "Keep responses short.",
    }),
  ).rejects.toThrow(/Internal server error/i)
  expect(db.sessions.findMany()).toHaveLength(sessionCountBefore)
})

async function startServer(
  options: {
    useExistingHome?: boolean
    idleSessionShutdownTimeoutMs?: number
  } = {},
): Promise<DaemonServer> {
  if (!options.useExistingHome) {
    await useTempHome()
  }

  const daemon = await startDaemonServer(
    {
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
        create: async () => ({
          number: 1,
          url: "https://github.com/example/repo/pull/1",
        }),
        reply: async () => ({ success: true }),
      },
    },
    {
      port: 0,
      idleSessionShutdownTimeoutMs: options.idleSessionShutdownTimeoutMs,
    },
  )

  cleanup.push(async () => {
    await daemon.close().catch(() => {})
  })

  return daemon
}

async function useTempHome(): Promise<void> {
  sharedHomeDir ??= await mkdtemp(join(tmpdir(), "goddard-daemon-home-"))
  process.env.HOME = sharedHomeDir
  resetDb()
}

async function createRepoFixture(options: { includeSrc?: boolean } = {}): Promise<string> {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-daemon-repo-"))
  cleanup.push(async () => {
    await rm(repoDir, { recursive: true, force: true })
  })

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "repo", private: true }, null, 2),
    "utf-8",
  )

  if (options.includeSrc) {
    await mkdir(join(repoDir, "src"), { recursive: true })
    await writeFile(join(repoDir, "src", "index.ts"), "export const ready = true\n", "utf-8")
  }

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "."])
  runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })

  expect(result.status).toBe(0)
}

function readGitOutput(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })

  expect(result.status).toBe(0)
  return result.stdout.trim()
}

async function writeLocalRootConfig(repoDir: string, config: Record<string, unknown>) {
  const configPath = getLocalConfigPath(repoDir)
  await mkdir(dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify({ $schema: rootConfigSchemaUrl, ...config }, null, 2)}\n`,
    "utf-8",
  )
}

async function writeGlobalRootConfig(config: Record<string, unknown>) {
  const configPath = getGlobalConfigPath()
  await mkdir(dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify({ $schema: rootConfigSchemaUrl, ...config }, null, 2)}\n`,
    "utf-8",
  )
}

async function createFakePackageManager(
  name: string,
  options: {
    exitCode: number
    outputFile: string
  },
) {
  const binDir = await mkdtemp(join(tmpdir(), `goddard-${name}-bin-`))
  cleanup.push(async () => {
    await rm(binDir, { recursive: true, force: true })
  })

  const scriptPath = join(binDir, name)
  await writeFile(
    scriptPath,
    [
      "#!/bin/sh",
      `printf '%s\\n' "$@" > "${options.outputFile}"`,
      `exit ${options.exitCode}`,
      "",
    ].join("\n"),
    "utf-8",
  )
  await chmod(scriptPath, 0o755)

  return binDir
}

function buildPromptMessage(sessionId: string, id: string, text: string) {
  return {
    jsonrpc: "2.0" as const,
    id,
    method: "session/prompt",
    params: {
      sessionId,
      prompt: [{ type: "text", text }],
    },
  }
}

function getDiagnosticEventTypes(sessionId: ReturnType<typeof db.sessions.newId>) {
  return (
    db.sessionDiagnostics.first({
      where: { sessionId },
    })?.events ?? []
  ).map((event) => event.type)
}

async function waitFor(check: () => Promise<boolean>, timeoutMs = 5_000) {
  const start = Date.now()
  while (Date.now() - start < timeoutMs) {
    if (await check()) {
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 25))
  }

  throw new Error("Timed out waiting for condition")
}
