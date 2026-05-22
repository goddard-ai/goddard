import { signal } from "@preact/signals"
import {
  Bot,
  Brain,
  Command,
  Folder,
  FolderOpen,
  GitBranch,
  Inbox,
  Keyboard,
  Lightbulb,
  ListTodo,
  Map,
  MapPin,
  MessageSquarePlus,
  PanelTopClose,
  Search,
  SendHorizontal,
  Settings,
} from "lucide-react"
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
          if (
            (appCommandHandlerCounts.value[getActiveCommandLayerId()]?.[id as AppCommandId] ??
              0) === 0
          ) {
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
      icon: PanelTopClose,
      when: "workbench.hasClosableActiveTab",
    },
  },
  navigation: {
    openProposeTaskDialog: {
      label: "Open Propose Task Dialog",
      icon: Lightbulb,
    },
    openNewSessionDialog: {
      label: "Open New Session Dialog",
      icon: MessageSquarePlus,
    },
    openSwitchProject: {
      label: "Switch Project",
      icon: FolderOpen,
    },
    openCommandPalette: {
      label: "Open Command Menu",
      icon: Command,
    },
    openKeyboardShortcuts: {
      label: "Open Keyboard Shortcuts",
      icon: Keyboard,
    },
    openInbox: {
      label: "Open Inbox",
      icon: Inbox,
    },
    openSessions: {
      label: "Open Sessions",
      icon: MessageSquarePlus,
    },
    openSearch: {
      label: "Open Search",
      icon: Search,
    },
    openSpecs: {
      label: "Open Specs",
      icon: Folder,
    },
    openTasks: {
      label: "Open Tasks",
      icon: ListTodo,
    },
    openRoadmap: {
      label: "Open Roadmap",
      icon: Map,
    },
    openSettings: {
      label: "Open Settings",
      icon: Settings,
    },
  },
  projects: {
    openFolder: {
      label: "Projects: Open Folder",
      icon: FolderOpen,
      description: "Open a project from your filesystem.",
    },
  },
  sessionInput: {
    openProjectSelector: {
      label: "Session Input: Open Project Selector",
      icon: FolderOpen,
    },
    openAdapterSelector: {
      label: "Session Input: Open Adapter Selector",
      icon: Bot,
    },
    openLocationSelector: {
      label: "Session Input: Open Launch Location Selector",
      icon: MapPin,
    },
    openBranchSelector: {
      label: "Session Input: Open Branch Selector",
      icon: GitBranch,
    },
    openModelSelector: {
      label: "Session Input: Open Model Selector",
      icon: Brain,
    },
    openThinkingLevelSelector: {
      label: "Session Input: Open Thinking Level Selector",
      icon: Brain,
    },
    submit: {
      label: "Session Input: Submit",
      icon: SendHorizontal,
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
  return (appCommandHandlerCounts.value[getActiveCommandLayerId()]?.[commandId] ?? 0) > 0
}

export function isAppCommandHandledAnywhere(commandId: AppCommandId) {
  return Object.values(appCommandHandlerCounts.value).some(
    (layerCounts) => (layerCounts[commandId] ?? 0) > 0,
  )
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
