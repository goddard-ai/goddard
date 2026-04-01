# App Best Practices

These practices are derived from the current app plans, `spec/`, and `app/glossary.md`. They are written for implementation work in the Electrobun app, not for backend or SDK packages.

For app bootstrap rules already promoted into `app/AGENTS.md` and `references/app-contributing.md`, follow those first.

## Sections

- Workspace shell
- State shape
- Data and async flows
- Product workflow patterns
- Integrations and side effects
- Naming and alignment

## Workspace Shell

### Keep the workspace tab-first

- Treat the app as one non-closable primary workbench view plus closable detail tabs.
- Open drill-down content by focusing an existing tab or creating one new tab, not by inventing parallel page stacks.
- Use left-hanging sidebars only as page-local filters or navigators, not as independent app panels.
- Why: preserve one consistent navigation model and keep the workspace aligned with the current shell plan and app glossary.

## State Shape

### Keep shared state out of components

- Put every complex domain in a dedicated `preact-sigma` state module.
- Expose shared sigma instances through Preact Context hooks instead of threading them through intermediate component props.
- Let components render props and call semantic actions. Do not let them own cross-tab data, persistence, or IPC.
- Use local component state only for short-lived UI concerns such as hover, focus, popovers, and splitter sizes.
- Why: the planned app has many cross-cutting concerns such as auth, project scope, tab caching, realtime activity, and action applicability. Those become brittle quickly if they leak into view components.

### Keep hook usage deliberate

- Avoid `useMemo` and `useCallback` by default. Use them only for known hot paths or real identity-sensitive APIs.
- Use `useEffect` for lifecycle-bound setup or cleanup and narrow bridge or bootstrap work, not for prop mirroring, derived state, generic watch-and-sync logic, or routine fetch-on-render.
- Prefer event-driven updates and explicit actions over reactive synchronization.
- Use refs for imperative DOM or resource access, not as a hidden state store.
- Why: the app favors explicit state modules, semantic actions, and render-time derivation. Overusing hooks turns straightforward flows into lifecycle-driven synchronization.

### Model UI state from minimal truth

- Keep state local when possible and store the minimal source of truth.
- Derive cheap display values during render instead of duplicating them in state.
- Prefer explicit status fields or discriminated unions over piles of booleans, and model non-trivial transitions explicitly.
- Do not keep state that no rendered component currently uses.
- Remove sigma fields and actions that only support unused UI paths.
- Do not mint separate ids when a stable source value already exists.
- Trim path strings at input boundaries, not inside shared domain state.
- Why: minimal, explicit state keeps impossible states harder to represent and makes tab-heavy UI flows easier to reason about.

### Normalize shared records by stable identity

- Store shared entities in maps keyed by durable ids or refs, then derive ordered lists, filters, and view models from that normalized state.
- Keep raw contract values intact and derive UI-only labels separately.
- Why: the same session, pull request, task, proposal, project, or action may appear in multiple pages, search results, and tabs at once. Normalization prevents drift and makes cross-view updates predictable.

### Use semantic actions, never ad hoc setters

- Expose intentful actions such as `openOrFocusTab`, `submitLaunch`, `startDeviceFlow`, `addProject`, or `submitPullRequest`.
- Avoid generic setters that force callers to know internal state shape.
- Why: semantic actions make state modules easier to change without breaking callers and communicate the real domain transition being requested.

### Coordinate domains through explicit state actions

- Let one state module call another through well-defined actions or injected adapters when a workflow crosses boundaries.
- Do not have components manually orchestrate multi-step flows across auth, tabs, projects, actions, and realtime state.
- Why: the most important app flows are cross-domain by design: auth gates PR actions, session launch depends on projects and actions, and search opens tabs across many domains.

## Data And Async Flows

### Keep async work out of presentational components

- Prefer state modules, semantic actions, or the established app data-loading layer over fetch-on-render inside view components.
- Keep presentational components focused on rendering props and invoking actions.
- Handle loading, empty, error, and success states explicitly, and account for cancellation or races when user input can trigger overlapping requests.
- Let `@tanstack/preact-query` own loading and error state for async reads instead of mirroring those flags in sigma modules.
- Why: async behavior in views quickly turns into stale-data and interleaving bugs. Centralizing it keeps ownership, retries, and identity clearer.

### Keep the app thin over shared contracts

- Reuse SDK, daemon, schema, and shared configuration contracts instead of inventing app-only payloads or storage models.
- If a shared concept already exists in `spec/`, `core/schema`, or `core/sdk`, adapt it for presentation rather than redefining it locally.
- Why: the repository is explicitly SDK-first. The app should be a control surface and renderer, not a parallel product logic layer.

## Product Workflow Patterns

### Treat projects as the app's machine-wide scope

- Only operate on local roots the user explicitly adds to the app's project scope.
- Pass project identity through state and tab payloads whenever a workflow is project-scoped.
- Why: the app is intentionally machine-wide, but it still needs a bounded, explicit set of local projects for discovery, actions, loops, specs, tasks, and roadmap work.

### Make auth lazy and protected-action based

- Gate only the workflows that require backend or GitHub identity.
- Preserve the user's pending intent, run auth, then resume the protected action.
- Keep local-only workflows usable while unauthenticated.
- Why: this matches the spec's lazy-auth model and avoids turning the whole app into a login wall.

### Use one shared realtime subscription

- Maintain one authenticated user-scoped activity stream and fan normalized events into inbox, sessions, and pull-request state.
- Do not open separate event streams per page or repository.
- Why: the spec and ADRs define realtime delivery as user-scoped rather than repository-scoped. Centralizing the stream keeps reconnection and event routing tractable.

### Make detail tabs stable and resumable

- Give every tab kind a stable identity derived from its underlying entity or workflow.
- Persist tab snapshots and keep hidden tabs restorable from in-memory cache.
- Reopen existing tabs instead of creating duplicates for the same entity.
- Why: the app is explicitly tab-centric, and the user experience falls apart if tabs duplicate, lose draft state, or cold-start unnecessarily.

### Prefer list-first pages for operator workflows

- Use sortable, filterable lists for tasks, roadmap proposals, pull requests, projects, actions, and loops.
- Avoid board-specific abstractions unless the product direction changes.
- Why: the current MVP direction is list-first, and the planned pages already lean on left-side filters plus full-width list layouts.

### Use contextual actions consistently

- Put the action dropdown in tab headers and toolbars where the current tab context is available.
- Always distinguish between global actions and actions applicable to the current tab.
- Let actions prefill session launch or other workflow-specific forms instead of bypassing those flows.
- Why: actions are a first-class app concept now, and their usefulness depends on predictable context resolution rather than hidden heuristics.

### Keep creation and launch flows modal when the plan says so

- Use modal flows for session launch, device auth, project add, loop start, and PR or action creation where the current plans already define them that way.
- Do not smuggle those flows into inline composers or ephemeral header controls.
- Why: these workflows need validation, intent confirmation, and sometimes protected-action gating. A modal boundary keeps them explicit and reusable.

## Integrations And Side Effects

### Put side effects behind adapters

- Encapsulate daemon clients, filesystem reads, store persistence, and other host effects behind injected service or adapter boundaries.
- Keep view and state modules independent of raw transport setup wherever possible.
- Why: adapter boundaries improve testability, make future host changes less invasive, and keep state modules focused on domain behavior.

### Do not let UI components call desktop host APIs directly

- Trigger host RPC, store writes, dialog opens, and daemon operations from state modules or service adapters.
- Keep component props declarative and callback-based.
- Why: direct host calls from arbitrary components make it much harder to reason about lifecycle, retries, gating, and persistence.

## Naming And Alignment

### Call out spec mismatches before implementing around them

- If a planned feature conflicts with `spec/app.md`, `app/AGENTS.md`, or shared contracts, document the conflict clearly before adding implementation code.
- Current example: terminal and browser preview plans still assume host capabilities that the current app runtime does not yet expose.
- Why: silent divergence is expensive. The app is still pre-alpha, so alignment is cheaper than cleanup.

### Update nearby docs when concepts change

- When implementation changes a domain concept, update the relevant plan docs and `app/glossary.md` in the same change.
- Do not let component, state, and glossary terminology drift apart.
- Why: the app now has enough moving parts that stale planning docs will quickly mislead future contributors.

### Prefer clear names that match user-facing nouns

- Use names like `ProjectsPage`, `SessionLaunch`, `ContextActionDropdown`, or `PullRequestCompose`.
- Avoid generic buckets such as `manager`, `store`, or `utils` when the module owns a real domain concept.
- Why: the plans are intentionally domain-shaped. Matching the code to those nouns makes ownership easier to recover later.

### Build MVP flows end-to-end before layering deferred features

- Prioritize the planned MVP surfaces first: auth, projects, sessions, actions, pull requests, specs, tasks, roadmap, loops, search, inbox, diff, MDX, and the shell itself.
- Leave deferred work such as dedicated PR-feedback UI, workforce UI, settings, extension catalog, and richer project dashboards in the recommendation backlog until the core flows exist.
- Why: the app has a wide surface area already. Finishing the thin vertical slices is more valuable than widening the backlog further.
