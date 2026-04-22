import { createContext } from "preact"
import { useContext, useEffect, useState } from "preact/hooks"

/** Shared viewport metrics exposed to detail-tab surfaces that render inside one scroller. */
export type TabViewportSnapshot = {
  viewportRef: preact.RefObject<HTMLDivElement | null>;
  width: number;
  height: number;
  scrollTop: number;
  fontsReady: boolean;
}

const tabViewportContext = createContext<TabViewportSnapshot | null>(null)

/** Throws when a shared tab viewport consumer renders outside its provider. */
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

/** Props accepted by the shared tab viewport provider. */
export type TabViewportProviderProps = {
  children: preact.ComponentChildren;
  viewportRef: preact.RefObject<HTMLDivElement | null>;
  scrollTop: number;
}

/** Publishes one externally owned scroller into the shared tab viewport context. */
export function TabViewportProvider(props: TabViewportProviderProps) {
  const fontsReady = useFontsReady()
  const [viewport, setViewport] = useState(
    {
      width: 0,
      height: 0,
    },
  )

  useEffect(() => {
    const viewportElement = props.viewportRef.current

    if (!viewportElement) {
      return
    }

    setViewport(
      {
        width: viewportElement.clientWidth,
        height: viewportElement.clientHeight,
      },
    )

    const resizeObserver = new ResizeObserver((entries) => {
      const nextEntry = entries[0]

      if (!nextEntry) {
        return
      }

      setViewport(
        {
          width: nextEntry.contentRect.width,
          height: nextEntry.contentRect.height,
        },
      )
    })

    resizeObserver.observe(viewportElement)

    return () => {
      resizeObserver.disconnect()
    }
  }, [props.viewportRef])

  return <tsx:react>
    <tabViewportContext.Provider value={{
      viewportRef: props.viewportRef,
      width: viewport.width,
      height: viewport.height,
      scrollTop: props.scrollTop,
      fontsReady,
    }}>
      {props.children}
    </tabViewportContext.Provider>
  </tsx:react>
}

/** Returns the shared viewport metrics for the nearest tab viewport provider. */
export function useTabViewport(): TabViewportSnapshot {
  return requireContext(useContext(tabViewportContext), "tabViewportContext")
}
