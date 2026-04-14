import {
  FolderKanban,
  FolderOpen,
  Inbox,
  Keyboard,
  ListTodo,
  Route,
  Rows3,
  ScrollText,
} from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { AppearanceDialog } from "~/appearance/appearance-dialog.tsx"
import { AppCommand, resolveAppCommand, useAppCommand } from "~/commands/app-command.ts"
import { browseForProject } from "~/desktop-host.ts"
import type { NavigationItemId } from "~/navigation.ts"
import { lookupProject } from "~/projects/project-registry.ts"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { AppShellChrome } from "./app-shell/chrome.tsx"
import { appShellSections } from "./app-shell/config.ts"
import { AppShellWorkbenchContent } from "./app-shell/views.tsx"
import {
  useNavigation,
  useProjectRegistry,
  useShortcutRegistry,
  useWorkbenchTabSet,
} from "./app-state-context.tsx"
import { CommandMenu } from "./command-menu.tsx"
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
  const projectRegistry = useProjectRegistry()
  const shortcutRegistry = useShortcutRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const [isCommandMenuOpen, setIsCommandMenuOpen] = useState(false)
  const [isAppearanceDialogOpen, setIsAppearanceDialogOpen] = useState(false)
  const tabStrip = useAppShellTabStrip(
    workbenchTabSet.activeTabId,
    navigation.selectedNavId,
    workbenchTabSet.tabList,
  )

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

  useEffect(() => {
    shortcutRegistry.syncWorkbenchContext({
      activeTabKind,
      hasClosableActiveTab: workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id,
      selectedNavId: navigation.selectedNavId,
    })
  }, [activeTabKind, navigation.selectedNavId, shortcutRegistry, workbenchTabSet.activeTabId])

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

  const commandMenuItems = [
    {
      id: "open-folder",
      group: "Actions",
      icon: FolderOpen,
      keywords: ["browse", "directory", "project", "add"],
      label: "Open folder",
      onSelect: AppCommand.projects.openFolder,
    },
    {
      id: "view-projects",
      group: "Views",
      icon: FolderKanban,
      keywords: ["navigation", "projects"],
      label: "View projects",
      onSelect: AppCommand.navigation.openProjects,
    },
    {
      id: "view-inbox",
      group: "Views",
      icon: Inbox,
      keywords: ["navigation", "inbox"],
      label: "View inbox",
      onSelect: AppCommand.navigation.openInbox,
    },
    {
      id: "view-sessions",
      group: "Views",
      icon: Rows3,
      keywords: ["navigation", "sessions"],
      label: "View sessions",
      onSelect: AppCommand.navigation.openSessions,
    },
    {
      id: "view-specs",
      group: "Views",
      icon: ScrollText,
      keywords: ["navigation", "specs", "documents"],
      label: "View specs",
      onSelect: AppCommand.navigation.openSpecs,
    },
    {
      id: "view-tasks",
      group: "Views",
      icon: ListTodo,
      keywords: ["navigation", "tasks"],
      label: "View tasks",
      onSelect: AppCommand.navigation.openTasks,
    },
    {
      id: "view-roadmap",
      group: "Views",
      icon: Route,
      keywords: ["navigation", "roadmap", "plan"],
      label: "View roadmap",
      onSelect: AppCommand.navigation.openRoadmap,
    },
    {
      id: "view-keyboard-shortcuts",
      group: "Views",
      icon: Keyboard,
      keywords: ["shortcuts", "keyboard", "bindings", "keys"],
      label: "View keyboard shortcuts",
      onSelect: AppCommand.navigation.openKeyboardShortcuts,
    },
  ] as const

  useListener(globalEventHub, "appMenu", ({ command }) => {
    resolveAppCommand(command)()
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

  useAppCommand(AppCommand.closeActiveTab, () => {
    if (workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id) {
      workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
    }
  })

  useAppCommand(AppCommand.openKeyboardShortcuts, () => {
    workbenchTabSet.openOrFocusTab({
      id: "workbench:keyboard-shortcuts",
      kind: "keyboardShortcuts",
      title: "Keyboard Shortcuts",
      payload: {},
      dirty: false,
    })
  })

  useAppCommand(AppCommand.openInbox, () => {
    selectNavigationSurface("inbox")
  })

  useAppCommand(AppCommand.openProjects, () => {
    selectNavigationSurface("projects")
  })

  useAppCommand(AppCommand.openSessions, () => {
    selectNavigationSurface("sessions")
  })

  useAppCommand(AppCommand.openSearch, () => {
    selectNavigationSurface("search")
  })

  useAppCommand(AppCommand.openSpecs, () => {
    selectNavigationSurface("specs")
  })

  useAppCommand(AppCommand.openTasks, () => {
    selectNavigationSurface("tasks")
  })

  useAppCommand(AppCommand.openRoadmap, () => {
    selectNavigationSurface("roadmap")
  })

  useAppCommand(AppCommand.openFolder, async () => {
    const selectedPath = await browseForProject()

    if (!selectedPath) {
      return
    }

    const existingProject = lookupProject(projectRegistry, selectedPath)
    const projectName = existingProject?.name ?? deriveProjectName(selectedPath)

    if (!existingProject) {
      projectRegistry.addProject({
        path: selectedPath,
        name: projectName.length > 0 ? projectName : selectedPath,
      })
    }

    selectNavigationSurface("projects")
  })

  useAppCommand(AppCommand.openSettings, () => {
    workbenchTabSet.openOrFocusTab({
      id: "surface:settings",
      kind: "settings",
      title: "Settings",
      payload: {},
      dirty: false,
    })
    setIsAppearanceDialogOpen(true)
  })

  useListener(window, "keydown", (event) => {
    const keyboardEvent = event as KeyboardEvent

    if (keyboardEvent.defaultPrevented || keyboardEvent.altKey || keyboardEvent.isComposing) {
      return
    }

    if (
      (keyboardEvent.metaKey || keyboardEvent.ctrlKey) &&
      keyboardEvent.key.toLowerCase() === "k"
    ) {
      keyboardEvent.preventDefault()
      setIsCommandMenuOpen((open) => !open)
    }
  })

  return (
    <>
      <CommandMenu
        items={commandMenuItems}
        open={isCommandMenuOpen}
        onOpenChange={setIsCommandMenuOpen}
      />
      <AppShellChrome
        activeTabId={workbenchTabSet.activeTabId}
        indicator={tabStrip.indicator}
        navigationItems={navigationItems}
        onCommandMenuOpen={() => {
          setIsCommandMenuOpen(true)
        }}
        onNavigationSelect={selectNavigationSurface}
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
        command={AppCommand.newSession}
        content={() => import("~/sessions/dialog.tsx")}
      />
      <AppearanceDialog
        isOpen={isAppearanceDialogOpen}
        onOpenChange={(isOpen) => {
          setIsAppearanceDialogOpen(isOpen)
        }}
      />
    </>
  )
}
