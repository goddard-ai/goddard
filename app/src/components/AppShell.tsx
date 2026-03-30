import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { ProjectsPage } from "./Projects/ProjectsPage"
import { lookupProject } from "./Projects/state/ProjectRegistry"
import { SidebarNav } from "./SidebarNav"
import { ShellIcon } from "../support/shell-icons"
import { useNavigation, useProjectRegistry, useWorkbenchTabSet } from "./state/AppStateContext"
import { WORKBENCH_DETAIL_TAB_LIMIT, WORKBENCH_PRIMARY_TAB } from "./state/WorkbenchTabSet"
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

const detailBodyClass = css({
  color: "muted",
  lineHeight: "1.72",
})

/** Renders the tab-first shell and its primary workbench view. */
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
        position: "relative",
        display: "grid",
        gridTemplateColumns: "92px minmax(0, 1fr)",
        minHeight: "100vh",
        background:
          `radial-gradient(circle at top left, color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent), transparent 28%), ` +
          `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
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
          minHeight: "100vh",
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
  const navigation = useNavigation()
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

/** Renders the active detail tab panel. */
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
          gap: "18px",
          height: "100%",
          padding: "28px",
          background:
            `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 32%), ` +
            `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
        })}
      >
        <section
          class={css({
            maxWidth: "880px",
            padding: "32px",
            borderRadius: "28px",
            border: "1px solid",
            borderColor: "border",
            background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
            boxShadow: "0 28px 80px rgba(121, 138, 160, 0.14)",
          })}
        >
          <div
            class={css({
              display: "inline-flex",
              alignItems: "center",
              gap: "8px",
              padding: "8px 12px",
              borderRadius: "999px",
              backgroundColor: "surface",
              marginBottom: "10px",
              color: "accentStrong",
              fontSize: "0.72rem",
              fontWeight: "700",
              letterSpacing: "0.14em",
              textTransform: "uppercase",
            })}
          >
            <span class={css({ width: "14px", height: "14px" })}>
              <ShellIcon name="projects" />
            </span>
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
            This project detail tab keeps the primary workbench view and closable detail tabs alive
            side by side. It can later be replaced with richer project-scoped surfaces such as
            sessions, specs, tasks, and pull requests.
          </p>
          <p class={detailBodyClass}>Path: {project.path}</p>
        </section>
      </div>
    )
  }

  return null
}
