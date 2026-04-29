# Sprint: Session Chat Usable State

Status: planned

## Objective

Make session chat usable for daily development by fixing session tab opening, preserving failed-send drafts, adding live daemon streaming state, exposing lifecycle controls, rendering daily-use ACP lifecycle rows, and supporting older history pages.

## Base Branch Assumption

Current HEAD `dafe4793`; branch from this commit unless a different base is specified.

## Ordered Tasks

1. `010-pass-workbench-tab-payloads` - Pass detail-tab payloads into tab components
2. `020-add-session-chat-load-states` - Add recoverable session chat loading, empty, and error states
3. `030-preserve-composer-draft-on-send-failure` - Make session prompt sends draft-safe
4. `040-support-electrobun-session-subscriptions` - Add app bridge support for daemon `sessionMessage` subscriptions
5. `050-add-session-chat-state` - Introduce `SessionChatState` for history, live messages, sends, and connection status
6. `060-wire-session-chat-view-to-state` - Move `SessionChatView` onto `SessionChatState`
7. `070-add-session-chat-header-controls` - Add the session chat header and lifecycle actions
8. `080-render-turn-stop-and-permission-rows` - Render daily-use ACP lifecycle rows
9. `090-page-older-session-history` - Load older session history pages

## Review Flow

Review one task-sized change at a time. The executor may continue at most one task ahead while review is pending, but should pause after `040-support-electrobun-session-subscriptions` and `080-render-turn-stop-and-permission-rows` because those have the highest lifecycle and protocol risk.

