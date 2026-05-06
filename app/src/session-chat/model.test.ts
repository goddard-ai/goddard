import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse, SessionHistoryTurn } from "@goddard-ai/sdk"
import { expect, test } from "bun:test"

import { SessionChat } from "./model.ts"

function createSession(overrides: Partial<DaemonSession> = {}) {
  return {
    id: "ses_session-1" as DaemonSession["id"],
    acpSessionId: "acp-session-1",
    status: "active",
    stopReason: null,
    agent: "pi-acp",
    agentName: "pi",
    cwd: "/repo-a",
    mcpServers: [],
    connectionMode: "live",
    supportsLoadSession: false,
    activeDaemonSession: true,
    token: null,
    permissions: null,
    title: "New session",
    titleState: "placeholder",
    repository: null,
    prNumber: null,
    metadata: null,
    createdAt: 1_743_968_000_000,
    updatedAt: 1_743_968_300_000,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    models: null,
    availableCommands: [],
    ...overrides,
  } satisfies DaemonSession
}

function createTurn(overrides: Partial<SessionHistoryTurn> = {}) {
  return {
    turnId: "turn-1",
    sequence: 1,
    promptRequestId: "prompt-1",
    startedAt: "2026-04-14T00:00:00.000Z",
    completedAt: "2026-04-14T00:00:01.000Z",
    completionKind: "result",
    stopReason: "end_turn",
    inboxScope: null,
    inboxHeadline: null,
    messages: [],
    ...overrides,
  } satisfies SessionHistoryTurn
}

function createHistory(turns: SessionHistoryTurn[]): GetSessionHistoryResponse {
  return {
    id: "ses_session-1" as DaemonSession["id"],
    acpSessionId: "acp-session-1",
    connection: {
      activeDaemonSession: true,
      mode: "live",
      reconnectable: true,
    },
    turns,
    nextCursor: null,
    hasMore: false,
  }
}

function createChat(input: { history?: GetSessionHistoryResponse; session?: DaemonSession }) {
  return new SessionChat({
    history: input.history ?? createHistory([]),
    session: input.session ?? createSession(),
  })
}

function promptMessage(id = "prompt-1") {
  return {
    jsonrpc: "2.0",
    id,
    method: acp.AGENT_METHODS.session_prompt,
    params: {
      sessionId: "acp-session-1",
      prompt: [{ type: "text", text: "Review the diff." }],
    },
  } satisfies acp.AnyMessage
}

function agentChunk(text: string) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: "acp-session-1",
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text },
      },
    },
  } satisfies acp.AnyMessage
}

function promptResult(id = "prompt-1") {
  return {
    jsonrpc: "2.0",
    id,
    result: {
      stopReason: "end_turn",
    },
  } satisfies acp.AnyMessage
}

test("SessionChat normalizes history turns into deterministic order and statuses", () => {
  const chat = createChat({
    session: createSession({ status: "done", activeDaemonSession: false }),
    history: createHistory([
      createTurn({ turnId: "turn-2", sequence: 2, completedAt: null, completionKind: null }),
      createTurn({ turnId: "turn-1", sequence: 1 }),
    ]),
  })

  expect(chat.turns.map((turn) => [turn.turnId, turn.status])).toEqual([
    ["turn-1", "completed"],
    ["turn-2", "running"],
  ])
  expect(chat.summary).toMatchObject({
    activeTurnId: "turn-2",
    status: "running",
  })
})

test("SessionChat creates one live turn and ignores repeated messages", () => {
  const chat = createChat({})

  chat.applyMessage(promptMessage("prompt-live"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessage(promptMessage("prompt-live"), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })

  expect(chat.turns).toHaveLength(1)
  expect(chat.turns[0]).toMatchObject({
    promptRequestId: "prompt-live",
    source: "live",
    status: "running",
  })
  expect(chat.turns[0].messages).toHaveLength(1)
  expect(chat.summary.activeTurnId).toBe("live:prompt-live")
})

test("SessionChat merges live updates into an active history turn", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessage(agentChunk("Working"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessage(agentChunk("Working"), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })
  chat.applyMessage(promptResult(), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })

  expect(chat.turns).toHaveLength(1)
  expect(chat.turns[0]).toMatchObject({
    source: "merged",
    completedAt: "2026-04-14T00:00:04.000Z",
    status: "completed",
  })
  expect(chat.turns[0].messages).toHaveLength(3)
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "sessionUpdate",
    "turnCompletion",
  ])
})

test("SessionChat keeps prompt and terminal messages deterministic when updates arrive out of order", () => {
  const chat = createChat({})

  chat.applyMessage(promptResult("prompt-late"), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })
  chat.applyMessage(promptMessage("prompt-late"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })

  expect(
    chat.turns[0].messages.map((message) => ("method" in message ? message.method : "result")),
  ).toEqual([acp.AGENT_METHODS.session_prompt, "result"])
  expect(chat.turns[0].status).toBe("completed")
})

test("SessionChat returns to ready status after a live turn completes", () => {
  const chat = createChat({})

  chat.applyMessage(promptMessage("prompt-ready"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  expect(chat.summary.status).toBe("running")

  chat.applyMessage(promptResult("prompt-ready"), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })
  expect(chat.summary.status).toBe("idle")
})

test("SessionChat treats an active session without a running turn as ready", () => {
  const chat = createChat({})

  expect(chat.summary.status).toBe("idle")
})

test("SessionChat exposes pending permission and plan events", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })
  const permissionRequest = {
    jsonrpc: "2.0",
    id: "permission-1",
    method: acp.CLIENT_METHODS.session_request_permission,
    params: {
      sessionId: "acp-session-1",
      options: [{ optionId: "allow", name: "Allow", kind: "allow_once" }],
      toolCall: {
        toolCallId: "tool-1",
        title: "Write file",
        kind: "edit",
        status: "pending",
      },
    },
  } satisfies acp.AnyMessage
  const planUpdate = {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: "acp-session-1",
      update: {
        sessionUpdate: "plan",
        entries: [{ content: "Inspect state", priority: "high", status: "in_progress" }],
      },
    },
  } satisfies acp.AnyMessage

  chat.applyMessage(permissionRequest)
  chat.applyMessage(planUpdate)

  expect(chat.summary.status).toBe("blocked")
  expect(chat.summary.pendingPermissionRequest?.requestId).toBe("permission-1")
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "permissionRequest",
    "planUpdate",
  ])
})

test("SessionChat preserves live messages that are not in refreshed history yet", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessage(agentChunk("Still working"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.syncLoadedData({
    session: createSession({ title: "Updated title" }),
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  expect(chat.session.title).toBe("Updated title")
  expect(chat.turns).toHaveLength(1)
  expect(chat.turns[0].messages).toHaveLength(2)
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual(["prompt", "sessionUpdate"])
})
