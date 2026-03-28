# App Best Practices

- Scope:
  - These practices are derived from the current app plans, `spec/`, and the app glossary.
  - They are written for future implementation work in the Tauri app, not for backend or SDK packages.

- Keep the workspace tab-first.
  - What:
    - Treat the app as one non-closable primary workbench view plus closable detail tabs.
    - Open drill-down content by focusing an existing tab or creating one new tab, not by inventing parallel page stacks.
    - Use left-hanging sidebars only as page-local filters or navigators, not as independent app “panels”.
  - Why:
    - This preserves one consistent navigation model and keeps the workspace aligned with the current shell plan and app glossary.

- Keep shared state out of components.
  - What:
    - Put every complex domain in a dedicated `preact-sigma` state module.
    - Let components render props and call semantic actions; do not let them own cross-tab data, persistence, or IPC.
    - Use local component state only for short-lived UI concerns such as hover, focus, popovers, and splitter sizes.
  - Why:
    - The planned app has many cross-cutting concerns such as auth, repository registry, tab caching, realtime activity, and action applicability. Those become brittle quickly if they leak into view components.

- Normalize shared records by stable identity.
  - What:
    - Store shared entities in maps keyed by durable ids or refs, then derive ordered lists, filters, and view models from that normalized state.
    - Keep raw contract values intact and derive UI-only labels separately.
  - Why:
    - The same session, pull request, task, proposal, repository, or action may appear in multiple pages, search results, and tabs at once. Normalization prevents drift and makes cross-view updates predictable.

- Use semantic actions, never ad hoc setters.
  - What:
    - Expose intentful actions such as `openOrFocusTab`, `submitLaunch`, `startDeviceFlow`, `addManagedRepository`, or `submitPullRequest`.
    - Avoid generic setters that force callers to know internal state shape.
  - Why:
    - Semantic actions make state modules easier to change without breaking callers and communicate the real domain transition being requested.

- Coordinate domains through explicit state actions.
  - What:
    - Let one state module call another through well-defined actions or injected adapters when a workflow crosses boundaries.
    - Do not have components manually orchestrate multi-step flows across auth, tabs, repositories, actions, and realtime state.
  - Why:
    - The most important app flows are cross-domain by design: auth gates PR actions, session launch depends on repositories and actions, and search opens tabs across many domains.

- Keep the app thin over shared contracts.
  - What:
    - Reuse SDK, daemon, schema, and shared configuration contracts instead of inventing app-only payloads or storage models.
    - If a shared concept already exists in `spec/`, `core/schema`, or `core/sdk`, adapt it for presentation rather than redefining it locally.
  - Why:
    - The repository is explicitly SDK-first. The app should be a control surface and renderer, not a parallel product logic layer.

- Treat managed repositories as the app’s machine-wide scope.
  - What:
    - Only operate on repositories the user explicitly adds to the repository registry.
    - Pass repository identity through state and tab payloads whenever a workflow is repository-scoped.
  - Why:
    - The app is intentionally machine-wide, but it still needs a bounded, explicit set of repositories for discovery, actions, loops, specs, tasks, and roadmap work.

- Make auth lazy and protected-action based.
  - What:
    - Gate only the workflows that require backend or GitHub identity.
    - Preserve the user’s pending intent, run auth, then resume the protected action.
    - Keep local-only workflows usable while unauthenticated.
  - Why:
    - This matches the spec’s lazy-auth model and avoids turning the whole app into a login wall.

- Use one shared realtime subscription.
  - What:
    - Maintain one authenticated user-scoped activity stream and fan normalized events into inbox, sessions, and pull-request state.
    - Do not open separate event streams per page or repository.
  - Why:
    - The spec and ADRs define realtime delivery as user-scoped rather than repository-scoped. Centralizing the stream keeps reconnection and event routing tractable.

- Make detail tabs stable and resumable.
  - What:
    - Give every tab kind a stable identity derived from its underlying entity or workflow.
    - Persist tab snapshots and keep hidden tabs restorable from in-memory cache.
    - Reopen existing tabs instead of creating duplicates for the same entity.
  - Why:
    - The app is explicitly tab-centric, and the user experience falls apart if tabs duplicate, lose draft state, or cold-start unnecessarily.

- Prefer list-first pages for operator workflows.
  - What:
    - Use sortable, filterable lists for tasks, roadmap proposals, pull requests, repositories, actions, and loops.
    - Avoid board-specific abstractions unless the product direction changes.
  - Why:
    - The current MVP direction is list-first, and the planned pages already lean on left-side filters plus full-width list layouts.

- Use contextual actions consistently.
  - What:
    - Put the action dropdown in tab headers and toolbars where the current tab context is available.
    - Always distinguish between global actions and actions applicable to the current tab.
    - Let actions prefill session launch or other workflow-specific forms instead of bypassing those flows.
  - Why:
    - Actions are a first-class app concept now, and their usefulness depends on predictable context resolution rather than hidden heuristics.

- Keep creation and launch flows modal when the plan says so.
  - What:
    - Use modal flows for session launch, device auth, repository add, loop start, and PR or action creation where the current plans already define them that way.
    - Do not smuggle those flows into inline composers or ephemeral header controls.
  - Why:
    - These workflows need validation, intent confirmation, and sometimes protected-action gating. A modal boundary keeps them explicit and reusable.

- Put side effects behind adapters.
  - What:
    - Encapsulate Tauri plugins, daemon clients, filesystem reads, store persistence, and other host effects behind injected service or adapter boundaries.
    - Keep view and state modules independent of raw transport setup wherever possible.
  - Why:
    - Adapter boundaries improve testability, make future host changes less invasive, and keep state modules focused on domain behavior.

- Do not let UI components call Tauri directly.
  - What:
    - Trigger IPC, store writes, dialog opens, and daemon operations from state modules or service adapters.
    - Keep component props declarative and callback-based.
  - Why:
    - Direct host calls from arbitrary components make it much harder to reason about lifecycle, retries, gating, and persistence.

- Call out spec mismatches before implementing around them.
  - What:
    - If a planned feature conflicts with `spec/app.md`, `app/AGENTS.md`, or shared contracts, document the conflict clearly before adding implementation code.
    - Current example:
      - Terminal and browser preview plans still assume custom Rust capabilities that the current app spec does not yet allow.
  - Why:
    - Silent divergence is expensive. The app is still pre-alpha, so alignment is cheaper than cleanup.

- Update nearby docs when concepts change.
  - What:
    - When implementation changes a domain concept, update the relevant plan docs and `app/glossary.md` in the same change.
    - Do not let component, state, and glossary terminology drift apart.
  - Why:
    - The app now has enough moving parts that stale planning docs will quickly mislead future contributors.

- Prefer clear names that match user-facing nouns.
  - What:
    - Use names like `RepositoriesPage`, `SessionLaunchState`, `ContextActionDropdown`, or `PullRequestComposeState`.
    - Avoid generic buckets such as `manager`, `store`, or `utils` when the module owns a real domain concept.
  - Why:
    - The plans are intentionally domain-shaped. Matching the code to those nouns makes ownership easier to recover later.

- Build MVP flows end-to-end before layering deferred features.
  - What:
    - Prioritize the planned MVP surfaces first: auth, repositories, sessions, actions, pull requests, specs, tasks, roadmap, loops, search, inbox, diff, MDX, and the shell itself.
    - Leave deferred work such as dedicated PR-feedback UI, workforce UI, settings, extension catalog, and richer repository dashboards in the recommendation backlog until the core flows exist.
  - Why:
    - The app has a wide surface area already. Finishing the thin vertical slices is more valuable than widening the backlog further.
