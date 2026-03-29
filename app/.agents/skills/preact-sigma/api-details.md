# preact-sigma API Details

This file contains extended API documentation for `preact-sigma`, an extension to `SKILL.md`.

## Runtime Exports

Import runtime APIs from `preact-sigma`.

```typescript
import {
  SigmaType,
  action,
  batch,
  computed,
  effect,
  freeze,
  immerable,
  isSigmaState,
  listen,
  query,
  replaceState,
  setAutoFreeze,
  snapshot,
  untracked,
  useListener,
  useSigma,
} from "preact-sigma";
```

## Type Exports

- `AnyDefaultState`: Describes the object accepted by `.defaultState(...)`.
- `AnyEvents`: Describes an event map from event names to payload objects or `void`.
- `AnyResource`: Describes a supported setup cleanup resource.
- `AnySigmaState`: Describes the public shape shared by all sigma-state instances.
- `AnySigmaStateWithEvents`: Describes a sigma-state instance with a typed event map.
- `AnyState`: Describes the top-level state object for a sigma type.
- `InferEventType`: Infers the supported event names for a target used with `listen(...)` or `useListener(...)`.
- `InferListener`: Infers the listener signature for a target and event name.
- `InferSetupArgs`: Infers the `setup(...)` argument list for a sigma-state instance.
- `SigmaObserveChange`: Describes the object received by `.observe(...)` listeners.
- `SigmaObserveOptions`: Describes the options object accepted by `.observe(...)`.
- `SigmaState`: Describes the public instance shape produced by a configured sigma type.

## Public Instance Shape

A sigma-state instance exposes:

- one readonly enumerable own property for every state property
- one tracked non-enumerable getter for every computed
- one method for every query
- one method for every action
- `get(key): ReadonlySignal<...>` for state-property and computed keys
- `setup(...args): () => void` when the builder has at least one setup handler
- `on(name, listener): () => void`
- `Object.keys(instance)` includes only top-level state properties

## Reactivity Model

- each top-level state property is backed by its own Preact signal
- public state reads are reactive
- signal access is reactive, so reading `.value` tracks like any other Preact signal read
- computed getters are reactive and lazily memoized
- queries are reactive at the call site, including queries with arguments
- query calls are not memoized across invocations; each call uses a fresh `computed(...)` wrapper and does not retain that signal

## Actions details

- actions create drafts lazily when reads or writes need draft-backed mutation semantics
- actions may call other actions, queries, and computeds
- same-instance sync nested action calls reuse the current draft
- any other action call starts a different invocation and is a draft boundary
- `emit()` is a draft boundary
- `await` inside an async action is a draft boundary
- `this.commit()` publishes the current draft immediately
- `this.commit()` is only needed when the current action has unpublished draft changes and is about to cross a draft boundary
- a synchronous action does not need `this.commit()` when it finishes without crossing a draft boundary
- declared async actions publish their initial synchronous draft on return
- after an async action resumes from `await`, top-level reads of draftable state and state writes may open a hidden draft for that async invocation
- non-async actions must stay synchronous; if one returns a promise, sigma throws
- if an async action reaches `await` or `return` with unpublished changes, the action promise rejects when it settles
- if an action crosses a boundary while it owns unpublished changes, sigma throws until `this.commit()` publishes them
- if a different invocation crosses a boundary while unpublished changes still exist, sigma warns and discards them before continuing
- successful publishes deep-freeze draftable public state and write it back to per-property signals while auto-freezing is enabled
- custom classes participate in Immer drafting only when the class opts into drafting with `[immerable] = true`
- actions can emit typed events with `this.emit(...)`
- action wrappers are shared across instances
- action typing only exposes computeds, queries, and actions that were already present when its `.actions(...)` call happened

Nested sigma states stored in state stay usable as values. Actions do not proxy direct mutation into a nested sigma state's internals.

## Events

Events are emitted from actions or setup through `this.emit(name, payload?)`.

Behavior:

- the event map controls allowed event names and payload types
- `void` events emit no payload
- object events emit one payload object
- `.on(name, listener)` returns an unsubscribe function
- listeners receive the payload directly, or no argument for `void` events

## Advanced Utilities

- **`immerable`**: Re-exported from Immer so custom classes can opt into drafting with `[immerable] = true`. Unmarked custom classes stay outside Immer drafting and deep-freezing. Plain objects, arrays, `Map`, and `Set` work by default.
- **`setAutoFreeze(autoFreeze)`**: Controls whether sigma deep-freezes published public state at runtime (enabled by default).
- **`snapshot(instance)`**: Returns a shallow snapshot of an instance's committed public state (does not include computeds, queries, or recurse into nested sigma states).
- **`replaceState(instance, snapshot)`**: Replaces committed public state from a plain snapshot object, notifying observers.
- **`query(fn)`**: Creates a standalone tracked query helper, useful for helpers that are large or rarely needed and can live outside the sigma state.

## Hooks & Listeners

- **`useSigma(create, setupParams?)`**: Creates one sigma-state instance for a component and manages setup cleanup. Calls `create()` once per mounted instance. Reruns setup when params change.
- **`listen(target, name, listener)`**: Adds an event listener (for sigma-states or DOM targets) and returns a cleanup function.
- **`useListener(target, name, listener)`**: Attaches an event listener inside a component via `useEffect`. Handles unsubscribe automatically.
- **`isSigmaState(value)`**: Checks whether a value is a sigma-state instance.
