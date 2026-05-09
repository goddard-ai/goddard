import type { InboxItem, InboxItemEvent } from "@goddard-ai/schema/daemon"
import { expect, test, vi } from "bun:test"

import { Inbox } from "./model.ts"

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

function createInboxClient(
  input: {
    items?: InboxItem[]
    update?: (entityId: string) => Promise<InboxItem>
    subscribe?: (onItem: (event: InboxItemEvent) => void) => Promise<() => void>
  } = {},
) {
  const items = input.items ?? []

  return {
    list: vi.fn(async () => ({
      items,
      nextCursor: null,
      hasMore: false,
    })),
    update: vi.fn(async ({ entityId }) => ({
      item: input.update
        ? await input.update(entityId)
        : createInboxItem({
            entityId,
            status: "read",
            readAt: 2,
            updatedAt: 2,
          }),
    })),
    subscribe: vi.fn(input.subscribe ?? (async () => () => {})),
  }
}

async function waitFor(check: () => boolean) {
  for (let index = 0; index < 10; index += 1) {
    if (check()) {
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 0))
  }

  throw new Error("Timed out waiting for condition")
}

test("Inbox keeps one entity item in one current section", () => {
  const inbox = new Inbox(createInboxClient())
  const first = createInboxItem({ entityId: "ses_1", status: "unread", updatedAt: 1 })
  const second = createInboxItem({ entityId: "ses_1", status: "completed", updatedAt: 2 })

  inbox.replaceItems([first])
  expect(inbox.sections.unread.map((item) => item.entityId)).toEqual(["ses_1"])

  inbox.applyItem(second)

  expect(inbox.items).toHaveLength(1)
  expect(inbox.sections.unread).toEqual([])
  expect(inbox.sections.completed.map((item) => item.entityId)).toEqual(["ses_1"])
})

test("Inbox marks only unread session items read after a successful visit", async () => {
  const client = createInboxClient()
  const inbox = new Inbox(client)
  inbox.replaceItems([
    createInboxItem({ entityId: "ses_unread", status: "unread" }),
    createInboxItem({ entityId: "ses_read", status: "read" }),
    createInboxItem({ entityId: "pr_1", status: "unread" }),
  ])

  await inbox.markSessionVisited("ses_unread")
  await inbox.markSessionVisited("ses_read")
  await inbox.markSessionVisited("pr_1")

  expect(client.update).toHaveBeenCalledTimes(1)
  expect(client.update).toHaveBeenCalledWith({
    entityId: "ses_unread",
    status: "read",
  })
  expect(inbox.itemsByEntityId.ses_unread?.status).toBe("read")
  expect(inbox.itemsByEntityId.pr_1?.status).toBe("unread")
})

test("Inbox marks an unread session item read when it arrives after a successful visit", async () => {
  const client = createInboxClient()
  const inbox = new Inbox(client)

  await inbox.markSessionVisited("ses_late")
  inbox.applyItem(createInboxItem({ entityId: "ses_late", status: "unread" }))
  await waitFor(() => inbox.itemsByEntityId.ses_late?.status === "read")

  expect(client.update).toHaveBeenCalledTimes(1)
  expect(client.update).toHaveBeenCalledWith({
    entityId: "ses_late",
    status: "read",
  })
  expect(inbox.itemsByEntityId.ses_late?.status).toBe("read")
})

test("Inbox marks an unread session item read when it arrives in a later refresh", async () => {
  const client = createInboxClient({
    items: [createInboxItem({ entityId: "ses_late", status: "unread" })],
  })
  const inbox = new Inbox(client)

  await inbox.markSessionVisited("ses_late")
  await inbox.refresh()
  await waitFor(() => inbox.itemsByEntityId.ses_late?.status === "read")

  expect(client.update).toHaveBeenCalledTimes(1)
  expect(client.update).toHaveBeenCalledWith({
    entityId: "ses_late",
    status: "read",
  })
  expect(inbox.itemsByEntityId.ses_late?.status).toBe("read")
})

test("Inbox preserves unread state and marks stale when read-on-visit fails", async () => {
  const client = createInboxClient({
    items: [createInboxItem({ entityId: "ses_unread", status: "unread" })],
    update: async () => {
      throw new Error("daemon unavailable")
    },
  })
  const inbox = new Inbox(client)

  await inbox.refresh()

  await inbox.markSessionVisited("ses_unread")

  expect(inbox.itemsByEntityId.ses_unread?.status).toBe("unread")
  expect(inbox.connectionStatus).toBe("stale")
  expect(inbox.errorMessage).toContain("daemon unavailable")
})

test("Inbox keeps the last list visible when refresh fails after data loaded", async () => {
  const client = createInboxClient({
    items: [createInboxItem({ entityId: "ses_1", status: "unread" })],
  })
  const inbox = new Inbox(client)

  await inbox.refresh()
  client.list.mockRejectedValueOnce(new Error("disconnected"))

  await inbox.refresh()

  expect(inbox.items.map((item) => item.entityId)).toEqual(["ses_1"])
  expect(inbox.connectionStatus).toBe("stale")
  expect(inbox.errorMessage).toContain("disconnected")
})

test("Inbox starts realtime only for focused use and cleans it up", async () => {
  let onItem: ((event: InboxItemEvent) => void) | null = null
  const unsubscribe = vi.fn()
  const client = createInboxClient({
    subscribe: async (nextOnItem) => {
      onItem = nextOnItem
      return unsubscribe
    },
  })
  const inbox = new Inbox(client)

  expect(client.subscribe).not.toHaveBeenCalled()

  const stopFocusedRealtime = inbox.startFocusedRealtime()
  await waitFor(() => client.subscribe.mock.calls.length === 1)

  onItem?.({
    mutation: "touched",
    item: createInboxItem({ entityId: "ses_live", status: "unread" }),
  })
  expect(inbox.itemsByEntityId.ses_live?.status).toBe("unread")

  stopFocusedRealtime()

  expect(unsubscribe).toHaveBeenCalledTimes(1)
})
