import { expect, test } from "vitest"
import type {
  DaemonSession,
  GetDaemonSessionHistoryResponse,
  SessionPromptRequest,
} from "@goddard-ai/sdk"
import { SessionIndex } from "~/sessions/session-index.ts"
import type { SessionService } from "~/sessions/session-service.ts"
import { SessionChat } from "./chat.ts"

function createSession(id: string) {
  return {
    id: id as DaemonSession["id"],
    acpSessionId: `${id}-acp`,
    status: "active",
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
    lastAgentMessage: "Latest agent summary.",
    models: null,
  } satisfies DaemonSession
}

function createHistoryResponse(
  session: DaemonSession,
  history: GetDaemonSessionHistoryResponse["history"],
) {
  return {
    id: session.id,
    acpSessionId: session.acpSessionId,
    connection: {
      mode: session.connectionMode,
      reconnectable: true,
      activeDaemonSession: session.activeDaemonSession,
    },
    history,
  } satisfies GetDaemonSessionHistoryResponse
}

test("promptSession refreshes the daemon session and keeps transcript history in sync", async () => {
  const originalSession = createSession("ses_session-1")
  const refreshedSession = {
    ...originalSession,
    updatedAt: 1_743_968_600_000,
    lastAgentMessage: "I reviewed the diff and found one issue.",
  }
  const promptCalls: SessionPromptRequest[] = []
  const service: SessionService = {
    async createSession() {
      return originalSession
    },
    async listSessions() {
      return []
    },
    async getSession(id) {
      expect(id).toBe("ses_session-1")
      return refreshedSession
    },
    async getHistory() {
      return createHistoryResponse(refreshedSession, [
        {
          jsonrpc: "2.0",
          id: "prompt-1",
          method: "session/prompt",
          params: {
            sessionId: refreshedSession.acpSessionId,
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
      ])
    },
    async promptSession(input) {
      promptCalls.push(input)
      return { accepted: true }
    },
  }
  const sessionIndex = new SessionIndex()
  const sessionChat = new SessionChat()

  sessionIndex.upsertSession(originalSession)

  const nextSession = await sessionChat.promptSession(
    service,
    sessionIndex,
    originalSession,
    "Review the diff and summarize problems.",
  )

  expect(promptCalls).toEqual([
    {
      id: "ses_session-1",
      acpId: "ses_session-1-acp",
      prompt: "Review the diff and summarize problems.",
    },
  ])
  expect(nextSession?.updatedAt).toBe(1_743_968_600_000)
  expect(sessionIndex.getSession("ses_session-1")?.lastAgentMessage).toBe(
    "I reviewed the diff and found one issue.",
  )
  expect(sessionChat.lastMessageForSession("ses_session-1")?.text).toBe(
    "I reviewed the diff and found one issue.",
  )
})
