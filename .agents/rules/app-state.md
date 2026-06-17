# App State

Read this ruleset when changing app Sigma models, page models, contexts, component state ownership, workflow transitions, or event ownership.

- Keep complex shared state, persistence, and IPC in `preact-sigma` modules rather than components.
- Keep non-trivial Sigma workflow transitions on the owning Sigma class instead of hiding them behind pure-state reducers.
- When moving helper logic onto a Sigma class, prefer class methods for transformation or mutation-oriented helpers. Keep stateless checks, readers, rankings, and lookups as top-level helpers when that is clearer.
- Do not split one feature's state ownership between a Sigma owner module and a generic `state.ts` module when the Sigma instance can own and test the transitions directly.
- Collapse obvious one-owner state into the Sigma module, or use a narrowly named helper module only when it represents a distinct protocol, parser, or reusable transform boundary.
- Do not suffix app model class names or same-name model interfaces with generic terms such as `Model` or `Runtime`; name the owning concept directly.
- In Sigma classes, add a short human-readable comment to each `#private` field explaining the runtime or bookkeeping it owns and why it stays outside reactive state.
- Do not add private methods that only mirror a public Sigma action with the same parameters. Inline that logic into the public action.
- Do not suffix Sigma owner class or module names with `State`; reserve `State` for explicit state-shape types.
- Use `useSignal()` or local component state for UI state owned by one component or UI primitive, such as open flags, drafts, and ephemeral form status.
- Use page models for page-level UI state shared across a page tree, such as pending tasks, action errors, selected IDs, or one-off page bookkeeping.
- Do not model page UI state in `preact-sigma`; keep `preact-sigma` focused on domain models and workflow state.
- For singleton or page-wide UI components, prefer subscribing to shared Preact context directly instead of threading pass-through JSX props through parent components just to preserve an abstraction boundary.
- Avoid relay-layer parents. If a parent component mostly renames or forwards context-derived values and callbacks into a single app-specific child, move that wiring closer to the child unless the parent is coordinating behavior across multiple subtrees.
- Keep event logic close to the event target. Do not hoist single-use UI event handlers into shell-level components unless coordination outside the local subtree requires it.
- Prefer derived render values over sync effects when the next value can be computed during render.
- Use the local query cache rules in `app-data.md` for shared SDK or daemon-backed reads.
- In UI components, prefer `useListener` from `preact-sigma` over manual `addEventListener` and `removeEventListener` wiring.
- Avoid `forwardRef` for cross-component coordination in `app/` unless there is no simpler option. Prefer semantic actions through context or `src/shared/global-event-hub.ts`.
- Keep custom Preact hooks for state management local to the component that uses them.
- Do not extract single-use state hooks into shared modules.
