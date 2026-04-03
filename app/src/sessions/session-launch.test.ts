import { expect, test } from "vitest"
import type {
  DaemonSession,
  GetDaemonSessionHistoryResponse,
  SessionPromptRequest,
} from "@goddard-ai/sdk"
import { SessionChat } from "~/session-chat/chat.ts"
import { SessionIndex } from "./session-index.ts"
import { SessionLaunch } from "./session-launch.ts"
import type { SessionService } from "./session-service.ts"

function createSession(id: string, cwd: string, lastAgentMessage: string | null) {
  return {
    id: id as DaemonSession["id"],
    acpSessionId: `${id}-acp`,
    status: "active",
    agentName: "pi",
    cwd,
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

test("submitLaunch creates a daemon-backed session and seeds the parsed transcript", async () => {
  const createdSession = createSession("ses_session-1", "/repo-a", "Ready to review the diff.")
  const service: SessionService = {
    async createSession(input) {
      expect(input).toMatchObject({
        agent: "pi",
        cwd: "/repo-a",
        initialPrompt: "Review the current diff.",
      })
      return createdSession
    },
    async listSessions() {
      return []
    },
    async getSession() {
      return createdSession
    },
    async getHistory() {
      return createHistoryResponse(createdSession, [
        {
          jsonrpc: "2.0",
          id: "prompt-1",
          method: "session/prompt",
          params: {
            sessionId: createdSession.acpSessionId,
            prompt: [{ type: "text", text: "Review the current diff." }],
          },
        },
        {
          jsonrpc: "2.0",
          method: "session/update",
          params: {
            text: "Ready to review the diff.",
          },
        },
      ])
    },
    async promptSession(_input: SessionPromptRequest) {
      return { accepted: true }
    },
  }
  const sessionIndex = new SessionIndex()
  const sessionChat = new SessionChat()
  const sessionLaunch = new SessionLaunch()

  sessionLaunch.openDialog("/repo-a")
  sessionLaunch.setDraftPrompt("Review the current diff.")

  const launchedSession = await sessionLaunch.submitLaunch(service, sessionIndex, sessionChat)

  expect(launchedSession?.id).toBe("ses_session-1")
  expect(sessionIndex.sessionList.map((session) => session.id)).toEqual(["ses_session-1"])
  expect(sessionChat.messagesForSession("ses_session-1")).toEqual([
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
      text: "Review the current diff.",
    },
    {
      id: "ses_session-1:update:1",
      role: "assistant",
      authorName: "pi",
      timestampLabel: "Update",
      text: "Ready to review the diff.",
    },
  ])
})
