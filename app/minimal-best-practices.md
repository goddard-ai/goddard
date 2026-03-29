# Minimal App Best Practices

- Scope:
  - These practices are for app implementation work.
  - They intentionally stay framework-light and avoid host-specific wording.

- Keep the workspace tab-first.
  - What:
    - Treat the app as one non-closable primary workbench view plus closable detail tabs.
    - Reuse existing tabs before opening duplicates.
    - Use left-hanging sidebars only for page-local filtering or navigation.
  - Why:
    - The app’s navigation model is simplest when drill-down work always resolves into stable tabs.

- Keep complex domain logic in `preact-sigma` state modules.
  - What:
    - Put auth, repositories, tabs, sessions, actions, search, realtime activity, and other shared workflows in dedicated state modules.
    - Keep component-local state limited to short-lived UI concerns such as hover, focus, popovers, and layout sizing.
  - Why:
    - Shared state becomes hard to reason about when it leaks across many components and tabs.

- Normalize shared records by stable identity.
  - What:
    - Store shared entities in maps keyed by durable ids or refs.
    - Derive sorted lists, filters, and view models from normalized state.
  - Why:
    - The same entity can appear in many places at once, and normalization keeps updates consistent.

- Use semantic actions instead of generic setters.
  - What:
    - Prefer actions like `openOrFocusTab`, `submitLaunch`, `startDeviceFlow`, or `addManagedRepository`.
    - Avoid generic mutation APIs that expose raw internal structure.
  - Why:
    - Semantic actions preserve domain intent and make state modules easier to evolve safely.

- Keep the app thin over shared contracts.
  - What:
    - Reuse shared schema, SDK, daemon, and configuration contracts instead of inventing parallel app-only models.
    - Adapt shared contracts for presentation rather than redefining them locally.
  - Why:
    - The app should be a control surface and renderer, not a second implementation of platform behavior.

- Treat repositories as an explicit registry.
  - What:
    - Only operate on repositories the user has explicitly added to the workspace.
    - Carry repository identity through workflows whenever the domain is repository-scoped.
  - Why:
    - The app is machine-wide, but its working set still needs to be intentional and bounded.

- Make auth lazy and protected-action based.
  - What:
    - Gate only the workflows that need authenticated identity.
    - Preserve pending intent, complete auth, then resume the protected action.
    - Keep local-only workflows available while unauthenticated.
  - Why:
    - This keeps the app useful before login and avoids unnecessary auth friction.

- Use one shared realtime subscription.
  - What:
    - Maintain one authenticated user-scoped activity stream.
    - Fan normalized events into inbox, sessions, pull requests, and other consumers from one place.
  - Why:
    - A single realtime owner makes reconnect behavior and event routing much more predictable.

- Make tabs stable and resumable.
  - What:
    - Give every tab kind a stable identity derived from the underlying entity or workflow.
    - Persist tab snapshots and keep hidden tabs cheaply restorable.
  - Why:
    - Tabs are a primary workspace primitive, so they must preserve continuity and avoid duplicate state.

- Prefer list-first operator surfaces.
  - What:
    - Use sortable, filterable lists for tasks, roadmap items, pull requests, repositories, actions, and loops.
    - Avoid introducing board-specific models unless product intent changes.
  - Why:
    - The current app direction is list-first, and consistency across pages reduces UI and state complexity.

- Use contextual actions consistently.
  - What:
    - Surface actions in tab headers and toolbars when current context is available.
    - Distinguish global actions from actions applicable to the current tab.
    - Let actions prefill workflow forms instead of bypassing them.
  - Why:
    - Contextual actions are only reliable when context resolution is explicit and predictable.

- Keep creation and launch workflows modal when planned that way.
  - What:
    - Use modal flows for session launch, auth, repository add, loop start, and other planned create flows.
    - Do not spread those workflows across ad hoc inline inputs.
  - Why:
    - Modal boundaries help with validation, confirmation, and flow reuse.

- Put side effects behind adapters.
  - What:
    - Encapsulate persistence, filesystem access, dialogs, daemon calls, and other external effects behind service or adapter boundaries.
    - Keep views and state focused on domain behavior rather than transport details.
  - Why:
    - Adapter boundaries improve testability and reduce coupling to execution environment details.

- Do not let components perform raw external operations directly.
  - What:
    - Trigger persistence, dialogs, runtime calls, and other external work from state modules or adapters.
    - Keep components declarative and callback-driven.
  - Why:
    - Direct side effects in arbitrary components make lifecycle, retries, and gating much harder to control.

- Call out spec and plan mismatches before implementation.
  - What:
    - If a feature conflicts with existing specs, glossary terms, or plan docs, document the mismatch clearly before coding around it.
  - Why:
    - Pre-alpha is the cheapest phase to fix alignment problems.

- Update nearby docs when concepts change.
  - What:
    - When a domain concept changes, update the relevant plan docs and glossary entries in the same change.
  - Why:
    - The codebase will get harder to navigate if plans, glossary terms, and implementation drift apart.

- Prefer domain-shaped names.
  - What:
    - Use names that match the nouns users and planners already use, such as `RepositoriesPage`, `SessionLaunchState`, or `PullRequestComposeState`.
    - Avoid vague buckets like `manager`, `helpers`, or `misc`.
  - Why:
    - Clear naming makes ownership and intent recoverable without reading deep implementation details.

- Finish MVP flows before expanding the backlog.
  - What:
    - Prioritize the currently planned core surfaces and workflows before adding deferred or speculative features.
  - Why:
    - Thin end-to-end slices are more valuable than a wider but incomplete surface area.
