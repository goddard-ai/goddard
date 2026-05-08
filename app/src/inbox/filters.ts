import type { InboxStatus } from "@goddard-ai/schema/daemon"

export const DEFAULT_INBOX_FILTER_ID = "unread"

export const inboxFilterDefinitions = {
  unread: {
    label: "Unread",
    statuses: ["unread", "read"],
    emptyTitle: "No unread inbox items",
    emptyDescription:
      "Unread and acknowledged items that remain in the active queue will appear here.",
  },
  saved: {
    label: "Saved",
    statuses: ["saved"],
    emptyTitle: "No saved inbox items",
    emptyDescription: "Items you park for later will appear here.",
  },
  replied: {
    label: "Replied",
    statuses: ["replied"],
    emptyTitle: "No replied inbox items",
    emptyDescription: "Items handed back to an agent through a reply will appear here.",
  },
  completed: {
    label: "Completed",
    statuses: ["completed"],
    emptyTitle: "No completed inbox items",
    emptyDescription: "Entity-specific completed items will appear here.",
  },
  archived: {
    label: "Archived",
    statuses: ["archived"],
    emptyTitle: "No archived inbox items",
    emptyDescription: "Items hidden from the active workflow will appear here.",
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
