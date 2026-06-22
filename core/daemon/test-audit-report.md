# Core Daemon Test Audit Report

Initial audit used `sem entities core/daemon/test/*.test.ts` against the daemon test files at the time.

Event-system refresh on 2026-06-22 used the current daemon event catalog and test inventory. Current `sem entities core/daemon/test/*.test.ts` reports 129 daemon test cases; the detailed table below still contains the original 125-row inventory and should be fully regenerated before using row counts or line numbers as authoritative.

This report maps current test cases to the contract they appear to protect, the assertion surfaces they use, and whether better seams would make the tests less coupled to daemon internals or logs. It is an audit only; it does not refactor tests.

## Summary

- Test files audited: 12.
- Detailed table rows: 125 original audit rows.
- Current test inventory: 129 test cases.
- Strongest immediate refactor candidates:
  - `core/daemon/test/daemon.test.ts` runtime tests that use logs as lifecycle assertions or synchronization.
  - `core/daemon/test/config-reload.test.ts` config reload failure detection through logs.
  - `core/daemon/test/ipc-security.test.ts` mixed behavior tests with embedded logging/correlation assertions.
- Most DB assertions in `session-lifecycle.test.ts` are defensible when the test explicitly covers persistence, restart recovery, stored records, or hidden state transitions. Some are `db-proxy` candidates where a follow-up IPC query would express the contract better.
- The daemon now has a unified event system with both IPC `events.stream` and an in-process `daemon.events.stream` test seam. Runtime events are logged automatically by `observeDaemonEventsForLogging` when emitted through the daemon event bus.
- Several previously proposed seams now exist, including `pull_request.feedback.ignored`, `pull_request.feedback.finished`, `backend.stream.degraded`, `session.message`, `session.lifecycle.updated`, `session.lifecycle.deleted`, and session lifecycle/worktree events.
- Remaining likely event gaps are `config.reload.failed`, repo subscription started, PR feedback launch/coalesced/failed/session-create-failed, explicit idle-shutdown timer state events, and structured worktree bootstrap failure events.

## Counts

Primary contract categories from the original table:

| Contract type | Count | Notes |
|---|---:|---|
| `public-daemon-behavior` | 67 | Mostly IPC, backend-client, session, worktree, and workforce flows. |
| `diagnostic-contract` | 22 | Logging format, log mode, redaction, failure details, explicit diagnostics. |
| `persistence-guarantee` | 15 | Store migrations, restart recovery, durable session/worktree/title/history state. |
| `test-harness-infrastructure` | 10 | Config/schema/backend client wrappers, fixture agents, timers, daemon package wrappers. |
| `implementation-coupled` | 6 | Service-level internals and direct helper behavior without a public daemon surface. |
| `missing-seam` | 5 | Behavior looks valid, but logs/DB/timing are standing in for a better event/API seam. |

Common smells:

| Smell | Where it appears |
|---|---|
| `log-proxy` | `daemon.test.ts`, `config-reload.test.ts`, mixed IPC security tests. |
| `db-proxy` | PR feedback flow, inbox/session state checks that could use IPC follow-up queries. |
| `internal-import` | Service/config/schema/session tests intentionally import daemon internals. |
| `fake-first-party` | Agent install/update service tests use local fake service collaborators. |
| `time-sensitive` | Long-running daemon/session lifecycle tests use polling and timeouts. |
| `multi-contract` | Several daemon runtime and session lifecycle tests combine behavior, persistence, and diagnostics. |

## Current Unified Event Status

Current daemon event infrastructure:

- `startDaemonServer()` composes `daemonRuntimeEvents` with plugin events and returns `daemon.events`, so integration tests can observe events in-process without going through logs.
- IPC exposes the same composed stream through `events.stream`, with name and exact payload-property filters.
- The IPC server observes daemon events and logs them automatically with `eventId` and `eventAt`, using debug scopes when event definitions request debug logging.

Events that already cover earlier audit suggestions:

| Earlier need | Current event status | Notes |
|---|---|---|
| feedback ignored | `pull_request.feedback.ignored` exists | Covers unmanaged pull requests. |
| feedback flow completed | `pull_request.feedback.finished` exists | Earlier report used the proposed name `pull_request.feedback.finish`; current code uses `finished`. |
| stream subscription degraded | `backend.stream.degraded` exists | Covers unauthenticated stream startup. |
| session message stream | `session.message` exists | Replaces older session message stream-specific IPC routes and is used by idle-shutdown subscriber tests. |
| session lifecycle updates | `session.lifecycle.updated` and `session.lifecycle.deleted` exist | Useful for connection/status/list invalidation behavior. |
| session worktree and launch lifecycle | `session.worktree.prepared`, `session.persisted`, `session.activated`, `session.launch.finished`, `session.launch.failed`, `session.stopping` exist | Covers many launch/worktree/restart observations. |
| inbox updates | `inbox.item.updated` exists | Good replacement for some direct inbox DB assertions. |
| pull request attention updates | `pull_request.created` and `pull_request.updated` exist | Good replacement for some PR/inbox coupling assertions when attention is the contract. |

Remaining event gaps to consider:

| Missing or incomplete event | Tests/behavior it would help | Suggested payload |
|---|---|---|
| `config.reload.failed` | `config-reload.test.ts` invalid local config recovery currently counts logs. | scope, cwd or config path, error message, previous version if available. |
| `backend.stream.started` | `daemon.test.ts` IPC-only/stream-enabled startup assertions still inspect subscription-start logs. | daemon URL/port when IPC is active; backend base URL if useful. |
| `pull_request.feedback.started` or `pull_request.feedback.launched` | Runtime feedback flow still asserts `pr_feedback.launch` logs. | repository, owner, repo, prNumber, feedbackType. Avoid prompt text unless explicitly needed. |
| `pull_request.feedback.failed` | Runtime feedback flow still checks absence of `pr_feedback.session_create_failed`/failed logs. | repository, owner, repo, prNumber, feedbackType, failure phase, error message. |
| `pull_request.feedback.coalesced` | Current coalescing is log-only. | repository, owner, repo, prNumber, feedbackType. |
| idle shutdown timer events | Idle shutdown tests still assert persisted diagnostics for timer started/cancelled/expired. | sessionId, action (`started`, `cancelled`, `expired`, `skipped`), reason, timeoutMs. |
| worktree bootstrap failure event | Bootstrap failure tests still lean on diagnostics and launch failure behavior. | sessionId if allocated, requested cwd, worktree info when available, phase, exit code/error message. |

Rows below that recommend `replace-log-with-event` should first check whether the current event already exists. If it does, prefer converting assertions to `events.stream` or `daemon.events.stream` before adding new events.

## Audit Table

| File:line | Test | Contract type | Contract protected | Current surfaces | Smells | Candidate event/seam | Recommendation |
|---|---|---|---|---|---|---|---|
| `features/agent/test/install-service.test.ts:88` | agent install service resolves configured and registry agents | implementation-coupled | Agent install service resolution precedence and missing-agent error. | service API, fake registry | internal-import, fake-first-party | none needed if service API is accepted unit boundary | keep |
| `features/agent/test/install-service.test.ts:110` | agent install service forwards deterministic cache options and launch fallback policy | implementation-coupled | Managed install API receives cache and launch fallback options. | fake managed install API calls | fake-first-party | narrower option-normalization helper if this grows | keep |
| `features/agent/test/install-service.test.ts:170` | agent install service records usage after resolving a managed launch | persistence-guarantee | Managed launch usage is recorded after resolution. | usage store state, service API | internal-import | usage store state is the contract here | assert-persistence |
| `features/agent/test/install-service.test.ts:210` | agent install service does not gate launch resolution behind background update work | implementation-coupled | Background update does not block launch resolution. | service API, delayed fake update | time-sensitive, fake-first-party | explicit controlled promise helper is already adequate | keep |
| `features/agent/test/update-scheduler.test.ts:136` | managed install update checks update daily managed-install agents | implementation-coupled | Daily managed-install agents with recent usage are updated and state is recorded. | fake install service, state store | fake-first-party | scheduler/service unit boundary | keep |
| `features/agent/test/update-scheduler.test.ts:178` | managed install update checks skip fresh state until config changes | implementation-coupled | Fresh update state suppresses checks unless config changes. | fake install service, state store | fake-first-party | scheduler/service unit boundary | keep |
| `features/agent/test/update-scheduler.test.ts:255` | managed install update checks skip agents not used recently | implementation-coupled | Unused/stale managed-install agents are skipped. | fake install service, usage/state stores | fake-first-party | scheduler/service unit boundary | keep |
| `features/agent/test/update-scheduler.test.ts:304` | managed install update checks record and log failed updates | diagnostic-contract | Failed managed install updates record state and emit failure diagnostic. | fake logger events, state store | diagnostic assertion in unit test | typed scheduler event or accepted logger contract | split-diagnostic-contract |
| `features/agent/test/update-scheduler.test.ts:344` | managed install update scheduler clears pending timers on close | test-harness-infrastructure | Scheduler closes pending timer. | fake timers | time-sensitive | injected timer seam is explicit | keep |
| `backend.test.ts:6` | daemon backend client creates PRs and checks managed status through rouzer route helpers | public-daemon-behavior | Backend client talks to rouzer backend routes. | real in-memory backend server | none | backend harness is public collaborator | keep |
| `backend.test.ts:46` | daemon backend client uses injected auth state for authenticated requests | public-daemon-behavior | Auth header provider is used for authenticated backend calls. | backend client/server | none | backend harness | keep |
| `backend.test.ts:70` | daemon backend client subscribes to unified stream via rouzer route response | public-daemon-behavior | Backend stream delivers PR events. | backend stream event | time-sensitive minor sleep | stream close acknowledgement if flaky | keep |
| `backend.test.ts:117` | daemon backend client reports missing stream auth as unauthenticated errors | public-daemon-behavior | Missing auth maps to backend unauthenticated error. | backend client error | none | public client error | keep |
| `backend.test.ts:126` | daemon backend client reports stream auth failures as unauthenticated errors | public-daemon-behavior | Invalid auth maps to backend unauthenticated error. | backend client/server error | none | public client error | keep |
| `config-reload.test.ts:47` | config manager promotes valid root config edits and preserves the last good snapshot after invalid edits | missing-seam | Invalid local config does not replace last good snapshot. | config manager snapshots, logs | log-proxy, time-sensitive | typed `config.reload.failed` event or config manager diagnostic recorder | replace-log-with-event |
| `config-reload.test.ts:155` | action.run picks up updated root-config agent defaults without restarting the daemon | public-daemon-behavior | Action sessions use refreshed root config. | IPC action.run/session shutdown | time-sensitive | IPC surface is adequate | keep |
| `config-reload.test.ts:215` | pull request feedback handler picks up updated root-config agent defaults without restarting the daemon | public-daemon-behavior | PR feedback flow uses refreshed root config. | feature backend event handler, DB sessions, IPC shutdown | db-proxy | public session query or feedback-flow completion event | assert-public-api |
| `config-resolver.test.ts:27` | rejects worktree plugin references in repository-local config | public-daemon-behavior | Local config cannot set worktree plugin references. | config resolver error | internal-import | config resolver is accepted unit boundary | keep |
| `config-resolver.test.ts:47` | allows repository-local worktree bootstrap config and replaces inherited arrays | public-daemon-behavior | Local worktree bootstrap config merges safely. | config resolver result | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:88` | merges agents.default from repository-local config | public-daemon-behavior | Local config can set default agent. | config resolver result | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:113` | rejects managed-install agents in repository-local config | public-daemon-behavior | Agents stay global-only. | config resolver error | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:132` | merges managed-install agents from global config with repository-local default agent | public-daemon-behavior | Global managed-install agents combine with local default agent. | config resolver result | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:169` | merges session idle-shutdown duration from repository-local config | public-daemon-behavior | Local idle-shutdown duration resolves. | config resolver result | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:194` | rejects disabled session idle-shutdown config | public-daemon-behavior | Disabled idle shutdown config is invalid. | config resolver error | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:209` | merges session env policy restrictions with global fixed env | public-daemon-behavior | Env policy merges local restrictions with global fixed env. | config resolver result | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:250` | rejects fixed session env injection in repository-local config | public-daemon-behavior | Local fixed env injection is rejected. | config resolver error | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:269` | repository-local security policy can tighten pull request operations | public-daemon-behavior | Local security can tighten PR operations. | config resolver result | internal-import | config resolver unit boundary | keep |
| `config-resolver.test.ts:302` | rejects repository-local security policy that loosens pull request operations | public-daemon-behavior | Local security cannot loosen PR operations. | config resolver error | internal-import | config resolver unit boundary | keep |
| `config-schema.test.ts:6` | daemon root config schema accepts session title generator model config | test-harness-infrastructure | Generated schema accepts title generator config. | schema parse | none | schema is contract | keep |
| `config-schema.test.ts:22` | daemon root config schema accepts worktree branch prefix config | test-harness-infrastructure | Generated schema accepts worktree branch prefix. | schema parse | none | schema is contract | keep |
| `config-schema.test.ts:32` | daemon root config schema accepts managed agent policies | test-harness-infrastructure | Generated schema accepts managed agent policies. | schema parse | none | schema is contract | keep |
| `config-schema.test.ts:51` | managed agent policies must declare install or update intent | test-harness-infrastructure | Schema rejects empty managed agent policy. | schema parse | none | schema is contract | keep |
| `config-schema.test.ts:63` | generated goddard schema embeds the model schema once under local defs | test-harness-infrastructure | Schema generation deduplicates model schema. | schema object | none | schema is contract | keep |
| `config-schema.test.ts:78` | root config merging rejects non-object config fragments before merging | test-harness-infrastructure | Config merging rejects invalid fragments. | merge helper | internal-import | helper is schema/config boundary | keep |
| `config-schema.test.ts:95` | root config merging keeps managed-install agents global only | test-harness-infrastructure | Config merging preserves global-only managed-install agents. | merge helper | internal-import | helper is schema/config boundary | keep |
| `daemon.test.ts:50` | daemon package ships agent-bin wrappers for goddard and workforce | test-harness-infrastructure | Packaged daemon includes agent wrappers. | filesystem | none | package artifact check | keep |
| `daemon.test.ts:61` | daemon run subscribes once and launches managed PR feedback sessions across repositories | missing-seam | Stream feedback creates completed sessions for matching repositories. | backend harness, DB sessions, logs, partial event assertion | log-proxy, db-proxy, time-sensitive | use existing `pull_request.feedback.finished`; add feedback started/failed events only if launch/failure is a contract | replace-log-with-event |
| `daemon.test.ts:215` | daemon run can start only the IPC server when stream is disabled | missing-seam | IPC starts without stream subscription. | health check, backend count, logs | log-proxy | IPC health already covers listening; remove log assertions | remove-incidental-assertion |
| `daemon.test.ts:258` | daemon run skips backend stream without IPC-owned backend event handlers | missing-seam | Backend stream does not start when no feature-owned handler can run. | backend harness, DB sessions, logs | log-proxy, db-proxy | `backend.stream.started` plus no sessions | replace-log-with-event |
| `daemon.test.ts:313` | daemon run keeps IPC available when stream startup is unauthenticated | missing-seam | IPC remains available when stream subscription degrades. | health check, backend count, logs | log-proxy | `backend.stream.degraded` event if degraded state is contract | replace-log-with-event |
| `daemon.test.ts:369` | daemon run defaults to compact terminal logs | diagnostic-contract | Default terminal log mode is compact. | stdout | none | log output is contract | keep |
| `daemon.test.ts:385` | daemon run supports raw json terminal logs when requested | diagnostic-contract | JSON log mode writes raw JSON entries. | stdout JSON | none | log output is contract | keep |
| `daemon.test.ts:402` | daemon run supports verbose terminal logs with expanded fields | diagnostic-contract | Verbose log mode expands fields. | stdout | none | log output is contract | keep |
| `daemon.test.ts:419` | daemon run logs startup failures after logging is configured | diagnostic-contract | Startup failures are logged after logger setup. | logs, exit code | none | log output is contract | keep |
| `daemon.test.ts:442` | daemon URL round-trips the TCP address | test-harness-infrastructure | Daemon URL parser round-trips host/port. | pure helper | none | helper contract | keep |
| `daemon.test.ts:452` | daemon runtime resolves the global daemon port override | public-daemon-behavior | Runtime honors `GODDARD_DAEMON_PORT`. | env, health check, IPC client | process/env | IPC health is adequate | keep |
| `ipc-security.test.ts:46` | daemon submit request requires a valid session token | diagnostic-contract | Invalid token is rejected and request logging redacts token/correlates failure. | IPC error, logs | log assertion in behavior test | split redaction/correlation into logging-security test | split-diagnostic-contract |
| `ipc-security.test.ts:76` | daemon hides unexpected handler crashes from IPC clients | diagnostic-contract | Client sees generic error while daemon logs internal detail. | IPC error, logs | log assertion | explicit crash observability contract | split-diagnostic-contract |
| `ipc-security.test.ts:120` | daemon submit request enforces trusted repo context and records created PR access | public-daemon-behavior | PR submit uses trusted session repo and records access. | backend call, DB, IPC follow-up, logs | db-proxy, log-proxy | PR IPC query plus separate correlation logging test | split-diagnostic-contract |
| `ipc-security.test.ts:248` | daemon submit request honors repository-local security deny policy | public-daemon-behavior | Local security deny blocks PR submission. | IPC error, backend calls | none | public IPC/backend collaborator | keep |
| `ipc-security.test.ts:298` | daemon reply request rejects PRs outside the session allowlist | public-daemon-behavior | Session token cannot reply to unallowed PR. | IPC error | none | public IPC | keep |
| `ipc-security.test.ts:325` | daemon reply request records pull request checkout locations | persistence-guarantee | PR checkout location is persisted and inbox row is created. | DB, IPC | db-proxy maybe | PR get/list IPC if sufficient; DB okay for persistence | assert-persistence |
| `ipc-security.test.ts:384` | daemon session reporting creates and updates session inbox rows | public-daemon-behavior | Session reporting drives inbox row lifecycle. | IPC, DB inbox | db-proxy | public inbox IPC query would be clearer | assert-public-api |
| `ipc-security.test.ts:436` | daemon workforce request rejects mismatched roots for token-backed sessions | public-daemon-behavior | Workforce request enforces token root. | IPC error, seeded DB | seed setup only | public IPC | keep |
| `ipc-security.test.ts:464` | daemon workforce respond rejects mismatched roots for token-backed sessions | public-daemon-behavior | Workforce response enforces token root. | IPC error, seeded DB | seed setup only | public IPC | keep |
| `ipc-security.test.ts:491` | daemon workforce request rejects token-backed sessions without a workforce root | public-daemon-behavior | Workforce request requires workforce root for token session. | IPC error, seeded DB | seed setup only | public IPC | keep |
| `logging.test.ts:13` | compact logging flattens plain object fields one level | diagnostic-contract | Compact daemon log formatting. | log output | none | logging is contract | keep |
| `logging.test.ts:49` | json logging preserves null-valued daemon context fields | diagnostic-contract | JSON daemon logs preserve ambient context nulls. | log output | none | logging is contract | keep |
| `logging.test.ts:103` | snapshot logger preserves captured async context outside the original run | diagnostic-contract | Logger snapshots preserve context. | log output | none | logging is contract | keep |
| `logging.test.ts:157` | debug logger writes scoped durable rows without terminal output | diagnostic-contract | Debug logs persist without terminal output. | log store | none | logging is contract | keep |
| `mock-seed.test.ts:29` | seed mock writes deterministic isolated fixture data through the daemon IPC surface | public-daemon-behavior | Mock profile seeding creates inspectable IPC fixture data. | IPC queries | none | public IPC | keep |
| `mock-seed.test.ts:116` | seed mock reset is mock-profile only and repeated seeding does not duplicate records | persistence-guarantee | Mock seeding is idempotent and profile-isolated. | store counts | db-proxy acceptable | persistence/counts are contract | assert-persistence |
| `session-lifecycle.test.ts:107` | daemon store repairs duplicate session turn rows before adding unique constraints | persistence-guarantee | Store migration repairs duplicate turn rows. | DB migration store | none | persistence is contract | assert-persistence |
| `session-lifecycle.test.ts:264` | daemon revokes session tokens when agent processes exit | persistence-guarantee | Agent exit revokes token/permissions. | IPC create, DB state | db-proxy | public token resolve after exit plus DB for persistence | assert-persistence |
| `session-lifecycle.test.ts:291` | daemon persists repository context into durable session storage | persistence-guarantee | Repository metadata is stored on session. | IPC create, DB state | db-proxy | session.get if it exposes metadata; DB if durable storage is contract | assert-persistence |
| `session-lifecycle.test.ts:320` | daemon resolves the default agent for direct session creation | public-daemon-behavior | Session create uses configured default agent. | IPC create/shutdown | none | public IPC | keep |
| `session-lifecycle.test.ts:344` | loadable sessions remain reconnectable after shutdown | public-daemon-behavior | Shutdown loadable session can reconnect and stream later prompt. | IPC, stream, DB wait | db-proxy for wait | lifecycle event or public state query for inactive wait | assert-public-api |
| `session-lifecycle.test.ts:416` | session completion hides from the default list but stays interactive | public-daemon-behavior | Completed sessions hide from list but remain usable/reactivated. | IPC list/get/history/send, DB inbox | db-proxy | inbox IPC query for row state | assert-public-api |
| `session-lifecycle.test.ts:475` | loadable sessions remain reconnectable after daemon restart | public-daemon-behavior | Restarted daemon can reconnect loadable session. | IPC, DB wait | db-proxy | public session connection state query | assert-public-api |
| `session-lifecycle.test.ts:516` | session reconnect fails when the resolved agent no longer supports ACP session/load | public-daemon-behavior | Reconnect rejects unsupported agent. | IPC, DB update setup | seed/setup only | public IPC error | keep |
| `session-lifecycle.test.ts:545` | daemon persists ACP stop reasons on the session record | persistence-guarantee | ACP stop reason is stored durably. | IPC send, DB state | db-proxy | session.get stopReason if public; DB if storage contract | assert-persistence |
| `session-lifecycle.test.ts:562` | daemon coalesces stored agent message chunks while keeping the live stream granular | persistence-guarantee | Live stream stays granular while stored history is coalesced. | IPC stream/history, DB turn | DB confirmation maybe redundant | history IPC covers persisted shape | assert-public-api |
| `session-lifecycle.test.ts:659` | daemon stores usage updates on the session instead of durable turn history | persistence-guarantee | Usage updates update session context usage and are excluded from turn history. | IPC history, DB state | db-proxy | session.get/history public query likely sufficient | assert-public-api |
| `session-lifecycle.test.ts:724` | daemon creates placeholder session titles before any user prompt is sent | public-daemon-behavior | Session create returns placeholder title. | IPC create | none | public IPC | keep |
| `session-lifecycle.test.ts:741` | daemon derives a fallback title immediately when the session starts with an initial prompt | public-daemon-behavior | Initial prompt derives fallback title. | IPC create | none | public IPC | keep |
| `session-lifecycle.test.ts:759` | daemon promotes placeholder titles after the first later prompt is accepted | persistence-guarantee | Title state updates after prompt. | IPC send, DB wait/get | db-proxy | session.get polling | assert-public-api |
| `session-lifecycle.test.ts:789` | daemon marks pending title generation as failed when provider config is present but unusable | persistence-guarantee | Title generation failure updates title state. | IPC, DB state | db-proxy | session.get polling plus diagnostics if contract | assert-public-api |
| `session-lifecycle.test.ts:824` | daemon reconciles interrupted sessions on restart and leaves archived history readable | persistence-guarantee | Restart reconciliation archives interrupted session and keeps history/diagnostics readable. | seeded DB, IPC get/history/diagnostics | DB setup | public IPC after seeded state | keep |
| `session-lifecycle.test.ts:913` | daemon promotes interrupted turn drafts into incomplete turn history on restart | persistence-guarantee | Restart promotes draft into incomplete history. | seeded DB, IPC history | DB setup | public IPC after seeded state | keep |
| `session-lifecycle.test.ts:1008` | multiple clients can observe the same live session stream independently | public-daemon-behavior | Multiple stream subscribers receive same live session updates. | IPC subscriptions | time-sensitive | public stream | keep |
| `session-lifecycle.test.ts:1061` | daemon auto-shuts down idle loadable sessions with no connected clients | diagnostic-contract | Idle shutdown occurs and records diagnostics. | IPC, DB, diagnostics | time-sensitive, diagnostics | session lifecycle event plus diagnostics if accepted | split-diagnostic-contract |
| `session-lifecycle.test.ts:1091` | session idle auto-shutdown uses configured duration | diagnostic-contract | Configured idle timeout controls shutdown and diagnostics. | IPC, DB, diagnostics | time-sensitive, diagnostics | lifecycle event plus public session state | split-diagnostic-contract |
| `session-lifecycle.test.ts:1119` | session.message event stream subscribers cancel idle auto-shutdown before expiry | diagnostic-contract | Stream subscribers cancel idle timer. | daemon event stream, DB, diagnostics | time-sensitive, diagnostics | add idle timer events if operational contract | split-diagnostic-contract |
| `session-lifecycle.test.ts:1149` | session lifecycle subscribers do not cancel idle auto-shutdown | diagnostic-contract | Lifecycle subscribers do not hold sessions alive. | IPC subscription, DB, diagnostics | time-sensitive, diagnostics | lifecycle/idle event | split-diagnostic-contract |
| `session-lifecycle.test.ts:1195` | idle auto-shutdown waits for the last session.message event stream subscriber to disconnect | diagnostic-contract | Idle timer starts after last stream subscriber disconnects. | daemon event stream, DB, diagnostics | time-sensitive, diagnostics | add idle timer events if operational contract | split-diagnostic-contract |
| `session-lifecycle.test.ts:1254` | busy loadable sessions do not time out until they become quiescent | diagnostic-contract | Active turn delays idle shutdown. | IPC, DB, diagnostics | time-sensitive, diagnostics | lifecycle/idle event | split-diagnostic-contract |
| `session-lifecycle.test.ts:1278` | sessions waiting on permission responses do not time out until the permission resolves | diagnostic-contract | Pending permission delays idle shutdown. | IPC, DB, diagnostics | time-sensitive, diagnostics | lifecycle/idle event | split-diagnostic-contract |
| `session-lifecycle.test.ts:1316` | sessions without session/load support never use idle auto-shutdown | diagnostic-contract | Unsupported sessions skip idle timer. | IPC, DB, diagnostics | time-sensitive, diagnostics | diagnostics may be explicit contract | split-diagnostic-contract |
| `session-lifecycle.test.ts:1337` | manual session shutdown clears any pending idle auto-shutdown timer | diagnostic-contract | Manual shutdown cancels idle timer. | IPC, diagnostics | diagnostics | lifecycle/idle event | split-diagnostic-contract |
| `session-lifecycle.test.ts:1361` | daemon shutdown clears pending idle auto-shutdown timers | diagnostic-contract | Daemon shutdown cancels idle timers. | IPC, diagnostics | diagnostics | lifecycle/idle event | split-diagnostic-contract |
| `session-lifecycle.test.ts:1384` | agent process exit clears pending idle auto-shutdown timers | diagnostic-contract | Agent exit cancels idle timers. | IPC, DB, diagnostics | time-sensitive, diagnostics | lifecycle/idle event | split-diagnostic-contract |
| `session-lifecycle.test.ts:1409` | daemon queues concurrent prompts per session and drains them in arrival order | public-daemon-behavior | Concurrent prompts are serialized in arrival order. | IPC stream/send | time-sensitive | public stream/order | keep |
| `session-lifecycle.test.ts:1473` | daemon cancel returns queued prompts, emits terminal errors for queued raw prompts, and prevents them from being sent | public-daemon-behavior | Cancellation returns queued prompts and rejects raw queued prompts. | IPC stream/cancel | multi-contract | split cancel response from raw-prompt stream errors if needed | split-test |
| `session-lifecycle.test.ts:1567` | daemon steering ignores message chunks and dispatches on tool updates | public-daemon-behavior | Steering waits for tool boundary. | IPC stream/steer | time-sensitive | public stream | keep |
| `session-lifecycle.test.ts:1657` | daemon steering falls back to the cancelled prompt response when no tool boundary appears | public-daemon-behavior | Steering fallback returns cancelled prompt response. | IPC stream/steer | time-sensitive | public stream | keep |
| `session-lifecycle.test.ts:1732` | session worktree opt-in maps cwd into a real worktree subdirectory | public-daemon-behavior | Worktree launch maps cwd to subdir. | IPC, filesystem/git | none | public session/worktree response | keep |
| `session-lifecycle.test.ts:1763` | session.changes reads tracked and untracked diff content from the session workspace root | public-daemon-behavior | Session changes returns tracked/untracked diff. | IPC, filesystem/git | none | public IPC | keep |
| `session-lifecycle.test.ts:1802` | session completion enforces worktree cleanliness without blocking local dirty repos | public-daemon-behavior | Completion requires attached worktree clean but ignores local dirty repo. | IPC, filesystem/git | none | public IPC | keep |
| `session-lifecycle.test.ts:1886` | session worktree launch branches from the selected base branch | public-daemon-behavior | Worktree launch uses selected base branch. | IPC, git | none | public worktree response/git state | keep |
| `session-lifecycle.test.ts:1923` | session.create promotes a compatible prepared launch worktree | persistence-guarantee | Prepared worktree is promoted into session. | IPC, DB setup/state | db-proxy | public worktree query | assert-public-api |
| `session-lifecycle.test.ts:1960` | daemon startup cleans cold prepared launch worktrees | persistence-guarantee | Startup cleans stale prepared worktrees. | seeded DB, filesystem/git | DB setup/state | persistence cleanup contract | assert-persistence |
| `session-lifecycle.test.ts:1984` | fileSearch.composerEntries scopes `@` lookups to indexed results under the requested cwd | public-daemon-behavior | Composer file search is cwd-scoped. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2027` | session suggestion routes reject removed `@` triggers | public-daemon-behavior | Removed suggestion trigger routes are rejected. | IPC error | none | public IPC | keep |
| `session-lifecycle.test.ts:2048` | session.composerSuggestions prefers local `$` skills over global duplicates | public-daemon-behavior | Local skills shadow global skills. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2098` | session.composerSuggestions reads `/` commands from the latest ACP history update | public-daemon-behavior | Slash suggestions come from latest ACP command update. | IPC, DB active history | db-proxy setup | public IPC result is primary | keep |
| `session-lifecycle.test.ts:2161` | session.draftSuggestions reads launch-dialog `$` suggestions without a session id | public-daemon-behavior | Draft suggestions work without session id. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2254` | session.launchPreview loads agent capabilities and repository branches for the launch dialog | public-daemon-behavior | Launch preview returns capabilities and branches. | IPC, git, fixture agent log | fixture log | structured fixture event maybe cleaner | keep |
| `session-lifecycle.test.ts:2231` | session.launchPreview reports dirty local checkout state | public-daemon-behavior | Launch preview reports dirty checkout. | IPC, git | none | public IPC | keep |
| `session-lifecycle.test.ts:2246` | session.create checks out the selected local branch before the initial prompt | public-daemon-behavior | Session create checks out selected branch before prompt. | IPC, git, history | none | public IPC/git state | keep |
| `session-lifecycle.test.ts:2285` | session.create refuses local branch checkout with uncommitted changes | public-daemon-behavior | Dirty checkout blocks branch switch. | IPC error, git | none | public IPC | keep |
| `session-lifecycle.test.ts:2307` | session.create promotes compatible launch leases instead of creating a second ACP session | public-daemon-behavior | Launch lease promotion reuses prepared ACP session. | IPC, fixture state | none | public IPC plus fixture agent events | keep |
| `session-lifecycle.test.ts:2349` | session.create falls back to a fresh session for worktree launches | public-daemon-behavior | Worktree launch does not promote incompatible lease. | IPC, fixture state | none | public IPC plus fixture agent events | keep |
| `session-lifecycle.test.ts:2381` | released launch leases remain promotable until delayed cleanup expires | public-daemon-behavior | Released lease remains promotable during cleanup delay. | IPC | time-sensitive | explicit lease state query if needed | keep |
| `session-lifecycle.test.ts:2420` | session.subpackages discovers package manifests breadth-first while skipping ignored directories | public-daemon-behavior | Subpackage discovery order/filtering. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2466` | session.subpackages extends built-in manifests from local Goddard config | public-daemon-behavior | Configured manifests extend subpackage discovery. | IPC, filesystem/config | none | public IPC | keep |
| `session-lifecycle.test.ts:2511` | session.create applies initial model and thinking configuration before the first prompt | public-daemon-behavior | Initial config is applied before prompt. | IPC, fixture agent messages | fixture log | fixture event is acceptable external agent observation | keep |
| `session-lifecycle.test.ts:2567` | session.create applies the foreground prompt to interactive initial prompts by default | public-daemon-behavior | Interactive initial prompt is framed by foreground prompt. | IPC history | none | public history | keep |
| `session-lifecycle.test.ts:2594` | session.create returns before an interactive initial prompt completes | public-daemon-behavior | Interactive initial prompt is deferred after create response. | IPC/history | time-sensitive | public history and status | keep |
| `session-lifecycle.test.ts:2640` | session.create leaves one-shot initial prompts unframed by default | public-daemon-behavior | One-shot prompt is not foreground-framed. | IPC history | none | public history | keep |
| `session-lifecycle.test.ts:2666` | session.configOption.set updates active session config options | public-daemon-behavior | Config option update returns updated session options. | IPC | none | public IPC | keep |
| `session-lifecycle.test.ts:2693` | session.model.set updates active session model | public-daemon-behavior | Model update returns updated session options. | IPC | none | public IPC | keep |
| `session-lifecycle.test.ts:2719` | sync-enabled worktree launch mounts after bootstrap and mirrors bootstrap output | public-daemon-behavior | Sync worktree bootstrap mounts and mirrors output. | IPC, filesystem/process | process fixture | public worktree response plus filesystem | keep |
| `session-lifecycle.test.ts:2765` | session creation fails when fresh worktree bootstrap install exits unsuccessfully | public-daemon-behavior | Bootstrap install failure aborts session create. | IPC error, DB diagnostics | diagnostic assertion | split diagnostic from launch failure if needed | split-diagnostic-contract |
| `workforce.test.ts:35` | daemon IPC discovers and initializes workforce config through daemon-owned handlers | public-daemon-behavior | Workforce config is discovered and initialized through daemon IPC. | IPC, filesystem/config | none | public IPC | keep |
| `workforce.test.ts:97` | daemon workforce event stream rejects inactive repositories | public-daemon-behavior | Workforce stream rejects inactive repo. | IPC stream error | none | public IPC stream | keep |

## Proposed Seams

- Use existing daemon event streams in tests:
  - in-process `daemon.events.stream(...)` for integration tests that start a daemon server directly,
  - IPC `events.stream` for end-to-end daemon-client behavior.
- Add or complete missing events:
  - `config.reload.failed`,
  - `backend.stream.started`,
  - `pull_request.feedback.started` or `pull_request.feedback.launched`,
  - `pull_request.feedback.failed`,
  - `pull_request.feedback.coalesced`,
  - idle shutdown timer state changes,
  - structured worktree bootstrap failure.
- Backend harness delivery acknowledgement for stream events, so tests can know a backend event was delivered without polling logs.
- Public IPC follow-up queries for inbox/session state currently asserted directly through DB rows.
- Continue deriving operational logs from daemon events where practical, avoiding adjacent manual log calls used only for observability.

## Recommended Work Order

1. Convert `daemon.test.ts` assertions that can already use `pull_request.feedback.finished`, `pull_request.feedback.ignored`, and `backend.stream.degraded`.
2. Add missing pull-request/config events for remaining log-proxy assertions: backend stream started, feedback launched/coalesced/failed, and config reload failed.
3. Refactor `config-reload.test.ts` invalid config reload detection to use `config.reload.failed` once available.
4. Decide whether idle shutdown timer diagnostics should become daemon events; if yes, add those events and update idle-shutdown tests to assert them.
5. Split `ipc-security.test.ts` so redaction/correlation/crash-detail assertions live in explicit diagnostic-contract tests.
6. Revisit `session-lifecycle.test.ts` DB assertions opportunistically, replacing `db-proxy` cases with public IPC or existing events where it clarifies the contract.
7. Fully regenerate this audit table after the event-based refactors land, since the current table predates browser-access tests and line-number drift.
