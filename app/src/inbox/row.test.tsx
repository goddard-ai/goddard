import { createFixtureInboxItem } from "@goddard-ai/fixtures"
import type { InboxItem } from "@goddard-ai/inbox/schema"
import { render } from "preact"
import { act } from "preact/test-utils"
import { expect, test, vi } from "vitest"

import { InboxPageMutations } from "./mutations.ts"
import { InboxRow } from "./row.tsrx"

function Icon(props: Record<string, unknown>) {
  return <svg {...props} />
}

vi.mock("lucide-react", () => ({
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

function renderInboxRow(input: {
  item?: InboxItem
  isSelected?: boolean
  showProjectName?: boolean
  onSelectionChange?: (isSelected: boolean) => void
}) {
  const container = document.createElement("div")
  const item = input.item ?? createFixtureInboxItem({ entityId: "ses_session_1" })
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
        showProjectName={input.showProjectName}
        onSelectionChange={input.onSelectionChange}
      />
    </InboxPageMutations>,
    container,
  )

  return {
    container,
    item,
    mutations,
    openTarget: container.querySelector("[role='button']") as HTMLElement,
  }
}

function findButton(container: HTMLElement, label: string) {
  const button = container.querySelector(`button[aria-label='${label}']`)

  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Expected button "${label}".`)
  }

  return button
}

function countTextMatches(value: string, pattern: RegExp) {
  return value.match(pattern)?.length ?? 0
}

test("InboxRow opens rows and toggles selection through observable controls", async () => {
  const onSelectionChange = vi.fn()
  const harness = renderInboxRow({
    isSelected: false,
    onSelectionChange,
  })
  const checkbox = harness.container.querySelector("input[type='checkbox']") as HTMLInputElement

  expect(harness.container.textContent).toContain("Fixture session")
  expect(harness.container.textContent).toContain("Review the latest agent update.")

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

test("InboxRow can hide repeated project context for project-scoped lists", () => {
  const globalHarness = renderInboxRow({})
  const globalSessionLabelCount = countTextMatches(
    globalHarness.container.textContent ?? "",
    /Session/g,
  )

  expect(globalSessionLabelCount).toBeGreaterThan(0)

  render(null, globalHarness.container)
  globalHarness.container.remove()

  const scopedHarness = renderInboxRow({ showProjectName: false })

  expect(countTextMatches(scopedHarness.container.textContent ?? "", /Session/g)).toBe(
    globalSessionLabelCount - 1,
  )
  expect(scopedHarness.container.textContent).toContain("Review the latest agent update.")

  render(null, scopedHarness.container)
  scopedHarness.container.remove()
})

test("InboxRow action buttons submit focused row mutations without opening the row", async () => {
  const harness = renderInboxRow({
    item: createFixtureInboxItem({
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
    item: createFixtureInboxItem({
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
    item: createFixtureInboxItem({
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
    item: createFixtureInboxItem({
      entityId: "ses_saved",
      status: "saved",
    }),
  })

  expect(findButton(savedHarness.container, "Saved").disabled).toBe(true)

  render(null, savedHarness.container)
  savedHarness.container.remove()

  const archivedHarness = renderInboxRow({
    item: createFixtureInboxItem({
      entityId: "ses_archived",
      status: "archived",
    }),
  })

  expect(findButton(archivedHarness.container, "Archived").disabled).toBe(true)

  render(null, archivedHarness.container)
  archivedHarness.container.remove()
})
