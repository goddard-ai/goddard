import { afterEach, beforeEach, expect, test } from "bun:test"

import { createInboxManager } from "../src/inbox/manager.ts"
import { db, resetDb } from "../src/persistence/store.ts"

beforeEach(() => {
  resetDb({ filename: ":memory:" })
})

afterEach(() => {
  resetDb({ filename: ":memory:" })
})

test("daemon attention creates and refreshes one inbox row per entity", () => {
  const inbox = createInboxManager()
  const sessionId = db.sessions.newId()

  const created = inbox.touchInboxItem({
    entityId: sessionId,
    reason: "session.blocked",
    scope: "Checkout flow",
    headline: "Edge case needs review",
  })
  inbox.updateInboxItem({
    entityId: sessionId,
    status: "saved",
    priority: "low",
  })
  const refreshed = inbox.touchInboxItem({
    entityId: sessionId,
    reason: "session.turn_ended",
    headline: "Decision still needed",
  })

  expect(db.inboxItems.findMany({ where: { entityId: sessionId } })).toHaveLength(1)
  expect(refreshed.id).toBe(created.id)
  expect(refreshed).toMatchObject({
    entityId: sessionId,
    reason: "session.turn_ended",
    status: "unread",
    priority: "low",
    readAt: null,
    scope: "Checkout flow",
    headline: "Decision still needed",
  })
})

test("bulk inbox updates dedupe ids, report missing ids, and share one timestamp", () => {
  const inbox = createInboxManager()
  const firstSessionId = db.sessions.newId()
  const secondSessionId = db.sessions.newId()
  const missingSessionId = db.sessions.newId()

  inbox.touchInboxItem({
    entityId: firstSessionId,
    reason: "session.turn_ended",
    scope: "Search ranking",
    headline: "Review needed",
  })
  inbox.touchInboxItem({
    entityId: secondSessionId,
    reason: "session.blocked",
    scope: "SSO login",
    headline: "Azure path blocked",
  })

  const result = inbox.bulkUpdateInboxItems({
    entityIds: [firstSessionId, secondSessionId, firstSessionId, missingSessionId],
    status: "read",
    priority: "low",
  })

  expect(result.items).toHaveLength(2)
  expect(result.missingEntityIds).toEqual([missingSessionId])
  expect(new Set(result.items.map((item) => item.updatedAt)).size).toBe(1)
  expect(new Set(result.items.map((item) => item.readAt)).size).toBe(1)
  expect(result.items.map((item) => item.status).sort()).toEqual(["read", "read"])
  expect(result.items.map((item) => item.priority).sort()).toEqual(["low", "low"])
})

test("session replies move non-archived rows to replied but preserve archived rows", () => {
  const inbox = createInboxManager()
  const savedSessionId = db.sessions.newId()
  const completedSessionId = db.sessions.newId()
  const archivedSessionId = db.sessions.newId()

  inbox.touchInboxItem({
    entityId: savedSessionId,
    reason: "session.turn_ended",
    scope: "Customer export",
    headline: "Ready for review",
  })
  inbox.touchInboxItem({
    entityId: completedSessionId,
    reason: "session.turn_ended",
    scope: "Rollout plan",
    headline: "Marked complete",
  })
  inbox.touchInboxItem({
    entityId: archivedSessionId,
    reason: "session.blocked",
    scope: "Schema migration",
    headline: "Archived blocker",
  })
  inbox.updateInboxItem({ entityId: savedSessionId, status: "saved" })
  inbox.completeSession(completedSessionId)
  inbox.updateInboxItem({ entityId: archivedSessionId, status: "archived" })

  inbox.markSessionReplied(savedSessionId)
  inbox.markSessionReplied(completedSessionId)
  inbox.markSessionReplied(archivedSessionId)

  expect(db.inboxItems.first({ where: { entityId: savedSessionId } })?.status).toBe("replied")
  expect(db.inboxItems.first({ where: { entityId: completedSessionId } })?.status).toBe("replied")
  expect(db.inboxItems.first({ where: { entityId: archivedSessionId } })?.status).toBe("archived")
})

test("generic inbox updates reject entity-specific completion", () => {
  const inbox = createInboxManager()
  const sessionId = db.sessions.newId()
  inbox.touchInboxItem({
    entityId: sessionId,
    reason: "session.blocked",
    scope: "Checkout flow",
    headline: "Needs a decision",
  })

  expect(() => inbox.updateInboxItem({ entityId: sessionId, status: "completed" })).toThrow(
    /entity-specific/i,
  )
  expect(inbox.completeSession(sessionId)?.status).toBe("completed")
})
