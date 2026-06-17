# Cancellation and Steering

- **Cancellation**
  - Cancellation asks the daemon to stop active work for one live session.
  - The daemon also reports queued prompts it aborted instead of replaying silently.
  - Clients can decide whether to resubmit those aborted prompts later.

- **Steering**
  - Steering is cancel-and-reprompt behavior owned by the daemon.
  - It cancels active work, waits for a safe boundary, then injects one replacement prompt.
  - This gives client surfaces a supported way to redirect active work without hand-rolling timing against raw agent traffic.

- **Safe boundary**
  - The daemon waits for a boundary where replacement work can be injected without mixing it into the cancelled turn.
  - Conceptually, that boundary is after cancellation has produced enough agent-side signal to avoid treating late updates as part of the new prompt.

- **Boundaries**
  - Cancellation does not mean queued work should be replayed automatically.
  - Steering is for redirecting active work, not editing historical records.
  - Both actions require a live session; history-only sessions can be inspected but not actively steered.
