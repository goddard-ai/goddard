import { randomUUID } from "node:crypto"
import { IpcClientError } from "@goddard-ai/ipc"
import type { SessionId } from "@goddard-ai/session/schema"
import { afterEach, beforeEach, expect, test } from "bun:test"
import { kindstore, type Kindstore } from "kindstore"

import { createInboxManager, inboxPlugin } from "../src/daemon.ts"
import { InboxErrorCodes } from "../src/schema.ts"

let store: Kindstore<(typeof inboxPlugin)["db"]["schema"], {}>

beforeEach(() => {
  store = kindstore({
    filename: ":memory:",
    schema: inboxPlugin.db.schema,
  })
})

afterEach(() => {
  store.close()
})

const noopInboxEvents: Parameters<typeof createInboxManager>[0]["events"] = {
  emit: async () => {},
}

function createTestInboxManager() {
  return createInboxManager({ db: store, events: noopInboxEvents })
}

function newSessionId() {
  return `ses_${randomUUID()}` as SessionId
}

test("daemon attention creates and refreshes one inbox row per entity", () => {
  const inbox = createTestInboxManager()
  const sessionId = newSessionId()

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

  expect(store.inboxItems.findMany({ where: { entityId: sessionId } })).toHaveLength(1)
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
  const inbox = createTestInboxManager()
  const firstSessionId = newSessionId()
  const secondSessionId = newSessionId()
  const missingSessionId = newSessionId()

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
  const inbox = createTestInboxManager()
  const savedSessionId = newSessionId()
  const completedSessionId = newSessionId()
  const archivedSessionId = newSessionId()

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

  expect(store.inboxItems.first({ where: { entityId: savedSessionId } })?.status).toBe("replied")
  expect(store.inboxItems.first({ where: { entityId: completedSessionId } })?.status).toBe(
    "replied",
  )
  expect(store.inboxItems.first({ where: { entityId: archivedSessionId } })?.status).toBe(
    "archived",
  )
})

test("generic inbox updates reject entity-specific completion", () => {
  const inbox = createTestInboxManager()
  const sessionId = newSessionId()
  inbox.touchInboxItem({
    entityId: sessionId,
    reason: "session.blocked",
    scope: "Checkout flow",
    headline: "Needs a decision",
  })

  expect(() => inbox.updateInboxItem({ entityId: sessionId, status: "completed" })).toThrow(
    IpcClientError,
  )
  try {
    inbox.updateInboxItem({ entityId: sessionId, status: "completed" })
    throw new Error("Expected inbox update to reject entity-specific completion")
  } catch (error) {
    expect(error).toHaveProperty("code", InboxErrorCodes.CompletedRequiresEntityOperation)
  }
  expect(inbox.completeSession(sessionId)?.status).toBe("completed")
})

test("inbox manager emits one daemon event per changed item", () => {
  const events: Array<{
    name: string
    entityId: string
    status: string
  }> = []
  const inbox = createInboxManager({
    db: store,
    events: {
      emit: async (name, item) => {
        events.push({
          name,
          entityId: item.entityId,
          status: item.status,
        })
      },
    },
  })
  const firstSessionId = newSessionId()
  const secondSessionId = newSessionId()

  inbox.touchInboxItem({
    entityId: firstSessionId,
    reason: "session.turn_ended",
    scope: "Search ranking",
    headline: "Review needed",
  })
  inbox.touchInboxItem({
    entityId: secondSessionId,
    reason: "session.turn_ended",
    scope: "SSO login",
    headline: "Review needed",
  })
  inbox.updateInboxItem({ entityId: firstSessionId, status: "read" })
  inbox.bulkUpdateInboxItems({
    entityIds: [firstSessionId, secondSessionId],
    priority: "low",
  })
  inbox.markSessionReplied(firstSessionId)
  inbox.completeSession(secondSessionId)

  expect(events).toEqual([
    { name: "inbox.item.updated", entityId: firstSessionId, status: "unread" },
    { name: "inbox.item.updated", entityId: secondSessionId, status: "unread" },
    { name: "inbox.item.updated", entityId: firstSessionId, status: "read" },
    { name: "inbox.item.updated", entityId: firstSessionId, status: "read" },
    { name: "inbox.item.updated", entityId: secondSessionId, status: "unread" },
    { name: "inbox.item.updated", entityId: firstSessionId, status: "replied" },
    { name: "inbox.item.updated", entityId: secondSessionId, status: "completed" },
  ])
  expect(store.inboxItems.findMany({ where: { entityId: firstSessionId } })).toHaveLength(1)
  expect(store.inboxItems.findMany({ where: { entityId: secondSessionId } })).toHaveLength(1)
})
