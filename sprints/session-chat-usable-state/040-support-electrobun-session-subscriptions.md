# 040-support-electrobun-session-subscriptions

Status: planned

## Title

Add app bridge support for daemon `sessionMessage` subscriptions

## Objective

Let the webview-backed SDK subscribe and unsubscribe to live daemon session messages through the Electrobun bridge.

## Scope

- Extend the Bun/webview bridge to support daemon stream subscriptions for `sessionMessage`.
- Implement `subscribe` in `app/src/daemon-client.ts`.
- Ensure teardown is explicit and idempotent.
- Preserve existing request/response daemon forwarding.

## Dependencies

None.

## Acceptance Criteria

- The Bun/webview bridge can subscribe and unsubscribe to one daemon session stream.
- `app/src/daemon-client.ts` implements `subscribe` instead of throwing.
- Subscription teardown is explicit and idempotent.
- Existing request/response daemon calls continue to work.

## Constraints And Risks

- Review carefully because this changes host lifecycle behavior.
- The bridge must not leak daemon subscribers when tabs close, reload, or switch.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

