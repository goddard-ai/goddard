# Devin stable-runtime adapter qualification

`waku-runtime-devin::DevinFactory` retains the original ACP child, transport,
actor, prompt bookkeeping and permission responders inside the stable host.
It consumes the existing [stage-1 contract](stable-runtime.md), with no
contract changes or dependency on daemon, product policy, UI or localization.
This adapter is available for host registration; the legacy daemon driver is
unchanged. It is not a completed Stage 2B release qualification or a cutover.

## Integration interface

The runtime executable chooses a pinned absolute Devin executable:

```rust
let factory = waku_runtime_devin::DevinFactory::new(devin_binary);
```

Register that factory for adapter `devin`. Admission launch data is exactly
`{"cwd":"/absolute/existing/directory"}`. Recovery cursors, arbitrary executable
selection and daemon callback URLs are refused rather than substituted.
Admission currently supports provider defaults only: explicit requested model,
effort and options are refused. Initial effective configuration revision 0
records the advertised current model/effort when present, original native
session ID and owned child PID; it does not echo requested values as facts.

`Submit.input` is a text string (maximum 256 KiB). The adapter holds one
unresolved prompt at a time and reports `Unknown` after queueing it; only a
turn-scoped update, native permission request or successful prompt response
establishes provider acceptance. Session updates retain original ACP payloads.
`ResolveRequest.resolution` is `{"optionId":"an-original-offered-option"}`.
Responders retain their original IDs and live connection; invalid options and
missing responders are refused. A responder can be consumed locally only once.

ACP v1 responder delivery has no provider-consumption acknowledgement. Even a
successful SDK write remains uncertain and emits no fabricated
`RequestAnswered`. Reconnection cannot resend an answer. Daemon/app
reconciliation must retain that uncertainty; completion of a turn does not
prove a particular answer was consumed.

Cancellation names the active runtime turn, sends native `session/cancel`, and
remains unknown until the original prompt returns cancelled. Pending permission
responders are cancelled on cancel, settlement and teardown. An unsolicited
cancel or external stop interrupts rather than inventing success. Explicit
termination remains unconfirmed until the owner observes child exit. Cleanup
runs outside the SDK connection future, so cancellation of that future cannot
skip process cleanup. The Unix watchdog follows the runtime PID, never the
app/daemon PID. It kills the owned provider group after runtime death.

## Bounds and authority limits

Command intake is bounded to eight commands; ACP records to 1 MiB before JSON
allocation; pending original responders to 64. Stderr drains in fixed chunks
without retaining private diagnostics. Full host intake stops the transport
and preserves an ordered terminal backpressure record after the retained
prefix, rather than blocking the SDK response pump or silently dropping data.

Inherited `GODDARD_*`/`WAKU_*` authority and mutable daemon endpoints are removed
from provider environment. No replacement tool route or grant is invented.
Devin-native authority follows provider defaults and remains unqualified for
Goddard grant enforcement. Revocation is unavailable, callback deadlines are
unmeasured, intake cannot pause, and cancellation only qualifies provider
execution. Escaped descendants and remote effects are not confirmed stopped.
Non-Unix crash cleanup has not been qualified.

## Verification

Reserve build capacity and run the deterministic external ACP peer plus real
host journal/controller tests:

```sh
goddard-agent resource run --json '{"resources":{"native_builds":1},"purpose":"Devin adapter tests"}' -- mbx test -p waku-runtime-devin --lib
```

The fixture uses Python 3 as the external peer and explicit request barriers,
not elapsed-time sleeps. It checks original responder retention/option scope,
named cancellation, full-intake interruption, bounded wire records, refusal of
recovery fallback, connection loss without replay, a 25-epoch host reload storm,
provider refusal/budget exhaustion without false success, and Unix provider-group cleanup after killing a disposable runtime parent.
It also rejects inherited product credentials. This is harness evidence,
not a real routed-tool acceptance claim.

With authenticated `/opt/homebrew/bin/devin`, the ignored, billable test creates
one disposable provider session and checks native streaming, 25 controller
transfers, stable runtime/native identities, ordered replay, one completion and
observed child exit:

```sh
goddard-agent resource run --json '{"resources":{"native_builds":1},"purpose":"Actual Devin qualification"}' -- mbx test -p waku-runtime-devin installed_devin_streams -- --ignored --nocapture
```

Actual macOS evidence on 2026-10-12: Devin 3000.11.3 (`9c803229faa4`).
The test passed with runtime `364d8bab-f1e5-4f0f-90de-9c20df577b4b`, logical
session `af789fdb-9df7-4482-9bda-0300002980f6`, generation 1, native session
`canyon-opal`, owned PID 49734, 25 controller transfers and one completion.
These are host-level controller transfers, not rebuilt-daemon reloads. No user
daemon, watcher or existing provider session was restarted or migrated.

## Remaining gates

The shared executable/IPC owner must register this factory. Stage 2C must supply
stable session-scoped tool ingress, reconcile grants/resources, and consume the
runtime journal; no old daemon address may be passed through. Explicit
model/effort mapping and revisioned reconfiguration still need qualification.
Devin-specific free-form input methods/responder qualification, actual approval/cancel behavior, native callback deadlines, lost-answer
acknowledgement handling across daemon replacement, and rebuilt-daemon/app
routed-tool reloads remain unverified. Keep new admission/cutover disabled until
those provider-specific gates pass. Do not mark the overall runtime outcome or
the full Stage 2B gate complete from these narrower results.

Shared-host prerequisite at stage-1 baseline: `RuntimeHost::pump_session`
synthesizes `Terminated(Interrupted)` after sender disconnect even when
`Terminated(Requested)` was already applied. `apply_event` clears the adapter
but leaves the receiver installed; `ingest` accepts another terminal event.
This can overwrite a confirmed stopped projection on the next pump. The actual
provider check above establishes child-exit evidence, not a fully reconciled
stopped projection. The shared host owner must repair that boundary and qualify
the final inventory/one-terminal invariant before production acceptance.

The adapter is registered in root workspace members/default-members and the
lockfile. The shared executable owner must still register its factory;
workspace membership alone does not enable production admission.
