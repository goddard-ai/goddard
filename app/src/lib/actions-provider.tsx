import { Context, createContext, FunctionComponent } from "preact"
import { useContext, useEffect, useMemo, useRef } from "preact/hooks"
import { mapValues } from "radashi"

type AnyFunction = (...args: any[]) => any

type ActionsProvider<T extends Record<string, AnyFunction>> = FunctionComponent<
  T & { children: preact.ComponentChildren }
> & {
  context: Context<T | null>
}

export function useActions<T extends Record<string, AnyFunction>>(provider: ActionsProvider<T>) {
  const actions = useContext(provider.context)
  if (!actions) {
    throw new Error(`Actions provider "${provider.displayName}" was not found.`)
  }
  return actions
}

export function createActionsProvider<T extends Record<string, AnyFunction>>(displayName: string) {
  const ActionsContext = createContext<T | null>(null)
  ActionsContext.displayName = displayName

  function ActionsProvider({
    children,
    ...actions
  }: T & {
    children: preact.ComponentChildren
  }) {
    const actionsRef = useRef(actions)
    useEffect(() => {
      actionsRef.current = actions
    })

    const context = useMemo(() => {
      return mapValues(actions, (_, key) => {
        return (...args: any[]) => actionsRef.current[key](...args)
      }) as T
    }, Object.keys(actions))

    return <ActionsContext value={context}>{children}</ActionsContext>
  }

  ActionsProvider.context = ActionsContext
  return ActionsProvider
}
