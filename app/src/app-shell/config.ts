import { inboxAppPlugin } from "@goddard-ai/inbox/app"
import type { Protected } from "preact-sigma"

import type { Inbox } from "~/inbox/model.ts"
import type { MainTabItemId } from "~/main-tab-items.ts"

/** App-shell state available to sidebar indicator rules without hiding hook calls in config. */
export type AppShellSidebarState = {
  inbox: Protected<Inbox>
}

/** Reactive predicate and accessibility copy for one optional sidebar dot. */
export type AppShellSidebarDot = {
  getAriaLabel: (label: string) => string
  isVisible: (state: AppShellSidebarState) => boolean
}

/** Default no-op dot configuration for sidebar items without a custom indicator. */
export const appShellDefaultSidebarDot: AppShellSidebarDot = {
  getAriaLabel: (label) => label,
  isVisible: () => false,
}

/** Optional dot predicates keyed by sidebar main tab item. */
export const appShellSidebarDots: Partial<Record<MainTabItemId, AppShellSidebarDot>> = {
  [inboxAppPlugin.navigation.id]: {
    getAriaLabel: (label) => `${label}, unread items`,
    isVisible: (state) => state.inbox.hasUnreadItems,
  },
}
