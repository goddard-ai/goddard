# Unified memory and handoff engine

Design proposal against `dev` commit `4cd318a9`. This document specifies a
daemon-owned engine; it implements nothing. The approved direction is an
OptMem-inspired layered chunk store, roughly 60 lines per chunk, with progressive
zoom. This is a local design choice, not a claim of compatibility with OptMem.

The engine has three primitives: **insert** places supplied facts using Jev,
**surface** selects authorized context using Jev, and **handoff** deterministically
renders that selection for a new reader. Boss memory, session rotation, provider
switches, employee completion indexes, side chats, and project memory configure
these primitives instead of maintaining separate retrieval systems.

**search** is a fourth read primitive, added alongside surface and handoff: an
exact-match pull (ripgrep over the store's markdown) for literal identifiers —
commit SHAs, paths, error strings, names — where judged relevance is the wrong
tool. It complements zoom rather than replacing it: search is lexical (locate
the chunk containing a string); zoom is spatial (descend the hierarchy to see
the neighborhood a fact lives in). Search to locate, zoom to expand. The full
read surface: **surface** (Jev, proactive per-prompt), **search** (exact,
pull), **zoom** (hierarchy navigation), **handoff** (render for a new reader).

Jev placement is the default write path, not the only one. A principal with
write access to a scope — the Boss for its own scope, a project agent or the
user for project memory — may reorganize directly: refile chunks between
topics, merge or split topics, edit labels and cues, prune or supersede facts.
When the user's request is about memory itself, direct editing is expected,
not exceptional. Manual edits write ordinary chunk revisions and update
affected indexes, so Jev routing continues over whatever structure the manual
pass leaves behind.

## Existing seams and confirmed constraints

References below are source locations at the base commit, not proposed APIs.

| Surface | Confirmed behavior and evidence |
| --- | --- |
| Boss ownership | `BossState` has stable identity, persona, session pointer, employees, and revision (`crates/waku-protocol/src/boss.rs:140`). `BossService` reads `boss.json`, fails closed on corruption, and owns `files/memory` (`crates/waku-core/src/boss.rs:58`). |
| Boss persistence | `update` saves before publishing state; `save` writes personas and `boss.json` atomically per file (`crates/waku-core/src/boss.rs:868`, `:885`). This is not a transaction across session storage and Boss JSON. |
| Read grants | `authorize_file` checks employee folder prefixes, exact knowledge files, and its persona document; expired employees are denied. Discovery reveals only permitted ancestors (`crates/waku-core/src/boss.rs:899`). |
| Context routing | `router_questions` batches attach Noul and focus Choice; `apply_verdict` applies probability thresholds (`crates/waku-core/src/boss_context.rs:286`, `:337`). `route_boss_prompt` evaluates in a background thread and steers or defers attachment (`crates/waku-core/src/daemon.rs:5884`). |
| Session coupling | `BossRouter` holds session, focus, recent prompts, and pending context; changing session resets its entry (`crates/waku-core/src/boss.rs:24`, `:323`). |
| Completion | `finish_boss_employee` hydrates and saves the transcript, renders its turn index, and queues it to an active supervisor or the current Boss session (`crates/waku-core/src/daemon.rs:6376`). |
| Side chats | `side_chat_parent_block` snapshots a parent's transcript index on first prompt (`crates/waku-core/src/daemon.rs:5745`). |
| Provider handoff | Compaction uses per-item Nouls; `assemble_index_envelope` retains user messages and a pull index (`src/app/provider_switch.rs:218`, `:486`). |
| Project memory | `.goddard/memory` contains `MEMORY.md`, `LOG.txt`, and bookkeeping; its injectable summary already has a 60-line cap (`crates/waku-core/src/memory.rs:1`, `:35`). Incognito skips read/feed (`:215`). |
| Persistence and UI | Projects and sessions live in SQLite (`crates/waku-core/src/persistence.rs:1391`); saving excludes incognito (`:1880`). Sidebar rows use daemon Boss keys (`src/app/sidebar.rs:3800`), consistent with [remote-boss.md](remote-boss.md). |

Inference: these seams permit shared selection and rendering without changing
provider-native transcript storage. Stable sidebar row identity already exists;
conversation selection/subscriptions still need to follow the new session pointer.

## Data model and progressive zoom

Keep canonical records as Markdown files on disk — this is the storage model
OptMem validates: an append-only fact log, chunk files, and derived indexes. A
scope is a directory; every write flows through daemon ops, so file mutation is
serialized without needing a database. The daemon's existing SQLite remains for
bookkeeping only (handoff manifests, delivery pointers, rotation journals) —
never for memory content. No automatic remote replication: scope IDs include
daemon identity; each remote daemon enforces its own authority as described in
[remote-boss.md](remote-boss.md).

File layout per scope:

    <scope-root>/
      LOG.txt              append-only raw fact log, one fact per line, never edited
      INDEX.md             top-level index: ordered cue lines, regenerated not authored
      topics/<topic>/
        INDEX.md           topic index (overview cues -> chunk files)
        <slug>.md          a chunk: ~60 lines, frontmatter header + body
      handoffs/            rendered handoff artifacts for specific readers

A chunk file's frontmatter carries its metadata (chunk_id, layer, title, cue,
status, source refs, revision); the body is human-readable Markdown. Indexes are
**derived files**: regenerated deterministically from chunk cues and topic
membership whenever the store changes — never authored, always rebuildable
(`rebuild-index` recreates them from LOG + chunks). They exist so Jev and agents
read cheap cue lists instead of the whole store; `rg` over the scope handles
the search primitive. Because indexes are derived, manual reorganization
(moving a chunk file between topic directories) just triggers an index rebuild
— structure lives in the layout, not in any index's memory.

| Record | Required fields and invariant |
| --- | --- |
| Scope | `scope_id`, `daemon_id`, `kind: session/persona/boss/project`, `owner_id`, `acl_revision`. Scopes are namespaces, not an automatic inheritance tree. |
| Grant | `principal_id`, `scope_id`, optional `collection_id/source_id`, `read/write`, `grantor`, `revision`, optional expiry. Explicit subsets never expand to entire scopes. |
| Layer | `scope_id`, `level: overview/topic/detail`, `label`, `ordinal`. Layer controls zoom, never authority. |
| Chunk | Stable `chunk_id`, `scope_id`, `collection_id`, `layer`, `revision`, `body`, `title`, bounded `cue`, timestamps, `status: active/superseded/tombstoned`, source refs, related IDs. Every revision is immutable. |
| Source | `source_id`, owner scope, kind (`fact/file/turn/index`), stable locator, digest, source revision, author, extraction provenance. Transcript locators use daemon/session/turn/item IDs, not mutable line numbers. |
| Index | `index_id`, scope, revision, ordered entries `(cue, target_id, target_revision, locator)`. Derived entries retain their source restrictions. |
| Handoff | `handoff_id`, source reader, destination principal, purpose, snapshot revision, selection manifest, verbatim turn refs, rendered digest, token budget. Rebuildable from pinned revisions. |
| Boss delivery | `boss_id`, `active_session_id`, `generation`, pending mailbox, router focus, rotation journal. This outlives every provider session. |

Target 60 logical Markdown lines per chunk, with an additional byte/token cap so
one long line cannot defeat the budget. Split oversized supplied text at stable
paragraph/list boundaries; preserve code blocks and link overflow to detail
chunks. These caps are configurable implementation parameters; benchmark token
cost before choosing byte limits. Never silently discard overflow.

Zoom is **index → overview → topic → detail → original source**. Each level can
be requested separately; loading an index does not load all children. For example,
an index cue “release ownership” can lead to a topic chunk naming the owner and
then a cited employee turn with the exact verification output. Bound candidates
by authorized collection, explicit project, lexical matches, and recent activity;
page indexes instead of submitting the entire store to Jev.

Jev does not write summaries. Supplied facts, human-authored overview text, and
existing distiller output can become chunk bodies. Initially generate overview
indexes mechanically from titles/cues. A later text distiller is a separate
writer with provenance and validation; neither placement nor handoff hides a
generative summarization call. Relations across chunks are navigational, not grants.

## Scope authority and read ACL

Authenticate the reader on the daemon before fetching bodies, producing cues,
forming Jev state, listing indexes, or following source links. Unauthorized titles,
counts, IDs, and snippets must not enter the candidate set or evaluator request.
Recheck grants when applying delayed results and at every detail fetch.

| Reader | Session scope | Persona scope | Boss scope | Project scope |
| --- | --- | --- | --- | --- |
| Human through authenticated daemon client | All owned daemon sessions | All owned personas | Owned daemon Boss | Authorized daemon projects |
| Boss principal | Own session lineage; managed employee sources by explicit supervisory policy | Assigned persona; other persona collections by administrative grant | Own Boss collections | Explicit project bindings/grants; work catalog access alone is not a memory grant |
| Employee | Own session only; other transcripts by explicit supervisory grant | Assigned persona's granted knowledge collections only | Explicit folder/source grants captured at summon only | Assigned project's granted collections only |
| Ordinary task | Own session only | None unless explicitly assigned | None | Explicit project binding, subject to memory setting and incognito |
| Side chat | Own session plus explicitly exported parent artifacts | None by parentage; independently assigned grants only | None by parentage; explicit destination grant required | Independent project binding/grant only |
| Background engine worker | Delegated principal's authorized subset | Same | Same | Same |

Scope ownership and read capability are separate: assignment to a persona does
not imply every persona collection is readable. Preserve existing exact-file and
folder grants as collection/source filters. Boss administrative authority is
explicit daemon-local policy, not something passed through session membership.

Inheritance rules:

1. Rotation preserves the stable Boss principal and its grants. It does not copy
   Boss content into a publicly readable session collection.
2. An employee's effective grants are the requested persona grants intersected
   with its supervisor's delegable grants, captured at summon. Later expansion
   requires explicit authorization; revocation takes effect immediately.
3. A side chat receives an explicit export manifest, not the parent's credentials,
   reader identity, or union of scopes. Its parent transcript link is still checked.
4. Provider/model switching retains the same principal and grant filters; it
   cannot widen reads. Changing provider observes the existing user authorization
   and external disclosure settings.
5. Moving information across scopes requires an authorized export. The default
   artifact retains all source restrictions; a reviewed redaction/export creates
   a new source with explicit destination grants and export provenance.

In particular, session messages containing injected Boss memory remain labeled
with Boss source restrictions. Index cues and verbatim replay inherit those
restrictions too. A side chat may see its parent's ordinary conversation while
being denied a Boss-derived passage in that same turn. Do not summarize denied
content into a session cue. Revocation cannot retract text already delivered to
a reader, but prevents subsequent retrieval and rendering.

Employees remain read-only for durable memory. They submit findings as completion
sources; the authorized Boss/project writer decides whether to insert facts.
Incognito uses an ephemeral session store and creates no durable chunks, indexes,
handoffs, or evaluator payloads from its content by default.

## Primitive contracts and Jev questions

Conceptual interfaces, not proposed wire-schema compatibility:

```text
insert(writer, scope, fact, sources, expected_revision, idempotency_key)
  -> {chunk_id, revision, placement, decision_ref}
surface(reader, context, zoom, token_budget, snapshot_revision)
  -> {authorized_selection, index_pointer, decision_ref}
handoff(reader, destination, selection, verbatim_turns, budget)
  -> {artifact_id, rendered_text, manifest, index_pointer}
```

The writer chooses the scope; Jev may choose only among legal layers/chunks in
that scope. Scope selection, access control, and rotation timing are ordinary
code. Follow [jev.md](jev.md): calls go through `Command::Evaluate` on the owning
session's daemon, off the UI thread, with distinct feature tags and bounded state.
Jev returns decisions, never text. Log calibration metadata without introducing
additional raw secret/tool-argument logging; audit the existing eval-log retention
and access policy before using private memory as evaluator input.

Placement state contains the supplied fact, provenance cues, authorized chunk
IDs/revisions, bounded bodies, and capacity. Selection state contains the current
prompt, bounded recent prompts, sticky project focus, purpose, budget, and
authorized candidate cues. Batch questions that are independent; selecting a
layer and then fetching its chunks is genuinely dependent and uses a second
bounded step only when needed.

| Feature/question | Shape and wording intent | Deterministic application |
| --- | --- | --- |
| `memory-place` / `layer` | Choice: `overview/topic/detail`; “Which level fits this fact's specificity?” | Select only configured levels; missing/weak answer uses detail inbox. |
| `memory-place` / `target` | Choice over candidate chunk IDs plus `new`; “Which chunk covers this fact without changing its meaning?” | Apply only to same-scope candidates at the current revision; validate capacity. |
| `memory-place` / `duplicate:<id>` | Noul per plausible match: “Is this already the same supported fact?” | High probability can link provenance instead of adding a duplicate; uncertain means retain the fact. |
| `memory-place` / `conflict:<id>` | Noul: “Do these assertions contradict each other under the same conditions?” | Flag for reconciliation. Never authorize deletion or invent a replacement fact. |
| `memory-surface` / `attach` | Noul: “Does this request need prior context from these authorized candidates?” | Below threshold keeps deterministic baseline context. |
| `memory-surface` / `focus` | Choice over authorized projects plus `none`, matching the Boss router pattern | Sticky focus changes only with adequate probability and margin. |
| `memory-surface` / `relevant:<id>` | Noul per candidate: “Would this chunk materially help answer or continue this request?” | Gate individual chunks; multiple may qualify. |
| `memory-surface` / `utility:<id>` | Score rubric, low to high: unrelated; background; useful; needed for next action | Rank gated chunks within the budget; confidence qualifies use. |

Use protocol shapes exactly: Noul has `instructions` and optional criteria;
Choice has a map of option names to optional descriptions; Score has an ordered
`criteria: Vec<String>` (`crates/waku-protocol/src/eval.rs:89`). No unsupported
free-text answer fields. Validate selected option IDs and finite probabilities.

Proposed initial gates, to calibrate in shadow mode: Choice top probability ≥0.65,
confidence ≥0.70, margin ≥0.15; duplicate Noul ≥0.90; relevance Noul ≥0.60.
Conflict flags do not mutate content. Router focus can preserve its existing
thresholds independently. Score is a ranking signal, not truth or permission.
All numeric gates here are proposed product choices, not measured performance.

On missing credentials, timeout, malformed/weak answers, or stale revision:
insert into a scoped detail inbox without losing the fact; surface a small
authorized recency/explicit-reference baseline; handoff renders deterministically.
Do not retry indefinitely or charge for an evaluation that cannot change output.
Use a transactional revision check and idempotency key on writes. Supersession
requires the authorized writer and leaves provenance/history intact.

Handoff performs no Jev call. It filters against destination ACL, orders by the
selection's stable rank then ID, emits exact chunk text with citations, and lists
omitted authorized entries as pull pointers. Repeated rendering of the same
snapshot, grants, policy version, and budget produces identical bytes. A grant
change invalidates the artifact. Never truncate a fact into an altered assertion.

## Rotation: first client

Rotate when **context usage exceeds the configured threshold AND the provider
prompt cache is cold**, at a safe idle boundary. Cache-capable providers use a
five-minute default TTL with a per-provider override. Providers without caching
rotate immediately once over threshold at that boundary. The TTL is Goddard's
policy estimate, not a guaranteed vendor cache lifetime.

Track `provider/model/cache_key`, `last_cache_refresh_at`, cache capability, TTL,
and observation quality. A cache hit or confirmed prefix cache write refreshes
the clock; unrelated activity does not. Without usable telemetry, estimate from
the latest request that could refresh the same prefix. Missing timestamp means
cold. A prefix/model change invalidates warmth. Cached token counts alone do not
prove future cache availability. Persist wall-clock observations; use monotonic
deadlines within a run, and treat invalid/future timestamps as unknown/cold.

At turn settle, on a new prompt, or on the cold deadline, reevaluate the trigger.
An active tool call, permission decision, or open turn blocks rotation until it
settles. High continuous activity can keep the cache warm and defer rotation;
provider context-limit protection remains a separate safety path, with an explicit
reason if it must override the cache preference. Recompute thresholds for the
target model's usable context budget, reserving output and tool overhead.

New session input has: current persona and tool surface; deterministic handoff
(objective, constraints, accepted decisions, active employee/worktree refs and
pending work); last 3–5 complete conversational turns verbatim, default four;
and an authorized index pointer to older context. This tail preserves exact user
and assistant messages, not raw tool dumps. Tool results remain pullable through
indexed sources. Preserve ACL labels on replay and complete tool-call/result
pairs where the provider transport requires them. If the tail cannot fit, reduce
from five toward three, then use source pointers with an explicit omission marker;
never split structured calls or conceal that verbatim continuity was reduced.

Rotation protocol:

1. Under Boss lifecycle serialization, record a durable rotation intent with old
   generation/session, frozen source revisions, and handoff manifest. New incoming
   prompts and employee reports enter a Boss-addressed mailbox.
2. Stage and initialize the new provider session with the carry packet. Until it
   is ready, the old session remains active; staging does not publish a new pointer.
3. Persist the new session and atomically compare-and-swap
   `active_session_id` plus generation in the authoritative Boss lifecycle record.
   Publish the new Boss view only after durable commit.
4. Drain mailbox items to the active generation using stable delivery IDs and
   persisted acknowledgement state. Resolve the pointer at delivery time, including
   a report from an employee summoned by an older Boss session.
5. Retire the old runtime after the switch; keep its transcript/index available.

Move router focus/recent prompts/pending attachment from session identity to
Boss identity. Asynchronous decisions carry request and generation IDs; stale
results are discarded or requeued as Boss-scoped context, never steered into the
wrong turn. A live employee supervisor remains a session target; fallback-to-Boss
reports use Boss ID, not the supervisor's remembered Boss session UUID.

Initialization failure leaves the old session active and mailbox intact. Restart
recovery uses the journal to abandon incomplete staging or resume committed
delivery. Persisted pointers and the mailbox live in daemon bookkeeping; legacy `boss.json` cannot jointly commit — migrate the pointer
and mailbox authority into the transactional lifecycle record, or use a replayable
journal during the transition. Do not claim exactly-once provider execution:
acknowledgement loss can leave an uncertain submission. Record it and reconcile
the transcript before replay; never blindly duplicate a prompt or external action.

Sidebar row continuity comes from existing Boss-state-driven rows: no synthetic
history fold is required. Selection, transcript subscriptions, composer drafts,
permissions and notifications must follow the pointer/generation. These are
necessary lifecycle integration checks, not a promise that the entire UI needs
no work.

## Other clients and migration

| Client | Same-engine configuration |
| --- | --- |
| Boss memory | Boss-scope collections; explicit fact insert; surface by prompt/focus. Personal memory stays Boss-only unless explicitly granted. |
| Provider/model switch | Same reader, target provider budget, selected older chunks and exact recent turns; reuse deterministic handoff while retaining existing user-message preservation policy until deliberately changed. |
| Employee completion | Insert a deterministic session index with source refs; enqueue its pointer in supervisor/Boss mailbox. Durable fact promotion is a separate authorized insert, not automatic trust in an employee summary. |
| Side chat | Handoff to a separate reader with a purpose and explicit parent export manifest; progressive pull respects source ACL on every zoom level. |
| Project memory | Project collections with existing opt-in/incognito policy; existing distiller supplies candidate facts, shared placement/surface replaces bespoke ranking. |

Migration is resumable and non-destructive:

1. Import Boss `memory/<folder>` files as Boss collections with original paths,
   digests and stable source aliases. Folder names do not automatically imply
   project scopes. Split text deterministically; quarantine unparseable files and
   preserve original bytes. No Jev required just to copy existing content.
2. Translate employee `memory_folders` to collection-prefix read grants and
   `knowledge_files` to exact-source grants. Persona-owned reusable knowledge may
   become persona collections only when its authority is equivalent; retain
   Boss-scoped aliases/grants otherwise. Keep persona instructions distinct from
   learned facts. Never broaden access merely to simplify migration.
3. Import project `MEMORY.md` and `LOG.txt` into project overview/detail records;
   preserve distiller offsets/bookkeeping to avoid feeding old turns twice.
4. Index retained employee/session transcripts lazily using stable turn/item refs.
   Existing employee expiration/roster retirement must not delete indexed sources.
   Preserve supervisory read restrictions after the runtime expires.
5. Keep old CLI file paths and transcript operations as scoped adapters during
   transition. Choose one canonical writer per migrated collection (the daemon); mirror
   Markdown exports from committed engine revisions. External file edits become
   validated import revisions, with conflict detection rather than last-write wins.
6. Record a migration ledger keyed by source path/ID and digest; restart resumes
   safely. Verify counts, digests, grants and rendering before switching reads.
   Back up originals and support reverting adapters without discarding new facts.

Index/source deletion produces tombstones and invalidates dependent selections;
retention and user deletion must include rendered artifacts, evaluator caches and
derived cues. Do not automatically ingest credentials or raw tool arguments.
Metadata caches are keyed by daemon, principal, ACL revision, store revision,
purpose and budget; a shared cache may not bridge readers' grants.

## Phased implementation and acceptance

1. **Store and authority.** Add versioned records, transactions, immutable source
   refs, scoped imports and ACL-filtered indexes. Verify idempotent imports,
   corruption handling and a negative ACL matrix, especially Boss-derived session
   passages requested by a side chat. No automatic rotation yet.
2. **Deterministic handoff and rotation.** Implement bounded render, Boss mailbox,
   active pointer, journal/recovery and cache-aware trigger behind an opt-in flag.
   Start with explicit/recency surface and detail-inbox insert fallbacks. Exercise
   warm/cold/no-cache providers, provider overrides, initialization failure, daemon
   restart, stale router results and employee completion during pointer swap.
3. **Jev placement and surfacing.** Add bounded question batches and feature labels;
   extend the Boss context-router pattern without duplicate focus evaluations.
   Shadow decisions first, measure useful retrieval, misses, latency and spend;
   then enable calibrated gates. Verify deterministic operation with Jev disabled.
4. **Migrate remaining clients.** Adapt provider switches, completion indexes, side
   chats and project memory; compare their rendered context and permissions with
   legacy behavior. Remove bespoke stores/rankers only after equivalence checks
   and recoverable migration. Keep human-readable exports and pull operations.

Runtime implementation must follow [performance.md](performance.md) and
[testing.md](testing.md): no I/O/evaluation from render paths, bounded snapshots,
background work and event-pump application. Gate shipped rotation with exact
provider interaction checks; a Rust build alone will not establish continuity.

Open implementation choices are the configurable context threshold, byte/token
caps, provider capability/telemetry mapping, and calibration gates. Resolve them
from workload measurements; they do not change the three primitives or ACL rules.
