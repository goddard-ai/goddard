# Component: SessionChatTranscript ACP Requirements
- **Goal:** Pin down the ACP-driven component surface that `SessionChatTranscript` must support so the transcript can evolve from a dumb text timeline into a correct Agent Client Protocol session view.
- **Why now:** ACP makes the transcript more than alternating user and assistant text. The client must tolerate streamed message chunks, tool execution updates, permission requests, replayed session history, and plan updates without inventing app-only message shapes.

## Definite Transcript Components

- `UserMessageRow`
  - Must accumulate and render `user_message_chunk` updates into one user turn.
  - Must support ACP baseline prompt content blocks: `text` and `resource_link`.
- `AgentMessageRow`
  - Must accumulate and render `agent_message_chunk` updates into one agent turn.
  - User-visible text should be treated as Markdown by default.
- `AgentThoughtRow`
  - Must handle `agent_thought_chunk` updates when an agent emits them.
  - If the product later chooses to hide thoughts, keep this as a transcript item type with a presentation rule instead of dropping the ACP event shape.
- `ResourceLinkAttachment`
  - Must render linked prompt context for `resource_link` content blocks because ACP requires agents to accept that prompt shape.
  - Should preserve absolute file paths and 1-based line references when the link points at a local file.
- `ToolCallCard`
  - Must render `tool_call` updates with ACP fields such as `toolCallId`, `title`, `kind`, and `status`.
  - Must keep the same visible card alive across `tool_call_update` notifications.
- `ToolCallContentRenderer`
  - Must support ACP tool-call content variants:
    - `content`
    - `diff`
    - `terminal`
  - `diff` requires a transcript-friendly file-change preview.
  - `terminal` requires a live or replayable terminal attachment surface keyed by `terminalId`.
- `ToolCallLocationList`
  - Must render tool call `locations` so the user can follow file reads or edits.
  - Paths must remain absolute and lines must remain 1-based.
- `PermissionRequestCard`
  - Must present pending `session/request_permission` prompts inside the session surface because ACP makes permission approval part of the active turn lifecycle.
  - Must support the ACP option kinds `allow_once`, `allow_always`, `reject_once`, and `reject_always`.
  - Must handle the `cancelled` outcome when the turn is cancelled before the user answers.
- `PlanUpdatePanel`
  - Must render `plan` session updates as complete replacement snapshots, not incremental merges.
  - Each row must support ACP `priority` and `status`.
- `TurnStopState`
  - Must surface the final stop reason for the active prompt turn:
    - `end_turn`
    - `max_tokens`
    - `max_turn_requests`
    - `refusal`
    - `cancelled`
  - `cancelled` is especially important because ACP allows final tool updates to arrive before the cancelled prompt request resolves.

## ACP Updates The Transcript Should Route Elsewhere

- `SessionInfoHeaderState`
  - `session_info_update` belongs in the session header or tab title rather than the transcript body.
- `SessionModeSelector`
  - `current_mode_update` and `session/set_mode` belong in header controls, not inline transcript rows.
- `SessionConfigOptions`
  - `config_option_update` and `session/set_config_option` belong in header or toolbar controls.
- `SessionCommandPalette`
  - `available_commands_update` belongs in composer slash-command UI or command help, not as transcript history rows.

## Capability-Gated Follow-Ons

- `EmbeddedResourceAttachment`
  - Needed only when ACP `promptCapabilities.embeddedContext` is advertised and the client sends `resource` blocks.
- `ImageAttachment`
  - Needed only when ACP `promptCapabilities.image` is advertised.
- `AudioAttachment`
  - Needed only when ACP `promptCapabilities.audio` is advertised.
- `SessionLoadReplayBoundary`
  - Useful once `session/load` is implemented so replayed history and live continuation are visually distinguishable, but not a separate ACP baseline requirement.

## Implications For App Structure

- `SessionChatTranscript` should stop modeling transcript items as plain role-tagged text messages.
- `SessionChatState` should normalize ACP update variants into transcript item records that preserve chunking, tool-call identity, permission state, and plan replacement semantics.
- The transcript renderer should prefer a discriminated row model over a single message bubble component so ACP additions remain additive instead of turning into special cases inside one message type.
