import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"
import { useListener } from "preact-sigma"
import { AppShellChrome } from "./app-shell/chrome"
import { appShellSections, type AppShellTopbarAction } from "./app-shell/config"
import { AppShellWorkbenchContent } from "./app-shell/views"
import { useNavigation, useWorkbenchTabSet } from "./app-state-context"
import { getWorkbenchTabIcon } from "./workbench-tab-registry"
import { WORKBENCH_PRIMARY_TAB } from "./workbench-tab-set"
import { APP_MENU_EVENT_NAME, type AppMenuEventDetail } from "~/shared/app-menu"
import { DEBUG_MENU_EVENT_NAME, type DebugMenuEventDetail } from "~/shared/debug-menu"
import type { SvgIconName } from "./svg-icon"

/** Tracks the shell tab strip's active-tab underline and drag state. */
function useAppShellTabStrip(
  activeTabId: string,
  selectedNavigationId: string,
  tabs: readonly { id: string }[],
) {
  const tabStripRef = useRef<HTMLDivElement | null>(null)
  const tabRefs = useRef<Record<string, HTMLDivElement | null>>({})
  const dragSourceTabId = useRef<string | null>(null)
  const [indicator, setIndicator] = useState({ left: 0, width: 0, opacity: 0 })

  function syncIndicator() {
    const activeTabElement = tabRefs.current[activeTabId]
    const tabStripElement = tabStripRef.current

    if (!activeTabElement || !tabStripElement) {
      setIndicator((current) =>
        current.opacity === 0 ? current : { left: current.left, width: current.width, opacity: 0 },
      )
      return
    }

    setIndicator({
      left: activeTabElement.offsetLeft,
      width: activeTabElement.offsetWidth,
      opacity: 1,
    })
  }

  useLayoutEffect(() => {
    syncIndicator()
  }, [activeTabId, selectedNavigationId, tabs])

  useEffect(() => {
    const tabStripElement = tabStripRef.current

    if (!tabStripElement) {
      return
    }

    const observer = new ResizeObserver(() => {
      syncIndicator()
    })

    observer.observe(tabStripElement)

    for (const tabElement of Object.values(tabRefs.current)) {
      if (tabElement) {
        observer.observe(tabElement)
      }
    }

    return () => {
      observer.disconnect()
    }
  }, [selectedNavigationId, tabs])

  return {
    dragSourceTabId,
    indicator,
    tabRefs,
    tabStripRef,
  }
}

/** Renders the app shell by wiring shared app state into the presentational shell chrome. */
export function AppShell() {
  const navigation = useNavigation()
  const workbenchTabSet = useWorkbenchTabSet()
  const tabStrip = useAppShellTabStrip(
    workbenchTabSet.activeTabId,
    navigation.selectedNavId,
    workbenchTabSet.tabList,
  )
  const navigationItems: Array<{
    ariaLabel: string
    badgeCount?: number
    group: "primary" | "secondary"
    icon: SvgIconName
    id: (typeof navigation.items)[number]["id"]
    label: string
  }> = navigation.items.map((item) => {
    const section = appShellSections.find((candidate) => candidate.tabKinds.includes(item.id))

    if (!section) {
      throw new Error(`Missing app shell section for navigation item ${item.id}.`)
    }

    return {
      ...item,
      badgeCount: navigation.badgeCounts[item.id],
      group: section.group,
      icon: getWorkbenchTabIcon(item.id),
    }
  })
  const selectedNavigation =
    navigationItems.find((item) => item.id === navigation.selectedNavId) ?? navigationItems[0]

  useListener(window, APP_MENU_EVENT_NAME, (event) => {
    const detail = (event as CustomEvent<AppMenuEventDetail>).detail

    if (detail.action === "closeTab" && workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id) {
      workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
    }
  })

  useListener(window, DEBUG_MENU_EVENT_NAME, (event) => {
    const detail = (event as CustomEvent<DebugMenuEventDetail>).detail

    if (detail.surface !== "sessionChatTranscript") {
      return
    }

    workbenchTabSet.openOrFocusTab({
      id: "debug:session-chat-transcript",
      kind: "sessionChatTranscriptDebug",
      title: "Transcript Debug",
      payload: {
        surface: "sessionChatTranscript",
      },
      dirty: false,
    })
  })

  function handleTopbarAction(action: AppShellTopbarAction) {
    if (action === "proposeTask") {
      // TODO: Wire this button to the real propose-task flow once that surface exists.
      return
    }

    if (action === "newSession") {
      // TODO: Wire this button to the real new-session flow once that surface exists.
      return
    }

    // TODO: Wire this button to the real settings/preferences surface once it exists.
  }

  return (
    <AppShellChrome
      activeTabId={workbenchTabSet.activeTabId}
      indicator={tabStrip.indicator}
      navigationItems={navigationItems}
      onAction={handleTopbarAction}
      onNavigationSelect={(id, options) => {
        if (options?.openInTab) {
          const nextNavigationItem =
            navigation.items.find((item) => item.id === id) ?? navigation.selectedItem
          workbenchTabSet.openOrFocusTab({
            id: `surface:${id}`,
            kind: id,
            title: nextNavigationItem.label,
            payload: {},
            dirty: false,
          })
          return
        }

        navigation.selectNavItem(id)
        workbenchTabSet.activateTab(WORKBENCH_PRIMARY_TAB.id)
      }}
      onTabClose={(id) => {
        workbenchTabSet.closeTab(id)
      }}
      onTabDragEnd={() => {
        tabStrip.dragSourceTabId.current = null
      }}
      onTabDragEnter={(id) => {
        if (tabStrip.dragSourceTabId.current && tabStrip.dragSourceTabId.current !== id) {
          workbenchTabSet.reorderTabs(tabStrip.dragSourceTabId.current, id)
        }
      }}
      onTabDragStart={(id) => {
        tabStrip.dragSourceTabId.current = id
      }}
      onTabSelect={(id) => {
        workbenchTabSet.activateTab(id)
      }}
      selectedNavigationId={navigation.selectedNavId}
      selectedNavigationLabel={selectedNavigation?.label ?? WORKBENCH_PRIMARY_TAB.title}
      setTabRef={(id, element) => {
        tabStrip.tabRefs.current[id] = element
      }}
      tabStripRef={tabStrip.tabStripRef}
      tabs={workbenchTabSet.tabList}
    >
      <AppShellWorkbenchContent
        activeTabId={workbenchTabSet.activeTabId}
        selectedNavId={navigation.selectedNavId}
      />
    </AppShellChrome>
  )
}
