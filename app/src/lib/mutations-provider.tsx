import { createContext, type Context, type FunctionComponent } from "preact"
import { useContext, useEffect, useMemo, useRef } from "preact/hooks"
import { mapValues } from "radashi"

type AnyFunction = (...args: any[]) => any

type MutationsProvider<T extends Record<string, AnyFunction>> = FunctionComponent<{
  children: preact.ComponentChildren
  mutations: T
}> & {
  context: Context<T | null>
}

export function useMutations<T extends Record<string, AnyFunction>>(
  provider: MutationsProvider<T>,
) {
  const mutations = useContext(provider.context)
  if (!mutations) {
    throw new Error(`Mutations provider "${provider.displayName}" was not found.`)
  }
  return mutations
}

export function createMutationsProvider<T extends Record<string, AnyFunction>>(
  displayName: string,
) {
  const MutationsContext = createContext<T | null>(null)
  MutationsContext.displayName = displayName

  function MutationsProvider(props: { children: preact.ComponentChildren; mutations: T }) {
    const mutationsRef = useRef(props.mutations)
    useEffect(() => {
      mutationsRef.current = props.mutations
    })

    const context = useMemo(() => {
      return mapValues(props.mutations, (_, key) => {
        return (...args: any[]) => mutationsRef.current[key](...args)
      }) as T
    }, Object.keys(props.mutations))

    return <MutationsContext value={context}>{props.children}</MutationsContext>
  }

  MutationsProvider.context = MutationsContext
  return MutationsProvider
}
