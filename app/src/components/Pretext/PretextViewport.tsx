import type { ComponentChildren, RefObject } from "preact"
import { createContext } from "preact"
import { useContext, useEffect, useState } from "preact/hooks"

/** Shared viewport metrics exposed to Pretext-driven paragraph renderers. */
export type PretextViewportSnapshot = {
  viewportRef: RefObject<HTMLDivElement | null>
  width: number
  height: number
  scrollTop: number
  fontsReady: boolean
}

const pretextViewportContext = createContext<PretextViewportSnapshot | null>(null)

/** Throws when a shared Pretext viewport consumer renders outside its provider. */
function requireContext<Value>(value: Value | null, name: string): Value {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

/** Waits for document fonts before trusting Pretext measurements as final. */
function useFontsReady(): boolean {
  const [fontsReady, setFontsReady] = useState(
    typeof document === "undefined" || !("fonts" in document) || document.fonts.status === "loaded",
  )

  useEffect(() => {
    if (fontsReady || typeof document === "undefined" || !("fonts" in document)) {
      return
    }

    let cancelled = false

    document.fonts.ready.then(() => {
      if (!cancelled) {
        setFontsReady(true)
      }
    })

    return () => {
      cancelled = true
    }
  }, [fontsReady])

  return fontsReady
}

/** Props accepted by the shared Pretext viewport provider. */
export type PretextViewportProviderProps = {
  children: ComponentChildren
  viewportRef: RefObject<HTMLDivElement | null>
  scrollTop: number
}

/** Publishes one externally owned scroller into the shared Pretext viewport context. */
export function PretextViewportProvider(props: PretextViewportProviderProps) {
  const fontsReady = useFontsReady()
  const [viewport, setViewport] = useState({
    width: 0,
    height: 0,
  })

  useEffect(() => {
    const viewportElement = props.viewportRef.current

    if (!viewportElement) {
      return
    }

    setViewport({
      width: viewportElement.clientWidth,
      height: viewportElement.clientHeight,
    })

    const resizeObserver = new ResizeObserver((entries) => {
      const nextEntry = entries[0]

      if (!nextEntry) {
        return
      }

      setViewport({
        width: nextEntry.contentRect.width,
        height: nextEntry.contentRect.height,
      })
    })

    resizeObserver.observe(viewportElement)

    return () => {
      resizeObserver.disconnect()
    }
  }, [props.viewportRef])

  return (
    <pretextViewportContext.Provider
      value={{
        viewportRef: props.viewportRef,
        width: viewport.width,
        height: viewport.height,
        scrollTop: props.scrollTop,
        fontsReady,
      }}
    >
      {props.children}
    </pretextViewportContext.Provider>
  )
}

/** Returns the shared viewport metrics for the nearest Pretext viewport provider. */
export function usePretextViewport(): PretextViewportSnapshot {
  return requireContext(useContext(pretextViewportContext), "pretextViewportContext")
}
