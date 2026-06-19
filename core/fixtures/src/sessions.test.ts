import { expect, test } from "bun:test"

import {
  createAcpSessionUpdateMatrixScenario,
  createFixtureInboxItem,
  createFixturePullRequest,
  createFixtureSession,
  createListSessionsResponse,
  createSessionChangesResponse,
  createSessionHistoryResponse,
  createSessionPermissionRequestMessage,
  fixtureId,
  fixtureSessionId,
} from "./index.ts"

test("fixture ids are deterministic and normalized", () => {
  expect(fixtureId("turn", "Needs Review!")).toBe("turn_needs_review")
  expect(fixtureSessionId("Launch Blocked")).toBe("ses_launch_blocked")
})

test("session fixtures derive live connection state from status", () => {
  const activeSession = createFixtureSession({ status: "active" })
  const blockedSession = createFixtureSession({ status: "blocked" })

  expect(activeSession.activeDaemonSession).toBe(true)
  expect(activeSession.connectionMode).toBe("live")
  expect(blockedSession.activeDaemonSession).toBe(true)
  expect(blockedSession.connectionMode).toBe("live")
})

test("completed session fixtures derive historical connection state from status", () => {
  const doneSession = createFixtureSession({ status: "done" })
  const errorSession = createFixtureSession({ status: "error" })

  expect(doneSession.activeDaemonSession).toBe(false)
  expect(doneSession.connectionMode).toBe("history")
  expect(errorSession.activeDaemonSession).toBe(false)
  expect(errorSession.connectionMode).toBe("live")
})

test("session response envelopes preserve supplied records", () => {
  const session = createFixtureSession({ id: fixtureSessionId("enveloped") })
  const response = createListSessionsResponse([session])

  expect(response).toEqual({
    hasMore: false,
    nextCursor: null,
    sessions: [session],
  })
})

test("detail fixtures reference the session identity", () => {
  const session = createFixtureSession({ id: fixtureSessionId("history") })
  const permission = createSessionPermissionRequestMessage({
    requestId: "permission-edit",
    session,
  })
  const history = createSessionHistoryResponse({
    session,
    turns: [{ messages: [permission] }],
  })
  const changes = createSessionChangesResponse({ session, diff: "diff --git a/file b/file" })

  expect(history.id).toBe(session.id)
  expect(history.acpSessionId).toBe(session.acpSessionId)
  expect(history.turns[0]?.messages[0]).toMatchObject({
    sequence: 0,
    sequenceStart: 0,
    message: {
      id: "permission-edit",
      method: "session/request_permission",
    },
  })
  expect(changes.id).toBe(session.id)
  expect(changes.acpSessionId).toBe(session.acpSessionId)
  expect(changes.hasChanges).toBe(true)
})

test("cross-feature fixtures create stable linked defaults", () => {
  const session = createFixtureSession({ id: fixtureSessionId("linked") })
  const inboxItem = createFixtureInboxItem({ entityId: session.id })
  const pullRequest = createFixturePullRequest({ prNumber: 42 })

  expect(inboxItem.entityId).toBe(session.id)
  expect(pullRequest.id).toBe("pr_42")
})

test("ACP update matrix fixture covers every routed session/update discriminator", () => {
  const { historyResponse } = createAcpSessionUpdateMatrixScenario()
  const sessionUpdates = new Set(
    historyResponse.turns.flatMap((turn) =>
      turn.messages.flatMap(({ message }) => {
        const sessionUpdate = getSessionUpdateDiscriminator(message)
        return sessionUpdate ? [sessionUpdate] : []
      }),
    ),
  )

  expect(sessionUpdates).toEqual(
    new Set([
      "agent_message_chunk",
      "agent_thought_chunk",
      "available_commands_update",
      "config_option_update",
      "current_mode_update",
      "plan",
      "session_info_update",
      "tool_call",
      "tool_call_update",
      "usage_update",
      "user_message_chunk",
    ]),
  )
})

function getSessionUpdateDiscriminator(message: unknown) {
  if (!isRecord(message) || !isRecord(message.params) || !isRecord(message.params.update)) {
    return null
  }

  const { sessionUpdate } = message.params.update
  return typeof sessionUpdate === "string" ? sessionUpdate : null
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}
