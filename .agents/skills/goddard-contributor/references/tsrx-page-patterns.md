# TSRX Page Patterns

Use these patterns when refining page-like TSRX components. In app guidance, a page is the logical UI boundary defined in `app/glossary.md`: it may be a route, detail tab, panel, dialog, or embedded surface that owns any query data, page model, and internal UI coordination it needs.

## Keep The Page Shape Visible

- Prefer one exported page component that shows the whole page skeleton when the boundary is only used once.
- Inline single-use local components unless extraction gives the JSX a meaningful reusable concept.
- Keep `export default` on the `component` declaration when the module exports one primary page component.
- Avoid relay components whose main job is to rename loaded data or forward props into a single child.

## Use TSRX Control Flow Directly

- Use `try { ... } pending { ... } catch (error, retry) { ... }` for query-driven loading and error UI.
- Do not detect loading by catching promises manually with helpers such as `isPromise(error)`.
- Use the retry function supplied by the TSRX `catch` block when re-running the failed render path.
- Wrap shared layout outside the `try-pending-catch` block when all states share it. For example, render one `<main>` and put success, pending, and error content inside it.
- Inline trivial retry invalidation logic in the catch action when it is single-use and easy to read.

## Separate Query Data From Page UI State

- Keep query data and page UI state in separate contexts.
- Do not wrap query data inside the page model. Query data should remain available as query data, even when a page model also exists.
- Use a query data context for loaded query results and any reactive domain model derived from them, including Sigma instances that wrap query data.
- Use a page model context for page-level UI state such as pending tasks, local action errors, selected IDs, or one-off UI bookkeeping.
- Prefer making both contexts available to the whole page tree when practical. Prop drilling should be the fallback, not the default.
- Consume page-wide query data and page models through feature hooks instead of pass-through props.
- Memoize provided context value objects based on their property values so consumers do not rerender just because a fresh object literal was created.
- Let the page model provider wrap `try-pending-catch` when the model does not need query data. Put the query data provider in the successful loaded branch when it depends on loaded query results.

## Prefer Page Models For Page-Level UI State

- Represent page-level UI state with signals on a page model instead of scattered `useState` calls in the page component.
- Keep the page model focused on UI state and common UI mutations. Domain data and query results belong outside it.
- Keep UI state owned by one component or UI primitive local to that component or primitive.
- Use readonly model access for consumers so subcomponents can observe page signals without casually mutating them.
- Initialize the page model with a provider, then consume it through the page's `use...PageModel()` hook.
- Create page model contexts from the model factory and rely on inference instead of duplicating explicit page model types.
- TSRX allows a component to provide and consume the page model in the same component scope; do not add a wrapper component solely to make context consumption possible.
- Prefer the local variable name `page` for the consumed page model. The `page.*` prefix is useful context.

## Use Tasks For Promise State

- Use task primitives for common async UI state such as `isPending`, `error`, `run()`, and `clearError()`.
- Use keyed tasks when a group of mutually exclusive actions needs one active key and one shared error surface.
- Let task `run()` own concurrent-run guarding so callers do not repeat redundant `isPending` checks.
- Have `run()` no-op while pending and return `Promise<void>` rather than returning a shared active promise whose result type may not match concurrent callers.
- Keep task result values out of task state unless the UI needs them. Most page tasks only need pending and error state.
- Convert caught errors to the UI-facing error shape at the task boundary when the display surface needs a title or description.

## Let Domain Models Own Domain Mutations

- Move domain-specific data loading and merging into the owning Sigma/domain model when the operation is part of the domain model's behavior.
- It is acceptable for app domain models to call the SDK when that keeps a domain transition cohesive.
- Keep page code focused on triggering domain actions and rendering state, not stitching together domain data transforms.

## Keep Context Modules Feature-Local

- Keep page model and query data contexts near the page that owns them.
- Keeping them in the same module is fine while they stay private and the file remains readable.
- Move them into focused sibling modules once subcomponents consume them or the page module starts mixing too many concerns.

## Place Logic Near Use

- Put hooks, derived values, and event handlers near the JSX that uses them. TSRX supports hooks anywhere in component scope, including inside JSX expression blocks and conditional branches.
- Move purely presentational derived values into the JSX subtree that renders them.
- Avoid a large top-of-component variable block when the values are only meaningful in one local region.
- Avoid local aliases that only hide useful ownership or reactivity context such as `page.*` or `*.value`.
- Keep locals when they name domain meaning, avoid duplicated non-trivial logic, or are reused enough to improve clarity.

## Keep Handlers Local And Direct

- Prefer inline JSX callbacks for tiny single-use handlers.
- For larger single-use handlers, declare them in the nearest JSX or control-flow block that uses them.
- Use arrow function bindings for handlers declared inside `if` blocks so their scope matches the block.
- Pass async event handlers directly, such as `onClick={saveChanges}`, instead of wrapping them only to discard the returned promise.
- Keep explicit guards only when they express real domain or UX rules. Do not duplicate generic task concurrency checks at every call site.

## Keep Loading And Error UI Generic

- Prefer generic page-level loading and error messages unless the missing context is common enough to justify a specific branch.
- Keep state-specific UI in the shared page layout so loading, error, and success states feel structurally consistent.
- Use the page model for recoverable action errors inside the loaded page, and TSRX catch blocks for query/render failure states.

## Inline Low-Complexity Helpers

- Inline helpers that are single-use and low complexity, especially when their extraction hides nearby context.
- Keep top-level helpers for reusable presentation logic, validation, type guards, or formatting that is used in multiple places or would distract from the JSX.
- Do not create explicit helper types when the model or factory type can be inferred without duplication.
