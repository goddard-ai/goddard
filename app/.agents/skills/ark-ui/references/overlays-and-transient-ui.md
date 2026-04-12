# Overlays and Transient UI

Use this reference for overlay-like React Ark components:

- `Dialog`
- `Popover`
- `Tooltip`
- `Hover Card`
- `Menu`
- `Floating Panel`
- `Toast`
- `Tour`

## Shared Contracts

- Keep the documented trigger, backdrop, positioner, content, and close parts present when the component family expects them.
- Portal overlay content unless the surrounding code intentionally keeps it in-tree for layout or environment reasons.
- Let Ark manage focus, dismissal, escape handling, and pointer-down-outside behavior.
- Keep visible titles, descriptions, and labels explicit so the accessible name stays obvious.

## `Dialog`

Use `Dialog` for blocking or semi-blocking content that needs proper focus management and content semantics.

- Keep `Backdrop`, `Positioner`, and `Content` in the expected shape.
- Use `modal={false}` only when outside interaction must remain available.
- Use `lazyMount` and `unmountOnExit` when heavy dialog content should leave the DOM while closed.

## `Popover`, `Tooltip`, and `Hover Card`

Choose by interaction intent:

- `Popover`: interactive anchored content.
- `Tooltip`: short, non-interactive hint content.
- `Hover Card`: richer preview content that appears on hover or focus.

For all three:

- Keep the trigger focusable and semantically correct.
- Preserve `Positioner` and `Content` relationships.
- Prefer `Portal` for viewport-safe layering.

## `Menu`

Use `Menu` for command lists and nested actions, not for freeform dialog content.

- Keep item, item-group, label, and indicator parts intact.
- Preserve typeahead, checked-state, and nested-menu behavior.
- Use `Menu` instead of custom absolutely positioned button lists when keyboard navigation matters.

## `Floating Panel`

Use `Floating Panel` when the content behaves like a dockable or movable tool palette rather than a simple popup.

- Preserve panel chrome and movement affordances.
- Treat mount and exit timing as part of the interaction model.

## `Toast`

Use `Toast` for transient status reporting.

- Keep toaster wiring stable and avoid duplicating transient-state infrastructure.
- Prefer Ark toast controls over bespoke timers and dismissal bookkeeping when Ark is already in use.

## `Tour`

Use `Tour` for multi-step onboarding that points at existing page elements.

- Treat step definitions and target selectors as product logic.
- Validate that steps still point at live elements after UI refactors.

## Cross-Cutting Validation

- Confirm focus enters the overlay predictably and returns correctly after close.
- Confirm overlay layering, portal placement, and scroll behavior match product intent.
- Confirm mount timing still allows exit animation when `lazyMount` or `unmountOnExit` is involved.
