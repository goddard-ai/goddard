import {
  createFixtureInboxItem,
  createFixturePullRequest,
  createFixtureSession,
  createSessionHistoryResponse,
} from "@goddard-ai/fixtures"
import type { InboxItem } from "@goddard-ai/inbox/schema"
import type { DaemonPullRequest } from "@goddard-ai/pull-request/schema"
import type { DaemonSession } from "@goddard-ai/sdk"
import { afterEach, beforeEach, expect, mock, test, vi } from "bun:test"

import { queryClient } from "~/lib/query.ts"
import { WorkbenchTabSet } from "~/workbench-tab-set.ts"

const inboxClient: any = {}
const sessionClient: any = {}
const prClient: any = {}
const cleanups: Array<() => void> = []

mock.module("~/sdk.ts", () => ({
  goddardSdk: {
    inbox: inboxClient,
    pr: prClient,
    session: sessionClient,
  },
}))

function createInboxItem(input: Partial<InboxItem> & Pick<InboxItem, "entityId">): InboxItem {
  return createFixtureInboxItem({
    headline: "Needs review",
    scope: "Stability work",
    updatedAt: 1_800_000_000_000,
    ...input,
  })
}

function createSession(overrides: Partial<DaemonSession> = {}): DaemonSession {
  return createFixtureSession({
    id: "ses_session_1",
    acpSessionId: "acp-session-1",
    status: "done",
    stopReason: "end_turn",
    agent: "codex",
    agentName: "Codex",
    cwd: "/repo",
    mcpServers: [],
    connectionMode: "history",
    supportsLoadSession: true,
    activeDaemonSession: false,
    completedHidden: false,
    token: null,
    permissions: null,
    title: "Review inbox routing",
    titleState: "generated",
    repository: "goddard-ai",
    prNumber: null,
    metadata: null,
    createdAt: 1_800_000_000_000,
    lastSessionActivityAt: 1_800_000_001_000,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    models: null,
    configOptions: [],
    availableCommands: [],
    contextUsage: null,
    ...overrides,
  })
}

function resetSdk() {
  inboxClient.list = vi.fn(async () => ({
    items: [],
    nextCursor: null,
    hasMore: false,
  }))
  inboxClient.update = vi.fn(async ({ entityId }: { entityId: InboxItem["entityId"] }) => ({
    item: createInboxItem({ entityId, status: "read" }),
  }))
  inboxClient.bulkUpdate = vi.fn(async ({ entityIds }: { entityIds: InboxItem["entityId"][] }) => ({
    items: entityIds.map((entityId) => createInboxItem({ entityId, status: "read" })),
    missingEntityIds: [],
  }))
  inboxClient.completeSession = vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
    item: createInboxItem({ entityId: id, status: "completed" }),
  }))
  sessionClient.get = vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
    session: createSession({ id }),
  }))
  sessionClient.history = vi.fn(async () =>
    createSessionHistoryResponse({
      session: createSession(),
      overrides: {
        connection: {
          activeDaemonSession: false,
          mode: "history",
          reconnectable: true,
        },
      },
    }),
  )
  prClient.get = vi.fn(async ({ id }: { id: DaemonPullRequest["id"] }) => ({
    pullRequest: createFixturePullRequest({
      id,
      cwd: "/repo",
      owner: "goddard-ai",
      prNumber: 42,
      repo: "goddard",
      updatedAt: 1_800_000_001_000,
    }),
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
  const workbenchTabSet = new WorkbenchTabSet()

  await openInboxItemInWorkbench({
    item: createInboxItem({ entityId: "ses_session_1" }),
    workbenchTabSet,
  })

  expect(sessionClient.get).toHaveBeenCalledWith({
    id: "ses_session_1",
  })
  expect(workbenchTabSet.activeClosableTab).toMatchObject({
    id: "session:ses_session_1",
    kind: "sessionChat",
    title: "Review inbox routing",
    props: {
      relatedFilesystemPath: "/repo",
      sessionId: "ses_session_1",
      sessionTitle: "Review inbox routing",
    },
  })
})

test("openInboxItemInWorkbench opens pull request rows as pull request tabs", async () => {
  const { openInboxItemInWorkbench } = await import("./open.ts")
  const workbenchTabSet = new WorkbenchTabSet()

  await openInboxItemInWorkbench({
    item: createInboxItem({
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
    title: "goddard-ai/goddard #42",
    props: {
      relatedFilesystemPath: "/repo",
      pullRequestId: "pr_1",
      pullRequestTitle: "goddard-ai/goddard #42",
    },
  })
})
