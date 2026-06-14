# 050-create-transparent-overlay-window-shell

## Objective

Add the dedicated overlay window as a separate full-screen transparent surface on the active display.

## Scope

- Add overlay window lifecycle.
- Position and size the overlay to the active display.
- Keep the overlay fully transparent outside the launch surface.
- Focus the launch prompt area.
- Implement outside-click, Escape, close-button, and repeat-shortcut hide behavior.

## Acceptance Criteria

- The overlay appears only on the active display.
- The overlay does not foreground, resize, or maximize the main window.
- The overlay focuses the launch prompt area.
- Hiding the overlay does not destroy in-memory overlay state.

