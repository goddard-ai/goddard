import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"
import { useListener } from "preact-sigma"
import { AppShellChrome } from "./AppShellChrome"
import { createPrimaryWorkbenchTab, navigationById, handleTopbarAction } from "./AppShell.config"
import type { SvgIconName } from "../support/svg-icon"
import { AppShellWorkbenchContent } from "./AppShellViews"
import { useNavigation, useWorkbenchTabSet } from "./state/AppStateContext"
import { WORKBENCH_PRIMARY_TAB } from "./state/WorkbenchTabSet"
import { APP_MENU_EVENT_NAME, type AppMenuEventDetail } from "../shared/app-menu"
import { DEBUG_MENU_EVENT_NAME, type DebugMenuEventDetail } from "../shared/debug-menu"

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
    const navigationConfig = navigationById[item.id]

    return {
      ...item,
      badgeCount: navigation.badgeCounts[item.id],
      group: navigationConfig.group,
      icon: navigationConfig.icon,
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
      icon: "sessions",
      payload: {
        surface: "sessionChatTranscript",
      },
      dirty: false,
    })
  })

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
          const tab = createPrimaryWorkbenchTab(
            id,
            nextNavigationItem.label,
            nextNavigationItem.icon,
          )

          if (tab) {
            workbenchTabSet.openOrFocusTab(tab)
            return
          }
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
