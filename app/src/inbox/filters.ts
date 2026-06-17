import type { InboxStatus } from "@goddard-ai/inbox/schema"

import { text } from "~/language/text.ts"

export const DEFAULT_INBOX_FILTER_ID = "unread"

export const allInboxStatuses = [
  "unread",
  "read",
  "saved",
  "replied",
  "completed",
  "archived",
] as const satisfies readonly InboxStatus[]

export const inboxFilterDefinitions = {
  unread: {
    label: text.unread,
    statuses: ["unread", "read"],
    emptyTitle: text.noUnreadInboxItems,
    emptyDescription: text.unreadAndAcknowledgedItemsThatRemainInTheActiveQueueWillAppearHere,
  },
  saved: {
    label: text.saved,
    statuses: ["saved"],
    emptyTitle: text.noSavedInboxItems,
    emptyDescription: text.itemsYouParkForLaterWillAppearHere,
  },
  replied: {
    label: text.replied,
    statuses: ["replied"],
    emptyTitle: text.noRepliedInboxItems,
    emptyDescription: text.itemsHandedBackToAnAgentThroughAReplyWillAppearHere,
  },
  completed: {
    label: text.completed,
    statuses: ["completed"],
    emptyTitle: text.noCompletedInboxItems,
    emptyDescription: text.entitySpecificCompletedItemsWillAppearHere,
  },
  archived: {
    label: text.archived,
    statuses: ["archived"],
    emptyTitle: text.noArchivedInboxItems,
    emptyDescription: text.itemsHiddenFromTheActiveWorkflowWillAppearHere,
  },
} as const satisfies Record<
  string,
  {
    label: string
    statuses: readonly InboxStatus[]
    emptyTitle: string
    emptyDescription: string
  }
>

export const inboxFilterOrder = [
  "unread",
  "saved",
  "replied",
  "completed",
  "archived",
] as const satisfies readonly InboxFilterId[]

/** Stable filter ids supported by the inbox primary view. */
export type InboxFilterId = keyof typeof inboxFilterDefinitions
