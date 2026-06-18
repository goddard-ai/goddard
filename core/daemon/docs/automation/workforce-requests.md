# Workforce Requests

> Workforce requests are queued units of delegated agent work inside one repository workforce. This page explains how requests enter the queue, change over time, and advance only through daemon-owned validation.

- **Core idea**
  - A workforce request is one queued unit of delegated work for a workforce agent.
  - Requests are recorded through the daemon so queue projection and ownership validation stay centralized.

- **Request**
  - Adds delegated work for a target workforce agent.
  - The daemon returns the updated workforce projection so clients can stay aligned.
  - A requested item is pending until the daemon selects it for handling by the target agent.

- **Update**
  - Appends new information to an existing request.
  - Keeps the request identity and queue position visible through daemon state.
  - Updates should refine the same delegated concern rather than create a hidden second queue item.

- **Cancel**
  - Cancels an existing request with an optional reason.
  - Cancelled work should not continue as active delegated work.
  - Cancellation is a queue decision; it should be visible in daemon workforce state so operators can understand why work stopped.

- **Truncate**
  - Removes pending work for a workforce scope.
  - Useful when a coordinator needs to clear queued work for an agent or broader scope.
  - Truncation should not pretend already active handling completed successfully.

- **Respond**
  - Responds to the active request from the handling session.
  - Acts as a validation gate before queue advancement.
  - A response can complete work only after daemon validation accepts the result.
  - If validation fails, the request remains a recovery concern rather than disappearing from the queue projection.

- **Suspend**
  - Blocks the active request with a reason.
  - Suspended work remains blocked until explicit recovery.
  - Suspension preserves the request and reason so a coordinator or root agent can decide how to proceed.

- **Ordering**
  - Requests for the same workforce agent are handled sequentially.
  - Queue mutations should preserve one daemon-owned projection of current workforce state.
  - Related pages: [workforce](./workforce.md), [workforce suspension and recovery](./workforce-suspension-and-recovery.md), and [session tokens](../sessions/session-tokens.md).
