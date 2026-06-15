import {
  createFixtureSession,
  createSessionHistoryResponse,
  createSessionHistoryTurn,
} from "@goddard-ai/fixtures"
import type {
  DaemonSession,
  GetSessionHistoryResponse,
  SessionHistoryTurn,
  SessionTurnMessage,
} from "@goddard-ai/sdk"
import * as acp from "acp-client/protocol"
import { expect, test } from "bun:test"

import { SessionChat } from "./model.ts"

function createSession(overrides: Partial<DaemonSession> = {}) {
  return createFixtureSession({
    id: "ses_session-1" as DaemonSession["id"],
    acpSessionId: "acp-session-1",
    status: "active",
    agent: "pi-acp",
    agentName: "pi",
    cwd: "/repo-a",
    supportsLoadSession: false,
    title: "New session",
    titleState: "placeholder",
    repository: null,
    createdAt: 1_743_968_000_000,
    lastSessionActivityAt: 1_743_968_300_000,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    configOptions: [],
    availableCommands: [],
    contextUsage: null,
    ...overrides,
  })
}

function isTurnMessage(
  message: acp.AnyMessage | SessionTurnMessage,
): message is SessionTurnMessage {
  return "message" in message && "sequence" in message && "sequenceStart" in message
}

function turnMessage(message: acp.AnyMessage, sequence: number): SessionTurnMessage {
  return turnMessageRange(message, sequence, sequence)
}

function turnMessageRange(
  message: acp.AnyMessage,
  sequenceStart: number,
  sequence: number,
): SessionTurnMessage {
  return {
    sequence,
    sequenceStart,
    message,
  }
}

function liveMessage(message: acp.AnyMessage, sequence: number) {
  return turnMessage(message, sequence)
}

function turnMessages(messages: readonly (acp.AnyMessage | SessionTurnMessage)[]) {
  return messages.map((message, index) =>
    isTurnMessage(message) ? message : turnMessage(message, index),
  )
}

function createTurn(
  overrides: Partial<Omit<SessionHistoryTurn, "messages">> & {
    messages?: readonly (acp.AnyMessage | SessionTurnMessage)[]
  } = {},
) {
  const { messages, ...turnOverrides } = overrides
  return createSessionHistoryTurn({
    turnId: "turn-1",
    sequence: 1,
    promptRequestId: "prompt-1",
    startedAt: "2026-04-14T00:00:00.000Z",
    completedAt: "2026-04-14T00:00:01.000Z",
    completionKind: "result",
    stopReason: "end_turn",
    ...turnOverrides,
    ...(messages && { messages: turnMessages(messages) }),
  })
}

function createHistory(
  turns: SessionHistoryTurn[],
  overrides: Partial<GetSessionHistoryResponse> = {},
): GetSessionHistoryResponse {
  return {
    ...createSessionHistoryResponse({
      session: createSession(),
      turns,
    }),
    turns,
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

function thoughtChunk(text: string) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: "acp-session-1",
      update: {
        sessionUpdate: "agent_thought_chunk",
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

function toolCallMessage(
  status: "pending" | "in_progress" | "completed" | "failed" = "in_progress",
) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: "acp-session-1",
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "tool-1",
        title: "Read file",
        kind: "read",
        status,
      },
    },
  } satisfies acp.AnyMessage
}

function toolCallUpdateMessage(status: "pending" | "in_progress" | "completed" | "failed") {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: "acp-session-1",
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "tool-1",
        status,
      },
    },
  } satisfies acp.AnyMessage
}

function transcriptWorkOrder(chat: SessionChat) {
  return chat.transcriptMessages.flatMap((message) => {
    if (message.kind === "thought") {
      return [`thought:${message.text}`]
    }

    if (message.kind === "toolCall") {
      return [`tool:${message.toolCallId}:${message.status}`]
    }

    if (message.kind === "workDrawer") {
      return message.items.map((item) =>
        item.kind === "thought" ? `thought:${item.text}` : `tool:${item.toolCallId}:${item.status}`,
      )
    }

    return []
  })
}

test("SessionChat preserves history turn order and normalizes statuses", () => {
  const chat = createChat({
    session: createSession({ status: "done", activeDaemonSession: false }),
    history: createHistory([
      createTurn({ turnId: "turn-1", sequence: 1 }),
      createTurn({ turnId: "turn-2", sequence: 2, completedAt: null, completionKind: null }),
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

  chat.applyMessageNow(liveMessage(promptMessage("prompt-live"), 0), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(liveMessage(promptMessage("prompt-live"), 0), {
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

  chat.applyMessageNow(liveMessage(agentChunk("Working"), 1), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(liveMessage(agentChunk("Working"), 2), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })
  chat.applyMessageNow(liveMessage(promptResult(), 3), {
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

  chat.receiveMessage(liveMessage(agentChunk("Still "), 1))
  chat.receiveMessage(liveMessage(agentChunk("working"), 2))

  expect(chat.turns[0].messages).toHaveLength(1)

  await wait(40)

  expect(chat.turns[0].messages).toHaveLength(3)
  expect(chat.transcriptMessages.find((message) => message.id === "turn-1:agent")).toMatchObject({
    content: [{ type: "text", text: "Still working" }],
  })

  chat.receiveMessage(liveMessage(agentChunk("."), 3))
  chat.receiveMessage(liveMessage(promptResult(), 4))

  expect(chat.turns[0]).toMatchObject({
    completedAt: expect.any(String),
    status: "completed",
  })
  expect(chat.transcriptMessages.find((message) => message.id === "turn-1:agent")).toMatchObject({
    content: [{ type: "text", text: "Still working." }],
  })
})

test("SessionChat batches thought chunks separately from agent message chunks", async () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.receiveMessage(liveMessage(thoughtChunk("Inspecting "), 1))
  chat.receiveMessage(liveMessage(thoughtChunk("state."), 2))
  chat.receiveMessage(liveMessage(agentChunk("Done"), 3))

  await wait(40)

  expect(chat.turns[0].messages).toHaveLength(4)
  expect(
    chat.transcriptMessages.flatMap((message) =>
      message.kind === "thought" ? [message.text] : [],
    ),
  ).toEqual(["Inspecting state."])
  expect(chat.transcriptMessages.find((message) => message.id === "turn-1:agent")).toMatchObject({
    content: [{ type: "text", text: "Done" }],
  })
})

test("SessionChat flushes queued thought chunks before tool call boundaries", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.receiveMessage(liveMessage(thoughtChunk("Before "), 1))
  chat.receiveMessage(liveMessage(thoughtChunk("tool."), 2))
  chat.receiveMessage(liveMessage(toolCallMessage("completed"), 3))
  chat.receiveMessage(liveMessage(thoughtChunk("After tool."), 4))
  chat.flushReceivedMessages()

  expect(
    chat.transcriptMessages.flatMap((message) =>
      message.kind === "thought" ? [message.text] : [],
    ),
  ).toEqual(["Before tool.", "After tool."])
  expect(transcriptWorkOrder(chat)).toEqual([
    "thought:Before tool.",
    "tool:tool-1:completed",
    "thought:After tool.",
  ])
})

test("SessionChat preserves live work when refreshed history lags an active turn", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessageNow(liveMessage(thoughtChunk("Before tool."), 1), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(liveMessage(toolCallMessage("completed"), 2), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })
  chat.applyMessageNow(liveMessage(thoughtChunk("After tool."), 3), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })

  expect(transcriptWorkOrder(chat)).toEqual([
    "thought:Before tool.",
    "tool:tool-1:completed",
    "thought:After tool.",
  ])

  chat.syncLoadedData({
    session: createSession(),
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  expect(transcriptWorkOrder(chat)).toEqual([
    "thought:Before tool.",
    "tool:tool-1:completed",
    "thought:After tool.",
  ])
})

test("SessionChat preserves live work tail when refreshed history contains earlier work", () => {
  const beforeThought = thoughtChunk("Before tool.")
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessageNow(liveMessage(beforeThought, 1), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(liveMessage(toolCallMessage("completed"), 2), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })
  chat.applyMessageNow(liveMessage(thoughtChunk("After tool."), 3), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })

  chat.syncLoadedData({
    session: createSession(),
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage(), beforeThought],
      }),
    ]),
  })

  expect(transcriptWorkOrder(chat)).toEqual([
    "thought:Before tool.",
    "tool:tool-1:completed",
    "thought:After tool.",
  ])
})

test("SessionChat does not acknowledge repeated thought text across a tool boundary", () => {
  const repeatedThought = thoughtChunk("Checking state.")
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessageNow(liveMessage(repeatedThought, 1), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(liveMessage(toolCallMessage("completed"), 2), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })
  chat.applyMessageNow(liveMessage(thoughtChunk("Checking state."), 3), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })

  chat.syncLoadedData({
    session: createSession(),
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage(), repeatedThought],
      }),
    ]),
  })

  expect(chat.turns[0].messageRanges).toEqual([
    { sequence: 0, sequenceStart: 0 },
    { sequence: 1, sequenceStart: 1 },
    { sequence: 2, sequenceStart: 2 },
    { sequence: 3, sequenceStart: 3 },
  ])
  expect(chat.turns[0].messages).toHaveLength(4)
  expect(
    chat.transcriptMessages.flatMap((message) =>
      message.kind === "thought" ? [message.text] : [],
    ),
  ).toEqual(["Checking state.", "Checking state."])
})

test("SessionChat keeps duplicate id-less tool updates by sequence", () => {
  const repeatedToolUpdate = toolCallUpdateMessage("completed")
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessageNow(liveMessage(repeatedToolUpdate, 1), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(liveMessage(repeatedToolUpdate, 2), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })

  expect(chat.turns[0].messageRanges).toEqual([
    { sequence: 0, sequenceStart: 0 },
    { sequence: 1, sequenceStart: 1 },
    { sequence: 2, sequenceStart: 2 },
  ])
  expect(chat.turns[0].messages).toHaveLength(3)
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "sessionUpdate",
    "sessionUpdate",
  ])
})

test("SessionChat does not repeat streamed text after history refreshes with coalesced chunks", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessageNow(liveMessage(agentChunk("Still "), 1), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  chat.applyMessageNow(liveMessage(agentChunk("working"), 2), {
    receivedAt: "2026-04-14T00:00:03.000Z",
  })
  chat.applyMessageNow(liveMessage(agentChunk(" now"), 3), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })
  const refreshedHistory = createHistory([
    createTurn({
      completedAt: null,
      completionKind: null,
      messages: [promptMessage(), turnMessageRange(agentChunk("Still working"), 1, 2)],
    }),
  ])

  chat.syncLoadedData({
    session: createSession(),
    history: refreshedHistory,
  })
  chat.syncLoadedData({
    session: createSession(),
    history: refreshedHistory,
  })

  expect(chat.turns).toHaveLength(1)
  expect(chat.turns[0].messages).toHaveLength(3)
  expect(chat.transcriptMessages.find((message) => message.id === "turn-1:agent")).toMatchObject({
    content: [{ type: "text", text: "Still working now" }],
  })
})

test("SessionChat keeps prompt and terminal messages deterministic when updates arrive out of order", () => {
  const chat = createChat({})

  chat.applyMessageNow(liveMessage(promptResult("prompt-late"), 1), {
    receivedAt: "2026-04-14T00:00:04.000Z",
  })
  chat.applyMessageNow(liveMessage(promptMessage("prompt-late"), 0), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })

  expect(
    chat.turns[0].messages.map((message) => ("method" in message ? message.method : "result")),
  ).toEqual([acp.AGENT_METHODS.session_prompt, "result"])
  expect(chat.turns[0].status).toBe("completed")
})

test("SessionChat returns to ready status after a live turn completes", () => {
  const chat = createChat({})

  chat.applyMessageNow(liveMessage(promptMessage("prompt-ready"), 0), {
    receivedAt: "2026-04-14T00:00:02.000Z",
  })
  expect(chat.summary.status).toBe("running")

  chat.applyMessageNow(liveMessage(promptResult("prompt-ready"), 1), {
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

  chat.applyMessageNow(liveMessage(permissionRequest, 1))
  chat.applyMessageNow(liveMessage(planUpdate, 2))

  expect(chat.summary.status).toBe("blocked")
  expect(chat.summary.pendingPermissionRequest?.requestId).toBe("permission-1")
  expect(chat.turns[0].events.map((event) => event.kind)).toEqual([
    "prompt",
    "permissionRequest",
    "planUpdate",
  ])
})

test("SessionChat shows thinking while a turn is running outside tool calls", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  expect(chat.summary.showThinkingLabel).toBe(true)

  chat.applyMessageNow(liveMessage(toolCallMessage("in_progress"), 1))

  expect(chat.summary.showThinkingLabel).toBe(false)

  chat.applyMessageNow(liveMessage(toolCallUpdateMessage("completed"), 2))

  expect(chat.summary.showThinkingLabel).toBe(true)

  chat.applyMessageNow(liveMessage(promptResult(), 3))

  expect(chat.summary.showThinkingLabel).toBe(false)
})

test("SessionChat hides thinking while blocked on permission", () => {
  const chat = createChat({
    history: createHistory([
      createTurn({
        completedAt: null,
        completionKind: null,
        messages: [promptMessage()],
      }),
    ]),
  })

  chat.applyMessageNow(liveMessage(permissionRequestMessage(), 1))

  expect(chat.summary.status).toBe("blocked")
  expect(chat.summary.showThinkingLabel).toBe(false)
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

  chat.applyMessageNow(liveMessage(permissionRequest, 1))
  expect(chat.summary.status).toBe("blocked")
  expect(chat.summary.pendingPermissionRequest?.requestId).toBe("permission-1")

  chat.applyMessageNow(liveMessage(permissionResponse, 2))

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

  chat.applyMessageNow(liveMessage(permissionRequest, 1))
  chat.applyMessageNow(liveMessage(permissionResponse, 2))
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

  chat.applyMessageNow(liveMessage(agentChunk("Still working"), 1), {
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
