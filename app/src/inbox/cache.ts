import type {
  InboxItem,
  InboxStatus,
  ListInboxRequest,
  ListInboxResponse,
} from "@goddard-ai/inbox/schema"

import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"

const defaultInboxStatuses: readonly InboxStatus[] = ["unread"]

function getRequestStatuses(request: ListInboxRequest) {
  return request.statuses ?? defaultInboxStatuses
}

function includesItemStatus(request: ListInboxRequest, item: InboxItem) {
  return getRequestStatuses(request).includes(item.status)
}

/**
 * Applies daemon-returned inbox rows to one cached list response without re-sorting API order.
 */
export function applyInboxItemsToListResponse(
  response: ListInboxResponse,
  request: ListInboxRequest,
  items: readonly InboxItem[],
) {
  let nextItems = response.items
  let changed = false

  for (const item of items) {
    const existingIndex = nextItems.findIndex((candidate) => candidate.entityId === item.entityId)
    const shouldInclude = includesItemStatus(request, item)

    if (existingIndex >= 0) {
      if (shouldInclude) {
        const replacedItems = [...nextItems]
        replacedItems[existingIndex] = item
        nextItems = replacedItems
      } else {
        nextItems = nextItems.filter((candidate) => candidate.entityId !== item.entityId)
      }
      changed = true
      continue
    }

    if (shouldInclude) {
      nextItems = [item, ...nextItems]
      changed = true
    }
  }

  if (request.limit !== undefined && nextItems.length > request.limit) {
    nextItems = nextItems.slice(0, request.limit)
  }

  return changed ? { ...response, items: nextItems } : response
}

/** Applies daemon-returned inbox rows to every existing cached inbox list response. */
export function applyInboxItemsToCache(items: readonly InboxItem[]) {
  if (items.length === 0) {
    return 0
  }

  return queryClient.writeAll(goddardSdk.inbox.list, (response, [request]) =>
    applyInboxItemsToListResponse(response, request ?? {}, items),
  )
}
