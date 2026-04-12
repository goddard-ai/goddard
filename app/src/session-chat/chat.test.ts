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

test("buildTranscriptMessages parses prompt and update events without duplicating the latest message", () => {
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
        value: "I reviewed the diff and found one issue.",
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
      text: "Working directory: /repo-a",
    },
    {
      kind: "message",
      id: "ses_session-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      text: "Review the diff and summarize problems.",
    },
    {
      kind: "message",
      id: "ses_session-1:update:1",
      role: "assistant",
      authorName: "pi",
      timestampLabel: "Update",
      text: "I reviewed the diff and found one issue.",
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
    text: "Ready to review the diff.",
  })
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
      text: "Working directory: /repo-a",
    },
    {
      kind: "message",
      id: "ses_session-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      text: "Inspect the transcript implementation.",
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
