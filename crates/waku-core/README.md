# waku-core

`waku-core` preserves the existing `waku_core::` API as a compatibility facade
for [`waku-daemon`](../waku-daemon). The daemon library hosts provider sessions,
task persistence, workspace services, and daemon-owned settings through the
extracted runtime crates. Desktop clients use [`waku-client`](../waku-client).

The transport is an authenticated WebSocket (loopback by default). Requests
have stable UUIDs for idempotency; session events carry monotonically
increasing sequence numbers and runtime-generation IDs. The server keeps a
bounded replay journal, and stale events or commands from a replaced runtime
are ignored.

`DaemonClient` lives in [`waku-client`](../waku-client), which is what Goddard
Desktop depends on. `serve` and `WakuBackend` are used by the `goddard-daemon`
binary in `waku-daemon`.

Configuration ownership is explicit:

- the desktop owns `~/.goddard/app.json` in Release and checkout-local
  `temp/app.json` in Debug;
- the daemon owns `~/.goddard/settings.json`.

Task SQLite rows and durable attachment materializations are daemon-owned as
well. Client-local attachment paths are upload inputs or caches only; provider
prompts and persisted messages use daemon-issued paths and references.
Projectless task directories are daemon-owned too and live beneath
`~/.goddard/projects`.

The protocol types use Serde's tagged JSON representation and are exported by
`waku-protocol`, including checked-in TypeScript bindings.
