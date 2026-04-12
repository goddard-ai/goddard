# Animation and Presence

Use this reference when motion and mount timing are the real problem, not the part structure itself.

## CSS State Animation

Ark parts expose stateful `data-*` attributes, so prefer animating on `[data-state]` before introducing extra state.

```css
[data-scope='tooltip'][data-part='content'][data-state='open'] {
  animation: fadeIn 300ms ease-out;
}

[data-scope='tooltip'][data-part='content'][data-state='closed'] {
  animation: fadeOut 300ms ease-in;
}
```

Use this pattern for popups, disclosure content, and any part that already exposes `open` and `closed` states.

## `Presence`

Use `Presence` for arbitrary React elements that need enter and exit animation independent of a larger Ark component.

```tsx
import { Presence } from '@ark-ui/react/presence'

<Presence present={present}>Content</Presence>
```

Keep these rules:

- Use `present` as the source of truth for whether the element should be visible.
- Use `lazyMount` when the child should not mount until first shown.
- Use `unmountOnExit` when the DOM should disappear after the exit transition finishes.
- Keep `Presence` around arbitrary regions; let components such as `Dialog`, `Popover`, or `Tooltip` own their own semantics.

## `lazyMount` and `unmountOnExit`

Many disclosure and overlay roots support `lazyMount` and `unmountOnExit`. Use them when content is expensive, lazy-loaded, or should leave the DOM while closed.

Common cases:

- `Accordion` and `Collapsible` content that should mount on first open.
- popup content such as `Dialog`, `Popover`, `Hover Card`, `Menu`, `Select`, `Combobox`, `Color Picker`, or `Date Picker`.
- `Floating Panel` or other transient content that should fully leave the tree when hidden.

## JS Animation Libraries

When CSS is not enough, keep Ark in charge of state and wire your animation library around it.

- Drive animation from Ark state such as `open`, `present`, or context state.
- Do not replace Ark focus, dismissal, or accessibility logic with animation-library state.
- Keep exit animations compatible with delayed unmounting by pairing them with `present`, `lazyMount`, or `unmountOnExit`.

## Validation

- Confirm the component stays mounted long enough for exit animation to complete.
- Confirm focus does not jump unexpectedly when animated overlay content opens or closes.
- Confirm closed-state styles do not block pointer events or layout after unmount rules apply.
