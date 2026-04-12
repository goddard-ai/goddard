# List Selection

Use `useListSelection` when you need Ark-style single or multiple selection behavior on custom React markup instead of a built-in widget such as `Select`, `Listbox`, or `Tree View`.

## Core Shape

```tsx
import { createListCollection, useListSelection } from '@ark-ui/react/collection'

const collection = createListCollection({
  items: [
    { label: 'Apple', value: 'apple' },
    { label: 'Banana', value: 'banana' },
    { label: 'Cherry', value: 'cherry' },
  ],
})

const selection = useListSelection({
  collection,
  selectionMode: 'single',
  deselectable: true,
})
```

## When to Use It

- Build custom list UIs with checkboxes, pills, cards, or rows.
- Share selection state across multiple renderers for the same collection.
- Add keyboard or pointer selection behavior without adopting a full `Listbox`, `Select`, or `Tree View`.

## Key Options

- `selectionMode`: use `'single'`, `'multiple'`, or `'none'`.
- `deselectable`: disable deselection when one item must always remain selected.
- `collection`: keep one stable list collection and let selection point at its values.

## Useful Methods

- `isSelected(value)`: read selection state for one item.
- `select(value)`: select one value.
- `deselect(value)`: clear one value.
- `toggle(value)`: toggle one value.
- `replace(value | null)`: replace the active selection.
- `clear()`: remove all selection.
- `setSelectedValues(values)`: sync from app state when needed.

## Decision Rules

- Prefer `Select` or `Combobox` when you need popup, trigger, and form semantics.
- Prefer `Listbox` when the collection itself should own keyboard navigation and aria roles.
- Prefer `useListSelection` when the surrounding markup is custom and you only need selection logic.
- In multi-select flows, keep values stable and derived from the collection rather than array indexes.

## Validation

- Confirm selected values match the rendered item mapping.
- Confirm single-select versus multi-select behavior matches product expectations.
- Confirm deselection behavior is intentional.
