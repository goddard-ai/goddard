import type { InboxEntityId, InboxItem, InboxStatus } from "@goddard-ai/schema/daemon"
import { Sigma } from "preact-sigma"
import { getErrorMessage, objectify } from "radashi"

import { goddardSdk } from "~/sdk.ts"

export type InboxConnectionStatus = "loading" | "ready" | "stale" | "error"

export type InboxSectionId = InboxStatus

export type InboxState = {
  itemsByEntityId: Record<string, InboxItem>
  connectionStatus: InboxConnectionStatus
  errorMessage: string | null
  loadedAt: number | null
}

const inboxStatuses: InboxStatus[] = ["unread", "read", "replied", "completed", "saved", "archived"]

function compareInboxItems(left: InboxItem, right: InboxItem) {
  return right.updatedAt - left.updatedAt || right.id.localeCompare(left.id)
}

function isSessionEntityId(entityId: InboxEntityId) {
  return entityId.startsWith("ses_")
}

/** Reactive owner for app-visible daemon inbox rows and realtime updates. */
export class Inbox extends Sigma<InboxState> {
  /** Entity ids with an in-flight read-on-visit mutation to prevent duplicate updates. */
  #pendingReadEntityIds = new Set<InboxEntityId>()
  /** Session entity ids that have loaded successfully, including before their inbox rows arrive. */
  #visitedSessionEntityIds = new Set<InboxEntityId>()

  constructor() {
    super({
      itemsByEntityId: {},
      connectionStatus: "loading",
      errorMessage: null,
      loadedAt: null,
    })
  }

  /** Returns all known inbox items in daemon ordering. */
  get items() {
    return Object.values(this.itemsByEntityId).sort(compareInboxItems)
  }

  /** Returns all known inbox items grouped into exactly one section by current status. */
  get sections() {
    const sections = objectify(
      inboxStatuses,
      (key) => key,
      () => [] as InboxItem[],
    )

    for (const item of this.items) {
      sections[item.status].push(item)
    }

    return sections
  }

  /** Returns whether any known item is unread. */
  get hasUnreadItems() {
    return this.sections.unread.length > 0
  }

  /** Loads the current inbox snapshot while preserving stale data on failure. */
  async refresh() {
    if (this.loadedAt === null) {
      this.connectionStatus = "loading"
      this.commit()
    }

    try {
      const response = await goddardSdk.inbox.list({
        statuses: inboxStatuses,
        limit: 100,
      })
      this.replaceItems(response.items)
      this.connectionStatus = "ready"
      this.errorMessage = null
      this.loadedAt = Date.now()
      this.commit()
    } catch (error) {
      this.markConnectionFailed(error)
      this.commit()
    }
  }

  /** Replaces the current snapshot with a daemon list response. */
  replaceItems(items: readonly InboxItem[]) {
    this.itemsByEntityId = Object.fromEntries(items.map((item) => [item.entityId, item]))
    this.#markVisitedUnreadItemsRead(items)
  }

  /** Merges one daemon-published inbox row into the app snapshot. */
  applyItem(item: InboxItem) {
    this.itemsByEntityId[item.entityId] = item
    this.connectionStatus = this.loadedAt === null ? "ready" : this.connectionStatus
    this.errorMessage = null

    if (item.status === "unread" && this.#visitedSessionEntityIds.has(item.entityId)) {
      queueMicrotask(() => {
        void this.markSessionVisited(item.entityId)
      })
    }
  }

  /** Marks a session inbox item read after its associated session entity has loaded successfully. */
  async markSessionVisited(sessionId: InboxEntityId) {
    if (!isSessionEntityId(sessionId)) {
      return
    }

    this.#visitedSessionEntityIds.add(sessionId)

    if (this.#pendingReadEntityIds.has(sessionId)) {
      return
    }

    const item = this.itemsByEntityId[sessionId]
    if (!item || item.status !== "unread") {
      return
    }

    this.#pendingReadEntityIds.add(sessionId)
    try {
      const result = await goddardSdk.inbox.update({
        entityId: sessionId,
        status: "read",
      })
      this.applyItem(result.item)
      this.commit()
    } catch (error) {
      this.markConnectionFailed(error)
      this.commit()
    } finally {
      this.#pendingReadEntityIds.delete(sessionId)
    }
  }

  /** Starts Inbox-tab scoped realtime updates. */
  startFocusedRealtime() {
    let active = true
    let unsubscribe: (() => void) | null = null

    void goddardSdk.inbox
      .subscribe((event) => {
        if (active) {
          this.applyItem(event.item)
        }
      })
      .then(
        (nextUnsubscribe) => {
          if (active) {
            unsubscribe = nextUnsubscribe
          } else {
            nextUnsubscribe()
          }
        },
        (error) => {
          if (active) {
            this.markConnectionFailed(error)
          }
        },
      )

    return () => {
      active = false
      unsubscribe?.()
      unsubscribe = null
    }
  }

  #markVisitedUnreadItemsRead(items: readonly InboxItem[]) {
    for (const item of items) {
      if (item.status === "unread" && this.#visitedSessionEntityIds.has(item.entityId)) {
        queueMicrotask(() => {
          void this.markSessionVisited(item.entityId)
        })
      }
    }
  }

  markConnectionFailed(error: unknown) {
    this.connectionStatus =
      this.loadedAt === null && Object.keys(this.itemsByEntityId).length === 0 ? "error" : "stale"
    this.errorMessage = getErrorMessage(error)
  }
}

export interface Inbox extends InboxState {}
