import type { ShortcutMatch } from "powerkeys"
import { SigmaTarget, useListener } from "preact-sigma"
import { mapValues } from "radashi"

const appCommandBus = new SigmaTarget<Record<string, ShortcutMatch | undefined>>()

type AppCommandDefinition = {
  id: string
  label: string
  description: string
}

interface AppCommandFunction<Id extends string> extends AppCommandDefinition {
  (match?: ShortcutMatch): void
  id: Id
}

type AppCommands<TCommands extends Record<string, AppCommandDefinition>> = {
  [K in keyof TCommands]: AppCommandFunction<TCommands[K]["id"]>
}

function defineAppCommands<const TCommands extends Record<string, AppCommandDefinition>>(
  commands: TCommands,
): AppCommands<TCommands> {
  return mapValues(commands, (def) => {
    return Object.assign(function (match?: ShortcutMatch) {
      appCommandBus.emit(def.id, match)
    }, def)
  }) as any
}

export const AppCommand = defineAppCommands({
  closeActiveTab: {
    id: "workbench.closeActiveTab",
    label: "Close Active Tab",
    description: "Closes the current closable workbench tab.",
  },
  proposeTask: {
    id: "agent.proposeTask",
    label: "Run Agent: Propose Task",
    description: "Proposes a task to the user.",
  },
  newSession: {
    id: "session.new",
    label: "New Session",
    description: "Opens the new-session dialog from the app shell.",
  },
  openKeyboardShortcuts: {
    id: "workbench.openKeyboardShortcuts",
    label: "Open Keyboard Shortcuts",
    description: "Opens the keyboard shortcut browser in a detail tab.",
  },
  openInbox: {
    id: "navigation.openInbox",
    label: "Open Inbox",
    description: "Selects the Inbox main workbench view.",
  },
  openProjects: {
    id: "navigation.openProjects",
    label: "Open Projects",
    description: "Selects the Projects main workbench view.",
  },
  openSessions: {
    id: "navigation.openSessions",
    label: "Open Sessions",
    description: "Selects the Sessions main workbench view.",
  },
  openSearch: {
    id: "navigation.openSearch",
    label: "Open Search",
    description: "Selects the Search main workbench view.",
  },
  openSpecs: {
    id: "navigation.openSpecs",
    label: "Open Specs",
    description: "Selects the Specs main workbench view.",
  },
  openTasks: {
    id: "navigation.openTasks",
    label: "Open Tasks",
    description: "Selects the Tasks main workbench view.",
  },
  openRoadmap: {
    id: "navigation.openRoadmap",
    label: "Open Roadmap",
    description: "Selects the Roadmap main workbench view.",
  },
  openFolder: {
    id: "project.openFolder",
    label: "Open Folder",
    description: "Browses for a local project and opens the Projects main workbench view.",
  },
  openSettings: {
    id: "app.openSettings",
    label: "Open Settings",
    description: "Opens the settings surface and appearance dialog.",
  },
})

export type AppCommand = (typeof AppCommand)[keyof typeof AppCommand]

export function useAppCommand(command: AppCommand, listener: (match?: ShortcutMatch) => void) {
  useListener(appCommandBus, command.id, listener)
}
