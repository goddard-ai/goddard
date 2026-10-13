# Stable agent runtime — host and daemon reconciliation

`crates/waku-runtime` is the stable provider host: a third, separately
surviving owner for provider sessions per the approved
`plans/stable-agent-runtime.md` and Xavier's stage-0 contract. This page is
the orientation note for the owners who consume its seam — Codex, Devin,
daemon/app reconciliation, watcher/updater.

## What landed

- `waku-runtime::contract` — the typed protocol every owner shares:
  `ActionEnvelope`/`CanonicalAction`/`CommandDisposition`, ordered
  `EventEnvelope`/`EventCursor`, `SessionGrant`/`OfflineMatrix` schema, and
  `HostError`. Nobody invents a parallel envelope.
- `waku-runtime::host::RuntimeHost` — namespace singleton (OS-held lock),
  controller credentials + exclusive control epochs, durable command
  deduplication with conservative uncertain acceptance, ordered durable
  event capture/replay, stop/cancel/drain fences, bounded budgets with
  protected control capacity, crash-vs-clean recovery.
- `waku-runtime::journal` — framed append-only record log (`len|crc32|json`),
  corrupt-tail truncation, snapshot compaction.
- `waku-runtime::adapter` — the provider-ownership seam: `AdapterFactory`,
  `AdapterHandle`, `SessionContext` (bounded intake channel),
  `DispatchOutcome`/`CancelOutcome`/`Termination`, `OfflineMatrix`.
- `waku-runtime::fake` — the deterministic harness: `FakeFactory`,
  `FakeAdapter` (scripted dispatch/cancel/terminate outcomes, `emit`,
  `kill`, `dispatched`), `ManualClock`, `journal_fault` injection.
- `waku-runtime::wire`/`ipc`/`server`/`client` — the private transport:
  length-prefixed JSON over a same-user-verified Unix socket, roles
  (controller, epoch-bound service, emergency, session-scoped agent),
  request/response with long-poll waits, and `endpoint.json` discovery
  inside the owner-only namespace root. `waku-runtime-host` is the
  dedicated executable; `registry` maps admission adapter names to
  factories.
- Scoped ingress and claims — `SessionGrant` (journaled token, immutable
  expiry, constant-time check), deduplicated `IngressRequest`/
  `IngressRecord` with `Pending`/`Forwarded`/terminal-or-`Uncertain`
  dispositions, service drain/resolve for the current daemon, and
  `ResourceClaim`s that survive controller replacement and release only
  explicitly.
- `waku-daemon::stable_runtime` — the opt-in `StableLink`: a
  `DriverControl` over IPC that attaches (never resumes) a task to a
  surviving host session, pumps replay into the forwarder from the durable
  `stable_runtime_links.applied_seq` cursor, acks only after the forwarder
  applies, and answers forwarded agent requests from durable
  `stable_runtime_receipts` before executing them. Opt-in is
  `WAKU_STABLE_RUNTIME_ROOT`/`WAKU_STABLE_RUNTIME_PROVIDERS` (and
  `WAKU_RUNTIME_HOST_BIN` to locate the host executable); without them the
  legacy in-process drivers own providers until stage 4.

The crate has **no `waku-*` dependencies** by design: nothing in its
transitive inputs is product, locale, UI, daemon, or CLI code, so ordinary
product edits cannot rebuild it. Keep it that way.

## Semantics owners must rely on

- **Intent before contact.** Commands journal before dispatch, fences
  before termination, events before publish. `dispatch` is called only
  after `DispatchAttempted` is durable.
- **Epoch fencing.** `acquire_control` atomically invalidates the previous
  epoch. New work under a stale epoch is rejected; recorded commands return
  their stored truth to any live controller credential (dedup precedes the
  epoch fence deliberately).
- **Conservative uncertainty.** `DispatchOutcome::Unknown` becomes
  `Uncertain { AcceptanceUnknown }`; never resent automatically. Crash
  recovery maps `DispatchAttempted` → `DispatchUnknown`,
  `ProviderAccepted` → `OutcomeUnknown`.
- **Single order.** Sequence is assigned at journal time, monotonic per
  generation, identical for replay and live reads. `EventCursor` below the
  retained floor is `CursorExpired` (blocked recovery), never silent loss.
- **Honest stop.** `Stopping` until termination is confirmed; an
  unconfirmed terminate stays visibly stopping and the command is
  `Uncertain { OutcomeUnknown }`. Interrupt (backpressure/crash/drain-forced)
  is reported as `Interrupted`, distinct from a clean `Terminated`.
- **Held dispatch.** `CancelTurn` records a hold; queued submits become
  `DispatchPending` until `PermitDispatch` — the controller's explicit
  policy release, never implicit unwind.
- **Tools/approvals.** `ResolveRequest` resolves once under current
  authority and original request identity. Answer delivery
  (`RequestDisposition`) and provider consumption (`RequestAnswered`
  events) are separate facts; adapter evidence reconciles uncertain
  deliveries without resending.

- **Ingress truth.** Requests are durable before forwarding; a `Pending`
  deadline miss is `Expired`, a `Forwarded` one is `Uncertain` — never
  resent. The daemon dedups effects by `request_id` receipt before
  executing, and records the outcome before reporting it.
- **Attach, not resume.** `start_link` reconciles the host inventory
  before admitting; a live recorded session reattaches under the new
  epoch with its event cursor, pending requests, and claims intact.

## Verified by deterministic tests (`mbx test -p waku-runtime`)

Daemon detach/reattach mid-stream with exactly-once replay; 25-epoch
reload storm with zero duplicate dispatch; stale-epoch fencing; same-id
dedup and payload-mismatch rejection; ambiguous-acceptance hold; stop
fences and unconfirmed termination; cancel-hold + permit; crash vs clean
drain distinction (survivors fenced, commands uncertain, no replay, no
adapter respawn); budget interruption vs pausable backpressure; oversized
message drop markers; disk-failure bounded buffering + recovery; corrupt
tail; emergency stop without a controller; incompatible-hello preserving
sessions; singleton lock; retry-window expiry + clock-rollback floor;
pending-request single resolution and evidence reconciliation; journal
compaction preserving pending requests and requested config.

Daemon-side coverage (`mbx test -p waku-daemon stable_runtime`): link
attach/reattach over IPC, applied-marker ack boundaries, replay of only
unapplied events, receipt-deduped ingress effects, agent-grant ingress
over real sockets, loud failure on unregistered adapters, fenced-epoch
surfacing, wholesale daemon replacement over one `app.db` reattaching to
the surviving session, the env opt-in gate, strict per-provider launch
payloads decoded by the real adapter structs, native delta/completion/
request projection, effective-config writes onto the durable session
record, and native request-resolution dialects.

## Product mapping and controller safety

The opt-in daemon link now shares one controller epoch per canonical host root
and daemon incarnation. Opening a second task does not fence the first. Each
namespace service routes requests by their host session/generation, serializes
execution, and persists an uncertain acceptance receipt before any product
effect. A result-write failure leaves that receipt in place; retry returns
uncertainty instead of executing again. Applied cursor writes must succeed
before host acknowledgement, and the forwarder checks generation ownership
before accepting an applied marker.

Codex admissions use the strict adapter launch schema: supervised mode maps to
`untrusted`/`read-only`, automatic edits to `on-request`/`workspace-write`, and
full access to `never`/`danger-full-access`. Devin admissions carry only `cwd`
and require supervised mode. Unsupported product options fail before admission:
CLI credentials, MCP, Computer Use, nonempty presets/subagents, service tiers,
context windows, sandbox VMs and legacy resume cursors. This is a qualification
boundary, not a replacement bridge. The default remains the legacy driver.

Provider prompts use the adapters' text input contract. Native notifications
use the existing Codex and ACP product parsers; they no longer print raw JSON
as assistant text. Completion reads native status and error evidence rather
than always reporting success. Codex permission/input replies retain the
original request identity and native response shape. Steering and attachments
are refused explicitly because these adapters cannot carry them.

This unit changes the runtime inventory contract as well as daemon code.
Live pinned hosts are not rebuilt or replaced automatically.

Inventory includes the adapter, requested configuration and its revision in
addition to effective activation. Reattachment restores the open turn for
cancellation and the highest accepted configuration revision. A different
provider or an interrupted/missing recorded generation blocks attachment;
reconnect does not authorize a replacement. Unconfirmed stop retains the link
identity. Confirmed retirement deletes only that instance/session/generation,
so delayed cleanup cannot remove a replacement attachment. Temporary forwarder clones do not retire the link's workers.

Focused verification commands:

```sh
mbx test -p waku-daemon --lib stable_runtime
mbx test -p waku-drivers --lib stable_projection
mbx test -p waku-runtime
```

The native protocol test uses a controlled Python peer through the production
Codex adapter. It is not actual Codex/Devin reload qualification. Confirmed
configuration now projects to app/catalog/detail through
`DriverEvent::ConfigActivated`, which carries the host generation, the
config revision and the activation boundary; each session's durable
`config_activation` mark rejects stale-revision and foreign-generation
redeliveries and gates generic client saves that predate the confirmation.
Pending native cards need reconstructible attachment data, originating
command/delivery identities must survive app and daemon replacement, and
event effects still require atomic projection/cursor settlement. These remain Stage 2C gates
alongside real CLI/MCP ingress and resource/credential lease transfer.

## Not yet in this stage

Provider replacement (new generation machinery), full Codex/Devin product
qualification, MCP/callback binaries speaking ingress (the contract, grant
machinery, descriptor provisioning and helper pinning are landed;
`goddard-agent` inside a stable session uses its provisioned grant),
watcher/updater integration, truthful reconnect surface states in the app
projections, measured native callback deadlines, non-Unix crash cleanup
qualification.

## Independent executable prerequisite

`crates/waku-runtime-host` now owns the production `waku-runtime-host` binary.
The runtime crate's explicit fake binary is `waku-runtime-fixture`; it is a
harness, not the independently pinned provider owner. The production launcher
accepts `start|serve|status|drain --root ABS_PATH --namespace ENV_PROJECT`; flags
without an operation retain the daemon's existing launch interface. `start`
copies its executable to a SHA256-addressed, mode-0500 artifact inside the private
root and detaches that copy. Compatible launches adopt the authenticated live
instance. Namespace, protocol or instance disagreement never causes replacement.
No provider resumes or command retries occur during adoption.

This consumes the landed `RuntimeClient`/`RuntimeServer` and ingress vocabulary.
Discovery is atomically published, private and verified against the live hello;
both ends verify the OS user. Framing is bounded at 8 MiB, with eight normal
connections and two reserved emergency connections on a separate private socket.
Read/write waits and long-polls are bounded. `HostError` uses adjacent `error` and
`detail` fields: the previous internally tagged encoding could not serialize
scalar errors such as `Incompatible`, silently closing the handshake instead.
Clients and servers must be updated together for this private prerelease wire.

Confirmed drain exits the independent owner and removes only its own discovery
record. Confirmed termination requires quiesced adapter intake. Admission,
controller acquisition, dispatch intent and acknowledgements fail closed when
the journal cannot persist. Callback consumption observed during dispatch is
not overwritten by subsequent bookkeeping. Recovery requires the registered
factory's `fence_recovered` evidence; the default is unconfirmed, blocks admission
and drain, and never signals a process from a PID alone.

Verification: `mbx test -p waku-runtime -p waku-runtime-host`. The executable tests
use disposable roots and prove launcher exit, 25 controller transfers, stale
instance rejection, and emergency status under saturated normal connections.
These are harness results. Full aggregate
metadata budgets, stable tool ingress/grants, daemon event projection and watcher
build/run lane integration remain independent qualification gates.


The production executable now registers Codex and the landed Devin factory
(using an explicit trusted binary or startup PATH). Hello includes the protocol
range, runtime package version plus immutable executable SHA256, and declared
adapter ABI versions. A compatible existing instance missing a required provider
adapter is preserved and requires explicit drain, never automatic replacement.
Replay pages fit the 8-MiB frame bound without acknowledging or skipping the
remaining suffix. See [Codex qualification](stable-runtime-codex.md) for actual
provider evidence, recovery ownership proof and remaining daemon/tool gates.

## Opt-in product CLI ingress

On Unix, `goddard-agent` can use runtime ingress when the session launcher sets
`GODDARD_RUNTIME_INGRESS` to a private descriptor file. Without that variable it
keeps the existing daemon transport. An invalid descriptor fails closed; it does
not fall back to a daemon token. This is a transport seam, not provider cutover.

The descriptor is a version-1 JSON object with `version`, `root`, `taskId` and
`grant` (the original runtime-issued `SessionGrant`). It must be a bounded regular
file owned by the current user with no group/other permissions. Keep its grant
secret; do not place the descriptor contents in logs or command arguments.
The runtime verifies the scoped grant on every call. The CLI neither issues nor
renews grants and has no controller credential.

Before submitting a product `Command`, the CLI persists its request identity,
task/session/generation binding, canonical subject and original deadline beside
the descriptor, in a private `.requests` directory. File and directory sync
precede submission. A partial/corrupt record blocks that identity. Concurrent
writers serialize under a directory lock. Retention stops new requests at 1024
records or 16 MiB; individual records are bounded at 1 MiB. No automatic pruning
can erase unresolved execution evidence. Retention exhaustion requires explicit
reconciliation before further admission through this descriptor.

For a single-request CLI invocation, `GODDARD_RUNTIME_REQUEST_ID` selects a UUID
for querying/repeating the same persisted intent after CLI replacement. Changed
payload or owner rejects. Without it the CLI creates and persists a fresh UUID;
errors after submission identify the UUID needed for reconciliation. Do not reuse
one explicit UUID across a compound command's different requests. The host's
recorded result is authoritative; uncertainty is reported and never resubmitted
under a new identity automatically. Ordinary requests retain a 120-second bound;
explicit long/human waits are capped at 600 seconds on this qualified transport.
The legacy transport's wait behavior is unchanged.

`mbx test -p waku-agent --test stable_ingress` runs the real CLI against a private
socket host with a fake provider and scripted daemon service. It exercises
detachment, 25 controller transfers, original-identity/deadline replay, caller
death after forwarding, uncertainty, changed payload, private file/task binding,
corrupt intents, expiry and retention refusal.

## Provisioned CLI ingress and surviving leases

Daemon attachment provisions the surface end to end on Unix. When a stable
launch carries an agent surface, the admit payload's `agent` field holds a
shared `AgentIngress` — non-secret env pairs (the descriptor path, task
binding, inherited reservation) plus `cliDir`, the pinned helper directory.
Both adapters scrub inherited `GODDARD_*`/`WAKU_*` as before and then apply
only the launch-carried pairs; the daemon token and daemon address never
reach the provider child. Codex announces the surface through its
`thread/start` developer instructions; the launch schema still refuses
unqualified product options.

After the session exists the daemon issues a fresh `SessionGrant` (bounded
30-day expiry) and atomically rewrites a mode-0600 descriptor at
`<data>/agent/<task>/ingress.json` — a shared `IngressDescriptor` shape both
daemon and CLI use. Reattach reuses the original grant identity, token, scope and expiry. Missing,
invalid, expired or fenced authority blocks attachment rather than minting a
replacement. The private descriptor must agree with the durable runtime claim;
controller replacement does not extend authorization. The
goddard-agent binary is pinned by content under `<runtime-root>/helpers/
<sha256>/goddard-agent` (mode 0500, atomic staging): a product lane rebuild
can add new addresses but never substitute a running session's helper. The
lease is recorded as a durable runtime `ResourceClaim` whose claim id is a
deterministic UUIDv5 of `agent-ingress:<session>:<generation>` — reattach
overwrites the same claim rather than accumulating. Confirmed session stop
releases the claim and removes the descriptor; an unconfirmed stop keeps
both for honest reconciliation.

The resource broker's `ResourceOperation::Reanchor { id, owner_pid }` moves
a reservation's liveness owner to a verified live pid — daemon-internal
only, fenced to the owning task, refused on dead rows and dead pids, and it
changes no deadline or authority. `Reservation.owner_pid` is the surviving
owner anchor: daemon-held rows (admission tickets) answer liveness through
the owner, while agent-acquired rows keep their own holder pid so a dead
CLI still ends its lease. Attach re-anchors every live task reservation to
the host pid, and new acquires for stable-linked tasks re-anchor at grant
time, so daemon detachment can never silently release provider-held work.

`mbx test -p waku-daemon --lib stable_link` covers provisioning, original grant
restoration without renewal, claim release on confirmed stop, the pinned
helper, reservation anchoring and end-to-end ingress over the provisioned
grant. `mbx test -p waku-base --lib reanchor` covers lease survival across
daemon death, owner death, foreign-owner fencing and dead-row refusal.
MCP/callback adaptation and actual rebuilt daemon/provider reload evidence
remain open. App configuration consumers, pending-card reconstruction and
atomic event/effect/cursor settlement are separate Stage 2C work.
