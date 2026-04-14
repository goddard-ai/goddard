import type { ShortcutMatch } from "powerkeys"
import { SigmaTarget, useListener } from "preact-sigma"
import { mapValues } from "radashi"

import type { AppCommandId } from "~/shared/app-commands.ts"

const appCommandBus = new SigmaTarget<Record<string, ShortcutMatch | undefined>>()

type AppCommandDefinition = {
  label: string
  description: string
}

type AppCommandTable = {
  [namespace: string]: { [commandId: string]: AppCommandDefinition }
}

export interface AppCommandFunction<Id extends string> extends AppCommandDefinition {
  (match?: ShortcutMatch): void
  id: Id
}

type AppCommands<T extends AppCommandTable> = {
  [TNamespace in keyof T & string]: {
    [TName in keyof T[TNamespace] & string]: AppCommandFunction<`${TNamespace}.${TName}`>
  }
}

function defineAppCommands<const TCommands extends AppCommandTable>(
  table: TCommands,
): AppCommands<TCommands> {
  return mapValues(table, (namespace, namespaceKey) => {
    return mapValues(namespace, (command, commandKey) => {
      const id = `${namespaceKey as string}.${commandKey as string}`
      return Object.assign(
        function (match?: ShortcutMatch) {
          appCommandBus.emit(id, match)
        },
        command,
        { id },
      )
    })
  }) as any
}

export const AppCommand = defineAppCommands({
  workbench: {
    closeActiveTab: {
      label: "Close Active Tab",
      description: "Closes the current closable workbench tab.",
    },
  },
  agents: {
    proposeTask: {
      label: "Run Agent: Propose Task",
      description: "Proposes a task to the user.",
    },
    newSession: {
      label: "New Session",
      description: "Opens the new-session dialog from the app shell.",
    },
  },
  navigation: {
    openKeyboardShortcuts: {
      label: "Open Keyboard Shortcuts",
      description: "Opens the keyboard shortcut browser in a detail tab.",
    },
    openInbox: {
      label: "Open Inbox",
      description: "Selects the Inbox main workbench view.",
    },
    openProjects: {
      label: "Open Projects",
      description: "Selects the Projects main workbench view.",
    },
    openSessions: {
      label: "Open Sessions",
      description: "Selects the Sessions main workbench view.",
    },
    openSearch: {
      label: "Open Search",
      description: "Selects the Search main workbench view.",
    },
    openSpecs: {
      label: "Open Specs",
      description: "Selects the Specs main workbench view.",
    },
    openTasks: {
      label: "Open Tasks",
      description: "Selects the Tasks main workbench view.",
    },
    openRoadmap: {
      label: "Open Roadmap",
      description: "Selects the Roadmap main workbench view.",
    },
    openSettings: {
      label: "Open Settings",
      description: "Opens the settings surface and appearance dialog.",
    },
  },
  projects: {
    openFolder: {
      label: "Open Folder",
      description: "Browses for a local project and opens the Projects main workbench view.",
    },
  },
})

export type AppCommand = (typeof AppCommand)[keyof typeof AppCommand] extends infer TNamespace
  ? TNamespace extends object
    ? TNamespace[keyof TNamespace]
    : never
  : never

export function useAppCommand(command: AppCommand, listener: (match?: ShortcutMatch) => void) {
  useListener(appCommandBus, command.id, listener)
}

export function resolveAppCommand(id: AppCommandId): AppCommand {
  const [namespaceKey, commandKey] = id.split(".")
  const command = (AppCommand as any)[namespaceKey][commandKey]
  if (!command) {
    throw new Error(`Unknown command: ${id}`)
  }
  return command
}
