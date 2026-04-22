/** Workbench content surfaces rendered inside the merged app shell chrome. */
import { css } from "@goddard-ai/styled-system/css"
import { Suspense } from "preact/compat"
import { useLayoutEffect, useState } from "preact/hooks"

import { useNavigation, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { TabViewportProvider } from "~/tab-viewport.tsrx"
import { getWorkbenchTabComponent } from "~/workbench-tab-registry.ts"
import { WORKBENCH_PRIMARY_TAB } from "~/workbench-tab-set.ts"

const panelScrollCache = new Map<string, number>()

/** Renders the shell's main content area for the current primary view or detail tab. */
export component AppShellWorkbenchContent() {
  const navigation = useNavigation()
  const workbenchTabSet = useWorkbenchTabSet()

  <Suspense fallback={null}>
    if (workbenchTabSet.activeTabId === WORKBENCH_PRIMARY_TAB.id) {
      <WorkbenchScrollPanel scrollKey={`main:${navigation.selectedNavId}`}>
        <MainWorkbenchView />
      </WorkbenchScrollPanel>
    } else {
      <WorkbenchScrollPanel scrollKey={`detail:${workbenchTabSet.activeTabId}`}>
        <WorkbenchTabPanel />
      </WorkbenchScrollPanel>
    }
  </Suspense>
}

/** Owns one tab panel scroller and restores its previous offset when the panel remounts. */
component WorkbenchScrollPanel(props: {
  scrollKey: string;
  children: preact.ComponentChildren;
}) {
  let scrollerElement: HTMLDivElement | null = null
  const [scrollTop, setScrollTop] = useState(panelScrollCache.get(props.scrollKey) ?? 0)
  const [viewportElement, setViewportElement] = useState<HTMLDivElement | null>(null)

  useLayoutEffect(() => {
    setViewportElement(
      (currentElement) => currentElement === scrollerElement ? currentElement : scrollerElement,
    )
  })

  useLayoutEffect(() => {
    if (!viewportElement) {
      return
    }

    const restoredScrollTop = panelScrollCache.get(props.scrollKey) ?? 0
    viewportElement.scrollTop = restoredScrollTop
    setScrollTop(restoredScrollTop)
  }, [props.scrollKey, viewportElement])

  <TabViewportProvider {scrollTop} {viewportElement}>
    <div
      {ref scrollerElement}
      class={css(
        {
          minHeight: "0",
          height: "100%",
          overflowY: "auto",
          overscrollBehavior: "contain",
        },
      )}
      onScroll={(event) => {
        const nextScrollTop = event.currentTarget.scrollTop
        panelScrollCache.set(props.scrollKey, nextScrollTop)
        setScrollTop(nextScrollTop)
      }}
    >
      <div
        class={css(
          {
            height: "100%",
            minHeight: "100%",
          },
        )}
      >
        {props.children}
      </div>
    </div>
  </TabViewportProvider>
}

/** Renders the non-closable main workbench view for the currently selected primary domain. */
component MainWorkbenchView() {
  const navigation = useNavigation()
  const ActiveNavigationComponent = getWorkbenchTabComponent(navigation.selectedNavId)

  <ActiveNavigationComponent />
}

/** Renders the active closable workbench tab panel. */
component WorkbenchTabPanel() {
  const workbenchTabSet = useWorkbenchTabSet()
  const activeTab = workbenchTabSet.activeTab

  if (!activeTab) {
    return
  }

  const ActiveTabComponent = getWorkbenchTabComponent(activeTab.kind)
  const activeTabPayload = activeTab.payload as Record<string, unknown>

  <ActiveTabComponent />
}
