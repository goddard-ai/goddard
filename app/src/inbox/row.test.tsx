import type { InboxItem } from "@goddard-ai/inbox/schema"
import { expect, mock, test, vi } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { InboxPageMutations } from "./mutations.ts"
import { InboxRow } from "./row.tsrx"

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

function renderInboxRow(input: {
  item?: InboxItem
  isSelected?: boolean
  onSelectionChange?: (isSelected: boolean) => void
}) {
  const container = document.createElement("div")
  const item = input.item ?? createInboxItem({ entityId: "ses_session_1" })
  const mutations = {
    bulkUpdateInboxItems: vi.fn(),
    completeSessionInboxItem: vi.fn(async () => {}),
    openInboxItem: vi.fn(),
    updateInboxItem: vi.fn(async () => {}),
  }

  document.body.append(container)
  render(
    <InboxPageMutations mutations={mutations}>
      <InboxRow
        item={item}
        isSelected={input.isSelected}
        onSelectionChange={input.onSelectionChange}
      />
    </InboxPageMutations>,
    container,
  )

  return {
    container,
    item,
    mutations,
    openTarget: container.querySelector("[data-inbox-row-id]") as HTMLElement,
  }
}

function findButton(container: HTMLElement, label: string) {
  const button = container.querySelector(`button[aria-label='${label}']`)

  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Expected button "${label}".`)
  }

  return button
}

test("InboxRow opens rows and toggles selection through observable controls", async () => {
  const onSelectionChange = vi.fn()
  const harness = renderInboxRow({
    isSelected: false,
    onSelectionChange,
  })
  const checkbox = harness.container.querySelector("input[type='checkbox']") as HTMLInputElement

  await act(async () => {
    harness.openTarget.click()
    checkbox.checked = true
    checkbox.dispatchEvent(new Event("change", { bubbles: true }))
  })

  expect(harness.mutations.openInboxItem).toHaveBeenCalledWith(harness.item)
  expect(onSelectionChange).toHaveBeenCalledWith(true)

  render(null, harness.container)
  harness.container.remove()
})

test("InboxRow action buttons submit focused row mutations without opening the row", async () => {
  const harness = renderInboxRow({
    item: createInboxItem({
      entityId: "ses_session_1",
      status: "unread",
      priority: "normal",
    }),
  })

  await act(async () => {
    findButton(harness.container, "Mark read").click()
    await Promise.resolve()
  })
  expect(harness.mutations.updateInboxItem).toHaveBeenCalledWith({
    entityId: "ses_session_1",
    status: "read",
  })

  await act(async () => {
    findButton(harness.container, "Set low priority").click()
    await Promise.resolve()
  })
  expect(harness.mutations.updateInboxItem).toHaveBeenCalledWith({
    entityId: "ses_session_1",
    priority: "low",
  })

  await act(async () => {
    findButton(harness.container, "Archive").click()
    await Promise.resolve()
  })
  expect(harness.mutations.updateInboxItem).toHaveBeenCalledWith({
    entityId: "ses_session_1",
    status: "archived",
  })
  expect(harness.mutations.openInboxItem).not.toHaveBeenCalled()

  render(null, harness.container)
  harness.container.remove()
})

test("InboxRow uses entity-specific completion only for session rows", async () => {
  const sessionHarness = renderInboxRow({
    item: createInboxItem({
      entityId: "ses_session_1",
      status: "read",
    }),
  })

  await act(async () => {
    findButton(sessionHarness.container, "Complete").click()
    await Promise.resolve()
  })

  expect(sessionHarness.mutations.completeSessionInboxItem).toHaveBeenCalledWith({
    id: "ses_session_1",
  })

  render(null, sessionHarness.container)
  sessionHarness.container.remove()

  const pullRequestHarness = renderInboxRow({
    item: createInboxItem({
      entityId: "pr_1",
      reason: "pull_request.created",
    }),
  })

  expect(pullRequestHarness.container.querySelector("button[aria-label='Complete']")).toBeNull()

  render(null, pullRequestHarness.container)
  pullRequestHarness.container.remove()
})

test("InboxRow disables already-applied saved and archived actions", () => {
  const savedHarness = renderInboxRow({
    item: createInboxItem({
      entityId: "ses_saved",
      status: "saved",
    }),
  })

  expect(findButton(savedHarness.container, "Saved").disabled).toBe(true)

  render(null, savedHarness.container)
  savedHarness.container.remove()

  const archivedHarness = renderInboxRow({
    item: createInboxItem({
      entityId: "ses_archived",
      status: "archived",
    }),
  })

  expect(findButton(archivedHarness.container, "Archived").disabled).toBe(true)

  render(null, archivedHarness.container)
  archivedHarness.container.remove()
})
