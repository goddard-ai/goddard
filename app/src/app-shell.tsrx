import { useListener } from "preact-sigma"
import { useEffect } from "preact/hooks"

import { AppCommand, resolveAppCommand, useAppCommand } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { GoodToaster } from "~/lib/good-toaster.tsrx"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { openProjectFromFilesystem } from "~/projects/mutations.ts"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { AppShellChrome } from "./app-shell/chrome.tsrx"
import { AppShellOnboarding } from "./app-shell/onboarding.tsrx"
import {
  useMainTab,
  useProjectContext,
  useProjectRegistry,
  useWorkbenchTabSet,
} from "./app-state-context.tsrx"
import { CommandDialog } from "./commands/command-dialog.tsrx"
import { WORKBENCH_MAIN_TAB } from "./workbench-tab-set.ts"

export component AppShell() {
  const mainTab = useMainTab()
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const projects = projectRegistry.projectList
  const activeTabKind = workbenchTabSet.activeClosableTab?.kind ?? mainTab.selectedKind

  useEffect(() => {
    commandContext.activeTabKind.value = activeTabKind
    commandContext.hasClosableActiveTab.value = workbenchTabSet.activeTabId !==
    WORKBENCH_MAIN_TAB.id
    commandContext.selectedKind.value = mainTab.selectedKind
  }, [activeTabKind, mainTab.selectedKind, workbenchTabSet.activeTabId])

  useEffect(() => {
    projectContext.syncProjects(projects.map((project) => project.path))
  }, [projectContext, projects])

  useEffect(() => {
    if (workbenchTabSet.activeTabId === WORKBENCH_MAIN_TAB.id) {
      projectContext.applyFocusedTabProject(WORKBENCH_MAIN_TAB.id, null)
      return
    }

    const activeTab = workbenchTabSet.activeClosableTab

    if (!activeTab) {
      projectContext.applyFocusedTabProject(WORKBENCH_MAIN_TAB.id, null)
      return
    }

    switch (activeTab.kind) {
      case "project":
        projectContext.applyFocusedTabProject(
          activeTab.id,
          findNearestProjectPath(projects, activeTab.payload.projectPath),
        )
        return
      case "sessionChat":
        projectContext.applyFocusedTabProject(
          activeTab.id,
          findNearestProjectPath(projects, activeTab.payload.projectPath),
        )
        return
      default:
        projectContext.applyFocusedTabProject(activeTab.id, null)
    }
  }, [
    projectContext,
    projects,
    workbenchTabSet.activeClosableTab,
    workbenchTabSet.activeTabId,
  ])

  useListener(globalEventHub, "appMenu", ({ command }) => {
    resolveAppCommand(command)?.()
  })

  useListener(globalEventHub, "debugMenu", ({ surface }) => {
    switch (surface) {
      case "Inbox":
        workbenchTabSet.openOrFocusTab({
          id: "debug:inbox",
          kind: "inboxDebug",
          title: "Inbox Debug",
          payload: {},
          dirty: false,
        })
        return
      case "SessionChatTranscript":
        workbenchTabSet.openOrFocusTab({
          id: "debug:session-chat-transcript",
          kind: "sessionChatTranscriptDebug",
          title: "Session Chat Debug",
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

  useAppCommand(AppCommand.projects.openFolder, async () => {
    await openProjectFromFilesystem({
      projectContext,
      projectRegistry,
      workbenchTabSet,
    })
  })

  if (projects.length === 0) {
    <AppShellOnboarding />
  } else {
    <CommandDialog
      command={AppCommand.navigation.openCommandPalette}
      content={() => import("~/command-palette.tsrx")}
    />
    <AppShellChrome />
    <CommandDialog
      command={AppCommand.navigation.openNewSessionDialog}
      content={() => import("~/sessions/dialog.tsrx")}
    />
  }

  <GoodToaster />
}
