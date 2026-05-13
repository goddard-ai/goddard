import { createContext } from "preact"
import { useContext } from "preact/hooks"
import { createModel, useModel, type Model, type ReadonlySignal } from "@preact/signals"

type PageModelValue<Value> = Value extends ReadonlySignal<unknown> ? Value : Value extends (...args: any[]) => any ? Value : Value extends object ? { [Key in keyof Value]: PageModelValue<Value[Key]> } : never

export function createPageModelContext<CreatePageModel extends () => object>(createPageModel: CreatePageModel & (() => PageModelValue<ReturnType<CreatePageModel>>)) {
  type PageModel = ReturnType<CreatePageModel>

  const PageModel = createModel<PageModel>(createPageModel as never)
  const pageModelContext = createContext<Model<PageModel> | null>(null)

  component PageModelProvider(props: {
    children: preact.ComponentChildren;
  }) {
    const page = useModel(PageModel)

    <pageModelContext.Provider value={page}>
      {props.children}
    </pageModelContext.Provider>
  }

  function usePageModel() {
    const page = useContext(pageModelContext)

    if (!page) {
      throw new Error("Page model is missing.")
    }

    return page
  }

  return {
    PageModelProvider,
    usePageModel,
  }
}
