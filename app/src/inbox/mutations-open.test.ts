import {
  createFixtureInboxItem,
  createFixturePullRequest,
  createFixtureSession,
  createSessionHistoryResponse,
} from "@goddard-ai/fixtures"
import type { InboxItem } from "@goddard-ai/inbox/schema"
import type { DaemonPullRequest } from "@goddard-ai/pull-request/schema"
import type { DaemonSession } from "@goddard-ai/sdk"
import { afterEach, beforeEach, expect, test, vi } from "vitest"

import { queryClient } from "~/lib/query.ts"

const inboxClient: any = {}
const sessionClient: any = {}
const prClient: any = {}
const agentClient: any = {}
const cleanups: Array<() => void> = []

vi.mock("~/sdk.ts", () => ({
  goddardSdk: {
    agent: agentClient,
    inbox: inboxClient,
    pr: prClient,
    session: sessionClient,
  },
}))

vi.mock("~/pull-requests/view.tsrx", () => ({
  default: () => null,
}))

vi.mock("~/session-chat/view.tsrx", () => ({
  default: () => null,
}))

function resetSdk() {
  inboxClient.list = vi.fn(async () => ({
    items: [],
    nextCursor: null,
    hasMore: false,
  }))
  inboxClient.update = vi.fn(async ({ entityId }: { entityId: InboxItem["entityId"] }) => ({
    item: createFixtureInboxItem({ entityId, status: "read" }),
  }))
  inboxClient.bulkUpdate = vi.fn(async ({ entityIds }: { entityIds: InboxItem["entityId"][] }) => ({
    items: entityIds.map((entityId) => createFixtureInboxItem({ entityId, status: "read" })),
    missingEntityIds: [],
  }))
  inboxClient.completeSession = vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
    item: createFixtureInboxItem({ entityId: id, status: "completed" }),
  }))
  sessionClient.get = vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
    session: createFixtureSession({ id }),
  }))
  sessionClient.history = vi.fn(async () =>
    createSessionHistoryResponse({
      session: createFixtureSession(),
      overrides: {
        connection: {
          activeDaemonSession: false,
          mode: "history",
          reconnectable: true,
        },
      },
    }),
  )
  sessionClient.worktree = {
    get: vi.fn(async () => ({
      worktree: null,
    })),
  }
  prClient.get = vi.fn(async ({ id }: { id: DaemonPullRequest["id"] }) => ({
    pullRequest: createFixturePullRequest({ id }),
  }))
  agentClient.list = vi.fn(async () => ({
    agents: [],
  }))
}

async function waitFor(check: () => boolean) {
  for (let index = 0; index < 20; index += 1) {
    if (check()) {
      return
    }

    await new Promise((resolve) => setTimeout(resolve, 0))
  }

  throw new Error("Timed out waiting for condition.")
}

async function resolveCachedRead<TQueryFn extends (...args: any[]) => Promise<any>>(
  queryFn: TQueryFn,
  args: Parameters<TQueryFn>,
) {
  const queryKey = queryClient.getQueryKey(queryFn, args)

  try {
    queryClient.read(queryKey, queryFn, args)
  } catch (value) {
    if (value instanceof Promise) {
      await value
      return queryKey
    }

    throw value
  }

  return queryKey
}

async function activateInboxQuery() {
  const queryKey = await resolveCachedRead(inboxClient.list, [
    {
      statuses: ["unread"],
      limit: 50,
    },
  ])
  const unsubscribe = queryClient.subscribe(queryKey, () => {})
  cleanups.push(unsubscribe)

  await waitFor(() => inboxClient.list.mock.calls.length >= 2)
  inboxClient.list.mockClear()
}

beforeEach(() => {
  resetSdk()
})

afterEach(() => {
  while (cleanups.length > 0) {
    cleanups.pop()?.()
  }

  vi.clearAllMocks()
})

test("inbox mutations refresh mounted inbox lists", async () => {
  const { bulkUpdateInboxItems, completeSessionInboxItem, updateInboxItem } =
    await import("./mutations.ts")

  await activateInboxQuery()

  await updateInboxItem({
    entityId: "ses_session_1",
    status: "read",
  })
  await waitFor(() => inboxClient.list.mock.calls.length === 1)
  expect(inboxClient.update).toHaveBeenCalledWith({
    entityId: "ses_session_1",
    status: "read",
  })

  inboxClient.list.mockClear()
  await bulkUpdateInboxItems({
    entityIds: ["ses_session_1", "pr_1"],
    priority: "low",
  })
  await waitFor(() => inboxClient.list.mock.calls.length === 1)
  expect(inboxClient.bulkUpdate).toHaveBeenCalledWith({
    entityIds: ["ses_session_1", "pr_1"],
    priority: "low",
  })

  inboxClient.list.mockClear()
  await completeSessionInboxItem({
    id: "ses_session_1",
  })
  await waitFor(() => inboxClient.list.mock.calls.length === 1)
  expect(inboxClient.completeSession).toHaveBeenCalledWith({
    id: "ses_session_1",
  })
})

test("openInboxItemInWorkbench opens session inbox rows as session chat tabs", async () => {
  const { openInboxItemInWorkbench } = await import("./open.ts")
  const { WorkbenchTabSet } = await import("~/workbench-tab-set.ts")
  const workbenchTabSet = new WorkbenchTabSet()

  await openInboxItemInWorkbench({
    item: createFixtureInboxItem({ entityId: "ses_session_1" }),
    workbenchTabSet,
  })

  expect(sessionClient.get).toHaveBeenCalledWith({
    id: "ses_session_1",
  })
  expect(sessionClient.history).not.toHaveBeenCalled()
  expect(sessionClient.worktree.get).toHaveBeenCalledWith({
    id: "ses_session_1",
  })
  expect(agentClient.list).toHaveBeenCalledWith({
    cwd: "/Users/alec/Projects/goddard-ai",
    includeUninstalled: true,
  })
  expect(workbenchTabSet.activeClosableTab).toMatchObject({
    id: "session:ses_session_1",
    kind: "sessionChat",
    title: "Fixture session",
    props: {
      relatedFilesystemPath: "/Users/alec/Projects/goddard-ai",
      sessionId: "ses_session_1",
      sessionTitle: "Fixture session",
    },
  })
})

test("prepareInboxItemWorkbenchTarget eagerly warms session tabs without opening them", async () => {
  const { prepareInboxItemWorkbenchTarget } = await import("./open.ts")
  const { WorkbenchTabSet } = await import("~/workbench-tab-set.ts")
  const workbenchTabSet = new WorkbenchTabSet()

  const target = await prepareInboxItemWorkbenchTarget(
    createFixtureInboxItem({ entityId: "ses_prepared" }),
  )

  expect(target).toMatchObject({
    entityId: "ses_prepared",
    itemId: "inb_ses_prepared",
    tab: {
      kind: "sessionChat",
      props: {
        relatedFilesystemPath: "/Users/alec/Projects/goddard-ai",
        sessionId: "ses_prepared",
        sessionTitle: "Fixture session",
      },
    },
  })
  expect(sessionClient.get).toHaveBeenCalledWith({
    id: "ses_prepared",
  })
  expect(sessionClient.history).not.toHaveBeenCalled()
  expect(sessionClient.worktree.get).toHaveBeenCalledWith({
    id: "ses_prepared",
  })
  expect(agentClient.list).toHaveBeenCalledWith({
    cwd: "/Users/alec/Projects/goddard-ai",
    includeUninstalled: true,
  })
  expect(workbenchTabSet.activeClosableTab).toBeNull()
})

test("warmPreparedInboxWorkbenchTargetOnIdle warms session history", async () => {
  const { prepareInboxItemWorkbenchTarget, warmPreparedInboxWorkbenchTargetOnIdle } =
    await import("./open.ts")
  const target = await prepareInboxItemWorkbenchTarget(
    createFixtureInboxItem({ entityId: "ses_idle" }),
  )

  sessionClient.history.mockClear()

  expect(target).not.toBeNull()
  await warmPreparedInboxWorkbenchTargetOnIdle(target!)

  expect(sessionClient.history).toHaveBeenCalledWith({
    id: "ses_idle",
  })
})

test("openInboxItemInWorkbench reuses a matching prepared session target", async () => {
  const { openInboxItemInWorkbench, prepareInboxItemWorkbenchTarget } = await import("./open.ts")
  const { WorkbenchTabSet } = await import("~/workbench-tab-set.ts")
  const item = createFixtureInboxItem({ entityId: "ses_reused" })
  const preparedTarget = await prepareInboxItemWorkbenchTarget(item)
  const workbenchTabSet = new WorkbenchTabSet()

  sessionClient.get.mockClear()
  sessionClient.history.mockClear()
  sessionClient.worktree.get.mockClear()
  agentClient.list.mockClear()

  await openInboxItemInWorkbench({
    item,
    preparedTarget,
    workbenchTabSet,
  })

  expect(sessionClient.get).not.toHaveBeenCalled()
  expect(sessionClient.history).not.toHaveBeenCalled()
  expect(sessionClient.worktree.get).not.toHaveBeenCalled()
  expect(agentClient.list).not.toHaveBeenCalled()
  expect(workbenchTabSet.activeClosableTab).toMatchObject({
    id: "session:ses_reused",
    kind: "sessionChat",
  })
})

test("openInboxItemInWorkbench closes the previous clean tab when requested", async () => {
  const { openInboxItemInWorkbench } = await import("./open.ts")
  const { WorkbenchTabSet } = await import("~/workbench-tab-set.ts")
  const workbenchTabSet = new WorkbenchTabSet()

  workbenchTabSet.openOrFocusTab({
    kind: "sessionChat",
    props: {
      relatedFilesystemPath: null,
      sessionId: "ses_previous",
      sessionTitle: "Previous session",
    },
  })

  await openInboxItemInWorkbench({
    closeCurrentCleanTab: true,
    item: createFixtureInboxItem({ entityId: "ses_next" }),
    workbenchTabSet,
  })

  expect(workbenchTabSet.tabs["session:ses_previous"]).toBeUndefined()
  expect(workbenchTabSet.activeClosableTab).toMatchObject({
    id: "session:ses_next",
    kind: "sessionChat",
  })
})

test("openInboxItemInWorkbench keeps the previous dirty tab when requested", async () => {
  const { openInboxItemInWorkbench } = await import("./open.ts")
  const { WorkbenchTabSet } = await import("~/workbench-tab-set.ts")
  const workbenchTabSet = new WorkbenchTabSet()

  workbenchTabSet.openOrFocusTab({
    kind: "sessionChat",
    props: {
      relatedFilesystemPath: null,
      sessionId: "ses_previous_dirty",
      sessionTitle: "Previous dirty session",
    },
  })
  workbenchTabSet.setTabDirty("session:ses_previous_dirty", true)

  await openInboxItemInWorkbench({
    closeCurrentCleanTab: true,
    item: createFixtureInboxItem({ entityId: "ses_next_dirty" }),
    workbenchTabSet,
  })

  expect(workbenchTabSet.tabs["session:ses_previous_dirty"]).toBeDefined()
  expect(workbenchTabSet.activeClosableTab).toMatchObject({
    id: "session:ses_next_dirty",
    kind: "sessionChat",
  })
})

test("openInboxItemInWorkbench opens pull request rows as pull request tabs", async () => {
  const { openInboxItemInWorkbench } = await import("./open.ts")
  const { WorkbenchTabSet } = await import("~/workbench-tab-set.ts")
  const workbenchTabSet = new WorkbenchTabSet()

  await openInboxItemInWorkbench({
    item: createFixtureInboxItem({
      entityId: "pr_1",
      reason: "pull_request.created",
    }),
    workbenchTabSet,
  })

  expect(prClient.get).toHaveBeenCalledWith({
    id: "pr_1",
  })
  expect(workbenchTabSet.activeClosableTab).toMatchObject({
    id: "pull-request:pr_1",
    kind: "pullRequest",
    title: "goddard-ai/goddard-ai #128",
    props: {
      relatedFilesystemPath: "/Users/alec/Projects/goddard-ai",
      pullRequestId: "pr_1",
      pullRequestTitle: "goddard-ai/goddard-ai #128",
    },
  })
})

test("prepareInboxItemWorkbenchTarget warms pull request tabs without opening them", async () => {
  const { prepareInboxItemWorkbenchTarget } = await import("./open.ts")
  const { WorkbenchTabSet } = await import("~/workbench-tab-set.ts")
  const workbenchTabSet = new WorkbenchTabSet()

  const target = await prepareInboxItemWorkbenchTarget(
    createFixtureInboxItem({
      entityId: "pr_prepared",
      reason: "pull_request.created",
    }),
  )

  expect(target).toMatchObject({
    entityId: "pr_prepared",
    itemId: "inb_pr_prepared",
    tab: {
      kind: "pullRequest",
      props: {
        relatedFilesystemPath: "/Users/alec/Projects/goddard-ai",
        pullRequestId: "pr_prepared",
        pullRequestTitle: "goddard-ai/goddard-ai #128",
      },
    },
  })
  expect(prClient.get).toHaveBeenCalledWith({
    id: "pr_prepared",
  })
  expect(workbenchTabSet.activeClosableTab).toBeNull()
})
