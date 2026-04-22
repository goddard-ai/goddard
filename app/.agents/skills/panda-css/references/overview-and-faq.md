# Overview and FAQ

Use this reference for Panda's core model, browser support, high-level tradeoffs, and common FAQ answers.

## Browser Support

Learn about the browser support for Panda

Panda CSS is built with modern CSS features and uses [PostCSS](https://postcss.org/) to add support for older browsers.
Panda supports the latest, stable releases of major browsers that support the following features:

- [CSS Variables](https://caniuse.com/css-variables)
- [CSS Cascade Layers](https://caniuse.com/css-cascade-layers)
- Modern selectors, such as [`:where()`](https://caniuse.com/mdn-css_selectors_where) and
  [`:is()`](https://caniuse.com/css-matches-pseudo)

## Browserlist

Based on the above criteria, the following browsers are supported:

```txt
>= 1%
last 1 major version
not dead
Chrome >= 99
Edge >= 99
Firefox >= 97
iOS >= 15.4
Safari >= 15.4
Android >= 115
Opera >= 73
```

## Polyfills

In event that you need to support older browsers, you can use the following polyfills in your PostCSS config:

- [autoprefixer](https://github.com/postcss/autoprefixer): Adds vendor prefixes to CSS rules using values from
  [Can I Use](https://caniuse.com/).
- [postcss-cascade-layers](https://www.npmjs.com/package/@csstools/postcss-cascade-layers): Adds support for CSS Cascade
  Layers.

Here is an example of a `postcss.config.js` file that uses these polyfills:

```js
module.exports = {
  plugins: [
    "@pandacss/dev/postcss",
    "autoprefixer",
    "@csstools/postcss-cascade-layers",
  ],
};
```

---

## Frequently Asked Questions

Frequently asked questions and how to resolve common issues

## How does Panda manage style conflicts ?

When you combine shorthand and longhand properties, Panda will resolve the styles in a predictable way. The shorthand
property will take precedence over the longhand property.

```jsx
import { css } from "../styled-system/css";

const styles = css({
  paddingTop: "20px",
  padding: "10px",
});
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

---

## Imported Image is not working in Vite App

This is a known limitation of Panda due to our static extraction approach.

> Think of it this way: there's no way for the compiler to know what the final asset URL will be since Vite controls it.

We recommend moving the imported `backgroundImage` to the `style` attribute.

```jsx
import myImageBackground from "./my-image.png";

const Demo = () => {
  return (
    <p
      className={css({ bg: "red.300", backgroundRepeat: "repeat" })}
      style={{ backgroundImage: `url("${myImageBackground}")` }}
    >
      Hello World
    </p>
  );
};
```

---

## How to get Panda to work with Jest?

If you run into error messages like `SyntaxError: Unexpected token 'export'` when running Jest tests. Here's what you
can:

In your tsconfig, add

```json
{
  "compilerOptions": {
    "allowJs": true
  }
}
```

In your Jest configuration, add the `ts-jest` transformer:

```ts
export default {
  // ...
  transform: {
    "^.+\\.tsx?$": "ts-jest",
    "^.+\\.(ts|tsx|js|jsx)?$": "ts-jest",
  },
};
```

In your Panda config, set the `outExtension` to `js`:

```ts
export default defineConfig({
  // ...
  outExtension: "js",
});
```

---

## HMR does not work when I use `tsconfig` paths?

Panda tries to automatically infer and read the custom paths defined in `tsconfig.json` file. However, there might be
scenarios where the hot module replacement doesn't work.

To fix this add the `importMap` option to your `panda.config.js` file, setting it's value to the specified `paths` in
your `tsconfig.json` file.

```json
// tsconfig.json

{
  "compilerOptions": {
    "baseUrl": "./src",
    "paths": {
      "@my-path/*": ["./styled-system/*"]
    }
  }
}
```

```js
// panda.config.js

module.exports = {
  importMap: "@my-path",
};
```

This will ensure that the paths are resolved correctly, and HMR works as expected.

---

## HMR not triggered

If you are having issues with HMR not being triggered after a `panda.config.ts` change (or one of its
[dependencies](/docs/references/config#dependencies)), you can manually specify the files that should trigger a rebuild
by adding the following to your `panda.config.ts`:

```js filename="panda.config.ts"
import { defineConfig } from "@pandacss/dev";

export default defineConfig({
  // ...
  dependencies: ["path/to/files/**.ts"],
});
```

---

## Why are my styles not applied?

Check that the [`@layer` rules](/docs/concepts/cascade-layers#layer-css) are set and the corresponding `.css` file is
included. [If you're not using `postcss`](/docs/installation/cli), ensure that `styled-system/styles.css` is imported
and that the `panda` command has been run (or is running with `--watch`).

---

## How can I debug the styles?

You can use the `panda debug` to debug design token extraction & css generated from files.

If the issue persists, you can try looking for it in the [issues](https://github.com/chakra-ui/panda/issues) or in the
[discord](https://discord.gg/VQrkpsgSx7). If you can't find it, please create a minimal reproduction and submit
[a new github issue](https://github.com/chakra-ui/panda/issues/new/choose) so we can help you.

---

## Why is my IDE not showing `styled-system` imports?

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
tsconfig.json file.

---

## How do I get a type with each recipe properties?

You can get a [`config recipe`](/docs/concepts/recipes#config-recipe) properties types by using `XXXVariantProps`. Let's
say you have a `config recipe` named `button`, you can import its type like this:

```ts
import { button, type ButtonVariantProps } from "../styled-system/recipes";
```

---

You can get an [`atomic recipe`](/docs/concepts/recipes#atomic-recipe) properties types by using `RecipeVariantProps`.
Let's say you have a `atomic recipe` named `button`, you can get its type like this:

```ts
import { cva, type RecipeVariantProps } from "../styled-system/css";

export type ButtonVariantProps = RecipeVariantProps<typeof buttonStyle>;
```

---

## How do I split recipe props from the rest?

You can split recipe props by using `xxx.splitVariantProps`. Let's say you have a `recipe` named `button`, you can split
its props like this:

```tsx Button.tsx {8}
import { css, cx } from "../styled-system/css";
import { ButtonVariantProps, button } from "../styled-system/recipes";

interface ButtonProps extends ButtonVariantProps {
  children: React.ReactNode;
}

export function Button(props: ButtonProps) {
  const { children, ...rest } = props;
  const [buttonProps, cssProps] = button.splitVariantProps(rest);
  return (
    <button className={cx(button(buttonProps), css(cssProps))}>
      {children}
    </button>
  );
}
```

The same `xxx.splitVariantProps` method is available for both `config recipes` and `atomic recipes`.

---

## How do I reference a token value or css var?

You can reference a token value or it's associated css variable using the
[`token` function](/docs/theming/usage#vanilla-js). This function allows you to access and use the values stored in your
theme tokens at runtime.

```tsx
import { token } from "../styled-system/tokens";

function App() {
  return (
    <div
      style={{
        background: token("colors.blue.200"),
      }}
    />
  );
}
```

---

## Should I commit the styled-system folder?

Just like the `node_modules` folder, you most likely don't want to commit the `styled-system` folder. It contains code
that is auto-generated and can be re-generated at any time.

---

## How does Panda work?

When running `pnpm panda`, here's what's happening under the hood:

- **Load Panda context**:
  - Find and evaluate app config, merge result with presets.
  - Create panda context: prepare code generator from config, parse user's file as AST.
- **Generating artifacts**:
  - Write lightweight JS runtime and types to output directory
- **Extracting used styles in app code**:
  - Run parser on each user's file: identify and extract styles, compute CSS, write to styles.css.

---

## I'm seeing a "Could not resolve xxx" error with esbuild/tsup. What should I do?

In such a case, check the [`outExtension`](/docs/references/config#outextension) in your `panda.config` and set it to
"js". This will ensure your modules are resolved correctly.

---

## Why does importing `styled` not exist?

You should use [`config.jsxFramework`](/docs/concepts/style-props#configure-jsx) when you need to import styled
components. You can then use the [`jsxFactory`](/references/config#jsxfactory) option to set the name of the factory
component.

---

## Why is my preset overriding the base one, even after adding it to the array?

You might have forgotten to include the `extend` keyword in your config. Without `extend`, your preset will completely
replace the base one, instead of merging with it.

---

## Why is my base condition not working in this example?

```ts
css({ color: { _base: "red.600", _dark: "white" } });
```

You used `_base` instead of `base`, there is no underscore `_`.

---

## What's the difference between using `defineConfig()` vs `definePreset()`

`defineConfig` is intended to be used in your app config, and will show you all the config keys that are available.
`definePreset` will only show you the config keys that will be merged into an app's config, the rest will be ignored.

---

## How can I completely override the default tokens?.

If you want to **completely override all** of the default presets theme tokens, you can omit the `extends` keyword from
your `theme` config object.

If you want to **keep some of the defaults**, you can install the `@pandacss/preset-panda` package, import it, then
specifically pick what you need in there (or use the JS spread operator `...` and override the other keys).

---

## How do I make a design system / component library with Panda?

There is a detailed guide on how to do this [here](/docs/guides/component-library).

---

## Can I use dynamic styles with Panda?

Yes, you can use dynamic styles with Panda. More on that [here](/docs/guides/dynamic-styling#runtime-conditions).

---

## Should I use atomic or config recipes ?

[Config recipes](/docs/concepts/recipes#config-recipe) are generated just in time, meaning that only the recipes and
variants you use will exist in the generated CSS, regardless of the number of recipes in the config.

This contrasts with [Atomic recipes](/docs/concepts/recipes#atomic-recipe) (cva), which generates all of the variants
regardless of what was used in your code. The reason for this difference is that all `config.recipes` are known at the
start of the panda process when we evaluate your config. In contrast, the CVA recipes are scattered throughout your
code. To get all of them and find their usage across your code, we would need to scan your app code multiple times,
which would not be ideal performance-wise.

When dealing with simple use cases, or if you need code colocation, or even avoiding dynamic styling, atomic recipes
shine by providing all style variants. Config recipes are preferred for design system components, delivering leaner CSS
with only the styles used. Choose according to your component needs.

---

## Why does the panda codegen command fail ?

If you run into any error related to "Transforming const to the configured target environment ("es5") is not supported
yet", update your tsconfig to use es6 or higher:

```json filename="tsconfig.json"
{
  "compilerOptions": {
    "target": "es6"
  }
}
```

---

## How can I generate all possible CSS variants at build time?

While it's possible to generate all variants, even unused ones, by using
[`config.staticCss`](https://panda-css.com/docs/guides/dynamic-styling#using-static-css), it's generally **not
recommended** to use it for more than a few values. However, keep in mind this approach compromises one of Panda's
strengths: lean, usage-based CSS generation.

---

## Can I use one-off media query and other at rules?

Yes, you can! You can apply one-off media queries and other at rules (such as `@container`, `@supports`) in your CSS as
shown below:

```javascript
css({
  containerType: "size",
  "@media (min-width: 10px)": {
    fontSize: "xl",
    color: "blue.300",
  },
  "@container (min-width: 10px)": {
    fontSize: "2xl",
    color: "green.300",
  },
  "@supports (display: flex)": {
    fontSize: "3xl",
    color: "red.300",
  },
});
```

---

## How can I prevent other libraries from overriding my styles?

You can use
[Layer Imports](<https://developer.mozilla.org/en-US/docs/Web/CSS/@import#layer-name:~:text=%40import%20url%20layer(layer%2Dname)%3B>)
to prevent other libraries from overriding your styles.

First of all you cast the css from the other library(s) to a css layer:

```css
@import url("bootstrap.css") layer(bootstrap);

@import url("ionic.css") layer(ionic);
```

Then update the default layer list to deprioritize the styles from the other library(s):

```css
@layer bootstrap, reset, base, token, recipes, utilities;

@layer ionic, reset, base, token, recipes, utilities;
```

---

## Get started with Panda

The universal design system solution for the web

Panda is a styling engine that generates styling primitives to author atomic CSS and recipes in a type-safe and readable
manner.

Panda combines the developer experience of CSS-in-JS and the performance of atomic CSS. It leverages static analysis to
scan your JavaScript and TypeScript files for JSX style props and function calls, generating styles on-demand (aka
Just-in-Time)

> TLDR; Panda is a CSS-in-JS engine that generates atomic CSS at build time (via CLI or PostCSS)

## Installation

### General Guides

- [Panda CLI](/docs/installation/cli): The simplest way to use Panda is with the Panda CLI tool.

- [Using PostCSS](/docs/installation/postcss): (**Recommended**) Installing Panda as a PostCSS plugin is the recommended
  way to integrate it with your project.

### Framework Guides

Start using Panda CSS in your JavaScript framework using our framework-specific guides that cover our recommended
approach.

<FrameworkCards />

## Next Steps

Get familiar with the core features and concepts in Panda.

<Cards>
  <Card
    arrow
    title="Tokens"
    description="Learn how setup and customize design tokens to customize your colors, typography, and more"
    href="/docs/theming/tokens"
  />
  <Card
    arrow
    title="Recipes"
    description="Create composable style variants that are typed and generates the atomic or layer recipe"
    href="/docs/concepts/recipes"
  />
  <Card
    arrow
    title="Patterns"
    description="Use the built-in layout patterns like stack, flex, grid to speed up your development"
    href="/docs/concepts/patterns"
  />
  <Card
    arrow
    title="Utilities"
    description="Learn how to create your own custom css properties and speed up your development"
    href="/docs/customization/utilities"
  />
</Cards>

## Playground

You can use the [online playground](https://play.panda-css.com) to get a taste of what Panda can do.

- See the live results of your JSX code
- Inspect what panda can extract using static analysis from your code
- Preview the statically generated `.css` files

## Acknowledgement

The development of Panda was only possible due to the inspiration and ideas from these amazing projects.

- [Chakra UI](https://chakra-ui.com/) - where it all started
- [Vanilla Extract](https://vanilla-extract.style/) - for inspiring the utilities API
- [Stitches](https://stitches.dev/) - for inspiring the recipes and variants API
- [Tailwind CSS](https://tailwindcss.com/) - for inspiring the JIT compiler and strategy
- [Class Variance Authority](https://cva.style/) - for inspiring the `cva` name
- [Styled System](https://styled-system.com/) - for the initial idea of Styled Props
- [Linaria](https://linaria.dev/) - for inspiring the initial atomic css strategy
- [Uno CSS](https://unocss.dev) - for inspiring the studio and astro integration

---

## Why Panda

From the endless list of CSS-in-JS libraries, why should you choose Panda?

Runtime CSS-in-JS and style props are powerful features that allow developers to build dynamic UI components that are
composable, predictable, and easy to use. However, it comes at the cost of performance and runtime.

## Backstory

With the release of Server Components and the rise of server-first frameworks, most existing runtime CSS-in-JS styling
solutions (like emotion, styled-components) either can't work reliably, or can't work anymore. This new paradigm is a
huge win for performance, development, and user experience, however, it poses a new "Innovate or Die" challenge for
CSS-in-JS libraries.

> **Fun Fact:** Most CSS-in-JS libraries have a pinned issue on their GitHub repo about "Next app dir" or/and "Server
> Components" 😅, making the challenge even more obvious.

So, the question is, **is CSS-in-JS dead?** The answer is **no, but it needs to evolve!**

## The new era of CSS-in-JS

Panda is a new CSS-in-JS engine that aims to solve the challenges of CSS-in-JS in the server-first era. It provides
styling primitives to create, organize, and manage CSS styles in a type-safe and readable manner.

- **Static Analysis:** Panda uses static analysis to parse and analyze your styles at build time, and generate CSS files
  that can be used in any JavaScript framework.

- **PostCSS:** After static analysis, Panda uses a set of PostCSS plugins to convert the parsed data to atomic css at
  build time. **This makes Panda compatible with any framework that supports PostCSS.**

- **Codegen:** Panda generates a lightweight runtime JS code that is used to author styles. **Think of it as an
  optimized function that joins key-value pairs of an object**. It doesn't generate styles in the browser nor inject
  styles in the `<head>`.

- **Type-Safety:** Panda combines `csstype` and auto-generated typings to provide type-safety for css properties and
  design tokens.

- **Performance:** Panda uses a unique approach to generate atomic CSS files that are optimized for performance and
  readability.

- **Developer Experience:** Panda provides a great developer experience with a rich set of features like recipes,
  patterns, design tokens, JSX style props, and more.

- **Modern CSS**: Panda uses modern CSS features like cascade layers, css variables, modern selectors like `:where` and
  `:is` in generated styles.

## When to use Panda?

### Styling engine

If you're building a JavaScript application with a framework that supports PostCSS, Panda is a great choice for you.

```jsx
import { css } from "../styled-system/css";
import { circle, stack } from "../styled-system/patterns";

function App() {
  return (
    <div
      className={stack({
        direction: "row",
        p: "4",
        rounded: "md",
        shadow: "lg",
        bg: "white",
      })}
    >
      <div className={circle({ size: "5rem", overflow: "hidden" })}>
        <img src="https://via.placeholder.com/150" alt="avatar" />
      </div>
      <div className={css({ mt: "4", fontSize: "xl", fontWeight: "semibold" })}>
        John Doe
      </div>
      <div className={css({ mt: "2", fontSize: "sm", color: "gray.600" })}>
        john@doe.com
      </div>
    </div>
  );
}
```

> If your framework doesn't support PostCSS, you can use the [Panda CLI](/docs/installation/cli)

### Token generator

Panda has first-class support for design tokens. It provides a way to express raw and semantic tokens for your project.
The generator can be used to create a set of CSS variables for your design tokens.

```ts filename="panda.config.ts"
export default defineConfig({
  emitTokensOnly: true,
  theme: {
    tokens: {
      colors: {
        gray50: { value: "#F9FAFB" },
        gray100: { value: "#F3F4F6" },
      },
    },
    semanticTokens: {
      colors: {
        primary: { value: "{colors.gray50}" },
        success: {
          value: { _light: "{colors.green500}", _dark: "{colors.green200}" },
        },
      },
    },
  },
});
```

Running the `panda codegen` will generate

```css filename="styled-system/tokens/index.css"
:root {
  --colors-gray50: #f9fafb;
  --colors-gray100: #f3f4f6;
  --colors-primary: var(--colors-gray50);
  --colors-success: var(--colors-green500);
}

[data-theme="dark"] {
  --colors-primary: var(--colors-gray50);
  --colors-success: var(--colors-green200);
}
```

Then you have a set of css variables that you can use in your project.

```css
@import "../styled-system/tokens/index.css";

.card {
  background-color: var(--colors-gray50);
}
```

## When not to use Panda?

Panda isn't the right fit for your project if:

- You're building with HTML and CSS.
- You're using a template-based framework like PHP.
- You're looking for an absolute zero JS solution.

In these scenarios, we recommend that you use vanilla CSS (which is getting awesome by the day), or other utility based
CSS libraries.

---
