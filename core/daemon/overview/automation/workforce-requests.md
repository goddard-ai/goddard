# Workforce Requests

- **Core idea**
  - A workforce request is one queued unit of delegated work for a workforce agent.
  - Requests are recorded through the daemon so queue projection and ownership validation stay centralized.

- **Request**
  - Adds delegated work for a target workforce agent.
  - The daemon returns the updated workforce projection so clients can stay aligned.

- **Update**
  - Appends new information to an existing request.
  - Keeps the request identity and queue position visible through daemon state.

- **Cancel**
  - Cancels an existing request with an optional reason.
  - Cancelled work should not continue as active delegated work.

- **Truncate**
  - Removes pending work for a workforce scope.
  - Useful when a coordinator needs to clear queued work for an agent or broader scope.

- **Respond**
  - Responds to the active request from the handling session.
  - Acts as a validation gate before queue advancement.

- **Suspend**
  - Blocks the active request with a reason.
  - Suspended work remains blocked until explicit recovery.

- **Ordering**
  - Requests for the same workforce agent are handled sequentially.
  - Queue mutations should preserve one daemon-owned projection of current workforce state.
