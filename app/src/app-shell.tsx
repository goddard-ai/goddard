import { useListener } from "preact-sigma"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { AppCommand, resolveAppCommand, useAppCommand } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { browseForProject } from "~/desktop-host.ts"
import type { NavigationItemId } from "~/navigation.ts"
import {
  findNearestProjectPath,
  orderProjectsByRecentActivity,
} from "~/projects/project-context.ts"
import { lookupProject } from "~/projects/project-registry.ts"
import { SwitchProjectDropdown } from "~/projects/switch-project-dropdown.tsx"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { AppShellChrome } from "./app-shell/chrome.tsx"
import { appShellSections } from "./app-shell/config.ts"
import { AppShellWorkbenchContent } from "./app-shell/views.tsx"
import {
  useNavigation,
  useProjectContext,
  useProjectRegistry,
  useWorkbenchTabSet,
} from "./app-state-context.tsx"
import { CommandDialog } from "./commands/command-dialog.tsx"
import type { SvgIconName } from "./lib/good-icon.tsx"
import { deriveProjectName } from "./projects/project-name.ts"
import { getWorkbenchTabIcon } from "./workbench-tab-registry.ts"
import { WORKBENCH_PRIMARY_TAB } from "./workbench-tab-set.ts"

function useAppShellTabStrip(
  activeTabId: string,
  selectedNavigationId: string,
  tabs: readonly { id: string }[],
) {
  const tabStripRef = useRef<HTMLDivElement | null>(null)
  const tabRefs = useRef<Record<string, HTMLDivElement | null>>({})
  const dragSourceTabId = useRef<string | null>(null)
  const [indicator, setIndicator] = useState({ left: 0, width: 0, opacity: 0 })

  function syncIndicator() {
    const activeTabElement = tabRefs.current[activeTabId]
    const tabStripElement = tabStripRef.current

    if (!activeTabElement || !tabStripElement) {
      setIndicator((current) =>
        current.opacity === 0 ? current : { left: current.left, width: current.width, opacity: 0 },
      )
      return
    }

    setIndicator({
      left: activeTabElement.offsetLeft,
      width: activeTabElement.offsetWidth,
      opacity: 1,
    })
  }

  useLayoutEffect(() => {
    syncIndicator()
  }, [activeTabId, selectedNavigationId, tabs])

  useEffect(() => {
    const tabStripElement = tabStripRef.current

    if (!tabStripElement) {
      return
    }

    const observer = new ResizeObserver(() => {
      syncIndicator()
    })

    observer.observe(tabStripElement)

    for (const tabElement of Object.values(tabRefs.current)) {
      if (tabElement) {
        observer.observe(tabElement)
      }
    }

    return () => {
      observer.disconnect()
    }
  }, [selectedNavigationId, tabs])

  return {
    dragSourceTabId,
    indicator,
    tabRefs,
    tabStripRef,
  }
}

export function AppShell() {
  const navigation = useNavigation()
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const [isProjectSwitchOpen, setIsProjectSwitchOpen] = useState(false)
  const projectSwitchContainerRef = useRef<HTMLDivElement | null>(null)
  const tabStrip = useAppShellTabStrip(
    workbenchTabSet.activeTabId,
    navigation.selectedNavId,
    workbenchTabSet.tabList,
  )
  const projects = projectRegistry.projectList

  const navigationItems: Array<{
    group: "primary" | "secondary"
    icon: SvgIconName
    id: (typeof navigation.items)[number]["id"]
    label: string
  }> = navigation.items.map((item) => {
    const section = appShellSections.find((candidate) => candidate.tabKinds.includes(item.id))

    if (!section) {
      throw new Error(`Missing app shell section for navigation item ${item.id}.`)
    }

    return {
      ...item,
      group: section.group,
      icon: getWorkbenchTabIcon(item.id),
    }
  })

  const selectedNavigation =
    navigationItems.find((item) => item.id === navigation.selectedNavId) ?? navigationItems[0]
  const activeTabKind = workbenchTabSet.activeTab?.kind ?? WORKBENCH_PRIMARY_TAB.kind
  const activeProject =
    projectContext.activeProjectPath === null
      ? null
      : lookupProject(projectRegistry, projectContext.activeProjectPath)
  const orderedProjects = orderProjectsByRecentActivity(projects, projectContext.recentProjectPaths)

  useEffect(() => {
    commandContext.activeTabKind.value = activeTabKind
    commandContext.hasClosableActiveTab.value =
      workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id
    commandContext.selectedNavId.value = navigation.selectedNavId
  }, [activeTabKind, navigation.selectedNavId, workbenchTabSet.activeTabId])

  useEffect(() => {
    projectContext.syncProjects(projects.map((project) => project.path))
  }, [projectContext, projects])

  useEffect(() => {
    if (workbenchTabSet.activeTabId === WORKBENCH_PRIMARY_TAB.id) {
      projectContext.applyFocusedTabProject(WORKBENCH_PRIMARY_TAB.id, null)
      return
    }

    const activeTab = workbenchTabSet.activeTab

    if (!activeTab) {
      projectContext.applyFocusedTabProject(WORKBENCH_PRIMARY_TAB.id, null)
      return
    }

    switch (activeTab.kind) {
      case "project":
        projectContext.applyFocusedTabProject(
          activeTab.id,
          findNearestProjectPath(
            projects,
            (activeTab.payload as { projectPath?: string | null }).projectPath ?? null,
          ),
        )
        return
      case "sessionChat":
        projectContext.applyFocusedTabProject(
          activeTab.id,
          findNearestProjectPath(
            projects,
            (activeTab.payload as { projectPath?: string | null }).projectPath ?? null,
          ),
        )
        return
      default:
        projectContext.applyFocusedTabProject(activeTab.id, null)
    }
  }, [
    projectContext,
    projects,
    workbenchTabSet.activeTab,
    workbenchTabSet.activeTabId,
  ])

  function openNavigationSurfaceTab(kind: NavigationItemId) {
    const nextNavigationItem =
      navigation.items.find((item) => item.id === kind) ?? navigation.selectedItem

    workbenchTabSet.openOrFocusTab({
      id: `surface:${kind}`,
      kind,
      title: nextNavigationItem.label,
      payload: {},
      dirty: false,
    })
  }

  function selectNavigationSurface(id: NavigationItemId, options?: { openInTab?: boolean }) {
    if (options?.openInTab) {
      openNavigationSurfaceTab(id)
      return
    }

    navigation.selectNavItem(id)
    workbenchTabSet.activateTab(WORKBENCH_PRIMARY_TAB.id)
  }

  function openProjectTab(projectPath: string) {
    const project = lookupProject(projectRegistry, projectPath)

    if (!project) {
      return
    }

    projectContext.setActiveProject(project.path)
    workbenchTabSet.openOrFocusTab({
      id: `project:${encodeURIComponent(project.path)}`,
      kind: "project",
      title: project.name,
      payload: { projectPath: project.path },
      dirty: false,
    })
  }

  async function openProjectFromFilesystem() {
    setIsProjectSwitchOpen(false)

    const selectedPath = await browseForProject()

    if (!selectedPath) {
      return
    }

    const existingProject = lookupProject(projectRegistry, selectedPath)
    const projectName = existingProject?.name ?? deriveProjectName(selectedPath)
    const nextProject = existingProject ?? {
      path: selectedPath,
      name: projectName.length > 0 ? projectName : selectedPath,
    }

    if (!existingProject) {
      projectRegistry.addProject(nextProject)
    }

    projectContext.setActiveProject(nextProject.path)
    workbenchTabSet.openOrFocusTab({
      id: `project:${encodeURIComponent(nextProject.path)}`,
      kind: "project",
      title: nextProject.name,
      payload: { projectPath: nextProject.path },
      dirty: false,
    })
  }

  useListener(globalEventHub, "appMenu", ({ command }) => {
    resolveAppCommand(command)?.()
  })

  useListener(globalEventHub, "debugMenu", ({ surface }) => {
    switch (surface) {
      case "SessionChatTranscript":
        workbenchTabSet.openOrFocusTab({
          id: "debug:session-chat-transcript",
          kind: "sessionChatTranscriptDebug",
          title: "Transcript Debug",
          payload: {},
          dirty: false,
        })
        return
      case "Terminal":
        workbenchTabSet.openOrFocusTab({
          id: "debug:terminal",
          kind: "terminalDebug",
          title: "Terminal Debug",
          payload: {},
          dirty: false,
        })
        return
    }
  })

  useAppCommand(AppCommand.workbench.closeActiveTab, () => {
    if (workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id) {
      workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
    }
  })

  useAppCommand(AppCommand.navigation.openKeyboardShortcuts, () => {
    workbenchTabSet.openOrFocusTab({
      id: "workbench:keyboard-shortcuts",
      kind: "keyboardShortcuts",
      title: "Keyboard Shortcuts",
      payload: {},
      dirty: false,
    })
  })

  useAppCommand(AppCommand.navigation.openSwitchProject, () => {
    setIsProjectSwitchOpen(true)
  })

  useAppCommand(AppCommand.navigation.openInbox, () => {
    selectNavigationSurface("inbox")
  })

  useAppCommand(AppCommand.navigation.openProjects, () => {
    selectNavigationSurface("projects")
  })

  useAppCommand(AppCommand.navigation.openSessions, () => {
    selectNavigationSurface("sessions")
  })

  useAppCommand(AppCommand.navigation.openSearch, () => {
    selectNavigationSurface("search")
  })

  useAppCommand(AppCommand.navigation.openSpecs, () => {
    selectNavigationSurface("specs")
  })

  useAppCommand(AppCommand.navigation.openTasks, () => {
    selectNavigationSurface("tasks")
  })

  useAppCommand(AppCommand.navigation.openRoadmap, () => {
    selectNavigationSurface("roadmap")
  })

  useAppCommand(AppCommand.projects.openFolder, async () => {
    await openProjectFromFilesystem()
  })

  useAppCommand(AppCommand.navigation.openSettings, () => {
    workbenchTabSet.openOrFocusTab({
      id: "surface:settings",
      kind: "settings",
      title: "Settings",
      payload: {},
      dirty: false,
    })
  })

  return (
    <>
      <CommandDialog
        command={AppCommand.navigation.openCommandMenu}
        content={() => import("~/command-palette.tsx")}
      />
      <AppShellChrome
        activeTabId={workbenchTabSet.activeTabId}
        activeProjectLabel={activeProject?.name ?? "Open project"}
        indicator={tabStrip.indicator}
        isProjectSwitchOpen={isProjectSwitchOpen}
        navigationItems={navigationItems}
        onNavigationSelect={selectNavigationSurface}
        onProjectSwitchToggle={() => {
          setIsProjectSwitchOpen((current) => !current)
        }}
        onTabClose={(id) => {
          workbenchTabSet.closeTab(id)
        }}
        onTabDragEnd={() => {
          tabStrip.dragSourceTabId.current = null
        }}
        onTabDragEnter={(id) => {
          if (tabStrip.dragSourceTabId.current && tabStrip.dragSourceTabId.current !== id) {
            workbenchTabSet.reorderTabs(tabStrip.dragSourceTabId.current, id)
          }
        }}
        onTabDragStart={(id) => {
          tabStrip.dragSourceTabId.current = id
        }}
        onTabSelect={(id) => {
          workbenchTabSet.activateTab(id)
        }}
        projectSwitcher={
          <SwitchProjectDropdown
            activeProjectPath={activeProject?.path ?? null}
            containerRef={projectSwitchContainerRef}
            isOpen={isProjectSwitchOpen}
            projects={orderedProjects}
            onOpenChange={(isOpen) => {
              setIsProjectSwitchOpen(isOpen)
            }}
            onOpenFolder={async () => {
              await openProjectFromFilesystem()
            }}
            onSelectProject={(projectPath) => {
              openProjectTab(projectPath)
            }}
          />
        }
        projectSwitchContainerRef={projectSwitchContainerRef}
        selectedNavigationId={navigation.selectedNavId}
        selectedNavigationLabel={selectedNavigation?.label ?? WORKBENCH_PRIMARY_TAB.title}
        setTabRef={(id, element) => {
          tabStrip.tabRefs.current[id] = element
        }}
        tabStripRef={tabStrip.tabStripRef}
        tabs={workbenchTabSet.tabList}
      >
        <AppShellWorkbenchContent
          activeTabId={workbenchTabSet.activeTabId}
          selectedNavId={navigation.selectedNavId}
        />
      </AppShellChrome>
      <CommandDialog
        command={AppCommand.navigation.openNewSessionDialog}
        content={() => import("~/sessions/dialog.tsx")}
      />
    </>
  )
}
