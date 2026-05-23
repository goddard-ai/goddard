import { useEffect } from "preact/hooks"

import { useWorkbenchTabCache } from "./app-state-context.tsrx"
import { useTabContext } from "./tab-context.tsrx"

type WorkbenchCachedValue<TValue> = {
  dispose?: () => void
  setup?: () => void | (() => void)
  value: TValue
}

/** Returns a retained value scoped to the current detail tab. */
export function useWorkbenchCachedValue<TValue>(
  key: string,
  createValue: () => WorkbenchCachedValue<TValue>,
) {
  const tabId = useTabContext().id
  const workbenchTabCache = useWorkbenchTabCache()
  const value = workbenchTabCache.getOrCreate(tabId, key, createValue)

  useEffect(() => {
    workbenchTabCache.setup(tabId, key)
  }, [key, tabId, workbenchTabCache])

  return value
}
