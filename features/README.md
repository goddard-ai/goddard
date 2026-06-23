# Feature Packages

`features/` contains internal full-stack product capability packages. Feature
packages are private workspace packages; they are not published plugin products
and are not loaded dynamically at runtime.

Use the workspace scaffold before adding a new package:

```sh
pnpm run scaffold:feature
pnpm run scaffold:feature -- --name my-feature --layers daemon,sdk --schema --daemon-ipc --dry-run
```

The scaffold creates only the selected layer entrypoints. It does not register
the feature in public composition roots; registration is the point where a
feature becomes part of a supported product surface.

## Boundaries

- Feature packages own product-specific schemas, daemon IPC contracts, daemon
  handlers, SDK namespace factories, feature-owned helpers, and
  feature-specific JSON config schemas.
- Feature schemas are self-declared from the package's own `./schema`
  entrypoint. `@goddard-ai/schema` is reserved for core
  daemon/backend/shared substrate schemas, not feature-owned product schemas.
- Layer packages own the substrate that composes and runs those contributions:
  SDK construction, daemon process and persistence substrate, backend
  authority, JSON config file loading/persistence, and shared diagnostics.
- Feature packages import thin plugin support packages such as
  `@goddard-ai/sdk-plugin` and `@goddard-ai/daemon-plugin`, not public
  composition roots such as `@goddard-ai/sdk`.
- Feature packages must not circularly depend on other feature packages.
  Daemon feature interop goes through explicit `consumes` declarations and
  first-class `context.<feature>` extensions.
- Layer entrypoints such as `src/daemon.ts` should stay as thin plugin
  entrypoints. Feature-owned daemon implementation can live in focused modules
  under `src/daemon/` and be reached from the entrypoint.
- Backend-capable features export a backend plugin definition from `./backend`.
  The plugin declares that feature's backend routes, event definitions, and event
  sources; default product composition imports the plugin definition rather than
  individual contribution constants.
- Provider feature tests own provider-specific parsing, signature verification,
  filtering, provenance, and raw upstream fixtures. Core backend tests should
  cover composition, publication, authorization, and fanout without duplicating
  provider fixture variants.
- JSON configuration is not a feature package. Daemon plugins may contribute
  namespaced config fragments, but daemon/core packages own
  `~/.goddard/config.json`, project-level `.goddard/config.json`, merge
  precedence, validation, persistence, and hot reload.
- App plugin packaging is deferred. App shell placement, workbench tab
  metadata, command routing, app state composition, query cache, and app-only UI
  composition remain app-owned substrate until a separate app composition sprint
  introduces a reviewed app composition root.

## Current Feature Boundaries

`features/action` owns action schemas, named action config schema metadata,
daemon action IPC handlers, named action resolution from local/global config
roots, and SDK action namespace construction. Config file storage and hot
reload remain daemon/core substrate, and session execution is consumed through
the first-class session feature extension.

`features/auth` owns auth schemas, backend auth route contracts, daemon auth IPC
handlers, and SDK auth namespace construction. Backend storage, GitHub device
flow persistence, daemon token persistence, and HTTP/router substrate remain in
core packages.

`features/github` owns GitHub provider integration: the `/webhooks/github`
backend route, raw webhook parsing, signature verification, bot filtering,
provider provenance, and GitHub App helpers. It maps supported GitHub webhook
payloads into remote-repo-owned backend event envelopes but does not own
provider-agnostic event definitions, event sources, authorization, or pull
request workflow behavior.

`features/file-search` owns file search schemas, file-search daemon IPC
handlers, SDK file-search namespace construction, and daemon-local file/folder
entry discovery for app composer suggestions. App composer UI, prompt chip
serialization, and session execution remain outside the file-search feature
boundary.

`features/agent` owns launchable ACP agent discovery, registry and
config-declared catalog merge behavior, local launch visibility markers,
managed install status and process-spec resolution, managed install usage/update
policy, daemon IPC handlers, and SDK agent namespace construction.
Root config file loading, daemon process lifecycle, generic persistence, and
session process lifecycle remain core or session-feature substrate.

`features/inbox` owns inbox IPC, SDK namespace construction, inbox manager
logic, inbox metadata resolution, and inbox item state transitions. Daemon
persistence remains core substrate.

`features/loop` owns loop schemas, loop IPC, daemon loop manager/runtime,
packaged-loop resolution from root config, SDK loop namespace construction, and
loop manager/runtime modules. JSON config file loading/persistence, daemon
process lifecycle, and session lifecycle mechanics remain core or
session-feature substrate.

`features/remote-repo` owns provider-agnostic remote repository backend event
vocabulary, event envelope producers, and event source authorization. It does
not own provider-specific webhook routes.

`features/pull-request` owns pull-request schemas, backend PR routes, backend
daemon PR IPC handlers, SDK PR namespace construction, git-backed PR request
resolution, PR feedback automation, and PR inbox attention behavior. Remote
repository provider event production, backend transport, daemon IPC server
mechanics, and daemon persistence substrate remain outside the pull-request
feature boundary.

`features/session` owns session feature schemas, session-owned daemon IPC
routes, session lifecycle implementation modules, SDK session method fragments,
and the first-class daemon `context.session` extension that downstream feature
packages consume. Low-level linked-worktree substrate and third-party worktree
provider contracts remain in core packages.

`features/workforce` owns workforce schemas, workforce IPC, daemon workforce
configuration discovery and initialization, workforce manager/runtime modules,
and SDK workforce namespace construction. The standalone
`workforce/` CLI remains a public client that talks to the composed SDK/daemon
surface, while daemon process lifecycle, request context, persistence substrate,
and the session feature extension remain outside the workforce feature boundary.
