# Agent reference for @goddard-ai/ui-primitives

This reference is optimized for AI coding agents that need to choose and wire the correct primitive quickly. Exact signatures live in generated declarations.

## Import rule

Use the package root only:

```ts
import {
  Menu,
  MenuItem,
  Modal,
  Popover,
  Tooltip,
  setOverlayPortalRoots,
  startFloatingPosition,
} from "@goddard-ai/ui-primitives"
```

## Setup checklist

1. Configure portal roots once with `setOverlayPortalRoots`.
2. Store open state in a `Signal<boolean>`.
3. Keep anchor refs stable before opening anchored overlays.
4. Pass labels or labelled-by IDs for accessible names.
5. Provide CSS for all visual styling.

## Public exports

| Export | Kind | Use when |
| --- | --- | --- |
| `Modal` | component | Blocking dialog with focus ownership and portal rendering. |
| `Popover` | component | Anchored overlay with floating positioning, dismissal, and optional focus behavior. |
| `Menu` | component | Menu popover with keyboard navigation. |
| `MenuItem` | component | Selectable item inside `Menu`; closes the containing menu after selection. |
| `Tooltip` | component | Non-interactive tooltip attached to one trigger child. |
| `OverlayPortal` | component | Low-level portal helper for overlay primitives. |
| `setOverlayPortalRoots` | function | Configure `dialog` and `menu` host elements. |
| `startFloatingPosition` | function | Position a custom floating element against an element or `{ x, y }` point. |

Exported types: `MenuProps`, `MenuItemProps`, `ModalProps`, `ModalCloseReason`, `PopoverProps`, `PopoverCloseReason`, `OverlayPortalRoot`, `OverlayPortalRootResolver`, `FloatingPoint`, `FloatingReference`, and `FloatingPositionOptions`.

## Component contracts

### `Modal`

Required props:

- `open: Signal<boolean>`
- `titleId: string`

Important optional props:

- `descriptionId`
- `backdropClass`, `positionerClass`, `contentClass`
- `closeOnEscape` defaults to true
- `closeOnOutsidePointer` defaults to false
- `initialFocus`
- `onBeforeClose(reason)`; return false to veto close

Close reasons: `"escape"`, `"explicit"`, `"outside"`.

Use `open.value = false` for explicit close buttons. The component does not render a close button for you.

### `Popover`

Required props:

- `open: Signal<boolean>`
- `anchor: () => Element | { x: number; y: number } | null`

Important optional props:

- Floating options: `placement`, `offset`, `sameWidth`, `strategy`
- `ariaLabel`, `role`, `class`
- `closeOnEscape` defaults to true
- `closeOnOutsidePointer` defaults to true
- `blockOutsidePointer` defaults to `closeOnOutsidePointer`
- `focusOnOpen` defaults to true
- `modalFocus`
- `initialFocus`
- `restoreFocus` defaults to true
- `onOpenChange(open, reason)`

Close reasons: `"escape"`, `"outside"`.

Do not open a popover until `anchor()` can return the mounted anchor. Point anchors are allowed for context-menu-like positioning.

### `Menu` and `MenuItem`

`Menu` wraps `Popover` with:

- `role="menu"`
- fixed strategy
- bottom-start placement
- Escape and outside-pointer dismissal
- initial focus on the first enabled menu item
- ArrowUp, ArrowDown, Home, End, Enter, and Space handling

`MenuItem` renders a button with `role="menuitem"`. Disabled items set `aria-disabled="true"` and do not call `onSelect`.

### `Tooltip`

`Tooltip` accepts exactly one trigger child vnode. It clones that child to add refs, pointer/focus handlers, Escape handling, and `aria-describedby` while open.

Useful props:

- `content`
- `side` defaults to `"top"`
- `sideOffset` defaults to 8
- `openDelay` defaults to 450 ms
- `closeDelay` defaults to 80 ms
- `group` coordinates delayed close/open behavior across related tooltips
- optional controlled `open: Signal<boolean>`

Tooltips use `Popover` with `focusOnOpen={false}`, `restoreFocus={false}`, `role="tooltip"`, and fixed positioning.

## Common patterns

### Modal with explicit close

```tsx
const open = signal(false)

<Modal open={open} titleId="settings-title" descriptionId="settings-description">
  <h2 id="settings-title">Settings</h2>
  <p id="settings-description">Update workspace settings.</p>
  <button onClick={() => (open.value = false)}>Close</button>
</Modal>
```

### Menu anchored to a button

```tsx
const open = signal(false)
let trigger: HTMLButtonElement | null = null

<button ref={(element) => (trigger = element)} onClick={() => (open.value = true)}>
  Actions
</button>
<Menu open={open} anchor={() => trigger} ariaLabel="Actions">
  <MenuItem onSelect={() => runAction()}>Run</MenuItem>
  <MenuItem disabled>Unavailable</MenuItem>
</Menu>
```

### Tooltip

```tsx
<Tooltip content="Saved automatically" side="right">
  <button type="button">Status</button>
</Tooltip>
```

## Avoid these mistakes

- Do not pass React state booleans; use `Signal<boolean>`.
- Do not rely on default portal roots; there are none.
- Do not place interactive content inside `Tooltip`.
- Do not use `MenuItem` outside `Menu` when you need close-after-select behavior.
- Do not duplicate default values downstream; defaults are owned by the primitive implementation.
