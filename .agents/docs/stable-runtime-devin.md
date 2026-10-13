# Devin stable-runtime adapter qualification

`waku-runtime-devin::DevinFactory` retains the original ACP child, transport,
actor, prompt bookkeeping and permission responders inside the stable host.
It consumes the existing [stage-1 contract](stable-runtime.md), with no
contract changes or dependency on daemon, product policy, UI or localization.
The production host registers this adapter when a trusted Devin binary is available; the legacy daemon driver is
unchanged. It is not a completed Stage 2B release qualification or a cutover.

## Integration interface

The runtime executable chooses a pinned absolute Devin executable:

```rust
let factory = waku_runtime_devin::DevinFactory::new(devin_binary);
```

Register that factory for adapter `devin`. Admission launch data is exactly
`{"cwd":"/absolute/existing/directory"}`. Recovery cursors, arbitrary executable
selection and daemon callback URLs are refused rather than substituted.
Admission accepts exact advertised `model` and `thought_level` values, with
`provider` absent or `devin` and `options: null`. For example, request
`{"model":"swe-2-high","effort":"max","options":null}`. Model aliases and
unadvertised values are refused; no fallback endpoint or default substitution
is used. Revision 0 records provider-confirmed current values, original native
session ID and owned child PID.

`Reconfigure` requires an idle session and a revision newer than the effective
revision. It returns uncertain acceptance while queued and blocks new prompts
until the original provider confirms activation. Model writes refresh the
catalog before effort writes. Each native configuration response has a ten-second
client deadline. Failed, mismatched or timed-out writes interrupt the transport
without emitting `ConfigActivated`, because partial activation may have occurred.
Requested revisions remain separate from effective revisions in the host.

`Submit.input` is a text string (maximum 256 KiB). The adapter holds one
unresolved prompt at a time and reports `Unknown` after queueing it; only a
turn-scoped update, native permission request or successful prompt response
establishes provider acceptance. Session updates retain original ACP payloads.
`ResolveRequest.resolution` is `{"optionId":"an-original-offered-option"}`.
Responders retain their original IDs and live connection; invalid options and
missing responders are refused. A responder can be consumed locally only once.

The `elicitation/create` form extension opens `RequestClass::Input` on the
original connection. Its subject preserves the native message and requested
schema. Resolve it with `{"action":"accept","content":{"label":"release 42"}}`,
`{"action":"decline"}` or `{"action":"cancel"}`. Acceptance checks required
primitive field types and offered enum values; this is not a full JSON Schema
validator. Native schema constraints remain the provider's responsibility.
A supplied session ID must match the original session. URL elicitation is
cancelled. Input answers are capped at 256 KiB, share the 64-responder bound with
approvals, and are cancelled with the original turn. Other unknown methods are
left unhandled rather than answered as success. This extension has fixture
coverage; actual Devin elicitation is still unqualified.

ACP v1 responder delivery has no provider-consumption acknowledgement. Even a
successful SDK write remains uncertain and emits no fabricated
`RequestAnswered`. Reconnection cannot resend an answer. Daemon/app
reconciliation must retain that uncertainty; completion of a turn does not
prove a particular answer was consumed.

Cancellation names the active runtime turn, sends native `session/cancel`, and
remains unknown until the original prompt returns cancelled. Pending permission and input
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
goddard-agent resource run '{"resources":{"native_builds":1},"purpose":"Devin adapter tests"}' -- mbx test -p waku-runtime-devin --lib
```

The fixture uses Python 3 as the external peer and explicit request barriers,
not elapsed-time sleeps. It checks original responder retention/option scope,
named cancellation, full-intake interruption, bounded wire records, refusal of
recovery fallback, connection loss without replay, a 25-epoch host reload storm,
provider refusal/budget exhaustion without false success, Unix provider-group
cleanup after killing a disposable runtime parent, advertised configuration
activation/refusal/response deadlines, original free-form responses, stopped
projection after sender disconnect, and recovered-owner refusal without signalling
an unrelated PID.
It also rejects inherited product credentials. This is harness evidence,
not a real routed-tool acceptance claim.

With authenticated `/opt/homebrew/bin/devin`, run the ignored live checks:

```sh
goddard-agent resource run '{"resources":{"native_builds":1},"purpose":"Actual Devin qualification"}' -- mbx test -p waku-runtime-devin installed_devin -- --ignored --nocapture
```

Actual macOS evidence on 2026-10-12, Devin 3000.11.3 (`9c803229faa4`), against
shared-host prerequisite `4ebca0751a50bc696a8d899340c2b5f43c68792f`:

- `installed_devin_streams_on_original_runtime_transport` passed with runtime
  `85cc9e7b-df96-4492-aacf-f86bd8c5a212`, logical session
  `6c27c0cc-a23b-490d-95c6-94b551865ef4`, generation 1, native session
  `legend-sage`, owned PID 39137 and 25 controller transfers. Ordered replay
  contained one start, one completion and one requested terminal event.
  Subsequent disconnect pumps preserved `SessionStatus::Terminated` and no live
  adapter in reconciled inventory.
- `installed_devin_confirms_revisioned_model_and_effort` passed on native session
  `shell-oregano`, PID 39134. Revision 1 confirmed `swe-2-high` / `max`; revision
  2 confirmed `gpt-6-sol-medium` / `medium`. Both retained native session and PID.
- `installed_devin_approval_and_cancel_preserve_original_responders` passed in a
  disposable directory. A Python shell write did not create its marker before
  approval; resolving the original `allow_once` option allowed the write and
  completion. Cancelling a second pending approval returned native cancelled
  settlement and prevented its marker. Neither settlement fabricated
  `RequestAnswered`. Original approvals remained pending for 2010 and 2005 ms.
  These observation windows do not establish a maximum native deadline.

These are isolated host/adapter checks, not rebuilt-daemon or app reloads.
No user daemon, watcher or existing provider session was restarted or migrated.

## Remaining gates

Stage 2B is **not complete**. Native free-form elicitation has not been observed
through this adapter: a disposable prompt asking Devin to ask for a release
label completed without a native input callback. The extension fixture proves
retention and wire delivery, not native availability. Permission/input callback
maximum deadlines remain `DeadlineBound::Unmeasured`; two-second retention is
only a lower observation window. Answer consumption remains uncertain because
ACP v1 has no consumption acknowledgement.

Recovered Devin owners deliberately return `Termination::Unconfirmed`.
Reconciliation and new admission stay blocked by `unfenced_survivors`; a recorded
PID is never sufficient to signal a recovered process. The Unix runtime-death
watchdog fixture proves group cleanup for its controlled descendant, but does not
provide authenticated owner-recovery evidence or qualify escaped descendants,
remote work or non-Unix cleanup. This conservative boundary must remain until
recovered-owner cleanup is independently qualified.

Stage 2C owns stable session-scoped tool ingress, grants/resources and
lost-answer reconciliation across daemon replacement. Rebuilt-daemon/app
routed-tool reloads and admission/cutover remain separate gates. Do not pass an
old daemon address through or mark the overall runtime outcome complete from
these narrower checks.

The shared terminal-disconnect repair and production Devin registration landed
in prerequisite `4ebca0751`. The strengthened actual-provider and external-peer
checks above verify the one-terminal/stopped invariant against that repair.

The approved `plans/stable-agent-runtime.md` snapshot was unavailable in this
employee workspace, and Boss file access was denied. This unit used the
repository stage-1 contract and this qualification document; the access issue
was reported rather than bypassed.
