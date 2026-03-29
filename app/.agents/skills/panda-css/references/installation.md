# Installation

Use this reference for framework setup, PostCSS or CLI integration, and framework-specific troubleshooting.

## Using Angular

Easily use Panda with Angular with our dedicated integration.

This guide shows you how to set up Panda CSS in an Angular project using PostCSS.

## Start a new project

<Steps>

### Create Vite project

To get started, we will need to create a new Angular project using the official
[scaffolding tool](https://angular.dev/tools/cli).

If you don't enter any parameter, the CLI will guide you through the process of creating a new Angular app.

```bash
ng new test-app
```

You will be asked a few questions, answer them as follows:

```bash
? Which stylesheet format would you like to use? CSS
? Do you want to enable Server-Side Rendering (SSR) and Static Site Generation (SSG/Prerendering)? No
```

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Configure PostCSS

Create a `postcss.config.json` file in the root of your project and add the following code:

```json filename="postcss.config.json"
{
  "plugins": {
    "@pandacss/dev/postcss": {}
  }
}
```

> You must use a JSON file for the PostCSS configuration, as the Angular CLI does not support JavaScript PostCSS
> configuration files.

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "ng": "ng",
    "start": "ng serve",
    "build": "ng build",
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Angular components are included in the `include` section of the
`panda.config.ts` file.

```js {8,17} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./src/**/*.{js,jsx,ts,tsx}'],

  // Files to exclude
  exclude: [],

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `src/index.css` file and import it in the root component of your project.

```css filename="src/index.css"
@layer reset, base, tokens, recipes, utilities;
```

## Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your
`src/app.component.ts` file.

```typescript filename="src/app.component.ts"
import { Component } from '@angular/core'
import { css } from '../styled-system/css'

@Component({
  selector: 'app-root',
  standalone: true,
  template: ` <div [class]="redBg"></div> `
})
export class App {
  redBg = css({ bg: 'red.400' })
}
```

</Steps>

---


## Using Astro

Easily use Panda with Astro with our dedicated integration.

This guide shows you how to set up Panda CSS in an Astro project using our dedicated integration.

## Setup

<Steps>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3}
{
  "scripts": {
+   "prepare": "panda codegen",
    "dev": "astro dev",
    "start": "astro start",
    "build": "astro build",
    "preview": "astro preview"
  }
}
```

    The `prepare` script that will run codegen after dependency installation. Read more about [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Add your panda config to your `panda.config.js` file, or wherever panda is configured in your project.

```js {6}
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  preflight: true,
  // define the content to scan 👇🏻
  include: ['./src/**/*.{ts,tsx,js,jsx,astro}', './pages/**/*.{ts,tsx,js,jsx,astro}'],
  exclude: [],
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add the layer css code to the `src/index.css` file

```css filename="src/index.css"
@layer reset, base, tokens, recipes, utilities;
```

Then, import the `src/index.css` file in your page or layout file

```md filename="src/pages/index.astro"
---
import '../index.css';
---
```

### Update the postcss config

Astro requires a little change for the `postcss.config.[c]js`:

```diff {3} filename="postcss.config.js"
module.exports = {
-  plugins: {
-   '@pandacss/dev/postcss': {}
-  }
+  plugins: [require('@pandacss/dev/postcss')()]
}
```

### Start your build process

Run your build process with `npm run dev` or whatever command is configured in your package.json file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Use the generated style utilities in your code, and panda will extract them to the generated CSS file.

```jsx
---
import { css } from '../../styled-system/css';
---
<div class={css({ fontSize: "2xl", fontWeight: 'bold' })}>Hello !</div>
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Panda CLI

An alternative way to use Panda is by running the Panda CLI tool.

This guide shows you how to use Panda as an alternative approach by running the Panda CLI tool.

<Steps>

### Install Panda

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Configure the content

Add the paths to all of your JavaScript or TypeScript code where you intend to use panda.

```js {5}
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  preflight: true,
  include: ['./src/**/*.{ts,tsx,js,jsx}', './pages/**/*.{ts,tsx,js,jsx}'],
  exclude: [],
  outdir: 'styled-system'
})
```

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3}
{
  "scripts": {
+    "prepare": "panda codegen",
  }
}
```

The `prepare` script that will run codegen after dependency installation. Read more about
[codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Import the generated CSS

For each Panda run, it emits the generated CSS at the `styled-system/styles.css` file path. Import this file at the root
component of your project.

```jsx {1}
import './styled-system/styles.css'

export function App() {
  return <div>Page</div>
}
```

### Start the Panda build process

Run the CLI tool to scan your JavaScript and TypeScript files for style properties and call expressions.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    # Run it once
    pnpm panda

    # Run it in watch mode
    pnpm panda --watch
    ```

  </Tab>
  <Tab>
    ```bash
    # Run it once
    npx panda

    # Run it in watch mode
    npx panda --watch
    ```

  </Tab>
  <Tab>
    ```bash
    # Run it once
    yarn panda

    # Run it in watch mode
    yarn panda --watch
    ```

  </Tab>
  <Tab>
    ```bash
    # Run it once
    bun panda

    # Run it in watch mode
    bun panda --watch
    ```

  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Use the generated style utilities in your code and panda will extract them to the generated CSS file. Then run your
build process.

```jsx
import { css } from './styled-system/css'

export function App() {
  return <div className={css({ bg: 'red.400' })} />
}
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using Ember

Easily use Panda with Ember with our dedicated integration.

This guide shows you how to set up Panda CSS in an Ember project using PostCSS.

## Start a new project

<Steps>

### Create Ember project

To get started, we will need to create a new Ember project using the `embroider` build system. We will name our project
`test-app` but you can name it whatever you want.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']} variant="code">
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dlx ember-cli@latest new test-app --embroider --no-welcome --typescript --pnpm
    ```
  </Tab>
  <Tab>
    ```bash
    npx ember-cli@latest new test-app --embroider --no-welcome --typescript
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dlx ember-cli@latest new test-app --embroider --no-welcome --typescript --yarn
    ```
  </Tab>
  <Tab>
    ```bash
    bunx ember-cli@latest new test-app --embroider --no-welcome --typescript --skip-install
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

Enter the newly created directory:

```bash
cd test-app
```

### Install Panda

Install panda and its peer dependencies, as well as `postcss-loader`. Run the init command to generate the
`panda.config.ts` and `postcss.config.js` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev postcss postcss-loader
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev postcss postcss-loader
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev postcss postcss-loader
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev postcss postcss-loader
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Enable PostCSS support

In your `ember-cli-build.js` file, configure PostCSS to process your CSS files.

```js {12-23} filename="ember-cli-build.js"
'use strict'

const EmberApp = require('ember-cli/lib/broccoli/ember-app')

module.exports = function (defaults) {
  const app = new EmberApp(defaults, {
    // Add options here
  })

  const { Webpack } = require('@embroider/webpack')
  return require('@embroider/compat').compatBuild(app, Webpack, {
    packagerOptions: {
      webpackConfig: {
        module: {
          rules: [
            {
              test: /\.css$/i,
              use: ['postcss-loader']
            }
          ]
        }
      }
    }
    // other options...
  })
}
```

### Configure the PostCSS plugin

Add the `.embroider` folder to the allow list so the Panda PostCSS plugin picks up your app CSS files.

```js {4} filename="postcss.config.cjs"
module.exports = {
  plugins: {
    '@pandacss/dev/postcss': {
      allow: [/node_modules\/.embroider/]
    }
  }
}
```

### Update package.json scripts

Open the `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    // ...
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Ember components are included in the `include` section of the `panda.config.ts`
file. Set the `outdir` to the app folder so the code can be imported in your Ember app. Adjust the `importMap`
accordingly to reflect your app name.

```js {8,19-22} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./app/**/*.{js,ts,gjs,gts}'],

  // Files to exclude
  exclude: [],

  // Useful for theme customization
  theme: {
    extend: {}
  },

  // The output directory for your css system
  outdir: 'app/styled-system',

  // Configure the import map to use your project name
  importMap: 'test-app/styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `app/index.css` file.

```css filename="app/index.css"
@layer reset, base, tokens, recipes, utilities;
```

Next, import the file in your `app/app.ts` file.

```ts {5} filename="app/app.ts"
import Application from '@ember/application'
import Resolver from 'ember-resolver'
import loadInitializers from 'ember-load-initializers'
import config from 'test-app/config/environment'
import 'test-app/index.css'

export default class App extends Application {
  modulePrefix = config.modulePrefix
  podModulePrefix = config.podModulePrefix
  Resolver = Resolver
}

loadInitializers(App, config.modulePrefix)
```

### Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm start
    ```
  </Tab>
  <Tab>
    ```bash
    npm run start
    ```
  </Tab>
  <Tab>
    ```bash
    yarn start
    ```
  </Tab>
  <Tab>
    ```bash
    bun start
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project.

```ts filename="app/components/hello-panda.ts"
import Component from '@glimmer/component'
import { css } from 'test-app/styled-system/css'

export default class HelloPanda extends Component {
  style = css({ fontSize: '5xl', fontWeight: 'bold' })
}
```

```hbs filename="app/components/hello-panda.hbs"
<div class={{this.style}}>Hello 🐼!</div>
```

```hbs {5} filename="app/templates/application.hbs"
{{page-title 'TestApp'}}

<h2 id='title'>Welcome to Ember</h2>

<HelloPanda />

{{outlet}}
```

> For the best developer experience, set up
> [template tag component authoring format](https://guides.emberjs.com/release/components/template-tag-format/) in
> Ember.

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["app/styled-system"]
}
```

---


## Using Gatsby

Easily use Panda with Gatsby with our dedicated integration.

This guide shows you how to set up Panda CSS in a Gatsby project using PostCSS.

<Steps>

### Create Gatsby project

To get started, we will need to create a new Gatsby project. We will name our project `test-app` but you can name it
whatever you want.

If you don't enter any parameter, the CLI will guide you through the process of creating a new Gatsby app.

```bash
npm init gatsby
```

You will be asked a few questions, answer them as follows:

```
✔ What would you like to call your site? ... My Gatsby Site
✔ What would you like to name the folder where your site will be created? ... projects/ test-app
✔ Will you be using JavaScript or TypeScript? ... TypeScript
✔ Will you be using a CMS? ... No (or I'll add it later)
✔ Would you like to install a styling system? ... No (or I'll add it later)
✔ Would you like to install additional features with other plugins? ... No items were selected
```

Enter the newly created directory:

```bash
cd test-app
```

### Install Panda CSS

Install Panda CSS and `gatsby-plugin-postcss` to your project. After that run the `panda init` command to setup Panda
CSS in your project.

```bash
npm install -D @pandacss/dev postcss gatsby-plugin-postcss
npx panda init --postcss
```

### Setup the Gatsby PostCSS plugin

Include the plugin in your `gatsby-config.ts` file. Check out the
[official documentation](https://www.gatsbyjs.com/plugins/gatsby-plugin-postcss/) for more information.

```ts {9} filename="gatsby-config.ts"
import type { GatsbyConfig } from 'gatsby'

const config: GatsbyConfig = {
  siteMetadata: {
    title: `My Gatsby Site`,
    siteUrl: `https://www.yourdomain.tld`
  },
  graphqlTypegen: true,
  plugins: [`gatsby-plugin-postcss`]
}

export default config
```

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "develop": "gatsby develop",
    "start": "gatsby develop",
    "build": "gatsby build",
    "serve": "gatsby serve",
    "clean": "gatsby clean",
    "typecheck": "tsc --noEmit"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your React components are included in the `include` section of the `panda.config.ts`
file.

> If you use [GraphQL Typegen](/docs/how-to/local-development/graphql-typegen/), you'll need to update the `include` to
> avoid infinite loop due to generated `src/gatsby-types.d.ts`.

```js {6} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  preflight: true,
  // Where to look for your css declarations
  include: ['./src/pages/*.{js,jsx,ts,tsx}', './src/components/**/*.{js,jsx,ts,tsx}'],
  exclude: [],
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Create `src/styles/index.css` file and add the following content:

```css filename="src/styles/index.css"
@layer reset, base, tokens, recipes, utilities;
```

### Import the entry CSS

Create a `gatsby-browser.ts` file in the root of your project and add the following content:

```ts filename="gatsby-browser.ts"
import './src/styles/index.css'
```

### Start your build process

Run the following command to start your development server.

```bash
npm run develop
```

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your
`src/pages/index.tsx` file.

```tsx {3,7} filename="src/pages/index.tsx"
import * as React from 'react'
import type { HeadFC, PageProps } from 'gatsby'
import { css } from '../../styled-system/css'

const IndexPage: React.FC<PageProps> = () => {
  return <div className={css({ fontSize: '2xl', fontWeight: 'bold' })}>Hello 🐼!</div>
}

export default IndexPage

export const Head: HeadFC = () => <title>Home Page</title>
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using Next.js

Easily use Panda with Next.js with our dedicated integration.

Setting up Panda CSS with Next.js is straightforward. Follow the steps below to get started.

If you don't have Next.js app installed, you can follow the [Create a Next.js app](#create-a-nextjs-app) section to
create a new Next.js app, otherwise you can skip to the [Install Panda CSS](#install-panda-css) section.

<RouteSwitch values={["app-dir","pages-dir"]}>

## Start a new project

You can chose between these two options to start a new project:

<RouteSwitchTrigger values={['Using App Router', 'Using Pages Router']} />

  <Steps>
  ### Create a Next.js app

First, create a Next.js app using the official [Create Next App](https://nextjs.org/docs/api-reference/create-next-app)
CLI. We will name our project `test-app` but you can name it whatever you want.

If you don't enter any parameter, the CLI will guide you through the process of creating a new Next.js app.

  <Tabs items={['pnpm', 'npm', 'yarn', 'bun']} variant="code">
    {/* <!-- prettier-ignore-start --> */}
    <Tab>
      ```bash
      pnpm dlx create-next-app@latest --use-pnpm
      ```
    </Tab>
    <Tab>
      ```bash
      npx create-next-app@latest --use-npm
      ```
    </Tab>
    <Tab>
      ```bash
      yarn dlx create-next-app@latest --use-yarn
      ```
    </Tab>
    <Tab>
      ```bash
      bunx create next-app@latest --use-bun
      ```
    </Tab>
    {/* <!-- prettier-ignore-end --> */}
  </Tabs>

You will be asked a few questions, answer them as follows:

  <RouteSwitchContent value="app-dir">
  {/* <!-- prettier-ignore-start --> */}
    ```bash
    ✔ What is your project named? … test-app
    ✔ Would you like to use TypeScript with this project? … Yes
    ✔ Would you like to use ESLint with this project? … Yes
    ✔ Would you like to use Tailwind CSS with this project? … No
    ✔ Would you like to use `src/` directory with this project? … Yes
    ✔ Use App Router (recommended)? … Yes
    ✔ Would you like to customize the default import alias? … No
    ```
  {/* <!-- prettier-ignore-end --> */}
  </RouteSwitchContent>
  <RouteSwitchContent value="pages-dir">
  {/* <!-- prettier-ignore-start --> */}
    ```bash
    ✔ What is your project named? … test-app
    ✔ Would you like to use TypeScript with this project? … Yes
    ✔ Would you like to use ESLint with this project? … Yes
    ✔ Would you like to use Tailwind CSS with this project? … No
    ✔ Would you like to use `src/` directory with this project? … Yes
    ✔ Use App Router (recommended)? … No
    ✔ Would you like to customize the default import alias? … No
    ```
  {/* <!-- prettier-ignore-end --> */}
  </RouteSwitchContent>

Enter the newly created directory:

```bash
cd test-app
```

### Install Panda CSS

Install Panda CSS dependency using your favorite package manager.

  <Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
    <Tab>
      ```bash
      pnpm install -D @pandacss/dev
      pnpm panda init --postcss
      ```
    </Tab>
    <Tab>
      ```bash
      npm install -D @pandacss/dev
      npx panda init --postcss
      ```
    </Tab>
    <Tab>
      ```bash
      yarn add -D @pandacss/dev
      yarn panda init --postcss
      ```
    </Tab>
    <Tab>
      ```bash
      bun add -D @pandacss/dev
      bun panda init --postcss
      ```
    </Tab>
  {/* <!-- prettier-ignore-end --> */}
  </Tabs>

`panda init --postcss` command will automatically create a `postcss.config.js` file at the root of your project with the
following code:

```js {3}
module.exports = {
  plugins: {
    '@pandacss/dev/postcss': {}
  }
}
```

For advanced configuration follow the Next.js PostCSS guide to set up a custom PostCSS configuration by referring to
this [link](https://nextjs.org/docs/pages/building-your-application/configuring/post-css#customizing-plugins).

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3}
{
  "scripts": {
+    "prepare": "panda codegen",
    "dev": "next dev",
    "build": "next build",
    "start": "next start",
    "lint": "next lint"
  }
}
```

The `prepare` script that will run codegen after dependency installation. Read more about
[codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your React components are included in the `include` section of the `panda.config.ts`
file.

<RouteSwitchContent value="app-dir">

```ts {7} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,
  // Where to look for your css declarations
  include: ['./src/components/**/*.{ts,tsx,js,jsx}', './src/app/**/*.{ts,tsx,js,jsx}'],
  // Files to exclude
  exclude: [],
  // The output directory for your css system
  outdir: 'styled-system'
})
```

</RouteSwitchContent>

<RouteSwitchContent value="pages-dir">

```ts {7} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,
  // Where to look for your css declarations
  include: ['./src/components/**/*.{ts,tsx,js,jsx}', './src/pages/**/*.{ts,tsx,js,jsx}'],
  // Files to exclude
  exclude: [],
  // The output directory for your css system
  outdir: 'styled-system'
})
```

</RouteSwitchContent>

### Configure the entry CSS with layers

<RouteSwitchContent value="app-dir">
  In your Next.js project, navigate to the `src/app` folder and open `globals.css` file. Replace all the content with
  the following code:
</RouteSwitchContent>
<RouteSwitchContent value="pages-dir">
  In your Next.js project, navigate to the `src/styles` folder and open `globals.css` file. Replace all the content with
  the following code:
</RouteSwitchContent>

```css
@layer reset, base, tokens, recipes, utilities;
```

<RouteSwitchContent value="app-dir">
  
  > **Note:** Feel free to remove the `page.module.css` file as we don't need it
  anymore.
  
</RouteSwitchContent>
<RouteSwitchContent value="pages-dir">

> **Note:** Feel free to remove the `Home.module.css` file as we don't need it anymore.

</RouteSwitchContent>

### Import the entry CSS in your app

<RouteSwitchContent value="app-dir">
  
Make sure that you import the `globals.css` file in your `src/app/layout.tsx` file as follows:

```tsx {1} filename="./src/app/layout.tsx"
import './globals.css'
import type { Metadata } from 'next'
import localFont from 'next/font/local'

const geistSans = localFont({
  src: './fonts/GeistVF.woff',
  variable: '--font-geist-sans',
  weight: '100 900'
})
const geistMono = localFont({
  src: './fonts/GeistMonoVF.woff',
  variable: '--font-geist-mono',
  weight: '100 900'
})

export const metadata: Metadata = {
  title: 'Create Next App',
  description: 'Generated by create next app'
}

export default function RootLayout({
  children
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="en">
      <body className={`${geistSans.variable} ${geistMono.variable}`}>{children}</body>
    </html>
  )
}
```

</RouteSwitchContent>
<RouteSwitchContent value="pages-dir">

Make sure that you import the `globals.css` file in your `src/pages/_app.tsx` file as follows:

```tsx {1} filename="./src/pages/_app.tsx"
import '../styles/globals.css'
import type { AppProps } from 'next/app'

export default function App({ Component, pageProps }: AppProps) {
  return <Component {...pageProps} />
}
```

</RouteSwitchContent>

### Start using Panda

<RouteSwitchContent value="app-dir">
  We will update the contents of `src/app/page.tsx` with the following snippet that uses Panda CSS:
</RouteSwitchContent>
<RouteSwitchContent value="pages-dir">
  We will update the contents of `src/pages/index.tsx` with the following snippet that uses Panda CSS:
</RouteSwitchContent>

```tsx
import { css } from '../../styled-system/css'

export default function Home() {
  return <div className={css({ fontSize: '2xl', fontWeight: 'bold' })}>Hello 🐼!</div>
}
```

### Start the development server

Run the following command to start the development server:

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
{/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
{/* <!-- prettier-ignore-end --> */}
</Tabs>

Open [http://localhost:3000](http://localhost:3000) with your browser to see the result.

</Steps>
</RouteSwitch>

## Troubleshooting

### I don't see any styles

Sometimes Next.js caches PostCSS generated styles and when that happens you need to clear the cache. To do that, delete
the `.next` folder and restart your development server.

You can also update you `package.json` scripts to delete the `.next` folder before each build:

```diff {3,4}
{
"scripts": {
-    "dev": "next dev",
+    "dev": "rm -rf .next && next dev",
},
}
```

This is a known issue with Next.js and you can track the progress here:

- [vercel/next.js#39410](https://github.com/vercel/next.js/issues/39410)
- [vercel/next.js#48748](https://github.com/vercel/next.js/issues/48748)
- [vercel/next.js#47533](https://github.com/vercel/next.js/issues/47533)

### I don't see any import autocomplete in my IDE

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

### Codegen fails using es5

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


## Using Nuxt

Easily use Panda with Nuxt with the vue integration.

Learn how to set up Panda CSS in a Nuxt project using PostCSS.

## Start a new project

<Steps>

### Create Nuxt project

To get started, we will need to create a new Nuxt project using npx.

<Tabs items={['npx', 'pnpm', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    npx nuxi@latest init test-app
    ```
  </Tab>
  <Tab>
    ```bash
    pnpm dlx nuxi@latest init test-app
    ```
  </Tab>
  <Tab>
    ```bash
    bunx nuxi@latest init test-app
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

Enter the newly created directory and install the dependencies.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    cd test-app
    pnpm install
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    npm install
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    yarn install
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    bun install
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "build": "nuxt build",
    "dev": "nuxt dev",
    "generate": "nuxt generate",
    "preview": "nuxt preview"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Vue components are included in the `include` section of the `panda.config.ts`
file.

```js {8,17} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./app.vue', './components/**/*.{js,jsx,ts,tsx,vue}', './pages/**/*.{js,jsx,ts,tsx,vue}'],

  // Files to exclude
  exclude: [],

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `assets/css/global.css` file.

```css filename="assets/css/global.css"
@layer reset, base, tokens, recipes, utilities;
```

### Configure Nuxt

Add the following configuration to the `nuxt.config.ts` file

```js {1-2,5-17}  filename="nuxt.config.ts"
import { createResolver } from '@nuxt/kit'
const { resolve } = createResolver(import.meta.url)

export default defineNuxtConfig({
  alias: {
    'styled-system': resolve('./styled-system')
  },

  css: ['@/assets/css/global.css'],

  postcss: {
    plugins: {
      '@pandacss/dev/postcss': {}
    }
  }
})
```

With the above we've performed the following:

- imported the global css file '@/assets/css/global.css' at the root of the system.
- created an alias that points to the `styled-system` directory.
- added panda into the postcss plugins section.

## Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project.

As an example here is a snippet of code for a `components/Demo.vue` file.

```xml filename="components/Demo.vue"
<script setup lang="ts">
import { css } from "styled-system/css";
</script>

<template>
  <div :class="css({ fontSize: '5xl', fontWeight: 'bold' })">Hello 🐼!</div>
</template>
```

</Steps>

---


## Using PostCSS

Installing Panda as a PostCSS plugin is the recommended way to integrate it with your project.

This guide shows you how to install Panda as a PostCSS plugin, which is the recommended way to integrate it with your
project.

<Steps>
### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev postcss
    pnpm panda init -p
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev postcss
    npx panda init -p
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev postcss
    yarn panda init -p
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev postcss
    bun panda init -p
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Add Panda to your PostCSS config

Add panda to your `postcss.config.cjs` file, or wherever PostCSS is configured in your project.

```js
module.exports = {
  plugins: {
    '@pandacss/dev/postcss': {}
  }
}
```

### Configure the content

Add your panda config to your `panda.config.js` file, or wherever panda is configured in your project.

```js {5}
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  preflight: true,
  include: ['./src/**/*.{ts,tsx,js,jsx}', './pages/**/*.{ts,tsx,js,jsx}'],
  exclude: [],
  outdir: 'styled-system'
})
```

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3}
{
  "scripts": {
+    "prepare": "panda codegen",
  }
}
```

The `prepare` script will run codegen after dependency installation. Read more about
[codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the entry CSS with layers

Add this code to an `index.css` file which is going to be the root css of your project.

```css
@layer reset, base, tokens, recipes, utilities;
```

### Start your build process

Run your build process by feeding the [root css](#configure-the-entry-css-with-layers) to PostCSS in your preferred way.

<Tabs items={['CLI', 'JS API']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    postcss -o output.css index.css
    ```
  </Tab>
  <Tab>
    ```js
    const postcss = require("postcss");
    const fs = require("fs");

    fs.readFile("index.css", (err, css) => {
      postcss()
        .process(css, { from: "index.css", to: "output.css" })
        .then((result) => {
          console.log(result.css);
        });
    });
    ```

  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

> If you're using a framework, PostCSS is probably already integrated with your build process. Check our other guides or
> the documentation of your framework to see how to configure PostCSS.

### Start using Panda

Use the generated style utilities in your code and panda will extract them to the generated CSS file.

```jsx
import { css } from './styled-system/css'

export function App() {
  return <div className={css({ bg: 'red.400' })} />
}
```

  </Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using Preact

Easily use Panda with Preact with our dedicated integration.

This guide shows you how to set up Panda CSS in a Preact project using PostCSS.

## Start a new project

<Steps>

### Create Vite project

To get started, we will need to create a new Preact project using `typescript` template.

<Tabs items={['npm', 'yarn']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    npx preact-cli create typescript test-app
    cd test-app
    ```
  </Tab>
  <Tab>
    ```bash
    npx preact-cli create typescript test-app --yarn
    cd test-app
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['npm', 'yarn']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "build": "cross-env NODE_OPTIONS=--openssl-legacy-provider preact build",
    "serve": "sirv build --cors --single",
    "dev": "cross-env NODE_OPTIONS=--openssl-legacy-provider preact watch",
    "lint": "eslint src",
    "test": "jest"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Preact components are included in the `include` section of the `panda.config.ts`
file.

```js {6} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  preflight: true,
  // Where to look for your css declarations
  include: ['./src/**/*.{js,jsx,ts,tsx}', './pages/**/*.{js,jsx,ts,tsx}'],
  exclude: [],
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `src/style/index.css` file imported in the root component of your project.

```css filename="src/style/index.css"
@layer reset, base, tokens, recipes, utilities;
```

## Start your build process

Run the following command to start your development server.

<Tabs items={['npm', 'yarn']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your
`src/routes/home/index.tsx` file.

```tsx filename="src/routes/home/index.tsx"
import { h } from 'preact'
import { css } from '../../../styled-system/css'

const Home = () => {
  return <div class={css({ fontSize: '2xl', fontWeight: 'bold', pt: '56px' })}>Hello 🐼!</div>
}

export default Home
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using Qwik

Easily use Panda with Qwik with our dedicated integration.

Learn how to set up Panda CSS in a Qwik project using PostCSS.

## Start a new project

<Steps>

### Create Qwik project

To get started, we will need to create a new Qwik project using `typescript` template.

<Tabs items={['npm', 'pnpm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    npm create qwik@latest
    ```
  </Tab>
  <Tab>
    ```bash
    pnpm create qwik@latest
    ```
  </Tab>
  <Tab>
    ```bash
    yarn create qwik
    ```
  </Tab>
  <Tab>
    ```bash
    bun create qwik@latest
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install and Configure Panda

Qwik provies an official script that installs panda and configures it for you.

<Tabs items={['npm', 'pnpm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    npm run qwik add pandacss
    ```
  </Tab>
  <Tab>
    ```bash
    pnpm qwik add pandacss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn qwik add pandacss
    ```
  </Tab>
  <Tab>
    ```bash
    bun qwik add pandacss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

## Start your build process

Run the following command to start your development server.

<Tabs items={['npm', 'pnpm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project.

```tsx filename="src/routes/layout.tsx"
import { component$, Slot } from '@builder.io/qwik'
import { routeLoader$ } from '@builder.io/qwik-city'
import { css } from 'styled-system/css'

export const useServerTimeLoader = routeLoader$(() => {
  return {
    date: new Date().toISOString()
  }
})

export default component$(() => {
  return (
    <div class={css({ p: '10', bg: 'gray.900', h: 'dvh' })}>
      <Slot />
    </div>
  )
})
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using React Router

Easily use Panda with React Router with our dedicated integration.

This guide will show you how to set up Panda CSS in a React Router project using PostCSS.

## Start a new project

<Steps>

### Create project

To get started, we will need to create a new React Router project using the official
[Create React Router](https://reactrouter.com/start/framework/installation) CLI. In this guide, we will use TypeScript.

If you don't enter any parameter, the CLI will guide you through the process of creating a new React Router app.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dlx create-react-router@latest
    ```
  </Tab>
  <Tab>
    ```bash
    npx create-react-router@latest
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dlx create-react-router@latest
    ```
  </Tab>
  <Tab>
    ```bash
    bunx create-react-router@latest
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

You will be asked a few questions, answer these as follows:

```sh
? Where should we create your new project? test-app
? Install dependencies? No
```

> **Note:** You should decline the dependency installation step as we will install dependencies together with Panda CSS.

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "build": "cross-env NODE_ENV=production react-router build",
    "dev": "react-router dev",
    "start": "cross-env NODE_ENV=production react-router-serve ./build/server/index.js",
    "typecheck": "react-router typegen && tsc"
  },
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the Panda CSS output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your React Router components are included in the `include` section of the
`panda.config.ts` file.

```ts {5,8,11} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./app/**/{**,.client,.server}/**/*.{js,jsx,ts,tsx}'],

  // Files to exclude
  exclude: [],

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Replace TailwindCSS with PandaCSS

Update the `vite.config.ts` file to use PandaCSS instead of TailwindCSS.

```ts {3,10} filename="vite.config.ts"
import { reactRouter } from '@react-router/dev/vite'
import autoprefixer from 'autoprefixer'
import pandacss from '@pandacss/dev/postcss'
import { defineConfig } from 'vite'
import tsconfigPaths from 'vite-tsconfig-paths'

export default defineConfig({
  css: {
    postcss: {
      plugins: [pandacss, autoprefixer]
    }
  },
  plugins: [reactRouter(), tsconfigPaths()]
})
```

### Configure the entry CSS with layers

Create a new file `app/app.css` and add the following content:

```css filename="app/app.css"
@layer reset, base, tokens, recipes, utilities;
```

### Update the root component

Import the `app.css` file in your `app/root.tsx` file and add the `styles` variable to the `links` function.

Please note the `?url` query parameter in the `import` statement. This is required by Vite to generate the correct path
to the CSS file.

```tsx {4,8} filename="app/root.tsx"
import { isRouteErrorResponse, Links, Meta, Outlet, Scripts, ScrollRestoration } from 'react-router'

import type { Route } from './+types/root'
import stylesheet from './app.css?url'

export const links: LinksFunction = () => [
  // ...
  { rel: 'stylesheet', href: stylesheet }
]

export function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <Meta />
        <Links />
      </head>
      <body>
        {children}
        <ScrollRestoration />
        <Scripts />
      </body>
    </html>
  )
}
```

### Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your
`app/routes/home.tsx` file.

```tsx filename="app/routes/home.tsx"
import type { Route } from './+types/home'
import { css } from 'styled-system/css'

export function meta({}: Route.MetaArgs) {
  return [{ title: 'New React Router App' }, { name: 'description', content: 'Welcome to React Router!' }]
}

export default function Home() {
  return (
    <div>
      <h1 className={css({ fontSize: '2xl', fontWeight: 'bold' })}>Welcome to the home page</h1>
    </div>
  )
}
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["app", "styled-system"]
}
```

---


## Using Redwood

Easily use Panda with Redwood.js with our dedicated integration.

This guide shows you how to set up Panda CSS in a Redwood project using PostCSS.

> Redwood uses `yarn` as its primary package manager.

## Start a new project

<Steps>

### Create Redwood project

To get started, we will need to create a new Redwood project using `typescript` template.

```bash
yarn create redwood-app app
```

Respond to the prompts as follows:

```bash
✔ Compatibility checks passed
✔ Select your preferred language · TypeScript
✔ Do you want to initialize a git repo? · no / Yes
✔ Enter a commit message · Initial commit
✔ Do you want to run yarn install? · no / Yes
✔ Project files created
✔ Initialized a git repo with commit message "Initial commit"
✔ Installed node modules
✔ Generated types
```

### Install Panda

Install panda and generate the `panda.config.ts` and `postcss.config.js` file.

```bash
cd web
yarn add -D @pandacss/dev postcss postcss-loader
yarn panda init --postcss
```

### Move to config folder

Redwood uses a `config` folder for all of the configuration files. Move the `panda.config.ts` and `postcss.config.js`
files to the `config` folder.

```bash
mv panda.config.ts postcss.config.js config/
```

### Update configs

Update the postcss config file to use the `panda.config.ts` file.

```diff filename="web/config/postcss.config.js"
+ const path = require('path')

module.exports = {
  plugins: {
    "@pandacss/dev/postcss": {
+      configPath: path.resolve(__dirname, 'panda.config.ts'),
   },
  },
}
```

Update the tsconfig file to include the `styled-system` folder.

```json {6} filename="web/tsconfig.json"
{
  // ...
  "compilerOptions": {
    "paths": {
      // ...
      "styled-system/*": ["./styled-system/*"]
    }
  }
}
```

### Update package.json scripts

Open the `web/package.json` file and update the `scripts` section as follows:

```diff {3} filename="web/package.json"
{
  "scripts": {
+    "prepare": "panda codegen"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Redwood components are included in the `include` section of the
`panda.config.ts` file.

```js {5} filename="web/config/panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  preflight: true,
  include: ['./src/**/*.{js,jsx,ts,tsx}'],
  exclude: [],
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `src/style/index.css` file imported in the root component of your project.

```css filename="web/src/index.css"
@layer reset, base, tokens, recipes, utilities;
```

## Start your build process

Run the following command to start your development server.

```bash
yarn rw dev
```

### Start using Panda

Now you can start using Panda CSS in your project.

```tsx filename="web/src/pages/HomePage/HomePage.tsx"
import { css } from 'styled-system/css'
import { stack } from 'styled-system/patterns'
import { Link, routes } from '@redwoodjs/router'
import { MetaTags } from '@redwoodjs/web'

const HomePage = () => {
  return (
    <>
      <MetaTags title="Home" description="Home page" />

      <div
        className={stack({
          bg: { base: 'red.300', _hover: 'red.500' },
          py: '12',
          px: '8'
        })}
      >
        <h1 className={css({ fontSize: '4xl', fontWeight: 'bold' })}>HomePage</h1>
        <p>Hello World</p>
      </div>
    </>
  )
}

export default HomePage
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="web/tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using Remix

How to use Panda with Remix with our dedicated integration.

This guide will show you how to set up Panda CSS in a Remix project using PostCSS.

## Start a new project

<Steps>

### Create Remix project

To get started, we will need to create a new Remix project using the official
[Create Remix](https://remix.run/docs/en/main/start/quickstart) CLI. In this guide, we will use TypeScript.

If you don't enter any parameter, the CLI will guide you through the process of creating a new Remix app.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dlx create-remix@latest
    ```
  </Tab>
  <Tab>
    ```bash
    npx create-remix@latest
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dlx create-remix@latest
    ```
  </Tab>
  <Tab>
    ```bash
    bunx create-remix@latest
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

You will be asked a few questions, answer these as follows:

```sh
? Where should we create your new project? test-app
? Install dependencies? No
```

> **Note:** You should decline the dependency installation step as we will install dependencies together with Panda CSS.

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "build": "remix build",
    "dev": "remix dev",
    "start": "remix-serve build",
    "typecheck": "tsc"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the Panda CSS output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Remix components are included in the `include` section of the `panda.config.ts`
file.

```js {5, 11} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./app/routes/**/*.{ts,tsx,js,jsx}', './app/components/**/*.{ts,tsx,js,jsx}'],

  // Files to exclude
  exclude: [],

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Create a new file `app/index.css` and add the following content:

```css filename="app/index.css"
@layer reset, base, tokens, recipes, utilities;
```

Import the `index.css` file in your `app/root.tsx` file and add the `styles` variable to the `links` function.

Please note the `?url` query parameter in the `import` statement. This is required by Vite to generate the correct path
to the CSS file.

```tsx filename="app/root.tsx" {4,6}
import type { LinksFunction } from '@remix-run/node'
import { Links, LiveReload, Meta, Outlet, Scripts, ScrollRestoration } from '@remix-run/react'

import styles from './index.css?url'

export const links: LinksFunction = () => [{ rel: 'stylesheet', href: styles }]

export default function App() {
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <Meta />
        <Links />
      </head>
      <body>
        <Outlet />
        <ScrollRestoration />
        <Scripts />
        <LiveReload />
      </body>
    </html>
  )
}
```

### Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your
`app/routes/_index.tsx` file.

```tsx filename="app/routes/_index.tsx"
import type { MetaFunction } from '@remix-run/node'
import { css } from 'styled-system/css'

export const meta: MetaFunction = () => {
  return [{ title: 'New Remix App' }, { name: 'description', content: 'Welcome to Remix!' }]
}

export default function Index() {
  return <div className={css({ fontSize: '2xl', fontWeight: 'bold' })}>Hello 🐼!</div>
}
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

If your app doesn't reload when making changes to the `panda.config.ts` file, consider adding `watchPaths` in your remix
config file.

```js filename="remix.config.js"
export default {
  // ...
  watchPaths: ['panda.config.ts']
}
```

---


## Using Rsbuild

Easily use Panda with Rsbuild, React and Typescript with our dedicated integration.

This guide shows you how to set up Panda CSS in a Rsbuild project using PostCSS.

## Start a new project

<Steps>

### Create Rsbuild project

To get started, we will need to create a new Rsbuild project using `react-ts` template.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm create rsbuild@latest --dir test-app --template react-ts
    cd test-app
    pnpm install
    ```
  </Tab>
  <Tab>
    ```bash
    npm create rsbuild@latest --dir test-app -- --template react-ts
    cd test-app
    npm install
    ```
  </Tab>
  <Tab>
    ```bash
    yarn create rsbuild@latest --dir test-app --template react-ts
    cd test-app
    yarn
    ```
  </Tab>
  <Tab>
    ```bash
    bun create rsbuild@latest --dir test-app --template react-ts
    cd test-app
    bun install
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {5} filename="package.json"
{
  "scripts": {
    "build": "rsbuild build",
    "dev": "rsbuild dev --open",
+   "prepare": "panda codegen",
    "preview": "rsbuild preview"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your React components are included in the `include` section of the `panda.config.ts`
file.

```js {8} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./src/**/*.{js,jsx,ts,tsx}', './pages/**/*.{js,jsx,ts,tsx}'],

  // Files to exclude
  exclude: [],

  // Generates JSX utilities with options of React, Preact, Qwik, Solid, Vue
  jsxFramework: 'react',

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `src/App.css` file imported in the root component of your project.

```css filename="src/App.css"
@layer reset, base, tokens, recipes, utilities;
```

## Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your `src/App.tsx`
file.

```tsx filename="src/App.tsx"
import { css } from '../styled-system/css'

function App() {
  return <div className={css({ fontSize: '2xl', fontWeight: 'bold' })}>Hello 🐼!</div>
}

export default App
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using SolidJS

Easily use Panda with SolidJS with our dedicated integration.

This guide will show you how to set up Panda CSS in a Solid.js project using PostCSS.

## Start a new project

<Steps>

### Create Vite project

To get started, we will need to create a new SolidJS project using `solidjs/templates/ts` template.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dlx degit solidjs/templates/ts test-app
    cd test-app
    pnpm install
    ```
  </Tab>
  <Tab>
    ```bash
    npx degit solidjs/templates/ts test-app
    cd test-app
    npm install
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dlx degit solidjs/templates/ts test-app
    cd test-app
    yarn
    ```
  </Tab>
  <Tab>
    ```bash
    bunx degit solidjs/templates/ts test-app
    cd test-app
    bun install
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "dev": "vite",
    "build": "tsc && vite build",
    "lint": "eslint src --ext ts,tsx --report-unused-disable-directives --max-warnings 0",
    "preview": "vite preview"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

  > This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
  > the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your SolidJS components are included in the `include` section of the
`panda.config.ts` file.

```ts {8,17} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./src/**/*.{js,jsx,ts,tsx}', './pages/**/*.{js,jsx,ts,tsx}'],

  // Files to exclude
  exclude: [],

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `src/index.css` file imported in the root component of your project.

```css filename="src/index.css"
@layer reset, base, tokens, recipes, utilities;
```

> **Note:** Feel free to remove `src/App.module.css` file as we don't need it anymore, and make sure to remove the
> import from the `src/App.tsx` file.

## Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your `src/App.tsx`
file.

```tsx filename="src/App.tsx"
import type { Component } from 'solid-js'
import { css } from '../styled-system/css'

const App: Component = () => {
  return <div class={css({ fontSize: '2xl', fontWeight: 'bold' })}>Hello 🐼!</div>
}

export default App
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using Storybook

Easily use Panda with Storybook with our dedicated integration.

Learn how to set up Panda CSS in a Storybook project using PostCSS.

## Setup

We are assuming that you already have a project set up with a framework like React, Vue or Svelte.

<Steps>

### Install Storybook

Storybook needs to be installed into a project that is already set up with a framework. It will not work on an empty
project.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
{/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dlx storybook@latest init
    ```
  </Tab>
  <Tab>
    ```bash
    npx storybook@latest init
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dlx storybook@latest init
    ```
  </Tab>
  <Tab>
    ```bash
    bunx storybook@latest init
    ```
  </Tab>
{/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

If you are using Storybook with the Vite builder, you will have to update your PostCSS config file to use the array
syntax for the plugins instead of the object syntax. Simply change `postcss.config.[c]js`:

```diff filename="postcss.config.js"
module.exports = {
-  plugins: {
-   '@pandacss/dev/postcss': {}
-  }
+  plugins: [require('@pandacss/dev/postcss')()]
}
```

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="web/package.json"
{
  "scripts": {
+    "prepare": "panda codegen"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Storybook components are included in the `include` section of the
`panda.config.ts` file.

```ts {7} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,
  // Where to look for your css declarations
  include: ['./src/**/*.{js,jsx,ts,tsx}', './pages/**/*.{js,jsx,ts,tsx}', './stories/**/*.{js,jsx,ts,tsx}'],
  // Files to exclude
  exclude: [],
  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Locate your main CSS file and add the following layers:

```css filename="src/index.css"
@layer reset, base, tokens, recipes, utilities;
```

### Import the CSS in your Storybook config

Locate your `.storybook/preview.ts` file and import the CSS file.

In this example CSS file is located in the `src` folder.

```ts {1} filename=".storybook/preview.ts"
import '../src/index.css'

import type { Preview } from '@storybook/react'

const preview: Preview = {
  parameters: {
    actions: { argTypesRegex: '^on[A-Z].*' },
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/
      }
    }
  }
}

export default preview
```

### Start the Storybook server

Run the following command to start your Storybook server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm storybook
    ```
  </Tab>
  <Tab>
    ```bash
    npm run storybook
    ```
  </Tab>
  <Tab>
    ```bash
    yarn storybook
    ```
  </Tab>
  <Tab>
    ```bash
    bun storybook
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in Storybook.

Here is the example of a Button component and its corresponding Storybook story:

```tsx filename="src/stories/Button.tsx"
import { ReactNode } from 'react'
import { css } from '../../styled-system/css'

export interface IButtonProps {
  children: ReactNode
}

export const Button = ({ children }: IButtonProps) => {
  return (
    <button
      className={css({
        bg: 'red.300',
        fontFamily: 'Inter',
        px: '4',
        py: '3',
        borderRadius: 'md',
        _hover: { bg: 'red.400' }
      })}
    >
      {children}
    </button>
  )
}
```

```tsx filename="src/stories/Button.stories.tsx"
import type { Meta, StoryObj } from '@storybook/react'
import { css } from '../../styled-system/css'

import { Button } from './Button'

const meta = {
  title: 'Example/Button',
  component: Button,
  tags: ['autodocs'],
  decorators: [
    Story => (
      <div className={css({ m: 10 })}>
        <Story />
      </div>
    )
  ]
} satisfies Meta<typeof Button>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {
  args: {
    children: 'Hello 🐼!'
  }
}
```

</Steps>

## Configuring Dark Mode

To enable dark mode in Storybook, you can use the `@storybook/addon-themes` package.

```bash
pnpm add -D @storybook/addon-themes
```

Then, update your `.storybook/preview.ts` file to include the following:

```ts filename=".storybook/preview.ts"
import { withThemeByClassName } from '@storybook/addon-themes'
import type { Preview, ReactRenderer } from '@storybook/react'

const preview: Preview = {
  // ...
  decorators: [
    withThemeByClassName<ReactRenderer>({
      themes: {
        light: '',
        dark: 'dark'
      },
      defaultTheme: 'light'
    })
  ]
}

export default preview
```

With that in place, you should see the light/dark switcher in Storybook's toolbar.

## Troubleshooting

### Cannot find postcss plugin

If you are having issues with the PostCSS plugin similar to `Cannot find module '@pandacss/dev/postcss'`, update the
PostCSS config as follows:

```js filename="postcss.config.js"
module.exports = {
  plugins: [require('@pandacss/dev/postcss')]
}
```

### HMR not triggered

If you are having issues with HMR not being triggered after a `panda.config.ts` change (or one of its
[dependencies](/docs/references/config#dependencies), you can manually specify the files that should trigger a rebuild
by adding the following to your `panda.config.ts`:

```js filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  dependencies: ['path/to/files/**.ts']
})
```

### Styles in `args` is not generated

If you are having issues with your `args` not generating the expected CSS, it's probably because of:

- you didn't add a file glob for the Storybook files in your [`config.include`](/docs/references/config#include) like
  `"./stories/**/*.{js,jsx,ts,tsx}"`
- you didn't wrap your `args` object (or some part of it) with the
  [`.raw()` marker that helps Panda find style usage](https://panda-css.com/docs/guides/dynamic-styling#alternative)

```tsx filename="stories/Button.stories.tsx"
import type { Meta, StoryObj } from '@storybook/react'
import { button } from '../../styled-system/recipes'

export const Funky: Story = {
  // mark this as a button recipe usage
  args: button.raw({
    visual: 'funky',
    shape: 'circle',
    size: 'sm'
  })
}
```

### Some recipes styles are missing

If you are having issues with your config `recipes` or `slotRecipes` not generating the expected CSS, it's probably
because of:

- you didn't add a file glob for the Storybook files in your [`config.include`](/docs/references/config#include) like
  `"./stories/**/*.{js,jsx,ts,tsx}"`
- you haven't used every recipes variants in your app, so you might want to pre-generate it (only for storybook usage)
  with [`staticCss`](/docs/guides/static)

```tsx filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  staticCss: {
    recipes: '*'
  }
})
```

---


## Using Svelte

Easily use Panda with Svelte with our dedicated integration.

This guide will show you how to set up Panda CSS in a Svelte project using PostCSS.

## Start a new project

<Steps>

### Create Svelte project

To get started, we will need to create a new Svelte project.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm create svelte@latest test-app
    ```
  </Tab>
  <Tab>
    ```bash
    npm create svelte@latest test-app
    ```
  </Tab>
  <Tab>
    ```bash
    yarn create svelte@latest test-app
    ```
  </Tab>
  <Tab>
    ```bash
    bun create svelte@latest test-app
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

You will be asked a few questions, answer them as follows:

```sh
┌  Welcome to SvelteKit!
│
◇  Which Svelte app template?
│  Skeleton project
│
◇  Add type checking with TypeScript?
│  Yes, using TypeScript syntax
│
◇  Select additional options (use arrow keys/space bar)
│  ◼ Add ESLint for code linting
│  ◼ Add Prettier for code formatting
│  ◻ Add Playwright for browser testing
│  ◻ Add Vitest for unit testing
│
└  Your project is ready!
```

Enter the newly created directory and install the dependencies.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    cd test-app
    pnpm install
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    npm install
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    yarn
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    bun install
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

To install Panda and corresponding dependencies run the following commands:

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```json {3}
{
  "scripts": {
    "prepare": "panda codegen",
    "dev": "vite dev",
    "build": "vite build",
    "preview": "vite preview",
    "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",
    "check:watch": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json --watch",
    "lint": "prettier --plugin-search-dir . --check . && eslint .",
    "format": "prettier --plugin-search-dir . --write ."
  }
}
```

This `"prepare"` script will run Panda CSS CLI codegen before each build. Read more about
[codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Svelte components are included in the `include` section of the `panda.config.ts`
file.

```js {8} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./src/**/*.{js,ts,svelte}'],

  // Files to exclude
  exclude: [],

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Update Svelte config

To configure Svelte preprocess to use PostCSS and add Panda alias update the `svelte.config.js` file as follows:

```js {15} filename="svelte.config.js"
import adapter from '@sveltejs/adapter-auto'
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte'

/** @type {import('@sveltejs/kit').Config} */
const config = {
  // Consult https://kit.svelte.dev/docs/integrations#preprocessors
  // for more information about preprocessors
  preprocess: [vitePreprocess()],
  kit: {
    // adapter-auto only supports some environments, see https://kit.svelte.dev/docs/adapter-auto for a list.
    // If your environment is not supported or you settled on a specific environment, switch out the adapter.
    // See https://kit.svelte.dev/docs/adapters for more information about adapters.
    adapter: adapter(),
    alias: {
      'styled-system': './styled-system/*'
    }
  }
}

export default config
```

### Update Vite config

To be able to import `styled-system` files in your Svelte components you will need to update the `vite.config.js` file
as follows:

```js {6-10} filename="vite.config.js"
import { sveltekit } from '@sveltejs/kit/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    fs: {
      allow: ['styled-system']
    }
  }
})
```

If you’re using Storybook for a SvelteKit project, you need to replicate the same Vite server config changes. In your
.storybook folder, you likely have a `main.js` (or `vite.config.js` in older Storybook versions). Update it as follows:

```js filename="main.js"
import { defineConfig, mergeConfig } from 'vite'

/** @type { import('@storybook/sveltekit').StorybookConfig } */

const config = {
  // other Storybook config...
  viteFinal: async config => {
    return mergeConfig(
      config,
      defineConfig({
        server: {
          fs: {
            allow: ['styled-system']
          }
        }
      })
    )
  }
}
export default config
```

### Configure the entry CSS with layers

Create the `app.css` file in the `src` directory and add the following content:

```css filename="src/app.css"
@layer reset, base, tokens, recipes, utilities;
```

### Import styles in the layout file

Create the `src/routes/+layout.svelte` file and add the following content:

```svelte {2} filename="src/routes/+layout.svelte"
<script>
  import '../app.css'
</script>

<slot />
```

### Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your
`src/routes/+page.svelte` file.

```svelte filename="src/routes/+page.svelte"
<script>
	import { css } from 'styled-system/css'
</script>

<div class={css({ fontSize: '2xl',	fontWeight: 'bold' })}>
	Hello 🐼!
</div>
```

</Steps>

## Troubleshooting

### Autocomplete not working

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
TypeScript config. However, in Svelte your main `tsconfig.json` file is extending the autogenerated one. To extend it
without overriding the defaults adjust your `svelte.config.js` to include following entry:

```js filename="svelte.config.js"
import adapter from '@sveltejs/adapter-auto'
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte'
/** @type {import('@sveltejs/kit').Config} */
const config = {
  // ...
  kit: {
    // ...
    typescript: {
      config: config => {
        config.include.push('../styled-system')
        return config
      }
    }
  }
}

export default config
```

---


## Using Vite

Easily use Panda with Vite, React and Typescript with our dedicated integration.

This guide will show you how to set up Panda CSS in a Vite project using PostCSS.

## Start a new project

<Steps>

### Create Vite project

To get started, we will need to create a new Vite project using `react-ts` template.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm create vite test-app --template react-ts
    cd test-app
    pnpm install
    ```
  </Tab>
  <Tab>
    ```bash
    npm create vite@latest test-app -- --template react-ts
    cd test-app
    npm install
    ```
  </Tab>
  <Tab>
    ```bash
    yarn create vite test-app --template react-ts
    cd test-app
    yarn
    ```
  </Tab>
  <Tab>
    ```bash
    bun create vite test-app --template react-ts
    cd test-app
    bun install
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "dev": "vite",
    "build": "tsc && vite build",
    "lint": "eslint src --ext ts,tsx --report-unused-disable-directives --max-warnings 0",
    "preview": "vite preview"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your React components are included in the `include` section of the `panda.config.ts`
file.

```js {8} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./src/**/*.{js,jsx,ts,tsx}', './pages/**/*.{js,jsx,ts,tsx}'],

  // Files to exclude
  exclude: [],

  // Generates JSX utilities with options of React, Preact, Qwik, Solid, Vue
  jsxFramework: 'react',

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `src/index.css` file imported in the root component of your project.

```css filename="src/index.css"
@layer reset, base, tokens, recipes, utilities;
```

> **Note:** Feel free to remove `src/App.css` file as we don't need it anymore, and make sure to remove the import from
> the `src/App.tsx` file.

## Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your `src/App.tsx`
file.

```tsx filename="src/App.tsx"
import { css } from '../styled-system/css'

function App() {
  return <div className={css({ fontSize: '2xl', fontWeight: 'bold' })}>Hello 🐼!</div>
}

export default App
```

</Steps>

## Troubleshooting

If you're not getting import autocomplete in your IDE, you may need to include the `styled-system` directory in your
`tsconfig.json` file:

```json filename="tsconfig.json"
{
  // ...
  "include": ["src", "styled-system"]
}
```

---


## Using Vue

Easily use Panda with Vue with our dedicated integration.

Learn how to set up Panda CSS in a Vue project using PostCSS.

## Start a new project

<Steps>

### Create Vite project

To get started, we will need to create a new Vue project using the official
[scaffolding tool](https://github.com/vuejs/create-vue).

If you don't enter any parameter, the CLI will guide you through the process of creating a new Vue app.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm create vue@latest
    ```
  </Tab>
  <Tab>
    ```bash
    npm create vue@latest
    ```
  </Tab>
  <Tab>
    ```bash
    yarn create vue@latest
    ```
  </Tab>
  <Tab>
    ```bash
    bun create vue@latest
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

You will be asked a few questions, answer them as follows:

```bash
Vue.js - The Progressive JavaScript Framework

✔ Project name: … test-app
✔ Add TypeScript? … Yes
✔ Add JSX Support? … Yes
✔ Add Vue Router for Single Page Application development? … No / Yes
✔ Add Pinia for state management? … No / Yes
✔ Add Vitest for Unit Testing? … No / Yes
✔ Add an End-to-End Testing Solution? › No
✔ Add ESLint for code quality? … No / Yes
✔ Add Prettier for code formatting? … No / Yes
```

Enter the newly created directory and install the dependencies.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    cd test-app
    pnpm install
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    npm install
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    yarn
    ```
  </Tab>
  <Tab>
    ```bash
    cd test-app
    bun install
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Install Panda

Install panda and create your `panda.config.ts` file.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm install -D @pandacss/dev
    pnpm panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    npm install -D @pandacss/dev
    npx panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    yarn add -D @pandacss/dev
    yarn panda init --postcss
    ```
  </Tab>
  <Tab>
    ```bash
    bun add -D @pandacss/dev
    bun panda init --postcss
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Update package.json scripts

Open your `package.json` file and update the `scripts` section as follows:

```diff {3} filename="package.json"
{
  "scripts": {
+    "prepare": "panda codegen",
    "dev": "vite",
    "build": "run-p type-check build-only",
    "preview": "vite preview",
    "build-only": "vite build"
  }
}
```

- `"prepare"` - script that will run Panda CSS CLI codegen before each build. Read more about
  [codegen](/docs/references/cli#panda-codegen) in the CLI section.

> This step ensures that the panda output directory is regenerated after each dependency installation. So you can add
> the output directory to your `.gitignore` file and not worry about it.

### Configure the content

Make sure that all of the paths of your Vue components are included in the `include` section of the `panda.config.ts`
file.

```js {8, 17} filename="panda.config.ts"
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // Whether to use css reset
  preflight: true,

  // Where to look for your css declarations
  include: ['./src/**/*.{js,jsx,ts,tsx,vue}'],

  // Files to exclude
  exclude: [],

  // The output directory for your css system
  outdir: 'styled-system'
})
```

### Configure the entry CSS with layers

Add this code to an `src/index.css` file and import it in the root component of your project.

```css filename="src/index.css"
@layer reset, base, tokens, recipes, utilities;
```

## Start your build process

Run the following command to start your development server.

<Tabs items={['pnpm', 'npm', 'yarn', 'bun']}>
  {/* <!-- prettier-ignore-start --> */}
  <Tab>
    ```bash
    pnpm dev
    ```
  </Tab>
  <Tab>
    ```bash
    npm run dev
    ```
  </Tab>
  <Tab>
    ```bash
    yarn dev
    ```
  </Tab>
  <Tab>
    ```bash
    bun dev
    ```
  </Tab>
  {/* <!-- prettier-ignore-end --> */}
</Tabs>

### Start using Panda

Now you can start using Panda CSS in your project. Here is the snippet of code that you can use in your `src/App.vue`
file.

```vue-html filename="src/App.vue"
<script setup lang="ts">
import { css } from "../styled-system/css";
</script>

<template>
  <div :class="css({ fontSize: '5xl', fontWeight: 'bold' })">Hello 🐼!</div>
</template>
```

</Steps>

---


