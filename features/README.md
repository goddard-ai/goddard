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
  handlers, SDK namespace factories, app metadata, feature-owned helpers, and
  feature-specific JSON config schemas.
- Feature schemas are self-declared from the package's own `./schema`
  entrypoint. `@goddard-ai/schema` is reserved for core
  daemon/backend/shared substrate schemas, not feature-owned product schemas.
- Layer packages own the substrate that composes and runs those contributions:
  SDK construction, daemon process and persistence substrate, app shell
  placement, backend authority, JSON config file loading/persistence, and
  shared diagnostics.
- Feature packages import thin plugin support packages such as
  `@goddard-ai/sdk-plugin`, `@goddard-ai/daemon-plugin`, and
  `@goddard-ai/app-plugin`, not public composition roots such as
  `@goddard-ai/sdk`.
- Feature packages must not circularly depend on other feature packages.
  Daemon feature interop goes through explicit `consumes` declarations and
  first-class `context.<feature>` extensions.
- Layer entrypoints such as `src/daemon.ts` should stay as thin plugin
  entrypoints. Feature-owned daemon implementation can live in focused modules
  under `src/daemon/` and be reached from the entrypoint.
- JSON configuration is not a feature package. Daemon plugins may contribute
  namespaced config fragments, but daemon/core packages own
  `~/.goddard/config.json`, project-level `.goddard/config.json`, merge
  precedence, validation, persistence, and hot reload.

## Current Feature Boundaries

`features/auth` owns auth schemas, backend auth route contracts, daemon auth IPC
handlers, and SDK auth namespace construction. Backend storage, GitHub device
flow persistence, daemon token persistence, and HTTP/router substrate remain in
core packages.

`features/inbox` owns inbox IPC, SDK namespace construction, app metadata, inbox
manager logic, inbox metadata resolution, and inbox item state transitions.
Daemon persistence remains core substrate.

`features/pull-request` owns pull-request schemas, backend PR route and webhook
contracts, daemon PR IPC handlers, SDK PR namespace construction, git-backed PR
request resolution, and PR inbox attention behavior. Backend transport, daemon
IPC server mechanics, and daemon persistence substrate remain in core packages.

`features/session` owns session feature schemas, session-owned daemon IPC
routes, session lifecycle implementation modules, SDK session method fragments,
app session metadata, and the first-class daemon `context.session` extension
that downstream feature packages consume. Low-level linked-worktree substrate
and third-party worktree provider contracts remain in core packages.
