# @tanstack/react-virtual setup

## Install

```sh
npm install @tanstack/react-virtual
# or: pnpm add @tanstack/react-virtual
# or: yarn add @tanstack/react-virtual
```

Use the React adapter for React apps:

```tsx
import { useVirtualizer, useWindowVirtualizer } from '@tanstack/react-virtual'
```

## Basic element-scroller list

```tsx
const parentRef = React.useRef<HTMLDivElement>(null)

const rowVirtualizer = useVirtualizer({
  count: rows.length,
  getScrollElement: () => parentRef.current,
  estimateSize: () => 40,
  getItemKey: (index) => rows[index].id,
})

return (
  <div ref={parentRef} style={{ height: 400, overflow: 'auto' }}>
    <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}>
      {rowVirtualizer.getVirtualItems().map((item) => (
        <div
          key={item.key}
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            width: '100%',
            height: item.size,
            transform: `translateY(${item.start}px)`,
          }}
        >
          {rows[item.index].name}
        </div>
      ))}
    </div>
  </div>
)
```

## Dynamic-size rows

Add `ref={virtualizer.measureElement}` to the rendered item and include `data-index={virtualItem.index}`. Estimate a realistic upper-ish size; poor estimates cause initial positioning and scroll correction issues. See [layouts-and-troubleshooting.md](./layouts-and-troubleshooting.md) for measurement details.

```tsx
<div
  key={item.key}
  data-index={item.index}
  ref={rowVirtualizer.measureElement}
  style={{ transform: `translateY(${item.start}px)` }}
>
  ...
</div>
```

## Window scroller

Use `useWindowVirtualizer` when the browser window owns scrolling. Position virtual rows relative to the document and account for any list offset from the top of the page. See `examples/window/`.

## React 19 / tests / performance

The React adapter defaults `useFlushSync` to `true`. Set `useFlushSync: false` when React 19 warns about `flushSync`, tests do not need synchronous DOM updates, or rapid scrolling performance is more important than perfectly synchronous visual updates.

For the complete curated option/method reference, read [options-and-api.md](./options-and-api.md).
