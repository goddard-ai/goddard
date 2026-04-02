/** Workbench content surfaces rendered inside the merged app shell chrome. */
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import type { ComponentChildren } from "preact"
import { Suspense } from "preact/compat"
import { useLayoutEffect, useRef, useState } from "preact/hooks"
import { SvgIcon } from "../support/svg-icon"
import { navigationById } from "./AppShell.config"
import { ProjectsPage } from "./Projects/ProjectsPage"
import { TabViewportProvider } from "./TabViewport"
import { useNavigation, useWorkbenchTabSet } from "./state/AppStateContext"
import { getWorkbenchTabComponent } from "./state/WorkbenchTabRegistry"
import { WORKBENCH_PRIMARY_TAB } from "./state/WorkbenchTabSet"

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
export function AppShellWorkbenchContent(props: { activeTabId: string; selectedNavId: string }) {
  return props.activeTabId === WORKBENCH_PRIMARY_TAB.id ? (
    <WorkbenchScrollPanel scrollKey={`main:${props.selectedNavId}`}>
      <MainWorkbenchView />
    </WorkbenchScrollPanel>
  ) : (
    <WorkbenchScrollPanel scrollKey={`detail:${props.activeTabId}`}>
      <WorkbenchTabPanel />
    </WorkbenchScrollPanel>
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
            <SvgIcon
              name={navigationById[navigation.selectedItem.id].icon}
              height="20px"
              width="20px"
            />
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
