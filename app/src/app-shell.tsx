import { useListener } from "preact-sigma"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"
import { APP_MENU_EVENT_NAME, type AppMenuEventDetail } from "~/shared/app-menu.ts"
import { DEBUG_MENU_EVENT_NAME, type DebugMenuEventDetail } from "~/shared/debug-menu.ts"
import { Dialog as SessionDialog } from "~/sessions/dialog.tsx"
import { AppShellChrome } from "./app-shell/chrome.tsx"
import { appShellSections, type AppShellTopbarAction } from "./app-shell/config.ts"
import { AppShellWorkbenchContent } from "./app-shell/views.tsx"
import {
  useNavigation,
  useProjectRegistry,
  useSessionLaunch,
  useWorkbenchTabSet,
} from "./app-state-context.tsx"
import type { SvgIconName } from "./lib/good-icon.tsx"
import { GoodTooltipProvider } from "./lib/good-tooltip.tsx"
import { getWorkbenchTabIcon } from "./workbench-tab-registry.ts"
import { WORKBENCH_PRIMARY_TAB } from "./workbench-tab-set.ts"

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

export function AppShell() {
  const navigation = useNavigation()
  const projectRegistry = useProjectRegistry()
  const sessionLaunch = useSessionLaunch()
  const workbenchTabSet = useWorkbenchTabSet()
  const tabStrip = useAppShellTabStrip(
    workbenchTabSet.activeTabId,
    navigation.selectedNavId,
    workbenchTabSet.tabList,
  )
  const navigationItems: Array<{
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

  useListener(window, DEBUG_MENU_EVENT_NAME, (event: CustomEvent<DebugMenuEventDetail>) => {
    switch (event.detail.surface) {
      case "SessionChatTranscript":
        workbenchTabSet.openOrFocusTab({
          id: "debug:session-chat-transcript",
          kind: "sessionChatTranscriptDebug",
          title: "Transcript Debug",
          payload: {},
          dirty: false,
        })
        break
      default:
        throw new Error(`Unknown debug surface: ${event.detail.surface}.`)
    }
  })

  function handleTopbarAction(action: AppShellTopbarAction) {
    if (action === "proposeTask") {
      return
    }

    if (action === "newSession") {
      sessionLaunch.openDialog(projectRegistry.projectList[0]?.path ?? null)
      return
    }
  }

  return (
    <GoodTooltipProvider>
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
      <SessionDialog />
    </GoodTooltipProvider>
  )
}
