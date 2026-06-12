import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import { expect, test } from "bun:test"

import { buildSessionChatTranscript } from "./transcript-items.ts"

function createSession(lastAgentMessage: string | null) {
  return {
    id: "ses_session-1" as DaemonSession["id"],
    acpSessionId: "ses_session-1-acp",
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
    lastSessionActivityAt: 1_743_968_300_000,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage,
    models: null,
    configOptions: [],
    availableCommands: [],
    contextUsage: null,
  } satisfies DaemonSession
}

function createTurn(overrides: Partial<GetSessionHistoryResponse["turns"][number]> = {}) {
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
  } satisfies GetSessionHistoryResponse["turns"][number]
}

function createTurns(
  messages: GetSessionHistoryResponse["turns"][number]["messages"],
  overrides: Partial<GetSessionHistoryResponse["turns"][number]> = {},
): GetSessionHistoryResponse["turns"] {
  return [createTurn({ messages, ...overrides })]
}

function createPermissionRequestMessage(
  requestId: string,
  input: {
    optionKind: "allow_once" | "reject_once"
    optionName: string
    rawInput?: unknown
  },
) {
  const optionId = `${input.optionKind}:${requestId}`

  return {
    jsonrpc: "2.0" as const,
    id: requestId,
    method: "session/request_permission" as const,
    params: {
      sessionId: "ses_session-1-acp",
      options: [
        {
          optionId,
          name: input.optionName,
          kind: input.optionKind,
        },
      ],
      toolCall: {
        toolCallId: `tool-${requestId}`,
        title: "Edit app/src/session-chat/view.tsrx",
        kind: "edit" as const,
        status: "pending" as const,
        locations: [{ path: "/repo-a/app/src/session-chat/view.tsrx", line: 42 }],
        rawInput: input.rawInput,
      },
    },
  } satisfies GetSessionHistoryResponse["turns"][number]["messages"][number]
}

function createPermissionSelectedResponseMessage(requestId: string, optionId: string) {
  return {
    jsonrpc: "2.0" as const,
    id: requestId,
    result: {
      outcome: {
        outcome: "selected" as const,
        optionId,
      },
    },
  } satisfies GetSessionHistoryResponse["turns"][number]["messages"][number]
}

function createPermissionFailedResponseMessage(requestId: string) {
  return {
    jsonrpc: "2.0" as const,
    id: requestId,
    error: {
      code: -32_000,
      message: "Permission response failed",
    },
  } satisfies GetSessionHistoryResponse["turns"][number]["messages"][number]
}

function createPlanUpdateMessage(
  entries: Array<{
    content: string
    priority: "high" | "medium" | "low"
    status: "pending" | "in_progress" | "completed"
  }>,
) {
  return {
    jsonrpc: "2.0" as const,
    method: "session/update" as const,
    params: {
      sessionId: "ses_session-1-acp",
      update: {
        sessionUpdate: "plan" as const,
        entries,
      },
    },
  } satisfies GetSessionHistoryResponse["turns"][number]["messages"][number]
}

function createTranscriptMessages(
  session: DaemonSession,
  turns: GetSessionHistoryResponse["turns"],
) {
  return buildSessionChatTranscript({ session, turns })
}

function withoutTurnStopRows(messages: ReturnType<typeof createTranscriptMessages>) {
  return messages.filter((message) => message.kind !== "turnStop")
}

test("buildSessionChatTranscript appends the latest daemon summary when turns have no assistant update yet", () => {
  const session = createSession("Ready to review the diff.")
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: "session/prompt",
      params: {
        sessionId: session.acpSessionId,
        prompt: [{ type: "text", text: "Review the current diff." }],
      },
    },
  ])
  const latestMessage = createTranscriptMessages(session, turns).find(
    (message) => message.id === "ses_session-1:latest",
  )

  expect(latestMessage).toEqual({
    kind: "message",
    id: "ses_session-1:latest",
    role: "assistant",
    authorName: "pi",
    timestampLabel: "Latest",
    streaming: true,
    content: [{ type: "text", text: "Ready to review the diff." }],
  })
})

test("buildSessionChatTranscript hides leading system prompt blocks from user prompts", () => {
  const session = createSession(null)
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: "session/prompt",
      params: {
        sessionId: session.acpSessionId,
        prompt: [
          {
            type: "text",
            text: '<system-prompt name="goddard">Keep responses short.</system-prompt>',
          },
          {
            type: "text",
            text: '<system-prompt source="launcher" priority="low">Use repo defaults.</system-prompt>\nReview the current diff.',
          },
        ],
      },
    },
  ])
  const promptMessage = createTranscriptMessages(session, turns).find(
    (message) => message.id === "turn-1:prompt:0",
  )

  expect(promptMessage).toMatchObject({
    kind: "message",
    role: "user",
    content: [{ type: "text", text: "Review the current diff." }],
  })
})

test("buildSessionChatTranscript accumulates agent_message_chunk updates into one assistant row", () => {
  const session = createSession(null)
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: "session/prompt",
      params: {
        sessionId: session.acpSessionId,
        prompt: [{ type: "text", text: "Summarize the transcript renderer." }],
      },
    },
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "text",
            text: "# Summary",
          },
        },
      },
    },
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "text",
            text: "\n\n- Uses Comark",
          },
        },
      },
    },
  ])

  expect(withoutTurnStopRows(createTranscriptMessages(session, turns))).toEqual([
    {
      kind: "message",
      id: "ses_session-1:context",
      role: "system",
      authorName: "System",
      timestampLabel: "active",
      content: [{ type: "text", text: "Working directory: /repo-a" }],
    },
    {
      kind: "message",
      id: "turn-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      content: [{ type: "text", text: "Summarize the transcript renderer." }],
    },
    {
      kind: "message",
      id: "turn-1:agent",
      role: "assistant",
      authorName: "pi",
      timestampLabel: "Update",
      content: [{ type: "text", text: "# Summary\n\n- Uses Comark" }],
      streaming: false,
    },
  ])
})

test("buildSessionChatTranscript merges tool_call updates into one stable work drawer item", () => {
  const session = createSession(null)
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: "session/prompt",
      params: {
        sessionId: session.acpSessionId,
        prompt: [{ type: "text", text: "Inspect the transcript implementation." }],
      },
    },
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "tool_call",
          toolCallId: "tool-1",
          title: "Read transcript.tsrx",
          kind: "read",
          status: "in_progress",
          rawInput: { path: "/repo-a/app/src/session-chat/transcript.tsrx" },
          locations: [{ path: "/repo-a/app/src/session-chat/transcript.tsrx", line: 12 }],
        },
      },
    },
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "tool_call_update",
          toolCallId: "tool-1",
          status: "completed",
          content: [
            {
              type: "content",
              content: [
                {
                  type: "text",
                  text: "Loaded transcript layout and measured row logic.",
                },
              ],
            },
          ],
        },
      },
    },
  ])

  expect(withoutTurnStopRows(createTranscriptMessages(session, turns))).toEqual([
    {
      kind: "message",
      id: "ses_session-1:context",
      role: "system",
      authorName: "System",
      timestampLabel: "active",
      content: [{ type: "text", text: "Working directory: /repo-a" }],
    },
    {
      kind: "message",
      id: "turn-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      content: [{ type: "text", text: "Inspect the transcript implementation." }],
    },
    {
      kind: "workDrawer",
      id: "turn-1:work",
      title: "Worked for 1s",
      expandedByDefault: false,
      items: [
        {
          kind: "toolCall",
          id: "turn-1:tool:tool-1",
          toolCallId: "tool-1",
          authorName: "pi",
          timestampLabel: "Tool",
          title: "Read transcript.tsrx",
          toolKind: "read",
          status: "completed",
          rawInput: { path: "/repo-a/app/src/session-chat/transcript.tsrx" },
          content: [
            {
              type: "content",
              text: "Loaded transcript layout and measured row logic.",
            },
          ],
          locations: [
            {
              path: "/repo-a/app/src/session-chat/transcript.tsrx",
              line: 12,
            },
          ],
        },
      ],
    },
  ])
})

test("buildSessionChatTranscript renders turn work before the turn assistant message", () => {
  const session = createSession(null)
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: "session/prompt",
      params: {
        sessionId: session.acpSessionId,
        prompt: [{ type: "text", text: "Inspect the transcript implementation." }],
      },
    },
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "tool_call",
          toolCallId: "tool-1",
          title: "Read transcript.tsrx",
          kind: "read",
          status: "completed",
        },
      },
    },
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "text",
            text: "The transcript builder appends each turn's work drawer.",
          },
        },
      },
    },
  ])

  expect(
    withoutTurnStopRows(createTranscriptMessages(session, turns)).map((message) => message.id),
  ).toEqual(["ses_session-1:context", "turn-1:prompt:0", "turn-1:work", "turn-1:agent"])
})

test("buildSessionChatTranscript renders turn work before the latest daemon summary", () => {
  const session = createSession("Finished after reading transcript state.")
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "tool_call",
          toolCallId: "tool-1",
          title: "Read transcript state",
          kind: "read",
          status: "completed",
        },
      },
    },
  ])

  expect(
    withoutTurnStopRows(createTranscriptMessages(session, turns)).map((message) => message.id),
  ).toEqual(["ses_session-1:context", "turn-1:work", "ses_session-1:latest"])
})

test("buildSessionChatTranscript renders permission requests and their response states", () => {
  const session = createSession(null)
  const pendingRequest = createPermissionRequestMessage("permission-pending", {
    optionKind: "allow_once",
    optionName: "Allow this edit",
    rawInput: "Replace the session chat header.",
  })
  const allowedRequest = createPermissionRequestMessage("permission-allowed", {
    optionKind: "allow_once",
    optionName: "Allow once",
  })
  const deniedRequest = createPermissionRequestMessage("permission-denied", {
    optionKind: "reject_once",
    optionName: "Reject once",
  })
  const failedRequest = createPermissionRequestMessage("permission-failed", {
    optionKind: "allow_once",
    optionName: "Allow once",
  })
  const messages = createTranscriptMessages(session, [
    createTurn({
      turnId: "turn-pending",
      sequence: 1,
      promptRequestId: "prompt-pending",
      completedAt: null,
      completionKind: null,
      messages: [pendingRequest],
    }),
    createTurn({
      turnId: "turn-allowed",
      sequence: 2,
      promptRequestId: "prompt-allowed",
      messages: [
        allowedRequest,
        createPermissionSelectedResponseMessage(
          "permission-allowed",
          "allow_once:permission-allowed",
        ),
      ],
    }),
    createTurn({
      turnId: "turn-denied",
      sequence: 3,
      promptRequestId: "prompt-denied",
      messages: [
        deniedRequest,
        createPermissionSelectedResponseMessage(
          "permission-denied",
          "reject_once:permission-denied",
        ),
      ],
    }),
    createTurn({
      turnId: "turn-failed",
      sequence: 4,
      promptRequestId: "prompt-failed",
      messages: [failedRequest, createPermissionFailedResponseMessage("permission-failed")],
    }),
  ])

  expect(messages.filter((message) => message.kind === "permissionRequest")).toEqual([
    {
      kind: "permissionRequest",
      id: "turn-pending:permission:permission-pending:0",
      requestId: "permission-pending",
      authorName: "pi",
      timestampLabel: "Permission",
      title: "Edit app/src/session-chat/view.tsrx",
      toolKind: "edit",
      status: "pending",
      context: "Replace the session chat header.",
      locations: [
        {
          path: "/repo-a/app/src/session-chat/view.tsrx",
          line: 42,
        },
      ],
      options: [
        {
          optionId: "allow_once:permission-pending",
          name: "Allow this edit",
          kind: "allow_once",
        },
      ],
      selectedOptionId: null,
      error: null,
    },
    {
      kind: "permissionRequest",
      id: "turn-allowed:permission:permission-allowed:0",
      requestId: "permission-allowed",
      authorName: "pi",
      timestampLabel: "Permission",
      title: "Edit app/src/session-chat/view.tsrx",
      toolKind: "edit",
      status: "allowed",
      context: null,
      locations: [
        {
          path: "/repo-a/app/src/session-chat/view.tsrx",
          line: 42,
        },
      ],
      options: [
        {
          optionId: "allow_once:permission-allowed",
          name: "Allow once",
          kind: "allow_once",
        },
      ],
      selectedOptionId: "allow_once:permission-allowed",
      error: null,
    },
    {
      kind: "permissionRequest",
      id: "turn-denied:permission:permission-denied:0",
      requestId: "permission-denied",
      authorName: "pi",
      timestampLabel: "Permission",
      title: "Edit app/src/session-chat/view.tsrx",
      toolKind: "edit",
      status: "denied",
      context: null,
      locations: [
        {
          path: "/repo-a/app/src/session-chat/view.tsrx",
          line: 42,
        },
      ],
      options: [
        {
          optionId: "reject_once:permission-denied",
          name: "Reject once",
          kind: "reject_once",
        },
      ],
      selectedOptionId: "reject_once:permission-denied",
      error: null,
    },
    {
      kind: "permissionRequest",
      id: "turn-failed:permission:permission-failed:0",
      requestId: "permission-failed",
      authorName: "pi",
      timestampLabel: "Permission",
      title: "Edit app/src/session-chat/view.tsrx",
      toolKind: "edit",
      status: "failed",
      context: null,
      locations: [
        {
          path: "/repo-a/app/src/session-chat/view.tsrx",
          line: 42,
        },
      ],
      options: [
        {
          optionId: "allow_once:permission-failed",
          name: "Allow once",
          kind: "allow_once",
        },
      ],
      selectedOptionId: null,
      error: "Permission response failed",
    },
  ])
})

test("buildSessionChatTranscript renders distinct plan updates in order and skips repeated snapshots", () => {
  const session = createSession(null)
  const firstPlan = createPlanUpdateMessage([
    {
      content: "Inspect transcript rows",
      priority: "high",
      status: "completed",
    },
    {
      content: "Render plan update panel",
      priority: "high",
      status: "in_progress",
    },
  ])
  const secondPlan = createPlanUpdateMessage([
    {
      content: "Inspect transcript rows",
      priority: "high",
      status: "completed",
    },
    {
      content: "Render plan update panel",
      priority: "high",
      status: "completed",
    },
    {
      content: "Validate plan rows",
      priority: "medium",
      status: "pending",
    },
  ])
  const messages = createTranscriptMessages(
    session,
    createTurns([firstPlan, firstPlan, secondPlan]),
  )

  expect(messages.filter((message) => message.kind === "planUpdate")).toEqual([
    {
      kind: "planUpdate",
      id: "turn-1:plan:0",
      authorName: "pi",
      timestampLabel: "Plan",
      title: "Plan updated · 1/2 complete",
      entries: [
        {
          content: "Inspect transcript rows",
          priority: "high",
          status: "completed",
        },
        {
          content: "Render plan update panel",
          priority: "high",
          status: "in_progress",
        },
      ],
    },
    {
      kind: "planUpdate",
      id: "turn-1:plan:2",
      authorName: "pi",
      timestampLabel: "Plan",
      title: "Plan updated · 2/3 complete",
      entries: [
        {
          content: "Inspect transcript rows",
          priority: "high",
          status: "completed",
        },
        {
          content: "Render plan update panel",
          priority: "high",
          status: "completed",
        },
        {
          content: "Validate plan rows",
          priority: "medium",
          status: "pending",
        },
      ],
    },
  ])
})

test("buildSessionChatTranscript ignores routed session/update payloads without rendering transcript text", () => {
  const session = createSession(null)
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "available_commands_update",
          availableCommands: [
            {
              name: "plan",
              description: "Create or revise the plan",
            },
          ],
        },
      },
    },
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          size: 258400,
          used: 35839,
          sessionUpdate: "usage_update",
        },
      },
    },
  ])

  expect(withoutTurnStopRows(createTranscriptMessages(session, turns))).toEqual([
    {
      kind: "message",
      id: "ses_session-1:context",
      role: "system",
      authorName: "System",
      timestampLabel: "active",
      content: [{ type: "text", text: "Working directory: /repo-a" }],
    },
  ])
})

test("buildSessionChatTranscript groups thought chunks in the turn work drawer", () => {
  const session = createSession(null)
  const turns = createTurns(
    [
      {
        jsonrpc: "2.0",
        id: "prompt-1",
        method: "session/prompt",
        params: {
          sessionId: session.acpSessionId,
          prompt: [{ type: "text", text: "Inspect the current task." }],
        },
      },
      {
        jsonrpc: "2.0",
        method: "session/update",
        params: {
          sessionId: session.acpSessionId,
          update: {
            sessionUpdate: "agent_thought_chunk",
            content: {
              type: "text",
              text: "Read the request. ",
            },
          },
        },
      },
      {
        jsonrpc: "2.0",
        method: "session/update",
        params: {
          sessionId: session.acpSessionId,
          update: {
            sessionUpdate: "agent_thought_chunk",
            content: {
              type: "text",
              text: "Checked the transcript model.",
            },
          },
        },
      },
    ],
    {
      startedAt: "2026-04-14T00:00:00.000Z",
      completedAt: "2026-04-14T00:10:25.000Z",
    },
  )

  const errors: unknown[][] = []
  const originalConsoleError = console.error
  console.error = (...args) => {
    errors.push(args)
  }

  try {
    expect(withoutTurnStopRows(createTranscriptMessages(session, turns))).toEqual([
      {
        kind: "message",
        id: "ses_session-1:context",
        role: "system",
        authorName: "System",
        timestampLabel: "active",
        content: [{ type: "text", text: "Working directory: /repo-a" }],
      },
      {
        kind: "message",
        id: "turn-1:prompt:0",
        role: "user",
        authorName: "You",
        timestampLabel: "Prompt",
        content: [{ type: "text", text: "Inspect the current task." }],
      },
      {
        kind: "workDrawer",
        id: "turn-1:work",
        title: "Worked for 10m 25s",
        expandedByDefault: false,
        items: [
          {
            kind: "thought",
            id: "turn-1:thought:1",
            authorName: "pi",
            timestampLabel: "Thought",
            text: "Read the request. Checked the transcript model.",
          },
        ],
      },
    ])
  } finally {
    console.error = originalConsoleError
  }

  expect(errors).toEqual([])
})

test("buildSessionChatTranscript reports non-text agent message chunks explicitly", () => {
  const session = createSession(null)
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "image",
            uri: "file:///tmp/output.png",
          },
        },
      },
    },
  ])

  const errors: unknown[][] = []
  const originalConsoleError = console.error
  console.error = (...args) => {
    errors.push(args)
  }

  try {
    expect(withoutTurnStopRows(createTranscriptMessages(session, turns))).toEqual([
      {
        kind: "message",
        id: "ses_session-1:context",
        role: "system",
        authorName: "System",
        timestampLabel: "active",
        content: [{ type: "text", text: "Working directory: /repo-a" }],
      },
    ])
  } finally {
    console.error = originalConsoleError
  }

  expect(errors).toHaveLength(1)
  expect(errors[0]?.[1]).toEqual(
    expect.objectContaining({
      reason: "agent_message_chunk only supports text content.",
    }),
  )
})

test("buildSessionChatTranscript logs an error instead of flattening unsupported session/update payloads", () => {
  const session = createSession(null)
  const turns = createTurns([
    {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: session.acpSessionId,
        update: {
          sessionUpdate: "mystery_update",
          content: {
            type: "text",
            text: "This should not render.",
          },
        },
      },
    },
  ])

  const errors: unknown[][] = []
  const originalConsoleError = console.error
  console.error = (...args) => {
    errors.push(args)
  }

  try {
    expect(withoutTurnStopRows(createTranscriptMessages(session, turns))).toEqual([
      {
        kind: "message",
        id: "ses_session-1:context",
        role: "system",
        authorName: "System",
        timestampLabel: "active",
        content: [{ type: "text", text: "Working directory: /repo-a" }],
      },
    ])
  } finally {
    console.error = originalConsoleError
  }

  expect(errors).toHaveLength(1)
  expect(errors[0]?.[0]).toBe("Unsupported session-chat transcript message.")
  expect(errors[0]?.[1]).toEqual(
    expect.objectContaining({
      reason: "Unsupported transcript session/update payload: mystery_update",
    }),
  )
})

test("buildSessionChatTranscript renders lifecycle rows for terminal and interrupted turns", () => {
  const session = createSession(null)
  const messages = createTranscriptMessages(
    {
      ...session,
      status: "error",
      activeDaemonSession: false,
      errorMessage: "Session interrupted when the previous daemon exited unexpectedly.",
    },
    [
      createTurn({
        turnId: "turn-completed",
        sequence: 1,
        promptRequestId: "prompt-completed",
        completedAt: "2026-04-14T00:00:01.000Z",
        completionKind: "result",
        stopReason: "end_turn",
      }),
      createTurn({
        turnId: "turn-stopped",
        sequence: 2,
        promptRequestId: "prompt-stopped",
        completedAt: "2026-04-14T00:00:02.000Z",
        completionKind: "result",
        stopReason: "max_tokens",
      }),
      createTurn({
        turnId: "turn-failed",
        sequence: 3,
        promptRequestId: "prompt-failed",
        completedAt: "2026-04-14T00:00:03.000Z",
        completionKind: "error",
        stopReason: null,
        messages: [
          {
            jsonrpc: "2.0",
            id: "prompt-failed",
            error: {
              code: -32_000,
              message: "Agent crashed",
            },
          },
        ],
      }),
      createTurn({
        turnId: "turn-interrupted",
        sequence: 4,
        promptRequestId: "prompt-interrupted",
        completedAt: null,
        completionKind: null,
        stopReason: null,
      }),
    ],
  )

  expect(messages.filter((message) => message.kind === "turnStop")).toEqual([
    {
      kind: "turnStop",
      id: "turn-stopped:stop",
      status: "stopped",
      title: "Stopped",
      reason: "Reached the token limit",
      timestamp: "2026-04-14T00:00:02.000Z",
    },
    {
      kind: "turnStop",
      id: "turn-failed:stop",
      status: "failed",
      title: "Failed",
      reason: "Agent crashed",
      timestamp: "2026-04-14T00:00:03.000Z",
    },
    {
      kind: "turnStop",
      id: "turn-interrupted:stop",
      status: "interrupted",
      title: "Interrupted",
      reason: "Session interrupted when the previous daemon exited unexpectedly.",
      timestamp: null,
    },
  ])
})
