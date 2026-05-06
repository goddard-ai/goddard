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
    lastAgentMessage,
    models: null,
    availableCommands: [],
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

test("buildSessionChatTranscript merges tool_call updates into one stable tool row", () => {
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
      kind: "toolCall",
      id: "turn-1:tool:tool-1",
      toolCallId: "tool-1",
      authorName: "pi",
      timestampLabel: "Tool",
      title: "Read transcript.tsrx",
      toolKind: "read",
      status: "completed",
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
      id: "turn-completed:stop",
      status: "completed",
      title: "Completed",
      reason: null,
      timestamp: "2026-04-14T00:00:01.000Z",
    },
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
