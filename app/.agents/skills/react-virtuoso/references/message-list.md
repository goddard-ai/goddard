# Message List

Use this reference only when the code touches `@virtuoso.dev/message-list`.

## Package And Licensing

- Import `VirtuosoMessageList`, `VirtuosoMessageListLicense`, and related types from `@virtuoso.dev/message-list`, not `react-virtuoso`.
- Treat the package as commercially licensed.
- Wrap every rendered message list with `VirtuosoMessageListLicense`.
- For evaluation and non-production development, an empty `licenseKey` enables a 30-day trial but still logs reminders.
- In production, a missing or invalid key renders an error state.
- Do not render the license provider only on the server. The license context must be available to the client-rendered message list.

## Rendering Model

- Prefer `VirtuosoMessageList` over plain `Virtuoso` for conversation UIs with append, prepend, and auto-scroll behavior.
- Keep message identity stable with `computeItemKey`.
- Model updates through the message-list `data` object rather than only swapping arrays.
- Use a ref to `VirtuosoMessageListMethods` when the code needs imperative control.

## Scroll Modifiers

- Use `scrollModifier.type = 'item-location'` for initial placement such as starting at the last item aligned to the end.
- Use `scrollModifier.type = 'auto-scroll-to-bottom'` when appending messages should scroll conditionally based on current position.
- Use `scrollModifier.type = 'items-change'` when an existing message grows in height and the list should continue following it smoothly.

## Testing

- Use `VirtuosoMessageListTestingContext` when tests need to bypass ResizeObserver-based measurements.
