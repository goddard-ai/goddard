# Styling Foundations

Use this reference for cascade layers, conditional styles, global styles, extend semantics, integration hooks, style context helpers, and style merging.

## Cascade Layers

CSS cascade layers refer to the order in which CSS rules are applied to an HTML element.

When multiple CSS rules apply
to the same element, the browser uses the cascade to determine which rule should take precedence. See the
[MDN article](https://developer.mozilla.org/en-US/docs/Web/CSS/@layer) to learn more.

Panda takes advantage of the cascade to provide a more efficient and flexible way to organize styles. This allows you to
define styles in a modular way, using CSS rules that are scoped to specific components or elements.

## Layer Types

Panda supports five types of cascade layers out of the box:

- `@layer reset` - The reset layer is used to reset the default styles of HTML elements. This is used when
  `preflight: true` is set in the config. You can also use this layer to add your own reset styles.

The generated CSS for the reset layer looks like this:

```css
@layer reset {
  * {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }
  /* ... */
}
```

- `@layer base` - The base layer contains global styles defined in the `globalStyles` key in the config. You can also
  use this layer to add your own global styles.

The generated CSS for the base layer looks like this:

```css
@layer base {
  a {
    color: #000;
    text-decoration: none;
  }
  /* ... */
}
```

- `@layer recipes` - The recipes layer contains styles for recipes created within the config (aka config recipes). You
  can also use this layer to add your own component styles.

The generated CSS for the recipes layer looks like this:

```css
@layer recipes {
  .button {
    /* ... */
  }

  .button--variant-primary {
    /* ... */
  }
  /* ... */
}
```

- `@layer tokens` - The tokens layer contains css variables for tokens and semantic tokens. You can also use this layer
  to add your own design tokens.

The generated CSS for the tokens layer looks like this:

```css
@layer tokens {
  :root {
    --color-primary: #000;
    --color-secondary: #fff;
    --color-tertiary: #ccc;
    --shadow-sm: 0 0 0 1px rgba(0, 0, 0, 0.05);
  }
  /* ... */
}
```

- `@layer utilities` - Styles that are scoped to a specific utility class. These styles are only applied to elements
  that have the utility class applied.

## Layer Order

The cascade layers are applied in the following order:

- `@layer utilities` (Highest priority)
- `@layer recipes`
- `@layer tokens`
- `@layer base`
- `@layer reset` (Lowest priority)

This means that styles defined in the `@layer utilities` will take precedence over styles defined in the
`@layer recipes`. This is useful when you want to override the default styles of a component.

## Layer CSS

The generated CSS in Panda is organized into layers. This allows you to define styles in a modular way, using CSS rules
that are scoped to specific components or elements.

Here's what the first line of the generated CSS looks like:

```css
@layer reset, base, tokens, recipes, utilities;
```

Adding this line to the top of your CSS file will determine the order in which the layers are applied. This is the most
exciting feature of CSS cascade layers.

## Customize layers

Panda lets you customize the cascade layers, so your project can coexist with other solutions. Learn more about customizing layers [here](/docs/references/config#layers).

## Polyfills

In event that you need to support older browsers, you can use the following postcss plugin in your PostCSS config:

- [postcss-cascade-layers](https://www.npmjs.com/package/@csstools/postcss-cascade-layers): Adds support for CSS Cascade Layers.

Here is an example of a `postcss.config.js` file that uses these polyfills:

```js
module.exports = {
  plugins: ["@pandacss/dev/postcss", "@csstools/postcss-cascade-layers"],
};
```

Since CSS `@layer`s have a lower priority than other CSS rules, this postcss plugin is also useful in cases where your styles are being overridden by some other stylesheets that you're not in total control of, since it will remove the `@layer` rules and still emulate their specificity.

---

## Color opacity modifier

How to dynamically set the opacity of a raw color or color token

Every utilities connected to the `colors` tokens in the `@pandacss/preset-base` (included by default) can use the
[`color-mix`](https://developer.mozilla.org/en-US/docs/Web/CSS/color_value/color-mix) CSS function. This means for
example: `background`, `backgroundColor`, `color`, `border`, `borderColor`, etc.

This function allows you to mix two colors together, and we use it to change the opacity of a color using the
`{color}/{opacity}` syntax.

You can use it like this:

```ts
css({
  bg: "red.300/40",
  color: "white",
});
```

This will generate:

```css
@layer utilities {
  .bg_red\.300\/40 {
    --mix-background: color-mix(
      in srgb,
      var(--colors-red-300) 40%,
      transparent
    );
    background: var(--mix-background, var(--colors-red-300));
  }

  .text_white {
    color: var(--colors-white);
  }
}
```

- If you're not using any opacity, the utility will **not** use `color-mix`
- The utility will automatically fallback to the original color if the `color-mix` function is not supported by the
  browser.
- You can use any of the color tokens, and any of the opacity tokens.

---

The `utilities` transform function also receives a `utils` object that contains the `colorMix` function, so you can also
use it on your own utilities:

```ts
export default defineConfig({
  utilities: {
    background: {
      shorthand: "bg",
      className: "bg",
      values: "colors",
      transform(value, args) {
        const mix = args.utils.colorMix(value);
        // This can happen if the value format is invalid (e.g. `bg: red.300/invalid` or `bg: red.300//10`)
        if (mix.invalid) return { background: value };

        return {
          background: mix.value,
        };
      },
    },
  },
});
```

---

Here's a cool snippet (that we use internally !) that makes it easier to create a utility transform for a given
property:

```ts
import type { PropertyTransform } from "@pandacss/types";
export const createColorMixTransform =
  (prop: string): PropertyTransform =>
  (value, args) => {
    const mix = args.utils.colorMix(value);
    if (mix.invalid) return { [prop]: value };
    const cssVar = "--mix-" + prop;
    return {
      [cssVar]: mix.value,
      [prop]: `var(${cssVar}, ${mix.color})`,
    };
  };
```

then the same utility transform as above can be written like this:

```ts
export default defineConfig({
  utilities: {
    background: {
      shorthand: "bg",
      className: "bg",
      values: "colors",
      transform: createColorMixTransform("background"),
  },
});
```

---

## Conditional Styles

Learn how to use conditional and responsive styles in Panda.

When writing styles, you might need to apply specific changes depending on a specific condition, whether it's based on
breakpoint, css pseudo state, media query or custom data attributes.

Panda allows you to write conditional styles, and provides common condition shortcuts to make your life easier. Let's
say you want to change the background color of a button when it's hovered. You can do it like this:

```jsx
<button
  className={css({
    bg: "red.500",
    _hover: { bg: "red.700" },
  })}
>
  Hover me
</button>
```

## Overview

### Property based condition

This works great, but might be a bit verbose. You can apply the condition `_hover` directly to the `bg` property,
leading to a more concise syntax:

```diff
<button
  className={css({
-   bg: 'red.500',
-   _hover: { bg: 'red.700' }
+   bg: { base: 'red.500', _hover: 'red.700' }
  })}
>
  Hover me
</button>
```

> Note: The `base` key is used to define the default value of the property, without any condition.

### Nested condition

Conditions in Panda can be nested, which means you can apply multiple conditions to a single property or another
condition.

Let's say you want to change the background color of a button when it's focused and hovered. You can do it like this:

```jsx
<button
  className={css({
    bg: { base: "red.500", _hover: { _focus: "red.700" } },
  })}
>
  Hover me
</button>
```

### Built-in conditions

Panda includes a set of common pseudo states that you can use to style your components:

- Pseudo Class: `_hover`, `_active`, `_focus`, `_focusVisible`, `_focusWithin`, `_disabled`
- Pseudo Element: `_before`, `_after`
- Media Query: `sm`, `md`, `lg`, `xl`, `2xl`
- Data Attribute Selector: `_horizontal`, `_vertical`, `_portrait`, `_landscape`

## Arbitrary selectors

What if you need a one-off selector that is not defined in your config's conditions? You can use the `css` function to
generate classes for arbitrary selectors:

```tsx
import { css } from "../styled-system/css";

const App = () => {
  return (
    <div
      className={css({
        "&[data-state=closed]": { color: "red.300" },
        "& > *": { margin: "2" },
      })}
    />
  );
};
```

This also works with the supported at-rules (`@media`, `@layer`, `@container`, `@supports`, and `@page`):

```tsx
import { css } from "../styled-system/css";

const App = () => {
  return (
    <div className={css({ display: "flex", containerType: "size" })}>
      <div
        className={css({
          "@media (min-width: 768px)": {
            color: "red.300",
          },
          "@container (min-width: 10px)": {
            color: "green.300",
          },
          "@supports (display: flex)": {
            fontSize: "3xl",
            color: "blue.300",
          },
        })}
      />
    </div>
  );
};
```

## Pseudo Classes

### Hover, Active, Focus, and Disabled

You can style the hover, active, focus, and disabled states of an element using their `_` modifier:

```jsx
<button
  className={css({
    bg: "red.500",
    _hover: { bg: "red.700" },
    _active: { bg: "red.900" },
  })}
>
  Hover me
</button>
```

### First, Last, Odd, Even

You can style the first, last, odd, and even elements of a group using their `_` modifier:

```jsx
<ul>
  {items.map((item) => (
    <li key={item} className={css({ _first: { color: "red.500" } })}>
      {item}
    </li>
  ))}
</ul>
```

You can also style even and odd elements using the `_even` and `_odd` modifier:

```jsx
<table>
  <tbody>
    {items.map((item) => (
      <tr
        key={item}
        className={css({
          _even: { bg: "gray.100" },
          _odd: { bg: "white" },
        })}
      >
        <td>{item}</td>
      </tr>
    ))}
  </tbody>
</table>
```

## Pseudo Elements

### Before and After

You can style the `::before` and `::after` pseudo elements of an element using their `_before` and `_after` modifier:

```jsx
<div
  className={css({
    _before: { content: '"👋"' },
  })}
>
  Hello
</div>
```

#### Notes

- **Before and After**: Ensure you wrap the content value in double quotes.
- **Mixing with Conditions**: When using condition and pseudo elements, prefer to place the condition **before** the
  pseudo element.

```jsx
css({
  // This works ✅
  _dark: { _backdrop: { color: 'red' } }
  // This doesn't work ❌
  _backdrop: { _dark: { color: 'red' } }
})
```

The reason `_backdrop: { _dark: { color: 'red' } }` doesn't work is because it generated an invalid CSS structure that
looks like:

```css
&::backdrop {
  &.dark,
  .dark & {
    color: red;
  }
}
```

### Placeholder

Style the placeholder text of any input or textarea using the `_placeholder` modifier:

```jsx
<input
  placeholder="Enter your name"
  className={css({
    _placeholder: { color: "gray.500" },
  })}
/>
```

### File Inputs

Style the file input button using the `_file` modifier:

```jsx
<input
  type="file"
  className={css({
    _file: { bg: "gray.500", px: "4", py: "2", marginEnd: "3" },
  })}
/>
```

## Media Queries

### Reduced Motion

Use the `_motionReduce` and `_motionSafe` modifiers to style an element based on the user's motion preference:

```jsx
<div
  className={css({
    _motionReduce: { transition: "none" },
    _motionSafe: { transition: "all 0.3s" },
  })}
>
  Hello
</div>
```

### Color Scheme

The `prefers-color-scheme` media feature is used to detect if the user has requested the system use a light or dark
color theme.

Use the `_osLight` and `_osDark` modifiers to style an element based on the user's color scheme preference:

```jsx
<div
  className={css({
    bg: "white",
    _osDark: { bg: "black" },
  })}
>
  Hello
</div>
```

Let's say your app is dark by default, but you want to allow users to switch to a light theme. You can do it like this:

```jsx
<div
  className={css({
    bg: "black",
    _osLight: { bg: "white" },
  })}
>
  Hello
</div>
```

### Color Contrast

The `prefers-contrast` media feature is used to detect if the user has requested the system use a high or low contrast
theme.

Use the `_highContrast` and `_lessContrast` modifiers to style an element based on the user's color contrast preference:

```jsx
<div
  className={css({
    bg: "white",
    _highContrast: { bg: "black" },
  })}
>
  Hello
</div>
```

### Orientation

The `orientation` media feature is used to detect if the user has a device in portrait or landscape mode.

Use the `_portrait` and `_landscape` modifiers to style an element based on the user's device orientation:

```jsx
<div
  className={css({
    pb: "4",
    _portrait: { pb: "8" },
  })}
>
  Hello
</div>
```

## Group Selectors

When you need to style an element based on its parent element's state or attribute, you can add the `group` class to the
parent element, and use any of the `_group*` modifiers on the child element.

```jsx
<div className="group">
  <p className={css({ _groupHover: { bg: "red.500" } })}>Hover me</p>
</div>
```

This modifer for every pseudo class modifiers like `_groupHover`, `_groupActive`, `_groupFocus`, and `_groupDisabled`,
etc.

## Sibling Selectors

When you need to style an element based on its sibling element's state or attribute, you can add the `peer` class to the
sibling element, and use any of the `_peer*` modifiers on the target element.

```jsx
<div>
  <p className="peer">Hover me</p>
  <p className={css({ _peerHover: { bg: "red.500" } })}>I'll change by bg</p>
</div>
```

> Note: This only works for when the element marked with `peer` is a previous siblings, that is, it comes before the
> element you want to start.

## Data Attribute

### LTR and RTL

You can style an element based on the direction of the text using the `_ltr` and `_rtl` modifiers:

```jsx
<div dir="ltr">
  <div
    className={css({
      _ltr: { ml: "3" },
      _rtl: { mr: "3" },
    })}
  >
    Hello
  </div>
</div>
```

For this to work, you need to set the `dir` attribute on the parent element. In most cases,you can set this on the
`html` element.

> **Note:** Consider using logical css properties like `marginInlineStart` and `marginInlineEnd` instead their physical
> counterparts like `marginLeft` and `marginRight`. This will reduce the need to use the `_ltr` and `_rtl` modifiers.

### State

You can style an element based on its `data-{state}` attribute using the corresponding `_{state}` modifier:

```jsx
<div
  data-loading
  className={css({
    _loading: { bg: "gray.500" },
  })}
>
  Hello
</div>
```

This also works for common states like `data-active`, `data-disabled`, `data-focus`, `data-hover`, `data-invalid`,
`data-required`, and `data-valid`.

```jsx
<div
  data-active
  className={css({
    _active: { bg: "gray.500" },
  })}
>
  Hello
</div>
```

> Most of the `data-{state}` attributes typically mirror the corresponding browser pseudo class. For example,
> `data-hover` is equivalent to `:hover`, `data-focus` is equivalent to `:focus`, and `data-active` is equivalent to
> `:active`.

### Orientation

You can style an element based on its `data-orientation` attribute using the `_horizontal` and `_vertical` modifiers:

```jsx
<div
  data-orientation="horizontal"
  className={css({
    _horizontal: { bg: "red.500" },
    _vertical: { bg: "blue.500" },
  })}
>
  Hello
</div>
```

## ARIA Attribute

You can style an element based on its `aria-{state}=true` attribute using the corresponding `_{state}` modifier:

```jsx
<div
  aria-expanded="true"
  className={css({
    _expanded: { bg: "gray.500" },
  })}
>
  Hello
</div>
```

> Most of the `aria-{state}` attributes typically mirror the support ARIA states in the browser pseudo class. For
> example, `aria-checked=true` is styled with `_checked`, `aria-disabled=true` is styled with `_disabled`.

## Container queries

You can define container names and sizes in your theme configuration and use them in your styles.

```ts
export default defineConfig({
  // ...
  theme: {
    extend: {
      containerNames: ["sidebar", "content"],
      containerSizes: {
        xs: "40em",
        sm: "60em",
        md: "80em",
      },
    },
  },
});
```

The default container sizes in the `@pandacss/preset-panda` preset are shown below:

```ts
export const containerSizes = {
  xs: "320px",
  sm: "384px",
  md: "448px",
  lg: "512px",
  xl: "576px",
  "2xl": "672px",
  "3xl": "768px",
  "4xl": "896px",
  "5xl": "1024px",
  "6xl": "1152px",
  "7xl": "1280px",
  "8xl": "1440px",
};
```

Then use them in your styles by referencing using `@<container-name>/<container-size>` syntax:

> The default container syntax is `@/<container-size>`.

```ts
import { css } from '/styled-system/css'

function Demo() {
  return (
    <nav className={css({ containerType: 'inline-size' })}>
      <div
        className={css({
          fontSize: { '@/sm': 'md' }
        })}
      />
    </nav>
  )
}
```

This will generate the following CSS:

```css
.cq-type_inline-size {
  container-type: inline-size;
}

@container (min-width: 60em) {
  .\@\/sm:fs_md {
    container-type: inline-size;
  }
}
```

You can also named container queries:

```ts
import { cq } from 'styled-system/patterns'

function Demo() {
  return (
    <nav className={cq({ name: 'sidebar' })}>
      <div
        className={css({
          fontSize: { base: 'lg', '@sidebar/sm': 'md' }
        })}
      />
    </nav>
  )
}
```

## Reference

Here's a list of all the condition shortcuts you can use in Panda:

| Condition name         | Selector                                                                                         |
| ---------------------- | ------------------------------------------------------------------------------------------------ |
| \_hover                | `&:is(:hover, [data-hover])`                                                                     |
| \_focus                | `&:is(:focus, [data-focus])`                                                                     |
| \_focusWithin          | `&:focus-within`                                                                                 |
| \_focusVisible         | `&:is(:focus-visible, [data-focus-visible])`                                                     |
| \_disabled             | `&:is(:disabled, [disabled], [data-disabled], [aria-disabled=true])`                             |
| \_active               | `&:is(:active, [data-active])`                                                                   |
| \_visited              | `&:visited`                                                                                      |
| \_target               | `&:target`                                                                                       |
| \_readOnly             | `&:is(:read-only, [data-read-only], [aria-readonly=true])`                                       |
| \_readWrite            | `&:read-write`                                                                                   |
| \_empty                | `&:is(:empty, [data-empty])`                                                                     |
| \_checked              | `&:is(:checked, [data-checked], [aria-checked=true], [data-state="checked"])`                    |
| \_enabled              | `&:enabled`                                                                                      |
| \_expanded             | `&:is([aria-expanded=true], [data-expanded], [data-state="expanded"])`                           |
| \_highlighted          | `&[data-highlighted]`                                                                            |
| \_complete             | `&[data-complete]`                                                                               |
| \_incomplete           | `&[data-incomplete]`                                                                             |
| \_dragging             | `&[data-dragging]`                                                                               |
| \_before               | `&::before`                                                                                      |
| \_after                | `&::after`                                                                                       |
| \_firstLetter          | `&::first-letter`                                                                                |
| \_firstLine            | `&::first-line`                                                                                  |
| \_marker               | `&::marker, &::-webkit-details-marker`                                                           |
| \_selection            | `&::selection`                                                                                   |
| \_file                 | `&::file-selector-button`                                                                        |
| \_backdrop             | `&::backdrop`                                                                                    |
| \_first                | `&:first-child`                                                                                  |
| \_last                 | `&:last-child`                                                                                   |
| \_only                 | `&:only-child`                                                                                   |
| \_even                 | `&:nth-child(even)`                                                                              |
| \_odd                  | `&:nth-child(odd)`                                                                               |
| \_firstOfType          | `&:first-of-type`                                                                                |
| \_lastOfType           | `&:last-of-type`                                                                                 |
| \_onlyOfType           | `&:only-of-type`                                                                                 |
| \_peerFocus            | `.peer:is(:focus, [data-focus]) ~ &`                                                             |
| \_peerHover            | `.peer:is(:hover, [data-hover]) ~ &`                                                             |
| \_peerActive           | `.peer:is(:active, [data-active]) ~ &`                                                           |
| \_peerFocusWithin      | `.peer:focus-within ~ &`                                                                         |
| \_peerFocusVisible     | `.peer:is(:focus-visible, [data-focus-visible]) ~ &`                                             |
| \_peerDisabled         | `.peer:is(:disabled, [disabled], [data-disabled], [aria-disabled=true]) ~ &`                     |
| \_peerChecked          | `.peer:is(:checked, [data-checked], [aria-checked=true], [data-state="checked"]) ~ &`            |
| \_peerInvalid          | `.peer:is(:invalid, [data-invalid], [aria-invalid=true]) ~ &`                                    |
| \_peerExpanded         | `.peer:is([aria-expanded=true], [data-expanded], [data-state="expanded"]) ~ &`                   |
| \_peerPlaceholderShown | `.peer:placeholder-shown ~ &`                                                                    |
| \_groupFocus           | `.group:is(:focus, [data-focus]) &`                                                              |
| \_groupHover           | `.group:is(:hover, [data-hover]) &`                                                              |
| \_groupActive          | `.group:is(:active, [data-active]) &`                                                            |
| \_groupFocusWithin     | `.group:focus-within &`                                                                          |
| \_groupFocusVisible    | `.group:is(:focus-visible, [data-focus-visible]) &`                                              |
| \_groupDisabled        | `.group:is(:disabled, [disabled], [data-disabled], [aria-disabled=true]) &`                      |
| \_groupChecked         | `.group:is(:checked, [data-checked], [aria-checked=true], [data-state="checked"]) &`             |
| \_groupExpanded        | `.group:is([aria-expanded=true], [data-expanded], [data-state="expanded"]) &`                    |
| \_groupInvalid         | `.group:is(:invalid, [data-invalid], [aria-invalid=true]) &`                                     |
| \_indeterminate        | `&:is(:indeterminate, [data-indeterminate], [aria-checked=mixed], [data-state="indeterminate"])` |
| \_required             | `&:is(:required, [data-required], [aria-required=true])`                                         |
| \_valid                | `&:is(:valid, [data-valid])`                                                                     |
| \_invalid              | `&:is(:invalid, [data-invalid], [aria-invalid=true])`                                            |
| \_autofill             | `&:autofill`                                                                                     |
| \_inRange              | `&:is(:in-range, [data-in-range])`                                                               |
| \_outOfRange           | `&:is(:out-of-range, [data-outside-range])`                                                      |
| \_placeholder          | `&::placeholder, &[data-placeholder]`                                                            |
| \_placeholderShown     | `&:is(:placeholder-shown, [data-placeholder-shown])`                                             |
| \_pressed              | `&:is([aria-pressed=true], [data-pressed])`                                                      |
| \_selected             | `&:is([aria-selected=true], [data-selected])`                                                    |
| \_grabbed              | `&:is([aria-grabbed=true], [data-grabbed])`                                                      |
| \_underValue           | `&[data-state=under-value]`                                                                      |
| \_overValue            | `&[data-state=over-value]`                                                                       |
| \_atValue              | `&[data-state=at-value]`                                                                         |
| \_default              | `&:default`                                                                                      |
| \_optional             | `&:optional`                                                                                     |
| \_open                 | `&:is([open], [data-open], [data-state="open"], :popover-open)`                                  |
| \_closed               | `&:is([closed], [data-closed], [data-state="closed"])`                                           |
| \_fullscreen           | `&:is(:fullscreen, [data-fullscreen])`                                                           |
| \_loading              | `&:is([data-loading], [aria-busy=true])`                                                         |
| \_hidden               | `&:is([hidden], [data-hidden])`                                                                  |
| \_current              | `&:is([aria-current=true], [data-current])`                                                      |
| \_currentPage          | `&[aria-current=page]`                                                                           |
| \_currentStep          | `&[aria-current=step]`                                                                           |
| \_today                | `&[data-today]`                                                                                  |
| \_unavailable          | `&[data-unavailable]`                                                                            |
| \_rangeStart           | `&[data-range-start]`                                                                            |
| \_rangeEnd             | `&[data-range-end]`                                                                              |
| \_now                  | `&[data-now]`                                                                                    |
| \_topmost              | `&[data-topmost]`                                                                                |
| \_motionReduce         | `@media (prefers-reduced-motion: reduce)`                                                        |
| \_motionSafe           | `@media (prefers-reduced-motion: no-preference)`                                                 |
| \_print                | `@media print`                                                                                   |
| \_landscape            | `@media (orientation: landscape)`                                                                |
| \_portrait             | `@media (orientation: portrait)`                                                                 |
| \_dark                 | `.dark &`                                                                                        |
| \_light                | `.light &`                                                                                       |
| \_osDark               | `@media (prefers-color-scheme: dark)`                                                            |
| \_osLight              | `@media (prefers-color-scheme: light)`                                                           |
| \_highContrast         | `@media (forced-colors: active)`                                                                 |
| \_lessContrast         | `@media (prefers-contrast: less)`                                                                |
| \_moreContrast         | `@media (prefers-contrast: more)`                                                                |
| \_ltr                  | `:where([dir=ltr], :dir(ltr)) &`                                                                 |
| \_rtl                  | `:where([dir=rtl], :dir(rtl)) &`                                                                 |
| \_scrollbar            | `&::-webkit-scrollbar`                                                                           |
| \_scrollbarThumb       | `&::-webkit-scrollbar-thumb`                                                                     |
| \_scrollbarTrack       | `&::-webkit-scrollbar-track`                                                                     |
| \_horizontal           | `&[data-orientation=horizontal]`                                                                 |
| \_vertical             | `&[data-orientation=vertical]`                                                                   |
| \_icon                 | `& :where(svg)`                                                                                  |
| \_starting             | `@starting-style`                                                                                |
| \_noscript             | `@media (scripting: none)`                                                                       |
| \_invertedColors       | `@media (inverted-colors: inverted)`                                                             |

## Custom conditions

Panda lets you create your own conditions, so you're not limited to the ones in the default preset. Learn more about
customizing conditions [here](/docs/customization/conditions).

---

## The extend keyword

What is and how to to use the extend keyword

The `extend` keyword allows you to extend the default Panda configuration. It is useful when you want to add your own
customizations to Panda, without erasing the default `presets` values (`conditions`, `tokens`, `utilities`, etc).

It will (deeply) merge your customizations with the default ones, instead of replacing them.

The `extend` keyword allows you to extend the following parts of Panda:

- [conditions](/docs/customization/conditions)
- [theme](/docs/customization/theme)
- [recipes](/docs/concepts/recipes) (included in theme)
- [patterns](/docs/customization/patterns)
- [utilities](/docs/customization/utilities)
- [globalCss](/docs/concepts/writing-styles#global-styles)
- [staticCss](/docs/guides/static)

> These keys are all allowed in [presets](/docs/customization/presets).

## Example

After running the `panda init` command you should see something similar to this:

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...

  // Useful for theme customization
  theme: {
    extend: {}, // 👈 it's already there! perfect, now you just need to add your customizations in this object
  },

  // ...
});
```

Let's say you want to add a new color to the default theme. You can do it like this:

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      colors: {
        primary: { value: "#ff0000" },
      },
    },
  },
});
```

This will add a new color to the default theme, without erasing the other ones.

Now, let's say we want to create new property `br` that applies a border radius to an element.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  utilities: {
    extend: {
      br: {
        className: "rounded", // css({ br: "sm" }) => rounded-sm
        values: "radii", // connect values to the radii tokens
        transform(value) {
          return { borderRadius: value };
        },
      },
    },
  },
});
```

What if this utility was coming from a preset (`@acme/my-preset`) ? You can extend any specific part, as it will be
deeply merged with the existing one:

```ts
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  presets: ['@acme/my-preset']
  utilities: {
    extend: {
      br: {
        className: 'br' // css({ br: "sm" }) => br-sm
      }
    }
  }
})
```

## Removing something from a preset

Let's say you want to remove the `br` utility from the `@acme/my-preset` preset. You can do it like this:

```ts
import { defineConfig } from '@pandacss/dev'
import myPreset from '@acme/my-preset'

const { br, ...utilities } = myPreset.utilities

export default defineConfig({
  presets: ['@acme/my-preset']
  utilities: {
    extend: {
      ...utilities, // 👈 we still want the other utilities from this preset
      // your customizations here
    }
  }
})
```

## Removing something from the base presets

Let's say you want to remove the `stack` pattern from the `@pandacss/preset-base` preset (included by default).

You can pick only the parts that you need with and spread the rest, like this:

```ts
import pandaBasePreset from "@pandacss/preset-base";

// omitting stack here
const { stack, ...pandaBasePresetPatterns } = pandaBasePreset.patterns;

export default defineConfig({
  presets: ["@pandacss/preset-panda"], // 👈 we still want the tokens, breakpoints and textStyles from this preset

  // ⚠️ we need to eject to prevent the `@pandacss/preset-base` from being resolved
  // https://panda-css.com/docs/customization/presets#which-panda-presets-will-be-included-
  eject: true,
  patterns: {
    extend: {
      ...pandaBasePresetPatterns,
      // your customizations here
    },
  },
});
```

## Minimal setup

If you want to use Panda with the bare minimum, without any of the defaults, you can read more about it
[here](/docs/guides/minimal-setup)

## FAQ

### Why is my preset overriding the base one, even after adding it to the array?

You might have forgotten to include the `extend` keyword in your config. Without `extend`, your preset will completely
replace the base one, instead of merging with it.

---

## Global Styles

How to work with resets, global styles, and global CSS variables in Panda.

Panda groups global styles into reset and base layers so you can control defaults predictably and override them safely.

## Layers overview

- **@layer reset**: Preflight/reset styles, enabled with `preflight`.
- **@layer base**: Your additional global styles via `globalCss`.

> See also: [Cascade layers](/docs/concepts/cascade-layers)

## Reset (preflight)

Enable or scope the reset styles.

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  preflight: true,
});
```

Scope and level:

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  preflight: { scope: ".extension", level: "element" },
});
```

## Exposed global CSS variables

These variables are used by the reset and defaults. Set them in `globalCss`:

- `--global-font-body`
- `--global-font-mono`
- `--global-color-border`
- `--global-color-placeholder`
- `--global-color-selection`
- `--global-color-focus-ring`

## Setting global styles (base)

Use `globalCss` to define additional global styles and set variables.

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  globalCss: {
    html: {
      "--global-font-body": "Inter, sans-serif",
      "--global-font-mono": "Mononoki Nerd Font, monospace",
      "--global-color-border": "colors.gray.400",
      "--global-color-placeholder": "rgba(0,0,0,0.5)",
      "--global-color-selection": "rgba(0,115,255,0.3)",
      "--global-color-focus-ring": "colors.blue.400",
    },
  },
});
```

### Theming patterns

You can set variables on `:root`, a `.dark` class, or via media queries.

```css
:root {
  --global-color-border: oklch(0.8 0 0);
}
.dark {
  --global-color-border: oklch(0.72 0 0);
}

@media (prefers-color-scheme: dark) {
  :root {
    --global-color-border: oklch(0.72 0 0);
  }
}
```

## Custom global variables (`globalVars`)

Define additional global CSS variables or `@property` entries.

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  globalVars: {
    "--button-color": {
      syntax: "<color>",
      inherits: false,
      initialValue: "blue",
    },
  },
});
```

> Keys from `globalVars` are suggestable in style objects and generated near your tokens at `cssVarRoot`.

## Troubleshooting

- **Global styles aren't applied:** Confirm `preflight` is enabled (if you expect reset), and ensure your selector
  (`html`, `:root`, `.dark`, etc.) matches the element where variables are set.

- **Global styles are overridden by utilities or component styles:** Verify layer order and specificity. Ensure
  `@layer reset` and `@layer base` are emitted before utilities. If you customize insertion or injection order (SSR,
  framework plugins), preserve `@layer` order so globals are not overridden.

---

## Panda Integration Hooks

Leveraging hooks in Panda to create custom functionality.

Panda hooks can be used to add new functionality or modify existing behavior during certian parts of the compiler
lifecycle.

Hooks are mostly callbacks that can be added to the panda config via the `hooks` property, or installed via `plugins`.

Here are some examples of what you can do with hooks:

- modify the resolved config (`config:resolved`), like strip out tokens or keyframes.
- modify presets after they are resolved (`preset:resolved`), like removing specific tokens or theme properties from a
  preset.
- tweak the design token or classname engine (`tokens:created`, `utility:created`), like prefixing token names, or
  customizing the hashing function
- transform a source file to a `tsx` friendly syntax before it's parsed (`parser:before`) so that Panda can
  automatically extract its styles usage
- create your own styles parser (`parser:before`, `parser:after`) using the file's content so that Panda could be used
  with any templating language
- alter the generated JS and DTS code (`codegen:prepare`)
- modify the generated CSS (`cssgen:done`), allowing all kinds of customizations like removing the unused CSS variables,
  etc.
- restrict `strictTokens` to a specific set of token categories, ex: only affect `colors` and `spacing` tokens and
  therefore allow any value for `fontSizes` and `lineHeights`

## Examples

### Prefixing token names

This is especially useful when migrating from other css-in-js libraries, [like Stitches.](/docs/migration/stitches)

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  hooks: {
    "tokens:created": ({ configure }) => {
      configure({
        formatTokenName: (path) => "$" + path.join("-"),
      });
    },
  },
});
```

### Customizing the hash function

When using the [`hash: true`](/docs/concepts/writing-styles) config property, you can customize the function used to
hash the classnames.

```ts
export default defineConfig({
  // ...
  hash: true,
  hooks: {
    "utility:created": ({ configure }) => {
      configure({
        toHash: (paths, toHash) => {
          const stringConds = paths.join(":");
          const splitConds = stringConds.split("_");
          const hashConds = splitConds.map(toHash);
          return hashConds.join("_");
        },
      });
    },
  },
});
```

### Modifying the config

Here's an example of how to leveraging the provided `utils` functions in the `config:resolved` hook to remove the
`stack` pattern from the resolved config.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  hooks: {
    "config:resolved": ({ config, utils }) => {
      return utils.omit(config, ["patterns.stack"]);
    },
  },
});
```

### Modifying presets

You can use the `preset:resolved` hook to modify presets after they are resolved. This is useful for customizing or
filtering out parts of a preset.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  hooks: {
    "preset:resolved": ({ utils, preset, name }) => {
      if (name === "@pandacss/preset-panda") {
        return utils.omit(preset, [
          "theme.tokens.colors",
          "theme.semanticTokens.colors",
        ]);
      }
      return preset;
    },
  },
});
```

### Configuring JSX extraction

Use the `matchTag` / `matchTagProp` functions to customize the way Panda extracts your JSX.

This can be especially useful when working with libraries that have properties that look like CSS properties but are not
and should be ignored.

Let's see a Radix UI example where the `Select.Content` component has a `position` property that should be ignored:

```js
// Here, the `position` property will be extracted because `position` is a valid CSS property, but we don't want that
<Select.Content position="popper" sideOffset={5}>
```

```ts
export default defineConfig({
  // ...
  hooks: {
    "parser:before": ({ configure }) => {
      configure({
        // ignore the Select.Content entirely
        matchTag: (tag) => tag !== "Select.Content",
        // ...or specifically ignore the `position` property
        matchTagProp: (tag, prop) =>
          tag === "Select.Content" && prop !== "position",
      });
    },
  },
});
```

### Remove unused variables from final css

Here's an example of how to transform the generated css in the `cssgen:done` hook.

```ts file="panda.config.ts"
import { defineConfig } from "@pandacss/dev";
import { removeUnusedCssVars } from "./remove-unused-css-vars";
import { removeUnusedKeyframes } from "./remove-unused-keyframes";

export default defineConfig({
  // ...
  hooks: {
    "cssgen:done": ({ artifact, content }) => {
      if (artifact === "styles.css") {
        return removeUnusedCssVars(removeUnusedKeyframes(content));
      }
    },
  },
});
```

Get the snippets for the removal logic from our Github Sandbox in the
[remove-unused-css-vars](https://github.com/chakra-ui/panda/blob/main/sandbox/vite-ts/remove-unused-css-vars.ts) and
[remove-unused-keyframes](https://github.com/chakra-ui/panda/blob/main/sandbox/vite-ts/remove-unused-keyframes.ts)
files.

> Note: Using this means you can't use the JS function [`token.var`](/docs/guides/dynamic-styling#using-tokenvar) (or
> [token(xxx)](/docs/guides/dynamic-styling#using-token) where `xxx` is the path to a
> [semanticToken](/docs/theming/tokens#semantic-tokens)) from `styled-system/tokens` as the CSS variables will be
> removed based on the usage found in the generated CSS

## Sharing hooks

Hooks can be shared as a snippet or as a `plugin`. Plugins are currently simple objects that contain a `name` associated
with a `hooks` object with the same structure as the `hooks` object in the config.

> Plugins differ from `presets` as they can't be extended, but they will be called in sequence in the order they are
> defined in the `plugins` array, with the user's config called last.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  plugins: [
    {
      name: "token-format",
      hooks: {
        "tokens:created": ({ configure }) => {
          configure({
            formatTokenName: (path) => "$" + path.join("-"),
          });
        },
      },
    },
  ],
});
```

## Reference

```ts
export interface PandaHooks {
  /**
   * Called when the config is resolved, after all the presets are loaded and merged.
   * This is the first hook called, you can use it to tweak the config before the context is created.
   */
  "config:resolved": (
    args: ConfigResolvedHookArgs,
  ) => MaybeAsyncReturn<void | ConfigResolvedHookArgs["config"]>;
  /**
   * Called when a preset is resolved, allowing you to modify it before it's merged into the config.
   */
  "preset:resolved": (
    args: PresetResolvedHookArgs,
  ) => MaybeAsyncReturn<void | PresetResolvedHookArgs["preset"]>;
  /**
   * Called when the token engine has been created
   */
  "tokens:created": (args: TokenCreatedHookArgs) => MaybeAsyncReturn;
  /**
   * Called when the classname engine has been created
   */
  "utility:created": (args: UtilityCreatedHookArgs) => MaybeAsyncReturn;
  /**
   * Called when the Panda context has been created and the API is ready to be used.
   */
  "context:created": (args: ContextCreatedHookArgs) => void;
  /**
   * Called when the config file or one of its dependencies (imports) has changed.
   */
  "config:change": (args: ConfigChangeHookArgs) => MaybeAsyncReturn;
  /**
   * Called after reading the file content but before parsing it.
   * You can use this hook to transform the file content to a tsx-friendly syntax so that Panda's parser can parse it.
   * You can also use this hook to parse the file's content on your side using a custom parser, in this case you don't have to return anything.
   */
  "parser:before": (args: ParserResultBeforeHookArgs) => string | void;
  /**
   * Called after the file styles are extracted and processed into the resulting ParserResult object.
   * You can also use this hook to add your own extraction results from your custom parser to the ParserResult object.
   */
  "parser:after": (args: ParserResultAfterHookArgs) => void;
  /**
   * Called right before writing the codegen files to disk.
   * You can use this hook to tweak the codegen files before they are written to disk.
   */
  "codegen:prepare": (
    args: CodegenPrepareHookArgs,
  ) => MaybeAsyncReturn<Artifact[]>;
  /**
   * Called after the codegen is completed
   */
  "codegen:done": (args: CodegenDoneHookArgs) => MaybeAsyncReturn;
  /**
   * Called right before adding the design-system CSS (global, static, preflight, tokens, keyframes) to the final CSS
   * Called right before writing/injecting the final CSS (styles.css) that contains the design-system CSS and the parser CSS
   * You can use it to tweak the CSS content before it's written to disk or injected through the postcss plugin.
   */
  "cssgen:done": (args: CssgenDoneHookArgs) => string | void;
}
```

---

## JSX Style Context

JSX Style Context provides an ergonomic way to style compound components with slot recipes.

It uses a context-based approach to distribute recipe styles across multiple child components, making it easier to style
headless UI libraries like Ark UI, and Radix UI.

## Atomic Slot Recipe

- Create a slot recipe using the `sva` function
- Pass the slot recipe to the `createStyleContext` function
- Use the `withProvider` and `withContext` functions to create compound components

```tsx
// components/ui/card.tsx

import { sva } from "styled-system/css";
import { createStyleContext } from "styled-system/jsx";

const card = sva({
  slots: ["root", "label"],
  base: {
    root: {},
    label: {},
  },
  variants: {
    size: {
      sm: { root: {} },
      md: { root: {} },
    },
  },
  defaultVariants: {
    size: "sm",
  },
});

const { withProvider, withContext } = createStyleContext(card);

const Root = withProvider("div", "root");
const Label = withContext("label", "label");

export const Card = {
  Root,
  Label,
};
```

Then you can use the `Root` and `Label` components to create a card.

```tsx
// app/page.tsx

import { Card } from "./components/ui/card";

export default function App() {
  return (
    <Card.Root>
      <Card.Label>Hello</Card.Label>
    </Card.Root>
  );
}
```

## Config Slot Recipe

The `createStyleContext` function can also be used with slot recipes defined in the `panda.config.ts` file.

- Pass the config recipe to the `createStyleContext` function
- Use the `withProvider` and `withContext` functions to create compound components

```tsx
// components/ui/card.tsx

import { card } from "../styled-system/recipes";
import { createStyleContext } from "styled-system/jsx";

const { withProvider, withContext } = createStyleContext(card);

const Root = withProvider("div", "root");
const Label = withContext("label", "label");

export const Card = {
  Root,
  Label,
};
```

Then you can use the `Root` and `Label` components to create a card.

```tsx
// app/page.tsx

import { Card } from "./components/ui/card";

export default function App() {
  return (
    <Card.Root>
      <Card.Label>Hello</Card.Label>
    </Card.Root>
  );
}
```

## createStyleContext

This function is a factory function that returns three functions: `withRootProvider`, `withProvider`, and `withContext`.

### withRootProvider

Creates the root component that provides the style context. Use this when the root component **does not render an
underlying DOM element**.

```tsx
import { Dialog } from "@ark-ui/react";

//...

const DialogRoot = withRootProvider(Dialog.Root);
```

### withProvider

Creates a component that both provides context and applies the root slot styles. Use this when the root component
**renders an underlying DOM element**.

> **Note:** It requires the root `slot` parameter to be passed.

```tsx
import { Avatar } from "@ark-ui/react";

//...

const AvatarRoot = withProvider(Avatar.Root, "root");
```

### withContext

Creates a component that consumes the style context and applies slot styles. It does not accept variant props directly,
but gets them from context.

```tsx
import { Avatar } from "@ark-ui/react";

//...

const AvatarImage = withContext(Avatar.Image, "image");
const AvatarFallback = withContext(Avatar.Fallback, "fallback");
```

### unstyled prop

Every component created with `createStyleContext` supports the `unstyled` prop to disable styling. It is useful when you
want to opt-out of the recipe styles.

- When applied the root component, will disable all styles
- When applied to a child component, will disable the styles for that specific slot

```tsx
// Removes all styles
<AvatarRoot unstyled>
  <AvatarImage />
  <AvatarFallback />
</AvatarRoot>

// Removes only the styles for the image slot
<AvatarRoot>
  <AvatarImage unstyled css={{ bg: 'red' }} />
  <AvatarFallback />
</AvatarRoot>
```

## Guides

### Config Recipes

The rules of config recipes still applies when using `createStyleContext`. Ensure the name of the final component
matches the name of the recipe.

> If you want to use a custom name, you can configure the recipe's `jsx` property in the `panda.config.ts` file.

```tsx
// recipe name is "card"
import { card } from "../styled-system/recipes";

const { withRootProvider, withContext } = createStyleContext(card);

const Root = withRootProvider("div");
const Header = withContext("header", "header");
const Body = withContext("body", "body");

// The final component name must be "Card"
export const Card = {
  Root,
  Header,
  Body,
};
```

### Default Props

Use `defaultProps` option to provide default props to the component.

```tsx
const { withContext } = createStyleContext(card);

export const CardHeader = withContext("header", "header", {
  defaultProps: {
    role: "banner",
  },
});
```

---

## Merging Styles

Learn how to merge multiple styles without conflicts.

## Merging `css` objects

You can merge multiple style objects together using the `css` function.

```js
import { css } from "styled-system/css";

const style1 = {
  bg: "red",
  color: "white",
};

const style2 = {
  bg: "blue",
};

const className = css(style1, style2); // => 'bg_blue text_white'
```

In some cases though, the style object might not be colocated in the same file as the component. In this case, you can
use the `css.raw` function to preserve the original style object.

> All `.raw(...)` signatures are identity functions that return the same value as the input, but serve as a hint to the
> compiler that the value is a style object.

```js
// style.js
import { css } from "styled-system/css";

export const style1 = css.raw({
  bg: "red",
  color: "white",
});

// component.js
import { css } from "styled-system/css";
import { style1 } from "./style.js";

const style2 = css.raw({
  bg: "blue",
});

const className = css(style1, style2); // => 'bg_blue text_white'
```

## Spreading `css.raw` objects

> **Added in v1.6.1**

You can also spread `css.raw` objects within style declarations. This is particularly useful for reusing styles in
nested selectors, conditions, and complex compositions:

### Child selectors

```js
import { css } from "styled-system/css";

const baseStyles = css.raw({ margin: 0, padding: 0 });

const component = css({
  "& p": { ...baseStyles, fontSize: "1rem" },
  "& h1": { ...baseStyles, fontSize: "2rem" },
});
```

### Nested conditions

```js
import { css } from "styled-system/css";

const interactive = css.raw({ cursor: "pointer", transition: "all 0.2s" });

const card = css({
  _hover: {
    ...interactive,
    _dark: { ...interactive, color: "white" },
  },
});
```

## Merging `cva` + `css` styles

The same technique can be used to merge an atomic `cva` recipe and a style object.

```js
import { css, cx, cva } from "styled-system/css";

const overrideStyles = css.raw({
  bg: "red",
  color: "white",
});

const buttonStyles = cva({
  base: {
    bg: "blue",
    border: "1px solid black",
  },
  variants: {
    size: {
      small: { fontSize: "12px" },
    },
  },
});

const className = css(
  // returns the resolved style object
  buttonStyles.raw({ size: "small" }),
  // add the override styles
  overrideStyles,
);

// => 'bg_red border_1px_solid_black color_white font-size_12px'
```

## Merging `sva` + `css` styles

The same technique can be used to merge an atomic `sva` recipe and a style object.

```js
import { css, sva } from 'styled-system/css'

const overrideStyles = css.raw({
  bg: 'red',
  color: 'white'
})

const buttonStyles = sva({
  slots: ['root']
  base: {
    root: {
      bg: 'blue',
      border: '1px solid black'
    }
  },
  variants: {
    size: {
      root: {
        small: { fontSize: '12px' }
      }
    }
  }
})

// returns the resolved style object for all slots
const { root } = buttonStyles.raw({ size: 'small' })

const className = css(
  root,
  // add the override styles
  overrideStyles
)

// => 'bg_red border_1px_solid_black color_white font-size_12px'
```

## Merging config recipe and style object

Due to the fact that the generated styles of a config recipe are saved in the `@layer recipe` cascade layer, they can be
overridden with any atomic styles. Use the `cx` function to achieve that.

> The `utilties` layer has more precedence than the `recipe` layer.

```js
import { css, cx } from "styled-system/css";
import { button } from "styled-system/recipes";

const className = cx(
  // returns the resolved class name: `button button--size-small`
  button({ size: "small" }),
  // add the override styles
  css({ bg: "red" }), // => 'bg_red'
);

// => 'button button--size-small bg_red'
```

## Merging within JSX component

Using these techniques, you can apply them to a component by exposing a `css` prop and merge with local styles.

> **Note:** For this to work, Panda requires that you set `jsxFramework` config option to `react`

```jsx
const cardStyles = css.raw({
  bg: "red",
  color: "white",
});

function Card({ title, description, css: cssProp }) {
  return (
    // merge the `cardStyles` with the `cssProp` passed in
    <div className={css(cardStyles, cssProp)}>
      <h1>{title}</h1>
      <p>{description}</p>
    </div>
  );
}

// usage
function Demo() {
  return (
    <Card
      title="Hello World"
      description="This is a card component"
      css={{ bg: "blue" }}
    />
  );
}
```

If you use any other prop name other than `css`, then you must use the `css.raw(...)` function to ensure Panda extracts
the style object.

```jsx
const cardStyles = css.raw({
  bg: "red",
  color: "white",
});

function Card({ title, description, style }) {
  return (
    // merge the `cardStyles` with the `style` passed in
    <div className={css(cardStyles, style)}>
      <h1>{title}</h1>
      <p>{description}</p>
    </div>
  );
}

// usage
function Demo() {
  return (
    <Card
      title="Hello World"
      description="This is a card component"
      // use `css.raw(...)` to ensure Panda extracts the style object
      style={css.raw({ bg: "blue" })}
    />
  );
}
```

---
