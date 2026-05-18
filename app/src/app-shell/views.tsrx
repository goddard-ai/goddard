/** Workbench content surfaces rendered inside the merged app shell chrome. */
import { css } from "@goddard-ai/styled-system/css"
import { useLayoutEffect, useRef } from "preact/hooks"

import { useMainTab, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { TabContextProvider } from "~/tab-context.tsrx"
import {
  getWorkbenchTabComponent,
  shouldRestoreWorkbenchTabScroll,
} from "~/workbench-tab-registry.ts"
import { WORKBENCH_MAIN_TAB } from "~/workbench-tab-set.ts"

const panelScrollCache = new Map<string, number>()
const tabFailure = {
  root: css({
    display: "grid",
    alignContent: "start",
    gap: "12px",
    minHeight: "100%",
    padding: "20px",
    color: "muted",
  }),
  content: css({
    display: "grid",
    gap: "10px",
    maxWidth: "28rem",
  }),
  title: css({
    color: "text",
    fontSize: "1.15rem",
    fontWeight: "720",
    lineHeight: "1.3",
  }),
  body: css({
    fontSize: "0.93rem",
    lineHeight: "1.6",
  }),
  action: css({
    justifySelf: "start",
    height: "30px",
    paddingInline: "12px",
    border: "1px solid {colors.border}",
    borderRadius: "7px",
    backgroundColor: "panel",
    color: "text",
    cursor: "pointer",
    fontSize: "0.86rem",
    fontWeight: "640",
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "2px",
    },
    _hover: {
      backgroundColor: "surface",
    },
  }),
}

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
    class={css({
      minHeight: "0",
      height: "100%",
      overflowY: "auto",
      overscrollBehavior: "contain",
    })}
    onScroll={(event) => {
      panelScrollCache.set(props.scrollKey, event.currentTarget.scrollTop)
    }}
  >
    <div
      class={css({
        height: "100%",
        minHeight: "100%",
      })}
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
  <div class={tabFailure.root}>
    <div class={tabFailure.content}>
      <h2 class={tabFailure.title}>{props.title}</h2>
      <p class={tabFailure.body}>{props.description}</p>
      <button class={tabFailure.action} type="button" onClick={props.onRetry}>"Try again"</button>
    </div>
  </div>
}
