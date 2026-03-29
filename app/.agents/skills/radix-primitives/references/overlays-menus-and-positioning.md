# Overlays, Menus, And Positioning

Use this file for popup structure, overlay selection, trigger behavior, layering, or popper-style placement.

## Dialog Family

### `Dialog`

- Use for general-purpose modal workflows, editors, and task UIs.
- Preserve the usual structure: `Root`, `Trigger`, `Portal`, optional `Overlay`, `Content`, `Title`, `Description`, and one or more `Close` controls.
- Keep `Title` and `Description` meaningful because focus moves into the dialog and that copy becomes the context screen readers hear.
- Use `asChild` on `Trigger` or `Close` when integrating with design-system buttons.

### `AlertDialog`

- Use for destructive or high-consequence confirmation, not generic form editing.
- Preserve `Cancel` and `Action` parts so there is an obvious safe exit.
- Keep the title and description explicit and concrete about the consequence being confirmed.

## Anchored Overlays

### `Popover`

- Use for small interactive content anchored to a trigger or explicit anchor.
- Preserve `Root`, `Trigger`, optional `Anchor`, optional `Portal`, `Content`, optional `Arrow`, and optional `Close`.
- Reach for `sideOffset`, alignment props, and collision handling before inventing custom positioning code.
- Use the exposed trigger-width, available-height, transform-origin, `data-side`, and `data-align` hooks for sizing and animation.
- Prefer `Popover` over `Tooltip` when the content contains form controls, buttons, or anything beyond a short hint.

### `HoverCard`

- Use for supplemental preview information revealed on hover or focus.
- Treat it as non-essential content. Do not hide required actions or mandatory context in it.

### `Tooltip`

- Use for short descriptive text tied to hover or focus.
- Wrap the app or feature area in `Tooltip.Provider` when you need shared delay behavior.
- Preserve the usual structure: `Provider`, `Root`, `Trigger`, optional `Portal`, `Content`, optional `Arrow`.
- If a tooltip trigger is a custom component, use `asChild` and keep the rendered element interactive and focusable.
- Do not turn `Tooltip` into a menu or editor surface. If the content needs interaction, switch to `Popover` or `Dialog`.

## Menus

### `DropdownMenu`

- Use for command menus opened from a trigger button.
- Expect structures built from `Trigger`, `Portal`, `Content`, `Item`, `Separator`, `Label`, `Group`, `Sub`, `SubTrigger`, `SubContent`, `CheckboxItem`, `RadioGroup`, and `RadioItem`.
- Use menu items for actions, not persisted form values.

### `ContextMenu`

- Use when the interaction is invoked from right click or a context gesture.
- Preserve the same menu semantics as `DropdownMenu`, but let the context trigger own opening behavior.

### `Menubar`

- Use for a persistent application menu bar with top-level triggers and nested menu content.
- Keep menu items command-oriented and keyboard navigable.

## `Select`

- Use for button-triggered single-value selection with typeahead and managed focus.
- Preserve the common structure: `Root`, `Trigger`, `Value`, `Icon`, optional `Portal`, `Content`, optional scroll buttons, `Viewport`, `Item`, `ItemText`, `ItemIndicator`, and optional `Group`, `Label`, `Separator`, and `Arrow`.
- Keep `ItemText` limited to the text that should appear in the trigger when selected.
- Use `Group` plus `Label` for sectioned options.
- By default `Select.Content` positions more like a native menu. Set `position="popper"` when you want Popover-like `side`, `align`, and offset behavior.
- Use `placeholder` on `Value` and the `data-placeholder` styling hook on `Trigger` for empty state presentation.
- If you need a custom scrollbar, compose `Select` with `ScrollArea` rather than rebuilding the viewport yourself.

## `Toast`

- Use for transient feedback that is safe to ignore.
- Preserve `Provider`, `Root`, `Title`, `Description`, optional `Action`, optional `Close`, and `Viewport`.
- Only use `Action` for a safe secondary action. If the user must answer, render an `AlertDialog` styled as a toast instead.
- Use `type="foreground"` for user-triggered toasts that should announce immediately, and `background` for passive updates.
- Always supply `altText` for `Toast.Action`.
- Keep the viewport discoverable if you customize the hotkey.

## Positioning And Portal Rules

- Use `Portal` to move overlay content out of clipping or stacking contexts.
- Use a custom `container` when overlays must stay inside a particular subtree.
- Own z-index yourself in CSS.
- Favor exposed placement props, collision handling, and CSS variables over manual `getBoundingClientRect()` positioning code.
