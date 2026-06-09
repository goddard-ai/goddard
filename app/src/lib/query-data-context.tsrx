import { createContext } from "preact"
import { useContext, useRef } from "preact/hooks"

function hasSameObjectValues(
  previousValue: Record<string, unknown>,
  nextValue: Record<string, unknown>,
) {
  const previousKeys = Object.keys(previousValue)
  const nextKeys = Object.keys(nextValue)

  if (previousKeys.length !== nextKeys.length) {
    return false
  }

  for (const key of previousKeys) {
    if (!Object.hasOwn(nextValue, key) || !Object.is(previousValue[key], nextValue[key])) {
      return false
    }
  }

  return true
}

function useStableObjectByValues<QueryData extends Record<string, unknown>>(value: QueryData) {
  const stableValueRef = useRef(value)

  if (!hasSameObjectValues(stableValueRef.current, value)) {
    stableValueRef.current = value
  }

  return stableValueRef.current
}

export function createQueryDataContext<QueryData extends Record<string, unknown>>() {
  const queryDataContext = createContext<QueryData | null>(null)

  function QueryDataProvider(props: {
    children: preact.ComponentChildren;
    value: QueryData;
  }) @{
    const value = useStableObjectByValues(props.value)

    <queryDataContext.Provider {value}>{props.children}</queryDataContext.Provider>
  }

  function useQueryData() {
    const queryData = useContext(queryDataContext)

    if (!queryData) {
      throw new Error("Query data is missing.")
    }

    return queryData
  }

  return {
    QueryDataProvider,
    useQueryData,
  }
}
