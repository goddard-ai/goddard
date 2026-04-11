# State Management

## Choose the Access Pattern

| Approach | Use it when |
| --- | --- |
| `Component.Context` | Read component state inline via render props |
| `use*Context` hooks | Build custom child components that need state or methods |
| `useComponent` + `RootProvider` | Control the component from outside its subtree |

## `Component.Context`

Use `Component.Context` to read state inline via render props.

```tsx
<Avatar.Root>
  <Avatar.Context>
    {(avatar) => (
      <Avatar.Fallback>{avatar.loaded ? 'Loaded' : 'Loading...'}</Avatar.Fallback>
    )}
  </Avatar.Context>
</Avatar.Root>
```

## `use*Context` Hooks

Every component exports a `use*Context` hook, such as `useDialogContext` or `useMenuContext`. Call it from child components when you need state or methods outside an inline render prop.

```tsx
import { Dialog, useDialogContext } from '@ark-ui/react/dialog'

function CustomCloseButton() {
  const dialog = useDialogContext()
  return <button onClick={() => dialog.setOpen(false)}>Close ({dialog.open ? 'open' : 'closed'})</button>
}
```

## `useComponent` with `RootProvider`

Use a `useComponent` hook with `RootProvider` when control must live outside the component tree.

```tsx
import { Accordion, useAccordion } from '@ark-ui/react/accordion'

const Demo = () => {
  const accordion = useAccordion()

  return (
    <Accordion.RootProvider value={accordion}>
      {/* ... */}
    </Accordion.RootProvider>
  )
}
```

## Decision Rule

- Read state inside the tree with context first.
- Lift control outward only when another part of the app must own it.
