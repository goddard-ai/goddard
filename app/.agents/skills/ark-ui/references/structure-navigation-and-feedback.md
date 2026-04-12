# Structure, Navigation, and Feedback

Use this reference for React Ark components that structure page regions, progress, or feedback:

- `Accordion`
- `Collapsible`
- `Tabs`
- `Steps`
- `Pagination`
- `Carousel`
- `Scroll Area`
- `Splitter`
- `Marquee`
- `Avatar`
- `Clipboard`
- `Progress - Circular`
- `Progress - Linear`
- `QR Code`

## Disclosure and In-Place Navigation

- `Accordion`: grouped disclosure items.
- `Collapsible`: one disclosure region.
- `Tabs`: switch panels without navigation away from the current page.
- `Steps`: express ordered progress through a workflow.
- `Pagination`: move through paged data or content.

Keep trigger, content, indicator, and label parts intact. Use `lazyMount` and `unmountOnExit` only when hidden content should truly leave the DOM.

## Viewport and Layout Primitives

- `Carousel`: slide-based content navigation.
- `Scroll Area`: custom scrollbars and viewport wrappers.
- `Splitter`: resizable panel layouts.
- `Marquee`: continuous scrolling content.

Treat these as layout primitives with interaction logic, not as purely visual wrappers. Preserve required viewport and trigger parts and any CSS variables they expose.

## Small Feedback and Display Primitives

- `Avatar`: image plus fallback content.
- `Clipboard`: copy behavior and copied-state feedback.
- `Progress - Circular` and `Progress - Linear`: progress display.
- `QR Code`: generated QR display and optional download trigger.

Use these components when the behavior matters:

- `Clipboard` instead of ad hoc copy buttons and timeout state.
- `QR Code` instead of manual canvas or SVG generation when Ark already owns the feature.
- Ark progress primitives instead of plain div bars when semantics or parts matter.

## Validation

- Confirm viewport or panel layout still works at different sizes.
- Confirm disclosure content maintains keyboard and focus behavior.
- Confirm feedback primitives expose the expected value, copied, or loaded state after styling changes.
