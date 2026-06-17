---
name: app-implementation-patterns
description: Apply Goddard app implementation patterns for state ownership, contextual mutations, hooks, async work, cross-domain coordination, surface composition, TSRX organization, and spec alignment.
---

# App Implementation Patterns

Use this skill for implementation work in the Electrobun app, not for backend or SDK packages. These practices are derived from the current app plans, `spec/`, and `app/glossary.md`.

## State Ownership

- Expose shared sigma instances through Preact Context hooks instead of threading them through intermediate component props.
- Use local component state only for short-lived UI concerns such as hover, focus, popovers, and splitter sizes.
- Keep the minimal source of truth. Derive cheap display values during render instead of duplicating them in state.
- Prefer explicit status fields or discriminated unions over piles of booleans, and model non-trivial transitions explicitly.
- Normalize shared records by durable ids or refs, then derive ordered lists, filters, and view models from that normalized state.
- Expose semantic actions such as `openOrFocusTab` or `submitLaunch` rather than generic setters that leak internal state shape.

## Contextual Mutations

- Use `createMutationsProvider` from `app/src/lib/mutations-provider.tsx` when a feature surface owns semantic UI or data mutations that deeper descendants need to invoke.
- Put feature-surface providers in the feature's `mutations.ts` module, name them after the owner such as `SessionsPageMutations`, mount them at the owning surface, and consume them with `useMutations(...)` in rows, cards, or other descendants.
- Reach for contextual mutations when prop threading would make presentational descendants carry parent-owned callbacks like `openSession`, `removeProject`, or `openProjectTab` through intermediate components.
- Keep ordinary props for generic controls, one-level same-file callbacks, input/select/dialog plumbing, and tiny local interactions where context would hide the data flow.
- Do not use contextual mutation providers as replacements for Sigma model methods or SDK/cache write helpers.

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

## Surface Composition

- Keep shared presentational primitives near the surface that owns them when multiple sections reuse the same layout or control treatment.
- Keep section files focused on semantics, labels, and state wiring, and keep generic surface layout and control styling in the owning surface.
- Prefer plain document-flow utility surfaces over decorative headers, hero treatments, or stacked containers unless the product explicitly requires stronger framing.
- Inline single-use local components unless extraction gives the JSX a meaningful reusable concept.
- Avoid relay components whose main job is to rename loaded data or forward props into a single child.
- For component-local Panda classes, move non-trivial static `css(...)` calls into a sibling `*.style.ts` module that `export default`s a class map.
- Keep tiny single-use wrappers inline when they have only a few declarations, no pseudo selectors, no complex token usage, and no clearer semantic name than the inline properties themselves.
- In files that already use a sibling `*.style.ts`, add new non-trivial static classes there and keep only the trivial exceptions inline.
- Keep prop- or state-derived values out of `*.style.ts`. Use render-local `style={...}` objects or other local logic for dynamic values.
- Name extracted style entries by element role or intent, not by incidental visual details, and keep the exported object roughly ordered with the JSX structure.
- Use `styled(...)` for reusable presentational primitives shared within a feature or surface, not for singleton page shells or one-off elements.

## TSRX Component Organization

- Put hooks, derived values, and event handlers near the JSX that uses them. TSRX supports hooks anywhere in component scope, including inside JSX expression blocks and conditional branches.
- Do not hoist hooks, variables, or helper declarations to the top of a component solely to satisfy React-style hook ordering habits. Keep them in the smallest readable scope that still preserves the state lifetime and dependencies they need.
- Move purely presentational derived values into the JSX subtree that renders them.
- Avoid a large top-of-component variable block when the values are only meaningful in one local region.
- Avoid local aliases that only hide useful ownership or reactivity context such as `props.*`, `page.*`, or `*.value`.
- Keep locals when they name domain meaning, avoid duplicated non-trivial logic, or are reused enough to improve clarity.
- Prefer inline JSX callbacks for tiny single-use handlers.
- For larger single-use handlers, declare them in the nearest JSX or control-flow block that uses them.
- Use arrow function bindings for handlers declared inside `if` blocks so their scope matches the block.
- Pass async event handlers directly, such as `onClick={saveChanges}`, instead of wrapping them only to discard the returned promise.
- Keep explicit guards only when they express real domain or UX rules. Do not duplicate generic task concurrency checks at every call site.
- Inline helpers that are single-use and low complexity, especially when their extraction hides nearby context.
- Keep top-level helpers for reusable presentation logic, validation, type guards, or formatting that is used in multiple places or would distract from the JSX.
- Do not create explicit helper types when the model or factory type can be inferred without duplication.

## Alignment

- If a planned feature conflicts with `spec/app.md` or shared contracts, call out the mismatch before implementing around it.
- When implementation changes a domain concept, update the relevant plan docs and `app/glossary.md` in the same change.
