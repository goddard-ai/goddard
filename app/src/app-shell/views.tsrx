/** Workbench content surfaces rendered inside the merged app shell chrome. */
import { useLayoutEffect, useRef } from "preact/hooks"

import { useMainTab, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { TabContextProvider } from "~/tab-context.tsrx"
import {
  getWorkbenchTabComponent,
  shouldRestoreWorkbenchTabScroll,
} from "~/workbench-tab-registry.ts"
import { WORKBENCH_MAIN_TAB } from "~/workbench-tab-set.ts"
import styles from "./views.style.ts"

const panelScrollCache = new Map<string, number>()

/** Renders the shell's main content area for the current primary view or detail tab. */
export component AppShellWorkbenchContent() {
  const mainTab = useMainTab()
  const workbenchTabSet = useWorkbenchTabSet()

  try {
    if (workbenchTabSet.activeTabId === WORKBENCH_MAIN_TAB.id) {
      <WorkbenchScrollPanel scrollKey={`main:${mainTab.selectedKind}`}>
        <MainWorkbenchView />
      </WorkbenchScrollPanel>
    } else if (workbenchTabSet.activeClosableTab) {
      <WorkbenchScrollPanel
        restoreScroll={shouldRestoreWorkbenchTabScroll(workbenchTabSet.activeClosableTab.kind)}
        scrollKey={`detail:${workbenchTabSet.activeClosableTab.id}`}
        tabId={workbenchTabSet.activeClosableTab.id}
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
    class={styles.scrollPanel}
    onScroll={(event) => {
      panelScrollCache.set(props.scrollKey, event.currentTarget.scrollTop)
    }}
  >
    <div class={styles.scrollPanelInner}>
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
  const mainTab = useMainTab()
  const ActiveMainTabComponent = getWorkbenchTabComponent(mainTab.selectedKind)

  try {
    <ActiveMainTabComponent />
  } catch (error, retry) {
    console.error("Main workbench view render failed.", error)
    <WorkbenchTabFailure
      description="This primary view failed to render. The rest of the app is still available."
      onRetry={retry}
      title="View unavailable"
    />
  }
}

/** Renders the active closable workbench tab panel. */
component WorkbenchTabPanel() {
  const workbenchTabSet = useWorkbenchTabSet()
  const activeTab = workbenchTabSet.activeClosableTab

  if (!activeTab) {
    return
  }

  const ActiveTabComponent = getWorkbenchTabComponent(activeTab.kind)

  try {
    <ActiveTabComponent {...activeTab.payload} />
  } catch (error, retry) {
    console.error("Workbench tab render failed.", error)
    <WorkbenchTabFailure
      description="This tab failed to render. You can retry it or close the tab from the tab strip."
      onRetry={retry}
      title="Tab unavailable"
    />
  }
}

component WorkbenchTabFailure(props: {
  description: string;
  onRetry: () => void;
  title: string;
}) {
  <div class={styles.tabFailureRoot}>
    <div class={styles.tabFailureContent}>
      <h2 class={styles.tabFailureTitle}>{props.title}</h2>
      <p class={styles.tabFailureBody}>{props.description}</p>
      <button class={styles.tabFailureAction} type="button" onClick={props.onRetry}>
        "Try again"
      </button>
    </div>
  </div>
}
