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

export function createPageQueryDataContext<QueryData extends Record<string, unknown>>() {
  const pageQueryDataContext = createContext<QueryData | null>(null)

  component PageQueryDataProvider(props: {
    children: preact.ComponentChildren;
    value: QueryData;
  }) {
    const value = useStableObjectByValues(props.value)

    <pageQueryDataContext.Provider {value}>
      {props.children}
    </pageQueryDataContext.Provider>
  }

  function usePageQueryData() {
    const queryData = useContext(pageQueryDataContext)

    if (!queryData) {
      throw new Error("Page query data is missing.")
    }

    return queryData
  }

  return {
    PageQueryDataProvider,
    usePageQueryData,
  }
}
