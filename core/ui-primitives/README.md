# @goddard-ai/ui-primitives

Unstyled Preact overlay primitives for Goddard UI packages: modal dialogs, anchored popovers, menus, tooltips, portals, and floating positioning.

## Fit

Use this package when you need:

- Preact components that own overlay behavior but leave styling to the app.
- Signal-driven open state with `@preact/signals`.
- Accessible focus, dismissal, portal, and keyboard behavior for common overlays.
- Floating UI placement without adopting a full component system.

Do not use it when you need:

- Styled widgets or design tokens.
- React components; the package targets Preact.
- Server-only rendering; these primitives use DOM APIs, focus, portals, pointer events, and layout measurement.
- A general-purpose overlay framework with arbitrary root names or extension hooks.

## Requirements and tradeoff

Hard requirements:

- Preact 10 or newer as a peer dependency.
- Browser DOM APIs.
- `@preact/signals` state for controlled overlay visibility.
- Host applications must configure portal roots before overlays render.

Primary tradeoff: the package keeps behavior centralized and styling external. You get predictable shared overlay mechanics, but callers must provide markup labels, CSS classes, app-specific layout, and state ownership.

## Minimal proof

This example demonstrates the core contract: configure portal roots once, own `open` as a signal, anchor the overlay to a DOM element, and style externally.

```tsx
import { signal } from "@preact/signals"
import { Popover, setOverlayPortalRoots } from "@goddard-ai/ui-primitives"

setOverlayPortalRoots({
  dialog: () => document.getElementById("dialog-root"),
  menu: () => document.getElementById("overlay-root"),
})

const open = signal(false)
let button: HTMLButtonElement | null = null

export function ExamplePopover() {
  return (
    <>
      <button ref={(element) => (button = element)} onClick={() => (open.value = true)}>
        Open
      </button>
      <Popover
        open={open}
        anchor={() => button}
        ariaLabel="Example actions"
        class="popover"
        placement="bottom-start"
      >
        <button onClick={() => (open.value = false)}>Close</button>
      </Popover>
    </>
  )
}
```

## Documentation map

- [`docs/agent-reference.md`](./docs/agent-reference.md): compact API and usage reference optimized for coding agents.
- [`docs/context.md`](./docs/context.md): concepts, lifecycle rules, invariants, and API-selection guidance.
- [`examples/`](./examples): copyable component patterns for portal setup, popovers, modals, menus, and tooltips.
- Generated declarations in `dist/index.d.ts`: exact published signatures after build.

## Public entrypoint

Import all public API from the package root:

```ts
import { Menu, MenuItem, Modal, Popover, Tooltip } from "@goddard-ai/ui-primitives"
```
