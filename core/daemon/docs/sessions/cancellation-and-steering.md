# Cancellation and Steering

Live session work can be cancelled or redirected without clients talking directly to the agent process. This page explains how the daemon stops active work, preserves or reports queued prompts, and injects replacement direction safely.

## Cancellation

- Cancellation asks the daemon to stop active work for one live session.
- The daemon also reports queued prompts it aborted instead of replaying silently.
- Clients can decide whether to resubmit those aborted prompts later.
- If cancellation reaches a session after the active turn already ended, clients should treat the daemon's final session state as authoritative.
- Cancellation does not delete history, diagnostics, or attached worktree metadata.

## Steering

- Steering is cancel-and-reprompt behavior owned by the daemon.
- It cancels active work, waits for a safe boundary, then injects one replacement prompt.
- This gives client surfaces a supported way to redirect active work without hand-rolling timing against raw agent traffic.
- Existing queued prompts stay queued after the replacement prompt and continue draining in their original order.
- Steering is useful when the user wants to preserve the session while replacing the immediate direction of work.

## Safe boundary

- The daemon waits for a boundary where replacement work can be injected without mixing it into the cancelled turn.
- Conceptually, that boundary is after cancellation has produced enough agent-side signal to avoid treating late updates as part of the new prompt.

## Boundaries

- Cancellation does not mean queued work should be replayed automatically; steering preserves queued work behind the replacement prompt.
- Steering is for redirecting active work, not editing historical records.
- Both actions require a live session; history-only sessions can be inspected but not actively steered.
- Recovery is daemon-owned: if a client loses connection during cancellation or steering, it should reload session history and lifecycle state from the daemon.
