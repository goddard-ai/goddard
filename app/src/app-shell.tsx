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
import { Suspense } from "preact/compat"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { AppearanceDialog } from "~/appearance/appearance-dialog.tsx"
import { browseForProject } from "~/desktop-host.ts"
import type { NavigationItemId } from "~/navigation.ts"
import { lookupProject } from "~/projects/project-registry.ts"
import { SessionLaunchDialog } from "~/sessions/dialog.tsx"
import type { AppMenuEventDetail } from "~/shared/app-menu.ts"
import type { DebugMenuEventDetail } from "~/shared/debug-menu.ts"
import { globalEventHub, requestSessionLaunchDialog } from "~/shared/global-event-hub.ts"
import { ShortcutCommands } from "~/shared/shortcut-keymap.ts"
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

  async function handleOpenFolderCommand() {
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
  }

  const commandMenuItems = [
    {
      id: "open-folder",
      group: "Actions",
      icon: FolderOpen,
      keywords: ["browse", "directory", "project", "add"],
      label: "Open folder",
      onSelect: handleOpenFolderCommand,
    },
    {
      id: "view-projects",
      group: "Views",
      icon: FolderKanban,
      keywords: ["navigation", "projects"],
      label: "View projects",
      onSelect: () => {
        selectNavigationSurface("projects")
      },
    },
    {
      id: "view-inbox",
      group: "Views",
      icon: Inbox,
      keywords: ["navigation", "inbox"],
      label: "View inbox",
      onSelect: () => {
        selectNavigationSurface("inbox")
      },
    },
    {
      id: "view-sessions",
      group: "Views",
      icon: Rows3,
      keywords: ["navigation", "sessions"],
      label: "View sessions",
      onSelect: () => {
        selectNavigationSurface("sessions")
      },
    },
    {
      id: "view-specs",
      group: "Views",
      icon: ScrollText,
      keywords: ["navigation", "specs", "documents"],
      label: "View specs",
      onSelect: () => {
        selectNavigationSurface("specs")
      },
    },
    {
      id: "view-tasks",
      group: "Views",
      icon: ListTodo,
      keywords: ["navigation", "tasks"],
      label: "View tasks",
      onSelect: () => {
        selectNavigationSurface("tasks")
      },
    },
    {
      id: "view-roadmap",
      group: "Views",
      icon: Route,
      keywords: ["navigation", "roadmap", "plan"],
      label: "View roadmap",
      onSelect: () => {
        selectNavigationSurface("roadmap")
      },
    },
    {
      id: "view-keyboard-shortcuts",
      group: "Views",
      icon: Keyboard,
      keywords: ["shortcuts", "keyboard", "bindings", "keys"],
      label: "View keyboard shortcuts",
      onSelect: () => {
        shortcutRegistry.dispatch(ShortcutCommands.openKeyboardShortcuts, {
          source: "programmatic",
        })
      },
    },
  ] as const

  useListener(globalEventHub, "appMenu", (detail: AppMenuEventDetail) => {
    shortcutRegistry.dispatch(detail.action, { source: "native-menu" })
  })

  useListener(globalEventHub, "debugMenu", (detail: DebugMenuEventDetail) => {
    switch (detail.surface) {
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

  useListener(shortcutRegistry, ShortcutCommands.closeActiveTab, () => {
    if (workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id) {
      workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
    }
  })

  useListener(shortcutRegistry, ShortcutCommands.newSession, () => {
    requestSessionLaunchDialog(projectRegistry.projectList[0]?.path ?? null)
  })

  useListener(shortcutRegistry, ShortcutCommands.openKeyboardShortcuts, () => {
    workbenchTabSet.openOrFocusTab({
      id: "workbench:keyboard-shortcuts",
      kind: "keyboardShortcuts",
      title: "Keyboard Shortcuts",
      payload: {},
      dirty: false,
    })
  })

  useListener(shortcutRegistry, ShortcutCommands.openInbox, () => {
    selectNavigationSurface("inbox")
  })

  useListener(shortcutRegistry, ShortcutCommands.openSessions, () => {
    selectNavigationSurface("sessions")
  })

  useListener(shortcutRegistry, ShortcutCommands.openSearch, () => {
    selectNavigationSurface("search")
  })

  useListener(shortcutRegistry, ShortcutCommands.openSpecs, () => {
    selectNavigationSurface("specs")
  })

  useListener(shortcutRegistry, ShortcutCommands.openTasks, () => {
    selectNavigationSurface("tasks")
  })

  useListener(shortcutRegistry, ShortcutCommands.openRoadmap, () => {
    selectNavigationSurface("roadmap")
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
        onAction={(action) => {
          if (action === "proposeTask") {
            throw new Error("proposeTask action is not implemented.")
          }

          if (action === "newSession") {
            shortcutRegistry.dispatch(ShortcutCommands.newSession, { source: "programmatic" })
            return
          }

          if (action === "settings") {
            workbenchTabSet.openOrFocusTab({
              id: "surface:settings",
              kind: "settings",
              title: "Settings",
              payload: {},
              dirty: false,
            })
            setIsAppearanceDialogOpen(true)
          }
        }}
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
      <Suspense fallback={<div />}>
        <SessionLaunchDialog />
      </Suspense>
      <AppearanceDialog
        isOpen={isAppearanceDialogOpen}
        onOpenChange={(isOpen) => {
          setIsAppearanceDialogOpen(isOpen)
        }}
      />
    </>
  )
}
