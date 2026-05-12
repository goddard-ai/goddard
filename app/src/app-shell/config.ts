import { useListener } from "preact-sigma"
import { useEffect } from "preact/hooks"

import { useInbox } from "~/app-state-context.tsrx"
import type { NavigationItemId } from "~/navigation.ts"

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

/** One grouped section in the left app shell rail. */
type AppShellSection = {
  group: "primary" | "secondary"
  tabKinds: readonly NavigationItemId[]
}

/** Ordered left-rail sections rendered by the merged app shell. */
export const appShellSections: readonly AppShellSection[] = [
  {
    group: "primary",
    tabKinds: ["inbox", "sessions", "search"],
  },
  {
    group: "secondary",
    tabKinds: ["specs", "tasks", "roadmap"],
  },
]

function useInboxUnreadDot() {
  const inbox = useInbox()

  useEffect(() => {
    void inbox.refresh()
  }, [inbox])

  useListener(window, "focus", () => {
    void inbox.refresh()
  })

  useListener(document, "visibilitychange", () => {
    if (document.visibilityState === "visible") {
      void inbox.refresh()
    }
  })

  return inbox.hasUnreadItems
}

/** Optional dot predicates keyed by sidebar navigation item. */
export const appShellSidebarDots: Partial<Record<NavigationItemId, AppShellSidebarDot>> = {
  inbox: {
    getAriaLabel: (label) => `${label}, unread items`,
    usePredicate: useInboxUnreadDot,
  },
}
