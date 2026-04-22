# Migration

Use this reference when migrating code from Stitches, Styled Components, or Theme UI.

## Migrating from Stitches

Migrate your project from Stitches to Panda.

This guide helps you migrate from Stitches to Panda and understand the design differences between the libraries.

> **Disclaimer:** This isn't about comparing which one is best. Panda and Stitches are two different CSS-in-JS solutions
> with design decisions.

Here are some similarities between the two libraries.

- Panda uses the object literal syntax to define styles. It also supports the shorthand syntax for the `margin` and
  `padding` properties.
- Panda supports the `variants`, `defaultVariants` and `compoundVariants` APIs.
- Panda supports design tokens and themes.
- Panda supports all the variants of nested selectors (attribute, class, pseudo, descendant, child, sibling selectors
  and more). It also requires the use of the `&` to chain selectors.

Below are some of the differences between the two libraries.

## css function

In Stitches, the `css` function is used to author both regular style objects and variant style objects.

```tsx
import { css } from '@stitches/react'

// definition
const styles = css({
  border: 'solid 1px red',
  backgroundColor: 'transparent',

  variants: {
    variant: {
      // ...
    }
  }
})

// usage
<button className={styles({ variant: 'primary' })} />
```

In Panda, the `css` function is only used to author atomic styles, and the `cva` function to create variant style
objects.

**The css function**

```tsx
import { css } from '../styled-system/css'

// definition
const styles = css({
  border: 'solid 1px red',
  backgroundColor: 'transparent'
})

// usage
<button className={styles} />
```

**The cva function**

```tsx
import { cva } from '../styled-system/css'

// definition
const styles = cva({
  base: {
    border: 'solid 1px red',
    backgroundColor: 'transparent'
  },
  variants: {
    variant: {
      // ...
    }
  }
})

// usage
<button className={styles({ variant: 'primary' })} />
```

## styled function

In Stitches, the `styled` function can be used to create components that are bound to both regular and variant styles
objects.

```tsx
import { styled } from "@stitches/react";

const Button = styled("button", {
  // base styles
  backgroundColor: "gainsboro",
  borderRadius: "9999px",

  variants: {
    // variant styles
  },
});
```

In Panda, the base styles object needs to added to the `base` key.

```tsx
import { styled } from "../styled-system/jsx";

const Button = styled("button", {
  // base styles
  base: {
    backgroundColor: "gainsboro",
    borderRadius: "9999px",
  },
  variants: {
    // variant styles
  },
});
```

In Stitches, the styled function generates a unique className for each variant.

```tsx
import { styled } from "@stitches/react";

const Button = styled("button", {});
// => <button class="c-coNKBW c-coNKBW-dnSdJM-variant-primary">Button</button>
```

In Panda, you can decide if you want unique classNames for the recipe or you want atomic classNames.

- **Atomic classes** using the `cva` function or defining the recipe inline in the `styled` function

```tsx
import { styled } from "../styled-system/jsx";

const Button = styled("button", {
  base: {
    backgroundColor: "gainsboro",
    borderRadius: "9999px",
  },
});
// => <button class="bg_gainsboro rounded_999px">Button</button>
```

- **Selector classes** by defining the recipe in the `panda.config.ts` file. This approach only generates the classes
  and css for the variants that are used in the project.

```ts
import { defineConfig, defineRecipe } from "@pandacss/dev";

const buttonStyle = defineRecipe({
  className: "button",
  base: {
    backgroundColor: "gainsboro",
    borderRadius: "9999px",
  },
  variants: {
    // variant styles
  },
});

export default defineConfig({
  theme: {
    extend: {
      recipes: {
        buttonStyle,
      },
    },
  },
});
```

> You might need to run `panda codegen --clean` to generate the recipe functions.

```tsx
import { styled } from '../styled-system/jsx'
import { buttonStyle } from '../styled-system/recipes'

// create a styled component using the recipe function
const Button = styled('button', buttonStyle)

// or you can use directly in the JSX
<button className={buttonStyle({ variant: 'primary' })}>Button</button>

// => <button className="button button--variant-primary">Button</button>
```

## Responsive Styles

In Stitches, you configure breakpoints in the `media` key of the `createStitches` method, and use it via the
`@<breakpoint>` syntax.

```ts
import { createStitches } from "@stitches/react";

// configure
const { styled, css } = createStitches({
  media: {
    bp1: "(min-width: 640px)",
    bp2: "(min-width: 768px)",
  },
});

// usage
const styles = css({
  backgroundColor: "gainsboro",
  "@bp1": {
    backgroundColor: "tomato",
  },
});
```

In Panda, you configure breakpoints in the `theme.breakpoints` key of the `panda.config` function

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      breakpoints: {
        bp1: "640px",
        bp2: "768px",
      },
    },
  },
});

// usage
import { css } from "../styled-system/css";

const styles = css({
  bg: "gainsboro",
  bp1: { bg: "tomato" },
  // or
  margin: { base: "10px", bp1: "20px" },
});
```

In Stitches, you use the `@initial` keyword to target the base styles.

In Panda, you use the `base` key to target the base styles.

## Tokens and Theme

### Tokens

In Stitches, tokens are defined in the `theme` key of the `createStitches` method.

```ts
import { createStitches } from "@stitches/react";

const { styled, css } = createStitches({
  theme: {
    colors: {
      gray100: "hsl(206,22%,99%)",
      gray200: "hsl(206,12%,97%)",
    },
  },
  space: {},
  fonts: {},
});

// usage
const styles = css({
  backgroundColor: "$gray100",
});
```

In Panda, tokens are defined in the `theme` key of the `panda.config` function.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    tokens: {
      colors: {
        gray100: { value: "hsl(206,22%,99%)" },
        gray200: { value: "hsl(206,12%,97%)" },
      },
      spacing: {},
      fonts: {},
    },
    semanticTokens: {
      // ...
    },
  },
});

// usage
import { css } from "../styled-system/css";

const styles = css({
  backgroundColor: "gray100",
});
```

Notice that in Panda, you don't need to use the `$` prefix to access the tokens. If you really want to use the `$`
prefix, you can either update the name of the token:

```diff
export default defineConfig({
  theme: {
    colors: {
-      gray100: { value: 'hsl(206,22%,99%)' },
+      $gray100: { value: 'hsl(206,22%,99%)' },
    },
  }
})
```

Or you can tweak the token engine to format them with the `$` prefix:

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

### Themes

In Stitches, the `createTheme` function is used to define dark theme values.

```tsx
import { createStitches } from '@stitches/react'

const { createTheme } = createStitches({})

// create theme
const darkTheme = createTheme({
  colors: {
    gray100: 'hsl(206,8%,12%)',
    gray200: 'hsl(206,7%,14%)'
  }
})

// apply theme
<div className={darkTheme}>
  <div>Content nested in dark theme.</div>
</div>
```

In Panda, themes are designed as semantic tokens. You can define the semantic tokens in the `semanticTokens` key of the
`panda.config` function.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    semanticTokens: {
      colors: {
        gray100: {
          value: { base: "hsl(206,22%,99%)", _dark: "hsl(206,8%,12%)" },
        },
        gray200: {
          value: { base: "hsl(206,12%,97%)", _dark: "hsl(206,7%,14%)" },
        },
      },
    },
  },
});
```

### Token Aliases

In Stitches, you can create locally scoped tokens using the `$$` prefix

```ts
import { styled } from "@stitches/react";

const Button = styled("button", {
  $$shadowColor: "$colors$pink500",
  boxShadow: "0 0 0 15px $$shadowColor",
});
```

In Panda, there's no special syntax, you need to use the css variable syntax. CSS variables are able to query the theme
tokens directly using dot notation

```ts
import { styled } from "../styled-system/jsx";

const Button = styled("button", {
  base: {
    "--shadowColor": "colors.pink500",
    boxShadow: "0 0 0 15px var(--shadowColor)",
  },
});
```

## Animations

In Stitches, you can define keyframes using the `keyframes` method.

```ts
import { keyframes, styled } from "@stitches/react";

const scaleUp = keyframes({
  "0%": { transform: "scale(1)" },
  "100%": { transform: "scale(1.5)" },
});

// usage
const Button = styled("button", {
  "&:hover": {
    animation: `${scaleUp} 200ms`,
  },
});
```

In Panda, you define keyframes in the `theme.keyframes` key of the `panda.config` function.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      keyframes: {
        scaleUp: {
          "0%": { transform: "scale(1)" },
          "100%": { transform: "scale(1.5)" },
        },
      },
    },
  },
});

// usage
import { css } from "../styled-system/css";

const style = css({
  "&:hover": {
    animation: "scaleUp 200ms",
  },
});
```

## Utils

In Stitches, you can define utilities by using the `utils` key in the `createStitches` method.

```ts
import { createStitches, type PropertyValue } from "@stitches/react";

const { styled, css } = createStitches({
  utils: {
    linearGradient: (value: PropertyValue<"backgroundImage">) => ({
      backgroundImage: `linear-gradient(${value})`,
    }),
  },
});
```

In Panda, you get a lot of built-in utilities (like mx, marginX, my, py, etc.) that you can use out of the box. You can
also create custom utilites using the `utilities` key in the `panda.config` function.

The utilities API allows you define the connected token scale, generated className, and transform function.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  utilities: {
    extend: {
      linearGradient: {
        // (optional): the css property this maps to (to inherit the types from)
        property: "backgroundImage",
        // (optional): the className to generate
        className: "bg_gradient",
        // (optional): the shorthand name to use in the css
        shorthand: "gradient",
        // (required): maps the value to the raw css object
        transform: (value) => ({
          backgroundImage: `linear-gradient(${value})`,
        }),
      },
    },
  },
});
```

> Running `panda codegen` will update the typings for the utilities, allowing for a type-safe developer experience.

Then you can use the utility in your styles.

```tsx
import { css } from "../styled-system/css";

const buttonClass = css({
  linearGradient: "19deg, #21D4FD 0%, #B721FF 100%",
});
```

## Global Styles

In Stitches, you define the global styles using the `globalCss` function, and then call it in your app.

```tsx
import { globalCss } from "@stitches/react";

const globalStyles = globalCss({
  "*": { margin: 0, padding: 0 },
});

// then in your app
globalStyles();
```

In Panda, you define the global styles in the `panda.config.ts` using the `globalCss` function.

> The styles be injected automatically under the `base` cascade layer via PostCSS

```ts {3-5}
import { defineConfig, defineGlobalStyles } from "@pandacss/dev";

const globalCss = defineGlobalStyles({
  "*": { margin: 0, padding: 0 },
});

export default defineConfig({
  // ...
  globalCss,
});
```

## Targeting Components

In Stitches, you can directly target React or styled components via the `toString()` method.

```tsx
import { css } from '@stitches/react'

const Icon = () => (
  <svg className="right-arrow" ... />
);

// add a `toString` method
Icon.toString = () => '.right-arrow';

const buttonClass = css({
  [`& ${Icon}`]: {
    marginLeft: '5px'
  }
})
```

In Panda, you need to use the native selector directly. This is largely due to the static nature of Panda

```tsx
import { css } from '../styled-system/css'

const Icon = () => (
  <svg className="right-arrow" ... />
);


const buttonClass = css({
  "& .right-arrow": {
    marginLeft: '5px'
  }
})
```

## Server Side Rendering

In Stitches, you need to configure the server-side rendering for your framework.

```tsx
// stitches.config.ts
import { createStitches } from "@stitches/react";
export const { getCssText } = createStitches();

// _document.tsx
export default class Document extends NextDocument {
  render() {
    return (
      <Html lang="en">
        <Head>
          <style
            id="stitches"
            dangerouslySetInnerHTML={{ __html: getCssText() }}
          />
        </Head>
        <body>
          <Main />
          <NextScript />
        </body>
      </Html>
    );
  }
}
```

In Panda, you don't need to configure anything. Panda automatically extracts the styles and injects them at build time
using PostCSS.

## Conclusion

Before choosing your preferred CSS-in-JS library, be sure to consider your engineering and design goals. Both Stitches
and Panda are capable of achieving many of the same styling goals, but they have different approaches.

---

## Migrating from Styled Components

Migrate your project from Styled Components to Panda.

This guide outlines the steps needed to migrate your project from Styled Components to Panda and highlights key design
differences between the two libraries.

> **Disclaimer:** This isn't about comparing which one is best. Panda and Styled Components are two different CSS-in-JS
> solutions with design decisions.

Here are some similarities between the two libraries.

- Both libraries support the use of tagged template literals or object syntax to style components.
- Both libraries provide a way to define design tokens (variables) and use them in your styles.
- Both libraries require the use of `&` for nested selectors.

Below are some differences between the two libraries.

## Installation and Syntax

In styled-components, you can use both tagged template literals and object syntax to style components.

In Panda, you need to decide which syntax you want to use. Panda recommends using the object syntax, but provides a way
to opt-in to tagged template literals.

To initialize a project with the object syntax, run the following command.

```bash
panda init -p --jsx-framework react
```

To initialize a project with the tagged template literal syntax, run the following command.

```bash
panda init -p --syntax template-literal --jsx-framework react
```

Then you need to add the cascade layers to the global styles of your project.

```css
@layer reset, base, tokens, recipes, utilities;
```

## Tagged Template Syntax

In styled-components, the recommended way to style components is to use tagged template literals.

```jsx
import styled from "styled-components";

const Button = styled.button`
  background-color: #fff;
  border: 1px solid #000;
  color: #000;
  padding: 0.5rem 1rem;
`;
```

In Panda, you will use the autogenerate code in the `styled-system` directory at the root of your project.

> Remember to initialize your project with the `--syntax template-literal` flag or update the panda.config.ts file.

```jsx
import { styled } from "../styled-system/jsx";

const Button = styled.button`
  background-color: #fff;
  border: 1px solid #000;
  color: #000;
  padding: 0.5rem 1rem;
`;
```

## Object Syntax

In styled-components, you can use the object syntax to style components.

```jsx
import styled from "styled-components";

const Button = styled.button({
  backgroundColor: "#fff",
  border: "1px solid #000",
  color: "#000",
  padding: "0.5rem 1rem",
});
```

In Panda, you add the style object to the `base` key of the style object. The `styled` factory allows you define base
styles, variants and compound variants of your component.

```jsx
import { styled } from "../styled-system/jsx";

const Button = styled("button", {
  base: {
    backgroundColor: "#fff",
    border: "1px solid #000",
    color: "#000",
    padding: "0.5rem 1rem",
  },
});
```

## Prop Interpolation

In styled-components, you can interpolate the component's props to conditionally set styles.

```jsx
const Button = styled.button`
  ${(props) =>
    props.color === "violet" &&
    `
    background-color: 'blueviolet'
  `}

  ${(props) =>
    props.color === "gray" &&
    `
    background-color: 'gainsboro'
  `}
`;
```

In Panda, we model interpolations using the variants API. This allows define style groups or recipes that can be applied
to components.

````jsx
const Button = styled('button', {
  variants: {
    color: {
      violet: css`
        background-color: blueviolet;
      `,
      gray: css`
        background-color: gainsboro;
      `
    }
  }
})

// Usage
<Button color="violet">Button</Button>
``` -->

## Tokens and Themes

### Defining Tokens

In styled-components, you can define tokens in a theme object that is passed to the `ThemeProvider`.
This requires the use of React's context API to access the theme object in your styles

```tsx
import { ThemeProvider } from 'styled-components'

const theme = {
  colors: {
    primary: 'blue',
    secondary: 'red'
  }
}

const App = () => (
  <ThemeProvider theme={theme}>
    <Button>Button</Button>
  </ThemeProvider>
)
````

In Panda, you define tokens in the `theme` key of the `panda.config.ts` file. This allows you to access the tokens in
your styles without the need for React's context API.

```tsx
// panda.config.ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      tokens: {
        colors: {
          primary: { value: "blue" },
          secondary: { value: "red" },
        },
      },
    },
  },
});
```

### Using Tokens

In styled-components, you can use tokens in your styles using a function approach that provides the `theme` prop, and
requires ambient type declarations to get type safety.

```tsx
import styled from "styled-components";

// link.tsx
const StyledLink = styled.a(({ theme }) => ({
  color: theme.colors.primary,
  display: "block",
  textDecoration: "none",
}));

// theme.d.ts
declare module "styled-components" {
  export interface DefaultTheme {
    colors: {
      primary: string;
      secondary: string;
    };
  }
}
```

In Panda, the tokens are automatically available in your styles and connected to each css property, removing the need
for an interpolation function.

```tsx
// panda.config.ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    // extend the base theme
    extend: {
      tokens: {
        // add custom colors
        colors: {
          primary: { value: "blue" },
          secondary: { value: "red" },
        },
      },
    },
  },
});

// link.tsx
import { styled } from "../styled-system/jsx";

const StyledLink = styled("a", {
  base: {
    color: "primary",
    display: "block",
    textDecoration: "none",
  },
});
```

## Responsive Styles

### Tagged Template Syntax

In styled-components, you need to write the media query styles manually or use a helper function like
`styled-media-query`.

```tsx
import styled from "styled-components";

const Button = styled.button`
  background-color: #fff;
  border: 1px solid #000;
  color: #000;
  padding: 0.5rem 1rem;

  @media (min-width: 768px) {
    padding: 1rem 2rem;
  }
`;
```

In Panda, it's pretty much the same thing except that you can't do any interpolation in the media query styles due the
static nature of Panda.

```tsx
import { styled } from "../styled-system/jsx";

const Button = styled.button`
  background-color: #fff;
  border: 1px solid #000;
  color: #000;
  padding: 0.5rem 1rem;

  @media (min-width: 768px) {
    padding: 1rem 2rem;
  }
`;
```

### Object Syntax

In styled-components, you can use the `styled-media-query` helper function to write responsive styles.

```tsx
import styled from "styled-components";
import media from "styled-media-query";

const Button = styled.button({
  backgroundColor: "#fff",
  border: "1px solid #000",
  color: "#000",
  padding: "0.5rem 1rem",

  [media.greaterThan("medium")]: {
    padding: "1rem 2rem",
  },
});
```

In Panda, you can use the pseudo props API to define responsive styles.

```tsx
import { styled } from "../styled-system/jsx";

const Button = styled("button", {
  base: {
    backgroundColor: "#fff",
    border: "1px solid #000",
    color: "#000",
    padding: { base: "0.5rem 1rem", md: "1rem 2rem" },
  },
});
```

## Global Styles

In styled-components, you can use the `createGlobalStyle` function to define global styles.

```tsx
import { createGlobalStyle } from "styled-components";

const GlobalStyle = createGlobalStyle`
  body {
    margin: 0;
    padding: 0;
  }
`;
```

In Panda, you can use the `globalCss` key of the `panda.config.ts` file to define global styles. This will automatically
add styles to the project via PostCSS.

```tsx
// panda.config.ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  globalCss: {
    body: {
      margin: 0,
      padding: 0,
    },
  },
});
```

## Targeting Components

In styled-components, you can target existing styled components within the styled function

```tsx
import styled from "styled-components";

const Link = styled.a`
  background: papayawhip;
  color: #bf4f74;
`;

const Icon = styled.svg`
  width: 48px;
  height: 48px;

  ${Link}:hover & {
    fill: rebeccapurple;
  }
`;
```

In Panda, you need to use the native selector directly. This is largely due to the static nature of Panda

```tsx
import { styled } from "../styled-system/jsx";

const Link = styled.a`
  background: papayawhip;
  color: #bf4f74;
`;

const Icon = styled.svg`
  width: 48px;
  height: 48px;

  .Link:hover & {
    fill: rebeccapurple;
  }
`;

const App = () => (
  <Link className="Link">
    <Icon />
  </Link>
);
```

## Animations

In styled components, you can define keyframes using the `keyframes` method.

```ts
import styled, { keyframes } from "styled-components";

const rotate = keyframes`
  from {
    transform: rotate(0deg);
  }

  to {
    transform: rotate(360deg);
  }
`;

// usage
const Button = styled.button`
  &:hover {
    animation: ${rotate} 200ms;
  }
`;
```

In Panda, you define keyframes in the `theme.keyframes` key of the `panda.config` function.

```ts
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      keyframes: {
        rotate: {
          from: {
            transform: "rotate(0deg)",
          },
          to: {
            transform: "rotate(360deg)",
          },
        },
      },
    },
  },
});

// usage
import { styled } from "../styled-system/jsx";

const Button = styled.button`
  &:hover {
    animation: rotate 200ms;
  }
`;
```

## Server-Side Rendering

In styled components, you need to configure the server-side rendering for your framework.

```tsx
import { renderToString } from "react-dom/server";
import { ServerStyleSheet } from "styled-components";

const sheet = new ServerStyleSheet();
try {
  const html = renderToString(sheet.collectStyles(<YourApp />));
  const styleTags = sheet.getStyleTags(); // or sheet.getStyleElement();
} catch (error) {
  // handle error
  console.error(error);
} finally {
  sheet.seal();
}
```

In Panda, you don't need to configure anything. Panda automatically extracts the styles and injects them at build time
using PostCSS.

## Conclusion

Before choosing your preferred CSS-in-JS library, be sure to consider your engineering and design goals. Both Styled
components and Panda are capable of achieving many of the same styling goals, but they have different approaches.

---

## Migrating from Theme UI

Migrate your project from Theme UI to Panda.

This guide outlines the steps needed to migrate your project from Theme UI to Panda and highlights key design
differences between the two libraries.

Here are some similarities between the two libraries.

- Panda and Theme UI both support JSX style props.
- Supports design tokens and themes.
- Support for styling primitives like `Box`, `Flex`, `Grid`, etc.

Below are some of the differences between the two libraries.

## Performance

Theme UI relies on `@emotion/styled` to style components. This means that every time you use the `sx` prop, runtime
CSS-in-JS is required to compute the styles in the browser. This can lead to performance issues in larger applications.

Panda relies on `postcss` and converts CSS-in-JS styles to static CSS at build-time, leading to better performance in
larger applications.

## Theming

In Theme UI, you need to wrap your application in a `ThemeProvider` component which is a wrapper around `@emotion/react`
theme context.

```jsx
import { ThemeProvider } from "theme-ui";

const theme = {
  fonts: {
    body: "system-ui, sans-serif",
    heading: '"Avenir Next", sans-serif',
  },
  colors: {
    text: "#000",
    background: "#fff",
  },
};

export default function App({ Component, pageProps }) {
  return (
    <ThemeProvider theme={theme}>
      <Component {...pageProps} />
    </ThemeProvider>
  );
}
```

In Panda, you don't need to wrap your application in a `ThemeProvider` component. Instead, you can pass the theme object
to the `panda.config.js` file.

The theme object in Panda is broken down into multiple parts, `tokens` and `semanticTokens`. The theme specification
also required passing the tokens as `{ value: XX }`

```js
// panda.config.js
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      tokens: {
        fonts: {
          body: { value: "system-ui, sans-serif" },
          heading: { value: '"Avenir Next", sans-serif' },
        },
        colors: {
          text: { value: "#000" },
          background: { value: "#fff" },
        },
      },
    },
  },
});
```

## The `sx` prop

In Theme UI, you can use the `sx` prop to style any component when you add the `jsxImportSource` pragma to the top of
your file.

```jsx
/** @jsxImportSource theme-ui */

export const Demo = (props) => (
  <div
    {...props}
    sx={{
      color: "white",
      bg: "primary",
      fontSize: 4,
    }}
  />
);
```

Panda offers three similar ways to style components. The first approach is to use the `styled` element syntax and rename
`sx` to `css`

```jsx
import { styled } from "styled-system/jsx";

export const Demo = (props) => (
  <styled.div
    {...props}
    css={{
      color: "white",
      bg: "primary",
      fontSize: 4,
    }}
  />
);
```

The second approach is to create styled components using the `styled` function. This approach allows you to create style
variants.

```jsx
import { styled } from "styled-system/jsx";

export const Demo = styled("div", {
  base: {
    color: "white",
    bg: "primary",
    fontSize: 4,
  },
});
```

The simplest approach is to use the `css` function to write one-off styles.

```jsx
import { css } from "styled-system/css";

export const Demo = (props) => (
  <div
    {...props}
    className={css({
      color: "white",
      bg: "primary",
      fontSize: 4,
    })}
  />
);
```

## Variants

In Theme UI, variants are used to create groups of styles based on the theme. It offers variant groups in the theme for
several components.

- `Grid` maps to `theme.grids`
- `Button`, `IconButton` maps to `theme.buttons`
- `NavLink`, `Link` maps to `theme.links`
- `Input`, `Select`, `Textarea` maps to `theme.forms`
- `Heading`, `Text` maps to `theme.text`

```js
// theme.js

export default {
  colors: {
    primary: '#07c',
    secondary: '#30c',
    accent: '#609'
  },
  buttons: {
    primary: {
      color: 'white',
      bg: 'primary'
    },
    secondary: {
      color: 'white',
      bg: 'secondary'
    },
    accent: {
      color: 'white',
      bg: 'accent'
    }
  }
}

// Button.js
<button sx={{ variant: 'buttons.primary' }} />
```

In Panda, multi-variant styles are referred to as recipes. Recipes are a collection of styles that can be applied to any
component.

There are two ways to define recipes in Panda. The first approach is to use the `cva` function which produces atomic
classnames.

```js
import { cva } from "styled-system/css";

const buttonStyles = cva({
  base: {
    display: "inline-flex",
  },
  variants: {
    variant: {
      primary: {
        color: "white",
        bg: "primary",
      },
      secondary: {
        color: "white",
        bg: "secondary",
      },
      accent: {
        color: "white",
        bg: "accent",
      },
    },
  },
});

const Demo = () => (
  <button
    className={buttonStyles({
      variant: "accent",
    })}
  />
);
```

The second approach is to define the recipe in the `theme.recipes` property of the panda config. This is referred to as
'Config recipes' in Panda and allows for sharing recipes across components and projects.

```js
// panda.config.js
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      recipes: {
        button: {
          className: "button",
          base: { display: "inline-flex" },
          variants: {
            variant: {
              primary: { color: "white", bg: "primary" },
              secondary: { color: "white", bg: "secondary" },
              accent: { color: "white", bg: "accent" },
            },
          },
        },
      },
    },
  },
});

// Button.js
import { button } from "styled-system/recipes";

const Demo = () => <button className={button({ variant: "accent" })} />;
```

## Color Modes

In Theme UI, colors modes can be used to create a user-configurable light and dark mode values that are automatically
applied to components depending on color mode.

```jsx
// theme.js
const theme = {
  colors: {
    primary: "#07c",
    modes: {
      dark: {
        primary: "#0cf",
      },
    },
  },
};

// Button.js
const Demo = () => <button sx={{ color: "primary" }} />;
```

In Panda, color modes related values are defined as `semanticTokens` in the theme. Semantic tokens are tokens that
change depending on the color mode.

```js
// panda.config.js
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  theme: {
    extend: {
      semanticTokens: {
        colors: {
          primary: { value: { base: "#07c", _dark: "#0cf" } },
        },
      },
    },
  },
});

// Button.js
import { css } from "styled-system/css";

const Demo = () => (
  <button
    className={css({
      color: "primary",
    })}
  />
);
```

## Global Styles

Theme UI offers a Global component (that wraps Emotion’s) for adding global CSS with theme-based values.

```jsx
import { Global } from "theme-ui";

export default (props) => (
  <Global
    styles={{
      button: {
        m: 0,
        bg: "primary",
        color: "background",
        border: 0,
      },
    }}
  />
);
```

In Panda, global styles are defined in the `theme.global` property of the panda config.

```js
// panda.config.js
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  globalCss: {
    button: {
      m: 0,
      bg: "primary",
      color: "background",
      border: 0,
    },
  },
});
```

## Component Styles

Theme UI offers pre-defined layout components like `Box`, `Stack`, `Grid`, `Flex`

```jsx
import { Box, Grid } from "theme-ui";

const Demo = () => (
  <Grid width={[128, null, 192]}>
    <Box bg="primary">Box</Box>
    <Box bg="muted">Box</Box>
    <Box bg="primary">Box</Box>
    <Box bg="muted">Box</Box>
  </Grid>
);
```

In Panda, these are called "layout patterns", or "patterns" for short. Panda provides similar patterns that can be used
as a function or JSX element just like Theme UI.

```jsx
import { Box, Grid } from "styled-system/jsx";

const Demo = () => (
  <Grid width={[128, null, 192]}>
    <Box bg="primary">Box</Box>
    <Box bg="muted">Box</Box>
  </Grid>
);
```

The function approach can be handy as well

```jsx
import { css } from "styled-system/css";
import { grid } from "styled-system/patterns";

const Demo = () => (
  <div className={grid({ width: [128, null, 192] })}>
    <div className={css({ bg: "primary" })}>Box</div>
    <div className={css({ bg: "muted" })}>Box</div>
  </div>
);
```

---
