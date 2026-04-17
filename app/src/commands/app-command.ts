import type { RunnableInput, ShortcutMatch } from "powerkeys"
import { SigmaTarget, useListener } from "preact-sigma"
import { mapValues } from "radashi"

import type { AppCommandId } from "~/shared/app-commands.ts"

const appCommandBus = new SigmaTarget<Record<string, ShortcutMatch | undefined>>()

type AppCommandDefinition = RunnableInput & {
  /** The label for the command menu. */
  label: string
  /** Optional icon for the command menu. */
  icon?: preact.FunctionComponent<{
    className?: string
    style?: preact.CSSProperties
    size?: number
    strokeWidth?: number
    "aria-hidden"?: boolean
  }>
  /** Optional keywords for filtering in the command menu. */
  keywords?: readonly string[]
  /** Optional autocomplete description for JSON keymap files. */
  description?: string
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
      when: "workbench.hasClosableActiveTab",
    },
  },
  navigation: {
    openProposeTaskDialog: {
      label: "Open Propose Task Dialog",
    },
    openNewSessionDialog: {
      label: "Open New Session Dialog",
    },
    openSwitchProject: {
      label: "Switch Project",
    },
    openCommandPalette: {
      label: "Open Command Menu",
      when: "!sessionInput.isActive",
    },
    openKeyboardShortcuts: {
      label: "Open Keyboard Shortcuts",
    },
    openInbox: {
      label: "Open Inbox",
    },
    openProjects: {
      label: "Open Projects",
    },
    openSessions: {
      label: "Open Sessions",
    },
    openSearch: {
      label: "Open Search",
    },
    openSpecs: {
      label: "Open Specs",
    },
    openTasks: {
      label: "Open Tasks",
    },
    openRoadmap: {
      label: "Open Roadmap",
    },
    openSettings: {
      label: "Open Settings",
    },
  },
  projects: {
    openFolder: {
      label: "Projects: Open Folder",
      description: "Open a project from your filesystem.",
    },
  },
  sessionInput: {
    openProjectSelector: {
      label: "Session Input: Open Project Selector",
      when: "sessionInput.hasProjectSelector",
    },
    openModelSelector: {
      label: "Session Input: Open Model Selector",
      when: "sessionInput.hasModelSelector",
    },
    toggleThinkingLevel: {
      label: "Session Input: Toggle Thinking Level",
      when: "sessionInput.hasThinkingLevel",
    },
    submit: {
      label: "Session Input: Submit",
      when: "sessionInput.canSubmit",
    },
  },
})

export type AppCommand = (typeof AppCommand)[keyof typeof AppCommand] extends infer TNamespace
  ? TNamespace extends object
    ? TNamespace[keyof TNamespace]
    : never
  : never

export const appCommandList = Object.values(AppCommand).flatMap(
  (commands) => Object.values(commands) as AppCommand[],
)

export function onAppCommand(command: AppCommand, listener: (match?: ShortcutMatch) => void) {
  return appCommandBus.on(command.id, listener)
}

export function useAppCommand(command: AppCommand, listener: (match?: ShortcutMatch) => void) {
  useListener(appCommandBus, command.id, listener)
}

export function resolveAppCommand(id: AppCommandId): AppCommand | null {
  const [namespaceKey, commandKey] = id.split(".")
  const namespace = (AppCommand as any)[namespaceKey]
  const command = namespace?.[commandKey]
  return command ?? null
}
