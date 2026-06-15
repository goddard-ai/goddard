import { expect, test } from "bun:test"

import {
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
