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
    const pageModel = useModel(PageModel)

    <pageModelContext.Provider value={pageModel}>
      {props.children}
    </pageModelContext.Provider>
  }

  function usePageModel() {
    const pageModel = useContext(pageModelContext)

    if (!pageModel) {
      throw new Error("Page model is missing.")
    }

    return pageModel
  }

  return {
    PageModelProvider,
    usePageModel,
  }
}
