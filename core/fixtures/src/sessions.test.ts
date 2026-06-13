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
  fixtureNow,
  fixtureSessionId,
} from "./index.ts"

test("fixture ids are deterministic and normalized", () => {
  expect(fixtureId("turn", "Needs Review!")).toBe("turn_needs_review")
  expect(fixtureSessionId("Launch Blocked")).toBe("ses_launch_blocked")
})

test("session fixture derives live state from status and accepts overrides", () => {
  const session = createFixtureSession({
    id: fixtureSessionId("blocked"),
    blockedReason: "Needs approval.",
    status: "blocked",
    title: "Blocked fixture",
    updatedAt: fixtureNow - 1_000,
  })

  expect(session).toMatchObject({
    id: "ses_blocked",
    activeDaemonSession: true,
    blockedReason: "Needs approval.",
    connectionMode: "live",
    title: "Blocked fixture",
    updatedAt: fixtureNow - 1_000,
  })
})

test("completed session fixtures default to history mode", () => {
  const session = createFixtureSession({ status: "done" })

  expect(session.activeDaemonSession).toBe(false)
  expect(session.connectionMode).toBe("history")
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

test("history fixtures reference the session identity", () => {
  const session = createFixtureSession({ id: fixtureSessionId("history") })
  const permission = createSessionPermissionRequestMessage({
    requestId: "permission-edit",
    session,
  })
  const history = createSessionHistoryResponse({
    session,
    turns: [{ messages: [permission] }],
  })

  expect(history.id).toBe(session.id)
  expect(history.acpSessionId).toBe(session.acpSessionId)
  expect(history.turns[0]?.messages[0]).toMatchObject({
    id: "permission-edit",
    method: "session/request_permission",
  })
})

test("inbox, pull request, and changes fixtures use schema-shaped defaults", () => {
  const session = createFixtureSession({ id: fixtureSessionId("linked") })
  const inboxItem = createFixtureInboxItem({ entityId: session.id })
  const pullRequest = createFixturePullRequest({ prNumber: 42 })
  const changes = createSessionChangesResponse({ session, diff: "diff --git a/file b/file" })

  expect(inboxItem.entityId).toBe(session.id)
  expect(pullRequest.id).toBe("pr_42")
  expect(changes.hasChanges).toBe(true)
})
