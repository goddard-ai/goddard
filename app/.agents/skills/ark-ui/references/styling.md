# Styling

## Overview

Ark UI is a headless component library that works with any styling solution. Keep functional behavior from Ark and own presentation in your CSS or styling system.

## Data Attributes

Target `data-scope` and `data-part` to style specific component parts. Interactive components also expose state `data-*` attributes.

```html
<div data-scope="accordion" data-part="item" data-state="open"></div>
```

## Styling with CSS

Target the data attributes assigned to each part:

```css
[data-scope='accordion'][data-part='item'] {
  border-bottom: 1px solid #e5e5e5;
}

[data-scope='accordion'][data-part='item'][data-state='open'] {
  background-color: #f5f5f5;
}
```

## Styling with Panda CSS

Use `defineSlotRecipe` when the repo already styles Ark UI with Panda CSS.

```ts
import { accordionAnatomy } from '@ark-ui/react/anatomy'
import { defineSlotRecipe } from '@pandacss/dev'

export const accordionStyles = defineSlotRecipe({
  className: 'accordion',
  slots: accordionAnatomy.keys(),
  base: {
    item: {
      borderBottom: '1px solid #e5e5e5',
      _open: {
        backgroundColor: 'gray.100',
      },
    },
  },
})
```

## Styling with Tailwind CSS

Apply classes directly to parts with `class` or `className`.

```jsx
<Accordion.Item className="border-b border-gray-300 data-[state=open]:bg-gray-100">
  {/* … */}
</Accordion.Item>
```
