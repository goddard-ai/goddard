import { signal } from "@preact/signals"

/** One active overlay surface and any portal roots that should count as inside it. */
export type OverlayStackEntry = {
  id: string
  elements: readonly HTMLElement[]
  close?: () => void
  group?: string | null
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
    const remainingEntries = entries.value.filter((candidate) => candidate.id !== entry.id)
    // Ancestors stay open so nested popovers can share a group with their parent.
    const groupedEntriesToClose =
      entry.group == null
        ? []
        : remainingEntries.filter(
            (candidate) =>
              candidate.group === entry.group &&
              candidate.close &&
              !isAncestorOverlay(candidate, entry),
          )
    const groupedTokensToClose = new Set(groupedEntriesToClose.map((candidate) => candidate.token))

    entries.value = [
      ...remainingEntries.filter((candidate) => !groupedTokensToClose.has(candidate.token)),
      { ...entry, token },
    ]

    for (const groupedEntry of [...groupedEntriesToClose].reverse()) {
      groupedEntry.close?.()
    }

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

function isAncestorOverlay(candidate: OverlayStackRecord, entry: OverlayStackEntry) {
  return candidate.elements.some((candidateElement) =>
    entry.elements.some(
      (entryElement) =>
        candidateElement !== entryElement && candidateElement.contains(entryElement),
    ),
  )
}

export const overlayStack = createOverlayStack()
