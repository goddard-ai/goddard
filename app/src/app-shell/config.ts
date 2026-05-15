import { useInbox } from "~/app-state-context.tsrx"
import type { MainTabItemId } from "~/main-tab-items.ts"

/** Reactive predicate and accessibility copy for one optional sidebar dot. */
export type AppShellSidebarDot = {
  getAriaLabel: (label: string) => string
  usePredicate: () => boolean
}

/** Default no-op dot configuration for sidebar items without a custom indicator. */
export const appShellDefaultSidebarDot: AppShellSidebarDot = {
  getAriaLabel: (label) => label,
  usePredicate: () => false,
}

/** Optional dot predicates keyed by sidebar main tab item. */
export const appShellSidebarDots: Partial<Record<MainTabItemId, AppShellSidebarDot>> = {
  inbox: {
    getAriaLabel: (label) => `${label}, unread items`,
    usePredicate: () => {
      const inbox = useInbox()
      return inbox.hasUnreadItems
    },
  },
}
