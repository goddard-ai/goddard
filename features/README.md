# Feature Packages

`features/` contains internal full-stack product capability packages. Feature
packages are private workspace packages; they are not published plugin products
and are not loaded dynamically at runtime.

Use the workspace scaffold before adding a new package:

```sh
bun run scaffold:feature
bun run scaffold:feature --name my-feature --layers daemon,sdk,app --schema --daemon-ipc --dry-run
```

The scaffold creates only the selected layer entrypoints. It does not register
the feature in public composition roots; registration is the point where a
feature becomes part of a supported product surface.

## Boundaries

- Feature packages own product-specific schemas, daemon IPC contracts, daemon
  handlers, SDK namespace factories, app metadata, and feature-owned helpers.
- Layer packages own the substrate that composes and runs those contributions:
  SDK construction, daemon process and persistence substrate, app shell
  placement, backend authority, and shared diagnostics.
- Feature packages import thin plugin support packages such as
  `@goddard-ai/sdk-plugin`, `@goddard-ai/daemon-plugin`, and
  `@goddard-ai/app-plugin`, not public composition roots such as
  `@goddard-ai/sdk`.
- Feature packages must not circularly depend on other feature packages.
  Daemon feature interop goes through explicit `consumes` declarations and
  first-class `context.<feature>` extensions.

`features/inbox` is the current reference daemon + SDK + app feature package.
