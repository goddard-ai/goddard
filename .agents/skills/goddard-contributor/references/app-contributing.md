# App Contributing

Use this reference for app-local contribution guidance that intentionally does not live in `app/AGENTS.md`.

For the app bootstrap rules already promoted into `app/AGENTS.md`, follow that file first.

## Additional Read Triggers

- Read the relevant upstream package or platform docs when working with third-party APIs or patterns.

## Architecture

- Prefer shared host adapters over ad hoc browser-to-host calls so new desktop capabilities follow one transport boundary.

## Syntax And Naming

- Do not end `preact-sigma` module names with `State`.
- Prefer Preact Context over prop drilling `preact-sigma` instances through component trees.
- Do not use rollout labels such as `sprint-1` in API descriptions, code-facing comments, or other durable implementation naming.
- For component-local Panda classes, move non-trivial static `css(...)` calls into a sibling `*.style.ts` module that `export default`s a class map.
- Keep tiny single-use wrappers inline when they have only a few declarations, no pseudo selectors, no complex token usage, and no clearer semantic name than the inline properties themselves.
- In files that already use a sibling `*.style.ts`, add new non-trivial static classes there and keep only the trivial exceptions inline.
- Keep prop- or state-derived values out of `*.style.ts`. Use render-local `style={...}` objects or other local logic for dynamic values.
- Name extracted style entries by element role or intent, not by incidental visual details, and keep the exported object roughly ordered with the JSX structure.
- Use `styled(...)` for reusable presentational primitives shared within a feature or surface, not for singleton page shells or one-off elements.
- When a sigma instance helper type is essentially `InstanceType<typeof MySigma>`, export it as a same-name interface: `export interface MySigma extends InstanceType<typeof MySigma> {}`.
