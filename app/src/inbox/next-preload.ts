import type { InboxItem } from "@goddard-ai/inbox/schema"

import type { PreparedInboxWorkbenchTarget } from "./open.ts"

type NextInboxWorkbenchTargetEntry = {
  cancelIdleWarm: (() => void) | null
  key: string
  promise: Promise<PreparedInboxWorkbenchTarget | null>
}

type ScheduleIdleTask = (callback: () => void) => () => void

function getNextInboxWorkbenchTargetKey(item: InboxItem) {
  return `${item.id}\0${item.entityId}\0${item.updatedAt}`
}

function scheduleIdleTask(callback: () => void) {
  const handle = requestIdleCallback(callback, { timeout: 1500 })
  return () => cancelIdleCallback(handle)
}

/** Tracks the prepared workbench target for the current Next unread inbox item. */
export class NextInboxWorkbenchPreloader {
  // The entry is non-reactive because callers only need to await or replace the latest preload.
  #entry: NextInboxWorkbenchTargetEntry | null = null
  #prepareTarget: (item: InboxItem) => Promise<PreparedInboxWorkbenchTarget | null>
  #scheduleIdleTask: ScheduleIdleTask
  #warmTargetOnIdle: ((target: PreparedInboxWorkbenchTarget) => Promise<unknown>) | null

  constructor(input: {
    prepareTarget: (item: InboxItem) => Promise<PreparedInboxWorkbenchTarget | null>
    scheduleIdleTask?: ScheduleIdleTask
    warmTargetOnIdle?: (target: PreparedInboxWorkbenchTarget) => Promise<unknown>
  }) {
    this.#prepareTarget = input.prepareTarget
    this.#scheduleIdleTask = input.scheduleIdleTask ?? scheduleIdleTask
    this.#warmTargetOnIdle = input.warmTargetOnIdle ?? null
  }

  #clearEntry() {
    this.#entry?.cancelIdleWarm?.()
    this.#entry = null
  }

  prepare(item: InboxItem | null) {
    if (!item) {
      this.#clearEntry()
      return
    }

    const key = getNextInboxWorkbenchTargetKey(item)
    if (this.#entry?.key === key) {
      return
    }

    this.#entry?.cancelIdleWarm?.()

    const entry: NextInboxWorkbenchTargetEntry = {
      cancelIdleWarm: null,
      key,
      promise: this.#prepareTarget(item),
    }
    this.#entry = entry

    void entry.promise
      .then((target) => {
        if (!target || !this.#warmTargetOnIdle || this.#entry !== entry) {
          return
        }

        entry.cancelIdleWarm = this.#scheduleIdleTask(() => {
          void this.#warmTargetOnIdle?.(target).catch(() => {})
        })
      })
      .catch(() => {})
  }

  async resolve(item: InboxItem) {
    const entry = this.#entry

    if (!entry || entry.key !== getNextInboxWorkbenchTargetKey(item)) {
      return null
    }

    try {
      const target = await entry.promise
      return this.#entry === entry ? target : null
    } catch {
      return null
    }
  }
}
