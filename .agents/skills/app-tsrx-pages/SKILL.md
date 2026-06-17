---
name: app-tsrx-pages
description: Refine Goddard app page-like TSRX components, including page skeletons, TSRX control flow, query data contexts, page models, task helpers, loading/error UI, and domain model coordination.
---

# App TSRX Pages

Use this skill when refining page-like TSRX components. In app guidance, a page is the logical UI boundary defined in `app/glossary.md`: it may be a route, detail tab, panel, dialog, or embedded surface that owns any query data, page model, and internal UI coordination it needs.

## Keep The Page Shape Visible

- Prefer one exported page component that shows the whole page skeleton when the boundary is only used once.
- For workbench tab components, keep `export default` on the primary `function ... @{ ... }` declaration when the module exports one primary tab surface.
- Use TSRX function bodies for page-like components so the page skeleton remains the final JSX-producing statement.

## Use TSRX Control Flow Directly

- Use `@try { ... } @pending { ... } @catch (error, retry) { ... }` for query-driven loading and error UI.
- Do not detect loading by catching promises manually with helpers such as `isPromise(error)`.
- Use the retry function supplied by the TSRX `@catch` block when re-running the failed render path.
- Wrap shared layout outside the `@try` / `@pending` / `@catch` block when all states share it. For example, render one `<main>` and put success, pending, and error content inside it.
- Inline trivial retry invalidation logic in the `@catch` action when it is single-use and easy to read.
- Put local statements inside JSX children in a `@{ ... }` template block; keep the final statement JSX-producing.
- Use `@if`, `@for`, and `@switch` directly for page branching, repeated page sections, and selected-state UI instead of precomputing small JSX islands only to render them later.

## Separate Query Data From Page UI State

- Keep query data and page UI state in separate contexts.
- Do not wrap query data inside the page model. Query data should remain available as query data, even when a page model also exists.
- Page models must not depend on query data. Use query data context, domain models, or loaded-branch logic for state that derives from query results.
- Use a query data context for loaded query results and any reactive domain model derived from them, including Sigma instances that wrap query data.
- Use a page model context for page-level UI state such as pending tasks, local action errors, selected IDs, or one-off UI bookkeeping.
- Prefer making both contexts available to the whole page tree when practical. Prop drilling should be the fallback, not the default.
- Consume page-wide query data and page models through feature hooks instead of pass-through props.
- Use the page model and query data context helpers instead of hand-rolled providers; they handle provider value stability automatically.
- Let the page model provider wrap `@try` / `@pending` / `@catch`. Put the query data provider in the successful loaded branch when it depends on loaded query results.

## Prefer Page Models For Page-Level UI State

- Represent page-level UI state with signals on a page model instead of scattered `useState` calls in the page component.
- Keep the page model focused on UI state and common UI mutations. Domain data and query results belong outside it.
- Keep UI state owned by one component or UI primitive local to that component or primitive.
- Page model context consumers receive a readonly view of signal properties, so expose mutations as named page model methods.
- Initialize the page model with a provider, then consume it through the page's `use...PageModel()` hook.
- Create page model contexts from the model factory and rely on inference instead of duplicating explicit page model types.
- TSRX `@{ ... }` function bodies allow a component to provide and consume the page model in the same component scope; do not add a wrapper component solely to make context consumption possible.
- Prefer the local variable name `page` for the consumed page model. The `page.*` prefix is useful context.

## Use Tasks For Promise State

- Use the task helpers in `app/src/lib/task.ts` for common async UI state such as `isPending`, `error`, `run()`, and `clearError()`.
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

## Keep Loading And Error UI Generic

- Prefer generic page-level loading and error messages unless the missing context is common enough to justify a specific branch.
- Keep state-specific UI in the shared page layout so loading, error, and success states feel structurally consistent.
- Use the page model for recoverable action errors inside the loaded page, and TSRX `@catch` blocks for query/render failure states.
