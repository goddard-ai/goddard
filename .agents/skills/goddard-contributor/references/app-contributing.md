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
- Use inline `css(...)` calls by default. Only hoist a Panda class when the same class value is reused in multiple places.
- When a sigma instance helper type is essentially `InstanceType<typeof MySigma>`, export it as a same-name interface: `export interface MySigma extends InstanceType<typeof MySigma> {}`.
