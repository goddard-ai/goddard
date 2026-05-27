import { Menu, MenuItem } from "@goddard-ai/ui-primitives"
import { signal } from "@preact/signals"

const open = signal(false)
let trigger: HTMLButtonElement | null = null

/** Demonstrates a button-anchored menu with close-after-select items. */
export function MenuExample() {
  return (
    <>
      <button
        ref={(element) => {
          trigger = element
        }}
        onClick={() => (open.value = true)}
      >
        Actions
      </button>
      <Menu open={open} anchor={() => trigger} ariaLabel="Example actions" class="menu">
        <MenuItem onSelect={() => console.log("Run selected")}>Run</MenuItem>
        <MenuItem disabled>Unavailable</MenuItem>
      </Menu>
    </>
  )
}
