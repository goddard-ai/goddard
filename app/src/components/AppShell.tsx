import { css } from "../../styled-system/css"
import { useNavigation, useProjectRegistry, useWorkbenchTabSet } from "../state/app-context"
import { lookupProject } from "../state/project-registry"
import { WORKBENCH_DETAIL_TAB_LIMIT, WORKBENCH_PRIMARY_TAB } from "../state/workbench-tabs-state"
import { ProjectsPage } from "./ProjectsPage"
import { SidebarNav } from "./SidebarNav"
import { WorkbenchTabs } from "./WorkbenchTabs"

const placeholderPageClass = css({
  display: "grid",
  placeItems: "center",
  height: "100%",
  padding: "32px",
  backgroundColor: "background",
})

const placeholderCardClass = css({
  width: "min(640px, 100%)",
  padding: "36px",
  borderRadius: "24px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "panel",
  boxShadow: "0 20px 56px rgba(133, 149, 170, 0.08)",
})

const placeholderTitleClass = css({
  marginBottom: "14px",
  color: "text",
  fontSize: "1.5rem",
  fontWeight: "700",
  letterSpacing: "-0.02em",
})

const placeholderBodyClass = css({
  color: "muted",
  lineHeight: "1.65",
})

const detailBodyClass = css({
  color: "muted",
  lineHeight: "1.7",
})

/** Renders the sprint-1 tab-first shell and its primary workbench view. */
export function AppShell() {
  const navigation = useNavigation()
  const workbenchTabSet = useWorkbenchTabSet()
  const navigationItems = navigation.items.map((item) => ({
    ...item,
    badgeCount: navigation.badgeCounts[item.id],
  }))

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateColumns: "84px minmax(0, 1fr)",
        minHeight: "100vh",
        backgroundColor: "background",
        color: "text",
      })}
    >
      <SidebarNav
        items={navigationItems}
        onSelect={(id) => {
          navigation.selectNavItem(id)
          workbenchTabSet.activateTab(WORKBENCH_PRIMARY_TAB.id)
        }}
        selectedItemId={navigation.selectedNavId}
      />
      <div
        class={css({
          display: "grid",
          gridTemplateRows: "auto minmax(0, 1fr)",
          minWidth: "0",
        })}
      >
        <WorkbenchTabsConnected />
        <div class={css({ minHeight: "0" })}>
          {workbenchTabSet.activeTabId === WORKBENCH_PRIMARY_TAB.id ? (
            <MainWorkbenchView />
          ) : (
            <WorkbenchTabPanel />
          )}
        </div>
      </div>
    </div>
  )
}

/** Connects the shell's tab state to the rendered tab strip. */
function WorkbenchTabsConnected() {
  const workbenchTabSet = useWorkbenchTabSet()

  return (
    <WorkbenchTabs
      activeTabId={workbenchTabSet.activeTabId}
      detailTabLimit={WORKBENCH_DETAIL_TAB_LIMIT}
      detailTabs={workbenchTabSet.detailTabList}
      onClose={(id) => {
        workbenchTabSet.closeTab(id)
      }}
      onReorder={(fromId, toId) => {
        workbenchTabSet.reorderTabs(fromId, toId)
      }}
      onSelect={(id) => {
        workbenchTabSet.activateTab(id)
      }}
      primaryTab={WORKBENCH_PRIMARY_TAB}
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
        <h1 class={placeholderTitleClass}>{navigation.selectedItem.label}</h1>
        <p class={placeholderBodyClass}>
          Sprint 1 establishes the tab-first shell and machine-wide project registry first. This
          primary view is scaffolded so the sidebar and main tab model are already stable before the
          next sprint lands.
        </p>
      </div>
    </div>
  )
}

/** Renders the active sprint-1 detail tab panel. */
function WorkbenchTabPanel() {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const activeDetailTab = workbenchTabSet.activeDetailTab

  if (!activeDetailTab) {
    return (
      <div class={placeholderPageClass}>
        <div class={placeholderCardClass}>
          <h1 class={placeholderTitleClass}>No detail tab selected</h1>
          <p class={placeholderBodyClass}>
            Open one project into a detail tab to reuse the shell flow.
          </p>
        </div>
      </div>
    )
  }

  if (activeDetailTab.kind === "project") {
    const project = lookupProject(projectRegistry, activeDetailTab.payload.projectPath)

    if (!project) {
      return (
        <div class={placeholderPageClass}>
          <div class={placeholderCardClass}>
            <h1 class={placeholderTitleClass}>Project unavailable</h1>
            <p class={placeholderBodyClass}>
              The project record for this tab is no longer in the machine-wide project registry.
            </p>
          </div>
        </div>
      )
    }

    return (
      <div
        class={css({
          display: "flex",
          flexDirection: "column",
          gap: "16px",
          height: "100%",
          padding: "28px",
          backgroundColor: "background",
        })}
      >
        <section
          class={css({
            maxWidth: "880px",
            padding: "28px",
            borderRadius: "24px",
            border: "1px solid",
            borderColor: "border",
            backgroundColor: "panel",
            boxShadow: "0 20px 56px rgba(133, 149, 170, 0.08)",
          })}
        >
          <div
            class={css({
              marginBottom: "10px",
              color: "accentStrong",
              fontSize: "0.72rem",
              fontWeight: "700",
              letterSpacing: "0.14em",
              textTransform: "uppercase",
            })}
          >
            Detail Tab
          </div>
          <h1
            class={css({
              marginBottom: "12px",
              color: "text",
              fontSize: "1.4rem",
              fontWeight: "700",
            })}
          >
            {project.name}
          </h1>
          <p class={detailBodyClass}>
            This project detail tab proves the sprint-1 shell can keep the primary workbench view
            and closable detail tabs alive side by side. Later sprints can replace this summary with
            richer project-scoped surfaces such as sessions, specs, tasks, and pull requests.
          </p>
          <p class={detailBodyClass}>Path: {project.path}</p>
        </section>
      </div>
    )
  }

  return null
}
