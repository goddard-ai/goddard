import { computed, signal } from "@preact/signals"
import { createContext } from "preact"
import { useContext, useEffect, useId } from "preact/hooks"

import type { AppCommandId } from "~/shared/app-commands.ts"

export const rootCommandLayerId = "root"

const commandLayerContext = createContext(rootCommandLayerId)
const activeCommandLayerEntries = signal<readonly { id: string; token: number }[]>([])
const commandLayerHandlerCounts = signal<Record<string, Record<string, number>>>({})
let nextCommandLayerToken = 0

const activeCommandLayerId = computed(() => {
  return activeCommandLayerEntries.value.at(-1)?.id ?? rootCommandLayerId
})

/** Checks whether one command layer is currently allowed to receive command events. */
export function isCommandLayerActive(layerId: string) {
  return activeCommandLayerId.peek() === layerId
}

/** Checks whether the active command layer has any listener for one command id. */
export function hasActiveCommandLayerHandler(commandId: AppCommandId) {
  const activeLayerId = activeCommandLayerId.peek()
  const allLayerHandlers = commandLayerHandlerCounts.peek()
  const activeLayerHandlers = allLayerHandlers[activeLayerId]

  return (activeLayerHandlers?.[commandId] ?? 0) > 0
}

/** Pushes one UI command layer onto the active layer stack until it unmounts or deactivates. */
export function registerActiveCommandLayer(layerId: string) {
  const token = nextCommandLayerToken++

  activeCommandLayerEntries.value = [
    ...activeCommandLayerEntries.value.filter((entry) => entry.id !== layerId),
    { id: layerId, token },
  ]

  return () => {
    activeCommandLayerEntries.value = activeCommandLayerEntries.value.filter(
      (entry) => entry.token !== token,
    )
  }
}

/** Records that one layer can handle one command while the returned cleanup is retained. */
export function registerCommandLayerHandler(layerId: string, commandId: AppCommandId) {
  const currentHandlers = commandLayerHandlerCounts.value
  const layerHandlers = currentHandlers[layerId] ?? {}
  const nextLayerHandlers = { ...layerHandlers }
  const nextHandlers = { ...currentHandlers }

  nextLayerHandlers[commandId] = (layerHandlers[commandId] ?? 0) + 1
  nextHandlers[layerId] = nextLayerHandlers
  commandLayerHandlerCounts.value = nextHandlers

  return () => {
    const nextHandlers = { ...commandLayerHandlerCounts.value }
    const nextLayerHandlers = { ...(nextHandlers[layerId] ?? {}) }
    const nextCount = (nextLayerHandlers[commandId] ?? 0) - 1

    if (nextCount > 0) {
      nextLayerHandlers[commandId] = nextCount
    } else {
      delete nextLayerHandlers[commandId]
    }

    if (Object.keys(nextLayerHandlers).length === 0) {
      delete nextHandlers[layerId]
    } else {
      nextHandlers[layerId] = nextLayerHandlers
    }

    commandLayerHandlerCounts.value = nextHandlers
  }
}

/** Reads the command layer owned by the nearest command-layer provider. */
export function useCommandLayerId() {
  return useContext(commandLayerContext)
}

/** Provides a command layer for command handlers rendered inside one UI surface. */
export component CommandLayerProvider(props: {
  id?: string;
  active?: boolean;
  children: preact.ComponentChildren;
}) {
  const generatedId = useId()
  const layerId = props.id ?? generatedId
  const active = props.active ?? true

  useEffect(() => {
    if (!active) {
      return
    }

    return registerActiveCommandLayer(layerId)
  }, [active, layerId])

  <commandLayerContext.Provider value={layerId}>
    {props.children}
  </commandLayerContext.Provider>
}
