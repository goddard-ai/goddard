# Patterns, Recipes, and Responsive Design

Use this reference for layout patterns, atomic and config recipes, slot recipes, responsive behavior, and recipe tracking rules.

## Patterns

Patterns are layout primitives that can be used to create robust and responsive layouts with ease. Panda comes with predefined patterns like stack, hstack, vstack, wrap, etc. These patterns can be used as functions or JSX elements.

Think of patterns as a set of predefined styles to reduce repetition and improve readability. You can override the
properties as needed, just like in the `css` function.

## Creating Patterns

To learn how to create patterns, check out the [customization](/docs/customization/patterns) section.

## Predefined Patterns

### Box

The Box pattern does not contain any additional styles. With its function form it's the equivalent of the `css`
function. It can be useful with its JSX form and is the equivalent of a `styled.div` component, serving mostly to get
style props available in JSX.

```tsx
import { Box } from '../styled-system/jsx'

function App() {
  return (
    <Box color="blue.300">
      <div>Cool !</div>
    </Box>
  )
}
```

### Container

The Container pattern is used to create a container with a max-width and center the content.

By default, the container sets the following properties:

- `maxWidth: 8xl`
- `marginX: auto`
- `position: relative`
- `paddingX: { base: 4, md: 6, lg: 8 }`

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { container } from '../styled-system/patterns'

function App() {
  return (
    <div className={container()}>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Container } from '../styled-system/jsx'

function App() {
  return (
    <Container>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </Container>
  )
}
```

  </Tab>
</Tabs>

### Stack

The Stack pattern is a layout primitive that can be used to create a vertical or horizontal stack of elements.

The `stack` function accepts the following properties:

- `direction`: An alias for the css `flex-direction` property. Default is `column`.
- `gap`: The gap between the elements in the stack.
- `align`: An alias for the css `align-items` property.
- `justify`: An alias for the css `justify-content` property.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { stack } from '../styled-system/patterns'

function App() {
  return (
    <div className={stack({ gap: '6', padding: '4' })}>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Stack } from '../styled-system/jsx'

function App() {
  return (
    <Stack gap="6" padding="4">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </Stack>
  )
}
```

  </Tab>
</Tabs>

#### HStack

The HStack pattern is a wrapper around the `stack` pattern that sets the `direction` property to `horizontal`, and
centers the elements vertically.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { hstack } from '../styled-system/patterns'

function App() {
  return (
    <div className={hstack({ gap: '6' })}>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { HStack } from '../styled-system/jsx'

function App() {
  return (
    <HStack gap="6">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </HStack>
  )
}
```

  </Tab>
</Tabs>

#### VStack

The VStack pattern is a wrapper around the `stack` pattern that sets the `direction` property to `vertical`, and centers
the elements horizontally.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { vstack } from '../styled-system/patterns'

function App() {
  return (
    <div className={vstack({ gap: '6' })}>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { VStack } from '../styled-system/jsx'

function App() {
  return (
    <VStack gap="6">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </VStack>
  )
}
```

  </Tab>
</Tabs>

### Wrap

The Wrap pattern is used to add space between elements and wraps automatically if there isn't enough space.

The `wrap` function accepts the following properties:

- `gap`: The gap between the elements in the stack.
- `columnGap`: The gap between the elements in the stack horizontally.
- `rowGap`: The gap between the elements in the stack vertically.
- `align`: An alias for the css `align-items` property.
- `justify`: An alias for the css `justify-content` property.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { wrap } from '../styled-system/patterns'

function App() {
  return (
    <div className={wrap({ gap: '6' })}>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Wrap } from '../styled-system/jsx'

function App() {
  return (
    <Wrap gap="6">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </Wrap>
  )
}
```

  </Tab>
</Tabs>

### Aspect Ratio

The Aspect Ratio pattern is used to create a container with a fixed aspect ratio. It is used when displaying images,
maps, videos and other media.

> **Note:** In most cases, we recommend using the `aspectRatio` property instead of the pattern.

The `aspectRatio` function accepts the following properties:

- `ratio`: The aspect ratio of the container. Can be a number or a string.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { aspectRatio } from '../styled-system/patterns'

function App() {
  return (
    <div className={aspectRatio({ ratio: 16 / 9 })}>
      <iframe src="https://www.google.com/maps/embed?pb=!1m18!1m1" title="Google map" frameBorder="0" />
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { AspectRatio } from '../styled-system/jsx'

function App() {
  return (
    <AspectRatio ratio={16 / 9}>
      <iframe src="https://www.google.com/maps/embed?pb=!1m18!1m1" title="Google map" frameBorder="0" />
    </AspectRatio>
  )
}
```

  </Tab>
</Tabs>

### Flex

The Flex pattern is used to create a flex container and provides some shortcuts for the `flex` property.

The `flex` function accepts the following properties:

- `direction`: The flex direction of the container. Can be `row`, `column`, `row-reverse` or `column-reverse`.
- `wrap`: Whether to wrap the flex items. The value is a boolean.
- `align`: An alias for the css `align-items` property.
- `justify`: An alias for the css `justify-content` property.
- `basis`: An alias for the css `flex-basis` property.
- `grow`: An alias for the css `flex-grow` property.
- `shrink`: An alias for the css `flex-shrink` property.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { flex } from '../styled-system/patterns'

function App() {
  return (
    <div className={flex({ direction: 'row', align: 'center' })}>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Flex } from '../styled-system/jsx'

function App() {
  return (
    <Flex direction="row" align="center">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </Flex>
  )
}
```

  </Tab>
</Tabs>

### Center

The Center pattern is used to center the content of a container.

The `center` function accepts the following properties:

- `inline`: Whether to use `inline-flex` or `flex` for the container. The value is a boolean.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { center } from '../styled-system/patterns'

function App() {
  return (
    <div className={center({ bg: 'red.200' })}>
      <Icon />
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Center } from '../styled-system/jsx'

function App() {
  return (
    <Center bg="red.200">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </Center>
  )
}
```

  </Tab>
</Tabs>

### LinkOverlay

The link overlay pattern is used to expand a link's clickable area to its nearest parent with `position: relative`.

> We recommend using this pattern when the relative parent contains at most one clickable link.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { css } from '../styled-system/css'
import { linkOverlay } from '../styled-system/patterns'

function App() {
  return (
    <div className={css({ pos: 'relative' })}>
      <img src="https://via.placeholder.com/150" alt="placeholder" />
      <a href="#" className={linkOverlay()}>
        View more
      </a>
    </div>
  )
}
```

</Tab>

<Tab>

```tsx
import { Box, LinkOverlay } from '../styled-system/jsx'

function App() {
  return (
    <Box pos="relative">
      <img src="https://via.placeholder.com/150" alt="placeholder" />
      <LinkOverlay href="#">View more</LinkOverlay>
    </Box>
  )
}
```

</Tab>
</Tabs>

### Float

The Float pattern is used to anchor an element to the top, bottom, left or right of the container.

> It requires a parent element with `position: relative` styles.

The `float` function accepts the following properties:

- `placement`: The placement of the element. Can be `top-start`, `top`, `top-end`, `bottom-start`, `bottom`,
  `bottom-end`, `left-start`, `left`, `left-end`, `right-start`, `right` or `right-end`.
- `offset`: The offset of the element from the edge of the container. Can be a number or a string.
- `offsetX`: Same as `offset`, but only for the horizontal axis.
- `offsetY`: Same as `offset`, but only for the vertical axis.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { css } from '../styled-system/css'
import { float } from '../styled-system/patterns'

function App() {
  return (
    <div className={css({ position: 'relative' })}>
      <div className={float({ placement: 'top-start' })}>3</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { css } from '../styled-system/css'
import { Float } from '../styled-system/jsx'

function App() {
  return (
    <div className={css({ position: 'relative' })}>
      <Float placement="top-start">3</Float>
    </div>
  )
}
```

  </Tab>
</Tabs>

### Grid

The Grid pattern is used to create a grid layout.

The `grid` function accepts the following properties:

- `columns`: The number of columns in the grid.
- `gap`: The gap between the elements in the stack.
- `columnGap`: The gap between the elements in the stack horizontally.
- `rowGap`: The gap between the elements in the stack vertically.
- `minChildWidth`: The minimum width of the child elements before wrapping (must not be used with `columns`).

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { grid } from '../styled-system/patterns'

function App() {
  return (
    <div className={grid({ columns: 3, gap: '6' })}>
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Grid } from '../styled-system/jsx'

function App() {
  return (
    <Grid columns={3} gap="6">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
    </Grid>
  )
}
```

  </Tab>
</Tabs>

#### Grid Item

The Grid Item pattern is used to style the children of a grid container.

The `gridItem` function accepts the following properties:

- `colSpan`: The number of columns the item spans.
- `rowSpan`: The number of rows the item spans.
- `rowStart`: The row the item starts at.
- `rowEnd`: The row the item ends at.
- `colStart`: The column the item starts at.
- `colEnd`: The column the item ends at.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { grid, gridItem } from '../styled-system/patterns'

function App() {
  return (
    <div className={grid({ columns: 3, gap: '6' })}>
      <div className={gridItem({ colSpan: 2 })}>First</div>
      <div>Second</div>
      <div>Third</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Grid, GridItem } from '../styled-system/jsx'

function App() {
  return (
    <Grid columns={3} gap="6">
      <GridItem colSpan={2}>First</GridItem>
      <GridItem>Second</GridItem>
      <GridItem>Third</GridItem>
    </Grid>
  )
}
```

  </Tab>
</Tabs>

### Divider

The Divider pattern is used to create a horizontal or vertical divider.

The `divider` function accepts the following properties:

- `orientation`: The orientation of the divider. Can be `horizontal` or `vertical`.
- `thickness`: The thickness of the divider. Can be a sizing token, or arbitrary value.
- `color`: The color of the divider. Can be a color token, or arbitrary value.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { divider, stack } from '../styled-system/patterns'

function App() {
  return (
    <div className={stack()}>
      <button>First</button>
      <div className={divider({ orientation: 'horizontal' })} />
      <button>Second</button>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { Divider, Stack } from '../styled-system/jsx'

function App() {
  return (
    <Stack>
      <button>First</button>
      <Divider orientation="horizontal" />
      <button>Second</button>
    </Stack>
  )
}
```

  </Tab>
</Tabs>

### Circle

The Circle pattern is used to create a circle.

The `circle` function accepts the following properties:

- `size`: The size of the circle. Can be a sizing token, or arbitrary value.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { circle } from '../styled-system/patterns'

function App() {
  return <div className={circle({ size: '12', bg: 'red.300' })} />
}
```

  </Tab>
  <Tab>

```tsx
import { Circle } from '../styled-system/jsx'

function App() {
  return <Circle size="12" bg="red.300" />
}
```

  </Tab>
</Tabs>

### Square

The Square pattern is used to create a square with equal width and height.

The `square` function accepts the following properties:

- `size`: The size of the square. Can be a sizing token, or arbitrary value.

<Tabs items={['Function', 'JSX']}>
<Tab>

```tsx
import { square } from '../styled-system/patterns'

function App() {
  return <div className={square({ size: '12', bg: 'red.300' })} />
}
```

  </Tab>
  <Tab>

```tsx
import { Square } from '../styled-system/jsx'

function App() {
  return <Square size="12" bg="red.300" />
}
```

  </Tab>
</Tabs>

### Visually Hidden

The Visually Hidden pattern is used to hide an element visually, but keep it accessible to screen readers.

```tsx
import { visuallyHidden } from '../styled-system/patterns'

export function Checkbox() {
  return (
    <label>
      <input type="checkbox" className={visuallyHidden()}>
        I'm hidden
      </input>
      <span>Checkbox</span>
    </label>
  )
}
```

### Bleed

The Bleed pattern is used to create a full width element by negating the padding applied to its parent container.

The `bleed` function accepts the following properties:

- `inline`: The amount of padding to negate on the horizontal axis. Should match the parent's padding.
- `block`: The amount of padding to negate on the vertical axis. Should match the parent's padding.

<Tabs items={['Function', 'JSX']}>
  <Tab>

```tsx
import { css } from '../styled-system/css'
import { bleed } from '../styled-system/patterns'

export function Page() {
  return (
    <div className={css({ px: '6' })}>
      <div className={bleed({ inline: '6' })}>Welcome</div>
    </div>
  )
}
```

  </Tab>
  <Tab>

```tsx
import { css } from '../styled-system/css'
import { Bleed } from '../styled-system/jsx'

export function Page() {
  return (
    <div className={css({ px: '6' })}>
      <Bleed inline="6">Welcome</div>
    </div>
  )
}
```

  </Tab>
</Tabs>

### cq (Container Query)

To make it easier to use container queries, we've added a new `cq` pattern to `@pandacss/preset-base`. It is used to
apply styles based on the width of the container.

The `cq` function accepts the following properties:

- `name`: The name of the container query, Maps to the
  [`containerName` CSS property](https://developer.mozilla.org/en-US/docs/Web/CSS/container-name).
- `type`: The type of the container query. Maps to the
  [`containerType` CSS property](https://developer.mozilla.org/en-US/docs/Web/CSS/container-type). Defaults to
  `inline-size`.

```ts
import { cq } from 'styled-system/patterns'

function Demo() {
  return (
    <nav className={cq()}>
      <div
        className={css({
          fontSize: { base: 'lg', '@/sm': 'md' },
        })}
      />
    </nav>
  )
}
```

You can also named container queries:

```tsx
// 1 - Define container conditions

export default defineConfig({
  // ...
  theme: {
    containerNames: ['sidebar', 'content'],
    containerSizes: {
      xs: '40em',
      sm: '60em',
      md: '80em'
    }
  }
})
```

```tsx
// 2 - Automatically generate container query pattern

import { cq } from 'styled-system/patterns'

function Demo() {
  return (
    <nav className={cq({ name: 'sidebar' })}>
      <div
        className={css({
          // When the sidebar container reaches the `sm` size
          // change font size to `md`
          fontSize: { base: 'lg', '@sidebar/sm': 'md' }
        })}
      />
    </nav>
  )
}
```

Read more about container queries [here](/docs/concepts/conditional-styles#container-queries).

## Usage with JSX

To use the pattern in JSX, you need to set the `jsxFramework` property in the config. When this is set, Panda will emit
files for JSX elements based on the framework.

Every pattern can be used as a JSX element and imported from the `/jsx` entrypoint. By default, the pattern name is the
function name in PascalCase. You can override both the component name (with the `jsx` config property) and the element
rendered (with the `jsxElement` config property).

Learn more about pattern customization [here](/docs/customization/patterns).

```tsx
import { VStack, Center } from '../styled-system/jsx'

function App() {
  return (
    <VStack gap="6" mt="4">
      <div>First</div>
      <div>Second</div>
      <div>Third</div>
      <Center>4</Center>
    </VStack>
  )
}
```

### Advanced JSX Tracking

We recommend that you use the pattern functions in most cases, in design systems there might be a need to compose
existing components to create new components.

To track the usage of the patterns in these cases, you'll need to add the `jsx` hint for the pattern config

```js {12} filename="button.pattern.ts"
import { definePattern } from '@pandacss/dev'

const scrollable = definePattern({
  // ...
  // Add the jsx hint to track the usage of the pattern in JSX, you can also use a regex to match multiple components
  jsx: ['Scrollable', 'PageScrollable']
})
```

Then you can create a new component that uses the `PageScrollable` component and Panda will track the usage of the
`scrollable` pattern as well.

```tsx
const PageScrollable = (props: ButtonProps) => {
  const { children, size } = props
  return (
    <Scrollable {...props} size={size}>
      {children}
    </Scrollable>
  )
}
```

---


## Recipes

Panda provides a way to write CSS-in-JS with better performance, developer experience, and composability.

Recipes are a way to create multi-variant styles with a type-safe runtime API.

A recipe consists of four properties:

- `base`: The base styles for the component
- `variants`: The different visual styles for the component
- `compoundVariants`: The different combinations of variants for the component
- `defaultVariants`: The default variant values for the component

> **Credit:** This API was inspired by [Stitches](https://stitches.dev/),
> [Vanilla Extract](https://vanilla-extract.style/), and [Class Variance Authority](https://cva.style/).

[Comparison table between the different types of recipes here: "Should I use atomic or config recipes ?"](/docs/concepts/recipes#should-i-use-atomic-or-config-recipes-)

## Atomic Recipe (or cva)

Atomic recipes are a way to create multi-variant atomic styles with a type-safe runtime API.

They are defined using the `cva` function which was inspired by [Class Variance Authority](https://cva.style/). The
`cva` function which takes an object as its argument.

> **Note:** `cva` is not the same as [Class Variance Authority](https://cva.style/). The `cva` from Panda is a
> purpose-built function for creating atomic recipes that are connected to your design tokens and utilities.

### Defining the recipe

```jsx
import { cva } from '../styled-system/css'

const button = cva({
  base: {
    display: 'flex'
  },
  variants: {
    visual: {
      solid: { bg: 'red.200', color: 'white' },
      outline: { borderWidth: '1px', borderColor: 'red.200' }
    },
    size: {
      sm: { padding: '4', fontSize: '12px' },
      lg: { padding: '8', fontSize: '24px' }
    }
  }
})
```

### Using the recipe

The returned value from the `cva` function is a function that can be used to apply the recipe to a component. Here's an
example of how to use the `button` recipe:

```jsx
import { button } from './button'

const Button = () => {
  return <button className={button({ visual: 'solid', size: 'lg' })}>Click Me</button>
}
```

When a recipe is created, Panda will extract and generate CSS for every variant and compoundVariant `css` ahead of time,
as atomic classes.

```css
@layer utilities {
  .d_flex {
    display: flex;
  }

  .bg_red_200 {
    background-color: #fed7d7;
  }

  .color_white {
    color: #fff;
  }

  .border_width_1px {
    border-width: 1px;
  }
  /* ... */
}
```

### Setting the default variants

The `defaultVariants` property is used to set the default variant values for the recipe. This is useful when you want to
apply a variant by default. Here's an example of how to use `defaultVariants`:

```jsx
import { cva } from '../styled-system/css'

const button = cva({
  base: {
    display: 'flex'
  },
  variants: {
    visual: {
      solid: { bg: 'red.200', color: 'white' },
      outline: { borderWidth: '1px', borderColor: 'red.200' }
    },
    size: {
      sm: { padding: '4', fontSize: '12px' },
      lg: { padding: '8', fontSize: '24px' }
    }
  },
  defaultVariants: {
    visual: 'solid',
    size: 'lg'
  }
})
```

### Compound Variants

Compound variants are a way to combine multiple variants together to create more complex sets of styles. They are
defined using the `compoundVariants` property , which takes an array of objects as its argument. Each object in the
array represents a set of conditions that must be met in order for the corresponding styles to be applied.

Here's an example of how to use `compoundVariants` in Panda:

```js
import { cva } from '../styled-system/css'

const button = cva({
  base: {
    padding: '8px 16px',
    borderRadius: '4px',
    fontSize: '16px',
    fontWeight: 'bold'
  },

  variants: {
    size: {
      small: {
        fontSize: '14px',
        padding: '4px 8px'
      },
      medium: {
        fontSize: '16px',
        padding: '8px 16px'
      },
      large: {
        fontSize: '18px',
        padding: '12px 24px'
      }
    },
    color: {
      primary: {
        backgroundColor: 'blue',
        color: 'white'
      },
      secondary: {
        backgroundColor: 'gray',
        color: 'black'
      }
    },
    disabled: {
      true: {
        opacity: 0.5,
        cursor: 'not-allowed'
      }
    }
  },

  // compound variants
  compoundVariants: [
    // apply when both small size and primary color are selected
    {
      size: 'small',
      color: 'primary',
      css: {
        border: '2px solid blue'
      }
    },
    // apply when both large size and secondary color are selected and the button is disabled
    {
      size: 'large',
      color: 'secondary',
      disabled: true,
      css: {
        backgroundColor: 'lightgray',
        color: 'darkgray',
        border: 'none'
      }
    },
    // apply when both small or medium size, and secondary color variants are applied
    {
      size: ['small', 'medium'],
      color: 'secondary',
      css: {
        fontWeight: 'extrabold'
      }
    }
  ]
})
```

Here's an example usage of the `button` recipe:

```jsx
import { button } from './button'

const Button = () => {
  // will apply size: small, color: primary, css: { border: '2px solid blue' }
  return <button className={button({ size: 'small', color: 'primary' })}>Click Me</button>
}
```

Overall, using compound variants allows you to create more complex sets of styles that can be applied to your components
based on multiple conditions.

By combining simple variants together in this way, you can create a wide range of visual styles without cluttering up
your code with lots of conditional logic.

### TypeScript Guide

Panda provides a `RecipeVariantProps` type utility that can be used to infer the variant properties of a recipe.

This is useful when you want to use the recipe in JSX and want to get type safety for the variants.

```tsx
import { styled } from '../styled-system/jsx'
import { cva, type RecipeVariantProps } from '../styled-system/css'

const buttonStyle = cva({
  base: {
    color: 'red',
    textAlign: 'center'
  },
  variants: {
    size: {
      small: {
        fontSize: '1rem'
      },
      large: {
        fontSize: '2rem'
      }
    }
  }
})

export type ButtonVariants = RecipeVariantProps<typeof buttonStyle> // { size?: 'small' | 'large' }

export const Button = styled('button', buttonStyle)
```

### Usage in JSX

You can create a JSX component from any existing atomic recipe by using the `styled` function from the `/jsx`
entrypoint.

The `styled` function takes the element type as its first argument, and the recipe as its second argument.

> Make sure to add the `jsxFramework` option to your `panda.config` file, and run `panda codegen` to generate the JSX
> entrypoint.

```js
import { cva } from '../styled-system/css'
import { styled } from '../styled-system/jsx'

const buttonStyle = cva({
  base: {
    color: 'red',
    textAlign: 'center'
  },
  variants: {
    size: {
      small: {
        fontSize: '1rem'
      },
      large: {
        fontSize: '2rem'
      }
    }
  }
})

const Button = styled('button', buttonStyle)
```

Then you can use the component in JSX

```jsx
<Button size="large">Click me</Button>
```

## Config Recipe

Config recipes are extracted and generated just in time, this means regardless of the number of recipes in the config,
only the recipes and variants you use will exist in the generated CSS.

The config recipe takes the following additional properties:

- `className`: The name of the recipe. Used in the generated class name
- `jsx`: An array of JSX components that use the recipe. Defaults to the uppercase version of the recipe name
- `description`: An optional description of the recipe (used in the js-doc comments)

> As of v0.9, the `name` property is removed in favor of `className`

### Defining the recipe

To define a config recipe, import the `defineRecipe` helper function

```jsx filename="button.recipe.ts"
import { defineRecipe } from '@pandacss/dev'

export const buttonRecipe = defineRecipe({
  className: 'button',
  description: 'The styles for the Button component',
  base: {
    display: 'flex'
  },
  variants: {
    visual: {
      funky: { bg: 'red.200', color: 'white' },
      edgy: { border: '1px solid {colors.red.500}' }
    },
    size: {
      sm: { padding: '4', fontSize: '12px' },
      lg: { padding: '8', fontSize: '40px' }
    },
    shape: {
      square: { borderRadius: '0' },
      circle: { borderRadius: 'full' }
    }
  },
  defaultVariants: {
    visual: 'funky',
    size: 'sm',
    shape: 'circle'
  }
})
```

### Adding recipe to config

To add the recipe to the config, you’d need to add it to the `theme.recipes` object.

```jsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'
import { buttonRecipe } from './button.recipe'

export default defineConfig({
  //...
  jsxFramework: 'react',
  theme: {
    extend: {
      recipes: {
        button: buttonRecipe
      }
    }
  }
})
```

### Generate JS code

This generates a recipes folder the specified `outdir` which is `styled-system` by default. If Panda doesn’t
automatically generate your CSS file, you can run the `panda codegen` command.

You only need to import the recipes into the component files where you need to use them.

### Using the recipe

To use the recipe, you can import the recipe from the `<outdir>/recipes` entrypoint and use it in your component. Panda
tracks the usage of the recipe and only generates CSS of the variants used in your application.

```js
import { button } from '../styled-system/recipes'

function App() {
  return (
    <div>
      <button className={button()}>Click me</button>
      <button className={button({ shape: 'circle' })}>Click me</button>
    </div>
  )
}
```

The generated css is registered under the `recipe` [cascade layer](/docs/concepts/cascade-layers.mdx) with the class
name that matches the recipe-variant name pattern `<recipe-className>--<variant-name>`.

> **Technical Notes 📝:** Only the recipe and variants used in your application are generated. Not more!

```css
@layer recipes {
  @layer base {
    .button {
      font-size: var(--font-sizes-lg);
    }
  }

  .button--visual-funky {
    background-color: var(--colors-red-200);
    color: var(--colors-white);
  }

  .button--size-lg {
    padding: var(--space-8);
    font-size: var(--font-sizes-40px);
  }
}
```

### Responsive and Conditional variants

Recipes created in the config have a **special** feature; they can be applied based on a specific breakpoints or
conditions.

Here's how to tweak the size variant of the button recipe based on breakpoints.

```jsx
import { button } from '../styled-system/recipes'

function App() {
  return (
    <div>
      <button className={button({ size: { base: 'sm', md: 'lg' } })}>Click me</button>
    </div>
  )
}
```

> In most cases, we don't recommend applying conditional variants inline. Ideally, you might want to render different
> views for your responsive breakpoints.

### TypeScript Guide

Every recipe ships a type interface for its accepted variants. You can import them from the `styled-system/recipes`
entrypoint.

For the button recipe, we can import the `ButtonVariants` type like so:

```ts
import React from 'react'
import type { ButtonVariants } from '../styled-system/recipes'

type ButtonProps = ButtonVariants & {
  children: React.ReactNode
}
```

### Usage in JSX

Layer recipes can be consumed directly in your custom JSX components. Panda will automatically track the usage of the
recipe if the component name matches the recipe name.

For example, if your recipe is called `button` and you create a `Button` component from it, Panda will automatically
track the usage of the variant properties.

```tsx
import React from 'react'
import { button, type ButtonVariants } from '../styled-system/recipes'

type ButtonProps = ButtonVariants & {
  children: React.ReactNode
}

const Button = (props: ButtonProps) => {
  const { children, size } = props
  return (
    <button {...props} className={button({ size })}>
      {children}
    </button>
  )
}

const App = () => {
  return (
    <div>
      <Button size="lg">Click me</Button>
    </div>
  )
}
```

### Advanced JSX Tracking

We recommend that you use the recipe functions in most cases, in design systems there might be a need to compose
existing components (like Button) to create new components.

To track the usage of the recipes in these cases, you'll need to add the `jsx` hint for the recipe config

```js {12} filename="button.recipe.ts"
import { defineRecipe } from '@pandacss/dev'

const button = defineRecipe({
  base: {
    color: 'red',
    fontSize: '1.5rem'
  },
  variants: {
    // ...
  },
  // Add the jsx hint to track the usage of the recipe in JSX, you can use regex to match multiple components
  jsx: ['Button', 'PageButton']
})
```

Then you can create a new component that uses the `Button` component and Panda will track the usage of the `button`
recipe as well.

```tsx
const PageButton = (props: ButtonProps) => {
  const { children, size } = props
  return (
    <Button {...props} size={size}>
      {children}
    </Button>
  )
}
```

#### Extending a preset recipe

If you're using a recipe from a preset, you can still extend it in your config.

```js
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  jsxFramework: 'react',
  theme: {
    extend: {
      recipes: {
        button: {
          className: 'something-else', // 👈 override the className
          base: {
            color: 'red', // 👈 replace some part of the recipe
            fontSize: '1.5rem' // or add new styles
          },
          variants: {
            // ... // 👈 add or extend new variants
          },
          jsx: ['Button', 'PageButton'] // 👈 extend the jsx tracking hint
        }
      }
    }
  }
})
```

Learn more about the [extend](/docs/concepts/extend.md) keyword.

## Methods and Properties

Both atomic and config recipe ships a helper methods and properties that can be used to get information about the
recipe.

- `variantKeys`: An array of the recipe variant keys
- `variantMap`: An object of the recipe variant keys and their values
- `splitVariantProps`: A function that takes an object as its argument and returns an array containing the recipe
  variant props and the rest of the props

```js
import { cva } from '../styled-system/css'

const buttonRecipe = cva({
  base: {
    color: 'red',
    fontSize: '1.5rem'
  },
  variants: {
    size: {
      sm: {
        fontSize: '1rem'
      },
      md: {
        fontSize: '2rem'
      }
    }
  }
})

buttonRecipe.variantKeys
// => ['size']

buttonRecipe.variantMap
// => { size: ['sm', 'md'] }

buttonRecipe.splitVariantProps({ size: 'sm', onClick() {} })
// => [{ size: 'sm'}, { onClick() {} }]
```

These methods and properties are useful when creating custom components or writing Storybook stories for your recipes.

Here's a Storybook example.

```tsx filename="button.stories.tsx"
import { Button, buttonRecipe } from './components/button'

export default {
  title: 'Button',
  component: Button,
  argTypes: {
    size: {
      control: {
        type: 'select',
        options: buttonRecipe.variantMap.size
      }
    }
  }
}

export const Demo = {
  render: args => <Button {...args}>Click me</Button>
}
```

## Best Practices

- Leverage css variables in the base styles as much as possible. Makes it easier to theme the component with JS
- Don't mix styles by writing complex selectors. Separate concerns and group them in logical variants
- Use the `compoundVariants` property to create more complex sets of styles

## Limitations

- Recipes created from `cva` cannot have responsive or conditional values. Only layer recipes can have responsive or
  conditional values.

- Due to static nature of Panda, it's not possible to track the usage of the recipes in all cases. Here are some of use
  cases that Panda won't be able to track the usage of the recipe variants:

  **When you change the name of the variant prop in the JSX component**

  In below example, the `size` prop is renamed to `buttonSize`

  ```tsx
  const Button = ({ buttonSize, children }) => {
    return (
      <button {...props} className={button({ size: buttonSize })}>
        {children}
      </button>
    )
  }
  ```

  **When you use the recipe in a custom component that is not named as per the recipe name, Panda won't be able to track
  the usage of the recipe variants.**

  In below example, the component name `Button` is renamed to `Random` and we are using `button` recipe.

  ```tsx
  const Random = ({ size, children }) => {
    return (
      <button {...props} className={button({ size })}>
        {children}
      </button>
    )
  }
  ```

- When using `compoundVariants` in the recipe, you're not able to use responsive values in the variants.

```tsx
const button = defineRecipe({
  base: {
    color: 'red',
    fontSize: '1.5rem'
  },
  variants: {
    size: {
      sm: {
        fontSize: '1rem'
      },
      md: {
        fontSize: '2rem'
      }
    }
  },
  // this  will disable responsive values for the variants
  compoundVariants: [
    {
      size: 'sm',
      visual: 'funky',
      css: {
        color: 'blue'
      }
    },
    {
      size: 'md',
      visual: 'funky',
      css: {
        color: 'green'
      }
    }
  ]
})
```

## Static CSS

Panda provides a way to generate `static CSS` for your recipes. This is useful when you want to generate CSS for a
recipe without using the recipe in your code or if you use dynamic styling that Panda can't keep track of.

More information about static CSS can be found [here](/docs/guides/static.md#generating-recipes).

## Should I use atomic or config recipes ?

[Config recipes](/docs/concepts/recipes#config-recipe) are generated just in time, meaning that only the recipes and
variants you use will exist in the generated CSS, regardless of the number of recipes in the config.

This contrasts with [Atomic recipes](/docs/concepts/recipes#atomic-recipe-or-cva) (cva), which generates all of the
variants regardless of what was used in your code. The reason for this difference is that all `config.recipes` are known
at the start of the panda process when we evaluate your config.

In contrast, the CVA recipes are scattered throughout your code. To get all of them and find their usage across your
code, we would need to scan your app code multiple times, which would not be ideal performance-wise.

When dealing with simple use cases, or if you need code colocation, or even avoiding dynamic styling, atomic recipes
shine by providing all style variants. Config recipes are preferred for design system components, delivering leaner CSS
with only the styles used. Choose according to your component needs.

|                                                        | Config recipe                                                                                                                                                                                                             | Atomic recipe (cva)                                                                                                                                                                                                  |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Can both use any theme tokens, utilities or conditions | ✅ yes                                                                                                                                                                                                                    | ✅ yes                                                                                                                                                                                                               |
| Are generated just in time (JIT) based on usage        | ✅ yes, only the recipe variants found in your code will be generated                                                                                                                                                     | ❌ no, all variants found in your `cva` recipes will always be generated                                                                                                                                             |
| Can be shared in a preset                              | ✅ yes, you can include it in your `preset.theme.recipes`                                                                                                                                                                 | ❌ no                                                                                                                                                                                                                |
| Can be applied responsively                            | ✅ yes, `button({ size: { base: 'sm', md: 'lg' } })`                                                                                                                                                                      | ❌ no, only the styles in the recipe can be responsive                                                                                                                                                               |
| Can be colocated in your markup code                   | ❌ no, they must be defined or imported in your `panda.config`                                                                                                                                                            | ✅ yes, you can place it anywhere in your app                                                                                                                                                                        |
| Generate atomic classes                                | ❌ no, a specific className will be generated using your `recipe.className`                                                                                                                                               | ✅ yes                                                                                                                                                                                                               |
| Can be composed/merged at runtime                      | ❌ no, a specific className will be generated using your `recipe.className`, [you need to use `cx` to add each recipe classes](https://panda-css.com/docs/concepts/merging-styles#merging-config-recipe-and-style-object) | ✅ [yes, you can use the `.raw` function (ex: `button.raw({ size: "md" })`) to get the atomic style object and merge them all in a `css(xxx, yyy, zzz)` call](/docs/concepts/merging-styles#merging-cva--css-styles) |

---


## Responsive Design

How to write mobile responsive designs in your CSS in Panda

Responsive design is a fundamental aspect of modern web development, allowing websites and applications to adapt
seamlessly to different screen sizes and devices.

Panda provides a comprehensive set of responsive utilities and features to facilitate the creation of responsive
layouts. It lets you do this through conditional styles for different breakpoints.

Let's say you want to change the font weight of a text on large screens, you can do it like this:

```jsx
<span
  className={css({
    fontWeight: 'medium',
    lg: { fontWeight: 'bold' }
  })}
>
  Text
</span>
```

> Panda uses a mobile-first breakpoint system and leverages min-width media queries `@media(min-width)` when you write
> responsive styles.

Panda provides five breakpoints by default:

```ts
const breakpoints = {
  sm: '640px',
  md: '768px',
  lg: '1024px',
  xl: '1280px',
  '2xl': '1536px'
}
```

## Overview

### Property based modifier

Panda allows you apply the responsive condition directly to a style property, resulting in a more concise syntax:

```diff
<span
  className={css({
-   fontWeight: 'medium',
-   lg: { fontWeight: 'bold' }
+   fontWeight: { base: 'medium', lg: 'bold' }
  })}
>
  Text
</span>
```

### The Array syntax

Panda also accepts arrays as values for responsive styles. Pass the corresponding value for each breakpoint in the
array. Using our previous code as an example:

```jsx
<span
  className={css({
    fontWeight: ['medium', undefined, undefined, 'bold']
  })}
>
  Text
</span>
```

> We're leaving the corresponding values of the unused breakpoints `md` and `lg` as undefined.

### Targeting a breakpoint range

By default, styles assigned to a specific breakpoint will be effective at that breakpoint and will persist as applied
styles at larger breakpoints.

If you wish to apply a utility exclusively when a particular range of breakpoints is active, Panda offers properties
that restrict the style to that specific range. To construct the property, combine the minimum and maximum breakpoints
using the "To" notation in camelCase format.

Let's say we want to apply styles between the `md` and `xl` breakpoints, we use the `mdToXl` property:

```jsx
<span
  className={css({
    fontWeight: { mdToXl: 'bold' }
  })}
>
  Text
</span>
```

> This text will only be bold in `md`, `lg` and `xl` breakpoints.

### Targeting a single breakpoint

To target a single breakpoint, you can easily achieve this by simply adding the suffix "Only" to the breakpoint name in
camelCase format.

Let's say we want to apply styles only in the `lg` breakpoint, we use the `lgOnly` property:

```jsx
<span
  className={css({
    fontWeight: { lgOnly: 'bold' }
  })}
>
  Text
</span>
```

### Customizing Breakpoints

When encountering certain scenarios, it may become necessary to establish custom breakpoints tailored to your
application's needs. It is advisable to utilize commonly used aliases such as `sm`, `md`, `lg`, and `xl` for this
purpose.

In order to define custom breakpoints, you can easily accomplish this by passing them as an object within your Panda
config.

> Note: Make sure that the CSS units of your breakpoints are consistent. Use either all pixels (`px`) or all `em`, but
> do not mix them.

```js filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  theme: {
    extend: {
      breakpoints: {
        sm: '640px',
        md: '768px',
        lg: '1024px',
        xl: '1280px',
        '2xl': '1536px'
      }
    }
  }
})
```

### Hiding elements by breakpoint

If you need to limit the visibility of an element to any breakpoint, Panda provides
[display utilities](/docs/utilities/display) to help you achieve this.

---


## Slot Recipes

Learn how to style multiple parts components with slot recipes.

When using `cva` or `defineRecipe` might be enough for simple cases, slot recipes are a better fit for more complex
cases.

A slot recipe consists of these properties:

- `slots`: An array of component parts to style
- `base`: The base styles per slot
- `variants`: The different visual styles for each slot
- `defaultVariants`: The default variant for the component
- `compoundVariants`: The compound variant combination and style overrides for each slot.

> **Credit:** This API was inspired by multipart components in
> [Chakra UI](https://chakra-ui.com/docs/styled-system/component-style) and slot variants in
> [Tailwind Variants](https://tailwind-variants.org)

[See the comparison table between atomic recipes (`cva`) and `config recipes` here.](/docs/concepts/recipes#should-i-use-atomic-or-config-recipes-)
The same comparison applies to `sva` and `slot recipes`.

## Atomic Slot Recipe (or sva)

The `sva` function is a shorthand for creating a slot recipe with atomic variants. It takes the same arguments as `cva`
but returns a slot recipe instead.

### Defining the Recipe

```jsx filename="checkbox.recipe.ts"
import { sva } from '../styled-system/css'

const checkbox = sva({
  slots: ['root', 'control', 'label'],
  base: {
    root: { display: 'flex', alignItems: 'center', gap: '2' },
    control: { borderWidth: '1px', borderRadius: 'sm' },
    label: { marginStart: '2' }
  },
  variants: {
    size: {
      sm: {
        control: { width: '8', height: '8' },
        label: { fontSize: 'sm' }
      },
      md: {
        control: { width: '10', height: '10' },
        label: { fontSize: 'md' }
      }
    }
  },
  defaultVariants: {
    size: 'sm'
  }
})
```

### Using the recipe

The returned value from `sva` is a function that can be used to apply the recipe for each component part. Here's an
example of how to use the `checkbox` recipe:

```jsx filename="Checkbox.tsx"
import { css } from '../styled-system/css'
import { checkbox } from './checkbox.recipe'

const Checkbox = () => {
  const classes = checkbox({ size: 'sm' })
  return (
    <label className={classes.root}>
      <input type="checkbox" className={css({ srOnly: true })} />
      <div className={classes.control} />
      <span className={classes.label}>Checkbox Label</span>
    </label>
  )
}
```

When a slot recipe is created, Panda will pre-generate the css of all the possible combinations of variants and compound
variants as atomic classes.

```css
@layer utilities {
  .border_width_1px {
    border-width: 1px;
  }

  .rounded_sm {
    border-radius: var(--radii-sm);
  }

  .margin_start_2 {
    margin-inline-start: var(--spacing-2);
  }

  .w_8 {
    width: var(--sizing-8);
  }

  .h_8 {
    height: var(--sizing-8);
  }

  .font_size_sm {
    font-size: var(--fontSizes-sm);
  }

  .w_10 {
    width: var(--sizing-10);
  }

  .h_10 {
    height: var(--sizing-10);
  }

  .font_size_md {
    font-size: var(--fontSizes-md);
  }
  /* ... */
}
```

### Compound Variants

Compound variants are a way to apply style overrides to a slot based on the combination of variants.

Let's say you want to apply a different border color to the checkbox control based on its `size` and the `isChecked`
variant, here's how to do it:

```jsx filename="checkbox.recipe.ts" {14-22}
import { sva } from '../styled-system/css'
const checkbox = sva({
  slots: ['root', 'control', 'label'],
  base: {...},
  variants: {
    size: {
      sm: {...},
      md: {...}
    },
    isChecked: {
      true: { control: {}, label: {} }
    }
  },
  compoundVariants: [
    {
      size: 'sm',
      isChecked: true,
      css: {
        control: { borderColor: 'green.500' }
      }
    }
  ],
  defaultVariants: {...}
})
```

### Targeting slots

You can set an optional `className` property in the `sva` config which can be used to target slots in the DOM.

> Each slot will contain a `${className}__${slotName}` class in addition to the atomic styles.

Let's say you want to apply a different border color to the button text directly from the `root` slot. Here's how you
would do it:

```tsx
import { sva } from '../styled-system/css'

const button = sva({
  className: 'btn',
  slots: ['root', 'text'],
  base: {
    root: {
      bg: 'blue.500',
      _hover: {
        // v--- 🎯 this will target the `text` slot
        '& .btn__text': {
          color: 'white'
        }
      }
    }
  }
})
```

> Note: This doesn't work when you have the `hash: true` option in your panda config. We recommend using `data-x`
> selectors to target slots.

### TypeScript Guide

Panda provides a `RecipeVariantProps` type utility that can be used to infer the variant properties of a slot recipe.

This is useful when you want to use the recipe in JSX and want to get type safety for the variants.

```tsx
import { sva, type RecipeVariantProps } from '../styled-system/css'

const checkbox = sva({...})

export type CheckboxVariants = RecipeVariantProps<typeof checkbox>
//  => { size?: 'sm' | 'md', isChecked?: boolean }
```

### Usage in JSX

Unlike the atomic recipe or `cva`, slot recipes are not meant to be used directly in the `styled` factory since it
returns an object of classes instead of a single class.

```jsx
import { css } from '../styled-system/css'
import { styled } from '../styled-system/jsx'
import { checkbox, type CheckboxVariants } from './checkbox.recipe'

// ❌ Won't work
const Checkbox = styled('label', checkbox)

// ✅ Works
const Checkbox = (props: CheckboxVariants) => {
  const classes = checkbox(props)
  return (
    <label className={classes.root}>
      <input type="checkbox" className={css({ srOnly: true })} />
      <div className={classes.control} />
      <span className={classes.label}>Checkbox Label</span>
    </label>
  )
}
```

### Styling JSX Compound Components

Compound components are a great way to create reusable components for better composition. Slot recipes play nicely with
this pattern and requires a context provider for the component.

> **Note:** This is an advanced topic and you don't need to understand it to use slot recipes. If you use React, be
> aware that context require adding 'use client' to the top of the file.

Let's say you want to design a Checkbox component that can be used like this:

```jsx
<Checkbox size="sm|md" isChecked>
  <Checkbox.Control />
  <Checkbox.Label>Checkbox Label</Checkbox.Label>
</Checkbox>
```

First, create a shared context for ths styles

```jsx filename="style-context.tsx"
'use client'
import { createContext, forwardRef, useContext } from 'react'

export const createStyleContext = recipe => {
  const StyleContext = createContext(null)

  const withProvider = (Component, part) => {
    const Comp = forwardRef((props, ref) => {
      const [variantProps, rest] = recipe.splitVariantProps(props)
      const styles = recipe(variantProps)
      return (
        <StyleContext.Provider value={styles}>
          <Component ref={ref} className={styles?.[part ?? '']} {...rest} />
        </StyleContext.Provider>
      )
    })
    Comp.displayName = Component.displayName || Component.name
    return Comp
  }

  const withContext = (Component, part) => {
    if (!part) return Component

    const Comp = forwardRef((props, ref) => {
      const styles = useContext(StyleContext)
      return <Component ref={ref} className={styles?.[part ?? '']} {...props} />
    })
    Comp.displayName = Component.displayName || Component.name
    return Comp
  }

  return { withProvider, withContext }
}
```

> Note: For the TypeScript version of this file, refer to
> [create-style-context.tsx](https://github.com/cschroeter/park-ui/blob/main/website/src/lib/create-style-context.tsx)
> in Park UI

Then, use the context to create compound components connected to the recipe

```jsx filename="Checkbox.tsx"
import { createStyleContext } from './style-context'
import { checkbox } from './checkbox.recipe'

const { withProvider, withContext } = createStyleContext(checkbox)

//                                  👇🏻 points to the root slot
const Root = withProvider('label', 'root')
//                                    👇🏻 points to the control slot
const Control = withContext('div', 'control')
//                                  👇🏻 points to the label slot
const Label = withContext('span', 'label')

const Checkbox = { Root, Control, Label }
```

## Config Slot Recipe

Config slot recipes are very similar atomic recipes except that they use well-defined classNames and store the styles in
the `recipes` cascade layer.

The config slot recipe takes the following additional properties:

- `className`: The name of the recipe. Used in the generated class name
- `jsx`: An array of JSX components that use the recipe. Defaults to the uppercase version of the recipe name
- `description`: An optional description of the recipe (used in the js-doc comments)

### Defining the recipe

To define a config slot recipe, import the `defineSlotRecipe` function

```jsx filename="checkbox.recipe.ts"
import { defineSlotRecipe } from '@pandacss/dev'

export const checkboxRecipe = defineSlotRecipe({
  className: 'checkbox',
  description: 'The styles for the Checkbox component',
  slots: ['root', 'control', 'label'],
  base: {
    root: { display: 'flex', alignItems: 'center', gap: '2' },
    control: { borderWidth: '1px', borderRadius: 'sm' },
    label: { marginStart: '2' }
  },
  variants: {
    size: {
      sm: {
        control: { width: '8', height: '8' },
        label: { fontSize: 'sm' }
      },
      md: {
        control: { width: '10', height: '10' },
        label: { fontSize: 'md' }
      }
    }
  },
  defaultVariants: {
    size: 'sm'
  }
})
```

### Adding recipe to config

To add the recipe to the config, you’d need to add it to the `slotRecipes` property of the `theme`

```jsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'
import { checkboxRecipe } from './checkbox.recipe'

export default defineConfig({
  //...
  jsxFramework: 'react',
  theme: {
    extend: {
      slotRecipes: {
        checkbox: checkboxRecipe
      }
    }
  }
})
```

### Generate JS code

This generates a recipes folder the specified `outdir` which is `styled-system` by default. If Panda doesn’t
automatically generate your CSS file, you can run the `panda codegen` command.

You only need to import the recipes into the component files where you need to use them.

### Using the recipe

To use the recipe, you can import the recipe from the `<outdir>/recipes` entrypoint and use it in your component. Panda
tracks the usage of the recipe and only generates CSS of the variants used in your application.

```js
import { css } from '../styled-system/css'
import { checkbox } from '../styled-system/recipes'

const Checkbox = () => {
  const classes = checkbox({ size: 'sm' })
  return (
    <label className={classes.root}>
      <input type="checkbox" className={css({ srOnly: true })} />
      <div className={classes.control} />
      <span className={classes.label}>Checkbox Label</span>
    </label>
  )
}
```

The generated css is registered under the `recipe` [cascade layer](/docs/concepts/cascade-layers.mdx) with the class
name that matches the recipe-slot-variant name pattern `<recipe-className>__<slot-name>--<variant-name>`.

```css
@layer recipes {
  @layer base {
    .checkbox__root {
      display: flex;
      align-items: center;
      gap: var(--space-2);
    }

    .checkbox__control {
      border-width: var(--border-widths-1px);
      border-radius: var(--radii-sm);
    }

    .checkbox__label {
      margin-start: var(--space-2);
    }
  }

  .checkbox__control--size-sm {
    width: var(--space-8);
    height: var(--space-8);
  }

  .checkbox__label--size-sm {
    font-size: var(--font-sizes-sm);
  }

  .checkbox__control--size-md {
    width: var(--space-10);
    height: var(--space-10);
  }

  .checkbox__label--size-md {
    font-size: var(--font-sizes-md);
  }
}
```

### TypeScript Guide

Every slot recipe ships a type interface for its accepted variants. You can import them from the `styled-system/recipes`
entrypoint.

For the checkbox recipe, we can import the `CheckboxVariants` type like so:

```ts
import React from 'react'
import type { CheckboxVariants } from '../styled-system/recipes'

type CheckboxProps = CheckboxVariants & {
  children: React.ReactNode
  value?: string
  onChange?: (value: string) => void
}
```

### `defineParts`

It can be useful when you want to have the equivalent of a slot recipe without needing to split the class names bindings
and instead just having a className that handles children on 1 DOM element.

It pairs well with [ZagJs](https://zagjs.com/) and [Ark-UI](https://ark-ui.com/)

Let's refactor the previous example to use parts instead of slots:

```ts
import { defineParts, definetRecipe } from '@pandacss/dev'

const parts = defineParts({
  root: { selector: '& [data-part="root"]' },
  control: { selector: '& [data-part="control"]' },
  label: { selector: '& [data-part="label"]' }
})

export const checkboxRecipe = defineRecipe({
  className: 'checkbox',
  description: 'A checkbox style',
  base: parts({
    root: { display: 'flex', alignItems: 'center', gap: '2' },
    control: { borderWidth: '1px', borderRadius: 'sm' },
    label: { marginStart: '2' }
  }),
  variants: {
    size: {
      sm: parts({
        control: { width: '8', height: '8' },
        label: { fontSize: 'sm' }
      }),
      md: parts({
        control: { width: '10', height: '10' },
        label: { fontSize: 'md' }
      })
    }
  },
  defaultVariants: {
    size: 'sm'
  }
})
```

---


