# Codex stable runtime adapter — qualification boundary

`crates/waku-runtime-codex` provides `CodexFactory` for the stage-1
[`RuntimeHost`](stable-runtime.md). It owns a fresh Codex app-server child,
its original stdio workers, native thread/turn IDs, and pending native reply
IDs. The legacy daemon driver is unchanged. The independent executable registers this adapter and the landed Devin
adapter. Actual Codex transport, dynamic-tool continuity, cancellation and
emergency-stop probes passed; integration and remaining release gates are
listed below. The overall stable-runtime outcome is not complete.

## Admission and delivery

The independent host registers the factory; admit with adapter
`codex`. `CodexLaunch` accepts `binary`, `cwd`, `approvalPolicy`, `sandbox`,
optional `developerInstructions`, and `dynamicTools`. The safe defaults are
`untrusted` approval and `read-only` sandbox. Unknown fields, unsupported
policy, a foreign requested provider, and opaque configuration options are
rejected. There is no resume cursor, adoption of an existing child, or
automatic retry after uncertain delivery.

`Submit.input` is a JSON text string. The host journals the command identity
and turn/delivery IDs; the adapter sends one `turn/start` on the original
pipe. Native notifications are captured through the host's ordered event
journal. Controller transfer changes the host epoch, retaining the same
adapter, transport and callback map. Native numeric and string request IDs
remain distinct JSON values when resolving a request.

A reply write is an attempt, not consumption. Dynamic-tool consumption
requires an `item/completed` record matching the original call ID and the
submitted success/content values. A `serverRequest/resolved` notice alone
is retained as raw evidence and does not confirm consumption: native turn
transitions can abandon pending requests. Approval and input answer
consumption remain uncertain, without resend. This narrower guarantee is a
release-acceptance limitation, not full callback qualification. Relevant
upstream behavior is pinned to [Codex 0.162.1 dynamic tool response handling](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/app-server/src/dynamic_tools.rs)
and [pending request resolution](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/app-server/src/request_processors/thread_lifecycle.rs).

Cancellation uses the original native thread/turn IDs. An accepted interrupt
remains uncertain until `turn/completed` reports interruption. The host holds
later submits until explicit `PermitDispatch`. Stop kills the owned process group and waits for child exit and quiesced
reader intake. It does not claim termination of external services. The Unix guardian watches the adapter owner's PID, which must
be the independent host in production. Non-Unix crash cleanup is unqualified.

Model/effort changes apply to subsequent turns. Effective configuration comes
from Codex's `thread/settings/updated`, rather than echoing requested values.
Provider/access changes require a new generation. Native ingress is bounded
at 1 MiB/message, 128 pending native requests/RPCs, and eight queued writes;
write/RPC waits have a 30-second limit. Intake overflow interrupts the child
and publishes a terminal interruption rather than silently continuing.
Native callback deadlines and runtime expiry/revocation remain unqualified.

## Verification

The fixture uses a real subprocess and stdio, with protocol messages as
barriers. It is deterministic provider substitution, not actual-provider
qualification. Reserve `native_builds: 1` with `goddard-agent resource run` (use its installed
`--help` for the reservation syntax), then execute:

```sh
mbx test -p waku-runtime-codex --lib
```

The opt-in actual-provider test requires an authenticated, reachable Codex
backend and `CODEX_BINARY` set to its absolute executable path. It creates a
disposable ephemeral read-only thread and stops only its owned process:

```sh
mbx test -p waku-runtime-codex --lib actual_codex_tool_survives_controller_replacement -- --ignored
```

The initial Codex CLI **0.162.1** probe failed with backend connection refusal.
A later actual-provider probe through the independent pinned executable passed:

```sh
CODEX_BINARY=/absolute/path/to/codex mbx test -p waku-runtime-host --test process actual_codex_callback_through_independent_executable -- --ignored --nocapture
```

The successful run used Codex CLI **0.162.1**, runtime instance
`a502774d-45c6-4fd3-85c6-d2f939c3276d`, native thread
`01a1328e-291b-71d2-a015-49e75e6920c8`, native turn
`01a1328e-2985-7943-99f0-dc0d2dc3932a`. A native dynamic tool waited through 25
controller replacements; one answer was consumed on the original pipe and one
terminal completion included its probe token. Output completed while the client
was detached and replayed after attachment. A subsequent native turn confirmed
effort `low` activation at revision 1 under the original provider/model and
cancellation (`a2718588-dc2c-441d-8380-dd916c0bfe23` runtime turn ID). Emergency
stop ended another pending native tool while detached and fenced late submit.
All probes used disposable read-only threads and cleaned their own children.
No user daemon, app or active product session was restarted.

The executable fixtures additionally prove deduplication of all three native
callback ID classes, ordered replay, preserved live child PID/start count,
instance authentication and crash cleanup with no resume or redispatch. These
fixtures are distinct from actual Codex acceptance. Actual approval/input
consumption, crash-at-every-boundary and concurrent Boss/ordinary/employee reload
storm acceptance remain unqualified.

## Original-owner recovery evidence

Effective configuration carries private adapter options `codexThreadId`,
`ownerGroupPid` and a namespace/session/generation guardian marker. Native
settings activation preserves these options. Reconfiguration changes model and
effort only; callers cannot supply ownership evidence through requested options.
The host journals this metadata before reconciliation can return it.

Recovered Codex cleanup requires either absence of the original group or a live
current-user guardian with the exact durable marker before signalling that group.
Foreign/reused groups and missing evidence remain unconfirmed. Zombie records
are exited processes. Cleanup never starts a provider, resumes a thread or
replays a command. The guardian also fences its group when the stable owner dies.
Non-Unix cleanup remains unqualified. There is no adoption of legacy daemon
children.

## Required next work

The dedicated private IPC/executable prerequisite is implemented in
[`stable-runtime.md`](stable-runtime.md). Codex registration and actual transport qualification
through that process are implemented. This adapter does not add daemon
cutover, watcher integration, agent ingress credentials, resource leases,
sandbox launch, or computer-use helpers. Product-session CLI/MCP routing
must migrate through stage 2C before cutover; inherited Goddard daemon/agent
capability environment variables are removed at launch.

The adapter harness exposed stage-1 host defects, reported through the Boss
blocker mechanism and repaired in the shared prerequisite:

- `pump_session` fabricates an interrupted terminal event on channel
  disconnect after an already confirmed requested termination. This can
  overwrite a truthful stopped projection.
- `route_resolve` can overwrite answer-consumption evidence processed during
  `dispatch`'s synchronous pump, changing its answer state back to rejected.
- `route_admit` and `dispatch` use best-effort persistence for admission and
  dispatch-attempt records, then contact the adapter despite failure to
  persist those records. Intent-before-contact requires a hard gate.

Keep the legacy driver active until callback consumption limits, tool-bridge
qualification and daemon/app reconciliation pass. Do not reparent or resume live legacy sessions to claim
continuity. No user daemon or active product session was restarted for this
qualification work.


The landed opt-in daemon link is not yet adapter-compatible: its generic launch
payload includes unsupported mode/service-tier/preset/MCP/cursor fields; its
projection stringifies native JSON deltas, expects normalized approval/input
subjects, ignores effective configuration and reports every completion as
successful. Stage 2C/reconciliation must own the strict launch mapping, native
projection and stable tool/grant bridge before product cutover. Do not paper over
these conflicts by silently dropping configuration or resuming legacy sessions.

The shared executable selects Devin from `--devin-binary ABS_PATH`,
`WAKU_RUNTIME_DEVIN_BINARY`, or trusted startup PATH and passes the resolved path
to the detached copy. Devin admission remains its existing strict `cwd` payload.
Its factory is registered with the declared adapter ABI; its crash cleanup hook
still defaults to unconfirmed and blocks admission/drain after a live-owner
crash. This is an explicit Devin/shared-host dependency, not Codex acceptance.

Full aggregate metadata/receipt/channel retention qualification remains a shared
host gate. Provider executables/resources must remain available at trusted
launch paths; only the runtime executable is copied into the immutable artifact
store here. No frozen-snapshot requirement was expanded to include Nightly.

Dispatch/initialization RPC waits are bounded but synchronous. A native provider
stall can delay host controls by those waits; a control-latency bound under stall
is not qualified here. The legacy provider-stall investigation is untouched.
