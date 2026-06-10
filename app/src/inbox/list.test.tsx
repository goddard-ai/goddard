import type { InboxItem } from "@goddard-ai/inbox/schema"
import { expect, mock, test, vi } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { InboxBulkToolbar } from "./bulk-toolbar.tsrx"
import { InboxList } from "./list.tsrx"
import { InboxPageMutations } from "./mutations.ts"

function Icon(props: Record<string, unknown>) {
  return <svg {...props} />
}

mock.module("lucide-react", () => ({
  Archive: Icon,
  ArrowDown: Icon,
  ArrowUp: Icon,
  Bookmark: Icon,
  CheckCircle2: Icon,
  GitPullRequest: Icon,
  Mail: Icon,
  MailOpen: Icon,
  MessageSquare: Icon,
  X: Icon,
}))

function createInboxItem(input: Partial<InboxItem> & Pick<InboxItem, "entityId">): InboxItem {
  return {
    id: input.id ?? `inb_${input.entityId}`,
    entityId: input.entityId,
    reason: input.reason ?? "session.turn_ended",
    status: input.status ?? "unread",
    priority: input.priority ?? "normal",
    updatedAt: input.updatedAt ?? Date.now(),
    readAt: input.readAt ?? null,
    scope: input.scope ?? "Inbox automation",
    headline: input.headline ?? "Review the latest result",
    turnId: input.turnId ?? null,
  }
}

function renderInboxList(element: preact.ComponentChildren) {
  const container = document.createElement("div")
  document.body.append(container)
  render(
    <InboxPageMutations
      mutations={{
        bulkUpdateInboxItems: vi.fn(),
        completeSessionInboxItem: vi.fn(),
        openInboxItem: vi.fn(),
        updateInboxItem: vi.fn(),
      }}
    >
      {element}
    </InboxPageMutations>,
    container,
  )

  return container
}

test("InboxList renders loading, error, empty, and populated states from observable props", () => {
  const container = renderInboxList(
    <InboxList filterId="unread" items={[]} listStatus="loading" searchQuery="" />,
  )
  expect(container.textContent).toContain("Loading inbox")

  render(
    <InboxPageMutations
      mutations={{
        bulkUpdateInboxItems: vi.fn(),
        completeSessionInboxItem: vi.fn(),
        openInboxItem: vi.fn(),
        updateInboxItem: vi.fn(),
      }}
    >
      <InboxList
        errorMessage="Daemon unavailable"
        filterId="unread"
        items={[]}
        listStatus="error"
        searchQuery=""
      />
    </InboxPageMutations>,
    container,
  )
  expect(container.textContent).toContain("Couldn't load inbox")
  expect(container.textContent).toContain("Daemon unavailable")

  render(
    <InboxPageMutations
      mutations={{
        bulkUpdateInboxItems: vi.fn(),
        completeSessionInboxItem: vi.fn(),
        openInboxItem: vi.fn(),
        updateInboxItem: vi.fn(),
      }}
    >
      <InboxList filterId="saved" items={[]} searchQuery="" />
    </InboxPageMutations>,
    container,
  )
  expect(container.textContent).toContain("No saved inbox items")

  render(
    <InboxPageMutations
      mutations={{
        bulkUpdateInboxItems: vi.fn(),
        completeSessionInboxItem: vi.fn(),
        openInboxItem: vi.fn(),
        updateInboxItem: vi.fn(),
      }}
    >
      <InboxList
        activeItemId="inb_ses_session_1"
        filterId="unread"
        items={[createInboxItem({ entityId: "ses_session_1" })]}
        searchQuery=""
        selectedEntityIds={new Set(["ses_session_1"])}
      />
    </InboxPageMutations>,
    container,
  )
  expect(container.querySelector("ul[aria-label='Unread inbox items']")).not.toBeNull()
  expect((container.querySelector("input[type='checkbox']") as HTMLInputElement).checked).toBe(true)

  render(null, container)
  container.remove()
})

test("InboxBulkToolbar exposes selected-row actions and stays hidden without a selection", async () => {
  const callbacks = {
    onArchive: vi.fn(),
    onClear: vi.fn(),
    onMarkRead: vi.fn(),
    onMarkUnread: vi.fn(),
    onSave: vi.fn(),
    onSetLowPriority: vi.fn(),
    onSetNormalPriority: vi.fn(),
  }
  const container = document.createElement("div")
  document.body.append(container)

  render(<InboxBulkToolbar selectedCount={0} {...callbacks} />, container)
  expect(container.textContent).toBe("")

  render(<InboxBulkToolbar selectedCount={2} {...callbacks} />, container)

  await act(async () => {
    ;(
      container.querySelector("button[aria-label='Mark selected read']") as HTMLButtonElement
    ).click()
    ;(container.querySelector("button[aria-label='Archive selected']") as HTMLButtonElement).click()
    ;(container.querySelector("button[aria-label='Clear selection']") as HTMLButtonElement).click()
  })

  expect(container.textContent).toContain("2 selected")
  expect(callbacks.onMarkRead).toHaveBeenCalledTimes(1)
  expect(callbacks.onArchive).toHaveBeenCalledTimes(1)
  expect(callbacks.onClear).toHaveBeenCalledTimes(1)

  render(null, container)
  container.remove()
})
