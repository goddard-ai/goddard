/** Workbench content surfaces rendered inside the merged app shell chrome. */
import { css } from "@goddard-ai/styled-system/css"
import { Suspense } from "preact/compat"
import { useLayoutEffect, useRef, useState } from "preact/hooks"

import { useNavigation, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { TabViewportProvider } from "~/tab-viewport.tsx"
import { getWorkbenchTabComponent } from "~/workbench-tab-registry.ts"
import { WORKBENCH_PRIMARY_TAB } from "~/workbench-tab-set.ts"

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

/** Renders the shell's main content area for the current primary view or detail tab. */
export function AppShellWorkbenchContent(props: {
  activeTabId: string
  onRequestSessionLaunch: (preferredProjectPath?: string | null) => void
  selectedNavId: string
}) {
  return (
    <Suspense fallback={<div />}>
      {props.activeTabId === WORKBENCH_PRIMARY_TAB.id ? (
        <WorkbenchScrollPanel scrollKey={`main:${props.selectedNavId}`}>
          <MainWorkbenchView onRequestSessionLaunch={props.onRequestSessionLaunch} />
        </WorkbenchScrollPanel>
      ) : (
        <WorkbenchScrollPanel scrollKey={`detail:${props.activeTabId}`}>
          <WorkbenchTabPanel onRequestSessionLaunch={props.onRequestSessionLaunch} />
        </WorkbenchScrollPanel>
      )}
    </Suspense>
  )
}

/** Owns one tab panel scroller and restores its previous offset when the panel remounts. */
function WorkbenchScrollPanel(props: { scrollKey: string; children: preact.ComponentChildren }) {
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

/** Renders the non-closable main workbench view for the currently selected primary domain. */
function MainWorkbenchView(props: {
  onRequestSessionLaunch: (preferredProjectPath?: string | null) => void
}) {
  const navigation = useNavigation()
  const ActiveNavigationComponent = getWorkbenchTabComponent(navigation.selectedNavId)

  return <ActiveNavigationComponent onRequestSessionLaunch={props.onRequestSessionLaunch} />
}

/** Renders the active closable workbench tab panel. */
function WorkbenchTabPanel(props: {
  onRequestSessionLaunch: (preferredProjectPath?: string | null) => void
}) {
  const workbenchTabSet = useWorkbenchTabSet()
  const activeTab = workbenchTabSet.activeTab

  if (!activeTab) {
    return <div />
  }

  const ActiveTabComponent = getWorkbenchTabComponent(activeTab.kind)
  const activeTabPayload = activeTab.payload as Record<string, unknown>

  return (
    <ActiveTabComponent
      {...activeTabPayload}
      onRequestSessionLaunch={props.onRequestSessionLaunch}
    />
  )
}
