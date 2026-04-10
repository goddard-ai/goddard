import { expect, test } from "bun:test"
import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
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
      id: "ses_session-1:context",
      role: "system",
      authorName: "System",
      timestampLabel: "active",
      text: "Working directory: /repo-a",
    },
    {
      id: "ses_session-1:prompt:0",
      role: "user",
      authorName: "You",
      timestampLabel: "Prompt",
      text: "Review the diff and summarize problems.",
    },
    {
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
    id: "ses_session-1:latest",
    role: "assistant",
    authorName: "pi",
    timestampLabel: "Latest",
    text: "Ready to review the diff.",
  })
})
