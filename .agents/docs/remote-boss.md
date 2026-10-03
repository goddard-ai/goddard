# Cross-machine Boss design

## Current connection surface

The desktop app already stores remote daemon records (`RemoteHost`) and runs a
`DaemonSupervisor` for each saved host. Direct hosts connect to the daemon's
WebSocket endpoint with its bearer token. On Unix, SSH hosts establish a
forwarded local endpoint; the token is read from the remote machine during
provisioning and is not stored locally. The supervisor reconnects in the
background with backoff, and remote catalog data can remain visible while a
host is offline.

Boss state is daemon-owned. The app requests `BossOperation::View` from each
connected daemon and indexes the resulting state by `DaemonKey` (`Local` or a
remote host id). The sidebar already creates one Boss row per available state;
employee sessions remain associated with the daemon that owns them. Remote
Boss rows now show the configured machine name as their subtitle, while
retaining the same avatar, actions, and row behavior as the local Boss. Boss is
not a `PersistedSidebarGroup`: these rows are generated from live daemon Boss
states, not session-history folds.

## Connection lifecycle

1. The user saves a direct endpoint and token, or an SSH destination, in Remote
   Hosts settings.
2. The app starts the remote supervisor outside the render path. SSH prompts
   are permitted only for an explicit interactive connection action; background
   reconnects are noninteractive.
3. The authenticated daemon client performs the normal versioned Hello
   handshake and loads that daemon's settings, task catalog, and Boss view.
4. The app stores each machine's Boss state under its stable remote-host id.
   Disconnects do not merge or migrate Boss, employee, or memory state. The
   host supervisor can reconnect and refresh its own snapshot.
5. Removing a remote host drops the local connection and its local cached
   catalog. It does not modify the remote daemon's data.

## Trust and authentication

The direct WebSocket protocol authenticates with a bearer token in the Hello
message. The token is a secret and is stored with app settings, which are
written with restrictive file permissions. The token grants access to the
remote daemon API; it is not scoped to Boss messages. Direct `ws://` traffic
has no transport encryption, so it must be limited to a trusted network or
protected by a secure tunnel. Use `wss://` where configured or the existing SSH
port forward across untrusted networks. SSH host identity and user
authorization are delegated to the user's SSH configuration and keys.

A future Boss-to-Boss message operation must preserve this authority boundary:
the sender may submit text to the remote Boss, but must not gain control of
remote employees, access remote memory files, or act as the remote Boss. The
remote daemon remains the sole authority for its sessions, employee actions,
personas, and memory. A remote host's existing daemon token remains broad
administrative trust; adding a narrower Boss-only token is a separate security
project.

## Boss message protocol (proposed)

`BossOperation` currently has no message operation. Add a daemon-mediated
`Message` request that targets only the daemon's own Boss session, with a
request id for deduplication and a bounded text payload. The receiving daemon
validates the request, queues or submits it through the existing Boss session
prompt path, and returns an accepted response containing the local Boss session
id and message id. The reply is the Boss's ordinary transcript output, streamed
or replayed through the existing session subscription; a request id links the
response to the originating send. The sender does not receive employee
credentials or memory contents except what the remote Boss chooses to include
in its reply.

The operation should be additive and versioned through the existing daemon
protocol, with explicit errors for unsupported protocol versions, unavailable
Boss sessions, rejected input, and duplicate request ids. It must run through
the daemon command path off the UI thread and must not couple either machine's
Boss lifecycle or persistence. Implementing this operation requires protocol,
daemon, client, and conversation UI work, so it is outside the current
connection-and-sidebar milestone.
