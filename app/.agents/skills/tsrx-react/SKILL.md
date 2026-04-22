---
name: tsrx-react
description: Build, refactor, debug, review, and explain React `.tsrx` code written with TSRX. Use when Codex must translate JSX or TSX into TSRX syntax, preserve statement-based JSX and lexical template scoping, work with lazy destructuring or refs, edit scoped style blocks, or handle template control flow such as `if`, `for`, `switch`, and `try` / `pending` / `catch`.
---

# tsrx-react

Use this skill when working in React codebases that author UI in `.tsrx`. Treat TSRX as its own source language for React rather than as JSX with a few macros.

## Overview

TSRX is a TypeScript language extension for authoring React UI in `.tsrx` files.

Key traits:

- Treat JSX elements as statements rather than expressions.
- Keep control flow directly in the template with `if`, `for`, `switch`, and `try`.
- Keep locals scoped near the JSX that uses them.
- Support lazy destructuring with `&{ ... }` and `&[ ... ]`.
- Scope component styles automatically with hashed selectors.
- Preserve TypeScript types through the compile step.

## Mental Model

- Treat `.tsrx` as a React authoring language, not as plain JSX with a few helpers.
- Keep markup, control flow, local declarations, and style blocks close together in the component body.
- Prefer local clarity over JSX habits carried over from function components.
- Let TSRX features such as lexical scoping, lazy destructuring, and statement-position JSX do the structural work instead of recreating JSX-era patterns.

## Start Here

- Inspect the current `.tsrx` authoring patterns first: component shape, control flow, refs, styles, and any use of top-level `await`.
- Load only the reference files needed for the task:
  - Read [components-and-expression-rules.md](./references/components-and-expression-rules.md) before editing component declarations, text, props, refs, children, or `<tsx>` expression-position JSX.
  - Read [control-flow-and-styles.md](./references/control-flow-and-styles.md) before editing `if`, `for`, `switch`, `try` / `pending` / `catch`, early returns, or scoped styles.
  - Read [react-behavior-and-patterns.md](./references/react-behavior-and-patterns.md) before relying on React hook lifting, top-level `await`, or common escape-hatch patterns.
- Preserve the existing React semantics before abstracting anything.

## Workflow

1. Identify the current React authoring pattern and its constraints.
   - Check how components express guards, loops, nested scopes, and refs.
   - Check whether components rely on top-level component-body `await`.
   - Check how local style blocks and passed-through `#style` classes are used.
2. Apply TSRX syntax instead of JSX habits.
   - Declare UI building blocks with `component` or `export component`.
   - Place JSX elements directly in the component body as statements.
   - Wrap text and inline values in `{...}`. Bare text is invalid.
   - Use `<tsx>...</tsx>` only when JSX must appear in expression position.
   - Use a bare `return;` only to stop later template output after rendering a guard branch.
3. Use TSRX-specific features deliberately.
   - Use `&{ ... }` or `&[ ... ]` lazy destructuring when deferred property or index access is clearer than repeated lookups.
   - Use `{ref variable}` or `{ref callback}` instead of inventing custom React ref plumbing.
   - Keep local declarations next to the JSX they feed; nested element bodies create lexical scopes.
   - Use `children={expr}` when children is computed rather than nested JSX.
4. Preserve React behavior and safety.
   - Do not `return <JSX />`, `return someValue`, or assign bare JSX to variables outside `<tsx>`.
   - Do not mark components `async`.
   - Use top-level component-body `await` only when the surrounding React app expects it.
   - Do not use `for await...of` inside React component templates.
   - Do not pass scoped classes across component boundaries except through `#style.className`.
5. Validate the generated behavior after editing.
   - Check that keyed loops, refs, and lazy destructuring still behave correctly in the generated React code.
   - Check that scoped styles stay local and that any `:global(...)` selector is intentional.
   - Check that guard clauses and async boundaries still lower to the intended React behavior.

## Search Shortcuts

- Search [components-and-expression-rules.md](./references/components-and-expression-rules.md) with `rg -n "component|<tsx>|\\{text|&\\{|&\\[|ref|children"`.
- Search [control-flow-and-styles.md](./references/control-flow-and-styles.md) with `rg -n "if|for|index|key|switch|pending|catch|return;|<style>|:global|#style"`.
- Search [react-behavior-and-patterns.md](./references/react-behavior-and-patterns.md) with `rg -n "hook|await|for await|async|TypeScript|inline function"`.
