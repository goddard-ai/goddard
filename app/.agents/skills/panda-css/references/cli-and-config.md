# CLI and Config Reference

Use this reference for CLI commands, config keys, output options, filesystem scanning, token settings, JSX options, and other Panda config behavior.

## CLI Reference

You can use the Command-Line Interface (CLI) provided by Panda to develop, build, and preview your project from a terminal window.

Use Panda's Command-Line Interface (CLI) to develop, build, and preview your project from a terminal.

## init

Initialize Panda in a project. This process will:

- Create a `panda.config.ts` file in your project with the default settings and presets.
- Emit CSS utilities for your project in the specified `output` directory.

```bash
pnpm panda init

# Initialize with interactive mode
pnpm panda init --interactive

# Initialize with PostCSS config
pnpm panda init --postcss
```

| Flag                          | Description                                                   | Related                                                       |
| ----------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `--interactive, -i`           | Whether to run the interactive mode                           | -                                                             |
| `--force, -f`                 | Whether to overwrite existing files                           | -                                                             |
| `--postcss, -p`               | Whether to emit a [postcss](https://postcss.org/) config file | -                                                             |
| `--config, -c <path>`         | Path to Panda config file                                     | [`config`](/docs/references/config)                           |
| `--cwd <dir>`                 | Path to current working directory                             | -                                                             |
| `--silent`                    | Whether to suppress all output                                | -                                                             |
| `--no-gitignore`              | Whether to update gitignore with the output directory         | -                                                             |
| `--no-codegen`                | Whether to run the codegen process                            | -                                                             |
| `--out-extension <ext>`       | The extension of the generated js files (default: 'mjs')      | [`config.outExtension`](/docs/references/config#outextension) |
| `--outdir <dir>`              | The output directory for the generated files                  | [`config.outdir`](/docs/references/config#outdir)             |
| `--jsx-framework <framework>` | The jsx framework to use                                      | [`config.jsxFramework`](/docs/references/config#jsxframework) |
| `--syntax <syntax>`           | The css syntax preference                                     | [`config.syntax`](/docs/references/config#syntax)             |
| `--strict-tokens`             | Set strictTokens to true                                      | [`config.strictTokens`](/docs/references/config#stricttokens) |
| `--logfile <file>`            | Outputs logs to a file                                        | [Debugging](/docs/guides/debugging)                           |

---

## panda

Run the extract process to generate static CSS from your project.

By default it will scan and generate CSS for the entire project depending on your include and exclude options from your
config file.

```bash
pnpm panda
# You can also scan a specific file or folder
# using the optional glob argument
pnpm panda src/components/Button.tsx
pnpm panda "./src/components/**"
```

| Flag                    | Description                                                            | Related                                                           |
| ----------------------- | ---------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `--outdir, -o [dir]`    | The output directory for the generated CSS utilities                   | [`config.outdir`](/docs/references/config#outdir)                 |
| `--minify, -m`          | Whether to minify the generated CSS                                    | [`config.minify`](/docs/references/config#minify)                 |
| `--watch, -w`           | Whether to watch for changes in the project                            | [`config.watch`](/docs/references/config#watch)                   |
| `--poll`                | Whether to poll for file changes                                       | [`config.poll`](/docs/references/config#poll)                     |
| `--config, -c <path>`   | The path to the config file                                            | [`config`](/docs/references/config.md)                            |
| `--cwd <path>`          | The current working directory                                          | [`config.cwd`](/docs/references/config#cwd)                       |
| `--preflight`           | Whether to emit the preflight or reset CSS                             | [`config.preflight`](/docs/references/config#preflight)           |
| `--silent`              | Whether to suppress all output                                         | [`config.logLevel`](/docs/references/config#log-level)            |
| `--exclude, -e <files>` | Files to exclude from the extract process                              | [`config`](/docs/references/config.md)                            |
| `--clean`               | Whether to clean the output directory before emitting                  | [`config.clean`](/docs/references/config#clean)                   |
| `--hash`                | Whether to hash the output classnames                                  | [`config.hash`](/docs/references/config#hash)                     |
| `--lightningcss`        | Use `lightningcss` instead of `postcss` for css optimization           | [`config.lightningcss`](/docs/references/config#lightningcss)     |
| `--polyfill`            | Polyfill CSS @layers at-rules for older browsers                       | [`config.polyfill`](/docs/references/config#polyfill)             |
| `--emitTokensOnly`      | Whether to only emit the `tokens` directory                            | [`config.emitTokensOnly`](/docs/references/config#emittokensonly) |
| `--cpu-prof`            | Generate a `panda-{command}-{timestamp}.cpuprofile` file for profiling | [Debugging](/docs/guides/debugging)                               |
| `--logfile <file>`      | Outputs logs to a file                                                 | [Debugging](/docs/guides/debugging)                               |

---

## codegen

Generate new CSS utilities for your project based on the configuration file.

```bash
pnpm panda codegen

# Clean output directory before generating
pnpm panda codegen --clean

# Watch for config changes
pnpm panda codegen --watch
```

| Flag                  | Description                                                            | Related                                                |
| --------------------- | ---------------------------------------------------------------------- | ------------------------------------------------------ |
| `--silent`            | Whether to suppress all output                                         | [`config.logLevel`](/docs/references/config#log-level) |
| `--clean`             | Whether to clean the output directory before emitting                  | [`config.clean`](/docs/references/config#clean)        |
| `--config, -c <path>` | Path to Panda config file                                              | [`config`](/docs/references/config.md)                 |
| `--watch, -w`         | Whether to watch for changes in the project                            | [`config.watch`](/docs/references/config#watch)        |
| `--poll, -p`          | Whether to poll for file changes                                       | [`config.poll`](/docs/references/config#poll)          |
| `--cwd <path>`        | Current working directory                                              | [`config.cwd`](/docs/references/config#cwd)            |
| `--cpu-prof`          | Generate a `panda-{command}-{timestamp}.cpuprofile` file for profiling | [Debugging](/docs/guides/debugging)                    |
| `--logfile <file>`    | Outputs logs to a file                                                 | [Debugging](/docs/guides/debugging)                    |

## cssgen

Generate the CSS from files.

```bash
panda cssgen

# Generate CSS for specific type
panda cssgen tokens

# Generate CSS for specific glob
panda cssgen "src/**/*.css"

# Generate CSS files split into separate files per layer and recipe
panda cssgen --splitting
```

When using the `cssgen` command, you can pass a `{type}` argument to generate only a specific type of CSS. The supported
types are: `preflight`, `tokens`, `static`, `global`, `keyframes`.

### CSS Splitting

The `--splitting` flag enables CSS code splitting, which generates separate CSS files for different parts of your design
system instead of a single monolithic CSS file. This is useful for:

- **Selective loading** - Load only the CSS you need for specific pages
- **Easier debugging** - Identify which layer or recipe contributes to the final CSS

When using `--splitting`, Panda will generate the following structure:

```
styled-system/
├── styles.css              # @layer declarations + @imports for all layers
└── styles/
    ├── reset.css           # Preflight/reset CSS
    ├── global.css          # Global CSS styles
    ├── tokens.css          # Design token CSS variables
    ├── utilities.css       # Atomic utility classes
    ├── recipes.css         # @imports for all recipe files
    ├── recipes/
    │   ├── button.css      # Individual recipe: button
    │   ├── card.css        # Individual recipe: card
    │   └── ...             # Other recipes as separate files
    └── themes/
        └── dark.css        # Theme-specific tokens (not auto-imported)
        └── light.css       # Theme-specific tokens (not auto-imported)
```

The main `styles.css` file contains the `@layer` declarations and imports all the layer files (but not the themes):

```css
/* styled-system/styles.css */
@layer reset, base, tokens, recipes, utilities;

@import './styles/reset.css';
@import './styles/global.css';
@import './styles/tokens.css';
@import './styles/recipes.css';
@import './styles/utilities.css';
```

You can then choose how to import these files:

```css
/* Option 1: Import everything (default) */
@import './styled-system/styles.css';

/* Option 2: Import specific layers only */
@import './styled-system/styles/tokens.css';
@import './styled-system/styles/utilities.css';

/* Option 3: Import specific recipes */
@import './styled-system/styles/recipes/button.css';
@import './styled-system/styles/recipes/card.css';

/* Option 4: Import a specific theme (when using multiple themes) */
@import './styled-system/styles/themes/oceanic.css';
```

| Flag                   | Description                                                                            | Related                                                       |
| ---------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `--outfile, -o <file>` | Output file for extracted css, default to './styled-system/styles.css'                 | -                                                             |
| `--silent`             | Whether to suppress all output                                                         | [`config.logLevel`](/docs/references/config#log-level)        |
| `--minify, -m`         | Whether to minify the generated CSS                                                    | [`config.minify`](/docs/references/config#minify)             |
| `--clean`              | Whether to clean the output directory before emitting                                  | [`config.clean`](/docs/references/config#clean)               |
| `--config, -c <path>`  | Path to Panda config file                                                              | [`config`](/docs/references/config.md)                        |
| `--watch, -w`          | Whether to watch for changes in the project                                            | [`config.watch`](/docs/references/config#watch)               |
| `--minimal`            | Skip generating CSS for theme tokens, preflight, keyframes, static and global css      | -                                                             |
| `--splitting`          | Emit CSS as separate files per layer (reset, global, tokens, utilities) and per recipe | -                                                             |
| `--poll, -p`           | Whether to poll for file changes                                                       | [`config.poll`](/docs/references/config#poll)                 |
| `--cwd <path>`         | Current working directory                                                              | [`config.cwd`](/docs/references/config#cwd)                   |
| `--lightningcss`       | Use `lightningcss` instead of `postcss` for css optimization                           | [`config.lightningcss`](/docs/references/config#lightningcss) |
| `--polyfill`           | Polyfill CSS @layers at-rules for older browsers                                       | [`config.polyfill`](/docs/references/config#polyfill)         |
| `--cpu-prof`           | Generate a `panda-{command}-{timestamp}.cpuprofile` file for profiling                 | [Debugging](/docs/guides/debugging)                           |
| `--logfile <file>`     | Outputs logs to a file                                                                 | [Debugging](/docs/guides/debugging)                           |

## studio

Realtime documentation for your design tokens.

```bash
pnpm panda studio

# Build static studio site
pnpm panda studio --build

# Preview built studio
pnpm panda studio --preview

# Use custom port
pnpm panda studio --port 3000
```

| Flag                  | Description                       | Related                                     |
| --------------------- | --------------------------------- | ------------------------------------------- |
| `--build`             | Build                             | -                                           |
| `--preview`           | Preview                           | -                                           |
| `--port <port>`       | Use custom port                   | -                                           |
| `--host`              | Expose to custom host             | -                                           |
| `--config, -c <path>` | Path to Panda config file         | [`config`](/docs/references/config.md)      |
| `--cwd <path>`        | Current working directory         | [`config.cwd`](/docs/references/config#cwd) |
| `--outdir <dir>`      | Output directory for static files | -                                           |
| `--base <path>`       | Base path of project              | -                                           |

## spec

Generate spec files for your theme (useful for documentation).

```bash
pnpm panda spec

# Generate specs in custom directory
pnpm panda spec --outdir ./theme-specs
```

| Flag                  | Description                     | Related                                                |
| --------------------- | ------------------------------- | ------------------------------------------------------ |
| `--silent`            | Whether to suppress all output  | [`config.logLevel`](/docs/references/config#log-level) |
| `--outdir <dir>`      | Output directory for spec files | -                                                      |
| `--config, -c <path>` | Path to Panda config file       | [`config`](/docs/references/config.md)                 |
| `--cwd <path>`        | Current working directory       | [`config.cwd`](/docs/references/config#cwd)            |

> The spec output represents your entire design system, everything available in your Panda setup. If you want to
> understand which tokens or recipes your app is actually using, you should use the
> [Panda Analyze](/docs/references/cli#analyze), not the spec.

## analyze

Analyze design token and recipe usage.

By default, it will analyze your project based on the `include` and `exclude` config options.

```bash
pnpm panda analyze

# analyze a specific file
pnpm panda analyze src/components/Button.tsx

# analyze a specific glob
pnpm panda analyze "src/components/**"

# analyze only token usage
pnpm panda analyze --scope token

# analyze only recipe usage
pnpm panda analyze --scope recipe
```

| Flag                   | Description                                  | Related                                                |
| ---------------------- | -------------------------------------------- | ------------------------------------------------------ |
| `--outfile <filepath>` | Output analyze report in given JSON filepath | -                                                      |
| `--silent`             | Whether to suppress all output               | [`config.logLevel`](/docs/references/config#log-level) |
| `--scope <type>`       | Select analysis scope (token or recipe)      | -                                                      |
| `--config, -c <path>`  | Path to Panda config file                    | [`config`](/docs/references/config.md)                 |
| `--cwd <path>`         | Current working directory                    | [`config.cwd`](/docs/references/config#cwd)            |

## debug

Debug design token extraction & CSS generated from files in glob.

More details in [Debugging](/docs/guides/debugging) docs.

```bash
pnpm panda debug

# Debug a specific file
pnpm panda debug src/components/Button.tsx

# Output to stdout without writing files
pnpm panda debug --dry

# Only output resolved config
pnpm panda debug --only-config
```

| Flag                  | Description                                                            | Related                                                   |
| --------------------- | ---------------------------------------------------------------------- | --------------------------------------------------------- |
| `--silent`            | Whether to suppress all output                                         | -                                                         |
| `--dry`               | Output debug files in stdout without writing to disk                   | -                                                         |
| `--outdir <dir>`      | Output directory for debug files, defaults to `../styled-system/debug` | -                                                         |
| `--only-config`       | Should only output the config file, default to 'false'                 | -                                                         |
| `--config, -c <path>` | Path to Panda config file                                              | [`config`](/docs/references/config.md)                    |
| `--cwd <path>`        | Current working directory                                              | [`config.cwd`](/docs/references/config#cwd)               |
| `--cpu-prof`          | Generate a `panda-{command}-{timestamp}.cpuprofile` file for profiling | [Debugging](/docs/guides/debugging#performance-profiling) |
| `--logfile <file>`    | Outputs logs to a file                                                 | [Debugging](/docs/guides/debugging)                       |

## ship

Ship extract result from files in glob.

By default it will extract from the entire project depending on your include and exclude options from your config file.

```bash
pnpm panda ship
# You can also analyze a specific file or folder
# using the optional glob argument
pnpm panda ship src/components/Button.tsx
pnpm panda ship "./src/components/**"
```

| Flag                   | Description                                                                                 | Related                                                |
| ---------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| `--outfile <filepath>` | Output path for the JSON build info file, default to './styled-system/panda.buildinfo.json' | -                                                      |
| `--silent`             | Whether to suppress all output                                                              | [`config.logLevel`](/docs/references/config#log-level) |
| `--minify, -m`         | Whether to minify the generated JSON                                                        | -                                                      |
| `--config, -c <path>`  | Path to Panda config file                                                                   | [`config`](/docs/references/config.md)                 |
| `--cwd <path>`         | Current working directory                                                                   | [`config.cwd`](/docs/references/config#cwd)            |
| `--watch, -w`          | Whether to watch for changes in the project                                                 | [`config.watch`](/docs/references/config#watch)        |
| `--poll, -p`           | Whether to poll for file changes                                                            | [`config.poll`](/docs/references/config#poll)          |

## emit-pkg

Emit package.json with entrypoints, can be used to create a workspace package dedicated to the
[`config.outdir`](/docs/references/config#outdir), in combination with
[`config.importMap`](/docs/references/config#importmap)

```bash
pnpm panda emit-pkg

# Specify output directory
pnpm panda emit-pkg --outdir styled-system
```

| Flag             | Description                                          | Related                                                |
| ---------------- | ---------------------------------------------------- | ------------------------------------------------------ |
| `--outdir <dir>` | The output directory for the generated CSS utilities | [`config.outdir`](/docs/references/config#outdir)      |
| `--base <path>`  | The base directory of the package.json entrypoints   | -                                                      |
| `--silent`       | Whether to suppress all output                       | [`config.logLevel`](/docs/references/config#log-level) |
| `--cwd <path>`   | Current working directory                            | [`config.cwd`](/docs/references/config#cwd)            |

## mcp

Start the MCP (Model Context Protocol) server for AI assistants. This exposes your design system to AI tools like
Claude, Cursor, VS Code Copilot, and more.

```bash
pnpm panda mcp

# With custom config path
pnpm panda mcp --config ./panda.config.ts

# Specify working directory
pnpm panda mcp --cwd ./my-project
```

| Flag                  | Description               | Related                                     |
| --------------------- | ------------------------- | ------------------------------------------- |
| `--config, -c <path>` | Path to Panda config file | [`config`](/docs/references/config.md)      |
| `--cwd <path>`        | Current working directory | [`config.cwd`](/docs/references/config#cwd) |

> Note: This command is typically started automatically by AI clients. See the [MCP Server guide](/docs/ai/mcp-server)
> for setup instructions.

## init-mcp

Initialize MCP configuration for AI clients. This creates the necessary configuration files for AI assistants to connect
to the Panda MCP server.

```bash
# Interactive mode - select clients from a list
pnpm panda init-mcp

# Configure specific clients
pnpm panda init-mcp --client claude,cursor

# Specify working directory
pnpm panda init-mcp --cwd ./my-project
```

| Flag               | Description                               | Related                                     |
| ------------------ | ----------------------------------------- | ------------------------------------------- |
| `--client <names>` | AI clients to configure (comma-separated) | -                                           |
| `--cwd <path>`     | Current working directory                 | [`config.cwd`](/docs/references/config#cwd) |

Supported clients: `claude`, `cursor`, `vscode`, `windsurf`, `codex`

See the [MCP Server guide](/docs/ai/mcp-server) for detailed usage and available tools.

---


## Configuring Panda

Customize how Panda works via the `panda.config.ts` file in your project.

Customize how Panda works via the `panda.config.ts` file in your project.

```js
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // your configuration options here...
})
```

## Output css options

### presets

**Type**: `string[]`

**Default**: `['@pandacss/preset-base', '@pandacss/preset-panda']`

The set of reusable and shareable configuration presets.

By default, any preset you add will be smartly merged with the default configuration, with your own configuration acting
as a set of overrides and extensions.

```json
{
  "presets": ["@pandacss/preset-base", "@pandacss/preset-panda"]
}
```

### eject

**Type**: `boolean`

**Default**: `false`

Whether to opt-out of the defaults config presets: [`@pandacss/preset-base`, `@pandacss/preset-panda`]

```json
{
  "eject": true
}
```

### preflight

**Type**: `boolean` | `{ scope: string; }`

**Default**: `false`

Whether to enable css reset styles. See also [Global styles](/docs/concepts/global-styles) for how reset interacts with global variables and layering.

Enable preflight:

```json
{
  "preflight": true
}
```

You can also scope the preflight; Especially useful for being able to scope the CSS reset to only a part of the app for
some reason.

Enable preflight and customize the scope:

```json
{
  "preflight": { "scope": ".extension" }
}
```

The resulting `reset` css would look like this:

```css
.extension button,
.extension select {
  text-transform: none;
}

.extension table {
  text-indent: 0;
  border-color: inherit;
  border-collapse: collapse;
}
```

You can also set the level to `element` (defaults to `parent`) to only reset the elements that have the scope class
assigned.

```json
{
  "preflight": { "scope": ".extension", "level": "element" }
}
```

The resulting `reset` css would look like this:

```css
button.extension,
select.extension {
  text-transform: none;
}

table.extension {
  text-indent: 0;
  border-color: inherit;
  border-collapse: collapse;
}
```

### emitTokensOnly

**Type**: `boolean`

**Default**: `false`

Whether to only emit the `tokens` directory

```json
{
  "emitTokensOnly": false
}
```

### prefix

**Type**: `string`

The namespace prefix for the generated css classes and css variables.

Ex: when using a prefix of `panda-`

```json
{
  "prefix": "panda"
}
```

```tsx
import { css } from '../styled-system/css'

const App = () => {
  return <div className={css({ color: 'blue.500' })} />
}
```

would result in:

```css
.panda-text_blue\.500 {
  color: var(--panda-colors-blue-500);
}
```

### layers

**Type**: `Partial<Layer>`

Cascade layers used in generated css.

Ex: when customizing the utilities layer

```json
{
  "layers": {
    "utilities": "panda_utilities"
  }
}
```

```tsx
import { css } from '../styled-system/css'

const App = () => {
  return <div className={css({ color: 'blue.500' })} />
}
```

would result in:

```css
@layer panda_utilities {
  .text_blue\.500 {
    color: var(--colors-blue-500);
  }
}
```

You should update the layer in your root css also.

### separator

**Type**: `'_' | '=' | '-'`

**Default**: `'_'`

The separator for the generated css classes.

```json
{
  "separator": "_"
}
```

Using a `=` with:

```tsx
import { css } from '../styled-system/css'

const App = () => {
  return <div className={css({ color: 'blue.500' })} />
}
```

would result in:

```css
.text\=blue\.500 {
  color: var(--colors-blue-500);
}
```

### minify

**Type**: `boolean`

**Default**: `false`

Whether to minify the generated css. This can be set to `true` to reduce the size of the generated css.

```json
{
  "minify": false
}
```

### hash

**Type**: `boolean | { cssVar: boolean; className: boolean }`

**Default**: `false`

Whether to hash the generated class names / css variables. This is useful if want to shorten the class names or css
variables.

Hash the class names and css variables:

```json
{
  "hash": true
}
```

This

```tsx
import { css } from '../styled-system/css'

const App = () => {
  return <div className={css({ color: 'blue.500' })} />
}
```

would result in something that looks like:

```css
.dOFUTE {
  color: var(--cgpxvS);
}
```

You can also hash them individually.

E.g. only hash the css variables:

```json
{
  "hash": { "cssVar": true, "className": false }
}
```

Then the result looks like this:

```css
.text_blue\.500 {
  color: var(--cgpxvS);
}
```

Now only hash the class names:

```json
{
  "hash": { "cssVar": false, "className": true }
}
```

Then the result looks like this:

```css
.dOFUTE {
  color: var(--colors-blue-500);
}
```

## File system options

### gitignore

**Type**: `boolean`

**Default**: `true`

Whether to update the .gitignore file.

```json
{
  "gitignore": true
}
```

Will add the following to your `.gitignore` file:

```txt
# Panda
styled-system
styled-system-static
```

### cwd

**Type**: `string`

**Default**: `process.cwd()`

The current working directory.

```json
{
  "cwd": "src"
}
```

### clean

**Type**: `boolean`

**Default**: `false`

Whether to clean the output directory before generating the css.

```json
{
  "clean": false
}
```

### outdir

**Type**: `string`

**Default**: `styled-system`

The output directory for the generated css.

```json
{
  "outdir": "styled-system"
}
```

### importMap

**Type**: `string | Partial<OutdirImportMap>`

**Default**:
`{ "css": "styled-system/css", "recipes": "styled-system/recipes", "patterns": "styled-system/patterns", "jsx": "styled-system/jsx" }`

Allows you to customize the import paths for the generated outdir.

```json
{
  "importMap": {
    "css": "@acme/styled-system",
    "recipes": "@acme/styled-system",
    "patterns": "@acme/styled-system",
    "jsx": "@acme/styled-system"
  }
}
```

You can also use a string to customize the base import path and keep the default entrypoints:

```json
{
  "importMap": "@scope/styled-system"
}
```

is the equivalent of:

```json
{
  "importMap": {
    "css": "@scope/styled-system/css",
    "recipes": "@scope/styled-system/recipes",
    "patterns": "@scope/styled-system/patterns",
    "jsx": "@scope/styled-system/jsx"
  }
}
```

Check out the [Component Library](/docs/guides/component-library) guide for more information on how to use the
`importMap` option.

### include

**Type**: `string[]`

**Default**: `[]`

List of files glob to watch for changes.

```json
{
  "include": ["./src/**/*.{js,jsx,ts,tsx}", "./pages/**/*.{js,jsx,ts,tsx}"]
}
```

### exclude

**Type**: `string[]`

**Default**: `[]`

List of files glob to ignore.

```json
{
  "exclude": []
}
```

### dependencies

**Type**: `string[]`

**Default**: `[]`

Explicit list of config related files that should trigger a context reload on change.

> We automatically track the config file and (transitive) files imported by the config file as much as possible, but
> sometimes we might miss some. You can use this option as a workaround for those edge cases.

```json
{
  "dependencies": ["path/to/files/**.ts"]
}
```

### watch

**Type**: `boolean`

**Default**: `false`

Whether to watch for changes and regenerate the css.

```json
{
  "watch": false
}
```

### poll

**Type**: `boolean`

**Default**: `false`

Whether to use polling instead of filesystem events when watching.

```json
{
  "poll": false
}
```

### outExtension

**Type**: `'mjs' | 'js'`

**Default**: `mjs`

File extension for generated javascript files.

```json
{
  "outExtension": "mjs"
}
```

### forceConsistentTypeExtension

**Type**: `boolean`

**Default**: `false`

Whether to force consistent type extensions for generated typescript .d.ts files.

If set to `true` and `outExtension` is set to `mjs`, the generated typescript `.d.ts` files will have the extension
`.d.mts`.

```json
{
  "forceConsistentTypeExtension": true
}
```

### syntax

**Type**: `'object-literal' | 'template-literal'`

**Default**: `object-literal`

Decides which syntax to use when writing CSS. For existing projects, you might need to run the `panda codegen --clean`.

```json
{
  "syntax": "template-literal"
}
```

Ex object-literal:

```tsx
const styles = css({
  backgroundColor: 'gainsboro',
  padding: '10px 15px'
})
```

Ex template-literal:

```tsx
const Container = styled.div`
  background-color: gainsboro;
  padding: 10px 15px;
`
```

### lightningcss

**Type**: `boolean`

**Default**: `false`

Whether to use `lightningcss` instead of `postcss` for css optimization.

```json
{
  "lightningcss": true
}
```

### browserslist

**Type**: `string[]`

**Default**: `[]`

Browserslist query to target specific browsers. Only used when `lightningcss` is set to `true`.

```json
{
  "browserslist": ["last 2 versions", "not dead", "not < 2%"]
}
```

### polyfill

**Type**: `boolean`

**Default**: `false`

Polyfill CSS @layers at-rules for older browsers.

```json
{
  "polyfill": true
}
```

## Design token options

### shorthands

**Type**: `boolean`

**Default**: `true`

Whether to allow shorthand properties

```json
{
  "shorthands": true
}
```

Ex `true`:

```tsx
const styles = css({
  bgColor: 'gainsboro',
  p: '10px 15px'
})
```

Ex false:

```tsx
const styles = css({
  backgroundColor: 'gainsboro',
  padding: '10px 15px'
})
```

### cssVarRoot

**Type**: `string`

**Default**: `:where(:host, :root)`

The root selector for the css variables.

```json
{
  "cssVarRoot": ":where(:host, :root)"
}
```

### conditions

**Type**: `Extendable<Conditions>`

**Default**: `{}`

The css selectors or media queries shortcuts.

```json
{
  "conditions": { "hover": "&:hover" }
}
```

### globalCss

**Type**: `Extendable<GlobalStyleObject>`

**Default**: `{}`

The global styles for your project.

```json
{
  "globalCss": {
    "html, body": {
      "margin": 0,
      "padding": 0
    }
  }
}
```

### theme

**Type**: `Extendable<Theme>`

**Default**: `{}`

The theme configuration for your project.

```json
{
  "theme": {
    "tokens": {
      "colors": {
        "red": { "value": "#EE0F0F" },
        "green": { "value": "#0FEE0F" }
      }
    },
    "semanticTokens": {
      "colors": {
        "danger": { "value": "{colors.red}" },
        "success": { "value": "{colors.green}" }
      }
    }
  }
}
```

### themes

**Type**: `Extendable<ThemeVariantsMap>`

**Default**: `{}`

The theme variants configuration for your project.

```json
{
  "themes": {
    "primary": {
      "tokens": {
        "colors": {
          "text": { "value": "red" }
        }
      },
      "semanticTokens": {
        "colors": {
          "muted": { "value": "{colors.red.200}" },
          "body": {
            "value": {
              "base": "{colors.red.600}",
              "_osDark": "{colors.red.400}"
            }
          }
        }
      }
    },
    "secondary": {
      "tokens": {
        "colors": {
          "text": { "value": "blue" }
        }
      },
      "semanticTokens": {
        "colors": {
          "muted": { "value": "{colors.blue.200}" },
          "body": {
            "value": {
              "base": "{colors.blue.600}",
              "_osDark": "{colors.blue.400}"
            }
          }
        }
      }
    }
  }
}
```

### utilities

**Type**: `Extendable<UtilityConfig>`

**Default**: `{}`

The css utility definitions.

```js
{
  "utilities": {
    extend: {
      borderX: {
        values: ['1px', '2px', '4px'],
        shorthand: 'bx', // `bx` or `borderX` can be used
        transform(value, token) {
          return {
            borderInlineWidth: value,
            borderColor: token('colors.red.200'), // read the css variable for red.200
          }
        },
      },
    },
  }
}
```

### patterns

**Type**: `Extendable<Record<string, AnyPatternConfig>>`

**Default**: `{}`

Common styling or layout patterns for your project.

```js
{
  "patterns": {
    extend: {
      // Extend the default `flex` pattern
      flex: {
        properties: {
          // only allow row and column
          direction: { type: 'enum', value: ['row', 'column'] },
        },
      },
    },
  },
}
```

### staticCss

**Type**: `StaticCssOptions`

**Default**: `{}`

Used to generate css utility classes for your project.

```js
{
  "staticCss": {
    css: [
      {
        properties: {
          margin: ['*'],
          padding: ['*', '50px', '80px'],
        },
        responsive: true,
      },
      {
        properties: {
          color: ['*'],
          backgroundColor: ['green.200', 'red.400'],
        },
        conditions: ['light', 'dark'],
      },
    ],
  },
}
```

### strictTokens

**Type**: `boolean`

**Default**: `false`

Only allow token values and prevent custom or raw CSS values. Will only affect properties that have config tokens, such
as `color`, `bg`, `borderColor`, etc. [Learn more.](/docs/concepts/writing-styles#type-safety)

```json
{
  "strictTokens": false
}
```

### strictPropertyValues

**Type**: `boolean`

**Default**: `false`

Only use valid CSS values for properties that do have a predefined list of values. Will throw for properties that do not
have config tokens, such as `display`, `content`, `willChange`, etc.
[Learn more.](/docs/concepts/writing-styles#type-safety)

```json
{
  "strictPropertyValues": false
}
```

### globalFontface

**Type**: `GlobalFontfaceDefinition`

**Default**: `{}`

Global font face definitions.

```json
{
  "globalFontface": {
    "Inter": {
      "src": "url(/fonts/inter.woff2) format('woff2')",
      "fontWeight": "400",
      "fontStyle": "normal"
    },
    "Roboto": {
      "src": "url(/fonts/roboto.woff2) format('woff2')",
      "fontWeight": "400",
      "fontStyle": "normal"
    }
  }
}
```

Check out the [Custom Fonts](/docs/guides/fonts#global-font-face) guide for more information on how to use the
`globalFontface` option.

## JSX options

### jsxFramework

**Type**: `'react' | 'solid' | 'preact' | 'vue' | 'qwik' | (string & {})`

JS Framework for generated JSX elements.

```json
{
  "jsxFramework": "react"
}
```

### jsxFactory

**Type**: `string`

The factory name of the element

```json
{
  "jsxFactory": "panda"
}
```

Ex:

```tsx
<panda.button marginTop="40px">Click me</panda.button>
```

### jsxStyleProps

**Type**: `all` | `minimal` | `none`

**Default**: `all`

The style props allowed on generated JSX components

- When set to 'all', all style props are allowed.
- When set to 'minimal', only the `css` prop is allowed.
- When set to 'none', no style props are allowed and therefore the `jsxFactory` will not be usable as a component:
  - `<styled.div />` and `styled("div")` aren't valid
  - but the recipe usage is still valid `styled("div", { base: { color: "red.300" }, variants: { ...} })`

Ex with 'all':

```jsx
<styled.button marginTop="40px">Click me</styled.button>
```

Ex with 'minimal':

```jsx
<styled.button css={{ marginTop: '40px' }}>Click me</styled.button>
```

Ex with 'none':

```jsx
<button className={css({ marginTop: '40px' })}>Click me</button>
```

## Documentation options

### studio

**Type**: `Partial<Studio>`

**Default**: `{ title: 'Panda', logo: '🐼' }`

Used to customize the design system studio

```json
{
  "studio": {
    "logo": "🐼",
    "title": "Panda"
  }
}
```

### log level

**Type**: `'debug' | 'info' | 'warn' | 'error' | 'silent'`

**Default**: `info`

The log level for the built-in logger.

```json
{
  "logLevel": "info"
}
```

### validation

**Type**: `'none' | 'warn' | 'error'`

**Default**: `warn`

The validation strictness to use when validating the config.

- When set to 'none', no validation will be performed.
- When set to 'warn', warnings will be logged when validation fails.
- When set to 'error', errors will be thrown when validation fails.

```json
{
  "validation": "error"
}
```

## Other options

### Hooks

**Type**: `PandaHooks`

Panda provides a set of callbacks that you can hook into for more advanced use cases. Check the
[Hooks](/docs/concepts/hooks) docs for more information.

### Plugins

**Type**: `PandaPlugin[]`

Plugins are currently simple objects that contain a `name` and a `hooks` object with the same structure as the `hooks`
object in the config.

They will be called in sequence in the order they are defined in the `plugins` array, with the user's config called
last.

```ts
import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  // ...
  plugins: [
    {
      name: 'token-format',
      hooks: {
        'tokens:created': ({ configure }) => {
          configure({
            formatTokenName: path => '$' + path.join('-')
          })
        }
      }
    }
  ]
})
```

---


---

_This is the complete Panda CSS documentation, automatically generated from the official sources._
