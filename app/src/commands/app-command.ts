import { signal } from "@preact/signals"
import type { RunnableInput, ShortcutMatch } from "powerkeys"
import { SigmaTarget, useListener } from "preact-sigma"
import { useLayoutEffect } from "preact/hooks"
import { mapValues } from "radashi"

import type { AppCommandId } from "~/shared/app-commands.ts"
import { getActiveCommandLayerId, useCommandLayer } from "./command-layer.tsrx"

/** Event map for command invocations; event names are generated app command ids. */
type AppCommandEvents = Record<string, ShortcutMatch | undefined>

const appCommandBus = new SigmaTarget<AppCommandEvents>()
const appCommandHandlerCounts = signal<Record<string, Partial<Record<AppCommandId, number>>>>({})

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
          if (!hasActiveAppCommandHandler(id as AppCommandId)) {
            return
          }

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
    },
    openKeyboardShortcuts: {
      label: "Open Keyboard Shortcuts",
    },
    openInbox: {
      label: "Open Inbox",
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
    },
    openAdapterSelector: {
      label: "Session Input: Open Adapter Selector",
    },
    openLocationSelector: {
      label: "Session Input: Open Launch Location Selector",
    },
    openBranchSelector: {
      label: "Session Input: Open Branch Selector",
    },
    openModelSelector: {
      label: "Session Input: Open Model Selector",
    },
    openThinkingLevelSelector: {
      label: "Session Input: Open Thinking Level Selector",
    },
    submit: {
      label: "Session Input: Submit",
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

function hasActiveAppCommandHandler(commandId: AppCommandId) {
  return (appCommandHandlerCounts.value[getActiveCommandLayerId()]?.[commandId] ?? 0) > 0
}

function hasAnyAppCommandHandler(commandId: AppCommandId) {
  return Object.values(appCommandHandlerCounts.value).some(
    (layerCounts) => (layerCounts[commandId] ?? 0) > 0,
  )
}

function registerAppCommandHandler(layerId: string, commandId: AppCommandId) {
  const currentCounts = appCommandHandlerCounts.value
  const currentLayerCounts = currentCounts[layerId] ?? {}

  appCommandHandlerCounts.value = {
    ...currentCounts,
    [layerId]: {
      ...currentLayerCounts,
      [commandId]: (currentLayerCounts[commandId] ?? 0) + 1,
    },
  }

  return () => {
    const nextCounts = { ...appCommandHandlerCounts.value }
    const nextLayerCounts = { ...nextCounts[layerId] }
    const nextCount = (nextLayerCounts[commandId] ?? 0) - 1

    if (nextCount > 0) {
      nextLayerCounts[commandId] = nextCount
    } else {
      delete nextLayerCounts[commandId]
    }

    if (Object.keys(nextLayerCounts).length > 0) {
      nextCounts[layerId] = nextLayerCounts
    } else {
      delete nextCounts[layerId]
    }

    appCommandHandlerCounts.value = nextCounts
  }
}

export function isAppCommandHandled(commandId: AppCommandId) {
  return hasActiveAppCommandHandler(commandId)
}

export function isAppCommandHandledAnywhere(commandId: AppCommandId) {
  return hasAnyAppCommandHandler(commandId)
}

export function useAppCommand(
  command: AppCommand,
  listener: (match?: ShortcutMatch) => void,
  options?: {
    active?: boolean
  },
) {
  const layer = useCommandLayer()
  const active = options?.active ?? true

  useLayoutEffect(() => {
    if (!active) {
      return
    }

    return registerAppCommandHandler(layer.id, command.id as AppCommandId)
  }, [active, command.id, layer.id])

  useListener(appCommandBus, command.id, (match) => {
    if (active && layer.active) {
      listener(match)
    }
  })
}

export function resolveAppCommand(id: AppCommandId): AppCommand | null {
  const [namespaceKey, commandKey] = id.split(".")
  const namespace = (AppCommand as any)[namespaceKey]
  const command = namespace?.[commandKey]
  return command ?? null
}
