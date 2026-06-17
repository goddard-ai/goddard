import { useEffect, useRef } from "preact/hooks"

const initialErrorBoundaryResetDelayMs = 1000
const maximumErrorBoundaryResetDelayMs = 30000

function getErrorBoundaryResetDelay(attempt: number) {
  return Math.min(initialErrorBoundaryResetDelayMs * 2 ** attempt, maximumErrorBoundaryResetDelayMs)
}

export function useErrorBoundaryReset(reset: () => void) {
  const resetRef = useRef(reset)
  resetRef.current = reset

  useEffect(() => {
    let disposed = false
    let attempt = 0
    let timeoutId: number | null = null

    function clearScheduledReset() {
      if (timeoutId === null) {
        return
      }

      window.clearTimeout(timeoutId)
      timeoutId = null
    }

    function scheduleReset() {
      clearScheduledReset()
      timeoutId = window.setTimeout(runReset, getErrorBoundaryResetDelay(attempt))
    }

    function runReset() {
      if (disposed) {
        return
      }

      clearScheduledReset()
      resetRef.current()
      attempt += 1
      scheduleReset()
    }

    function resetOnVisibleDocument() {
      if (document.visibilityState === "hidden") {
        return
      }

      runReset()
    }

    scheduleReset()
    window.addEventListener("focus", runReset)
    document.addEventListener("focus", runReset, true)
    document.addEventListener("visibilitychange", resetOnVisibleDocument)

    return () => {
      disposed = true
      clearScheduledReset()
      window.removeEventListener("focus", runReset)
      document.removeEventListener("focus", runReset, true)
      document.removeEventListener("visibilitychange", resetOnVisibleDocument)
    }
  }, [])
}
