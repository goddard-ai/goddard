# Rendering And Streaming

Use this reference when the task changes the parse/render boundary, React bindings, server/client splitting, or live streaming behavior.

## Pick The Right Entry Point

- Use `parse(source, options?)` for one-off parsing.
- Use `createParse(options?)` when the same plugin configuration is reused across requests, files, or pages.
- Use `renderMarkdown(tree)` when the task needs normalized markdown output after AST changes.
- Use `<Comark>` when the component should parse and render in one step.
- Use `<ComarkRenderer>` when parsing happened elsewhere and the UI only needs to render a serialized tree.

## React Notes

- React:
  - import from `@comark/react`
  - `<Comark>` does not require `Suspense`
  - pass markdown through `children` or `markdown`

## Server Parse, Client Render

- Parse on the server when plugins, syntax highlighting, TOCs, or sanitation should not ship to the client bundle.
- Serialize the `ComarkTree` as plain JSON.
- Render the tree with `ComarkRenderer` in React.
- Do not call `parse()` in a client component when a server route or build step can do it once.

```ts
import { createParse } from 'comark'
import toc from 'comark/plugins/toc'

const parseDoc = createParse({
  plugins: [toc({ depth: 3 })],
})

const tree = await parseDoc(markdown)
```

```tsx
import { ComarkRenderer } from '@comark/react'

export function DocPage({ tree }: { tree: ComarkTree }) {
  return <ComarkRenderer tree={tree} />
}
```

## Streaming Rules

- `autoClose` defaults to `true`, so streamed partial markdown stays renderable without extra setup.
- While chunks are still arriving, set `streaming={true}` and usually `caret` as well.
- The caret disappears automatically when `streaming` becomes `false`.
- If you call `autoCloseMarkdown()` yourself on each chunk, then parse with `autoClose: false` to avoid double-closing.
- For the final parse, parse the raw completed markdown without the manual auto-close override.

```ts
import { autoCloseMarkdown, parse } from 'comark'

let accumulated = ''

async function onChunk(chunk: string) {
  accumulated += chunk
  const closed = autoCloseMarkdown(accumulated)
  return parse(closed, { autoClose: false })
}

async function onComplete() {
  return parse(accumulated)
}
```

## Performance Defaults

- Reuse parser instances for repeated parsing, especially when syntax highlighting is involved.
- Keep plugin configuration stable instead of recreating parsers on every render.
- Use `ComarkRenderer` when the parsed tree is already available; reparsing defeats the point of splitting parse and render.
