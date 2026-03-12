# @goddard/tree-build

A lightweight, zero-configuration topological build orchestrator for workspace dependencies.

## Purpose

When building a subpackage in a monorepo, you often need its local workspace dependencies (other packages in the same repo) to be built first. Standard tools like `pnpm -r run build` build the *entire* repo, which is slow. Manual ordering is brittle.

`tree-build` solves this by:
1.  **Deep Introspection**: It checks the current package's `node_modules` for symlinks pointing back into the workspace.
2.  **Topological Graphing**: It recursively maps the entire dependency tree of the current package.
3.  **Parallel Execution**: It runs the `build` scripts of all dependencies in the correct order, parallelizing independent branches of the tree.
4.  **Recursion Safety**: It uses environment flags to ensure that if a dependency also calls `tree-build`, it doesn't trigger a redundant or infinite build loop.

## Usage

Add `@goddard/tree-build` to your workspace root's `devDependencies`.

In any subpackage's `package.json`:

```json
{
  "name": "@goddard/sdk",
  "scripts": {
    "build": "tree-build && tsdown"
  }
}
```

### Custom Build Command

By default, it runs `npm run build` on dependencies. You can specify a different script name as an argument:

```json
{
  "scripts": {
    "build:fast": "tree-build build:fast && tsdown --fast"
  }
}
```

## How it Works

It relies on the standard behavior of workspace managers (`pnpm`, `npm`, `yarn`) where local packages are symlinked into `node_modules`. 

- It resolves symlinks in `node_modules`.
- If a symlink points *outside* of a `node_modules` directory, it identifies it as a workspace package.
- It builds a dependency graph and executes the builds from the leaves up to the root.
- The package that originally invoked `tree-build` is excluded from the internal build queue (since its own build process is already running and waiting for `tree-build` to finish).
