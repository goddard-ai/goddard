# Layout, Navigation, And Utilities

Use this file for disclosure, in-place navigation, presentation helpers, or Radix utility primitives.

## Disclosure And Navigation

### `Accordion`

- Use for stacked disclosure items where one or many sections may open.
- Preserve `Root`, `Item`, `Header`, `Trigger`, and `Content`.
- Use `type="single"` or `type="multiple"` to match the interaction instead of recreating selection logic yourself.
- Use `collapsible` when a single-open accordion should also allow closing the active item.

### `Collapsible`

- Use for a single show or hide region.
- Prefer it over `Accordion` when there is only one disclosure target.

### `Tabs`

- Use for peer panels that swap in place.
- Preserve `Root`, `List`, `Trigger`, and `Content`.
- Keep each `Trigger` and `Content` keyed by the same `value`.
- Treat tabs as navigation between related views, not as a dumping ground for unrelated sections.

### `NavigationMenu`

- Use for higher-level navigation with richer dropdown content, indicators, and a viewport.
- Expect structures built from `Root`, `List`, `Item`, `Trigger`, `Content`, `Link`, `Indicator`, and `Viewport`.
- Keep it navigation-oriented. For action menus, use `DropdownMenu` or `Menubar` instead.

### `Toolbar`

- Use for grouped controls that act on the current document or view.
- Combine with `ToggleGroup`, `Separator`, and icon buttons when building editor-like toolbars.

## Presentation And Scrolling

### `AspectRatio`

- Use to preserve a predictable box ratio for media or cards.

### `Avatar`

- Use for profile or entity images with a fallback state.

### `ScrollArea`

- Use when native scrolling behavior is correct but the visual scroll container needs custom chrome.
- Keep native scroll semantics intact and avoid replacing it with entirely custom drag logic.

### `Separator`

- Use as a visual or semantic divider between adjacent regions or controls.

## Accessibility Utilities

### `Portal`

- Use to render any subtree outside the normal DOM branch.
- Default container is `document.body`; pass `container` when a different mount point is needed.

### `VisuallyHidden`

- Use to keep content available to assistive technology while removing it from sighted layout.
- Prefer it over ad hoc off-screen CSS snippets when the goal is explicitly accessible hidden text.

### `AccessibleIcon`

- Wrap icons that carry meaning and give them an explicit label.
- Use it when the same glyph could mean different actions such as close, delete, or clear.

### `Direction.Provider`

- Wrap the app or a subtree when primitives should inherit RTL behavior globally.
- Prefer this over threading `dir` manually through every affected primitive when the direction is app-wide.

### `Slot`

- Use to implement your own `asChild` API or prop-merging leaf components.
