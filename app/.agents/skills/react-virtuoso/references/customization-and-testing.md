# Customization And Testing

Use this reference when replacing wrappers, integrating a UI kit, or writing tests around a virtualized view.

## Custom Components

- Pass overrides through the `components` prop.
- Ensure any custom `List`, `Scroller`, `Table`, or similar wrapper accepts and forwards the provided `ref` to the real DOM element.
- Keep component override objects outside render so React does not recreate component types on every render.
- Pass changing runtime state through Virtuoso's `context` prop instead of closing over it in freshly declared components.
- Use standard DOM props on the root element for styling and events like `onScroll`.

## Integration Patterns

- Custom scroller libraries usually fit best through `components.Scroller`.
- MUI list integrations typically map Virtuoso `List`, `Item`, and `Group` slots to MUI equivalents while forwarding refs.
- MUI table integrations typically map `Scroller`, `Table`, `TableHead`, `TableBody`, and `TableRow` through `components`, while keeping `border-collapse: separate` on the table.
- For sticky table headers, keep header cells non-transparent.

## Testing

- Expect plain JSDOM tests to render poorly or not at all without measurement data.
- Use `VirtuosoMockContext` to provide fixed measurement values such as viewport height and item height in tests.
- When testing `VirtuosoMessageList`, use the message-list-specific testing context noted in [message-list.md](./message-list.md) if the component depends on measured layout.
