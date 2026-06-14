import { createFixtureSession, createSessionHistoryResponse } from "@goddard-ai/fixtures"
import type { CreateSessionRequest, DaemonSession, SessionLifecycleEvent } from "@goddard-ai/sdk"
import { afterEach, beforeEach, expect, mock, test, vi } from "bun:test"

import { queryClient } from "~/lib/query.ts"
import { SESSION_LIST_LIMIT } from "./queries.ts"

const sessionClient: any = {}
const inboxClient: any = {}
const cleanups: Array<() => void> = []

mock.module("~/sdk.ts", () => ({
  goddardSdk: {
    inbox: inboxClient,
    session: sessionClient,
  },
}))

function resetSdk() {
  sessionClient.create = vi.fn(async () => ({ session: createFixtureSession() }))
  sessionClient.list = vi.fn(async () => ({
    sessions: [createFixtureSession()],
    nextCursor: null,
    hasMore: false,
  }))
  sessionClient.get = vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
    session: createFixtureSession({ id }),
  }))
  sessionClient.history = vi.fn(async ({ id }: { id: DaemonSession["id"] }) =>
    createSessionHistoryResponse({ session: createFixtureSession({ id }) }),
  )
  sessionClient.worktree = {
    get: vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
      id,
      acpSessionId: "acp-session-1",
      worktree: {
        repoRoot: "/repo",
        requestedCwd: "/repo",
        effectiveCwd: "/repo/.worktrees/session",
        worktreeDir: "/repo/.worktrees/session",
        branchName: "goddard/example",
        mergeTargetBranch: "main",
        poweredBy: "default",
      },
    })),
    mergeReadiness: vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
      id,
      acpSessionId: "acp-session-1",
      readiness: {
        status: "ready",
        mergeTargetBranch: "main",
        worktreeHeadOid: "abc123",
        worktreeHeadBranch: "goddard/example",
        targetBranchHeadOid: "def456",
        aheadCount: 1,
        syncMounted: false,
        willAutoUnmountSync: false,
      },
    })),
    mergeTargetBranch: {
      set: vi.fn(
        async ({
          id,
          mergeTargetBranch,
        }: {
          id: DaemonSession["id"]
          mergeTargetBranch: string | null
        }) => ({
          id,
          acpSessionId: "acp-session-1",
          mergeTargetBranch,
          readiness: {
            status: mergeTargetBranch ? "ready" : "merge_target_branch_required",
            mergeTargetBranch,
            worktreeHeadOid: "abc123",
            worktreeHeadBranch: "goddard/example",
            targetBranchHeadOid: mergeTargetBranch ? "def456" : null,
            aheadCount: mergeTargetBranch ? 1 : 0,
            syncMounted: false,
            willAutoUnmountSync: false,
          },
        }),
      ),
    },
    merge: vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
      id,
      acpSessionId: "acp-session-1",
      merged: true,
      targetBranch: "main",
      sourceHeadOid: "abc123",
      previousTargetHeadOid: "def456",
      nextTargetHeadOid: "abc123",
      syncUnmounted: false,
      warnings: [],
    })),
  }
  sessionClient.prompt = vi.fn(async () => ({ accepted: true }))
  sessionClient.configOption = {
    set: vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
      session: createFixtureSession({ id }),
    })),
  }
  sessionClient.model = {
    set: vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
      session: createFixtureSession({ id }),
    })),
  }
  sessionClient.respondPermission = vi.fn(async () => ({ accepted: true }))
  sessionClient.connect = vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
    session: createFixtureSession({ id }),
  }))
  sessionClient.cancel = vi.fn(async () => ({
    activeTurnCancelled: true,
    abortedQueue: [],
  }))
  sessionClient.launchPreview = vi.fn(async () => ({
    launchLeaseId: "lease_1",
    repoRoot: "/repo",
    bare: false,
    branches: ["main"],
    currentBranch: "main",
    dirty: false,
    configOptions: [],
    slashCommands: [],
  }))
  sessionClient.launchLease = {
    release: vi.fn(async ({ launchLeaseId }: { launchLeaseId: string }) => ({
      launchLeaseId,
      released: true,
    })),
  }
  sessionClient.streamLifecycle = vi.fn()

  inboxClient.list = vi.fn(async () => ({
    items: [],
    nextCursor: null,
    hasMore: false,
  }))
  inboxClient.completeSession = vi.fn(async ({ id }: { id: DaemonSession["id"] }) => ({
    item: null,
    session: createFixtureSession({ id }),
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

async function activateCachedQuery<TQueryFn extends (...args: any[]) => Promise<any>>(
  queryFn: TQueryFn,
  args: Parameters<TQueryFn>,
) {
  const mockQueryFn = queryFn as TQueryFn & {
    mock: { calls: unknown[] }
    mockClear(): void
  }
  const queryKey = await resolveCachedRead(queryFn, args)
  const unsubscribe = queryClient.subscribe(queryKey, () => {})
  cleanups.push(unsubscribe)

  await waitFor(() => mockQueryFn.mock.calls.length >= 2)
  mockQueryFn.mockClear()
}

async function activateSessionViewQueries(sessionId: DaemonSession["id"]) {
  await activateCachedQuery(sessionClient.list, [{ limit: SESSION_LIST_LIMIT }])
  await activateCachedQuery(sessionClient.get, [{ id: sessionId }])
  await activateCachedQuery(sessionClient.history, [{ id: sessionId }])
  await activateCachedQuery(sessionClient.worktree.get, [{ id: sessionId }])
  await activateCachedQuery(sessionClient.worktree.mergeReadiness, [{ id: sessionId }])
}

async function expectSessionViewsRefreshed() {
  await waitFor(
    () =>
      sessionClient.list.mock.calls.length === 1 &&
      sessionClient.get.mock.calls.length === 1 &&
      sessionClient.history.mock.calls.length === 1 &&
      sessionClient.worktree.get.mock.calls.length === 1 &&
      sessionClient.worktree.mergeReadiness.mock.calls.length === 1,
  )
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

test("createSession refreshes session lists and launch previews", async () => {
  const { createSession: runCreateSession } = await import("./mutations.ts")
  const input: CreateSessionRequest = {
    agent: "codex",
    cwd: "/repo",
    mcpServers: [],
    initialPrompt: [{ type: "text", text: "Add tests." }],
  }

  await activateCachedQuery(sessionClient.list, [{ limit: SESSION_LIST_LIMIT }])
  await activateCachedQuery(sessionClient.launchPreview, [{ agent: "codex", cwd: "/repo" }])

  await expect(runCreateSession(input)).resolves.toEqual({
    session: createFixtureSession(),
  })
  await waitFor(
    () =>
      sessionClient.list.mock.calls.length === 1 &&
      sessionClient.launchPreview.mock.calls.length === 1,
  )

  expect(sessionClient.create).toHaveBeenCalledWith(input)
})

test("session mutations refresh list, detail, and transcript queries", async () => {
  const {
    cancelSessionTurn,
    mergeSessionWorktree,
    reconnectSession,
    respondSessionPermission,
    setSessionWorktreeMergeTargetBranch,
    setSessionConfigOption,
    setSessionModel,
    submitSessionPrompt,
  } = await import("./mutations.ts")
  const sessionId = "ses_session_1" as DaemonSession["id"]

  await activateSessionViewQueries(sessionId)

  const cases = [
    {
      run: () =>
        submitSessionPrompt({
          id: sessionId,
          acpId: "acp-session-1",
          prompt: [{ type: "text", text: "Continue." }],
        }),
      assert: () =>
        expect(sessionClient.prompt).toHaveBeenCalledWith({
          id: sessionId,
          acpId: "acp-session-1",
          prompt: [{ type: "text", text: "Continue." }],
        }),
    },
    {
      run: () =>
        setSessionConfigOption({
          id: sessionId,
          configId: "mode",
          value: "plan",
        }),
      assert: () =>
        expect(sessionClient.configOption.set).toHaveBeenCalledWith({
          id: sessionId,
          configId: "mode",
          value: "plan",
        }),
    },
    {
      run: () =>
        setSessionModel({
          id: sessionId,
          modelId: "opus",
        }),
      assert: () =>
        expect(sessionClient.model.set).toHaveBeenCalledWith({
          id: sessionId,
          modelId: "opus",
        }),
    },
    {
      run: () =>
        respondSessionPermission({
          id: sessionId,
          requestId: "permission-1",
          outcome: {
            outcome: "selected",
            optionId: "allow",
          },
        }),
      assert: () =>
        expect(sessionClient.respondPermission).toHaveBeenCalledWith({
          id: sessionId,
          requestId: "permission-1",
          outcome: {
            outcome: "selected",
            optionId: "allow",
          },
        }),
    },
    {
      run: () => reconnectSession(sessionId),
      assert: () => expect(sessionClient.connect).toHaveBeenCalledWith({ id: sessionId }),
    },
    {
      run: () => cancelSessionTurn(sessionId),
      assert: () => expect(sessionClient.cancel).toHaveBeenCalledWith({ id: sessionId }),
    },
    {
      run: () =>
        setSessionWorktreeMergeTargetBranch({
          id: sessionId,
          mergeTargetBranch: "release/1.x",
        }),
      assert: () =>
        expect(sessionClient.worktree.mergeTargetBranch.set).toHaveBeenCalledWith({
          id: sessionId,
          mergeTargetBranch: "release/1.x",
        }),
    },
    {
      run: () => mergeSessionWorktree({ id: sessionId }),
      assert: () => expect(sessionClient.worktree.merge).toHaveBeenCalledWith({ id: sessionId }),
    },
  ]

  for (const item of cases) {
    sessionClient.list.mockClear()
    sessionClient.get.mockClear()
    sessionClient.history.mockClear()
    sessionClient.worktree.get.mockClear()
    sessionClient.worktree.mergeReadiness.mockClear()

    await item.run()
    await expectSessionViewsRefreshed()
    item.assert()
  }
})

test("completeSession uses the inbox completion mutation and refreshes session plus inbox queries", async () => {
  const { completeSession } = await import("./mutations.ts")
  const sessionId = "ses_session_1" as DaemonSession["id"]

  await activateSessionViewQueries(sessionId)
  await activateCachedQuery(inboxClient.list, [{ statuses: ["unread"], limit: 50 }])

  await completeSession(sessionId)
  await waitFor(
    () =>
      sessionClient.list.mock.calls.length === 1 &&
      sessionClient.get.mock.calls.length === 1 &&
      sessionClient.history.mock.calls.length === 1 &&
      sessionClient.worktree.get.mock.calls.length === 1 &&
      sessionClient.worktree.mergeReadiness.mock.calls.length === 1 &&
      inboxClient.list.mock.calls.length === 1,
  )

  expect(inboxClient.completeSession).toHaveBeenCalledWith({ id: sessionId })
})

test("releaseSessionLaunchLease ignores empty ids and releases concrete leases", async () => {
  const { releaseSessionLaunchLease } = await import("./mutations.ts")

  await releaseSessionLaunchLease(null)
  await releaseSessionLaunchLease(undefined)
  await releaseSessionLaunchLease("lease_1")

  expect(sessionClient.launchLease.release).toHaveBeenCalledTimes(1)
  expect(sessionClient.launchLease.release).toHaveBeenCalledWith({
    launchLeaseId: "lease_1",
  })
})

test("startSessionLifecycleSubscription refreshes caches for streamed lifecycle events", async () => {
  const { startSessionLifecycleSubscription } = await import("./lifecycle.ts")
  const sessionId = "ses_session_1" as DaemonSession["id"]
  let pushEvent!: (event: SessionLifecycleEvent) => void
  let wakeAbort!: () => void

  sessionClient.streamLifecycle = vi.fn(async (_input, options: { signal: AbortSignal }) => {
    const queue: SessionLifecycleEvent[] = []
    let wakeEvent: (() => void) | null = null
    pushEvent = (event) => {
      queue.push(event)
      wakeEvent?.()
    }

    options.signal.addEventListener("abort", () => {
      wakeAbort?.()
      wakeEvent?.()
    })

    return (async function* () {
      while (!options.signal.aborted) {
        if (queue.length === 0) {
          await new Promise<void>((resolve) => {
            wakeEvent = resolve
            wakeAbort = resolve
          })
          wakeEvent = null
        }

        const event = queue.shift()
        if (event) {
          yield event
        }
      }
    })()
  })

  await activateSessionViewQueries(sessionId)

  const stop = startSessionLifecycleSubscription()
  await waitFor(() => sessionClient.streamLifecycle.mock.calls.length === 1)

  sessionClient.list.mockClear()
  sessionClient.get.mockClear()
  sessionClient.history.mockClear()
  sessionClient.worktree.get.mockClear()
  sessionClient.worktree.mergeReadiness.mockClear()

  pushEvent({
    kind: "sessionUpdated",
    session: createFixtureSession({ id: sessionId }),
    changed: ["status"],
  })
  await expectSessionViewsRefreshed()

  stop()

  expect(sessionClient.streamLifecycle.mock.calls[0][1].signal.aborted).toBe(true)
})
