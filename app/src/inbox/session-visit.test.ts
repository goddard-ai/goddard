import { createFixtureInboxItem } from "@goddard-ai/fixtures"
import type { InboxItem } from "@goddard-ai/inbox/schema"
import { afterEach, expect, mock, test, vi } from "bun:test"

const inboxClient = {
  list: vi.fn(),
  update: vi.fn(),
}

mock.module("~/sdk.ts", () => ({
  goddardSdk: {
    inbox: inboxClient,
  },
}))

function createInboxItem(input: Partial<InboxItem> & Pick<InboxItem, "entityId">): InboxItem {
  return createFixtureInboxItem({ updatedAt: 1, scope: null, headline: null, ...input })
}

async function loadSessionVisitModule() {
  const module = await import("./session-visit.ts")
  module.resetInboxSessionVisitStateForTest()
  inboxClient.list.mockResolvedValue({
    items: [],
    nextCursor: null,
    hasMore: false,
  })
  inboxClient.update.mockImplementation(async ({ entityId }) => ({
    item: createInboxItem({
      entityId,
      status: "read",
      readAt: 2,
      updatedAt: 2,
    }),
  }))
  return module
}

afterEach(() => {
  vi.clearAllMocks()
})

async function waitFor(check: () => boolean) {
  for (let index = 0; index < 10; index += 1) {
    if (check()) {
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 0))
  }

  throw new Error("Timed out waiting for condition")
}

test("markInboxSessionVisited marks an existing unread session item read", async () => {
  const { handleInboxItemsLoaded, markInboxSessionVisited } = await loadSessionVisitModule()

  handleInboxItemsLoaded([createInboxItem({ entityId: "ses_unread", status: "unread" })])
  markInboxSessionVisited("ses_unread")
  await waitFor(() => inboxClient.update.mock.calls.length === 1)

  expect(inboxClient.update).toHaveBeenCalledWith({
    entityId: "ses_unread",
    status: "read",
  })
})

test("handleInboxItemsLoaded marks unread visited session items read when they arrive later", async () => {
  const { handleInboxItemsLoaded, markInboxSessionVisited } = await loadSessionVisitModule()

  markInboxSessionVisited("ses_late")
  handleInboxItemsLoaded([createInboxItem({ entityId: "ses_late", status: "unread" })])
  await waitFor(() => inboxClient.update.mock.calls.length === 1)

  expect(inboxClient.update).toHaveBeenCalledWith({
    entityId: "ses_late",
    status: "read",
  })
})

test("markInboxSessionVisited ignores pull request and already-read items", async () => {
  const { handleInboxItemsLoaded, markInboxSessionVisited } = await loadSessionVisitModule()

  handleInboxItemsLoaded([
    createInboxItem({ entityId: "pr_1", status: "unread" }),
    createInboxItem({ entityId: "ses_read", status: "read" }),
  ])
  markInboxSessionVisited("pr_1")
  markInboxSessionVisited("ses_read")
  await Promise.resolve()

  expect(inboxClient.update).not.toHaveBeenCalled()
})

test("markInboxSessionVisited avoids duplicate read updates while pending", async () => {
  const { handleInboxItemsLoaded, markInboxSessionVisited } = await loadSessionVisitModule()
  let resolveUpdate!: (value: unknown) => void
  inboxClient.update.mockImplementationOnce(({ entityId }) =>
    new Promise((resolve) => {
      resolveUpdate = resolve
    }).then(() => ({
      item: createInboxItem({
        entityId,
        status: "read",
        readAt: 2,
        updatedAt: 2,
      }),
    })),
  )

  handleInboxItemsLoaded([createInboxItem({ entityId: "ses_pending", status: "unread" })])
  markInboxSessionVisited("ses_pending")
  markInboxSessionVisited("ses_pending")
  await Promise.resolve()

  expect(inboxClient.update).toHaveBeenCalledTimes(1)

  resolveUpdate(undefined)
  await waitFor(() => inboxClient.update.mock.results[0]?.type === "return")
})

test("markInboxSessionVisited does not locally mark read after a failed update", async () => {
  const { handleInboxItemsLoaded, markInboxSessionVisited } = await loadSessionVisitModule()
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => {})
  inboxClient.update.mockRejectedValueOnce(new Error("daemon unavailable"))

  handleInboxItemsLoaded([createInboxItem({ entityId: "ses_retry", status: "unread" })])
  markInboxSessionVisited("ses_retry")
  await waitFor(() => inboxClient.update.mock.calls.length === 1)
  markInboxSessionVisited("ses_retry")
  await waitFor(() => inboxClient.update.mock.calls.length === 2)

  expect(inboxClient.update).toHaveBeenNthCalledWith(2, {
    entityId: "ses_retry",
    status: "read",
  })
  consoleError.mockRestore()
})
