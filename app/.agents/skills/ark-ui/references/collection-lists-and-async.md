# Collection Lists and Async Data

## Contents

- `useAsyncList`
- `createListCollection`

## `useAsyncList`

Use `useAsyncList` to manage asynchronous collection data, including loading state, errors, filtering, sorting, pagination, and cancellation.

```tsx
import { useAsyncList } from '@ark-ui/react/collection'

const list = useAsyncList<User>({
  async load({ signal }) {
    const response = await fetch('/api/users', { signal })
    const users = await response.json()
    return { items: users }
  },
})

console.log(list.items) // User[]
console.log(list.loading) // boolean
console.log(list.error) // Error | null
```

### Reload Data

Use `reload()` to re-run the loader and read `loading` to keep controls honest.

```tsx
import { useAsyncList } from '@ark-ui/react/collection'
import { LoaderIcon } from 'lucide-react'

export const Reload = () => {
  const list = useAsyncList<Quote>({
    autoReload: true,
    async load() {
      const response = await fetch(`https://dummyjson.com/quotes?limit=4`)
      if (!response.ok) throw new Error('Failed to fetch quotes')
      const data = await response.json()
      return { items: data.quotes }
    },
  })

  return (
    <div>
      <button onClick={() => list.reload()} disabled={list.loading}>
        {list.loading ? <LoaderIcon /> : 'Reload Quotes'}
      </button>
      {list.items.map((quote) => (
        <div key={quote.id}>"{quote.quote}" — {quote.author}</div>
      ))}
    </div>
  )
}
```

## `createListCollection`

Use a list collection for flat item arrays that need lookup, traversal, or item metadata helpers.

```ts
import { createListCollection } from '@ark-ui/react/collection'

const collection = createListCollection({
  items: [
    { label: 'Apple', value: 'apple' },
    { label: 'Banana', value: 'banana' },
  ],
})

console.log(collection.items)
// [{ label: 'Apple', value: 'apple' }, { label: 'Banana', value: 'banana' }]
```

### Convert Values to Items

Use `find` or `findMany` to map a value back to its item.

```ts
const item = collection.find('banana')

console.log(item) // { label: 'Banana', value: 'banana' }

const items = collection.findMany(['apple', 'banana'])

console.log(items)
// [{ label: 'Apple', value: 'apple' }, { label: 'Banana', value: 'banana' }]
```

### Traverse Values

Use `getNextValue` or `getPreviousValue` to move through the collection.

```ts
const nextValue = collection.getNextValue('apple')

console.log(nextValue) // banana

const previousValue = collection.getPreviousValue('banana')

console.log(previousValue) // apple
```

Use `firstValue` or `lastValue` when you only need the bounds.

```ts
console.log(collection.firstValue) // apple
console.log(collection.lastValue) // banana
```

### Check Existence

Use `has` to check whether a value exists.

```ts
const hasValue = collection.has('apple')

console.log(hasValue) // true
```

### Map Custom Objects

Provide `itemToString` and `itemToValue` when items do not expose `label` and `value`.

```ts
import { createListCollection } from '@ark-ui/react/collection'

const collection = createListCollection({
  items: [
    { id: 1, name: 'apple' },
    { id: 2, name: 'banana' },
    { id: 3, name: 'cherry' },
  ],
  itemToString: (item) => item.name,
  itemToValue: (item) => item.id,
})
```

Provide `isItemDisabled` when disabled state is derived instead of stored on `disabled`.

```ts
import { createListCollection } from '@ark-ui/react/collection'

const collection = createListCollection({
  items: [
    { id: 1, name: 'apple' },
    { id: 2, name: 'banana' },
    { id: 3, name: 'cherry' },
  ],
  isItemDisabled: (item) => item.id === 2,
})
```

### Reorder Items

Use `reorder` to move an item from one index to another.

```ts
const fromIndex = 1 // Banana
const toIndex = 0 // Apple

collection.reorder(fromIndex, toIndex)

console.log(collection.items)
// [{ label: 'Banana', value: 'banana' }, { label: 'Apple', value: 'apple' }]
```
