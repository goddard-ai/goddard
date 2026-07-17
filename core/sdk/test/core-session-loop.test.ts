import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { createDaemonRuntime, startDaemonServer } from "@goddard-ai/daemon/ipc"
import type { SessionId, SessionMessageEvent, SessionTurnMessage } from "@goddard-ai/session/schema"
import * as acp from "acp-client/protocol"
import { afterEach, expect, test } from "bun:test"

import { createWrappedNodeAgent } from "../../daemon/test/acp-fixture.ts"
import { GoddardSdk } from "../src/node/index.ts"

const queueAgentPath = fileURLToPath(
  new URL("../../daemon/test/fixtures/queue-agent.mjs", import.meta.url),
)
const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME

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

test("public SDK session loop recovers durable history after observation is interrupted", async () => {
  const homeDir = await makeTemporaryHome()
  process.env.HOME = homeDir

  const runtime = await createDaemonRuntime({ port: 0 })
  const daemon = await startDaemonServer(runtime)
  cleanup.push(async () => {
    await daemon.close().catch(() => {})
    await runtime.close().catch(() => {})
  })

  const sdk = new GoddardSdk({ daemonUrl: daemon.daemonUrl })
  const { session } = await sdk.session.create({
    agent: createWrappedNodeAgent(queueAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })
  const firstObserver = await observeSessionMessages(sdk, session.id)

  await sdk.session.prompt({
    id: session.id,
    acpId: session.acpSessionId,
    prompt: "permission:approve",
  })

  await waitFor(() => findPermissionRequest(firstObserver.messages) !== null)
  const permissionRequest = findPermissionRequest(firstObserver.messages)
  expect(permissionRequest).toMatchObject({
    method: acp.CLIENT_METHODS.session_request_permission,
    params: {
      toolCall: {
        kind: "other",
        status: "in_progress",
        title: "permission:permission:approve",
      },
    },
  })

  await firstObserver.stop()
  expect(
    permissionRequest && "id" in permissionRequest ? permissionRequest.id : null,
  ).not.toBeNull()

  const requestId = permissionRequest && "id" in permissionRequest ? permissionRequest.id : null
  if (requestId === null || requestId === undefined) {
    throw new Error("Fixture permission request did not expose an id")
  }

  await sdk.session.respondPermission({
    id: session.id,
    requestId,
    outcome: {
      outcome: "selected",
      optionId: `allow-${String(requestId)}`,
    },
  })

  await waitFor(async () => {
    const history = await sdk.session.history({ id: session.id })
    return history.turns.length === 1 && history.turns[0]?.completedAt !== null
  })

  const recoveredHistory = await sdk.session.history({ id: session.id })
  expect(recoveredHistory.turns).toHaveLength(1)
  expectTurnHasUniqueSequenceCoverage(recoveredHistory.turns[0]?.messages ?? [])

  const recoveredMessages = recoveredHistory.turns[0]?.messages.map(({ message }) => message) ?? []
  expect(recoveredMessages).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        method: acp.AGENT_METHODS.session_prompt,
      }),
      expect.objectContaining({
        method: acp.CLIENT_METHODS.session_request_permission,
        id: requestId,
      }),
      expect.objectContaining({
        id: requestId,
        result: {
          outcome: {
            outcome: "selected",
            optionId: `allow-${String(requestId)}`,
          },
        },
      }),
      expect.objectContaining({
        result: { stopReason: "end_turn" },
      }),
    ]),
  )
  expect(readAgentText(recoveredMessages).join("")).toContain("prompt_started:permission:approve")
  expect(readAgentText(recoveredMessages).join("")).toContain(
    "permission_resolved:permission:approve",
  )

  const resumedObserver = await observeSessionMessages(sdk, session.id)
  await sdk.session.prompt({
    id: session.id,
    acpId: session.acpSessionId,
    prompt: "follow-up",
  })
  await waitFor(() =>
    resumedObserver.messages.some((message) => {
      const payload = readAcpMessage(message)
      return (
        "result" in payload &&
        (payload.result as acp.PromptResponse | undefined)?.stopReason === "end_turn"
      )
    }),
  )
  await resumedObserver.stop()

  const finalHistory = await sdk.session.history({ id: session.id })
  expect(finalHistory.turns).toHaveLength(2)
  for (const turn of finalHistory.turns) {
    expect(turn.completedAt).not.toBeNull()
    expectTurnHasUniqueSequenceCoverage(turn.messages)
  }
  const finalAgentText = finalHistory.turns
    .flatMap((turn) => readAgentText(turn.messages.map(({ message }) => message)))
    .join("")
  expect(finalAgentText).toContain("prompt_started:permission:approve")
  expect(finalAgentText).toContain("permission_resolved:permission:approve")
  expect(finalAgentText).toContain("prompt_finished:permission:approve")
  expect(finalAgentText).toContain("prompt_started:follow-up")
  expect(finalAgentText).toContain("prompt_finished:follow-up")
})

async function observeSessionMessages(sdk: GoddardSdk, id: SessionId) {
  const controller = new AbortController()
  const messages: SessionMessageEvent[] = []
  const stream = await sdk.events.stream(
    {
      names: ["session.message"],
      where: [{ path: "id", equals: id }],
    },
    { signal: controller.signal },
  )
  const done = (async () => {
    for await (const event of stream) {
      messages.push(event.payload.message)
    }
  })()

  return {
    messages,
    async stop() {
      controller.abort()
      await done.catch(() => {})
    },
  }
}

function readAcpMessage(message: SessionMessageEvent) {
  return "message" in message ? message.message : message
}

function findPermissionRequest(messages: readonly SessionMessageEvent[]) {
  return (
    messages
      .map(readAcpMessage)
      .find(
        (message) =>
          "method" in message && message.method === acp.CLIENT_METHODS.session_request_permission,
      ) ?? null
  )
}

function readAgentText(messages: readonly acp.AnyMessage[]) {
  return messages.flatMap((message) => {
    if (!("method" in message) || message.method !== acp.CLIENT_METHODS.session_update) {
      return []
    }

    const { update } = message.params as acp.SessionNotification
    if (update.sessionUpdate !== "agent_message_chunk" || update.content.type !== "text") {
      return []
    }

    return [update.content.text]
  })
}

function expectTurnHasUniqueSequenceCoverage(messages: readonly SessionTurnMessage[]) {
  const coveredSequences = messages.flatMap(({ sequence, sequenceStart }) =>
    Array.from({ length: sequence - sequenceStart + 1 }, (_, index) => sequenceStart + index),
  )

  expect(new Set(coveredSequences).size).toBe(coveredSequences.length)
  expect(coveredSequences.toSorted((left, right) => left - right)).toEqual(
    Array.from({ length: Math.max(...coveredSequences) + 1 }, (_, sequence) => sequence),
  )
}

async function makeTemporaryHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-sdk-core-loop-"))
  cleanup.unshift(async () => {
    await rm(homeDir, { recursive: true, force: true })
  })
  return homeDir
}

async function waitFor(check: () => boolean | Promise<boolean>, timeoutMs = 5_000) {
  const startedAt = Date.now()
  while (Date.now() - startedAt < timeoutMs) {
    if (await check()) {
      return
    }
    await Bun.sleep(25)
  }

  throw new Error("Timed out waiting for core session loop state")
}
