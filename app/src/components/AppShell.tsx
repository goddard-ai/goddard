import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import type { ComponentChildren } from "preact"
import { Suspense } from "preact/compat"
import { useLayoutEffect, useRef, useState } from "preact/hooks"
import { useListener } from "preact-sigma"
import { ProjectsPage } from "./Projects/ProjectsPage"
import { SidebarNav } from "./SidebarNav"
import { TabViewportProvider } from "./TabViewport"
import { ShellIcon } from "../support/shell-icons"
import { APP_MENU_EVENT_NAME, type AppMenuEventDetail } from "../shared/app-menu"
import { DEBUG_MENU_EVENT_NAME, type DebugMenuEventDetail } from "../shared/debug-menu"
import type { NavigationItemId } from "./state/Navigation"
import { useNavigation, useWorkbenchTabSet } from "./state/AppStateContext"
import { getWorkbenchTabComponent, type WorkbenchTab } from "./state/WorkbenchTabRegistry"
import { WORKBENCH_PRIMARY_TAB } from "./state/WorkbenchTabSet"
import { WorkbenchTabs } from "./WorkbenchTabs"

const placeholderPageClass = css({
  display: "grid",
  placeItems: "center",
  height: "100%",
  padding: "32px",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent), transparent 34%), ` +
    `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
})

const placeholderCardClass = css({
  width: "min(680px, 100%)",
  padding: "40px",
  borderRadius: "28px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
  boxShadow: "0 28px 80px rgba(121, 138, 160, 0.14)",
})

const placeholderTitleClass = css({
  marginBottom: "12px",
  color: "text",
  fontSize: "1.7rem",
  fontWeight: "750",
  letterSpacing: "-0.02em",
})

const placeholderBodyClass = css({
  color: "muted",
  lineHeight: "1.72",
  maxWidth: "50ch",
})

const windowDragRegionClass = css({
  height: "36px",
  minHeight: "36px",
  borderBottom: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.panel")} 100%)`,
})

/** Creates one stable closable tab for a primary navigation surface when supported. */
function createPrimaryWorkbenchTab(
  navId: NavigationItemId,
  title: string,
  icon: WorkbenchTab<"projects">["icon"],
): WorkbenchTab<"projects"> | null {
  if (navId === "projects") {
    return {
      id: "primary:projects",
      kind: "projects",
      title,
      icon,
      payload: {},
      dirty: false,
    }
  }

  return null
}

const workbenchPanelScrollerClass = css({
  minHeight: "0",
  height: "100%",
  overflowY: "auto",
  overscrollBehavior: "contain",
})

const workbenchPanelBodyClass = css({
  minHeight: "100%",
})

const panelScrollCache = new Map<string, number>()

/** Renders the tab-first shell and its primary workbench view. */
export function AppShell() {
  const navigation = useNavigation()
  const workbenchTabSet = useWorkbenchTabSet()
  const navigationItems = navigation.items.map((item) => ({
    ...item,
    badgeCount: navigation.badgeCounts[item.id],
  }))

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
    <div
      class={css({
        position: "relative",
        display: "grid",
        gridTemplateColumns: "92px minmax(0, 1fr)",
        gridTemplateRows: "36px minmax(0, 1fr)",
        width: "100%",
        height: "100vh",
        maxHeight: "100vh",
        overflow: "hidden",
        background:
          `radial-gradient(circle at top left, color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent), transparent 28%), ` +
          `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
        color: "text",
      })}
    >
      <div class={`${windowDragRegionClass} electrobun-webkit-app-region-drag`} />
      <div class={`${windowDragRegionClass} electrobun-webkit-app-region-drag`} />
      <SidebarNav
        className={css({ gridRow: "2", minHeight: "0" })}
        items={navigationItems}
        onSelect={(id, options) => {
          if (options?.openInTab) {
            const tab = createPrimaryWorkbenchTab(
              id,
              navigation.items.find((item) => item.id === id)?.label ??
                navigation.selectedItem.label,
              navigation.items.find((item) => item.id === id)?.icon ?? navigation.selectedItem.icon,
            )

            if (tab) {
              workbenchTabSet.openOrFocusTab(tab)
              return
            }
          }

          navigation.selectNavItem(id)
          workbenchTabSet.activateTab(WORKBENCH_PRIMARY_TAB.id)
        }}
        selectedItemId={navigation.selectedNavId}
      />
      <div
        class={css({
          gridRow: "2",
          display: "grid",
          gridTemplateRows: "auto minmax(0, 1fr)",
          minWidth: "0",
          minHeight: "0",
          overflow: "hidden",
        })}
      >
        <WorkbenchTabsConnected />
        <div class={css({ minHeight: "0", overflow: "hidden" })}>
          {workbenchTabSet.activeTabId === WORKBENCH_PRIMARY_TAB.id ? (
            <WorkbenchScrollPanel scrollKey={`main:${navigation.selectedNavId}`}>
              <MainWorkbenchView />
            </WorkbenchScrollPanel>
          ) : (
            <WorkbenchScrollPanel scrollKey={`detail:${workbenchTabSet.activeTabId}`}>
              <WorkbenchTabPanel />
            </WorkbenchScrollPanel>
          )}
        </div>
      </div>
    </div>
  )
}

/** Owns one tab panel scroller and restores its previous offset when the panel remounts. */
function WorkbenchScrollPanel(props: { scrollKey: string; children: ComponentChildren }) {
  const scrollerRef = useRef<HTMLDivElement | null>(null)
  const [scrollTop, setScrollTop] = useState(panelScrollCache.get(props.scrollKey) ?? 0)

  useLayoutEffect(() => {
    const scrollerElement = scrollerRef.current

    if (!scrollerElement) {
      return
    }

    const restoredScrollTop = panelScrollCache.get(props.scrollKey) ?? 0
    scrollerElement.scrollTop = restoredScrollTop
    setScrollTop(restoredScrollTop)
  }, [props.scrollKey])

  return (
    <TabViewportProvider scrollTop={scrollTop} viewportRef={scrollerRef}>
      <div
        ref={scrollerRef}
        class={workbenchPanelScrollerClass}
        onScroll={(event) => {
          const nextScrollTop = event.currentTarget.scrollTop
          panelScrollCache.set(props.scrollKey, nextScrollTop)
          setScrollTop(nextScrollTop)
        }}
      >
        <div class={workbenchPanelBodyClass}>{props.children}</div>
      </div>
    </TabViewportProvider>
  )
}

/** Connects the shell's tab state to the rendered tab strip. */
function WorkbenchTabsConnected() {
  const navigation = useNavigation()
  const workbenchTabSet = useWorkbenchTabSet()

  return (
    <WorkbenchTabs
      activeTabId={workbenchTabSet.activeTabId}
      tabs={workbenchTabSet.tabList}
      onClose={(id) => {
        workbenchTabSet.closeTab(id)
      }}
      onReorder={(fromId, toId) => {
        workbenchTabSet.reorderTabs(fromId, toId)
      }}
      onSelect={(id) => {
        workbenchTabSet.activateTab(id)
      }}
      primaryTab={{
        ...WORKBENCH_PRIMARY_TAB,
        title: navigation.selectedItem.label,
        icon: navigation.selectedItem.icon,
      }}
    />
  )
}

/** Renders the non-closable main workbench view for the currently selected primary domain. */
function MainWorkbenchView() {
  const navigation = useNavigation()

  if (navigation.selectedNavId === "projects") {
    return <ProjectsPage />
  }

  return (
    <div class={placeholderPageClass}>
      <div class={placeholderCardClass}>
        <div
          class={css({
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            width: "48px",
            height: "48px",
            marginBottom: "20px",
            borderRadius: "16px",
            backgroundColor: "surface",
            color: "accentStrong",
            boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
          })}
        >
          <span class={css({ width: "20px", height: "20px" })}>
            <ShellIcon name={navigation.selectedItem.icon} />
          </span>
        </div>
        <h1 class={placeholderTitleClass}>{navigation.selectedItem.label}</h1>
        <p class={placeholderBodyClass}>
          This primary view is scaffolded so the sidebar and main tab model stay stable while the
          rest of the workbench surfaces are filled in.
        </p>
      </div>
    </div>
  )
}

/** Renders the active closable workbench tab panel. */
function WorkbenchTabPanel() {
  const workbenchTabSet = useWorkbenchTabSet()
  const activeTab = workbenchTabSet.activeTab

  if (!activeTab) {
    return (
      <div class={placeholderPageClass}>
        <div class={placeholderCardClass}>
          <h1 class={placeholderTitleClass}>No tab selected</h1>
          <p class={placeholderBodyClass}>Open one project into a tab to reuse the shell flow.</p>
        </div>
      </div>
    )
  }

  const ActiveTabComponent = getWorkbenchTabComponent(activeTab.kind)

  return (
    <Suspense
      fallback={
        <div class={placeholderPageClass}>
          <div class={placeholderCardClass}>
            <h1 class={placeholderTitleClass}>Loading tab</h1>
            <p class={placeholderBodyClass}>The selected detail surface is being prepared.</p>
          </div>
        </div>
      }
    >
      <ActiveTabComponent {...activeTab.payload} />
    </Suspense>
  )
}
