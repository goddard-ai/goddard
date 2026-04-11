import { useSignal } from "@preact/signals"
import { FolderOpen, FolderKanban, Inbox, ListTodo, Route, Rows3, ScrollText } from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import { AppearanceDialog } from "~/appearance/appearance-dialog.tsx"
import { browseForProject } from "~/desktop-host.ts"
import type { NavigationItemId } from "~/navigation.ts"
import { lookupProject } from "~/projects/project-registry.ts"
import { SessionLaunchDialog } from "~/sessions/dialog.tsx"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { AppShellChrome } from "./app-shell/chrome.tsx"
import { appShellSections, type AppShellTopbarAction } from "./app-shell/config.ts"
import { AppShellWorkbenchContent } from "./app-shell/views.tsx"
import { useNavigation, useProjectRegistry, useWorkbenchTabSet } from "./app-state-context.tsx"
import { CommandMenu } from "./command-menu.tsx"
import type { SvgIconName } from "./lib/good-icon.tsx"
import { useQuery } from "./lib/query.ts"
import { deriveProjectName } from "./projects/project-name.ts"
import { goddardSdk } from "./sdk.ts"
import { buildCreateSessionInput } from "./sessions/session-launch.ts"
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

function useSessionDialogState() {
  const isDialogOpen = useSignal(false)
  const draftAdapterId = useSignal<string | null>(null)
  const draftProjectPath = useSignal<string | null>(null)
  const draftPrompt = useSignal("")
  const adapterCatalog = useQuery(goddardSdk.adapter.list, [
    { cwd: draftProjectPath.value ?? undefined },
  ])

  useEffect(() => {
    const availableAdapterIds = new Set(adapterCatalog.adapters.map((adapter) => adapter.id))
    const nextAdapterId =
      draftAdapterId.value && availableAdapterIds.has(draftAdapterId.value)
        ? draftAdapterId.value
        : adapterCatalog.defaultAdapterId &&
            availableAdapterIds.has(adapterCatalog.defaultAdapterId)
          ? adapterCatalog.defaultAdapterId
          : (adapterCatalog.adapters[0]?.id ?? null)

    if (draftAdapterId.value !== nextAdapterId) {
      draftAdapterId.value = nextAdapterId
    }
  }, [
    adapterCatalog.adapters,
    adapterCatalog.defaultAdapterId,
    draftAdapterId.value,
    draftProjectPath.value,
  ])

  function openDialog(preferredProjectPath?: string | null) {
    isDialogOpen.value = true
    draftAdapterId.value = null
    draftProjectPath.value = preferredProjectPath ?? null
    draftPrompt.value = ""
  }

  function closeDialog() {
    isDialogOpen.value = false
    draftAdapterId.value = null
    draftProjectPath.value = null
    draftPrompt.value = ""
  }

  function setDraftAdapterId(adapterId: string | null) {
    draftAdapterId.value = adapterId
  }

  function setDraftProjectPath(projectPath: string | null) {
    draftProjectPath.value = projectPath
  }

  function setDraftPrompt(prompt: string) {
    draftPrompt.value = prompt
  }

  function createSessionInput() {
    return buildCreateSessionInput(draftProjectPath.value, draftAdapterId.value, draftPrompt.value)
  }

  function canSubmit() {
    return createSessionInput() !== null
  }

  return {
    adapters: adapterCatalog.adapters,
    canSubmit,
    closeDialog,
    createSessionInput,
    draftAdapterId,
    draftProjectPath,
    draftPrompt,
    isDialogOpen,
    openDialog,
    setDraftAdapterId,
    setDraftProjectPath,
    setDraftPrompt,
  }
}

export function AppShell() {
  const navigation = useNavigation()
  const projectRegistry = useProjectRegistry()
  const sessionDialog = useSessionDialogState()
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
  ] as const

  useListener(globalEventHub, "appMenu", (detail) => {
    switch (detail.action) {
      case "closeTab":
        if (workbenchTabSet.activeTabId !== WORKBENCH_PRIMARY_TAB.id) {
          workbenchTabSet.closeTab(workbenchTabSet.activeTabId)
        }
        return
    }
  })

  useListener(globalEventHub, "debugMenu", (detail) => {
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

  function handleTopbarAction(action: AppShellTopbarAction) {
    if (action === "proposeTask") {
      return
    }

    if (action === "newSession") {
      sessionDialog.openDialog(projectRegistry.projectList[0]?.path ?? null)
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
      return
    }
  }

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
        onAction={handleTopbarAction}
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
          onRequestSessionLaunch={sessionDialog.openDialog}
          selectedNavId={navigation.selectedNavId}
        />
      </AppShellChrome>
      <SessionLaunchDialog
        adapters={sessionDialog.adapters}
        canSubmit={sessionDialog.canSubmit()}
        createSessionInput={sessionDialog.createSessionInput}
        draftAdapterId={sessionDialog.draftAdapterId.value}
        draftProjectPath={sessionDialog.draftProjectPath.value}
        draftPrompt={sessionDialog.draftPrompt.value}
        isDialogOpen={sessionDialog.isDialogOpen.value}
        onChangeAdapterId={sessionDialog.setDraftAdapterId}
        onChangeProjectPath={sessionDialog.setDraftProjectPath}
        onChangePrompt={sessionDialog.setDraftPrompt}
        onClose={sessionDialog.closeDialog}
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
