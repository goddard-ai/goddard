# Session Features and Extensions

## Plans

Agents may expose execution plans through `session/update` using `sessionUpdate: "plan"`.

Each plan entry has:

- `content`
- `priority`: `high`, `medium`, `low`
- `status`: `pending`, `in_progress`, `completed`

Plan rule:

- Send the complete plan on every plan update.
- Clients replace the entire current plan, not merge by entry.

## Modes

Agents may return session modes during `session/new` or `session/load`.

Mode state contains:

- `currentModeId`
- `availableModes`

Each available mode has:

- `id`
- `name`
- optional `description`

Clients can switch modes with `session/set_mode`.
Agents can push mode changes with `current_mode_update`.

If you implement both modes and newer config options, keep them in sync.

## Config Options

ACP prefers configurable session selectors over the older mode-only API.

Agents may return `configOptions` during `session/new` or `session/load`, and may update them later with `config_option_update`.

Current spec notes:

- Option type is currently `select`.
- Each option must always have a default/current value.
- Clients should ignore unknown option types gracefully.
- Agents should order options by importance because clients may use ordering for placement and shortcuts.

Reserved semantic categories:

- `mode`
- `model`
- `thought_level`

Custom categories may start with `_`.

Clients can update a value with `session/set_config_option`.
Agents must respond with the complete current `configOptions` state.

## Slash Commands

Agents may advertise slash commands through `available_commands_update`.

Each command has:

- `name`
- `description`
- optional `input.hint`

Clients run slash commands by sending them as normal prompt text, for example `/plan add search support`.

## Session Metadata

Agents may push session metadata changes through `session_info_update`.

Fields include:

- `title`
- `updatedAt`
- `_meta`

Use this to rename sessions after a meaningful exchange or attach UI metadata.

## Extensibility

ACP reserves `_meta` for arbitrary custom data on all protocol types.

Rules:

- Put custom data in `_meta`, not at the top level of ACP objects.
- Reserve root `_meta` keys `traceparent`, `tracestate`, and `baggage` for W3C trace context.
- Prefix custom method names with `_`.
- Return JSON-RPC "method not found" for unknown custom requests.
- Ignore unknown custom notifications.
- Advertise extension support in capability `_meta` when possible.

## Schema Crib Sheet

Agent-side core methods:

- `authenticate`
- `initialize`
- `session/new`
- `session/load`
- `session/list`
- `session/prompt`
- `session/set_mode`
- `session/set_config_option`

Client-side core methods:

- `session/request_permission`
- `fs/read_text_file`
- `fs/write_text_file`
- `terminal/create`
- `terminal/output`
- `terminal/wait_for_exit`
- `terminal/kill`
- `terminal/release`

Important session update variants:

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

Important enums and unions to keep consistent:

- `StopReason`
- `ToolKind`
- `ToolCallStatus`
- `PermissionOptionKind`
- `RequestPermissionOutcome`
- `ContentBlock`
- `ToolCallContent`

## Review Checklist

- Confirm every optional method is capability-gated.
- Confirm every prompt/update payload uses ACP field names and discriminators exactly.
- Confirm all file paths are absolute and all line numbers are treated as 1-based.
- Confirm cancellation resolves the original prompt request with `cancelled`.
- Confirm terminals are eventually released.
- Confirm `_meta` is preserved and custom fields are not leaking into ACP roots.
