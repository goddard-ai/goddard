import type { InboxItem } from "@goddard-ai/schema/daemon"

import { getInboxEntityLabel } from "./entity-kind.ts"
import { getInboxPriorityLabel, getInboxReasonLabel, getInboxStatusLabel } from "./labels.ts"

function compactText(parts: Array<string | null | undefined>) {
  return parts
    .map((part) => part?.trim() ?? "")
    .filter((part) => part.length > 0)
    .join(" ")
}

/** Returns the primary row text, preferring the daemon-supplied stable scope. */
export function getInboxItemPrimaryText(item: InboxItem) {
  return item.scope?.trim() || getInboxEntityLabel(item.entityId)
}

/** Returns the secondary row text, preferring the daemon-supplied turn headline. */
export function getInboxItemSecondaryText(item: InboxItem) {
  return item.headline?.trim() || getInboxReasonLabel(item.reason)
}

/** Returns a single searchable row string without creating app-owned inbox data. */
export function getInboxItemSearchText(item: InboxItem) {
  return compactText([
    getInboxEntityLabel(item.entityId),
    getInboxStatusLabel(item.status),
    getInboxPriorityLabel(item.priority),
    getInboxReasonLabel(item.reason),
    item.scope,
    item.headline,
    item.entityId,
  ])
}
