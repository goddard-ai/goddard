import type { InboxStatus } from "@goddard-ai/inbox/schema"
import { t } from "@lingui/core/macro"

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
    get label() {
      return t`Unread`
    },
    statuses: ["unread", "read"],
    get emptyTitle() {
      return t`No unread inbox items`
    },
    get emptyDescription() {
      return t`Unread and acknowledged items that remain in the active queue will appear here.`
    },
  },
  saved: {
    get label() {
      return t`Saved`
    },
    statuses: ["saved"],
    get emptyTitle() {
      return t`No saved inbox items`
    },
    get emptyDescription() {
      return t`Items you park for later will appear here.`
    },
  },
  replied: {
    get label() {
      return t`Replied`
    },
    statuses: ["replied"],
    get emptyTitle() {
      return t`No replied inbox items`
    },
    get emptyDescription() {
      return t`Items handed back to an agent through a reply will appear here.`
    },
  },
  completed: {
    get label() {
      return t`Completed`
    },
    statuses: ["completed"],
    get emptyTitle() {
      return t`No completed inbox items`
    },
    get emptyDescription() {
      return t`Entity-specific completed items will appear here.`
    },
  },
  archived: {
    get label() {
      return t`Archived`
    },
    statuses: ["archived"],
    get emptyTitle() {
      return t`No archived inbox items`
    },
    get emptyDescription() {
      return t`Items hidden from the active workflow will appear here.`
    },
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
