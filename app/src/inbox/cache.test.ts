import type { InboxItem, ListInboxRequest, ListInboxResponse } from "@goddard-ai/inbox/schema"
import { expect, mock, test } from "bun:test"

mock.module("~/sdk.ts", () => ({
  goddardSdk: {
    inbox: {
      list: async () => ({
        items: [],
        nextCursor: null,
        hasMore: false,
      }),
    },
  },
}))

function createInboxItem(input: Partial<InboxItem> & Pick<InboxItem, "entityId">): InboxItem {
  return {
    id: input.id ?? `inb_${input.entityId}`,
    entityId: input.entityId,
    reason: input.reason ?? "session.turn_ended",
    status: input.status ?? "unread",
    priority: input.priority ?? "normal",
    updatedAt: input.updatedAt ?? 1,
    readAt: input.readAt ?? null,
    scope: input.scope ?? null,
    headline: input.headline ?? null,
    turnId: input.turnId ?? null,
  }
}

function createListResponse(items: InboxItem[]): ListInboxResponse {
  return {
    items,
    nextCursor: null,
    hasMore: false,
  }
}

test("applyInboxItemsToListResponse updates an existing matching row in API order", async () => {
  const { applyInboxItemsToListResponse } = await import("./cache.ts")
  const first = createInboxItem({ entityId: "ses_1", updatedAt: 1 })
  const second = createInboxItem({ entityId: "ses_2", updatedAt: 2 })
  const updatedFirst = createInboxItem({ entityId: "ses_1", headline: "Updated", updatedAt: 3 })

  const result = applyInboxItemsToListResponse(
    createListResponse([first, second]),
    { statuses: ["unread"], limit: 50 },
    [updatedFirst],
  )

  expect(result.items).toEqual([updatedFirst, second])
})

test("applyInboxItemsToListResponse removes an existing row outside the request statuses", async () => {
  const { applyInboxItemsToListResponse } = await import("./cache.ts")
  const first = createInboxItem({ entityId: "ses_1" })
  const second = createInboxItem({ entityId: "ses_2" })
  const archivedFirst = createInboxItem({ entityId: "ses_1", status: "archived" })

  const result = applyInboxItemsToListResponse(
    createListResponse([first, second]),
    { statuses: ["unread"], limit: 50 },
    [archivedFirst],
  )

  expect(result.items).toEqual([second])
})

test("applyInboxItemsToListResponse prepends a newly matching row and keeps metadata unchanged", async () => {
  const { applyInboxItemsToListResponse } = await import("./cache.ts")
  const existing = createInboxItem({ entityId: "ses_1" })
  const incoming = createInboxItem({ entityId: "ses_2", updatedAt: 10 })
  const response = {
    items: [existing],
    nextCursor: "cursor-1",
    hasMore: true,
  } satisfies ListInboxResponse

  const result = applyInboxItemsToListResponse(response, { statuses: ["unread"], limit: 50 }, [
    incoming,
  ])

  expect(result).toEqual({
    items: [incoming, existing],
    nextCursor: "cursor-1",
    hasMore: true,
  })
})

test("applyInboxItemsToListResponse ignores a new row outside the request statuses", async () => {
  const { applyInboxItemsToListResponse } = await import("./cache.ts")
  const existing = createInboxItem({ entityId: "ses_1" })
  const archived = createInboxItem({ entityId: "ses_2", status: "archived" })
  const response = createListResponse([existing])

  const result = applyInboxItemsToListResponse(response, { statuses: ["unread"], limit: 50 }, [
    archived,
  ])

  expect(result).toBe(response)
})

test("applyInboxItemsToListResponse uses the daemon default unread status filter", async () => {
  const { applyInboxItemsToListResponse } = await import("./cache.ts")
  const existing = createInboxItem({ entityId: "ses_1" })
  const readItem = createInboxItem({ entityId: "ses_2", status: "read" })
  const response = createListResponse([existing])

  const result = applyInboxItemsToListResponse(response, { limit: 50 }, [readItem])

  expect(result).toBe(response)
})

test("applyInboxItemsToListResponse trims temporary insertions to the requested limit", async () => {
  const { applyInboxItemsToListResponse } = await import("./cache.ts")
  const first = createInboxItem({ entityId: "ses_1" })
  const second = createInboxItem({ entityId: "ses_2" })
  const incoming = createInboxItem({ entityId: "ses_3" })
  const request = { statuses: ["unread"], limit: 2 } satisfies ListInboxRequest

  const result = applyInboxItemsToListResponse(createListResponse([first, second]), request, [
    incoming,
  ])

  expect(result.items).toEqual([incoming, first])
})
