# @goddard/tree-run

A lightweight script runner for workspace dependencies.

## Purpose

When building a subpackage in a monorepo, you often need its local workspace dependencies (other packages in the same repo) to be built first. Standard tools like `pnpm -r run build` build the *entire* repo, which is slow. Manual ordering is brittle.

`tree-run` solves this by:
1.  Walking the current package's installed workspace dependencies through `node_modules`.
2.  Visiting that dependency tree depth-first so dependencies run before dependents.
3.  Reusing the explicit script name you pass in for each dependency.
4.  Optionally starting from workspace leaf packages with `--leaves`.
5.  Short-circuiting nested invocations so packages can keep `tree-run` in their own scripts.

## Usage

Add `@goddard/tree-run` to your workspace root's `devDependencies`.

In any subpackage's `package.json`:

```json
{
  "name": "@goddard/sdk",
  "scripts": {
    "build": "tree-run build && tsdown"
  }
}
```

### Other Commands

Pass the script name you want to run on dependency packages:

```json
{
  "scripts": {
    "build:fast": "tree-run build:fast && tsdown --fast"
  }
}
```

From the workspace root, `--leaves` starts from packages that are not depended on by another workspace package and walks their dependency trees:

```json
{
  "scripts": {
    "typecheck": "tree-run --leaves build:types && tree-run --leaves typecheck"
  }
}
```

## How it Works

It relies on the standard behavior of workspace managers (`pnpm`, `npm`, `yarn`) where local packages are symlinked into `node_modules`. 

- It resolves symlinks in `node_modules`.
- If a symlink points *outside* of a `node_modules` directory, it identifies it as a workspace package.
- It recursively visits workspace dependencies from the leaves up.
- The package that originally invoked `tree-run` is skipped because its own script is already in progress.
