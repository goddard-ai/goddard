import { useEffect } from "preact/hooks"

import { useMainTab, useWorkbenchTabSet } from "~/app-state-context.tsrx"

export function LaunchableStates() {
  const mainTab = useMainTab()
  const workbenchTabSet = useWorkbenchTabSet()

  useEffect(() => {
    let cleanup: (() => void) | null = null
    let disposed = false

    void import("./install.ts").then(({ installLaunchableStates }) => {
      if (disposed) {
        return
      }

      cleanup = installLaunchableStates({
        mainTab,
        workbenchTabSet,
      })
    })

    return () => {
      disposed = true
      cleanup?.()
    }
  }, [mainTab, workbenchTabSet])

  return null
}
