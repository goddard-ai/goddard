# Session Chat Usable State Plan

## Goal

Get session chat to the point where a developer can open a session, see live progress, send follow-ups safely, understand what happened, and recover from common failures without leaving the app.

## Phase 1: Fix The Basic Tab Path

1. Pass active detail-tab payloads into registered tab components.
   - Update `app/src/app-shell/views.tsrx` so `SessionChatView` receives `sessionId`.
   - Smoke test opening a session from the session list and from the launch dialog.

2. Add local chat loading, error, and empty states.
   - A missing session, failed history load, or daemon error should render a recoverable UI instead of a blank panel.

Acceptance: opening any existing session reliably shows the correct transcript or a clear error.

## Phase 2: Make Sending Safe

3. Preserve composer drafts when sends fail.
   - Let submit errors propagate or return an explicit failure result from `Composer` and `SessionInput`.
   - Show a small inline or toast error.
   - Clear the editor only after the daemon accepts the prompt.

4. Add visible send state.
   - Disable duplicate sends while the request is in flight.
   - Make queued or accepted state obvious enough that users do not spam Enter.

Acceptance: network, daemon, or send failures never silently discard user input.

## Phase 3: Add Live Session State

5. Implement Electrobun-backed daemon subscriptions.
   - Extend the Bun and webview RPC bridge to support `sessionMessage`.
   - Replace the current throwing implementation in `app/src/daemon-client.ts`.

6. Introduce `SessionChatState`.
   - Own history page loading, live append, connection state, send status, draft state, and teardown.
   - Keep view components presentational.
   - Start with a provider keyed by session id, then avoid broader realtime architecture until another feature needs it.

7. Wire `SessionChatView` through `SessionChatState`.
   - Initial load from `session.history`.
   - Subscribe while the tab is active or cached.
   - Merge live ACP messages into the visible transcript without waiting for manual query invalidation.

Acceptance: active sessions stream agent text and tool updates into the chat without refresh or another prompt.

## Phase 4: Add The Session Header

8. Implement `SessionChatHeader`.
   - Show title, repository or project, lifecycle status, blocked reason, and connection mode.
   - Add actions for reconnect, stop or cancel active turn, and open changes.
   - Add PR and contextual action hooks later if those surfaces are not available yet.

Acceptance: users can tell whether the session is active, blocked, done, errored, or disconnected, and can take the obvious next action.

## Phase 5: Make The Transcript ACP-Usable

9. Expand transcript normalization beyond plain messages.
   - Add row types for `turnStop`, `permissionRequest`, `agentThought`, and `planUpdate`.
   - Keep tool call identity stable across updates.
   - Preserve prompt `resource_link` blocks.

10. Render daily-use row types.
    - Permission requests must be actionable inline.
    - Stop reasons must be visible.
    - Tool diffs and terminal references can stay compact, with deeper views added later.

Acceptance: a normal coding-agent turn can be understood from the transcript: prompt, response, tool activity, permissions, and how the turn ended.

## Phase 6: History And Recovery

11. Add older-history paging.
    - Use `nextCursor` and `hasMore`.
    - Load older turns when scrolling near the top.
    - Preserve scroll position when older rows are prepended.

12. Add reconnect and reload behavior.
    - Reconnect live sessions when possible.
    - Fall back to history mode clearly when live connection is unavailable.

Acceptance: long sessions are usable, and reopening the app or tab does not make chat feel broken or truncated.

## Suggested Implementation Order

1. `fix(app): pass workbench tab payloads`
2. `fix(app): preserve session composer drafts on send failure`
3. `feat(app): support daemon session subscriptions over electrobun`
4. `feat(app): add session chat state for history and live updates`
5. `feat(app): add session chat header controls`
6. `feat(app): render ACP permission and turn stop transcript rows`
7. `feat(app): page older session chat history`

This order gets risk out early: first make the tab open correctly, then protect user input, then make the surface live. Components after that are meaningful because the underlying session loop works.
