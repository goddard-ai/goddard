import { signal } from "@preact/signals"
import type { RunnableInput, ShortcutMatch } from "powerkeys"
import { SigmaTarget, useListener } from "preact-sigma"
import { useLayoutEffect } from "preact/hooks"
import { mapValues } from "radashi"

import type { AppCommandId } from "~/shared/app-commands.ts"
import { AppCommand } from "./app-command-definitions.ts"
import { getActiveCommandLayerId, useCommandLayer } from "./command-layer.tsrx"

/** Event map for command invocations; event names are generated app command ids. */
type AppCommandEvents = Record<string, ShortcutMatch | undefined>

const appCommandBus = new SigmaTarget<AppCommandEvents>()
const appCommandHandlerCounts = signal<Record<string, Partial<Record<AppCommandId, number>>>>({})

export type AppCommandDefinition = RunnableInput & {
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

/** Nested command definitions keyed by namespace and command name. */
export type AppCommandTable = {
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

/** Creates executable app command functions from the declarative command table. */
export function defineAppCommands<const TCommands extends AppCommandTable>(
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

export { AppCommand }

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
