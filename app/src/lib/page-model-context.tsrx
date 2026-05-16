import { createContext } from "preact"
import { useContext } from "preact/hooks"
import {
  createModel,
  useModel,
  type Model,
  type ReadonlySignal,
  type Signal,
} from "@preact/signals"
import { type ModelFactory } from "@preact/signals-core"

/** Read-only view of a page model where mutable Signal properties become ReadonlySignal properties. */
type ReadonlyPageModel<TModel> =
  TModel extends Signal<infer Value>
    ? ReadonlySignal<Value>
    : TModel extends ReadonlySignal<unknown>
      ? TModel
      : TModel extends (...args: any[]) => any
        ? TModel
        : TModel extends object ? { [Key in keyof TModel]: ReadonlyPageModel<TModel[Key]> } : TModel

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

    return page as Model<ReadonlyPageModel<TModel>>
  }

  return {
    PageModelProvider,
    usePageModel,
  }
}
