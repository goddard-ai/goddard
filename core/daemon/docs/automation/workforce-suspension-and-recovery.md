# Workforce Suspension and Recovery

> Workforce work is suspended when the daemon cannot safely treat a request as complete or valid. This page explains what suspension means, what remains durable, and how recovery preserves queue integrity.

## Suspension

- Suspended work remains blocked until explicit operator or root-agent action resolves it.
- Suspension is the safe outcome when the daemon cannot validate that a request completed within its ownership boundaries.
- The visible result is a workforce request that still needs a recovery decision rather than a queue item that silently vanished.

## Validation failures

- A response is not a blind completion signal.
- The daemon validates attributable git state and commits before advancing the queue.
- Work that touches paths outside the responding agent's ownership can suspend the request for human review.
- Dirty tracked changes inside the responding agent's owned paths can block completion.

## Session failures

- Individual agent-session failure should not corrupt the broader workforce queue.
- The affected request should remain inspectable so a coordinator can decide the next action.
- The coordinator can retry, update, cancel, truncate, or suspend follow-up work through daemon-owned workforce operations.

## Daemon restart

- Daemon restart should recover operator-visible workforce progress from durable state.
- New work should not be accepted until the runtime has reconstructed its queue state.
- Clients should treat post-restart workforce state as refreshed daemon truth, even if their previous stream showed an older active request.

## Shutdown

- Shutdown stops new handling cleanly.
- Durable intent should be preserved enough for later restart.
- Shutdown is not a validation success signal for active work.

## Boundaries

- Recovery should not silently erase validation problems.
- Clients should inspect daemon workforce state instead of trying to infer recovery from repository files alone.
