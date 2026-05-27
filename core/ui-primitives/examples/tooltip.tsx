import { Tooltip } from "@goddard-ai/ui-primitives"

/** Demonstrates a non-interactive tooltip attached to one trigger child. */
export function TooltipExample() {
  return (
    <Tooltip content="Saved automatically" side="right" group="status-actions">
      <button type="button">Status</button>
    </Tooltip>
  )
}
