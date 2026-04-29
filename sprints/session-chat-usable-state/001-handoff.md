# Executor Handoff

## Sprint Inputs

- Sprint name: `session-chat-usable-state`
- Objective: Make session chat usable for daily development by fixing session tab opening, preserving failed-send drafts, adding live daemon streaming state, exposing lifecycle controls, rendering daily-use ACP lifecycle rows, and supporting older history pages.
- Base branch assumption: current HEAD `dafe4793`; branch from this commit unless a different base is specified.

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

## Sequencing Constraints

- Payload wiring comes first because all chat work depends on a valid session id.
- Draft-safe send behavior comes before live state so state integration does not preserve unsafe composer semantics.
- Subscription bridge must land before live chat state.
- Header controls depend on state-owned lifecycle actions.
- ACP lifecycle rows depend on live/history normalization being centralized.
- Older history paging waits until row identity and state ownership are stable.

## Known Risks

- Confirm detached HEAD `dafe4793` as the base.
- Preserve the app-owned Virtuoso/Comark transcript path unless instructed otherwise.
- Do not pull full action catalog, PR review, terminal replay, or browser preview work into this sprint.
- Keep tasks independently reviewable and pause for close review after subscription bridge and ACP lifecycle row tasks.

## Activity Log

- Initialized sprint files from the confirmed plan.
- Initialized sprint-branch state with base ref `codex/assess-session-chat-readiness`, which pointed at confirmed base commit `dafe4793`.
