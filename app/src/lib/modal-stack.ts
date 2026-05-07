import { computed, signal } from "@preact/signals"
import { useEffect, useRef } from "preact/hooks"

/** One modal surface that can be closed by the app-wide close command. */
export type ModalStackEntry = {
  id: string
  close: () => void
}

type ModalStackRecord = ModalStackEntry & {
  token: number
}

/** Creates one LIFO stack for currently open modal surfaces. */
export function createModalStack() {
  const entries = signal<readonly ModalStackRecord[]>([])
  let nextToken = 0
  const hasOpenModal = computed(() => entries.value.length > 0)

  function register(entry: ModalStackEntry) {
    const token = nextToken++

    entries.value = [
      ...entries.value.filter((candidate) => candidate.id !== entry.id),
      { ...entry, token },
    ]

    return () => {
      entries.value = entries.value.filter((candidate) => candidate.token !== token)
    }
  }

  function closeTopmost() {
    const entry = entries.value.at(-1)

    if (!entry) {
      return false
    }

    entries.value = entries.value.filter((candidate) => candidate.token !== entry.token)
    entry.close()
    return true
  }

  return {
    closeTopmost,
    hasOpenModal,
    register,
  }
}

const appModalStack = createModalStack()

export const hasOpenModalDialog = appModalStack.hasOpenModal
export const registerModalStackEntry = appModalStack.register
export const closeTopmostModal = appModalStack.closeTopmost

/** Registers an open modal in the app stack until it closes or unmounts. */
export function useModalStackEntry(entry: ModalStackEntry & { open: boolean }) {
  const closeRef = useRef(entry.close)
  closeRef.current = entry.close

  useEffect(() => {
    if (!entry.open) {
      return
    }

    return registerModalStackEntry({
      id: entry.id,
      close() {
        closeRef.current()
      },
    })
  }, [entry.id, entry.open])
}
