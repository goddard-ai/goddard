# Safe idle daemon activation

Uppercase `D` in the dev watcher requests conditional shutdown. It never
uses session-count snapshots as shutdown authority and never escalates a slow
or failed conditional shutdown to a signal. Pressing `D` again cancels an uncommitted attempt. Automatic noninteractive
rebuilds use the same protocol. Lowercase `d` and `b` remain
explicit immediate restart commands.

## Owner protocol

Only the primary authenticated connection, with service-owner shutdown enabled,
can use these messages. Unsupported backends refuse by default.

1. `prepareIdleShutdown { requestId }` returns
   `idleShutdown { requestId, fence: UUID | null, committed: false }`.
   Null means busy or unsupported; existing work and admission continue.
2. A non-null fence holds admission for five monotonic seconds. The fence is
   bound to this connection and this daemon incarnation. Duplicate prepare
   for the same request returns the original token without renewing its lease.
3. `commitIdleShutdown { requestId, fence }` rechecks the same lease and
   inventory under the admission mutex and signals server shutdown there.
   Its reply uses `idleShutdown` with `committed: true` only on acceptance.
   New work cannot slip between the check and signal.
4. `releaseIdleShutdown { fence }`, connection teardown, or lease expiry
   releases an uncommitted fence. An expired or foreign token cannot commit
   or release a later fence. A committed shutdown keeps admission closed.

The watcher closes its connection after four seconds without a result and
retries busy attempts. It verifies build readiness before connecting, after
hello, and after prepare. It prevents build execution during the exchange and
through confirmed daemon exit/replacement startup. Source revisions and queued
builds invalidate prepare; no await separates the final readiness check from
sending commit. A lost acknowledgement never authorizes killing a process.

## Work ownership and conservative limits

Admission permits cover backend RPC execution, provider startup, runtime
entries and event forwarders through teardown, internal dispatch and chain
advancement, automation acceptance and workers, automatic prompts, instruction
capture and routing workers, and packaged-resource staging. The final inventory
also refuses while any runtime or terminal remains, any task has a busy status,
open turn or queued prompt, an employee is dispatching/working/finishing,
an automation is pending/running, an archive prune
is running, or a repository map is being built. Counts have no session limit.

This deliberately treats idle provider processes and terminals as busy. It
does not evict them to make activation possible. The sharing service holds a
permit for its entire lifetime because its separate incoming network
admissions do not yet have individual fencing. A daemon with sharing running
therefore stays busy, even between transfers. New RPCs during the short held
fence receive an error asking them to retry after reconnect; internal queue
passes leave their durable work pending. No automatic replay is introduced.

## Qualification and bootstrap

Protocol tests use isolated servers and fake provider drivers. Do not use
live work to test activation. A running daemon built before this protocol
cannot grant a safe shutdown fence; a new watcher must time out and preserve
it. The first upgrade from such a daemon requires a separately coordinated
maintenance boundary. Building these sources is not permission to restart the
old daemon or to resume paused dependency chains.
