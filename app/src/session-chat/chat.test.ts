import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import { expect, test } from "bun:test"

import { buildTranscriptMessages } from "./chat.ts"

function createSession(lastAgentMessage: string | null) {
  return {
    id: "ses_session-1" as DaemonSession["id"],
    acpSessionId: "ses_session-1-acp",
    status: "active",
    stopReason: null,
    agentName: "pi",
    cwd: "/repo-a",
    mcpServers: [],
    connectionMode: "live",
    activeDaemonSession: true,
    token: null,
    permissions: null,
    repository: null,
    prNumber: null,
    metadata: null,
    createdAt: 1_743_968_000_000,
    updatedAt: 1_743_968_300_000,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    lastAgentMessage,
    models: null,
  } satisfies DaemonSession
}

test("buildTranscriptMessages uses agent_message_chunk content without duplicating the latest message", () => {
  const session = createSession("I reviewed the diff and found one issue.")
  const history = [
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: "session/prompt",
      params: {
        sessionId: session.acpSessionId,
        prompt: [{ type: "text", text: "Review the diff and summarize problems." }],
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
            text: "I reviewed the diff and found one issue.",
          },
        },
      },
    },
  ] satisfies GetSessionHistoryResponse["history"]

  expect(buildTranscriptMessages(session, history)).toEqual([
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
      id: "ses_session-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      content: [{ type: "text", text: "Review the diff and summarize problems." }],
    },
    {
      kind: "message",
      id: "prompt-1:agent",
      role: "assistant",
      authorName: "pi",
      timestampLabel: "Update",
      content: [{ type: "text", text: "I reviewed the diff and found one issue." }],
      streaming: true,
    },
  ])
})

test("buildTranscriptMessages appends the latest daemon summary when history has no assistant update yet", () => {
  const session = createSession("Ready to review the diff.")
  const history = [
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: "session/prompt",
      params: {
        sessionId: session.acpSessionId,
        prompt: [{ type: "text", text: "Review the current diff." }],
      },
    },
  ] satisfies GetSessionHistoryResponse["history"]

  expect(buildTranscriptMessages(session, history).at(-1)).toEqual({
    kind: "message",
    id: "ses_session-1:latest",
    role: "assistant",
    authorName: "pi",
    timestampLabel: "Latest",
    streaming: true,
    content: [{ type: "text", text: "Ready to review the diff." }],
  })
})

test("buildTranscriptMessages accumulates agent_message_chunk updates into one assistant row", () => {
  const session = createSession(null)
  const history = [
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
    {
      jsonrpc: "2.0",
      id: "prompt-1",
      result: {
        stopReason: "end_turn",
      },
    },
  ] satisfies GetSessionHistoryResponse["history"]

  expect(buildTranscriptMessages(session, history)).toEqual([
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
      id: "ses_session-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      content: [{ type: "text", text: "Summarize the transcript renderer." }],
    },
    {
      kind: "message",
      id: "prompt-1:agent",
      role: "assistant",
      authorName: "pi",
      timestampLabel: "Update",
      content: [{ type: "text", text: "# Summary\n\n- Uses Comark" }],
      streaming: false,
    },
  ])
})

test("buildTranscriptMessages merges tool_call updates into one stable tool row", () => {
  const session = createSession(null)
  const history = [
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
          title: "Read transcript.tsx",
          kind: "read",
          status: "in_progress",
          locations: [{ path: "/repo-a/app/src/session-chat/transcript.tsx", line: 12 }],
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
              content: [{ type: "text", text: "Loaded transcript layout and measured row logic." }],
            },
          ],
        },
      },
    },
  ] satisfies GetSessionHistoryResponse["history"]

  expect(buildTranscriptMessages(session, history)).toEqual([
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
      id: "ses_session-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      content: [{ type: "text", text: "Inspect the transcript implementation." }],
    },
    {
      kind: "toolCall",
      id: "prompt-1:tool:tool-1",
      toolCallId: "tool-1",
      authorName: "pi",
      timestampLabel: "Tool",
      title: "Read transcript.tsx",
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
          path: "/repo-a/app/src/session-chat/transcript.tsx",
          line: 12,
        },
      ],
    },
  ])
})

test("buildTranscriptMessages ignores routed session/update payloads without rendering transcript text", () => {
  const session = createSession(null)
  const history = [
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
  ] satisfies GetSessionHistoryResponse["history"]

  expect(buildTranscriptMessages(session, history)).toEqual([
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

test("buildTranscriptMessages logs an error instead of flattening unsupported session/update payloads", () => {
  const session = createSession(null)
  const history = [
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
  ] satisfies GetSessionHistoryResponse["history"]

  const errors: unknown[][] = []
  const originalConsoleError = console.error
  console.error = (...args) => {
    errors.push(args)
  }

  try {
    expect(buildTranscriptMessages(session, history)).toEqual([
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
