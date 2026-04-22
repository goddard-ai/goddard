# JSX Style Props and Writing Styles

Use this reference for JSX style props, the styled-system runtime, template literals, virtual colors, and authored style composition.

## Style props

Build UIs quickly by passing css properties as "props" to your components.

While you can get very far by using the `className` prop and function from Panda, style props provide a more ergonomic
way of expressing styles.

Panda will extract the style props through static analysis and generate the CSS at build time.

> If you use Chakra UI, Styled System, or Theme UI, you'll feel right at home right away 😊

```jsx
import { css } from "../styled-system/css";
import { styled } from "../styled-system/jsx";

// The className approach
const Button = ({ children }) => (
  <button
    className={css({
      bg: "blue.500",
      color: "white",
      py: "2",
      px: "4",
      rounded: "md",
    })}
  >
    {children}
  </button>
);

// The style props approach
const Button = ({ children }) => (
  <styled.button bg="blue.500" color="white" py="2" px="4" rounded="md">
    {children}
  </styled.button>
);
```

## Configure JSX

Using JSX style props is turned off by default in Panda, but you can opt-in to this feature by using the `jsxFramework`
property in the panda config.

> ⚠️ Panda will not extract style props from JSX elements if you don't set the `jsxFramework` property. This is to avoid
> unnecessary work for projects that don't use JSX.

### Choose Framework

JSX is a JavaScript syntax extension that allows you to write HTML-like code directly within your JavaScript code and is
supported by most popular frameworks. Panda supports JSX style props in React, Preact, Vue 3, Qwik and Solid.js.

```js filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  jsxFramework: "react",
});
```

### Generate JSX runtime

Next, you need to run `panda codegen` to generate the JSX runtime for your framework.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
{/_ <!-- prettier-ignore-start --> _/}
<Tab>

```bash
pnpm panda codegen --clean
```

  </Tab>
  <Tab>
  ```bash
  npm panda codegen --clean
  ```
  </Tab>
  <Tab>
  ```bash
  yarn panda codegen --clean
  ```
  </Tab>
  <Tab>
  ```bash
  bun panda codegen --clean
  ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

That's it! You can now use JSX style props in your components.

## Using Style Props

### JSX Element

Style props are just CSS properties that you can pass to your components as props. With the JSX runtime, you can use
`styled.<element>` syntax to create supercharged JSX elements that support style props.

```jsx
import { styled } from "../styled-system/jsx";

const Button = ({ children }) => (
  <styled.button bg="blue.500" color="white" py="2" px="4" rounded="md">
    {children}
  </styled.button>
);
```

### Property Renaming

<Callout type="warning">Due to the static nature of Panda, you can't rename properties at runtime.</Callout>

```tsx filename="App.tsx"
import { Circle, CircleProps } from "../styled-system/jsx";

type Props = {
  circleSize?: CircleProps["size"];
};

const CustomCircle = (props: Props) => {
  const { circleSize = "3" } = props;
  return (
    <Circle
      // ❌ Avoid: Panda can't know that you're mapping `circleSize` to `size`
      size={circleSize}
    />
  );
};

// ...

const App = () => {
  return (
    // In this case, you should keep the `size` naming
    <CustomCircle circleSize="4" />
  );
};
```

The same principles apply to all style props, recipe variants, and pattern props.

<Callout type="info">
  If you still need to rename properties at runtime, you can use `config.staticCss` as an escape-hatch to pre-generate
  the CSS anyway for the properties you need.
</Callout>

### Recipe

You can use recipe variants as JSX props to quickly change the styles of your components, as long as
[you're tracking those components in your recipe config](/docs/concepts/recipes#advanced-jsx-tracking).

```tsx
import { styled } from "../styled-system/jsx";
import { button, type ButtonVariantProps } from "../styled-system/recipes";

const Button = (props: ButtonVariantProps) => (
  <button className={button(props)}>Button</button>
);

const App = () => <Button variant="secondary">Button</Button>;
```

## Factory Function

You can also use the `styled` function to create a styled component from any component or JSX intrinsic element (like
"a", "button").

```jsx
import { styled } from "../styled-system/jsx";
import { Button } from "component-library";

const StyledButton = styled(Button);

const App = () => (
  <StyledButton bg="blue.500" color="white" py="2" px="4" rounded="md">
    Button
  </StyledButton>
);
```

> You can configure the `styled` function name using the [`config.jsxFactory`](/docs/references/config#jsxfactory)
> option.

### Factory Recipe

You can define a recipe for your component using the `styled` function. This is useful when you want to create a
component that has a specific set of style props.

```jsx
import { styled } from "../styled-system/jsx";

const Button = styled("button", {
  base: {
    py: "2",
    px: "4",
    rounded: "md",
  },
  variants: {
    variant: {
      primary: {
        bg: "blue.500",
        color: "white",
      },
      secondary: {
        bg: "gray.500",
        color: "white",
      },
    },
  },
});

const App = () => (
  <Button variant="secondary" mt="10px">
    Button
  </Button>
);
```

### Factory Options

There's a few options you can pass to the `styled` function to customize the behavior of the generated component.

```ts
interface FactoryOptions<TProps extends Dict> {
  dataAttr?: boolean;
  defaultProps?: TProps;
  shouldForwardProp?(prop: string, variantKeys: string[]): boolean;
}
```

#### `dataAttr`

Setting `dataAttr` to true will add a `data-recipe="{recipeName}"` attribute to the element with the recipe name. This
is useful for testing and debugging.

```jsx
import { styled } from "../styled-system/jsx";
import { button } from "../styled-system/recipes";

const Button = styled("button", button, { dataAttr: true });

const App = () => <Button variant="secondary">Button</Button>;
// => <button data-recipe="button" class="btn btn--variant_purple">Button</button>
```

#### `defaultProps`

allows you to skip writing wrapper components just to set a few props. It also allows you to locall override the default
variants or base styles of a recipe.

```jsx
import { styled } from "../styled-system/jsx";
import { button } from "../styled-system/recipes";

const Button = styled("button", button, {
  defaultProps: {
    variant: "secondary",
    px: "10px",
  },
});

const App = () => <Button>Button</Button>;
// => <button class="btn btn--variant_secondary px_10px">Button</button>
```

#### `shouldForwardProp`

Used to customize which props are forwarded to the underlying element. By default, all props except recipe variants and
style props are forwarded.

For example, you could use it to integrate with [Framer Motion](https://www.framer.com/motion/).

```jsx
import { styled } from "../styled-system/jsx";
import { button } from "../styled-system/recipes";
import { motion, isValidMotionProp } from "framer-motion";

const StyledMotion = styled(
  motion.div,
  {},
  {
    shouldForwardProp: (prop, variantKeys) =>
      isValidMotionProp(prop) ||
      (!variantKeys.includes(prop) && !isCssProperty(prop)),
  },
);
```

### Unstyled prop

All styled components accept an `unstyled` prop that allows you to disable the recipe styles. This is useful when you
want to use a component's structure but apply completely custom styling.

```jsx
import { styled } from "../styled-system/jsx";
import { button } from "../styled-system/recipes";

const Button = styled("button", button);

const App = () => (
  <>
    {/* With recipe styles */}
    <Button>Styled Button</Button>

    {/* Without recipe styles, but inline styles still work */}
    <Button unstyled bg="red.500" color="white">
      Unstyled Button
    </Button>
  </>
);
```

### Reducing the allowed style props

You can reduce the allowed JSX properties on the factory using
[`config.jsxStyleProps`](/docs/references/config#jsxstyleprops):

- When set to 'all', all style props are allowed.
- When set to 'minimal', only the `css` prop is allowed.
- When set to 'none', no style props are allowed and therefore the `jsxFactory` will not be usable as a component:
  - `<styled.div />` and `styled("div")` aren't valid
  - but the recipe usage is still valid `styled("div", { base: { color: "red.300" }, variants: { ...} })`

> Removing style props (from `all` to either `minimal` or `none`) will reduce the size of the generated code due to not
> having to check which props are style props at runtime.

## JSX Patterns

Patterns are common layout patterns like `stack`, `grid`, `circle` that can be used to speed up your css. Think of them
as a way to avoid repetitive layout styles.

All the patterns provided by Panda are available as JSX components.

> Learn more about the [patterns](/docs/customization/patterns) we provide.

```jsx
import { Stack, Circle } from "../styled-system/jsx";

const App = () => (
  <Stack gap="4" align="flex-start">
    <button>Button</button>
    <Circle size="4" bg="red.300">
      4
    </Circle>
  </Stack>
);
```

## Making your own styled components

To make a custom JSX component that accepts style props, Use the `splitCssProps` function to split style props from
other component props.

> For this to work correctly, set the `jsxFramework` to the framework you're using in your panda config.

```tsx
import { splitCssProps } from "../styled-system/jsx";
import type { HTMLStyledProps } from "../styled-system/types";

export function Component(props: HTMLStyledProps<"div">) {
  const [cssProps, restProps] = splitCssProps(props);
  const { css: cssProp, ...styleProps } = cssProps;

  const className = css(
    { display: "flex", height: "20", width: "20" },
    styleProps,
    cssProp,
  );

  return <div {...restProps} className={className} />;
}

// Usage
function App() {
  return <Component w="2">Click me</Component>;
}
```

## TypeScript

Panda provides type definitions for all the style props that are supported by the JSX runtime.

You can use these types to get type safety in your components.

### Style Object

Use the `JsxStyleProps` to get the types of the style object that is compatible with JSX elements.

```tsx
import { styled } from '../styled-system/jsx'
import type { JsxStyleProps } from '../styled-system/types'

interface ButtonProps {
  color?: JsxStyleProps['color']
}

const Button = (props: ButtonProps) => {
  return <styled.button {...props}>
}
```

### Style Props

Use the `HTMLStyledProps` type to get the types of an element in addition to the style props.

```tsx {2}
import { styled } from '../styled-system/jsx'
import type { HTMLStyledProps } from '../styled-system/jsx'

type ButtonProps = HTMLStyledProps<'button'>

const Button = (props: ButtonProps) => {
  return <styled.button {...props}>
}
```

### Variant Props

Use the `StyledVariantProps` type to extract the variants from a styled component.

```tsx {2}
import { styled } from "../styled-system/jsx";
import type { StyledVariantProps } from "../styled-system/jsx";

const Button = styled("button", {
  base: { color: "black" },
  variants: {
    state: {
      error: { color: "red" },
      success: { color: "green" },
    },
  },
});

type ButtonVariantProps = StyledVariantProps<typeof Button>;
//   ^ { state?: 'error' | 'success' | undefined }
```

### Patterns

Every pattern provided by Panda has a corresponding type that you can use to get type safety in your components.

```tsx {2}
import { Stack } from "../styled-system/jsx";
import type { StackProps } from "../styled-system/jsx";
```

---

## Styled System

What is the styled-system folder and how does it work?

While Panda generates your CSS at **build-time** using static extraction, we still need a lightweight runtime to
transform the CSS-in-JS syntax (either [`object`](/docs/concepts/writing-styles#atomic-styles) or
[`template-literal`](/docs/concepts/template-literals)) to class names strings. This is where the `styled-system` folder
comes in.

When running the `panda` or `panda codegen` commands, the [`config.outdir`](/docs/references/config#outdir) will be used
as output path to generate the `styled-system` in.

This is the core of what the `styled-system` does:

```ts
css({ color: "blue.300" }); // => "text_blue_300"
```

Since Panda doesn't rely on any bundler's (`vite`, `webpack`, etc) plugin, there is no code transformation happening to
convert the CSS-in-JS syntax to class names at compile-time. This is why we need a lightweight runtime to do that.

The same principles applies to `patterns`, `recipes` and even `jsx` components, as they all use the `css` function under
the hood.

If you look inside your `styled-system` folder, you should see the main entrypoints for the runtime:

<FileTree>
  <FileTree.Folder name="styled-system" defaultOpen>
    <FileTree.Folder name="css" />
    <FileTree.Folder name="jsx" />
    <FileTree.Folder name="recipes" />
    <FileTree.Folder name="patterns" />
    <FileTree.Folder name="tokens" />
    <FileTree.Folder name="types" />
    <FileTree.File name="styles.css" />
  </FileTree.Folder>
</FileTree>

Feel free to explore the files inside the `styled-system` folder to get a better understanding of how it works in
details!

> Note: The `styled-system` folder is not meant to be edited manually. It is generated by Panda and should be treated as
> a build artifact. This also means you don't need to commit it to your repository.

## How does it work?

When running the `panda` command or with the postcss plugin, here's what's happening under the hood:

1. **Load Panda context**:

- Find and evaluate app config, merge result with presets.
- Create panda context: prepare code generator from config, parse user's file as AST.

2. **Generating artifacts**:

- Write lightweight JS runtime and types to output directory

3. **Extracting used styles in app code**:

- Run parser on each user's file: identify and extract styles, compute CSS, write to styles.css.

That `2. Generating artifacts` step is where the `styled-system` folder is generated, using the resolved config that
contains all your tokens, patterns, recipes, utilities etc. We generate a tailored runtime for your app, so that it only
contains enough code (and types!) to support the styles you're using.

## Pre-rendering

If you use some way to pre-render your components to static HTML, for example using Astro or RSC, the `styled-system`
functions like `css` and others will be removed at build-time and replaced by the generated class names, so that you
don't have to ship the runtime to your users.

---

## Template Literals

Panda allows you to write styles using template literals.

Writing styles using template literals provides a similar experience to
[styled-components](https://styled-components.com/) and [emotion](https://emotion.sh/), except that Panda generates
atomic class names instead of a single unique class name.

> Emitting atomic class names allows Panda to generate smaller CSS bundles.

Panda provides two functions to write template literal styles: `css` and `styled`.

## Getting started

To use template literals, you need to set the `syntax` option in your `panda.config.ts` file to `templateLiteral`:

```ts
// panda.config.ts
export default defineConfig({
  // ...
  syntax: "template-literal", // required
  jsxFramework: "react", // required for JSX utilities, e.g. `styled`
});
```

Then run the codegen command to generate the functions:

```sh
panda codegen --clean
```

## The `css` function

This the basic way of writing template styles. It converts the template literal into a set of atomic class name which
you can pass to the `className` prop of an element.

```js
import { css } from "../styled-system/css";

const className = css`
  font-size: 16px;
  font-weight: bold;
`;

function Heading() {
  return <h1 className={className}>This is a title</h1>;
}

// => <h1 className='font-size_16px font-weight_bold'></h1>
```

Here's what the emitted atomic CSS looks like:

```css
.font-size_16px {
  font-size: 16px;
}

.font-weight_bold {
  font-weight: bold;
}
```

## The `styled` tag

The `styled` tag allows you to create a component with encapsulated styles. It's similar to the `styled-components` or
`emotion` library.

```js
import { styled } from "../styled-system/jsx";

// Create a styled component
const Heading = styled.h1`
  font-size: 16px;
  font-weight: bold;
`;

function Demo() {
  // Use the styled component
  return <Heading>This is a title</Heading>;
}

// => <h1 class='font-size_16px font-weight_bold'>This is a title</h1>
```

Here's what the emitted atomic CSS looks like:

```css
.font-size_16px {
  font-size: 16px;
}

.font-weight_bold {
  font-weight: bold;
}
```

## Nested styles

You can nest selectors, pseudo-elements and pseudo-selectors.

```js
const Button = styled.button`
  color: black;

  &:hover {
    color: blue;
  }
`;
```

Using css nesting syntax, pseudo-elements, pseudo-selectors and combinators are also supported:

```js
const Demo = styled.div`
  color: black;

  &::after {
    content: "🐼";
  }

  & + & {
    background: yellow;
  }

  &.bordered {
    border: 1px solid black;
  }

  .parent & {
    color: red;
  }
`;
```

Nested media and container queries are also supported:

```js
const Demo = styled.div`
  color: black;

  @media (min-width: 200px) {
    color: blue;
  }

  @container (min-width: 200px) {
    color: red;
  }
`;
```

## Hashing class names

In some cases, it might be useful to shorten the class names by hashing them. Set the `hash: true` option in your
`panda.config.ts` file to enable this. This will generate shorter class names but will make it harder to debug.

To achieve this, set the `hash` option in your `panda.config.ts` file to `true`:

```ts
// panda.config.ts

export default defineConfig({
  // ...
  hash: true, // optional
});
```

> Run the `codegen` command to regenerate the functions with hashing enabled.

When hashing is enabled, the class names will go from this:

```css
.font-size_16px {
  font-size: 16px;
}

.font-weight_bold {
  font-weight: bold;
}
```

To a unique six character hash regardless of the length of the selector or the number of declarations:

```css
.adfg5r {
  font-size: 16px;
}

.bsdf35 {
  font-weight: bold;
}
```

## Using tokens

Use the `token()` function or `{}` syntax in your template literals to reference design tokens in your styles. Panda
will automatically generate the corresponding CSS variables.

```js
import { css } from "../styled-system/css";

const className = css`
  font-size: {fontSizes.md};
  font-weight: token(fontWeights.bold, 700);
`;
```

## Caveats

The object literal syntax is the recommended way of writing styles. But, if you stick with the template literal syntax,
there are some caveats to be aware of:

- Patterns and recipes are not generated
- Dynamic interpolation or component targeting is not supported
- Lack of autocompletion for tokens within the template literal Our
  [Eslint plugin](https://github.com/chakra-ui/eslint-plugin-panda/blob/main/docs/rules/no-invalid-token-paths.md) can
  help you overcome this by detecting invalid tokens
- JSX Style props are not supported

---

## Virtual Color

Panda allows you to create a virtual color or color placeholder in your project.

The `colorPalette` property is how you create virtual colors.

```js
import { css } from "../styled-system/css";

const className = css({
  colorPalette: "blue",
  bg: "colorPalette.100",
  _hover: {
    bg: "colorPalette.200",
  },
});
```

This will translate to the `blue.100` background color and `blue.200` background color on hover.

Virtual colors are useful when creating easily customizable components.

## Using with recipes

You can also use virtual colors with recipes.

```js
import { css, cva, cx } from "../styled-system/css";

const button = cva({
  base: {
    padding: 4,
    // you can also specify a default colorPalette in the `base` recipe key
    // colorPalette: 'blue',
    // ^^^^^^^^^^^^^^^^^^^^
  },
  variants: {
    variant: {
      primary: { color: "colorPalette.500" },
    },
  },
  defaultVariants: { variant: "primary" },
});
```

## Using with different color modes

You can also use virtual colors with different conditions, such as color modes.

```js
import { css, cva, cx } from "../styled-system/css";

const someButton = cva({
  base: { padding: 4 },
  variants: {
    variant: {
      primary: {
        bg: { base: "colorPalette.500", _dark: "colorPalette.200" },
        color: { base: "white", _dark: "gray.900" },
      },
    },
  },
  defaultVariants: { variant: "primary" },
});

export const App = () => {
  return (
    <>
      <div className="light">
        <button className={cx(css({ colorPalette: "blue" }), someButton())}>
          Click me
        </button>
        <button className={cx(css({ colorPalette: "green" }), someButton())}>
          Click me
        </button>
        <button className={cx(css({ colorPalette: "red" }), someButton())}>
          Click me
        </button>
      </div>
      <div className="dark">
        <button className={cx(css({ colorPalette: "blue" }), someButton())}>
          Click me
        </button>
        <button className={cx(css({ colorPalette: "green" }), someButton())}>
          Click me
        </button>
        <button className={cx(css({ colorPalette: "red" }), someButton())}>
          Click me
        </button>
      </div>
    </>
  );
};
```

## Semantic Virtual Colors

Semantic virtual colors gives you an ability to create a virtual color organized by category, variant and state.
Hierarchically organized virtual colors are useful when creating easily customizable components.

```js
const theme = {
  extend: {
    semanticTokens: {
      colors: {
        button: {
          dark: {
            value: "navy",
          },
          light: {
            DEFAULT: {
              value: "skyblue",
            },
            accent: {
              DEFAULT: {
                value: "cyan",
              },
              secondary: {
                value: "blue",
              },
            },
          },
        },
      },
    },
  },
};
```

You can now use the root `button` color palette and its values directly:

```tsx
import { css } from "../styled-system/css";

export const App = () => {
  return (
    <button
      className={css({
        colorPalette: "button",
        color: "colorPalette.light",
        backgroundColor: "colorPalette.dark",
        _hover: {
          color: "colorPalette.light.accent",
          background: "colorPalette.light.accent.secondary",
        },
      })}
    >
      Root color palette
    </button>
  );
};
```

Or you can use any deeply nested property (e.g. `button.light.accent`) as a root color palette:

```tsx
import { css } from "../styled-system/css";

export const App = () => {
  return (
    <button
      className={css({
        colorPalette: "button.light.accent",
        color: "colorPalette.secondary",
      })}
    >
      Nested color palette leaf
    </button>
  );
};
```

> **Note**: Nested tokens require glob patterns in the `colorPalette` config (e.g., `'button.*'`) to generate proper CSS
> variables.

## Pregenerated Virtual Colors

Use the `staticCss` option in the config to pre-generate values for the `colorPalette` property.

This is useful when you want to use a color palette that can be changed at runtime (e.g. in Storybook knobs).

> Learn more about [static css generation](/docs/guides/static).

```tsx
export default defineConfig({
  staticCss: {
    css: [
      {
        properties: { colorPalette: ["red", "blue"] },
      },
    ],
  },
});
```

Then in your code, you can design components that use the `colorPalette` property:

```tsx
import { css } from "../styled-system/css";

function ButtonShowcase() {
  const [colorPalette, setColorPalette] = useState("red");
  return (
    <div>
      <select
        value={colorPalette}
        onChange={(e) => setColorPalette(e.currentTarget.value)}
      >
        <option value="red">Red</option>
        <option value="blue">Blue</option>
      </select>

      <button
        className={css({
          bg: "colorPalette.50",
          color: "colorPalette.500",
          colorPalette,
        })}
      >
        Click me
      </button>
    </div>
  );
}
```

## Configuration

By default, color palette generation is enabled and includes all colors defined in your theme.

You can control which colors are used to generate color palettes by configuring the `colorPalette` property in your
theme.

### Disable Color Palette

To completely disable color palette generation, set `enabled` to `false`:

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    colorPalette: {
      enabled: false,
    },
  },
});
```

### Include Specific Colors

To generate color palettes for only specific colors, use the `include` option:

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    colorPalette: {
      include: ["gray", "blue", "red"],
    },
  },
});
```

This will only generate color palettes for `gray`, `blue`, and `red` colors, even if you have other colors defined in
your theme.

**Glob patterns** are supported for nested tokens:

```ts filename="panda.config.ts"
colorPalette: {
  include: ["gray.*", "blue.*"]; // Includes all nested tokens
}
```

### Exclude Specific Colors

To exclude certain colors from color palette generation, use the `exclude` option:

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    colorPalette: {
      exclude: ["yellow", "orange"],
    },
  },
});
```

This will generate color palettes for all colors except `yellow` and `orange`.

### Combination of Options

You can combine the `enabled`, `include`, and `exclude` options as needed:

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    colorPalette: {
      enabled: true,
      include: ["gray", "blue", "red", "green"],
      exclude: ["red"], // This will override the include for 'red'
    },
  },
});
```

In this example, color palettes will be generated for `gray`, `blue`, and `green`, but not for `red` (since it's
excluded).

## Nested Semantic Tokens

If nested tokens show as raw paths (e.g., `colors.base.accent`) instead of CSS variables, use glob patterns:

```ts filename="panda.config.ts"
semanticTokens: {
  colors: {
    button: {
      primary: {
        DEFAULT: { value: '{colors.blue.500}' },
        hover: { value: '{colors.blue.600}' }
      }
    }
  }
},
colorPalette: {
  include: ['button.*']  // Include all nested paths
}
```

Usage:

```tsx
className={css({
  colorPalette: 'button.primary',
  bg: 'colorPalette',
  _hover: { bg: 'colorPalette.hover' }
})}
```

---

## Writing Styles

Panda generates the utilities you need to style your components with confidence.

Using the object syntax is a fundamental approach to writing styles in Panda. It not only provides a type-safe style
authoring experience, but also improves readability and ensures a consistent experience with style overrides.

## Atomic Styles

When you write styles in Panda, it generates a modern atomic stylesheet that is automatically scoped to the
`@layer utilities` cascade layer.

The atomic stylesheets approach offers several advantages, such as improved code maintainability and reusability, as
well as a smaller overall CSS footprint.

Panda exposes a `css` function that can be used to author styles. It accepts a style object and returns a className
string.

```jsx
import { css } from '../styled-system/css'

const styles = css({
  backgroundColor: 'gainsboro',
  borderRadius: '9999px',
  fontSize: '13px',
  padding: '10px 15px'
})

// Generated className:
// --> bg_gainsboro rounded_9999px fs_13px p_10px_15px

<div className={styles}>
  <p>Hello World</p>
</div>
```

The styles generated at build time end up like this:

```css
@layer utilities {
  .bg_gainsboro {
    background-color: gainsboro;
  }

  .rounded_9999px {
    border-radius: 9999px;
  }

  .fs_13px {
    font-size: 13px;
  }

  .p_10px_15px {
    padding: 10px 15px;
  }
}
```

### Shorthand Properties

Panda provides shorthands for common css properties to help improve the speed of development and reduce the visual
density of your style declarations.

Properties like `borderRadius`, `backgroundColor`, and `padding` can be swapped to their shorthand equivalent `rounded`,
`bg`, and `p`.

```jsx
import { css } from "../styled-system/css";

// BEFORE - Good
const styles = css({
  backgroundColor: "gainsboro",
  borderRadius: "9999px",
  fontSize: "13px",
  padding: "10px 15px",
});

// AFTER - Better
const styles = css({
  bg: "gainsboro",
  rounded: "9999px",
  fontSize: "13px",
  p: "10px 15px",
});
```

> Shorthands are documented alongside their respective properties in the [utilities](/docs/utilities/background)
> section.

### Type safety

Panda is built with TypeScript and provides type safety for all style properties and shorthands. Most of the style
properties are connected to either the native CSS properties or their respective token value defined as defined in the
`theme` object.

```ts
import { css } from "../styled-system/css";

//                       ⤵ you'll get autocomplete for colors
const styles = css({ bg: "|" });
```

> You can also enable the `strictTokens: true` setting in the Panda configuration. This allows only token values and
> prevents the use of custom or raw CSS values.

- `config.strictTokens` will only affect properties that have config tokens, such as `color`, `bg`, `borderColor`, etc.
- `config.strictPropertyValues` will throw for properties that do not have config tokens, such as `display`, `content`,
  `willChange`, etc. when the value is not a predefined CSS value.

> In both cases, you can use the `[xxx]` escape-hatch syntax to use custom or raw CSS values without TypeScript errors.

#### strictTokens

With `config.strictTokens` enabled, you can only use token values in your styles. This prevents the use of custom or raw
CSS values.

```ts filename="panda.config.ts"
import { css } from "../styled-system/css";

css({ bg: "red" }); // ❌ Error: "red" is not a valid token value
css({ fontSize: "123px" }); // ❌ Error: "123px" is not a valid token value

css({ bg: "red.400" }); // ✅ Valid
css({ fontSize: "[123px]" }); // ✅ Valid, since `[123px]` is using the escape-hatch syntax
css({ content: "abc" }); // ✅ Valid, since `content` isn't bound to a config token
```

For one-off styles, you can always use the escape-hatch syntax `[xxx]` to use custom or raw CSS values without
TypeScript errors.

```ts filename="panda.config.ts"
import { css } from "../styled-system/css";

css({ bg: "[red]" }); // ✅ Valid, since `[red]` is using the escape-hatch syntax
css({ fontSize: "[123px]" }); // ✅ Valid, since `[123px]` is using the escape-hatch syntax
```

#### strictPropertyValues

With `config.strictPropertyValues` enabled, you can only use valid CSS values for properties that do have a predefined
list of values in your styles. This prevents the use of custom or raw CSS values.

```ts filename="panda.config.ts"
css({ display: "flex" }); // ✅ Valid
css({ display: "block" }); // ✅ Valid

css({ display: "abc" }); // ❌ will throw since 'abc' is not part of predefined values of 'display'
css({ pos: "absolute123" }); // ❌ will throw since 'absolute123' is not part of predefined values of 'position'
css({ display: "[var(--btn-display)]" }); // ✅ Valid, since `[var(--btn-display)]` is using the escape-hatch syntax

css({ content: '""' }); // ✅ Valid, since `content` does not have a predefined list of values
css({ flex: "0 1" }); // ✅ Valid, since `flex` does not have a predefined list of values
```

The `config.strictPropertyValues` option will only be applied to this exhaustive list of properties:

```ts
type StrictableProps =
  | "alignContent"
  | "alignItems"
  | "alignSelf"
  | "all"
  | "animationComposition"
  | "animationDirection"
  | "animationFillMode"
  | "appearance"
  | "backfaceVisibility"
  | "backgroundAttachment"
  | "backgroundClip"
  | "borderCollapse"
  | "border"
  | "borderBlock"
  | "borderBlockEnd"
  | "borderBlockStart"
  | "borderBottom"
  | "borderInline"
  | "borderInlineEnd"
  | "borderInlineStart"
  | "borderLeft"
  | "borderRight"
  | "borderTop"
  | "borderBlockEndStyle"
  | "borderBlockStartStyle"
  | "borderBlockStyle"
  | "borderBottomStyle"
  | "borderInlineEndStyle"
  | "borderInlineStartStyle"
  | "borderInlineStyle"
  | "borderLeftStyle"
  | "borderRightStyle"
  | "borderTopStyle"
  | "boxDecorationBreak"
  | "boxSizing"
  | "breakAfter"
  | "breakBefore"
  | "breakInside"
  | "captionSide"
  | "clear"
  | "columnFill"
  | "columnRuleStyle"
  | "contentVisibility"
  | "direction"
  | "display"
  | "emptyCells"
  | "flexDirection"
  | "flexWrap"
  | "float"
  | "fontKerning"
  | "forcedColorAdjust"
  | "isolation"
  | "lineBreak"
  | "mixBlendMode"
  | "objectFit"
  | "outlineStyle"
  | "overflow"
  | "overflowX"
  | "overflowY"
  | "overflowBlock"
  | "overflowInline"
  | "overflowWrap"
  | "pointerEvents"
  | "position"
  | "resize"
  | "scrollBehavior"
  | "touchAction"
  | "transformBox"
  | "transformStyle"
  | "userSelect"
  | "visibility"
  | "wordBreak"
  | "writingMode";
```

## Nested Styles

Panda provides different ways of nesting style declarations. You can use the native css nesting syntax, or the built-in
pseudo props like `_hover` and `_focus`. Pseudo props are covered more in-depth in the next section.

### Native CSS Nesting

Panda supports the native css nesting syntax. You can use the `&` selector to create nested styles.

> **Important:** It is required to use the "&" character when nesting styles.

```jsx
<div
  className={css({
    bg: "red.400",
    "&:hover": {
      bg: "orange.400",
    },
  })}
/>
```

You can also target children and siblings using the `&` syntax.

```jsx
<div
  className={css({
    bg: "red.400",
    "& span": {
      color: "pink.400",
    },
  })}
/>
```

We recommend not using descendant selectors as they can lead to specificity issues when managing style overrides.
Colocating styles directly on the element is the preferred way of writing styles in Panda.

### Using Pseudo Props

Panda provides a set of pseudo props that can be used to create nested styles. The pseudo props are prefixed with an
underscore `_` to avoid conflicts with the native pseudo selectors.

For example, to create a hover style, you can use the `_hover` pseudo prop.

```jsx
<div
  className={css({
    bg: "red.400",
    _hover: {
      bg: "orange.400",
    },
  })}
/>
```

> See the [pseudo props](/docs/concepts/conditional-styles#reference) section for a list of all available pseudo props.

## Global styles

Global styles are useful for applying additional global resets or font faces. Use the `globalCss` property in the
`panda.config.ts` file to define global styles.

Global styles are inserted at the top of the stylesheet and are scoped to the `@layer base` cascade layer.

> For resets, global variables, theming patterns, and more examples, see [Global styles](/docs/concepts/global-styles).

```js filename="panda.config.ts"
import { defineConfig, defineGlobalStyles } from "@pandacss/dev";

const globalCss = defineGlobalStyles({
  "html, body": {
    color: "gray.900",
    lineHeight: "1.5",
  },
});

export default defineConfig({
  // ...
  globalCss,
});
```

The styles generated at build time will look like this:

```css
@layer base {
  html,
  body {
    color: var(--colors-gray-900);
    line-height: 1.5;
  }
}
```

## Style Composition

### Merging styles

Passing multiple styles to the `css` function will deeply merge the styles, allowing you to override styles in a
predictable way.

```jsx
import { css } from "../styled-system/css";

const result = css({ mx: "3", paddingTop: "4" }, { mx: "10", pt: "6" });
//    ^? result = "mx_10 pt_6"
```

To design a component that supports style overrides, you can provide the `css` prop as a style object, and it'll be
merged correctly.

```tsx filename="src/components/Button.tsx"
import { css } from "../styled-system/css";

export const Button = ({ css: cssProp = {}, children }) => {
  const className = css(
    { display: "flex", alignItems: "center", color: "black" },
    cssProp,
  );
  return <button className={className}>{children}</button>;
};
```

Then you can use the `Button` component like this:

```tsx filename="src/app/page.tsx"
import { Button } from "./Button";

export default function Page() {
  return (
    <Button css={{ color: "pink", _hover: { color: "red" } }}>
      will result in `class="d_flex items_center text_pink hover:text_red"`
    </Button>
  );
}
```

---

You can use this approach as well with the `{cvaFn}.raw`, `{svaFn.raw}` and `{patternFn}.raw` functions, allowing style
objects to be merged as expected in any situation.

**Pattern Example:**

```tsx filename="src/components/Button.tsx"
import { hstack } from "../styled-system/patterns";
import { css } from "../styled-system/css";

export const Button = ({ css: cssProp = {}, children }) => {
  // using the flex pattern
  const hstackProps = hstack.raw({
    border: "1px solid",
    _hover: { color: "blue.400" },
  });

  // merging the styles
  const className = css(hstackProps, cssProp);

  return <button className={className}>{children}</button>;
};
```

**CVA Example:**

```tsx filename="src/components/Button.tsx"
import { css, cva } from "../styled-system/css";

const buttonRecipe = cva({
  base: { display: "flex", fontSize: "lg" },
  variants: {
    variant: {
      primary: { color: "white", backgroundColor: "blue.500" },
    },
  },
});

export const Button = ({ css: cssProp = {}, children }) => {
  const className = css(
    // using the button recipe
    buttonRecipe.raw({ variant: "primary" }),

    // adding style overrides (internal)
    { _hover: { color: "blue.400" } },

    // adding style overrides (external)
    cssProp,
  );

  return <button className={className}>{children}</button>;
};
```

**SVA Example:**

```tsx filename="src/components/Button.tsx"
import { css, sva } from "../styled-system/css";

const checkbox = sva({
  slots: ["root", "control", "label"],
  base: {
    root: { display: "flex", alignItems: "center", gap: "2" },
    control: { borderWidth: "1px", borderRadius: "sm" },
    label: { marginStart: "2" },
  },
  variants: {
    size: {
      sm: {
        control: { width: "8", height: "8" },
        label: { fontSize: "sm" },
      },
      md: {
        control: { width: "10", height: "10" },
        label: { fontSize: "md" },
      },
    },
  },
  defaultVariants: {
    size: "sm",
  },
});

export const Checkbox = ({ rootProps, controlProps, labelProps }) => {
  // using the checkbox recipe
  const slotStyles = checkbox.raw({ size: "md" });

  return (
    <label className={css(slotStyles.root, rootProps)}>
      <input type="checkbox" className={css({ srOnly: true })} />
      <div className={css(slotStyles.control, controlProps)} />
      <span className={css(slotStyles.label, labelProps)}>Checkbox Label</span>
    </label>
  );
};

// Usage

const App = () => {
  return (
    <Checkbox
      rootProps={css.raw({ gap: 4 })}
      controlProps={css.raw({ borderColor: "yellow.400" })}
      labelProps={css.raw({ fontSize: "lg" })}
    />
  );
};
```

### Classname concatenation

Panda provides a simple `cx` function to join classnames. It accepts a list of classnames and returns a string.

```jsx
import { css, cx } from "../styled-system/css";

const styles = css({
  borderWidth: "1px",
  borderRadius: "8px",
  paddingX: "12px",
  paddingY: "24px",
});

const Card = ({ className, ...props }) => {
  const rootClassName = cx("group", styles, className);
  return <div className={rootClassName} {...props} />;
};
```

### Hashing

When debugging or previewing DOM elements in the browser, the length of the generated atomic `className` can get quite
long, and a bit annoying. If you prefer to have terser classnames, use the `hash` option to enable className and css
variable name hashing.

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  hash: true,
});
```

> You might need to generate a new code artifact by running `panda codegen --clean`

When you write a style like this:

```jsx
import { css } from "../styled-system/css";

const styles = css({
  display: "flex",
  flexDirection: "row",
  _hover: {
    bg: "red.50",
  },
});
```

The hash generated css will look like:

```css
.fPSBzf {
  display: flex;
}

.ksWBqx {
  flex-direction: row;
}

.btpEVp:is(:hover, [data-hover]) {
  background: var(--bINrJX);
}
```

> We recommend that you use this in production builds only, as it can make debugging a bit harder.

## Important styles

Applying important styles works just like CSS

```js
css({
  color: "red !important",
});
```

You can also apply important using just the exclamation syntax `!`

```js
css({
  color: "red!",
});
```

## TypeScript

Use the `SystemStyleObject` type if you want to type your styles.

```ts {2}
import { css } from "../styled-system/css";
import type { SystemStyleObject } from "../styled-system/types";

const styles: SystemStyleObject = {
  color: "red",
};
```

## Property conflicts

When you combine shorthand and longhand properties, Panda will resolve the styles in a predictable way. The shorthand
property will take precedence over the longhand property.

```jsx
import { css } from '../styled-system/css'

const styles = css({
  paddingTop: '20px'
  padding: "10px",
})
```

The styles generated at build time will look like this:

```css
@layer utilities {
  .p_10px {
    padding: 10px;
  }

  .pt_20px {
    padding-top: 20px;
  }
}
```

## Global vars

You can use the `globalVars` property to define global
[CSS variables](https://developer.mozilla.org/en-US/docs/Web/CSS/--*) or custom CSS
[`@property`](https://developer.mozilla.org/en-US/docs/Web/CSS/@property) definitions.

Panda will automatically generate the corresponding CSS variables and suggest them in your style objects.

> They will be generated in the [`cssVarRoot`](/docs/references/config#cssvarroot) near your tokens.

This can be especially useful when using a 3rd party library that provides custom CSS variables, like a popper library
that exposes a `--popper-reference-width`.

```ts filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  globalVars: {
    "--popper-reference-width": "4px",
    // you can also generate a CSS @property
    "--button-color": {
      syntax: "<color>",
      inherits: false,
      initialValue: "blue",
    },
  },
});
```

> Note: Keys defined in `globalVars` will be available as a value for _every_ utilities, as they're not bound to token
> categories.

```ts
import { css } from "../styled-system/css";

const className = css({
  "--button-color": "colors.red.300",
  // ^^^^^^^^^^^^  will be suggested

  backgroundColor: "var(--button-color)",
  //                ^^^^^^^^^^^^^^^^^^  will be suggested
});
```

---
