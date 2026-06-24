# Core Daemon Test Audit Report

Initial audit used `sem entities` against the daemon-owned integration tests and the adjacent daemon-facing agent service unit tests.

Event-system refresh on 2026-06-24 regenerated the current event catalog summary and the detailed audit table. The audited set now contains 132 test cases across 12 files, and the table below matches the current inventory and line-number anchors.

This report maps current test cases to the contract they appear to protect, the assertion surfaces they use, and whether better seams would make the tests less coupled to daemon internals or logs. It is an audit only; it does not refactor tests.

## Summary

- Test files audited: 12 (`core/daemon/test/*.test.ts` plus the daemon-adjacent `features/agent/test/{install-service,update-scheduler}.test.ts` unit files).
- Detailed table rows: 132.
- Current test inventory: 132 test cases.
- The highest-priority audit refactors from the previous work order are now landed:
  - daemon runtime startup/feedback assertions no longer depend on incidental logs,
  - config reload failure coverage uses `config.reload.failed`,
  - IPC security logging assertions are split from behavior assertions,
  - idle auto-shutdown coverage uses `session.idle_shutdown.updated` instead of persisted diagnostics as a synchronization seam.
- Remaining follow-up is lower priority and mostly opportunistic:
  - a structured worktree bootstrap failure event would still clarify launch-failure coverage if that area grows,
  - a few large end-to-end tests still bundle several observable steps and could be split later if they become flaky or harder to maintain,
  - direct DB reads are now mostly concentrated in tests that explicitly protect persistence, restart repair, or migration behavior.
- The daemon now has a unified event system with both IPC `events.stream` and an in-process `daemon.events.stream` test seam. Runtime events are logged automatically by `observeDaemonEventsForLogging` when emitted through the daemon event bus.
- Several previously proposed seams now exist, including `backend.stream.started`, `backend.stream.degraded`, `config.reload.failed`, `pull_request.feedback.launched`, `pull_request.feedback.coalesced`, `pull_request.feedback.failed`, `pull_request.feedback.finished`, `pull_request.feedback.ignored`, and `session.idle_shutdown.updated`.
- Public follow-up queries such as `session.get`, `session.history`, `session.list`, `inbox.list`, and `pr.get` now cover most earlier `db-proxy` candidates in config reload, session lifecycle, and IPC security coverage.
- The main remaining seam gap is a structured worktree bootstrap failure event; no comparably urgent log-proxy gap remains in the current daemon runtime tests.

## Counts

Primary contract categories from the current table:

| Contract type | Count | Notes |
|---|---:|---|
| `public-daemon-behavior` | 90 | Runtime, browser-access, IPC, session, worktree, and workforce flows now mostly assert public or event seams. |
| `diagnostic-contract` | 12 | Logging format, log mode, redaction, crash detail, and explicit correlation assertions where logs are the contract. |
| `persistence-guarantee` | 14 | Store migrations, restart recovery, durable session/worktree/title/history state. |
| `test-harness-infrastructure` | 10 | Config/schema/backend client wrappers, fixture agents, timers, daemon package wrappers. |
| `implementation-coupled` | 6 | Service-level internals and direct helper behavior without a public daemon surface. |
| `missing-seam` | 0 | Earlier daemon/config/idle gaps are now covered by typed events or public follow-up queries. |

Common smells:

| Smell | Where it appears |
|---|---|
| `log-proxy` | Mostly retired from daemon runtime/config reload/IPC security; remaining log assertions are focused diagnostic-contract tests. |
| `db-proxy` | Narrowed to durable-state and migration assertions in `session-lifecycle.test.ts` and `mock-seed.test.ts`. |
| `internal-import` | Service/config/schema/session tests intentionally import daemon internals. |
| `fake-first-party` | Agent install/update service tests use local fake service collaborators. |
| `time-sensitive` | Long-running daemon/session lifecycle and browser-access tests still use polling, streaming, and timeouts. |
| `multi-contract` | Browser pairing and queued-prompt cancellation still bundle several observable steps. |

## Current Unified Event Status

Current daemon event infrastructure:

- `startDaemonServer()` composes `daemonRuntimeEvents` with plugin events and returns `daemon.events`, so integration tests can observe events in-process without going through logs.
- IPC exposes the same composed stream through `events.stream`, with name and exact payload-property filters.
- The IPC server observes daemon events and logs them automatically with `eventId` and `eventAt`, using debug scopes when event definitions request debug logging.

Events that already cover earlier audit suggestions:

| Earlier need | Current event status | Notes |
|---|---|---|
| config reload failed | `config.reload.failed` exists | Covers rejected hot-reload attempts without counting incidental logs. |
| stream subscription started | `backend.stream.started` exists | Covers successful runtime stream startup when IPC-owned handlers are present. |
| feedback ignored | `pull_request.feedback.ignored` exists | Covers unmanaged pull requests. |
| feedback launched/coalesced/failed | `pull_request.feedback.launched`, `pull_request.feedback.coalesced`, and `pull_request.feedback.failed` exist | Covers runtime feedback lifecycle without bespoke log names. |
| feedback flow completed | `pull_request.feedback.finished` exists | Earlier report used the proposed name `pull_request.feedback.finish`; current code uses `finished`. |
| stream subscription degraded | `backend.stream.degraded` exists | Covers unauthenticated stream startup. |
| idle shutdown timer state | `session.idle_shutdown.updated` exists | Covers idle timer `started`, `cancelled`, and `expired` transitions. |
| session message stream | `session.message` exists | Replaces older session message stream-specific IPC routes and is used by idle-shutdown subscriber tests. |
| session lifecycle updates | `session.lifecycle.updated` and `session.lifecycle.deleted` exist | Useful for connection/status/list invalidation behavior. |
| session worktree and launch lifecycle | `session.worktree.prepared`, `session.persisted`, `session.activated`, `session.launch.finished`, `session.launch.failed`, `session.stopping` exist | Covers many launch/worktree/restart observations. |
| inbox updates | `inbox.item.updated` exists | Good replacement for some direct inbox DB assertions. |
| pull request attention updates | `pull_request.created` and `pull_request.updated` exist | Good replacement for some PR/inbox coupling assertions when attention is the contract. |

Remaining event gaps to consider:

| Missing or incomplete event | Tests/behavior it would help | Suggested payload |
|---|---|---|
| worktree bootstrap failure event | Session launch/bootstrap failure tests still pair public IPC errors with stored diagnostics. | sessionId if allocated, requested cwd, worktree info when available, phase, exit code/error message. |

Most former `replace-log-with-event` candidates have now been converted. The regenerated rows below focus on whether the remaining assertions are clean public seams, true persistence guarantees, or candidates for a future structured worktree failure seam.

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
| `config-reload.test.ts:46` | config manager promotes valid root config edits and preserves the last good snapshot after invalid edits | public-daemon-behavior | Invalid local config does not replace the last good snapshot and emits a typed reload failure. | config manager snapshots, `config.reload.failed` event | time-sensitive | public event seam is adequate | keep |
| `config-reload.test.ts:164` | action.run picks up updated root-config agent defaults without restarting the daemon | public-daemon-behavior | Action sessions use refreshed root config. | IPC action.run/session shutdown | time-sensitive | IPC surface is adequate | keep |
| `config-reload.test.ts:224` | pull request feedback handler picks up updated root-config agent defaults without restarting the daemon | public-daemon-behavior | PR feedback flow uses refreshed root config. | feature backend event handler, `pull_request.feedback.finished`/`failed`, `session.list`, `session.get`, `session.shutdown` | time-sensitive | public IPC and event seams are adequate | keep |
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
| `daemon.test.ts:61` | daemon run subscribes once and launches managed PR feedback sessions across repositories | public-daemon-behavior | Stream feedback creates completed sessions for matching repositories. | backend harness, `pull_request.feedback.finished`, session follow-up queries | time-sensitive | current event and IPC seams are adequate | keep |
| `daemon.test.ts:234` | daemon run can start only the IPC server when stream is disabled | public-daemon-behavior | IPC starts without stream subscription. | health check, backend count | none | public runtime surface is adequate | keep |
| `daemon.test.ts:274` | daemon run skips backend stream without IPC-owned backend event handlers | public-daemon-behavior | Backend stream does not start when no feature-owned handler can run. | backend harness, `backend.stream.started`, session follow-up queries | time-sensitive | current event and IPC seams are adequate | keep |
| `daemon.test.ts:308` | daemon run keeps IPC available when stream startup is unauthenticated | public-daemon-behavior | IPC remains available when stream subscription degrades. | health check, backend count, `backend.stream.degraded` | none | public runtime surface is adequate | keep |
| `daemon.test.ts:381` | daemon run defaults to compact terminal logs | diagnostic-contract | Default terminal log mode is compact. | stdout | none | log output is contract | keep |
| `daemon.test.ts:397` | daemon run supports raw json terminal logs when requested | diagnostic-contract | JSON log mode writes raw JSON entries. | stdout JSON | none | log output is contract | keep |
| `daemon.test.ts:414` | daemon run supports verbose terminal logs with expanded fields | diagnostic-contract | Verbose log mode expands fields. | stdout | none | log output is contract | keep |
| `daemon.test.ts:431` | daemon run logs startup failures after logging is configured | diagnostic-contract | Startup failures are logged after logger setup. | logs, exit code | none | log output is contract | keep |
| `daemon.test.ts:454` | daemon URL round-trips the TCP address | test-harness-infrastructure | Daemon URL parser round-trips host/port. | pure helper | none | helper contract | keep |
| `daemon.test.ts:464` | daemon runtime resolves the global daemon port override | public-daemon-behavior | Runtime honors `GODDARD_DAEMON_PORT`. | env, health check, IPC client | process/env | IPC health is adequate | keep |
| `ipc-security.test.ts:51` | daemon submit request rejects invalid session tokens | public-daemon-behavior | Invalid session tokens are rejected before handler dispatch. | IPC error | none | public IPC error | keep |
| `ipc-security.test.ts:65` | daemon submit request redacts invalid session tokens in IPC logs | diagnostic-contract | Invalid-token requests redact the token and preserve request/failure correlation in IPC logs. | logs | explicit diagnostic assertion | log security is the contract | keep |
| `ipc-security.test.ts:95` | daemon browser access is unavailable until explicitly enabled | public-daemon-behavior | Browser-access bootstrap routes fail closed when the feature is disabled. | HTTP response | none | public HTTP surface | keep |
| `ipc-security.test.ts:108` | daemon browser access preflight and origin validation fail closed | public-daemon-behavior | Browser-access CORS and private-network handling only allow configured origins. | HTTP preflight/response headers | none | public HTTP surface | keep |
| `ipc-security.test.ts:146` | daemon browser pairing issues origin-bound revocable tokens | public-daemon-behavior | Pairing requires local confirmation, binds tokens to the requesting origin, supports browser event streams, and revokes access cleanly. | HTTP routes, browser IPC client, local IPC client, metadata state | multi-contract, time-sensitive | split token issuance from browser-stream access if this flow grows | split-test |
| `ipc-security.test.ts:311` | daemon desktop webview tokens are host-bootstrapped and origin-checked | public-daemon-behavior | Desktop webview tokens can only be created by a trusted local client and replay from the approved origin. | HTTP routes, local IPC client | none | public HTTP and IPC surfaces | keep |
| `ipc-security.test.ts:353` | daemon hides unexpected handler crashes from IPC clients | public-daemon-behavior | Unexpected handler failures surface as generic IPC errors. | IPC error | none | public IPC error | keep |
| `ipc-security.test.ts:391` | daemon logs unexpected handler crashes after returning generic IPC errors | diagnostic-contract | Unexpected handler failures retain internal error detail in IPC failure logs. | logs | explicit diagnostic assertion | log contract | keep |
| `ipc-security.test.ts:435` | daemon submit request enforces trusted repo context and records created PR access | public-daemon-behavior | PR submit uses the trusted session repo, updates session PR permissions, and records the created PR for follow-up queries. | backend call, `session.get`, `inbox.list`, `pr.get` | time-sensitive | current IPC follow-up seams are adequate | keep |
| `ipc-security.test.ts:529` | daemon submit request correlates IPC request and response logs after resolving session scope | diagnostic-contract | Successful IPC logs preserve a shared op id while enriching the resolved session scope. | logs | explicit diagnostic assertion | log contract | keep |
| `ipc-security.test.ts:567` | daemon submit request honors repository-local security deny policy | public-daemon-behavior | Local security deny blocks PR submission. | IPC error, backend calls | none | public IPC/backend collaborator | keep |
| `ipc-security.test.ts:617` | daemon reply request rejects PRs outside the session allowlist | public-daemon-behavior | Session token cannot reply to unallowed PR. | IPC error | none | public IPC | keep |
| `ipc-security.test.ts:644` | daemon reply request records pull request checkout locations | public-daemon-behavior | PR reply records checkout context and creates a follow-up attention item that is inspectable through public IPC queries. | `inbox.list`, `pr.get` | time-sensitive | current IPC follow-up seams are adequate | keep |
| `ipc-security.test.ts:687` | daemon session reporting creates and updates session inbox rows | public-daemon-behavior | Session reporting drives inbox row lifecycle and emits matching inbox update events. | IPC, `inbox.list`, `events.stream` | time-sensitive | public IPC and event seams are adequate | keep |
| `ipc-security.test.ts:749` | daemon workforce request rejects mismatched roots for token-backed sessions | public-daemon-behavior | Workforce request enforces token root. | IPC error, seeded DB | seed setup only | public IPC | keep |
| `ipc-security.test.ts:777` | daemon workforce respond rejects mismatched roots for token-backed sessions | public-daemon-behavior | Workforce response enforces token root. | IPC error, seeded DB | seed setup only | public IPC | keep |
| `ipc-security.test.ts:804` | daemon workforce request rejects token-backed sessions without a workforce root | public-daemon-behavior | Workforce request requires workforce root for token session. | IPC error, seeded DB | seed setup only | public IPC | keep |
| `logging.test.ts:13` | compact logging flattens plain object fields one level | diagnostic-contract | Compact daemon log formatting. | log output | none | logging is contract | keep |
| `logging.test.ts:49` | json logging preserves null-valued daemon context fields | diagnostic-contract | JSON daemon logs preserve ambient context nulls. | log output | none | logging is contract | keep |
| `logging.test.ts:103` | snapshot logger preserves captured async context outside the original run | diagnostic-contract | Logger snapshots preserve context. | log output | none | logging is contract | keep |
| `logging.test.ts:157` | debug logger writes scoped durable rows without terminal output | diagnostic-contract | Debug logs persist without terminal output. | log store | none | logging is contract | keep |
| `mock-seed.test.ts:29` | seed mock writes deterministic isolated fixture data through the daemon IPC surface | public-daemon-behavior | Mock profile seeding creates inspectable IPC fixture data. | IPC queries | none | public IPC | keep |
| `mock-seed.test.ts:116` | seed mock reset is mock-profile only and repeated seeding does not duplicate records | persistence-guarantee | Mock seeding is idempotent and profile-isolated. | store counts | db-proxy acceptable | persistence/counts are contract | assert-persistence |
| `session-lifecycle.test.ts:219` | daemon store repairs duplicate session turn rows before adding unique constraints | persistence-guarantee | Store migration repairs duplicate turn rows. | DB migration store | none | persistence is contract | assert-persistence |
| `session-lifecycle.test.ts:376` | daemon revokes session tokens when agent processes exit | persistence-guarantee | Agent exit revokes token/permissions. | IPC create, DB state | db-proxy | public token resolve after exit plus DB for persistence | assert-persistence |
| `session-lifecycle.test.ts:403` | daemon persists repository context into durable session storage | persistence-guarantee | Repository metadata is stored on session. | IPC create, DB state | db-proxy | session.get if it later exposes metadata; DB if durable storage is contract | assert-persistence |
| `session-lifecycle.test.ts:432` | daemon resolves the default agent for direct session creation | public-daemon-behavior | Session create uses configured default agent. | IPC create/shutdown | none | public IPC | keep |
| `session-lifecycle.test.ts:456` | loadable sessions remain reconnectable after shutdown | public-daemon-behavior | Shutdown loadable session can reconnect and stream a later prompt. | IPC, stream, `session.get`, `session.history` | time-sensitive | public IPC is adequate | keep |
| `session-lifecycle.test.ts:524` | session completion hides from the default list but stays interactive | public-daemon-behavior | Completed sessions hide from the default list but remain usable and reactivate on later prompts. | IPC list/get/history/send, `inbox.list` | time-sensitive | public IPC follow-up is adequate | keep |
| `session-lifecycle.test.ts:599` | loadable sessions remain reconnectable after daemon restart | public-daemon-behavior | Restarted daemon can reconnect a loadable session. | IPC, `session.get`, `session.history` | none | public session connection queries are adequate | keep |
| `session-lifecycle.test.ts:639` | session reconnect fails when the resolved agent no longer supports ACP session/load | public-daemon-behavior | Reconnect rejects an unsupported agent. | IPC, DB update setup | seed/setup only | public IPC error | keep |
| `session-lifecycle.test.ts:668` | daemon persists ACP stop reasons on the session record | persistence-guarantee | ACP stop reason is stored durably on the session record. | `session.get` | none | public session query is adequate | keep |
| `session-lifecycle.test.ts:687` | daemon coalesces stored agent message chunks while keeping the live stream granular | persistence-guarantee | Live stream stays granular while stored history is coalesced. | `session.message` stream, `session.history` | time-sensitive | public history and stream seams are adequate | keep |
| `session-lifecycle.test.ts:777` | daemon stores usage updates on the session instead of durable turn history | persistence-guarantee | Usage updates update session context usage and are excluded from turn history. | `session.get`, `session.history`, `session.message` stream | time-sensitive | public session and history queries are adequate | keep |
| `session-lifecycle.test.ts:839` | daemon creates placeholder session titles before any user prompt is sent | public-daemon-behavior | Session create returns a placeholder title. | IPC create | none | public IPC | keep |
| `session-lifecycle.test.ts:856` | daemon derives a fallback title immediately when the session starts with an initial prompt | public-daemon-behavior | Initial prompt derives a fallback title. | IPC create | none | public IPC | keep |
| `session-lifecycle.test.ts:874` | daemon promotes placeholder titles after the first later prompt is accepted | persistence-guarantee | Title state updates after the first later prompt. | IPC send, `session.get` polling | time-sensitive | public session query is adequate | keep |
| `session-lifecycle.test.ts:908` | daemon marks pending title generation as failed when provider config is present but unusable | persistence-guarantee | Title generation failure updates session title state. | IPC, `session.get` polling | time-sensitive | public session query is adequate | keep |
| `session-lifecycle.test.ts:947` | daemon reconciles interrupted sessions on restart and leaves archived history readable | persistence-guarantee | Restart reconciliation archives interrupted sessions and keeps history/diagnostics readable. | seeded DB, IPC get/history/diagnostics | DB setup | public IPC after seeded state | keep |
| `session-lifecycle.test.ts:1036` | daemon promotes interrupted turn drafts into incomplete turn history on restart | persistence-guarantee | Restart promotes a draft into incomplete turn history. | seeded DB, IPC history | DB setup | public IPC after seeded state | keep |
| `session-lifecycle.test.ts:1131` | multiple clients can observe the same live session stream independently | public-daemon-behavior | Multiple stream subscribers receive the same live session updates. | IPC subscriptions | time-sensitive | public stream | keep |
| `session-lifecycle.test.ts:1176` | daemon auto-shuts down idle loadable sessions with no connected clients | public-daemon-behavior | Idle loadable sessions shut down and expose idle lifecycle updates. | `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and session seams are adequate | keep |
| `session-lifecycle.test.ts:1211` | session idle auto-shutdown uses configured duration | public-daemon-behavior | Configured idle timeout controls the emitted idle lifecycle update. | `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and session seams are adequate | keep |
| `session-lifecycle.test.ts:1245` | session.message event stream subscribers cancel idle auto-shutdown before expiry | public-daemon-behavior | Session-message subscribers cancel the idle timer before expiry. | `session.message` stream, `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and stream seams are adequate | keep |
| `session-lifecycle.test.ts:1282` | session lifecycle subscribers do not cancel idle auto-shutdown | public-daemon-behavior | Lifecycle subscribers do not hold sessions alive. | session lifecycle stream, `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and stream seams are adequate | keep |
| `session-lifecycle.test.ts:1384` | idle auto-shutdown waits for the last session.message event stream subscriber to disconnect | public-daemon-behavior | Idle shutdown waits until the last session-message subscriber disconnects. | daemon event stream, `session.message` stream, `session.get` | time-sensitive | public event and stream seams are adequate | keep |
| `session-lifecycle.test.ts:1443` | busy loadable sessions do not time out until they become quiescent | public-daemon-behavior | Active turns delay idle shutdown until the session becomes quiescent. | `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and session seams are adequate | keep |
| `session-lifecycle.test.ts:1476` | sessions waiting on permission responses do not time out until the permission resolves | public-daemon-behavior | Pending permission responses delay idle shutdown. | IPC prompt flow, `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and session seams are adequate | keep |
| `session-lifecycle.test.ts:1523` | sessions without session/load support never use idle auto-shutdown | public-daemon-behavior | Unsupported sessions never start idle auto-shutdown. | `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and session seams are adequate | keep |
| `session-lifecycle.test.ts:1553` | manual session shutdown clears any pending idle auto-shutdown timer | public-daemon-behavior | Manual shutdown cancels any pending idle timer. | `session.shutdown`, `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and session seams are adequate | keep |
| `session-lifecycle.test.ts:1581` | daemon shutdown clears pending idle auto-shutdown timers | public-daemon-behavior | Daemon shutdown cancels pending idle timers. | daemon shutdown, `session.idle_shutdown.updated` | time-sensitive | public event seam is adequate | keep |
| `session-lifecycle.test.ts:1608` | agent process exit clears pending idle auto-shutdown timers | public-daemon-behavior | Agent exit clears pending idle timers without expiring them. | `session.get`, `session.idle_shutdown.updated` | time-sensitive | public event and session seams are adequate | keep |
| `session-lifecycle.test.ts:1640` | daemon queues concurrent prompts per session and drains them in arrival order | public-daemon-behavior | Concurrent prompts are serialized in arrival order. | IPC stream/send | time-sensitive | public stream/order | keep |
| `session-lifecycle.test.ts:1700` | daemon cancel returns queued prompts, emits terminal errors for queued raw prompts, and prevents them from being sent | public-daemon-behavior | Cancellation returns queued prompts and rejects queued raw prompts. | IPC stream/cancel | multi-contract | split cancel response from raw-prompt stream errors if needed | split-test |
| `session-lifecycle.test.ts:1790` | daemon steering ignores message chunks and dispatches on tool updates | public-daemon-behavior | Steering waits for a tool boundary. | IPC stream/steer | time-sensitive | public stream | keep |
| `session-lifecycle.test.ts:1876` | daemon steering falls back to the cancelled prompt response when no tool boundary appears | public-daemon-behavior | Steering fallback returns the cancelled prompt response. | IPC stream/steer | time-sensitive | public stream | keep |
| `session-lifecycle.test.ts:1947` | session worktree opt-in maps cwd into a real worktree subdirectory | public-daemon-behavior | Worktree launch maps `cwd` to a real worktree subdirectory. | IPC, filesystem/git | none | public session/worktree response | keep |
| `session-lifecycle.test.ts:1978` | session.changes reads tracked and untracked diff content from the session workspace root | public-daemon-behavior | Session changes returns tracked and untracked diff content. | IPC, filesystem/git | none | public IPC | keep |
| `session-lifecycle.test.ts:2017` | session completion enforces worktree cleanliness without blocking local dirty repos | public-daemon-behavior | Completion requires an attached worktree to be clean but ignores local dirty repos. | IPC, filesystem/git | none | public IPC | keep |
| `session-lifecycle.test.ts:2101` | session worktree launch branches from the selected base branch | public-daemon-behavior | Worktree launch uses the selected base branch. | IPC, git | none | public worktree response/git state | keep |
| `session-lifecycle.test.ts:2142` | session.create promotes a compatible prepared launch worktree | persistence-guarantee | A prepared worktree is promoted into the session. | IPC, DB setup/state | db-proxy | public worktree query | assert-public-api |
| `session-lifecycle.test.ts:2179` | daemon startup cleans cold prepared launch worktrees | persistence-guarantee | Startup cleans stale prepared worktrees. | seeded DB, filesystem/git | DB setup/state | persistence cleanup contract | assert-persistence |
| `session-lifecycle.test.ts:2203` | fileSearch.composerEntries scopes `@` lookups to indexed results under the requested cwd | public-daemon-behavior | Composer file search is cwd-scoped. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2246` | session suggestion routes reject removed `@` triggers | public-daemon-behavior | Removed suggestion trigger routes are rejected. | IPC error | none | public IPC | keep |
| `session-lifecycle.test.ts:2267` | session.composerSuggestions prefers local `$` skills over global duplicates | public-daemon-behavior | Local skills shadow global skills. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2317` | session.composerSuggestions reads `/` commands from the latest ACP history update | public-daemon-behavior | Slash suggestions come from the latest ACP command update. | IPC, DB active history | DB setup only | public IPC result is primary | keep |
| `session-lifecycle.test.ts:2380` | session.draftSuggestions reads launch-dialog `$` suggestions without a session id | public-daemon-behavior | Draft suggestions work without a session id. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2409` | session.launchPreview loads agent capabilities and repository branches for the launch dialog | public-daemon-behavior | Launch preview returns capabilities and repository branches. | IPC, git, fixture agent log | fixture log | structured fixture event maybe cleaner | keep |
| `session-lifecycle.test.ts:2450` | session.launchPreview reports dirty local checkout state | public-daemon-behavior | Launch preview reports dirty checkout state. | IPC, git | none | public IPC | keep |
| `session-lifecycle.test.ts:2465` | session.create checks out the selected local branch before the initial prompt | public-daemon-behavior | Session create checks out the selected branch before the initial prompt. | IPC, git, history | none | public IPC/git state | keep |
| `session-lifecycle.test.ts:2504` | session.create refuses local branch checkout with uncommitted changes | public-daemon-behavior | Dirty checkout blocks branch switching. | IPC error, git | none | public IPC | keep |
| `session-lifecycle.test.ts:2526` | session.create promotes compatible launch leases instead of creating a second ACP session | public-daemon-behavior | Launch lease promotion reuses the prepared ACP session. | IPC, fixture state | none | public IPC plus fixture agent events | keep |
| `session-lifecycle.test.ts:2568` | session.create falls back to a fresh session for worktree launches | public-daemon-behavior | Worktree launch does not promote an incompatible lease. | IPC, fixture state | none | public IPC plus fixture agent events | keep |
| `session-lifecycle.test.ts:2600` | released launch leases remain promotable until delayed cleanup expires | public-daemon-behavior | A released lease remains promotable during the cleanup delay. | IPC | time-sensitive | explicit lease state query if needed | keep |
| `session-lifecycle.test.ts:2639` | session.subpackages discovers package manifests breadth-first while skipping ignored directories | public-daemon-behavior | Subpackage discovery preserves breadth-first order and ignores configured directories. | IPC, filesystem | none | public IPC | keep |
| `session-lifecycle.test.ts:2685` | session.subpackages extends built-in manifests from local Goddard config | public-daemon-behavior | Configured manifests extend subpackage discovery. | IPC, filesystem/config | none | public IPC | keep |
| `session-lifecycle.test.ts:2730` | session.create applies initial model and thinking configuration before the first prompt | public-daemon-behavior | Initial config is applied before the first prompt. | IPC, fixture agent messages | fixture log | fixture event is acceptable external agent observation | keep |
| `session-lifecycle.test.ts:2786` | session.create applies the foreground prompt to interactive initial prompts by default | public-daemon-behavior | Interactive initial prompts are framed by the foreground prompt by default. | IPC history | none | public history | keep |
| `session-lifecycle.test.ts:2813` | session.create returns before an interactive initial prompt completes | public-daemon-behavior | Interactive initial prompts continue after the create response returns. | IPC/history | time-sensitive | public history and status | keep |
| `session-lifecycle.test.ts:2859` | session.create leaves one-shot initial prompts unframed by default | public-daemon-behavior | One-shot initial prompts are not foreground-framed by default. | IPC history | none | public history | keep |
| `session-lifecycle.test.ts:2885` | session.configOption.set updates active session config options | public-daemon-behavior | Config-option updates return updated session options. | IPC | none | public IPC | keep |
| `session-lifecycle.test.ts:2912` | session.model.set updates active session model | public-daemon-behavior | Model updates return updated session options. | IPC | none | public IPC | keep |
| `session-lifecycle.test.ts:2938` | sync-enabled worktree launch mounts after bootstrap and mirrors bootstrap output | public-daemon-behavior | Sync worktree bootstrap mounts after bootstrap and mirrors bootstrap output. | IPC, filesystem/process | process fixture | public worktree response plus filesystem | keep |
| `session-lifecycle.test.ts:2984` | session creation fails when fresh worktree bootstrap install exits unsuccessfully | public-daemon-behavior | Bootstrap install failure aborts session create. | IPC error, DB diagnostics | diagnostic assertion | future structured worktree bootstrap failure event | split-diagnostic-contract |
| `workforce.test.ts:35` | daemon IPC discovers and initializes workforce config through daemon-owned handlers | public-daemon-behavior | Workforce config is discovered and initialized through daemon IPC. | IPC, filesystem/config | none | public IPC | keep |
| `workforce.test.ts:97` | daemon workforce event stream rejects inactive repositories | public-daemon-behavior | Workforce stream rejects inactive repo. | IPC stream error | none | public IPC stream | keep |

## Proposed Seams

- Use existing daemon event streams in tests:
  - in-process `daemon.events.stream(...)` for integration tests that start a daemon server directly,
  - IPC `events.stream` for end-to-end daemon-client behavior.
- Prefer `session.get`, `session.history`, `session.list`, `inbox.list`, and `pr.get` over direct store reads unless the test is explicitly about durable storage, restart repair, or migration behavior.
- Add a structured worktree bootstrap failure event if launch/bootstrap failure coverage needs finer-grained assertions than the current IPC error plus diagnostics pairing.
- Consider a backend harness delivery acknowledgement only if event-stream tests become flaky enough that timeout-based synchronization is no longer acceptable.
- Continue deriving operational logs from daemon events where practical, so explicit log assertions stay reserved for diagnostic-contract tests.

## Recommended Work Order

1. Keep the new event and public follow-up seams as the default for future daemon runtime, config reload, IPC security, and idle-shutdown coverage.
2. Treat remaining direct DB assertions as acceptable when the test is explicitly about restart repair, migration, or durable stored state; otherwise prefer a public follow-up query.
3. If worktree bootstrap failure coverage expands, add a structured bootstrap failure event before introducing more diagnostics-only assertions.
4. Re-run the audit whenever browser-access, worktree launch, or daemon runtime behavior changes materially enough to add new mixed-contract tests.
