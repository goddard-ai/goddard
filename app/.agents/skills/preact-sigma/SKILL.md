---
name: preact-sigma
description: Find the right preact-sigma docs and examples. Use when code imports `preact-sigma` and you need to jump to the relevant documentation surface under `node_modules/preact-sigma/`.
---

# preact-sigma

Treat `node_modules/preact-sigma/` as the package root.

Use this skill as a supplemental coding checklist, not as the source of truth. The docs shipped in
`node_modules/preact-sigma/` must be enough to use the package without this skill.

## Documentation Map

- Read `node_modules/preact-sigma/README.md` for purpose, installation, and the high-level documentation map.
- Read `node_modules/preact-sigma/docs/context.md` for concepts, lifecycle, invariants, and API selection.
- Read `node_modules/preact-sigma/examples/basic-counter.ts` for the quick-start class shape.
- Read `node_modules/preact-sigma/examples/command-palette.tsx` for an advanced end-to-end example.
- Read `node_modules/preact-sigma/examples/async-commit.ts`, `node_modules/preact-sigma/examples/observe-and-restore.ts`, and `node_modules/preact-sigma/examples/setup-act.ts` for focused API patterns.
- Read `node_modules/preact-sigma/dist/index.d.mts` for exact exported signatures and the published API comments.

## Coding Checklist

Apply these defaults when writing or changing application code that uses `preact-sigma`:

- Put reactive, persisted, subscribed, or UI-rendered values in top-level state.
- Use ECMAScript `#private` fields only for ephemeral instance storage that should not persist or invalidate reactive reads by itself.
- Define state in a named `State` type, pass it to `Sigma<State>` or `SigmaTarget<Events, State>`, then merge `interface Model extends State {}` after the class for direct state property typing.
- Use getters for argument-free derived reads and `@query` for argument-based reads.
- Mutate state in ordinary prototype methods; prototype methods are actions by default unless marked with `@query`.
- Call `this.commit()` before `await`, before an action promise resolves, before `this.emit(...)`, or before invoking another sigma instance's action.
- Put side effects in `onSetup(...)`, not constructors.
- Call actions directly from setup-owned callbacks. Use `this.act(function () { ... })` only for ad hoc public state mutations that are not already inside an action; private field mutations do not need it.
- Use `useSigma(...)` for component-owned model instances.
- Use `preact-sigma/persist` for storage policy instead of putting persistence directly in model classes.
