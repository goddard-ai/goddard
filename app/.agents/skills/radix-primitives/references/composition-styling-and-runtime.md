# Composition, Styling, And Runtime

Use this file when the task is about wrappers, custom elements, styling, animation, SSR, or RTL behavior.

## `asChild` Rules

- Use `asChild` when a Radix part should attach its behavior to your own button, link, or design-system component instead of rendering its default DOM element.
- Spread every incoming prop onto the underlying DOM node of your custom component. Radix injects event handlers, ARIA props, and data attributes this way.
- Forward refs with `React.forwardRef` on leaf components. Many primitives need a ref for measurement, focus restoration, or positioning.
- Keep the rendered element accessible. If a trigger needs to be focusable and keyboard-activatable, do not switch it to a non-interactive `div`.
- Compose multiple primitives by nesting `asChild` layers when a single element needs multiple behaviors.

## `Slot` And Your Own APIs

- Use `Slot.Root` to build your own `asChild` prop on top of ordinary components.
- Use `Slottable` when your custom component has multiple children and only one subtree should receive the merged props.
- Expect child event handlers to win when `Slot` merges handlers. If parent logic depends on `event.defaultPrevented`, check handler order carefully.

## Styling Model

- Supply both functional and visual CSS yourself. Radix primitives are unstyled.
- Pass `className` directly to any part you want to target.
- Style stateful parts through `data-*` attributes instead of parallel local state when possible.
- Use `data-state` for open or closed, checked or unchecked, active or inactive states depending on the primitive.
- Use primitive-specific attributes such as `data-disabled`, `data-placeholder`, `data-side`, `data-align`, `data-valid`, and `data-invalid` where available.

## CSS Variables Worth Reusing

- Use trigger-width and available-height variables on `Popover`, `Tooltip`, and `Select` content to size overlays without manual measurement.
- Use transform-origin variables on `Popover` and `Tooltip` content to make animations originate from the computed placement.
- Use swipe-position variables plus `data-swipe` on `Toast` for gesture-driven animations.
- Prefer the CSS variables exposed by primitives like `Accordion` or `Collapsible` before manually measuring animated heights.

## Animation Rules

- CSS keyframes can animate both enter and exit. Radix suspends unmount during CSS exit animations.
- When using JavaScript animation libraries, rely on `forceMount` where supported so the animation library can own mount and unmount timing.
- Reuse collision-aware attributes such as `data-side` and `data-align` instead of hand-rolled placement state.

## Controlled And Uncontrolled State

- Most stateful primitives can run uncontrolled first. Use `defaultOpen`, `defaultValue`, `defaultChecked`, or similar defaults when the state only matters locally.
- Switch to controlled props such as `open`, `value`, or `checked` plus change handlers when URL state, global stores, or sibling coordination must own the value.
- Do not mirror internal state into local React state unless the app truly needs that extra ownership.

## Portals, Layering, And Focus

- Use `Portal` when overlay content should render under `document.body` or another container.
- Pass a custom `container` to `Portal` when the overlay must live under a specific subtree.
- Manage overlay z-index in your own CSS. Radix does not manage stacking order for you.
- Preserve the focus-management and dismissal hooks that a primitive already owns. Avoid duplicating close-on-escape or outside-click logic in app code unless you are intentionally customizing it.

## Accessibility And Labels

- Keep visible labels and accessible names explicit, even when Radix wires the ARIA relationship for you.
- Use `Label` for visual plus accessible labels on supported controls.
- Give icon-only controls an accessible name via text, `aria-label`, or `AccessibleIcon`.
- For dialog-like primitives, include a meaningful title and usually a description so the context announced on open is complete.

## SSR And RTL

- Radix primitives can render on the server.
- In React versions earlier than 18, some ids used for ARIA relationships are only finalized during hydration. Prefer React 18 if server-rendered ids must be available immediately.
- Use `Direction.Provider` or per-component `dir` props when localized layouts need RTL behavior.
- Preserve repo conventions if a codebase already passes `dir` explicitly instead of relying on the provider.
