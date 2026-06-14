# 080-add-async-launch-toasts-and-recovery

## Objective

Make overlay submission snappy with loading-to-success/failure toasts and retry/edit recovery.

## Scope

- Reset the overlay form immediately on submit.
- Add bottom-right overlay toast lifecycle.
- Keep completion from foregrounding the main Goddard window.
- Retain failed submitted payloads in toast state.
- Add Retry and Edit actions, with protection against silently overwriting a new draft.

## Acceptance Criteria

- Submit does not wait for launch completion.
- Overlay remains visible after submit.
- Success/failure updates the relevant toast.
- Failure can retry the same payload or restore it for editing.
- The main Goddard window does not come forward after overlay launch.

