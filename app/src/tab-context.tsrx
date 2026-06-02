import { createContext } from "preact"
import { useContext } from "preact/hooks"

/** Context shared by surfaces rendered inside one workbench tab. */
export type TabContextSnapshot = {
  id: string;
  scrollParentRef: preact.RefObject<HTMLDivElement | null>;
}

// Keep context identity stable when Vite re-evaluates this module during HMR.
const tabContext = (globalThis as typeof globalThis & {
    __goddardTabContext?: preact.Context<TabContextSnapshot | null>;
  }).__goddardTabContext ??= createContext<TabContextSnapshot | null>(null)

export component TabContextProvider(props: {
  children: preact.ComponentChildren;
  id: string;
  scrollParentRef: preact.RefObject<HTMLDivElement | null>;
}) {
  <tabContext.Provider value={{ id: props.id, scrollParentRef: props.scrollParentRef }}>
    {props.children}
  </tabContext.Provider>
}

export function useOptionalTabContext() {
  return useContext(tabContext)
}

export function useTabContext() {
  const context = useOptionalTabContext()

  if (!context) {
    throw new Error("tabContext is missing.")
  }

  return context
}
