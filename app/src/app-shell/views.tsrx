/** Workbench content surfaces rendered inside the merged app shell chrome. */
import { css } from "@goddard-ai/styled-system/css"
import { useLayoutEffect, useRef } from "preact/hooks"

import { useNavigation, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { TabContextProvider } from "~/tab-context.tsrx"
import {
  getWorkbenchTabComponent,
  shouldRestoreWorkbenchTabScroll,
} from "~/workbench-tab-registry.ts"
import { WORKBENCH_PRIMARY_TAB } from "~/workbench-tab-set.ts"

const panelScrollCache = new Map<string, number>()

/** Renders the shell's main content area for the current primary view or detail tab. */
export component AppShellWorkbenchContent() {
  const navigation = useNavigation()
  const workbenchTabSet = useWorkbenchTabSet()

  try {
    if (workbenchTabSet.activeTabId === WORKBENCH_PRIMARY_TAB.id) {
      <WorkbenchScrollPanel scrollKey={`main:${navigation.selectedNavId}`}>
        <MainWorkbenchView />
      </WorkbenchScrollPanel>
    } else if (workbenchTabSet.activeTab) {
      <WorkbenchScrollPanel
        restoreScroll={shouldRestoreWorkbenchTabScroll(workbenchTabSet.activeTab.kind)}
        scrollKey={`detail:${workbenchTabSet.activeTab.id}`}
        tabId={workbenchTabSet.activeTab.id}
      >
        <WorkbenchTabPanel />
      </WorkbenchScrollPanel>
    }
  } pending {
    <></>
  }
}

/** Owns one tab panel scroller and restores its previous offset when the panel remounts. */
component WorkbenchScrollPanel(props: {
  scrollKey: string;
  children: preact.ComponentChildren;
  restoreScroll?: boolean;
  tabId?: string;
}) {
  const scrollerRef = useRef<HTMLDivElement | null>(null)
  const shouldRestoreScroll = props.restoreScroll ?? true

  useLayoutEffect(() => {
    if (!shouldRestoreScroll) {
      return
    }

    const scrollerElement = scrollerRef.current

    if (!scrollerElement) {
      return
    }

    const restoredScrollTop = panelScrollCache.get(props.scrollKey) ?? 0
    scrollerElement.scrollTop = restoredScrollTop
  }, [props.scrollKey, shouldRestoreScroll])

  <div
    ref={scrollerRef}
    class={css(
      {
        minHeight: "0",
        height: "100%",
        overflowY: "auto",
        overscrollBehavior: "contain",
      },
    )}
    onScroll={(event) => {
      panelScrollCache.set(props.scrollKey, event.currentTarget.scrollTop)
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
      if (props.tabId) {
        <TabContextProvider id={props.tabId} scrollParentRef={scrollerRef}>
          {props.children}
        </TabContextProvider>
      } else {
        {props.children}
      }
    </div>
  </div>
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

  <ActiveTabComponent {...activeTab.payload} />
}
