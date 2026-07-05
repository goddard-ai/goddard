# UI primitives context

`@goddard-ai/ui-primitives` provides behavior-only overlay building blocks for Preact applications. It is not a design system and does not own visual styling.

## Core model

- Open state is caller-owned through `Signal<boolean>`.
- Components render nothing when `open.value` is false.
- Overlays render through configured portal roots, not in place.
- Callers provide labels, visible content, classes, and close buttons.
- Dismissal and focus behavior are centralized so product surfaces do not reimplement it.

## Portal roots

Call `setOverlayPortalRoots` during app setup before any overlay opens.

```ts
setOverlayPortalRoots({
  dialog: () => document.getElementById("dialog-root"),
  menu: () => document.getElementById("overlay-root"),
})
```

Root meanings:

- `dialog`: modal dialog surfaces and backdrops.
- `menu`: anchored overlays such as popovers, menus, and tooltips.

If a root resolves to `null`, `OverlayPortal` renders nothing. This is intentional and keeps primitives decoupled from the app shell.

## Lifecycle invariants

- A `Popover` must have a mounted anchor when it opens. If `anchor()` returns `null`, it logs an error and does not complete positioning or dismissal setup.
- `Modal` and `Popover` register themselves with the internal overlay stack while open so nested overlays dismiss in the right order.
- Focus is restored on close by default for `Modal` and `Popover` unless `Popover.restoreFocus` is false.
- `Tooltip` does not move focus into the tooltip. It wires pointer, focus, Escape, delay, and `aria-describedby` behavior onto its single trigger child.
- `Menu` is a popover preset with menu keyboard navigation and `MenuItem` selection behavior.

## API selection

Use `Modal` for blocking dialog content that should own focus and use `role="dialog"`.

Use `Popover` for anchored overlay content where the caller owns the trigger and content semantics.

Use `Menu` and `MenuItem` for action lists that need menu roles, roving keyboard movement, and close-after-select behavior.

Use `Tooltip` for non-interactive explanatory content attached to one trigger element.

Use `useListNavigation` for indexed list surfaces that need shared keyboard movement,
active-row DOM attributes, disabled-row skipping, pointer hover highlighting, and
scroll-into-view behavior without giving the primitive ownership of item rendering.
Use `onActiveIndexChange` when consumers need preview-on-highlight behavior while
leaving active-row ownership with the primitive.
The hook keeps keyboard and programmatic active-row movement visible by calling
`scrollIntoView` with default `{ block: "nearest" }`; pointer-origin hover
changes update the active row without scrolling. Callers still own the scroll
container, overflow styling, item rendering, and `itemRef(index)` wiring. It also
exposes focus helpers for registered rows and lets callers suppress pointer
highlighting while keyboard or typeahead movement is in progress.

Use `useSearchNavigation` when a search input should wire input, keyboard, active-row,
and activation behavior through DOM refs while inheriting `useListNavigation`
scroll visibility behavior. The caller keeps filtering, ranking, list layout,
and viewport styling local.

Use `startFloatingPosition` only when building a custom primitive that needs the same Floating UI positioning behavior without rendering `Popover`.

Use `OverlayPortal` only when building a new overlay primitive inside this package or a tightly coupled Goddard UI package.

## Styling contract

The package accepts class names and explicit style props but ships no CSS. `Modal` exposes separate backdrop, positioner, and content style props because it renders three surfaces. `Popover`, `Menu`, `MenuItem`, and `Tooltip` expose `style` on their primary rendered surface.

Overlay content receives inline positioning styles from `startFloatingPosition`, including:

- `position`, `top`, and `left` for placement.
- `--available-height` and `--available-width` CSS variables.
- `--reference-width` for same-width or responsive layouts.
- bounded `maxHeight`, `maxWidth`, `minHeight`, `minWidth`, and `width` values when relevant.

Callers should avoid using style props to override positioning-owned properties on anchored overlays.

## Accessibility responsibilities

The primitives provide behavior, not complete content accessibility. Callers must still provide:

- Stable title and description elements for `Modal.titleId` and `Modal.descriptionId`.
- Meaningful `ariaLabel` values when visible labels are absent, or `ariaLabelledBy` when a visible element names the overlay.
- Non-disabled interactive children where keyboard focus should land.
- Explicit close controls for modal dialogs and long-lived popovers.
