# App Best Practices

These practices are derived from the current app plans, `spec/`, and `app/glossary.md`. They are written for implementation work in the Electrobun app, not for backend or SDK packages.

For app bootstrap rules already promoted into `app/AGENTS.md` and `references/app-contributing.md`, follow those first.

This file should stay shorter than the former `app/best-practices.md`. Keep only recurring architectural heuristics here, not product-layout decisions or roadmap priorities.

## State Ownership

- Expose shared sigma instances through Preact Context hooks instead of threading them through intermediate component props.
- Use local component state only for short-lived UI concerns such as hover, focus, popovers, and splitter sizes.
- Keep the minimal source of truth. Derive cheap display values during render instead of duplicating them in state.
- Prefer explicit status fields or discriminated unions over piles of booleans, and model non-trivial transitions explicitly.
- Normalize shared records by durable ids or refs, then derive ordered lists, filters, and view models from that normalized state.
- Expose semantic actions such as `openOrFocusTab` or `submitLaunch` rather than generic setters that leak internal state shape.

## Hooks And Async Work

- Avoid `useMemo` and `useCallback` by default. Use them only for known hot paths or real identity-sensitive APIs.
- Use `useEffect` for lifecycle-bound setup or cleanup and narrow bridge or bootstrap work, not for prop mirroring, derived state, or generic watch-and-sync logic.
- Use refs for imperative DOM or resource access, not as a hidden state store.
- Keep async work out of presentational components. Prefer state modules, semantic actions, or the established app data-loading layer over fetch-on-render in view components.
- Let `@tanstack/preact-query` own loading and error state for async reads instead of mirroring those flags in sigma modules.

## Cross-Domain Coordination

- Let one state module call another through explicit actions or injected adapters when a workflow crosses boundaries.
- Keep host RPC, filesystem reads, store persistence, and daemon operations behind state modules or service adapters.
- Only operate on local roots the user explicitly adds to the app's project scope, and pass project identity through state and tab payloads for project-scoped workflows.
- Maintain one authenticated user-scoped activity stream and fan normalized events into inbox, sessions, and pull-request state.

## Alignment

- If a planned feature conflicts with `spec/app.md` or shared contracts, call out the mismatch before implementing around it.
- When implementation changes a domain concept, update the relevant plan docs and `app/glossary.md` in the same change.
