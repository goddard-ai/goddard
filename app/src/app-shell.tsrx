import { useListener } from "preact-sigma"
import { AppCommand, resolveAppCommand, useAppCommand } from "~/commands/app-command.ts"
import { GoodToaster } from "~/lib/good-toaster.tsrx"
import { openProjectFromFilesystem } from "~/projects/mutations.ts"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { AppShellChrome } from "./app-shell/chrome.tsrx"
import { AppShellOnboarding } from "./app-shell/onboarding.tsrx"
import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "./app-state-context.tsrx"
import { CommandDialog } from "./commands/command-dialog.tsrx"
import { SessionLaunchLeaseKeeper } from "./sessions/launch-lease-keeper.tsrx"

export component AppShell() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const projects = projectRegistry.projectList

  useListener(globalEventHub, "appMenu", ({ command }) => {
    resolveAppCommand(command)?.()
  })

  useListener(globalEventHub, "debugMenu", ({ surface }) => {
    switch (surface) {
      case "Inbox":
        workbenchTabSet.openOrFocusTab({
          kind: "inboxDebug",
          props: {},
        })
        return
      case "Terminal":
        workbenchTabSet.openOrFocusTab({
          kind: "terminalDebug",
          props: {},
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
    try {
      <SessionLaunchLeaseKeeper
        currentProjectPath={projectContext.activeProjectPath ?? projects[0]?.path ?? null}
      />
    } pending {}

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
