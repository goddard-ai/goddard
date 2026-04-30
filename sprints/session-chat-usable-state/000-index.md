# Sprint: Session Chat Usable State

Status: planned

## Objective

Make session chat usable for daily development by enforcing typed session tab payloads, adding clear load and send behavior, consuming existing daemon stream subscriptions through chat state, exposing useful session status and actions, rendering daily-use ACP transcript rows, and supporting older history pages.

## Base Branch Assumption

Use `main` as the base branch. Current `main` includes `d7e87aa4b`, which already provides the generic daemon stream subscription bridge.

## Ordered Tasks

1. `010-require-session-tab-payloads` - Require session tab payloads at the type level
2. `020-add-session-chat-load-states` - Add recoverable session chat loading, empty, and error states
3. `030-preserve-composer-draft-on-send-failure` - Make session prompt sends draft-safe
4. `050-add-session-chat-state` - Introduce `SessionChatState` for history, live messages, sends, and connection status
5. `060-wire-session-chat-view-to-state` - Move `SessionChatView` onto `SessionChatState`
6. `070-render-session-chat-header-status` - Render session chat header status
7. `080-add-session-chat-header-actions` - Add session chat header actions
8. `090-render-turn-stop-rows` - Render ACP turn stop rows
9. `100-render-permission-request-rows` - Render ACP permission request rows
10. `110-render-plan-update-rows` - Render ACP plan update rows
11. `120-page-older-session-history` - Load older session history pages

## Review Flow

Review one task-sized change at a time. The executor may continue at most one task ahead while review is pending, but must pause work-ahead after `050-add-session-chat-state` and `100-render-permission-request-rows` because those tasks define core state and security-sensitive behavior.
