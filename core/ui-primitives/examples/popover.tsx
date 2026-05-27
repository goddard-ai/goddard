import { Popover } from "@goddard-ai/ui-primitives"
import { signal } from "@preact/signals"

const open = signal(false)
let trigger: HTMLButtonElement | null = null

/** Demonstrates the standard anchored overlay contract: caller-owned signal, stable anchor ref, and external styling. */
export function PopoverExample() {
  return (
    <>
      <button
        ref={(element) => {
          trigger = element
        }}
        onClick={() => (open.value = true)}
      >
        Open popover
      </button>
      <Popover
        open={open}
        anchor={() => trigger}
        ariaLabel="Example popover"
        class="popover"
        placement="bottom-start"
      >
        <p>Popover content is styled by the application.</p>
        <button onClick={() => (open.value = false)}>Close</button>
      </Popover>
    </>
  )
}
