# 020-add-session-chat-load-states

Status: planned

## Title

Add recoverable session chat loading, empty, and error states

## Objective

Make session chat render clear, recoverable states for initial load, empty history, missing sessions, and daemon or history failures.

## Scope

- Add user-visible loading state for initial session and history reads.
- Add empty transcript state that keeps the composer usable.
- Add error state with a clear retry or reopen path.

## Dependencies

- `010-pass-workbench-tab-payloads`

## Acceptance Criteria

- Initial session and history loads have a visible loading state.
- Missing session, daemon failure, or history failure renders a clear retry/reopen path.
- Empty history renders a usable composer instead of an ambiguous blank transcript.

## Constraints And Risks

- This task should not introduce live streaming or broader state ownership yet.
- Use local app patterns for query and presentation state.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

