# Daemon-Backed ACP Session Bridge

## Current Decision

Keep the SDK daemon-backed ACP bridge private and minimal until acp-client exposes a composable primitive for already-initialized, daemon-routed sessions.

`sdk.session.run(...)` returns an `AgentSession` wrapper for callers that want to interact with a live daemon-owned ACP session. The daemon owns the real agent process, ACP initialization, persistence, permissions, and message history. The SDK only attaches a client-side bridge to the daemon's `session.send` and `session.streamMessages` routes.

## Current Duplication

`core/sdk/src/daemon/session/client.ts` currently duplicates ACP client machinery that acp-client already owns in its initialized connection path:

- pending request ids and response correlation;
- JSON-RPC response and error handling;
- ACP client method dispatch for `session/update`, permission requests, filesystem, terminal, elicitation, and extension methods;
- NDJSON framing between object streams and daemon IPC routes;
- stream shutdown behavior for pending requests.

This duplication exists because acp-client's public `createAcpClient()` helper owns the `initialize` handshake. The SDK bridge is attaching to a session that the daemon already initialized and persisted.

## Minimal Upstream Primitive

The useful upstream primitive is an initialized client-side ACP bridge that does not run `initialize`.

One possible shape:

```ts
const bridge = createInitializedAcpClientBridge({
  sessionId: "acp-session-1",
  stream,
  handler,
  agentCapabilities,
})

await bridge.prompt({ sessionId: "acp-session-1", prompt })
await bridge.close()
```

Required inputs:

- an already-open `acp.Stream` or equivalent readable/writable `AnyMessage` transport;
- the active ACP session id;
- the agent capabilities observed by the daemon during `initialize`;
- an `AcpClientHandler` for client-side ACP requests and notifications;
- optional request id prefix or request id factory for host diagnostics.

Required outputs:

- a small session-scoped handle with `prompt()`, `cancel()` if supported, and `close()`;
- request promises that resolve or reject from JSON-RPC responses;
- dispatch of client requests and notifications through the supplied handler;
- protocol-compatible JSON-RPC errors for unsupported client methods.

Lifecycle behavior:

- Do not send `initialize`, `session/new`, or `session/load`.
- Reject pending requests when the stream closes or errors.
- Let callers close the bridge without implying daemon session shutdown.
- Preserve extension method and notification routing for `_`-prefixed methods.

Capability behavior:

- Use the daemon-provided `agentCapabilities` for optional session methods.
- Do not infer new capabilities from the SDK handler; the daemon already negotiated capabilities with the agent.
- Continue treating omitted capabilities as unsupported.

## Upstream Feature Request Draft

Title: Expose an initialized ACP client bridge for daemon-routed sessions

Request:

Hosts that own daemon-managed ACP sessions sometimes need SDK consumers to attach to an already-initialized ACP session through host routes instead of connecting directly to the agent process. Today those hosts must duplicate JSON-RPC request tracking, client method dispatch, extension routing, and stream shutdown behavior because `createAcpClient()` always performs `initialize`.

Please expose a runtime-neutral helper that binds an already-initialized `acp.Stream` plus known agent capabilities into a session-scoped client handle. The helper should not create, load, or initialize a session. It should only send session-scoped agent requests, dispatch client-side requests/notifications to an `AcpClientHandler`, and handle JSON-RPC response/error lifecycle consistently with `AcpClient`.

This would let Goddard remove its private `DaemonBackedAcpClient` while keeping daemon-owned process launch, initialization, session persistence, permission state, and history storage unchanged.
