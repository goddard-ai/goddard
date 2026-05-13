import { createContext } from "preact"
import { useContext } from "preact/hooks"
import { createModel, useModel, type Model } from "@preact/signals"
import { type ModelFactory } from "@preact/signals-core"

export function createPageModelContext<TModel, TFactoryArgs extends any[] = []>(createPageModel: ModelFactory<TModel, TFactoryArgs>) {
  const PageModel = createModel<TModel>(createPageModel)
  const pageModelContext = createContext<Model<TModel> | null>(null)

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
