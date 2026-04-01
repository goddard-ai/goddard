# ACP Overview

## Mental Model

ACP standardizes communication between a coding agent and an editor-style client.

- The protocol uses JSON-RPC 2.0.
- Methods are request/response pairs.
- Notifications are one-way messages with no response.
- User-readable text defaults to Markdown.
- All file paths in protocol payloads must be absolute.
- Line numbers are 1-based.

## Required Baseline

Every agent must support:

- `initialize`
- `session/new`
- `session/prompt`
- `session/cancel`
- `session/update`

Every client should support:

- `session/request_permission`

Every agent must accept prompt content blocks of:

- `text`
- `resource_link`

Treat omitted capabilities as unsupported.

## Initialization

Start every ACP connection with `initialize`.

- The client sends its latest supported major `protocolVersion`.
- The client advertises `clientCapabilities`.
- The client should send `clientInfo`.
- The agent returns the chosen `protocolVersion`.
- The agent advertises `agentCapabilities`.
- The agent should send `agentInfo`.
- The agent may advertise `authMethods`.

Version rule:

- If the agent supports the client’s requested version, it must echo that version.
- Otherwise it must return the latest version it supports.
- If the client cannot support the returned version, it should disconnect.

Capability highlights:

- Client filesystem: `fs.readTextFile`, `fs.writeTextFile`
- Client terminals: `terminal`
- Agent prompt content: `promptCapabilities.image`, `audio`, `embeddedContext`
- Agent MCP transports: `mcpCapabilities.http`, `sse`
- Agent session features: `loadSession`, `sessionCapabilities.list`

## Session Lifecycle

Create a session with `session/new`.

- Send `cwd` as an absolute path.
- Optionally send `mcpServers`.
- Expect a `sessionId`.
- The agent may also return initial `modes` and `configOptions`.

Load a session with `session/load` only if `loadSession` is advertised.

- Send `sessionId`, `cwd`, and any `mcpServers`.
- Expect the agent to replay prior conversation entries through `session/update`.
- Expect the method response only after replay completes.

List sessions with `session/list` only if `sessionCapabilities.list` is present.

- Optional inputs: `cwd`, `cursor`
- Expect `sessions` plus optional `nextCursor`
- Treat cursors as opaque

## Prompt Turn

Run a user turn with `session/prompt`.

- Send `sessionId`.
- Send `prompt` as an array of content blocks.
- Restrict prompt content to the capabilities negotiated in `initialize`.

During execution, the agent streams `session/update` notifications such as:

- `user_message_chunk`
- `agent_message_chunk`
- `agent_thought_chunk`
- `tool_call`
- `tool_call_update`
- `plan`
- `available_commands_update`
- `current_mode_update`
- `config_option_update`
- `session_info_update`

Finish the turn by resolving the original `session/prompt` request with a `stopReason`.

## Stop Reasons

Use the ACP stop reasons:

- `end_turn`
- `max_tokens`
- `max_turn_requests`
- `refusal`
- `cancelled`

If the client sends `session/cancel`:

- The client should immediately mark unfinished tool calls as cancelled in its UI.
- The client must answer any pending permission requests with outcome `cancelled`.
- The agent should stop model/tool work promptly.
- The agent may still send final updates before completing the prompt request.
- The agent must finish the prompt request with stop reason `cancelled`.

## Transports

ACP is transport-agnostic, but the spec currently defines stdio and discusses streamable HTTP.

For stdio:

- Launch the agent as a subprocess.
- Exchange ACP JSON-RPC messages over stdin/stdout.
- Encode messages as UTF-8.
- Delimit each JSON-RPC message with a newline.
- Do not write non-ACP data to stdout.
- Use stderr only for logs.
