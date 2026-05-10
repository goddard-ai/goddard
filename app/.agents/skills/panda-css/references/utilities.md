# Utilities

Use this reference for Panda utility props and property-level authoring.

## Background

Panda provides the following utilities or style properties for styling background colors, gradients, and images.

Use these utilities to style background colors, gradients, and images.

## Background Colors

```jsx
<div className={css({ bg: 'red.200' })} />
<div className={css({ bg: 'blue.200/30' })} /> // with alpha
```

| Prop                         | CSS Property       | Token Category |
| ---------------------------- | ------------------ | -------------- |
| `bg`, `background`           | `background`       | `colors`       |
| `bgColor`, `backgroundColor` | `background-color` | `colors`       |
| `bgGradient`                 | `background-image` | `gradients`    |

## Background Gradients

Properties to create a background gradient based on color stops.

```jsx
<div
  className={css({
    bgGradient: 'to-r',
    gradientFrom: 'red.200',
    gradientTo: 'blue.200'
  })}
/>
```

Background and text gradients can be connected to design tokens. Here's how to define a gradient token in your theme.

```ts
const theme = {
  tokens: {
    gradients: {
      // string value
      simple: { value: 'linear-gradient(to right, red, blue)' },
      // composite value
      primary: {
        value: {
          type: 'linear',
          placement: 'to right',
          stops: ['red', 'blue']
        }
      }
    }
  }
}
```

These tokens can be used in the `bgGradient` or `textGradient` properties.

```jsx
<div
  className={css({
    bgGradient: "simple",
  })}
/>

<div
  className={css({
    bgGradient: "primary",
  })}
/>
```

| Prop           | CSS Property       | Token Category |
| -------------- | ------------------ | -------------- |
| `bgGradient`   | `background-image` | `gradients`    |
| `textGradient` | `background-image` | `gradients`    |
| `gradientFrom` | `--gradient-from`  | `colors`       |
| `gradientTo`   | `--gradient-to`    | `colors`       |
| `gradientVia`  | `--gradient-via`   | `colors`       |

## Background Position

Properties for controlling the src and position of a background image.

```jsx
<div
  className={css({
    bgImage: 'url(/images/bg.jpg)',
    bgPosition: 'center'
  })}
/>
```

| Prop                                   | CSS Property        | Token Category |
| -------------------------------------- | ------------------- | -------------- |
| `bgPosition`, `backgroundPosition`     | `background-image`  | none           |
| `bgPositionX`, `backgroundPositionX`   | `background-image`  | none           |
| `bgPositionY`, `backgroundPositionY`   | `background-image`  | none           |
| `bgAttachment` ,`backgroundAttachment` | `background-size`   | none           |
| `bgClip`, `backgroundClip`             | `background-size`   | none           |
| `bgOrigin`, `backgroundOrigin`         | `background-size`   | none           |
| `bgImage`, `backgroundImage`           | `background-size`   | assets         |
| `bgRepeat`, `backgroundRepeat`         | `background-repeat` | none           |
| `bgBlendMode`, `backgroundBlendMode`   | `background-size`   | none           |
| `bgSize`, `backgroundSize`             | `background-size`   | none           |

---


## Border

Panda's border utilities.

Panda provides CSS properties for styling borders.

## Compound Properties

The border compound property maps to the `borders` token category.

| Prop                                | CSS Property        | Token Category |
| ----------------------------------- | ------------------- | -------------- |
| `border`                            | `border`            | `borders`      |
| `borderX` , `borderInline`          | `borderInline`      | `borders`      |
| `borderY` , `borderBlock`           | `borderBlock`       | `borders`      |
| `borderStart` , `borderInlineStart` | `borderInlineStart` | `borders`      |
| `borderEnd` , `borderInlineEnd`     | `borderInlineEnd`   | `borders`      |

## Border Radius

### All sides

```jsx
<div className={css({ borderRadius: 'md' })} />
<div className={css({ rounded: 'md' })} /> // shorthand
```

### Specific sides

Use the `border{Left|Right|Top|Bottom}Radius` properties, or the shorthand equivalent to apply border radius on a
specific side of an element.

```jsx
<div className={css({ borderTopRadius: 'md' })} />
<div className={css({ roundedTop: 'md' })} /> // shorthand

<div className={css({ borderLeftRadius: 'md' })} />
<div className={css({ roundedLeft: 'md' })} /> // shorthand
```

### Specific corners

Use the `border{Top|Bottom}{Left|Right}Radius` properties, or the shorthand equivalent to round a specific corner.

```jsx
<div className={css({ borderTopLeftRadius: 'md' })} />
<div className={css({ roundedTopLeft: 'md' })} /> // shorthand
```

| Prop                                     | CSS Property                        | Token Category |
| ---------------------------------------- | ----------------------------------- | -------------- |
| `rounded`,`borderRadius`                 | `border-radius`                     | `radii`        |
| `roundedTopLeft`,`borderTopLeftRadius`   | `border-top-left-radius`            | `radii`        |
| `roundedTopRight`,`borderTopRight`       | `border-top-right-radius`           | `radii`        |
| `roundedBottomRight`,`borderBottomRight` | `border-bottom-right-radius`        | `radii`        |
| `roundedBottomLeft`,`borderBottomLeft`   | `border-bottom-left-radius`         | `radii`        |
| `roundedTop`,`borderTopRadius`           | `border-top-{left+right}-radius`    | `radii`        |
| `roundedRight`,`borderRightRadius`       | `border-{top+bottom}-right-radius`  | `radii`        |
| `roundedBottom`,`borderBottomRadius`     | `border-bottom-{left+right}-radius` | `radii`        |
| `roundedLeft`,`borderLeftRadius`         | `border-{top+bottom}-left-radius`   | `radii`        |

### Logical Properties

Panda also provides the logical properties for border radius, which map to corresponding physical properties based on
the document's writing mode.

> For example, `borderStartRadius` will map to `border-left-radius` in LTR mode, and `border-right-radius` in RTL mode.

```jsx
<div className={css({ borderStartRadius: 'md' })} />
<div className={css({ roundedStart: 'md' })} /> // shorthand
```

| Prop                                         | CSS Property                      | Token Category |
| -------------------------------------------- | --------------------------------- | -------------- |
| `roundedStartStart`,`borderStartStartRadius` | `border-start-start-radius`       | `radii`        |
| `roundedStartEnd`,`borderStartEndRadius`     | `border-start-end-radius`         | `radii`        |
| `roundedStart`,`borderStartRadius`           | `border-{start+end}-start-radius` | `radii`        |
| `roundedEndStart`,`borderEndStartRadius`     | `border-end-start-radius`         | `radii`        |
| `roundedEndEnd`,`borderEndEndRadius`         | `border-end-end-radius`           | `radii`        |
| `roundedEnd` ,`borderEndRadius`              | `border-{start+end}-end-radius`   | `radii`        |

## Border Width

### All sides

```jsx
<div className={css({ borderWidth: '1px' })} />
```

### Specific sides

Use the `border{Left|Right|Top|Bottom}Width` properties, to apply border width on a specific side of an element.

```jsx
<div className={css({ borderTopWidth: '1px' })} />
<div className={css({ borderLeftWidth: '1px' })} />
```

| Prop                                 | CSS Property                |
| ------------------------------------ | --------------------------- |
| `borderWidth`                        | `border-width`              |
| `borderTopWidth`                     | `border-top-width`          |
| `borderLeftWidth`                    | `border-left-width`         |
| `borderRightWidth`                   | `border-right-width`        |
| `borderBottomWidth`                  | `border-bottom-width`       |
| `borderXWidth` , `borderInlineWidth` | `border-{left+right}-width` |
| `borderYWidth` , `borderBlockWidth`  | `border-{top+bottom}-width` |

### Logical Properties

Panda also provides the logical properties for border width, which map to corresponding physical properties based on the
document's writing mode.

> For example, `borderStartWidth` will map to `border-left-width` in LTR mode, and `border-right-width` in RTL mode.

```jsx
<div className={css({ borderStartWidth: '1px' })} />
```

| Prop                                          | CSS Property               |
| --------------------------------------------- | -------------------------- |
| `borderStartWidth` , `borderInlineStartWidth` | `border-{start+end}-width` |
| `borderEndWidth` , `borderInlineEndWidth`     | `border-{start+end}-width` |

## Border Color

The border color utilities are used to set the border color of an element. It references the `colors` token category.

### All sides

```jsx
<div className={css({ borderColor: 'primary' })} />
```

### Specific sides

Use the `border{Left|Right|Top|Bottom}Color` properties to apply border color on a specific side of an element.

```jsx
<div className={css({ borderTopColor: 'primary' })} />
<div className={css({ borderLeftColor: 'primary' })} />
```

| Prop                | CSS Property          | Token Category |
| ------------------- | --------------------- | -------------- |
| `borderColor`       | `border-color`        | `colors`       |
| `borderTopColor`    | `border-top-color`    | `colors`       |
| `borderLeftColor`   | `border-left-color`   | `colors`       |
| `borderRightColor`  | `border-right-color`  | `colors`       |
| `borderBottomColor` | `border-bottom-color` | `colors`       |

### Logical Properties

Panda also provides the logical properties for border color, which map to corresponding physical properties based on the
document's writing mode.

> For example, `borderInlineStartColor` will map to `border-left-color` in LTR mode, and `border-right-color` in RTL
> mode.

```jsx
<div className={css({ borderInlineStartColor: 'red.500' })} />
```

| Prop                                          | CSS Property               | Token Category |
| --------------------------------------------- | -------------------------- | -------------- |
| `borderStartColor` , `borderInlineStartColor` | `border-{start+end}-color` | `colors`       |
| `borderEndColor` , `borderInlineEndColor`     | `border-{start+end}-color` | `colors`       |
| `borderXColor`, `borderInlineColor`           | `border-inline-color`      | `colors`       |
| `borderYColor`, `borderBlockColor`            | `border-block-color`       | `colors`       |

---


## Display

Panda provides style properties for styling display of an element

Panda provides utilities and style properties for styling display of an element.

## Display Property

```jsx
<div className={css({ display: 'flex' })} />
```

## Hiding Elements

Panda provides shortcut properties for hiding elements from and below a specific breakpoint.

### Hide From

```jsx
<div className={css({ display: 'flex', hideFrom: 'md' })} />
```

### Hide Below

```jsx
<div className={css({ display: 'flex', hideBelow: 'md' })} />
```

| Prop        | CSS Property | Token Category |
| ----------- | ------------ | -------------- |
| `hideFrom`  | `display`    | `breakpoints`  |
| `hideBelow` | `display`    | `breakpoints`  |

---


## Divide

Panda provides style properties for for dividing elements

Panda provides utilities and style properties for dividing elements.

## Divide X

```jsx
<div className={css({ divideX: '2px' })} />
```

## Divide Y

```jsx
<div className={css({ divideY: '2px' })} />
```

## Divide Color

```jsx
<div className={css({ divideColor: 'red.200' })} />
```

## Divide Style

```jsx
<div className={css({ divideStyle: 'dashed' })} />
```

---


## Effects

Panda provides utilities or style properties for applying various visual effects to elements.

Panda offers a range of utilities and style properties for applying visual effects to elements. These effects include
opacity, shadows, blending modes, filters, and more.

## Opacity

```jsx
<div className={css({ opacity: 0.5 })} />
```

| Prop      | CSS Property | Token Category |
| --------- | ------------ | -------------- |
| `opacity` | `opacity`    | `opacity`      |

## Box Shadow

Apply box shadows to elements.

```jsx
<div className={css({ boxShadow: 'lg' })} />
```

| Prop          | CSS Property     | Token Category |
| ------------- | ---------------- | -------------- |
| `boxShadow`   | `box-shadow`     | `shadows`      |
| `shadow`      | `box-shadow`     | `shadows`      |
| `shadowColor` | `--shadow-color` | `colors`       |

## Mix Blend Mode

Control the blending mode of an element.

```jsx
<div className={css({ mixBlendMode: 'multiply' })} />
```

| Prop           | CSS Property     | Token Category |
| -------------- | ---------------- | -------------- |
| `mixBlendMode` | `mix-blend-mode` | none           |

## Filter

Apply various filters to elements.

```jsx
<div className={css({ filter: 'auto', blur: 'sm' })} />
```

| Prop         | CSS Property    | Token Category |
| ------------ | --------------- | -------------- |
| `filter`     | `filter`        | none           |
| `blur`       | `--blur`        | `blurs`        |
| `brightness` | `--brightness`  | none           |
| `contrast`   | `--contrast`    | none           |
| `grayscale`  | `--grayscale`   | none           |
| `hueRotate`  | `--hue-rotate`  | none           |
| `invert`     | `--invert`      | none           |
| `saturate`   | `--saturate`    | none           |
| `sepia`      | `--sepia`       | none           |
| `dropShadow` | `--drop-shadow` | `dropShadows`  |

## Backdrop Filter

Apply filters to the backdrop of an element.

```jsx
<div className={css({ backdropFilter: 'auto', backdropBlur: 'sm' })} />
```

| Prop                 | CSS Property            | Token Category |
| -------------------- | ----------------------- | -------------- |
| `backdropFilter`     | `backdrop-filter`       | none           |
| `backdropBlur`       | `--backdrop-blur`       | `blurs`        |
| `backdropBrightness` | `--backdrop-brightness` | none           |
| `backdropContrast`   | `--backdrop-contrast`   | none           |
| `backdropGrayscale`  | `--backdrop-grayscale`  | none           |
| `backdropHueRotate`  | `--backdrop-hue-rotate` | none           |
| `backdropInvert`     | `--backdrop-invert`     | none           |
| `backdropOpacity`    | `--backdrop-opacity`    | none           |
| `backdropSaturate`   | `--backdrop-saturate`   | none           |
| `backdropSepia`      | `--backdrop-sepia`      | none           |

---


## Flex and Grid

Panda provides a set of utilities and style properties for flexible box layout (Flex) and grid layout (Grid). These utilities are designed to make it easy to create responsive and dynamic layouts in your application.

Panda provides a set of utilities and style properties for flexible box layout (Flex) and grid layout (Grid). These
utilities are designed to make it easy to create responsive and dynamic layouts in your application.

## Flex

Flex utilities are designed to control the layout and behavior of flex containers and items.

### Flex Basis

The `flexBasis` utility sets the initial main size of a flex item, distributing the available space along the main axis.
It supports `spacing` tokens and fractional literal values like “1/2”, “2/3", etc.

```jsx
<div className={css({ basis: '1/2' })} />
```

### Flex

The `flex` utility defines the flexibility of a flex container or item. Supported values:

| Value     |            |
| --------- | ---------- |
| `1`       | `1 1 0%`   |
| `auto`    | `1 1 auto` |
| `initial` | `0 1 auto` |
| `none`    | `none`     |

### Flex Direction

The `flexDirection` utility sets the direction of the main axis in a flex container. It's shorthand is `flexDir`.

```jsx
<div className={css({ flexDir: 'column' })} />
```

## Grid

Grid utilities offer control over various grid layout properties, providing a powerful system for creating layouts with
rows and columns.

### Grid Template Columns

The `gridTemplateColumns` utility defines the columns of a grid container.

```jsx
<div className={css({ gridTemplateColumns: 'repeat(3, minmax(0, 1fr))' })} />
```

### Grid Template Rows

The `gridTemplateRows` utility defines the rows of a grid container.

```jsx
<div className={css({ gridTemplateRows: 'repeat(3, minmax(0, 1fr))' })} />
```

---


## Focus Ring

Style focus states with accessibility-first focus ring utilities.

Focus rings are essential for accessibility, helping users identify which element currently has keyboard focus. Panda
provides comprehensive focus ring utilities that work with both regular focus and focus-visible states.

## Focus Ring Variants

The `focusRing` utility applies focus styles using the `&:is(:focus, [data-focus])` selector and supports four variants:

```jsx
// Outside focus ring (default 2px offset)
<button className={css({ focusRing: 'outside' })}>
  Click me
</button>

// Inside focus ring (no offset, with border)
<button className={css({ focusRing: 'inside' })}>
  Click me
</button>

// Mixed focus ring (semi-transparent with border)
<button className={css({ focusRing: 'mixed' })}>
  Click me
</button>

// No focus ring
<button className={css({ focusRing: 'none' })}>
  Click me
</button>
```

## Focus Visible Ring

The `focusVisibleRing` utility only applies focus styles during keyboard navigation using the
`&:is(:focus-visible, [data-focus-visible])` selector:

```jsx
<button className={css({ focusVisibleRing: 'outside' })}>Only shows focus ring on keyboard navigation</button>
```

### Focus Ring vs. Focus Visible Ring

The Focus Visible Ring functions similarly to the Focus Ring, but with a key difference: it only applies focus indicator
styles when an element receives keyboard focus.

This ensures that the focus ring is visible only when navigating via keyboard, improving accessibility without affecting
mouse interactions.

## Customization

You can customize focus ring appearance with additional utilities:

```jsx
<button
  className={css({
    focusRing: 'outside',
    focusRingColor: 'blue.500',
    focusRingWidth: '3px',
    focusRingStyle: 'dashed',
    focusRingOffset: '4px'
  })}
>
  Custom focus ring
</button>
```

| Prop               | CSS Property     | Token Category | Description              |
| ------------------ | ---------------- | -------------- | ------------------------ |
| `focusRing`        | Multiple         | Enum           | Focus ring variant       |
| `focusVisibleRing` | Multiple         | Enum           | Keyboard-only focus ring |
| `focusRingColor`   | `outline-color`  | `colors`       | Focus ring color         |
| `focusRingWidth`   | `outline-width`  | `borderWidths` | Focus ring thickness     |
| `focusRingStyle`   | `outline-style`  | `borderStyles` | Focus ring style         |
| `focusRingOffset`  | `outline-offset` | `spacing`      | Distance from element    |

### Ring Color

To change the focus ring color for a specific component, use the `focusRingColor` prop:

```jsx
<button className={css({ focusRing: 'outside', focusRingColor: 'red.500' })}>Red focus ring</button>
```

## Global Ring Color

You can set a global focus ring color by defining the CSS custom property:

```ts
// panda.config.ts
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  globalCss: {
    html: {
      '--global-color-focus-ring': '#3b82f6'
    }
  }
})
```

### Ring Width

To change the focus ring width for a specific component, use the `focusRingWidth` prop:

```jsx
<button className={css({ focusRing: 'outside', focusRingWidth: '4px' })}>Thick focus ring</button>
```

### Ring Style

To change the focus ring style for a specific component, use the `focusRingStyle` prop:

```jsx
<button className={css({ focusRing: 'outside', focusRingStyle: 'dashed' })}>Dashed focus ring</button>
```

This color will be used as the default for all focus rings unless overridden by the `focusRingColor` prop.

## Examples

### Basic Focus Ring

```jsx
import { css } from '../styled-system/css'

function Button({ children }) {
  return (
    <button
      className={css({
        px: '4',
        py: '2',
        bg: 'blue.500',
        color: 'white',
        rounded: 'md',
        focusRing: 'outside'
      })}
    >
      {children}
    </button>
  )
}
```

### Keyboard-Only Focus Ring

```jsx
import { css } from '../styled-system/css'

function Input({ ...props }) {
  return (
    <input
      className={css({
        px: '3',
        py: '2',
        borderWidth: '1px',
        borderColor: 'gray.300',
        rounded: 'md',
        focusVisibleRing: 'outside',
        focusRingColor: 'blue.500'
      })}
      {...props}
    />
  )
}
```

### Custom Focus Ring

```jsx
import { css } from '../styled-system/css'

function CustomButton({ children }) {
  return (
    <button
      className={css({
        px: '6',
        py: '3',
        bg: 'purple.600',
        color: 'white',
        rounded: 'lg',
        focusRing: 'mixed',
        focusRingColor: 'purple.400',
        focusRingWidth: '3px',
        focusRingOffset: '3px'
      })}
    >
      {children}
    </button>
  )
}
```

---


## Gradients

Create smooth color transitions with linear, radial, and conic gradient utilities.

Create smooth color transitions with linear, radial, and conic gradient utilities.

| Property             | Shorthand    | CSS Property       | Description                    |
| -------------------- | ------------ | ------------------ | ------------------------------ |
| `backgroundLinear`   | `bgLinear`   | `background-image` | Apply linear gradient effects  |
| `backgroundRadial`   | `bgRadial`   | `background-image` | Apply radial gradient effects  |
| `backgroundConic`    | `bgConic`    | `background-image` | Apply conic gradient effects   |
| `backgroundGradient` | `bgGradient` | `background-image` | Apply linear gradient (alias)  |
| `textGradient`       | -            | `background-image` | Apply gradient to text         |
| `gradientFrom`       | -            | `--gradient-from`  | Define starting gradient color |
| `gradientTo`         | -            | `--gradient-to`    | Define ending gradient color   |
| `gradientVia`        | -            | `--gradient-via`   | Define middle gradient color   |

## Examples

### Linear gradients

Use the `bgLinear` utility to create a linear gradient with a direction.

```tsx
css({ bgLinear: 'to-r', gradientFrom: 'cyan.500', gradientTo: 'blue.500' })
css({ bgLinear: 'to-t', gradientFrom: 'sky.500', gradientTo: 'indigo.500' })
css({
  bgLinear: 'to-bl',
  gradientFrom: 'violet.500',
  gradientTo: 'fuchsia.500'
})
css({ bgLinear: '65deg', gradientFrom: 'purple.500', gradientTo: 'pink.500' })
```

### Radial gradients

Build circular color transitions that radiate outward from a center point:

```tsx
css({
  bgRadial: 'in srgb',
  gradientFrom: 'pink.400',
  gradientFromPosition: '40%',
  gradientTo: 'fuchsia.700'
})
css({
  bgRadial: 'at 50% 75%',
  gradientFrom: 'sky.200',
  gradientVia: 'blue.400',
  gradientTo: 'indigo.900',
  gradientToPosition: '90%'
})
css({
  bgRadial: 'at 25% 25%',
  gradientFrom: 'white',
  gradientTo: 'zinc.900',
  gradientToPosition: '75%'
})
```

### Conic gradients

Create sweeping color transitions that rotate around a central point:

```jsx
css({
  boxSize: '24',
  rounded: 'full',
  bgConic: 'in srgb',
  gradientFrom: 'blue.600',
  gradientTo: 'sky.400',
  gradientToPosition: '50%'
})
css({
  boxSize: '24',
  rounded: 'full',
  bgConic: 'from 180deg',
  gradientFrom: 'blue.600',
  gradientVia: 'blue.50',
  gradientTo: 'blue.600'
})
css({
  boxSize: '24',
  rounded: 'full',
  bgConic: 'in oklch decreasing hue',
  gradientFrom: 'violet.700',
  gradientVia: 'lime.300',
  gradientTo: 'violet.700'
})
```

### Controlling color stops

Define which colors appear in your gradient using `gradientFrom`, `gradientVia`, and `gradientTo`:

```jsx
css({
  bgLinear: 'to-r',
  gradientFrom: 'indigo.500',
  gradientVia: 'purple.500',
  gradientTo: 'pink.500'
})
```

### Positioning color stops

Control exactly where each color appears along the gradient using position utilities:

```jsx
css({
  bgLinear: 'to-r',
  gradientFrom: 'indigo.500',
  gradientFromPosition: '10%',
  gradientVia: 'sky.500',
  gradientViaPosition: '30%',
  gradientTo: 'emerald.500',
  gradientToPosition: '90%'
})
```

### Text gradients

Apply colorful gradient effects to typography using `textGradient`:

```jsx
css({
  textGradient: 'to-r',
  gradientFrom: 'purple.400',
  gradientTo: 'pink.400',
  fontSize: '4xl',
  fontWeight: 'bold'
})
```

### Custom gradient values

Use bracket notation to create gradients with arbitrary values:

```jsx
css({
  bgLinear: '25deg',
  gradientFrom: 'red.50%',
  gradientVia: 'yellow.60%',
  gradientTo: 'lime.90%',
  gradientToPosition: 'teal.100%'
})
```

### Responsive gradients

Apply different gradient styles at different breakpoints using responsive objects:

```jsx
css({
  gradientFrom: { base: 'purple.400', md: 'yellow.500' }
})
```

---


## Helpers

Panda CSS offers utility classes to enhance accessibility and aid in debugging.

Panda CSS offers utility classes to enhance accessibility and aid in debugging.

## Screen Reader-Only Content

The srOnly utility class hides content visually while keeping it accessible to screen readers. It is particularly useful
when you want to provide information to screen readers without displaying it on the screen.

```jsx
<div className={css({ srOnly: true })}>Accessible only to screen readers</div>
```

## Debug

The debug utility class applies styles to aid in debugging by adding outlines to elements. This can be helpful during
development to visually inspect the layout and structure of your components.

```jsx
<div className={css({ debug: true })}>Debugging outline applied</div>
```

---


## Interactivity

Panda CSS provides a variety of utility classes to enhance interactivity and user experience on your web applications.

Panda CSS provides a variety of utility classes to enhance interactivity and user experience on your web applications.
These utilities cover aspects such as accent color, caret color, scrolling behavior, scrollbar customization, scroll
margins and paddings, scroll snapping, touch actions, and user selection.

## Accent Color

The `accentColor` utility class sets the accent color of an element. It supports `colors` tokens.

```jsx
<div className={css({ accentColor: 'blue.500' })}>Accent color applied</div>
```

## Caret Color

The `caretColor` utility class sets the color of the text cursor (caret) in an input or textarea. It supports `colors`
tokens.

```jsx
<input className={css({ caretColor: 'red.400' })} />
```

## Scrollbar

The `scrollbar` utility allows customization of scrollbar appearance. It supports `visible` and `hidden` values which
respectively show or hide the scrollbar.

```jsx
<div className={css({ scrollbar: 'hidden' })}>Scrollbar hidden</div>
```

## Scroll Margin

Scroll margin utilities set margins around scroll containers.

```jsx
<div className={css({ scrollMarginX: '2' })}>Scrollbar Container with Inline padding</div>
```

| Prop                                  | CSS Property                 | Token Category |
| ------------------------------------- | ---------------------------- | -------------- |
| `scrollMarginX` ,`scrollMarginInline` | `scroll-margin-inline`       | `spacing`      |
| `scrollMarginInlineStart`             | `scroll-margin-inline-start` | `spacing`      |
| `scrollMarginInlineEnd`               | `scroll-margin-inline-end`   | `spacing`      |
| `scrollMarginY` , `scrollMarginBlock` | `scroll-margin-block`        | `spacing`      |
| `scrollMarginBlockStart`              | `scroll-margin-block-start`  | `spacing`      |
| `scrollMarginBlockEnd`                | `scroll-margin-block-end`    | `spacing`      |
| `scrollMarginLeft`                    | `scroll-margin-left`         | `spacing`      |
| `scrollMarginRight`                   | `scroll-margin-right`        | `spacing`      |
| `scrollMarginTop`                     | `scroll-margin-top`          | `spacing`      |
| `scrollMarginBottom`                  | `scroll-margin-bottom`       | `spacing`      |

## Scroll Padding

Scroll padding utilities set padding inside scroll containers.

```jsx
<div className={css({ scrollPaddingY: '2' })}>Scrollbar Container with block padding</div>
```

| Prop                                     | CSS Property                  | Token Category |
| ---------------------------------------- | ----------------------------- | -------------- |
| `scrollPaddingX` , `scrollPaddingInline` | `scroll-padding-inline`       | `spacing`      |
| `scrollPaddingInlineStart`               | `scroll-padding-inline-start` | `spacing`      |
| `scrollPaddingInlineEnd`                 | `scroll-padding-inline-end`   | `spacing`      |
| `scrollPaddingY` , `scrollPaddingBlock`  | `scroll-padding-block`        | `spacing`      |
| `scrollPaddingBlockStart`                | `scroll-padding-block-start`  | `spacing`      |
| `scrollPaddingBlockEnd`                  | `scroll-padding-block-end`    | `spacing`      |
| `scrollPaddingLeft`                      | `scroll-padding-left`         | `spacing`      |
| `scrollPaddingRight`                     | `scroll-padding-right`        | `spacing`      |
| `scrollPaddingTop`                       | `scroll-padding-top`          | `spacing`      |
| `scrollPaddingBottom`                    | `scroll-padding-bottom`       | `spacing`      |

## Scroll Snapping

Scroll snapping utilities provide control over the scroll snap behavior.

### Scroll Snap Margin

| Prop                     | CSS Property           | Token Category |
| ------------------------ | ---------------------- | -------------- |
| `scrollSnapMargin`       | `scroll-margin`        | `spacing`      |
| `scrollSnapMarginTop`    | `scroll-margin-top`    | `spacing`      |
| `scrollSnapMarginBottom` | `scroll-margin-bottom` | `spacing`      |
| `scrollSnapMarginLeft`   | `scroll-margin-left`   | `spacing`      |
| `scrollSnapMarginRight`  | `scroll-margin-right`  | `spacing`      |

### Scroll Snap Strictness

It's values can be `mandatory` or `proximity` values, and maps to `var(--scroll-snap-strictness)`.

```jsx
<div className={css({ scrollSnapStrictness: 'proximity' })}>Scroll container with proximity scroll snap</div>
```

### Scroll Snap Type

Supported values

| Value  |                                      |
| ------ | ------------------------------------ |
| `none` | `none`                               |
| `x`    | `x var(--scroll-snap-strictness)`    |
| `y`    | `y var(--scroll-snap-strictness)`    |
| `both` | `both var(--scroll-snap-strictness)` |

---


## Layout

Panda provides style properties for styling layout of an element

Panda provides style properties for styling layout of an element.

## Aspect Ratio

Use the `aspectRatio` utilities to set the desired aspect ratio of an element.

Values can reference the `aspectRatios` token category.

```jsx
<div className={css({ aspectRatio: 'square' })} />
```

> This uses the native CSS property `aspect-ratio` which is might not supported in all browsers. Consider using the
> [`AspectRatio` pattern](/docs/concepts/patterns#aspect-ratio) instead

## Position

Use the `position` utilities to set the position of an element.

```jsx
<div className={css({ position: 'absolute' })} />
<div className={css({ pos: 'absolute' })} /> // shorthand
```

## Top / Right / Bottom / Left

Use the `top`, `right`, `bottom` and `left` utilities to set the position of an element.

Values can reference the `spacing` token category.

```jsx
<div className={css({ position: 'absolute', top: '0', left: '0' })} />
```

| Prop     | CSS Property | Token Category |
| -------- | ------------ | -------------- |
| `top`    | `top`        | `spacing`      |
| `right`  | `right`      | `spacing`      |
| `bottom` | `bottom`     | `spacing`      |
| `left`   | `left`       | `spacing`      |

### Logical Properties

Use the `inset{Start|End}` utilities to set the position of an element based on the writing mode.

> For example, `insetStart` will set the `left` property in `ltr` mode and `right` in `rtl` mode.

```jsx
<div className={css({ position: 'absolute', insetStart: '0' })} />
```

| Prop                                      | CSS Property         | Token Category |
| ----------------------------------------- | -------------------- | -------------- |
| `start`, `insetStart`, `insetInlineStart` | `inset-inline-start` | `spacing`      |
| `end` , `insetEnd`, `insetInlineEnd`      | `inset-inline-end`   | `spacing`      |
| `insetX`, `insetInline`                   | `inset-inline`       | `spacing`      |
| `insetY`, `insetBlock`                    | `inset-inline`       | `spacing`      |

## Container Query

You can define container names and sizes in your theme configuration and use them in your styles.
[Read more.](/docs/concepts/conditional-styles#container-queries)

| Prop            | CSS Property    | Token Category   |
| --------------- | --------------- | ---------------- |
| `containerName` | `containerName` | `containerNames` |

---


## List

Panda provides utilities for customizing list styles.

Panda provides utilities for customizing list styles.

## List Style Image

Use a custom image as the list marker. It supports the `assets` token category.

```js filename="panda.config.ts"
const theme = {
  tokens: {
    assets: {
      star: {
        value: { type: 'svg', value: '<svg>...</svg>' }
      }
    }
  }
}
```

```jsx
<div className={css({ listStyleImage: 'star' })} />
```

---


## Outline

Panda provides utilities for customizing outlines.

Panda provides utilities for customizing outlines.

## Compound Property

Set the width, color, and style of the outline.

```jsx
<div className={css({ outline: '2px solid blue.500' })} />
<div className={css({ ring: '2px solid blue.500' })} /> // shorthand
```

| Prop               | CSS Property | Token Category |
| ------------------ | ------------ | -------------- |
| `ring` , `outline` | `outline`    | `borders`      |

## Outline Width

Change the width of the outline.

```jsx
<div className={css({ outlineWidth: '4' })} />
<div className={css({ outlineWidth: '2px' })} />
<div className={css({ ringWidth: '2px' })} /> // shorthand
```

| Prop                         | CSS Property    | Token Category |
| ---------------------------- | --------------- | -------------- |
| `ringWidth` , `outlineWidth` | `outline-width` | `borderWidths` |

## Outline Color

Change the color of the outline.

```jsx
<div className={css({ outlineColor: 'blue.500' })} />
<div className={css({ ringColor: 'blue.500' })} /> // shorthand
```

| Prop           | CSS Property    | Token Category |
| -------------- | --------------- | -------------- |
| `outlineColor` | `outline-color` | `colors`       |

## Outline Offset

Adjust the space between the outline and the element.

```jsx
<div className={css({ outlineOffset: '4' })} />
<div className={css({ ringOffset: '4' })} /> // shorthand
```

| Prop            | CSS Property     | Token Category |
| --------------- | ---------------- | -------------- |
| `outlineOffset` | `outline-offset` | `spacing`      |

---


## Sizing

Style properties for controlling the size of an element.

Style properties for controlling the size of an element.

## Width

Use the `width` or `w` property to set the width of an element.

```jsx
<div className={css({ width: '5' })} />
<div className={css({ w: '5' })} /> // shorthand
```

### Fractional width

Use can set fractional widths using the `width` or `w` property.

Values can be within the following ranges:

- Thirds: `1/3` to `2/3`
- Fourths: `1/4` to `3/4`
- Fifths: `1/5` to `4/5`
- Sixths: `1/6` to `5/6`
- Twelfths: `1/12` to `11/12`

```jsx
<div className={css({ width: '1/2' })} />
<div className={css({ w: '1/2' })} /> // shorthand

<div className={css({ width: '1/3' })} />
<div className={css({ w: '1/3' })} /> // shorthand
```

### Max width

Use the `maxWidth` or `maxW` property to set the maximum width of an element.

```jsx
<div className={css({ maxWidth: '5' })} />
<div className={css({ maxW: '5' })} /> // shorthand
```

### Min width

Use the `minWidth` or `minW` property to set the minimum width of an element.

```jsx
<div className={css({ minWidth: '5' })} />
<div className={css({ minW: '5' })} /> // shorthand
```

| Prop               | CSS Property | Token Category |
| ------------------ | ------------ | -------------- |
| `w`, `width`       | `width`      | `sizes`        |
| `maxW`, `maxWidth` | `max-width`  | `sizes`        |
| `minW`, `minWidth` | `min-width`  | `sizes`        |

## Height

Use the `height` or `h` property to set the height of an element.

```jsx
<div className={css({ height: '5' })} />
<div className={css({ h: '5' })} /> // shorthand
```

### Fractional height

Use can set fractional heights using the `height` or `h` property.

Values can be within the following ranges:

- Thirds: `1/3` to `2/3`
- Fourths: `1/4` to `3/4`
- Fifths: `1/5` to `4/5`
- Sixths: `1/6` to `5/6`

```jsx
<div className={css({ height: '1/2' })} />
<div className={css({ h: '1/2' })} /> // shorthand
```

### Relative heights

You can use the modern relative height values `dvh`, `svh`, `lvh`.

```jsx
<div className={css({ height: 'dvh' })} />
<div className={css({ h: 'dvh' })} /> // shorthand
```

### Max height

Use the `maxHeight` or `maxH` property to set the maximum height of an element.

```jsx
<div className={css({ maxHeight: '5' })} />
<div className={css({ maxH: '5' })} /> // shorthand
```

### Min height

Use the `minHeight` or `minH` property to set the minimum height of an element.

```jsx
<div className={css({ minHeight: '5' })} />
<div className={css({ minH: '5' })} /> // shorthand
```

| Prop                | CSS Property | Token Category |
| ------------------- | ------------ | -------------- |
| `h`, `height`       | `height`     | `sizes`        |
| `maxH`, `maxHeight` | `max-height` | `sizes`        |
| `minH`, `minHeight` | `min-height` | `sizes`        |

### Size

Use the `boxSize` property to set the width and height of an element.

```jsx
<div className={css({ boxSize: '24' })} />
```

| Prop      | CSS Property    | Token Category |
| --------- | --------------- | -------------- |
| `boxSize` | `width, height` | `sizes`        |

---


## Spacing

Style properties for controlling the padding of an element.

## Padding

### All sides

Use the `padding` property to apply padding on all sides of an element

```jsx
<div className={css({ padding: '4' })} />
<div className={css({ p: '4' })} /> // shorthand
```

### Specific sides

Use the `padding{Left|Right|Top|Bottom}` to apply padding on one side of an element

```jsx
<div className={css({ paddingLeft: '3' })} />
<div className={css({ pl: '3' })} /> // shorthand

<div className={css({ paddingTop: '3' })} />
<div className={css({ pt: '3' })} /> // shorthand
```

### Horizontal and Vertical padding

Use the `padding{X|Y}` properties to apply padding on the horizontal and vertical axis of an element

```jsx
<div className={css({ paddingX: '8' })} />
<div className={css({ px: '8' })} /> // shorthand

<div className={css({ paddingY: '8' })} />
<div className={css({ py: '8' })} /> // shorthand
```

| Prop                  | CSS Property     | Token Category |
| --------------------- | ---------------- | -------------- |
| `p`,`padding`         | `padding`        | `spacing`      |
| `pl`, `paddingLeft`   | `padding-left`   | `spacing`      |
| `pr`, `paddingRight`  | `padding-right`  | `spacing`      |
| `pt`, `paddingTop`    | `padding-top`    | `spacing`      |
| `pb`, `paddingBottom` | `padding-bottom` | `spacing`      |
| `px`, `paddingX`      | `padding-inline` | `spacing`      |
| `py`, `paddingY`      | `padding-block`  | `spacing`      |

### Logical properties

Use the `padding{Start|End}` properties to apply padding on the logical axis of an element based on the text direction.

```jsx
<div className={css({ paddingStart: '8' })} />
<div className={css({ ps: '8' })} /> // shorthand

<div className={css({ paddingEnd: '8' })} />
<div className={css({ pe: '8' })} /> // shorthand
```

| Prop                 | CSS Property           | Token Category |
| -------------------- | ---------------------- | -------------- |
| `ps`, `paddingStart` | `padding-inline-start` | `spacing`      |
| `pe`, `paddingEnd`   | `padding-inline-end`   | `spacing`      |

## Margin

### All sides

Use the `margin` property to apply margin on all sides of an element

```jsx
<div className={css({ margin: '5' })} />
<div className={css({ m: '5' })} /> // shorthand
```

### Specific sides

Use the `margin{Left|Right|Top|Bottom}` to apply margin on one side of an element

```jsx
<div className={css({ marginLeft: '3' })} />
<div className={css({ ml: '3' })} /> // shorthand

<div className={css({ marginTop: '3' })} />
<div className={css({ mt: '3' })} /> // shorthand
```

### Horizontal and Vertical margin

Use the `margin{X|Y}` properties to apply margin on the horizontal and vertical axis of an element

```jsx
<div className={css({ marginX: '8' })} />
<div className={css({ mx: '8' })} /> // shorthand

<div className={css({ marginY: '8' })} />
<div className={css({ my: '8' })} /> // shorthand
```

| Prop                 | CSS Property    | Token Category |
| -------------------- | --------------- | -------------- |
| `m`,`margin`         | `margin`        | `spacing`      |
| `ml`, `marginLeft`   | `margin-left`   | `spacing`      |
| `mr`, `marginRight`  | `margin-right`  | `spacing`      |
| `mt`, `marginTop`    | `margin-top`    | `spacing`      |
| `mb`, `marginBottom` | `margin-bottom` | `spacing`      |
| `mx`, `marginX`      | `margin-left`   | `spacing`      |
| `my`, `marginY`      | `margin-top`    | `spacing`      |

### Logical properties

Use the `margin{Start|End}` properties to apply margin on the logical axis of an element based on the text direction.

```jsx
<div className={css({ marginStart: '8' })} />
<div className={css({ ms: '8' })} /> // shorthand

<div className={css({ marginEnd: '8' })} />
<div className={css({ me: '8' })} /> // shorthand
```

| Prop                | CSS Property          | Token Category |
| ------------------- | --------------------- | -------------- |
| `ms`, `marginStart` | `margin-inline-start` | `spacing`      |
| `me`, `marginEnd`   | `margin-inline-end`   | `spacing`      |

---


## SVG

Panda provides utilities for styling SVG elements.

Panda provides utilities for styling SVG elements.

## Fill

Change the fill color of an SVG element.

```jsx
<svg className={css({ fill: 'blue.500' })} />
```

| Prop   | CSS Property | Token Category |
| ------ | ------------ | -------------- |
| `fill` | `fill`       | `colors`       |

## Stroke

Change the stroke color of an SVG element.

```jsx
<svg className={css({ stroke: 'blue.500' })} />
```

| Prop     | CSS Property | Token Category |
| -------- | ------------ | -------------- |
| `stroke` | `stroke`     | colors         |

## Stroke Width

Change the stroke width of an SVG element.

```jsx
<svg className={css({ strokeWidth: '1px' })} />
```

| Prop          | CSS Property   | Token Category |
| ------------- | -------------- | -------------- |
| `strokeWidth` | `stroke-width` | borderWidths   |

---


## Tables

Panda provides utilities for styling tables.

Panda provides utilities for styling tables.

## Border Spacing

Control the border-spacing property of a table.

```jsx
<table className={css({ borderSpacing: '2' })} />
```

| Prop            | CSS Property     | Token Category |
| --------------- | ---------------- | -------------- |
| `borderSpacing` | `border-spacing` | `spacing`      |

## Border Spacing X

Control the horizontal border-spacing property of a table.

```jsx
<table className={css({ borderSpacingX: '2' })} />
```

| Prop             | CSS Property     | Token Category |
| ---------------- | ---------------- | -------------- |
| `borderSpacingX` | `border-spacing` | `spacing`      |

## Border Spacing Y

Control the vertical border-spacing property of a table.

```jsx
<table className={css({ borderSpacingY: '2' })} />
```

| Prop             | CSS Property     | Token Category |
| ---------------- | ---------------- | -------------- |
| `borderSpacingY` | `border-spacing` | `spacing`      |

---


## Transforms

Panda provides utilities for transforming elements.

Panda provides utilities for transforming elements.

## Scale

Control the scale property. Supported value is `auto`

```jsx
<div className={css({ scale: 'auto' })} /> // => 'var(--scale-x) var(--scale-y)'
```

### Scale X

Control the scaleX property.

```jsx
<div className={css({ scaleX: '1.3' })} /> // => --scale-x: 1.3;
```

### Scale Y

Control the scaleY property.

```jsx
<div className={css({ scaleY: '0.4' })} /> // => --scale-y: 0.4;
```

## Translate

Control the translate property. Supported value is `auto`

```jsx
<div className={css({ translate: 'auto' })} /> // => 'var(--translate-x) var(--translate-y)'
```

### Translate X

Control the translateX property.

```jsx
<div className={css({ translateX: '50%' })} /> // => --translate-x: 50%;
<div className={css({ x: '20px' })} /> // shorthand
```

### Translate Y

Control the translateY property.

```jsx
<div className={css({ translateY: '-40%' })} /> // => --translate-y: -40%;
<div className={css({ y: '4rem' })} /> // shorthand
```

---


## Transitions

Panda provides utilities for defining and customizing transitions.

Panda provides utilities for defining and customizing transitions.

## Transition

A shorthand utility for defining common transition sets.

Values are `all`, `common`, `colors`, `opacity`, `shadow`, `transform`.

```jsx
<div className={css({ transition: 'all' })} />
<div className={css({ transitionTimingFunction: 'linear' })} />
<div className={css({ transitionDelay: 'fast' })} />
<div className={css({ transitionDuration: 'faster' })} />
```

| Prop                       | CSS Property                 | Token Category |
| -------------------------- | ---------------------------- | -------------- |
| `transitionTimingFunction` | `transition-timing-function	` | `easings`      |
| `transitionDelay`          | `transition-delay	`           | `durations`    |
| `transitionDuration`       | `transition-duration	`        | `durations`    |

## Animation

Control the animation property. It supports the `animations` token category.

```jsx
<div className={css({ animation: 'bounce' })} />
<div className={css({ animationName: 'pulse' })} />
<div className={css({ animationDelay: 'fast' })} />
```

| Prop             | CSS Property      | Token Category |
| ---------------- | ----------------- | -------------- |
| `animation`      | `animation-name	`  | animations     |
| `animationName`  | `animation-name	`  | animationName  |
| `animationDelay` | `animation-delay	` | durations      |

---


## Typography

Panda's typography utilities.

Panda provides utilities and style properties for styling text.

## Font Properties

```jsx
<div className={css({ fontFamily: 'mono' })} />
```

| Prop                  | CSS Property            | Token Category   |
| --------------------- | ----------------------- | ---------------- |
| `fontFamily`          | `font-family`           | `fonts`          |
| `fontSize`            | `font-size`             | `fontSizes`      |
| `fontWeight`          | `font-weight`           | `fontWeights`    |
| `letterSpacing`       | `letter-spacing`        | `letterSpacings` |
| `lineHeight`          | `line-height`           | `lineHeights`    |
| `textDecorationColor` | `text-decoration-color` | `colors`         |
| `textEmphasisColor`   | `text-emphasis-color`   | `colors`         |
| `textIndent`          | `text-indent`           | `spacing`        |
| `textShadow`          | `text-shadow`           | `shadows`        |

## Line Clamp (Truncation)

How to truncate multi-line text

```jsx
<div className={css({ lineClamp: 2 })}>Some long piece of text</div>

<div className={css({ lineClamp: 2 })}>Truncated text</div>
```

| Prop        | CSS Property        | Token Category |
| ----------- | ------------------- | -------------- |
| `lineClamp` | `webkit-line-clamp` | none           |
| `truncate`  | `text-overflow`     | none           |

## Text Styles

Utilities for applying a composition of typography properties.

```jsx
<h1
  className={css({
    textStyle: 'heading/marketing'
  })}
>
  Hello World
</h1>
```

| Prop        | Config       |
| ----------- | ------------ |
| `textStyle` | `textStyles` |

---


