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
    completedHidden: false,
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
    contextUsage: null,
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

function createHistory(
  turns: SessionHistoryTurn[],
  overrides: Partial<GetSessionHistoryResponse> = {},
): GetSessionHistoryResponse {
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
    ...overrides,
  }
}

function createChat(input: { history?: GetSessionHistoryResponse; session?: DaemonSession }) {
  return new SessionChat({
    history: input.history ?? createHistory([]),
    session: input.session ?? createSession(),
  })
}

function wait(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
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

function permissionRequestMessage() {
  return {
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
}

function permissionResponseMessage() {
  return {
    jsonrpc: "2.0",
    id: "permission-1",
    result: {
      outcome: {
        outcome: "selected",
        optionId: "allow",
      },
    },
  } satisfies acp.AnyMessage
}

function usageUpdate(size: number, used: number) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: "acp-session-1",
      update: {
        size,
        used,
        sessionUpdate: "usage_update",
      },
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

test("SessionChat prepends older history pages and advances pagination", () => {
  const chat = createChat({
    history: createHistory(
      [
        createTurn({ turnId: "turn-2", sequence: 2, promptRequestId: "prompt-2" }),
        createTurn({ turnId: "turn-3", sequence: 3, promptRequestId: "prompt-3" }),
      ],
      {
        hasMore: true,
        nextCursor: "cursor-older",
      },
    ),
  })

  chat.prependOlderHistory(
    createHistory(
      [
        createTurn({ turnId: "turn-0", sequence: 0, promptRequestId: "prompt-0" }),
        createTurn({ turnId: "turn-1", sequence: 1, promptRequestId: "prompt-1" }),
      ],
      {
        hasMore: false,
        nextCursor: null,
      },
    ),
  )

  expect(chat.hasMore).toBe(false)
  expect(chat.nextCursor).toBeNull()
  expect(chat.turns.map((turn) => turn.turnId)).toEqual(["turn-0", "turn-1", "turn-2", "turn-3"])
})

test("SessionChat skips duplicate turns when older history overlaps the loaded page", () => {
  const chat = createChat({
    history: createHistory(
      [
        createTurn({
          turnId: "turn-2",
          sequence: 2,
          promptRequestId: "prompt-2",
          messages: [promptMessage("prompt-2")],
        }),
      ],
      {
        hasMore: true,
        nextCursor: "cursor-older",
      },
    ),
  })

  chat.prependOlderHistory(
    createHistory(
      [
        createTurn({ turnId: "turn-1", sequence: 1, promptRequestId: "prompt-1" }),
        createTurn({
          turnId: "turn-2",
          sequence: 2,
          promptRequestId: "prompt-2",
          messages: [promptMessage("prompt-2")],
        }),
      ],
      {
        hasMore: false,
        nextCursor: null,
      },
    ),
  )

  expect(chat.turns.map((turn) => turn.turnId)).toEqual(["turn-1", "turn-2"])
  expect(chat.turns[1].messages).toHaveLength(1)
})

test("SessionChat preserves loaded older history across refreshed latest history", () => {
  const chat = createChat({
    history: createHistory(
      [createTurn({ turnId: "turn-2", sequence: 2, promptRequestId: "prompt-2" })],
      {
        hasMore: true,
        nextCursor: "cursor-older",
      },
    ),
  })

  chat.prependOlderHistory(
    createHistory([createTurn({ turnId: "turn-1", sequence: 1, promptRequestId: "prompt-1" })], {
      hasMore: false,
      nextCursor: null,
    }),
  )
  chat.syncLoadedData({
    session: createSession({ title: "Updated title" }),
    history: createHistory(
      [createTurn({ turnId: "turn-2", sequence: 2, promptRequestId: "prompt-2" })],
      {
        hasMore: true,
        nextCursor: "cursor-older",
      },
    ),
  })

  expect(chat.session.title).toBe("Updated title")
  expect(chat.turns.map((turn) => turn.turnId)).toEqual(["turn-1", "turn-2"])
  expect(chat.hasMore).toBe(false)
  expect(chat.nextCursor).toBeNull()
})

test("SessionChat creates one live turn and ignores repeated messages", () => {
  const chat = createChat({})

  chat.applyMessageNow(promptMessage("prompt-live"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(promptMessage("prompt-live"), {
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

  chat.applyMessageNow(agentChunk("Working"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(agentChunk("Working"), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })
  chat.applyMessageNow(promptResult(), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })

  expect(chat.turns).toHaveLength(1)
  expect(chat.turns[0]).toMatchObject({
    source: "merged",
    completedAt: "2026-04-14T00:00:04.000Z",
    status: "completed",
  })
  expect(chat.turns[0].messages).toHaveLength(4)
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "sessionUpdate",
    "sessionUpdate",
    "turnCompletion",
  ])
  expect(chat.transcriptMessages.find((message) => message.id === "turn-1:agent")).toMatchObject({
    content: [{ type: "text", text: "WorkingWorking" }],
  })
})

test("SessionChat receiveMessage batches chunks and flushes before boundary messages", async () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.receiveMessage(agentChunk("Still "))
  chat.receiveMessage(agentChunk("working"))

  expect(chat.turns[0].messages).toHaveLength(1)

  await wait(40)

  expect(chat.turns[0].messages).toHaveLength(3)
  expect(chat.transcriptMessages.find((message) => message.id === "turn-1:agent")).toMatchObject({
    content: [{ type: "text", text: "Still working" }],
  })

  chat.receiveMessage(agentChunk("."))
  chat.receiveMessage(promptResult())

  expect(chat.turns[0]).toMatchObject({
    completedAt: expect.any(String),
    status: "completed",
  })
  expect(chat.transcriptMessages.find((message) => message.id === "turn-1:agent")).toMatchObject({
    content: [{ type: "text", text: "Still working." }],
  })
})

test("SessionChat keeps prompt and terminal messages deterministic when updates arrive out of order", () => {
  const chat = createChat({})

  chat.applyMessageNow(promptResult("prompt-late"), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })
  chat.applyMessageNow(promptMessage("prompt-late"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })

  expect(
    chat.turns[0].messages.map((message) => ("method" in message ? message.method : "result")),
  ).toEqual([acp.AGENT_METHODS.session_prompt, "result"])
  expect(chat.turns[0].status).toBe("completed")
})

test("SessionChat returns to ready status after a live turn completes", () => {
  const chat = createChat({})

  chat.applyMessageNow(promptMessage("prompt-ready"), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  expect(chat.summary.status).toBe("running")

  chat.applyMessageNow(promptResult("prompt-ready"), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })
  expect(chat.summary.status).toBe("idle")
})

test("SessionChat treats an active session without a running turn as ready", () => {
  const chat = createChat({})

  expect(chat.summary.status).toBe("idle")
})

test("SessionChat exposes the latest usage update", () => {
  const chat = createChat({
    session: createSession({
      contextUsage: {
        size: 258_400,
        used: 64_000,
      },
    }),
  })

  expect(chat.summary.contextUsage).toEqual({
    size: 258_400,
    used: 64_000,
  })

  chat.applyMessageNow(usageUpdate(258_400, 96_000), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })

  expect(chat.turns).toHaveLength(0)
  expect(chat.summary.contextUsage).toEqual({
    size: 258_400,
    used: 96_000,
  })
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

  chat.applyMessageNow(permissionRequest)
  chat.applyMessageNow(planUpdate)

  expect(chat.summary.status).toBe("blocked")
  expect(chat.summary.pendingPermissionRequest?.requestId).toBe("permission-1")
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "permissionRequest",
    "planUpdate",
  ])
})

test("SessionChat clears pending permission after the matching response", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })
  const permissionRequest = permissionRequestMessage()
  const permissionResponse = permissionResponseMessage()

  chat.applyMessageNow(permissionRequest)
  expect(chat.summary.status).toBe("blocked")
  expect(chat.summary.pendingPermissionRequest?.requestId).toBe("permission-1")

  chat.applyMessageNow(permissionResponse)

  expect(chat.summary.pendingPermissionRequest).toBeNull()
  expect(chat.summary.status).toBe("running")
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "permissionRequest",
    "permissionResponse",
  ])
})

test("SessionChat does not duplicate a permission response after refreshed history includes it", () => {
  const permissionRequest = permissionRequestMessage()
  const permissionResponse = permissionResponseMessage()
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessageNow(permissionRequest)
  chat.applyMessageNow(permissionResponse)
  chat.syncLoadedData({
    session: createSession(),
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage(), permissionRequest, permissionResponse],
      }),
    ]),
  })

  expect(chat.turns).toHaveLength(1)
  expect(chat.turns[0].messages).toHaveLength(3)
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "permissionRequest",
    "permissionResponse",
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

  chat.applyMessageNow(agentChunk("Still working"), {
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
