# Guides and Troubleshooting

Use this reference for component library packaging, dynamic styling, debugging, static CSS generation, multi-theme setups, and related troubleshooting.

## Using Panda in a Component Library

How to set up Panda in a component library

When creating a component library that uses Panda which can be used in a variety of different projects, you have four
options:

1. Ship a Panda [preset](/docs/customization/presets)
2. Ship a static CSS file
3. Use Panda as external package, and ship the src files
4. Use Panda as external package, and ship the build info file

> In the examples below, we use `tsup` as the build tool. You can use any other build tool.

## Recommendations

- If your library code shouldn't be published on npm and App code uses Panda, use
  [ship build info](#ship-the-build-info-file) approach
- If your app code doesn't use Panda, use the [static css](#ship-a-static-css-file) file approach
- If your app code lives in a monorepo, use the [include src files](#include-the-src-files) approach
- If your library code doesn't ship components but only ships tokens, patterns or recipes, use the
  [ship preset](#ship-a-panda-preset) approach

> ⚠️ If you use the `include src files` or `ship build info` approach, you might also need to ship a `preset` if your
> library code has any custom tokens, patterns or recipes.

## Ship a Panda Preset

This is the simplest approach. You can include the token, semantic tokens, text styles, etc. within a preset and consume
them in your projects.

**Library code**

```tsx filename="src/index.ts"
import { definePreset } from '@pandacss/dev'

export const acmePreset = definePreset({
  theme: {
    extend: {
      tokens: {
        colors: { primary: { value: 'blue.500' } }
      }
    }
  }
})
```

Build the preset code

```bash
pnpm tsup src/index.ts
```

**App code**

```tsx filename="panda.config.ts"
import { acmePreset } from '@acme-org/panda-preset'
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  presets: ['@pandacss/dev/presets', acmePreset]
})
```

> Adding a preset will remove the default theme from Panda. To add it back, you need to include the
> `@pandacss/dev/presets` preset.

## Ship a Static CSS File

This approach involves extracting the static css of your library at build time. Then you can import the CSS file in your
app code.

**Library code**

```tsx filename="src/index.tsx"
import { css } from '../styled-system/css'

export function Button({ children }) {
  return (
    <button type="button" className={css({ bg: 'red.300', px: '2', py: '3' })}>
      {children}
    </button>
  )
}
```

Then you can build the library code and generate the static CSS file:

```bash
# build the library code
tsup src/index.tsx

# generate the static CSS file
panda cssgen --outfile dist/styles.css
```

Finally, don't forget to include the [cascade layers](/docs/concepts/cascade-layers) as well in your app code:

**App code**

```tsx filename="src/App.tsx"
import { Button } from '@acme-org/design-system'
import './main.css'

export function App() {
  return <Button>Click me</Button>
}
```

**main.css**

```css filename="src/main.css"
@layer reset, base, tokens, recipes, utilities;
@import url('@acme-org/design-system/dist/styles.css');

/* Your own styles here */
```

This approach comes with a few downsides:

- You can't customize the styles since the css is already generated
- You might need add the [prefix](/docs/references/config#prefix) option to avoid className conflicts

  ```tsx filename="panda.config.ts"
  import { defineConfig } from '@pandacss/dev'

  export default defineConfig({
    //...
    prefix: 'acme'
  })
  ```

- You might have duplicate CSS classes when using multiple atomic css libraries

## Use Panda as external package

### Summary

- create a Panda [preset](/docs/customization/presets) so that you (and your users) can share the same design system
  tokens
- create a workspace package for your outdir (`@acme-org/styled-system`) and use that package name as the `importMap` in
  your app code
- have your component library (`@acme-org/components`) use the `@acme-org/styled-system` package as external

---

Let's make a dedicated workspace package for your `outdir`:

1. Create a new directory `packages/styled-system` (or any other name)
2. Install `@pandacss/dev` as a dev dependency
3. Run the `panda init` command in there to generate a `panda.config.ts` file, don't forget to set the `jsxFramework` if
   needed
4. [optional] you might want to install and import your [preset](/docs/customization/presets) in this `panda.config.ts`
   file as well
5. Run the [`panda emit-pkg`](/docs/references/cli#emit-pkg) command to set the entrypoints in
   [`exports`](https://nodejs.org/api/packages.html#exports)

This should look similar to this:

```json
{
  "name": "@acme-org/styled-system",
  "version": "1.0.0",
  "exports": {
    "./css": {
      "types": "./css/index.d.ts",
      "require": "./css/index.mjs",
      "import": "./css/index.mjs"
    },
    "./tokens": {
      "types": "./tokens/index.d.ts",
      "require": "./tokens/index.mjs",
      "import": "./tokens/index.mjs"
    },
    "./types": {
      "types": "./types/index.d.ts",
      "require": "./types/index.mjs",
      "import": "./types/index.mjs"
    },
    "./patterns": {
      "types": "./patterns/index.d.ts",
      "require": "./patterns/index.mjs",
      "import": "./patterns/index.mjs"
    },
    "./recipes": {
      "types": "./recipes/index.d.ts",
      "require": "./recipes/index.mjs",
      "import": "./recipes/index.mjs"
    },
    "./jsx": {
      "types": "./jsx/index.d.ts",
      "require": "./jsx/index.mjs",
      "import": "./jsx/index.mjs"
    },
    "./styles.css": "./styles.css"
  },
  "devDependencies": {
    "@pandacss/dev": "^1.4.2",
    "@types/react": "19.2.2",
    "react": "^19.2.0"
  },
  "peerDependencies": {
    "react": ">=19"
  }
}
```

> Notice that we've included the `react` and its corresponding `@types/react` in the `devDependencies` and
> `peerDependencies`. This is to ensure the types are correctly set up in the `styled-system` package.

Going forward, you'll now import the functions from the `@acme-org/styled-system` monorepo package.

```tsx
import { css } from '@acme-org/styled-system/css'

export function Button({ children }) {
  return (
    <button type="button" className={css({ bg: 'red.300', px: '2', py: '3' })}>
      {children}
    </button>
  )
}
```

**App code**

Install the newly created `@acme-org/styled-system` package in your app code.

```bash
pnpm add @acme-org/styled-system
```

Configure the `importMap` in your `panda.config.ts` to match the `name` field of your outdir `package.json`. This will
inform Panda which imports belong to the `styled-system`.

```tsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  importMap: '@acme-org/styled-system',
  outdir: 'styled-system'
})
```

Mark the `@acme-org/styled-system` as an external package in your library build tool. This ensures that the generated JS
runtime code is imported only once, avoiding duplication.

```bash
tsup src/index.tsx --external @acme-org/styled-system
```

### Include the src files

Include the `src` directory from the library code in the panda config.

```tsx {6} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  include: ['../@acme-org/design-system/src/**/*.tsx', './src/**/*.{ts,tsx}'],
  importMap: '@acme-org/styled-system',
  outdir: 'styled-system'
})
```

### Ship the build info file

This approach is similar to the previous one, but instead of shipping the source code, you ship the Panda build info
file. This will have **the exact same end-result** as adding the sources files in the `include`, but it will allow you
not to ship the source code.

The build info file is a JSON file that **only** contains the information about the static extraction result, you still
need to ship your app build/dist by yourself. It can be used by Panda to generate CSS classes without the need for
parsing the source code.

Generate the build info file:

```bash
panda ship --outfile dist/panda.buildinfo.json
```

**App code**

Install the newly created `@acme-org/styled-system` package in your app code.

```bash
pnpm add @acme-org/styled-system
```

Configure the `importMap` in your `panda.config.ts` to match the `name` field of your outdir `package.json`. This will
inform Panda which imports belong to the `styled-system`.

```tsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  importMap: '@acme-org/styled-system',
  outdir: 'styled-system'
})
```

Will allow imports like:

```tsx
import { css } from '@acme-org/styled-system/css'
import { button } from '@acme-org/styled-system/recipes'
```

Next, you need to include the build info file from the library code in the panda config.

```tsx {6} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  include: ['./node_modules/@acme-org/design-system/dist/panda.buildinfo.json', './src/**/*.{ts,tsx}'],
  importMap: '@acme-org/styled-system',
  outdir: 'styled-system'
})
```

## FAQ

### Why should my component library use an external package `styled-system`?

By de-coupling the component library from the `styled-system`, your users can share the same runtime code between your
library and their app code.

```tsx filename="component-lib/src/button.tsx"
import { css } from '@acme-org/styled-system/css'

export function Button({ children, css: cssProp }) {
  return (
    <button type="button" className={css({ bg: 'red.300', px: '2', py: '3' }, cssProp)}>
      {children}
    </button>
  )
}
```

```tsx filename="app/src/App.tsx"
import { Button } from '@acme-org/design-system'
import { css } from '@acme-org/styled-system/css'

export function App() {
  return <Button css={{ color: 'white' }}>Click me</Button>
}
```

Marking the `styled-system` as an external package in your build tool means that the generated JS runtime code (the
`css` function is the example above) is imported only once, avoiding duplication.

### How do I use the `@acme-org/styled-system` package ?

You can use your monorepo workspace package `@acme-org/styled-system` just like any other dependency in your app or
component library code.

```bash
pnpm add @acme-org/styled-system
```

Set the `importMap` in your `panda.config.ts` to that same package name. This will inform Panda which imports belong to
the `styled-system`.

```tsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  importMap: '@acme-org/styled-system'
})
```

Then you can import the functions from the `@acme-org/styled-system` monorepo package.

```tsx
import { css } from '@acme-org/styled-system/css'

export function Button({ children }) {
  return (
    <button type="button" className={css({ bg: 'red.300', px: '2', py: '3' })}>
      {children}
    </button>
  )
}
```

### How to override tokens used by the `@acme-org/styled-system` package?

You can override the tokens used by the `@acme-org/styled-system` package by extending the `theme` in your
`panda.config.ts` file.

```tsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  //...
  presets: ['@acme-org/preset']
  theme: {
    extend: {
      tokens: {
        colors: { primary: { value: 'blue.500' } }
      }
    }
  }
})
```

## Troubleshooting

- When using `tsup` or any other build tool for your component library, if you run into a module resolution error that
  looks similar to `ERROR: Could not resolve "../styled-system/xxx"`. Consider setting the `outExtension`in the panda
  config to`js`

- If you use Yarn PnP, you might need to set the `nodeLinker: node-modules` in the `.yarnrc.yml` file.

---


## Debugging

How can I debug my styles or profile the extraction process?

## panda debug

Panda's built-in debug command helps you see which files are processed, what styles are generated, and your final
config.

By default it will scan and output debug files for the entire project depending on your `include` and `exclude` options
from your config file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm panda debug
    # You can also debug a specific file or folder
    # using the optional glob argument
    pnpm panda debug src/components/Button.tsx
    pnpm panda debug "./src/components/**"
    ```
  </Tab>
  <Tab>
    ```bash
    npx panda debug
    # # You can also debug a specific file or folder
    # using the optional glob argument
    npx panda debug src/components/Button.tsx
    npx panda debug "./src/components/**"
    ```
  </Tab>
  <Tab>
    ```bash
    yarn panda debug
    # # You can also debug a specific file or folder
    # using the optional glob argument
    yarn panda debug src/components/Button.tsx
    yarn panda debug "./src/components/**"
    ```
  </Tab>
  <Tab>
    ```bash
    bun panda debug
    # # You can also debug a specific file or folder
    # using the optional glob argument
    bun panda debug src/components/Button.tsx
    bun panda debug "./src/components/**"
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

This would generate a `debug` folder in your `config.outdir` folder with the following structure:

<FileTree>
  <FileTree.Folder name="styled-system" defaultOpen>
    <FileTree.Folder name="debug" defaultOpen>
      <FileTree.File name="config.json" />
      <FileTree.File name="src__components__Button.ast.json" />
      <FileTree.File name="src__components__Button.css" />
    </FileTree.Folder>
  </FileTree.Folder>
</FileTree>

The `config.json` file will contain the resolved config result, meaning the output after merging config presets in your
own specific options.

It can be useful to check if your config contains everything you expect for your app to be working, such as tokens or
recipes.

`*.ast.json` files will look like:

```json
[
  {
    "name": "css",
    "type": "object",
    "data": [
      {
        "transitionProperty": "all",
        "opacity": "0.5",
        "border": "1px solid",
        "borderColor": "black",
        "color": "gray.600",
        "_hover": {
          "color": "gray.900"
        },
        "rounded": "md",
        "p": "1.5",
        "_dark": {
          "borderColor": "rgba(255, 255, 255, 0.1)",
          "color": "gray.400",
          "_hover": {
            "color": "gray.50"
          }
        }
      }
    ],
    "kind": "CallExpression",
    "line": 13,
    "column": 9
  }
]
```

And the `.css` file associated would just contain the styles generated from the extraction process on that file only.

## PANDA_DEBUG env variable

You can prefix any of the Panda CLI command with the `PANDA_DEBUG` environment variable to show debug logs.

```bash
PANDA_DEBUG=* pnpm panda
```

This can be useful to check if a specific file is being processed or not, or if a specific function/component has been
extracted.

```
❯ PANDA_DEBUG=* pnpm panda cssgen
🐼 debug [config:path] /Users/astahmer/dev/open-source/panda-clone/website/panda.config.ts
🐼 debug [ast:import] Found import { css } in /Users/astahmer/dev/open-source/panda-clone/website/theme.config.tsx
🐼 debug [ast:Icon] { kind: 'component' }
🐼 debug [ast:css] { kind: 'function' }
🐼 debug [hrtime] Parsed /Users/astahmer/dev/open-source/panda-clone/website/theme.config.tsx (9.66ms)
🐼 debug [ast:import] Found import { css } in /Users/astahmer/dev/open-source/panda-clone/website/src/DEFAULT_THEME.tsx
🐼 debug [ast:DiscordIcon] { kind: 'component' }
🐼 debug [ast:css] { kind: 'function' }
🐼 debug [ast:Anchor] { kind: 'component' }
🐼 debug [ast:GitHubIcon] { kind: 'component' }
🐼 debug [ast:Flexsearch] { kind: 'component' }
🐼 debug [ast:MatchSorterSearch] { kind: 'component' }
🐼 debug [hrtime] Parsed /Users/astahmer/dev/open-source/panda-clone/website/src/DEFAULT_THEME.tsx (4.51ms)
🐼 debug [ast:import] No import found in /Users/astahmer/dev/open-source/panda-clone/website/src/constants.tsx
🐼 debug [hrtime] Parsed /Users/astahmer/dev/open-source/panda-clone/website/src/constants.tsx (4.23ms)
🐼 debug [ast:import] Found import { css } in /Users/astahmer/dev/open-source/panda-clone/website/src/index.tsx
🐼 debug [ast:css] { kind: 'function' }
```

## Performance profiling

If Panda is taking too long to process your files, you can use the `--cpu-prof` with the `panda`, `panda cssgen`,
`panda codegen` and `panda debug` commands to generate a flamegraph of the whole process, which will allow you (or us as
maintainers) to see which part of the process is taking the most time.

This will generate a `panda-{command}-{timestamp}.cpuprofile` file in the current working directory, which can be opened
in tools like [Speedscope](https://www.speedscope.app/)

```bash
pnpm panda --cpu-prof
```

## FAQ

### Why are my styles not applied?

Check that the [`@layer` rules](/docs/concepts/cascade-layers#layer-css) are set and the corresponding `.css` file is
included. [If you're not using `postcss`](/docs/installation/cli), ensure that `styled-system/styles.css` is imported
and that the `panda` command has been run (or is running with `--watch`).

### Some CSS is missing when using absolute imports

This can happen when `tsconfig` (with `paths` or `baseUrl`) or with package.json
[`#imports`](https://nodejs.org/api/packages.html#subpath-imports). Panda tries to automatically infer and read the
custom paths defined in `tsconfig.json` file. However, there might be scenarios that won't work.

To fix this add the `importMap` option to your `panda.config.js` file, setting it's value to match the way you import
the `outdir` modules.

```app.tsx
// app.tsx

import { css } from "~/styled-system/css"
// tsconfig.json paths
// -> importMap: "~/styled-system"

import { css } from "styled-system/css"
// tsconfig.json baseUrl
// -> importMap: "styled-system"

import { css } from "@my-monorepo/ui-kit/css"
// monorepo workspace package
// -> importMap: "@my-monorepo/ui-kit"

import { css } from "#styled-system/css"
// package.json#imports
// -> importMap: "#styled-system

```

```js
// panda.config.js

export default defineConfig({
  importMap: '~/styled-system'
})
```

This will ensure that the paths are resolved correctly, and HMR works as expected.

---

### How can I debug the styles?

You can use the `panda debug` to debug design token extraction & css generated from files.

If the issue persists, you can try looking for it in the [issues](https://github.com/chakra-ui/panda/issues) or in the
[discord](https://discord.gg/VQrkpsgSx7). If you can't find it, please create a minimal reproduction and submit
[a new github issue](https://github.com/chakra-ui/panda/issues/new/choose) so we can help you.

---

### Why is my IDE not showing `styled-system` imports?

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
tsconfig.json file.

### HMR does not work when I use `tsconfig` paths?

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
  importMap: '@my-path'
}
```

This will ensure that the paths are resolved correctly, and HMR works as expected.

---

## The postcss plugin sometimes seems slow or runs too frequently

This is mostly specific to the host bundler (`vite`, `webpack` etc) you're using, it is up to them to decide when to run
the postcss plugin again, and sometimes it can be more than needed for your usage. We do our best to cache the results
of the postcss plugin by checking if the filesystem or your config have actually changed, but sometimes it might not be
enough.

In those rare cases, you might want to swap to using the [CLI](/docs/installation/cli) instead, as it will always be
more performant than the postcss alternative since we directly watch for filesystem changes and only run the
extract/codegen steps when needed.

If you want to keep the convenience of having just one command to run, you can use something like `concurrently` for
that:

```json file="package.json"
{
  "scripts": {
    "dev": "concurrently \"next dev\" \"panda --watch\""
  }
}
```

---


## Dynamic styling

How to manage dynamic styling in Panda

While Panda is mainly focused on the statically analyzable styles, you might need to handle dynamic styles in your
project.

> We recommend that you **avoid relying on runtime values for your styles** . Consider using recipes, css variables or
> `data-*` attributes instead.

Here are some ways you can handle dynamic styles in Panda:

## Runtime values

Using a value that is not statically analyzable at build-time will not work in Panda due to the inability to determine
the style values.

```tsx filename="App.tsx"
import { useState } from 'react'
import { css } from '../styled-system/css'

const App = () => {
  const [color, setColor] = useState('red.300')

  return (
    <div
      className={css({
        // ❌ Avoid: Panda can't determine the value of color at build-time
        color
      })}
    />
  )
}
```

The example above will not work because Panda can't determine the value of `color` at build-time. Here are some ways to
fix this:

### Using Static CSS

Panda supports a [`staticCss`](/docs/guides/static) option in the config you can use to pre-generate some styles ahead
of time.

```tsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  staticCss: {
    css: [
      {
        properties: {
          // ✅ Good: Pre-generate the styles for the color
          color: ['red.300']
        }
      }
    ]
  }
})
```

```tsx filename="Button.tsx"
import { useState } from 'react'
import { styled } from '../styled-system/jsx'

export const Button = () => {
  const [color, setColor] = useState('red.300')

  // ✅ Good: This will work because `red.300` is pre-generated using `staticCss` config
  return <styled.button color={color} />
}
```

### Using `token()`

The `token()` function is generated by Panda and contains an object of all tokens by dot-path, allowing you to query for
token's raw value at runtime.

```tsx filename="App.tsx"
import { useState } from 'react'
import { css } from '../styled-system/css'
import { token } from '../styled-system/tokens'

const Component = props => {
  return (
    <div
      className={css({
        // ✅ Good: Store the value in a CSS custom property
        color: 'var(--color)'
      })}
      style={{
        // ✅ Good: Handle the runtime value in the style attribute
        '--color': token(`colors.${props.color}`)
      }}
    >
      Dynamic color with runtime value
    </div>
  )
}

// App.tsx
const App = () => {
  const [runtimeColor, setRuntimeColor] = useState('pink.300')

  return <Component color={runtimeColor} />
}
```

### Using `token.var()`

You could also directly use the `token.var()` function to get a reference to the underlying CSS custom property for a
given token:

```tsx filename="App.tsx"
import { useState } from 'react'
import { token } from '../styled-system/tokens'

const Component = props => {
  return (
    <div
      style={{
        // ✅ Good: Dynamically generate CSS custom property from the token
        color: token.var(`colors.${props.color}`)
      }}
    >
      Dynamic color with runtime value
    </div>
  )
}

const App = () => {
  const [runtimeColor, setRuntimeColor] = useState('yellow.300')

  return <Component color={runtimeColor} />
}
```

## JSX Style Props

Panda supports forwarding JSX style properties to any element in your codebase.

For example, let's say we create a Card component that accepts a `color` prop:

```tsx filename="Card.tsx"
import { styled } from '../styled-system/jsx'

const Card = props => {
  return <styled.div px="4" py="3" {...props} />
}
```

Then you add more style props to the Card component in a different file:

```tsx filename="App.tsx"
const App = () => {
  return (
    <Card color="blue.300">
      <p>Some content</p>
    </Card>
  )
}
```

As long as all prop-value pairs are statically extractable, Panda will automatically generate the CSS, so avoid using
runtime values:

```tsx filename="App.tsx"
import { useState } from 'react'

const App = () => {
  const [color, setColor] = useState('blue.300')

  // ❌ Avoid: Panda can't determine the value of color at build-time
  return (
    <Card color={color}>
      <p>Some content</p>
    </Card>
  )
}
```

## Property Renaming

Due to the static nature of Panda, you can't rename properties at runtime.

```tsx filename="App.tsx"
import { Circle, CircleProps } from '../styled-system/jsx'

type Props = {
  circleSize?: CircleProps['size']
}

const CustomCircle = (props: Props) => {
  const { circleSize = '3' } = props
  return (
    <Circle
      // ❌ Avoid: Panda can't determine the value of circleSize at build-time
      size={circleSize}
    />
  )
}
```

In this case, you need to use the `size` prop.

### Alternative

As of v0.8, we added a new `{fn}.raw()` method to css, patterns and recipes. This function is an identity function and
only serves as a hint for the compiler to extract the css.

It can be useful, for example, in Storybook args or custom react props.

```tsx filename="App.tsx"
// mark the object as valid css for the extractor
<Button rootProps={css.raw({ bg: 'red.400' })} />
```

```tsx
export const Funky: Story = {
  // mark this as a button recipe usage
  args: button.raw({
    visual: 'funky',
    shape: 'circle',
    size: 'sm'
  })
}
```

### Enhanced `css.raw` spreading

> **Added in v1.6.1**

You can also spread `css.raw` objects within nested selectors and conditions for better style composition:

```tsx filename="App.tsx"
import { css } from '../styled-system/css'

const baseStyles = css.raw({ margin: 0, padding: 0 })
const interactive = css.raw({ cursor: 'pointer', transition: 'all 0.2s' })

const component = css({
  // Spreading in child selectors
  '& p': { ...baseStyles, fontSize: '1rem' },

  // Spreading in nested conditions
  _hover: {
    ...interactive,
    _dark: { ...interactive, color: 'white' }
  }
})
```

## Static expressions

Panda supports static expressions in your styles, as long as they are statically analyzable.

### Static Composition

You can compose different style objects together using the `css.raw()` function.

```tsx filename="App.tsx"
import { css } from 'styled-system/css'

const paragraphSpacingStyle = css.raw({
  '& p': { marginBlockEnd: '1em' }
})

export const proseCss = css.raw({
  '& h1': paragraphSpacingStyle
})
```

This will result in the following CSS:

```css
/* ... */
@layer utilities {
  .\[\&_p\]\:mb_1em p,
  .\[\&_h1\]\:\[\&_p\]\:mb_1em h1 p {
    margin-block-end: 1em;
  }
}
```

### Static Expressions

Panda supports the use of functions to generate the style objects as long they are statically analyzable.

You can only use functions that are defined in the ECMAScript spec such as `Math`, `Object`, `Array`, etc, to support
the evaluation of basic expressions like this:

```ts
import { cva } from '.panda/css'

const getVariants = () => {
  const spacingTokens = Object.entries({
    sm: 'token(spacing.1)',
    md: 'token(spacing.2)'
  })

  // Generate variants programmatically
  const variants = spacingTokens.map(([variant, token]) => [variant, { paddingX: token }])
  return Object.fromEntries(variants)
}

const baseStyle = cva({
  variants: {
    variant: getVariants()
  }
})
```

This will generate the following variants object:

```json
{
  "sm": { "paddingX": "token(spacing.1)" },
  "md": { "paddingX": "token(spacing.2)" }
}
```

And the following CSS

```css
@layer utilities {
  .px_token\(spacing\.1\) {
    padding-inline: var(--spacing-1);
  }

  .px_token\(spacing\.2\) {
    padding-inline: var(--spacing-2);
  }
}
```

## Runtime conditions

Even though we recommend that you first look for better alternatives (such as using
[recipe variants](/docs/concepts/recipes)), you may still occasionally need runtime conditions.

When encountering a runtime condition, Panda will first try to resolve it statically. If it can't, it will fallback to
the generating the corresponding CSS for each possible branches.

```tsx
import { useState } from 'react'
import { css } from '../styled-system/css'
import { Stack } from '../styled-system/jsx'

const App = () => {
  const [isHovered, setIsHovered] = useState(false)

  return (
    <Stack
      color={isHovered ? { _hover: 'red.100' } : 'red.200'}
      _hover={{
        color: { base: 'red.300', md: isHovered ? 'red.400' : undefined }
      }}
    >
      <div className={css({ color: isHovered ? 'red.500' : 'red.600' })} />
    </Stack>
  )
}
```

Since none of the conditions above are statically extractable, Panda will generate css for all possible code path,
resulting in a css that looks like this:

```css
/* ... */
@layer utilities {
  .hover\:text_red\.100:where(:hover, [data-hover]) {
    color: var(--colors-red-100);
  }

  .text_red\.200 {
    color: var(--colors-red-200);
  }

  .hover\:text_red\.300:where(:hover, [data-hover]) {
    color: var(--colors-red-300);
  }

  @media screen and (min-width: 768px) {
    .hover\:md\:text_red\.400:where(:hover, [data-hover]) {
      color: var(--colors-red-400);
    }
  }

  .text_red\.500 {
    color: var(--colors-red-500);
  }

  .text_red\.600 {
    color: var(--colors-red-600);
  }
}
/* ... */
```

## Referenced values

Although you should have your styles inlined most of the time, maybe you want to store a value in a variable and re-use
in multiple places. This should be fine as long as you keep it statically analyzable.

Here's a short list of things to avoid:

- Variables that are not defined in the same file
- Variables resulting from a function call (e.g. `const color = getColor()`)

> If you don't know what value a variable holds with a quick glance, Panda won't be able to either.

```tsx
import { css } from '../styled-system/css'

// ✅ Good: All values are statically extractable
const mainColor = 'red.300'
const sizes = { sm: '12px', md: '16px', '2xl': '42px' }

const App = () => {
  return (
    <div
      className={css({
        color: mainColor,
        fontSize: sizes.md,
        width: sizes['2xl']
      })}
    />
  )
}
```

### Runtime reference on known objects

Using a more complex but still common example :

```tsx
import { useState } from 'react'
import { css } from '../styled-system/css'

const colorByType = {
  primary: 'red.300',
  secondary: 'blue.300',
  tertiary: 'green.300'
}

const Section = () => {
  const [type, setType] = useState('primary')

  // ❌ Avoid: since only "gray.100" is statically extractable here
  // This will not work as expected, the color CSS won't be generated
  return <section className={css({ color: colorByType[type] ?? 'gray.100' })}>❌ Will not be extracted</section>
}
```

Even though `colorByType` is statically analyzable, Panda does not _yet_ support this kind of automatic extraction
fallback. This is the perfect opportunity to use the [recipes](/docs/concepts/recipes).

```tsx
import { useState } from 'react'
import { cva } from '../styled-system/cva'

const sectionRecipe = cva({
  base: { color: 'gray.100' },
  variants: {
    type: {
      primary: { color: 'red.300' },
      secondary: { color: 'blue.300' },
      tertiary: { color: 'green.300' }
    }
  }
})

const Section = () => {
  const [type, setType] = useState('primary')

  // ✅ Good: This will work as expected
  return <section className={sectionRecipe({ type })}>✅ With a recipe</section>
}
```

Not only did you get the same end result, but you also got a more readable and maintainable code !

You can now :

- add more variants to your recipe
- add more properties
- use a shorthand or a condition

All of this with complete type-safety and without having to make drastic changes to the component.

> Note that you can also [integrate this recipe directly into your theme](/docs/concepts/recipes) if you want to only
> generate the CSS that you use, among other things

## Summary

### What you can do

```tsx
// ✅ Good: Conditional styles
<styled.div color={{ base: "red.100", md: "red.200" }} />

// ✅ Good: Arbitrary value
<styled.div color="#121qsd" />

// ✅ Good: Arbitrary selector
<styled.div css={{ "&[data-thing] > span": { color: "red.100" } }} />

// ✅ Good: Runtime value (with config.`staticCss`)
const Button = () => {
  const [color, setColor] = useState('red.300')
  return <styled.button color={color} />
}

// ✅ Good: Runtime condition
<styled.div color={{ base: "red.100", md: isHovered ? "red.200" : "red.300" }} />

// ✅ Good: Referenced value
<styled.div color={mainColor} />

```

### What you can't do

```tsx
// ❌ Avoid: Runtime value (without config.`staticCss`)
const Button = () => {
  const [color, setColor] = useState('red.300')
  return <styled.button color={color} />
}

// ❌ Avoid: Referenced value (not statically analyzable or from another file)
<styled.div color={getColor()} />
<styled.div color={colors[getColorName()]} />
<styled.div color={colors[colorFromAnotherFile]} />

const CustomCircle = (props) => {
  const { circleSize = '3' } = props
  return (
    <Circle
      // ❌ Avoid: Panda can't determine the value of circleSize at build-time
      size={circleSize}
    />
  )
}
```

---


## Custom Font

How to use custom fonts in your project.

Adding custom fonts to your application or website is a typical requirement for projects. Panda recommends using custom
fonts through CSS variables for consistency.

## Setup

### Next.js

Next.js provides a built-in automatic self-hosting for any font file by using the `next/font` module. It allows you to
conveniently use all Google Fonts and any local font with performance and privacy in mind.

Here's an example of how to load a local "Mona Sans" font and a Google Font "Fira Code" in your Next.js project.

```js filename="styles/font.ts"
import { Fira_Code } from 'next/font/google'
import localFont from 'next/font/local'

export const MonaSans = localFont({
  src: '../fonts/Mona-Sans.woff2',
  display: 'swap',
  variable: '--font-mona-sans'
})

export const FiraCode = Fira_Code({
  weight: ['400', '500', '700'],
  display: 'swap',
  subsets: ['latin'],
  variable: '--font-fira-code'
})
```

> Ideally, you should load the font in the layout file.

Next, you need to add the font variables to your HTML document. You can do this using either the App Router or the Pages
Router.

#### App Router

```jsx filename="app/layout.tsx"
import { FiraCode, MonaSans } from '../styles/font'

export default function Layout(props) {
  const { children } = props
  return (
    <html className={`${MonaSans.variable} ${FiraCode.variable}`}>
      <body>{children}</body>
    </html>
  )
}
```

> **Note 🚨:** By default, Next.js attaches the className for the fonts to the `<body>` element, for panda to
> appropriately load fonts, update the code to attach the `className` to the `<html>` element.

#### Pages Router

```jsx filename="pages/_app.tsx"
import { FiraCode, MonaSans } from '../styles/font'

export default function App({ Component, pageProps }) {
  return (
    <>
      <style jsx global>
        {`
          :root {
            --font-mona-sans: ${MonaSans.style.fontFamily};
            --font-fira-code: ${FiraCode.style.fontFamily};
          }
        `}
      </style>
      <Component {...pageProps} />
    </>
  )
}
```

### Fontsource

[Fontsource](https://fontsource.org/) streamlines the process of integrating fonts into your web application.

To begin, install your desired font package:

```bash
pnpm add @fontsource-variable/fira-code
```

Next, import the font into your project:

```jsx
import '@fontsource-variable/fira-code'
```

Lastly, create a variable to use it as a token in the panda config

```css filename="styles/font.css"
:root {
  --font-fira-code: 'Fira Code Variable', monospace;
}
```

### Vanilla CSS

You can leverage the native font-face CSS property to load custom fonts in your project.

```css
@font-face {
  font-family: 'Mona Sans';
  src: url('../fonts/Mona-Sans.woff2') format('woff2');
  font-weight: 400;
  font-style: normal;
  font-display: swap;
}
```

Then alias the font names to css variables.

```css
:root {
  --font-mona-sans: 'Mona Sans', sans-serif;
}
```

### Global Font Face

You can also define global font face in your panda config.

```js
export default defineConfig({
  globalFontface: {
    Fira: {
      src: 'url(/fonts/fira.woff2) format("woff2")',
      fontWeight: 400,
      fontStyle: 'normal',
      fontDisplay: 'swap'
    }
  }
})
```

You can also define multiple font sources for the same weight.

```js
export default defineConfig({
  globalFontface: {
    Fira: {
      src: ['url(/fonts/fira.woff2) format("woff2")', 'url(/fonts/fira.woff) format("woff")'],
      fontWeight: 400,
      fontStyle: 'normal',
      fontDisplay: 'swap'
    }
  }
})
```

You can also define multiple font weights.

```js
export default defineConfig({
  globalFontface: {
    Fira: [
      {
        src: 'url(/fonts/fira.woff2) format("woff2")',
        fontWeight: 400,
        fontStyle: 'normal',
        fontDisplay: 'swap'
      },
      {
        src: 'url(/fonts/fira-bold.woff2) format("woff2")',
        fontWeight: 700,
        fontStyle: 'normal',
        fontDisplay: 'swap'
      }
    ]
  }
})
```

Then expose the font names to css variables.

```css
:root {
  --font-fira-code: 'Fira Code Variable', monospace;
}
```

You can also use [globalVars](/docs/concepts/writing-styles#global-vars) in your panda config to define the variables.

```js
export default defineConfig({
  globalVars: {
    '--font-fira-code': 'Fira Code Variable, monospace'
  }
})
```

## Update Panda Config

```js
export default defineConfig({
  theme: {
    extend: {
      tokens: {
        fonts: {
          fira: { value: 'var(--font-fira-code), Menlo, monospace' },
          mona: { value: 'var(--font-mona-sans), sans-serif' }
        }
      }
    }
  }
})
```

## Use the custom fonts

```jsx
import { css } from '../styled-system/css'

function Page() {
  return (
    <div>
      <h1 className={css({ fontFamily: 'mona' })}>Mona Sans</h1>
      <code className={css({ fontFamily: 'fira' })}>Fira Code</code>
    </div>
  )
}
```

---


## Minimal Setup

How to set up Panda with the bare minimum.

The default Panda setup includes utilities and design tokens by default. In this guide, you'll see how to strip out the
defaults and start from scratch.

## Removing default tokens

To remove the default design tokens injected by Panda, set the `presets` key to an empty array:

```js
export default defineConfig({
  // ...
  presets: []
})
```

This allows you to define your own tokens, without having to use the `extend` key in the theme.

```js
export default defineConfig({
  // ...
  theme: {
    tokens: {
      colors: {
        primary: { value: '#ff0000' }
      }
    }
  }
})
```

## Removing default utilities

Use the `eject: true` property to remove all the default utilities.

Panda doesn't automatically know which tokens are valid for which CSS properties, so it is necessary to tell Panda that
my tokens from the "colors" category are valid for the CSS property "color".

```js
export default defineConfig({
  // ...
  eject: true,
  utilities: {
    color: {
      values: 'colors'
    }
  },
  theme: {
    tokens: {
      colors: {
        primary: { value: '#ff0000' }
      }
    }
  }
})
```

This makes `<p className={css({ color: 'primary' })}> Text </p>` work as expected.

## Re-using Panda presets

Panda offers 2 presets:

- `@pandacss/preset-base`: This is a relatively unopinionated set of utilities for mapping CSS properties to values
  (almost everyone will want these)

You can use these presets by installing them via npm and adding them to your `presets` key:

```js
export default defineConfig({
  // ...
  presets: ['@pandacss/preset-base']
})
```

- `@pandacss/preset-panda` as an opinionated set of tokens if you don't want to define your own colors/spacing/fonts
  etc.

```js
export default defineConfig({
  // ...
  presets: ['@pandacss/preset-panda']
})
```

> Note: You don't need to install `@pandacss/preset-base` or `@pandacss/preset-panda`. Panda will automatically resolve
> them for you.

---


## Multi-Theme Tokens

Panda supports advance token definition beyond just light/dark mode; theming beyond just dark mode. You can define multi-theme tokens using nested conditions.

## Multi-Theme Tokens

Panda supports advance token definition beyond just light/dark mode; theming beyond just dark mode. You can define
multi-theme tokens using nested conditions.

Let's say your application supports a pink and blue theme, and each theme can have a light and dark mode. Let's see how
to model this in Panda.

We'll start by defining the following conditions for these theme and color modes:

```js
// panda.config.ts
const config = {
  conditions: {
    light: '[data-color-mode=light] &',
    dark: '[data-color-mode=dark] &',
    pinkTheme: '[data-theme=pink] &',
    blueTheme: '[data-theme=blue] &'
  }
}
```

> Conditions are a way to provide preset css selectors or media queries for use in your Panda project

Next, we'll define a `colors.text` semantic token for the pink and blue theme.

```js
// panda.config.ts
const theme = {
  // ...
  semanticTokens: {
    colors: {
      text: {
        value: {
          _pinkTheme: '{colors.pink.500}',
          _blueTheme: '{colors.blue.500}'
        }
      }
    }
  }
}
```

Next, we'll modify `colors.text` to support light and dark color modes for each theme.

```js
// panda.config.ts
const theme = {
  // ...
  semanticTokens: {
    colors: {
      text: {
        value: {
          _pinkTheme: { base: '{colors.pink.500}', _dark: '{colors.pink.300}' },
          _blueTheme: { base: '{colors.blue.500}', _dark: '{colors.blue.300}' }
        }
      }
    }
  }
}
```

Now, you can use the `text` token in your styles, and it will automatically change based on the theme and the color
scheme.

```jsx
// use pink and dark mode theme
<html data-theme="pink" data-color-mode="dark">
  <body>
    <h1 className={css({ color: 'text' })}>Hello World</h1>
  </body>
</html>

// use pink and light mode theme
<html data-theme="pink">
  <body>
    <h1 className={css({ color: 'text' })}>Hello World</h1>
  </body>
</html>
```

## Multi-Themes

The above example shows you how to define multi-theme tokens using nested conditions but you can also define clearly separated themes using the `themes` property in the config.

This allows you to apply a `theme` on multiple tokens at once, using data attributes and CSS variables.

> Theme variants can be applied using the `data-panda-theme` attribute with the theme key as the value.

```ts
// panda.config.ts
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  // main theme
  theme: {
    extend: {
      tokens: {
        colors: {
          text: { value: 'green' }
        }
      },
      semanticTokens: {
        colors: {
          body: {
            value: {
              base: '{colors.green.600}',
              _osDark: '{colors.green.400}'
            }
          }
        }
      }
    }
  },
  // alternative theme variants
  themes: {
    primary: {
      tokens: {
        colors: {
          text: { value: 'red' }
        }
      },
      semanticTokens: {
        colors: {
          muted: { value: '{colors.red.200}' },
          body: {
            value: {
              base: '{colors.red.600}',
              _osDark: '{colors.red.400}'
            }
          }
        }
      }
    },
    secondary: {
      tokens: {
        colors: {
          text: { value: 'blue' }
        }
      },
      semanticTokens: {
        colors: {
          muted: { value: '{colors.blue.200}' },
          body: {
            value: {
              base: '{colors.blue.600}',
              _osDark: '{colors.blue.400}'
            }
          }
        }
      }
    }
  }
})
```

### Pregenerating themes

By default, no additional theme variant is generated, you need to specify the specific themes you want to generate in
`staticCss.themes` to include them in the CSS output.

```ts
// panda.config.ts
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  staticCss: {
    themes: ['primary', 'secondary']
  }
})
```

This will generate the following CSS:

```css
@layer tokens {
  :where(:root, :host) {
    --colors-text: blue;
    --colors-body: var(--colors-blue-600);
  }

  [data-panda-theme='primary'] {
    --colors-text: red;
    --colors-muted: var(--colors-red-200);
    --colors-body: var(--colors-red-600);
  }

  @media (prefers-color-scheme: dark) {
    :where(:root, :host) {
      --colors-body: var(--colors-blue-400);
    }

    [data-panda-theme='primary'] {
      --colors-body: var(--colors-red-400);
    }
  }
}
```

### Dynamically importing themes

An alternative way of applying a theme is by using the new `styled-system/themes` entrypoint where you can import the
themes expected CSS and apply them in your app.

> ℹ️ The `styled-system/themes` will always contain every themes (tree-shaken if not used), whereas `staticCss.themes` only
> applies to the CSS output.

Each theme has a corresponding JSON file with a similar structure:

```json
{
  "name": "primary",
  "id": "panda-themes-primary",
  "css": "[data-panda-theme=primary] { ... }"
}
```

#### Dynamically import a theme using its name

```ts
import { getTheme } from '../styled-system/themes'

const theme = await getTheme('red')
//    ^? {
//     name: "red";
//     id: string;
//     css: string;
// }
```

#### Inject the theme styles into the DOM:

```ts
import { injectTheme } from '../styled-system/themes'

const theme = await getTheme('red')
injectTheme(document.documentElement, theme) // this returns the injected style element
```

#### SSR example with NextJS:

```tsx
// app/layout.tsx
import { cookies } from 'next/headers'
import './globals.css'
import { ThemeName, getTheme } from '../../styled-system/themes'

export default async function RootLayout({
  children
}: {
  children: React.ReactNode
}) {
  const store = cookies()
  const themeName = store.get('theme')?.value as ThemeName
  const theme = themeName && (await getTheme(themeName))

  return (
    <html lang="en" data-panda-theme={themeName ? themeName : undefined}>
      {themeName && (
        <head>
          <style
            type="text/css"
            id={theme.id}
            dangerouslySetInnerHTML={{ __html: theme.css }}
          />
        </head>
      )}
      <body>{children}</body>
    </html>
  )
}
```

```tsx
// app/page.tsx
'use client'
import { getTheme, injectTheme } from '../../styled-system/themes'

export default function Home() {
  return (
    <>
      <button
        onClick={async () => {
          const current = document.documentElement.dataset.pandaTheme
          const next = current === 'primary' ? 'secondary' : 'primary'
          const theme = await getTheme(next)
          setCookie('theme', next, 7)
          injectTheme(document.documentElement, theme)
        }}
      >
        swap theme
      </button>
    </>
  )
}

// Set a Cookie
function setCookie(cName: string, cValue: any, expDays: number) {
  let date = new Date()
  date.setTime(date.getTime() + expDays * 24 * 60 * 60 * 1000)
  const expires = 'expires=' + date.toUTCString()
  document.cookie = cName + '=' + cValue + '; ' + expires + '; path=/'
}
```

### Theme contract

Finally, you can create a theme contract to ensure that all themes have the same structure:

```ts
import { defineThemeContract } from '@pandacss/dev'

const defineTheme = defineThemeContract({
  tokens: {
    colors: {
      red: { value: '' } // theme implementations must have a red color
    }
  }
})

defineTheme({
  tokens: {
    colors: {
      // ^^^^ ❌  Property 'red' is missing in type '{}' but required in type '{ red: { value: string; }; }'
      //
      // ✅ fixed with
      // red: { value: 'red' },
    }
  }
})
```

---


## Static CSS Generator

Panda can be used to generate a static set of utility classes for your project.

Panda can be used to generate a static set of utility classes for your project.

This is useful if you want to use Panda in an HTML project or you want absolute zero runtime.

## Usage

To generate a static set of CSS classes, add them to your `panda.config.js` file:

```js
export default {
  staticCss: {
    // the css properties you want to generate
    css: [],
    // the recipes you want to generate
    recipes: {}
  }
}
```

The `static` property supports two properties:

- `css` - an array of CSS properties you want to generate with their `conditions`
- `recipes` - the component recipes you want to generate

## Generating CSS Properties

The `css` property is an array of CSS properties you want to generate with their `conditions`.

You can specify the following options:

- `properties`: the CSS properties you want to generate
- `conditions`: the CSS conditions or selectors you want to generate in addition to the default values. Values can be
  `light, dark`, etc.
- `responsive`: whether or not to generate responsive classes
- `values`: the values you want to generate for the CSS property. When set to `*`, all values defined in the tokens will
  be included. When set to an array, only the values in the array will be generated.

```js
export default {
  staticCss: {
    css: [
      {
        properties: {
          margin: ['*'],
          padding: ['*', '50px', '80px']
        },
        responsive: true
      },
      {
        properties: {
          color: ['*'],
          backgroundColor: ['green.200', 'red.400']
        },
        conditions: ['light', 'dark']
      }
    ]
  }
}
```

## Generating Recipes

The `recipes` property is an object of component recipes you want to generate with their `conditions`.

```js
export default {
  staticCss: {
    recipes: {
      button: [
        {
          size: ['sm', 'md'],
          responsive: true
        },
        { variant: ['*'] }
      ],
      // shorthand for all variants
      tooltip: ['*']
    }
  }
}
```

You can also directly specify a recipe's `staticCss` rules from inside a recipe config, e.g.:

```js
import { defineRecipe } from '@pandacss/dev'

const card = defineRecipe({
  className: 'card',
  base: { color: 'white' },
  variants: {
    size: {
      small: { fontSize: '14px' },
      large: { fontSize: '18px' }
    }
  },
  staticCss: [{ size: ['*'] }]
})
```

would be the equivalent of defining it inside the main config:

```js
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  staticCss: {
    recipes: {
      card: {
        size: ['*']
      }
    }
  }
})
```

Or you could even generate the CSS for every config `recipe` / `slotRecipes` (and each of their variants):

```tsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  staticCss: {
    recipes: '*'
  }
})
```

This is mostly useful for testing purposes with [`Storybook`](/docs/installation/storybook).

## Performance Considerations

Panda provides intelligent caching and memoization to optimize static CSS generation. However, **pre-generating large
numbers of styles can still impact build times**. It's important to be selective and only generate the styles you
actually need.

### Best Practices

**❌ Avoid: Generating every possible combination**

```js
export default {
  staticCss: {
    css: [
      {
        conditions: ['hover', 'focus', 'active', 'disabled'],
        properties: {
          // This expands to ALL tokens - very expensive!
          color: ['*'],
          backgroundColor: ['*'],
          borderColor: ['*'],
          width: ['*'],
          height: ['*']
          // ... 20+ more properties with wildcards
        }
      }
    ]
  }
}
```

**✅ Better: Only generate what you need**

```js
export default {
  staticCss: {
    css: [
      {
        conditions: ['_hover', '_focus'],
        properties: {
          // Only the colors you actually use
          color: ['red.500', 'blue.500', 'gray.600'],
          backgroundColor: ['white', 'gray.50', 'blue.50'],
          borderColor: ['gray.200', 'blue.500']
        }
      }
    ]
  }
}
```

### When to Use Wildcards

Wildcards (`['*']`) are appropriate when:

- **Small token sets**: Properties with < 20 values (e.g., `fontWeight: ['*']`)
- **Critical utilities**: Styles you genuinely need in all variants
- **Testing scenarios**: Storybook or visual regression testing

### Use Responsive Selectively

The `responsive` property multiplies the number of generated classes by your breakpoints. Only enable it for properties
that genuinely need responsive behavior.

**❌ Avoid: Responsive for all properties**

```js
export default {
  staticCss: {
    css: [
      {
        // This generates classes for ALL breakpoints (sm, md, lg, xl, 2xl)
        responsive: true,
        properties: {
          color: ['red.500', 'blue.500'],
          backgroundColor: ['white', 'gray.50'],
          fontWeight: ['400', '500', '600'],
          borderRadius: ['sm', 'md', 'lg']
        }
      }
    ]
  }
}
```

**✅ Better: Responsive only for layout properties**

```js
export default {
  staticCss: {
    css: [
      {
        // Responsive for layout properties that change across breakpoints
        responsive: true,
        properties: {
          display: ['none', 'block', 'flex'],
          flexDirection: ['row', 'column'],
          width: ['full', '1/2', '1/3']
        }
      },
      {
        // No responsive needed for colors/typography that stay the same
        properties: {
          color: ['red.500', 'blue.500'],
          fontWeight: ['400', '500', '600']
        }
      }
    ]
  }
}
```

Properties that commonly need `responsive: true`:

- Layout: `display`, `flexDirection`, `gridTemplateColumns`
- Sizing: `width`, `height`, `maxWidth`
- Spacing: `padding`, `margin`, `gap`
- Positioning: `position`, `top`, `left`

Properties that rarely need `responsive: true`:

- Colors: `color`, `backgroundColor`, `borderColor`
- Typography: `fontWeight`, `textDecoration`, `fontFamily`
- Effects: `boxShadow`, `opacity`, `cursor`

## Removing unused CSS

For an even smaller css output size, you can utilize [PurgeCSS](https://purgecss.com/) to treeshake and remove unused
CSS. This tool will analyze your template and match selectors against your CSS.

---


