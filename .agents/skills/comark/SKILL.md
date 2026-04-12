---
name: comark
description: Implement, refactor, debug, and review code that uses Comark for Markdown parsing, component-in-Markdown authoring, AST transforms, streaming markdown, or React rendering. Use when Codex needs to wire `parse`, `createParse`, `autoCloseMarkdown`, `renderMarkdown`, `Comark`, or `ComarkRenderer`; configure built-in plugins such as `highlight`, `math`, `mermaid`, `security`, `summary`, `toc`, `headings`, `emoji`, or `task-list`; map custom React components; or split parse-time work on the server from render-time work in React.
---

# comark

Use Comark when the task touches its parser, AST, component syntax, plugin system, or framework renderers. Keep parsing concerns and rendering concerns separate, and only load the reference file that matches the layer you are editing.

## Start Here

- Identify which layer the task touches before editing:
  - authoring syntax or AST shape
  - parse pipeline or serialization
  - framework renderer
  - plugin setup or custom plugin authoring
  - streaming or server/client split
- Load only the references you need:
  - [syntax-and-ast.md](./references/syntax-and-ast.md) for component syntax, attributes, AST tuples, and tree transforms
  - [rendering-and-streaming.md](./references/rendering-and-streaming.md) for `parse`, `createParse`, React rendering, `ComarkRenderer`, and streaming
  - [plugins.md](./references/plugins.md) for built-in plugins, peer dependencies, and custom plugin hooks
- Open the closest demo before writing new code from scratch. The demos cover the common integration paths.

## Workflow

- Decide whether the task should parse markdown now or receive a prebuilt `ComarkTree`.
- Prefer `createParse()` or a reusable renderer when processing more than one document or handling repeated requests.
- Keep plugins on the parse side. If the client already has a `ComarkTree`, render with `ComarkRenderer` instead of reparsing in the browser.
- Match markdown component tags to the keys in `components` or `componentsManifest`.
- Use `autoClose` plus the renderer `streaming` and `caret` props for live AI output or incremental previews.
- When transforming content, operate on `[tag, props, ...children]` tuples and re-serialize with `renderMarkdown()` only if the task needs markdown output instead of React UI output.

## Decision Rules

- Read [syntax-and-ast.md](./references/syntax-and-ast.md) before changing component syntax, attributes, tuple handling, or AST transforms.
- Read [rendering-and-streaming.md](./references/rendering-and-streaming.md) before editing imports across `comark` and `@comark/react`, or before implementing server-side parsing, client-side React rendering, or streaming.
- Read [plugins.md](./references/plugins.md) before adding highlighting, math, mermaid, security, summaries, TOCs, heading extraction, task lists, or a custom plugin.
- Use the demos as templates, not as fixed APIs. Adapt them to the surrounding framework, data flow, and component model that already exists in the codebase.

## Defaults And Caveats

- `parse()` is async.
- `autoClose` defaults to `true`; disable it only when you call `autoCloseMarkdown()` manually.
- React `<Comark>` does not require `Suspense`.
- `ComarkRenderer` only renders a `ComarkTree`; it does not parse markdown.
- The alerts plugin is built in and enabled by default. Task lists, TOC, summary, headings, security, math, mermaid, emoji, and highlighting are explicit plugin choices.
- Math and mermaid integrations require their documented peer dependencies.

## Demo Map

- [react-comark.tsx](./demos/react-comark.tsx): render markdown plus custom components in React
- [react-streaming.tsx](./demos/react-streaming.tsx): stream LLM output with `streaming` and `caret`
- [server-renderer.tsx](./demos/server-renderer.tsx): parse on the server and render a serialized tree with `ComarkRenderer`
- [ast-transform.ts](./demos/ast-transform.ts): traverse a `ComarkTree`, mutate nodes, and round-trip with `renderMarkdown`
- [custom-plugin.ts](./demos/custom-plugin.ts): define a parse-time plugin with `post()`
