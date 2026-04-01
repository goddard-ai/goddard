# Content, Tools, and IO

## Prompt and Message Content

ACP reuses MCP-style `ContentBlock` structures where possible.

Supported content block variants:

- `text`
- `image`
- `audio`
- `resource`
- `resource_link`

Rules:

- `text` is always required in prompts.
- `resource_link` is always required in prompts.
- `image` requires `promptCapabilities.image`.
- `audio` requires `promptCapabilities.audio`.
- `resource` requires `promptCapabilities.embeddedContext`.

Use `resource` when the client already has the bytes or text and wants to avoid extra round trips. Use `resource_link` when the agent can fetch the resource itself.

## Tool Calls

Report each model-requested tool invocation with a `tool_call` update, then keep it current with `tool_call_update`.

Common tool fields:

- `toolCallId`
- `title`
- `kind`
- `status`
- `content`
- `locations`
- `rawInput`
- `rawOutput`

Tool kinds include:

- `read`
- `edit`
- `delete`
- `move`
- `search`
- `execute`
- `think`
- `fetch`
- `switch_mode`
- `other`

Tool statuses:

- `pending`
- `in_progress`
- `completed`
- `failed`

Update rules:

- `tool_call_update` only needs changed fields.
- Preserve the same `toolCallId`.
- Replace `content` or `locations` collections when updating them.

## Tool Call Content

Tool call content can be:

- Regular content blocks via `{ "type": "content", "content": ... }`
- Diffs via `{ "type": "diff", "path": ..., "oldText": ..., "newText": ... }`
- Terminals via `{ "type": "terminal", "terminalId": ... }`

Use `diff` for user-visible file edits. Use terminal embedding when command output should stream live in the client.

## Permission Requests

Request user approval with `session/request_permission` when a tool should not run immediately.

Inputs:

- `sessionId`
- `toolCall`
- `options`

Permission option kinds:

- `allow_once`
- `allow_always`
- `reject_once`
- `reject_always`

Outcomes:

- `selected` with `optionId`
- `cancelled`

If the prompt turn is cancelled, the client must answer pending permission requests with `cancelled`.

## Following the Agent

Use tool call `locations` to expose files being accessed or modified.

- `path` must be absolute.
- `line` is optional and 1-based.

This enables editor features like follow-along navigation.

## Filesystem Bridge

Use client filesystem methods only if the client advertised support during `initialize`.

Available methods:

- `fs/read_text_file`
- `fs/write_text_file`

`fs/read_text_file`:

- Send `sessionId`
- Send absolute `path`
- Optionally send `line`
- Optionally send `limit`
- Expect `{ "content": "..." }`

`fs/write_text_file`:

- Send `sessionId`
- Send absolute `path`
- Send full file `content`
- Expect `null` on success

The client should expose unsaved editor state through these methods where applicable.

## Terminal Bridge

Use terminal methods only if the client advertised `terminal: true`.

Available methods:

- `terminal/create`
- `terminal/output`
- `terminal/wait_for_exit`
- `terminal/kill`
- `terminal/release`

Typical flow:

1. Call `terminal/create` with `command`, `args`, optional `env`, absolute `cwd`, and optional `outputByteLimit`.
2. Embed the returned `terminalId` in a tool call if live output should be visible.
3. Poll `terminal/output` or await `terminal/wait_for_exit`.
4. Optionally call `terminal/kill` for timeouts.
5. Always call `terminal/release` when finished.

Notes:

- `terminal/create` returns immediately with a `terminalId`.
- `terminal/output` may also return `exitStatus`.
- `terminal/release` invalidates the terminal ID and kills the process if it is still running.
- A released terminal can still remain visible in the client when it was already attached to a tool call.

## MCP Server Wiring

Clients may provide MCP server configs in `session/new` or `session/load`.

Supported MCP server transport shapes:

- Stdio is required for agents.
- HTTP is optional and gated by `mcpCapabilities.http`.
- SSE is optional and gated by `mcpCapabilities.sse`.

Stdio MCP config includes:

- `name`
- `command`
- `args`
- `env`

HTTP/SSE MCP config includes:

- `type`
- `name`
- `url`
- `headers`

When implementing an agent, connect to all provided MCP servers when feasible and reject unsupported transport types unless the advertised capabilities allow them.
