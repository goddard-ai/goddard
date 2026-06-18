import type { InboxItem } from "@goddard-ai/inbox/schema"
import { expect, test, vi } from "bun:test"

import { NextInboxWorkbenchPreloader } from "./next-preload.ts"
import type { PreparedInboxWorkbenchTarget } from "./open.ts"

function createInboxItem(input: Partial<InboxItem> & Pick<InboxItem, "entityId">): InboxItem {
  return {
    id: input.id ?? `inb_${input.entityId}`,
    entityId: input.entityId,
    reason: input.reason ?? "session.turn_ended",
    status: input.status ?? "unread",
    priority: input.priority ?? "normal",
    updatedAt: input.updatedAt ?? 1_800_000_000_000,
    readAt: input.readAt ?? null,
    scope: input.scope ?? "Stability work",
    headline: input.headline ?? "Needs review",
    turnId: input.turnId ?? null,
  }
}

function createPreparedTarget(item: InboxItem): PreparedInboxWorkbenchTarget {
  return {
    entityId: item.entityId,
    itemId: item.id,
    tab: {
      kind: "sessionChat",
      props: {
        relatedFilesystemPath: "/repo",
        sessionId: item.entityId as `ses_${string}`,
        sessionTitle: "Prepared session",
      },
    },
    updatedAt: item.updatedAt,
  }
}

function createDeferred<T>() {
  let reject!: (error: unknown) => void
  let resolve!: (value: T) => void
  const promise = new Promise<T>((nextResolve, nextReject) => {
    reject = nextReject
    resolve = nextResolve
  })

  return {
    promise,
    reject,
    resolve,
  }
}

test("NextInboxWorkbenchPreloader reuses the current prepared target", async () => {
  const item = createInboxItem({ entityId: "ses_1" })
  const preparedTarget = createPreparedTarget(item)
  const prepareTarget = vi.fn(async () => preparedTarget)
  const preloader = new NextInboxWorkbenchPreloader({ prepareTarget })

  preloader.prepare(item)
  preloader.prepare(item)

  await expect(preloader.resolve(item)).resolves.toBe(preparedTarget)
  expect(prepareTarget).toHaveBeenCalledTimes(1)
})

test("NextInboxWorkbenchPreloader schedules idle warming for the current prepared target", async () => {
  const item = createInboxItem({ entityId: "ses_1" })
  const preparedTarget = createPreparedTarget(item)
  const prepareTarget = vi.fn(async () => preparedTarget)
  const warmTargetOnIdle = vi.fn(async () => {})
  const scheduledTasks: Array<() => void> = []
  const preloader = new NextInboxWorkbenchPreloader({
    prepareTarget,
    scheduleIdleTask: vi.fn((callback) => {
      scheduledTasks.push(callback)
      return () => {}
    }),
    warmTargetOnIdle,
  })

  preloader.prepare(item)
  await expect(preloader.resolve(item)).resolves.toBe(preparedTarget)

  expect(scheduledTasks).toHaveLength(1)
  scheduledTasks[0]()
  expect(warmTargetOnIdle).toHaveBeenCalledWith(preparedTarget)
})

test("NextInboxWorkbenchPreloader cancels idle warming for stale prepared targets", async () => {
  const firstItem = createInboxItem({ entityId: "ses_1" })
  const secondItem = createInboxItem({ entityId: "ses_2" })
  const firstPreparedTarget = createPreparedTarget(firstItem)
  const secondPreparedTarget = createPreparedTarget(secondItem)
  const prepareTarget = vi
    .fn()
    .mockResolvedValueOnce(firstPreparedTarget)
    .mockResolvedValueOnce(secondPreparedTarget)
  const cancelIdleWarm = vi.fn()
  const preloader = new NextInboxWorkbenchPreloader({
    prepareTarget,
    scheduleIdleTask: vi.fn(() => cancelIdleWarm),
    warmTargetOnIdle: vi.fn(async () => {}),
  })

  preloader.prepare(firstItem)
  await expect(preloader.resolve(firstItem)).resolves.toBe(firstPreparedTarget)
  preloader.prepare(secondItem)

  expect(cancelIdleWarm).toHaveBeenCalledTimes(1)
})

test("NextInboxWorkbenchPreloader ignores stale prepared targets", async () => {
  const firstItem = createInboxItem({ entityId: "ses_1" })
  const secondItem = createInboxItem({ entityId: "ses_2" })
  const firstPreparedTarget = createPreparedTarget(firstItem)
  const secondPreparedTarget = createPreparedTarget(secondItem)
  const firstDeferred = createDeferred<PreparedInboxWorkbenchTarget | null>()
  const secondDeferred = createDeferred<PreparedInboxWorkbenchTarget | null>()
  const prepareTarget = vi
    .fn()
    .mockReturnValueOnce(firstDeferred.promise)
    .mockReturnValueOnce(secondDeferred.promise)
  const preloader = new NextInboxWorkbenchPreloader({ prepareTarget })

  preloader.prepare(firstItem)
  const firstResolved = preloader.resolve(firstItem)
  preloader.prepare(secondItem)
  firstDeferred.resolve(firstPreparedTarget)
  secondDeferred.resolve(secondPreparedTarget)

  await expect(firstResolved).resolves.toBeNull()
  await expect(preloader.resolve(secondItem)).resolves.toBe(secondPreparedTarget)
})

test("NextInboxWorkbenchPreloader returns null after a preparation failure", async () => {
  const item = createInboxItem({ entityId: "ses_1" })
  const prepareTarget = vi.fn(async () => {
    throw new Error("failed")
  })
  const preloader = new NextInboxWorkbenchPreloader({ prepareTarget })

  preloader.prepare(item)

  await expect(preloader.resolve(item)).resolves.toBeNull()
})
