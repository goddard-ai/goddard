import { computed, signal } from "@preact/signals"
import { createContext } from "preact"
import { useContext, useId, useLayoutEffect } from "preact/hooks"

const rootCommandLayerId = "root"

const commandLayerContext = createContext(rootCommandLayerId)
const activeCommandLayerIds = signal<readonly string[]>([])

const activeCommandLayerId = computed(() => {
  return activeCommandLayerIds.value.at(-1) ?? rootCommandLayerId
})

function registerActiveCommandLayer(layerId: string) {
  activeCommandLayerIds.value = [
    ...activeCommandLayerIds.value.filter((activeLayerId) => activeLayerId !== layerId),
    layerId,
  ]

  return () => {
    activeCommandLayerIds.value = activeCommandLayerIds.value.filter(
      (activeLayerId) => activeLayerId !== layerId,
    )
  }
}

/** Reads the currently active command layer for reactive command binding updates. */
export function getActiveCommandLayerId() {
  return activeCommandLayerId.value
}

/** Reads the command layer owned by the nearest command layer provider. */
export function useCommandLayer() {
  const layerId = useContext(commandLayerContext)

  return {
    active: activeCommandLayerId.value === layerId,
    id: layerId,
  }
}

/** Provides a command layer for command handlers rendered inside one UI surface. */
export function CommandLayerProvider(props: {
  active?: boolean;
  children?: preact.ComponentChildren;
}) @{
  const layerId = useId()
  const active = props.active ?? true

  useLayoutEffect(() => {
    if (!active) {
      return
    }

    return registerActiveCommandLayer(layerId)
  }, [active, layerId])

  <commandLayerContext.Provider value={layerId}>{props.children}</commandLayerContext.Provider>
}
