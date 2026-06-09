/** Workbench content surfaces rendered inside the merged app shell chrome. */
import { useLayoutEffect, useRef } from "preact/hooks"
import { useListener } from "preact-sigma"

import { useMainTab, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { TabContextProvider } from "~/tab-context.tsrx"
import {
  getWorkbenchTabComponent,
  shouldRestoreWorkbenchTabScroll,
} from "~/workbench-tab-registry.ts"
import { WORKBENCH_MAIN_TAB } from "~/workbench-tab-set.ts"
import { findActivationFocusTarget, findSearchFocusTarget } from "./focus-targets.ts"
import styles from "./views.style.ts"

const panelScrollCache = new Map<string, number>()

function isFocusSearchKeyboardEvent(event: KeyboardEvent) {
  return event.key.toLowerCase() === "f" && (event.metaKey || event.ctrlKey) && !event.altKey &&
    !event.shiftKey
}

/** Owns one tab panel scroller and restores its previous offset when the panel remounts. */
export component WorkbenchScrollPanel(props: {
  scrollKey: string;
  children: preact.ComponentChildren;
  restoreScroll?: boolean;
  tabId?: string;
}) {
  const scrollerRef = useRef<HTMLDivElement | null>(null)
  const lastFocusedScrollKeyRef = useRef(props.scrollKey)
  const shouldRestoreScroll = props.restoreScroll ?? true

  useListener(document, "keydown", (event) => {
    const scrollerElement = scrollerRef.current

    if (!scrollerElement || !isFocusSearchKeyboardEvent(event)) {
      return
    }

    const searchTarget = findSearchFocusTarget(scrollerElement)

    if (searchTarget) {
      searchTarget.focus()
      event.preventDefault()
    }
  })

  useLayoutEffect(() => {
    const scrollerElement = scrollerRef.current

    if (!scrollerElement) {
      return
    }

    if (shouldRestoreScroll) {
      const restoredScrollTop = panelScrollCache.get(props.scrollKey) ?? 0
      scrollerElement.scrollTop = restoredScrollTop
    }

    if (lastFocusedScrollKeyRef.current !== props.scrollKey) {
      lastFocusedScrollKeyRef.current = props.scrollKey
      const focusActivationTarget = () => {
        const target = findActivationFocusTarget(scrollerElement)

        if (!target) {
          return false
        }

        target.focus({ preventScroll: true })
        return true
      }

      if (focusActivationTarget()) {
        return
      }

      scrollerElement.focus({ preventScroll: true })

      const observer = new MutationObserver(() => {
        if (document.activeElement !== scrollerElement) {
          observer.disconnect()
          return
        }

        if (focusActivationTarget()) {
          observer.disconnect()
        }
      })

      observer.observe(scrollerElement, {
        attributes: true,
        childList: true,
        subtree: true,
      })

      const timeout = window.setTimeout(() => {
        observer.disconnect()
      }, 250)

      return () => {
        window.clearTimeout(timeout)
        observer.disconnect()
      }
    }
  }, [props.scrollKey, shouldRestoreScroll])

  <div
    ref={scrollerRef}
    class={styles.scrollPanel}
    tabIndex={-1}
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
export component MainWorkbenchView() {
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
export component WorkbenchTabPanel() {
  const workbenchTabSet = useWorkbenchTabSet()
  const activeTab = workbenchTabSet.activeClosableTab

  if (!activeTab) {
    return
  }

  const ActiveTabComponent = getWorkbenchTabComponent(activeTab.kind)

  try {
    <ActiveTabComponent {...activeTab.props} />
  } catch (error, retry) {
    console.error("Workbench tab render failed.", error)

    <WorkbenchTabFailure
      description="This tab failed to render. You can retry it or close the tab from the tab strip."
      onRetry={retry}
      title="Tab unavailable"
    />
  }
}

function WorkbenchTabFailure(props: {
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
