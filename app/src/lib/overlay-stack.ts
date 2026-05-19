import { signal } from "@preact/signals"

/** One active overlay surface and any portal roots that should count as inside it. */
export type OverlayStackEntry = {
  id: string
  elements: readonly HTMLElement[]
  close?: () => void
}

type OverlayStackRecord = OverlayStackEntry & {
  token: number
}

/** Tracks active overlay surfaces so nested portal content counts as inside its parent overlay. */
export function createOverlayStack() {
  const entries = signal<readonly OverlayStackRecord[]>([])
  let nextToken = 0

  function register(entry: OverlayStackEntry) {
    const token = nextToken++

    entries.value = [
      ...entries.value.filter((candidate) => candidate.id !== entry.id),
      { ...entry, token },
    ]

    return () => {
      entries.value = entries.value.filter((candidate) => candidate.token !== token)
    }
  }

  function isTopmost(id: string) {
    return entries.value.at(-1)?.id === id
  }

  function contains(target: EventTarget | null) {
    if (!(target instanceof Node)) {
      return false
    }

    return entries.value.some((entry) =>
      entry.elements.some((element) => element === target || element.contains(target)),
    )
  }

  function closeTopmost() {
    const entry = entries.value.at(-1)

    if (!entry?.close) {
      return false
    }

    entries.value = entries.value.filter((candidate) => candidate.token !== entry.token)
    entry.close()
    return true
  }

  return {
    closeTopmost,
    contains,
    entries,
    isTopmost,
    register,
  }
}

export const overlayStack = createOverlayStack()
