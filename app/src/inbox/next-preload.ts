import type { InboxItem } from "@goddard-ai/inbox/schema"

import type { PreparedInboxWorkbenchTarget } from "./open.ts"

type NextInboxWorkbenchTargetEntry = {
  key: string
  promise: Promise<PreparedInboxWorkbenchTarget | null>
}

function getNextInboxWorkbenchTargetKey(item: InboxItem) {
  return `${item.id}\0${item.entityId}\0${item.updatedAt}`
}

/** Tracks the prepared workbench target for the current Next unread inbox item. */
export class NextInboxWorkbenchPreloader {
  // The entry is non-reactive because callers only need to await or replace the latest preload.
  #entry: NextInboxWorkbenchTargetEntry | null = null
  #prepareTarget: (item: InboxItem) => Promise<PreparedInboxWorkbenchTarget | null>

  constructor(input: {
    prepareTarget: (item: InboxItem) => Promise<PreparedInboxWorkbenchTarget | null>
  }) {
    this.#prepareTarget = input.prepareTarget
  }

  prepare(item: InboxItem | null) {
    if (!item) {
      this.#entry = null
      return
    }

    const key = getNextInboxWorkbenchTargetKey(item)
    if (this.#entry?.key === key) {
      return
    }

    this.#entry = {
      key,
      promise: this.#prepareTarget(item),
    }
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
