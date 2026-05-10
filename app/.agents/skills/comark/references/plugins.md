# Plugins

Use this reference when the task adds, removes, configures, or authors Comark plugins.

## Choose The Package Layer

- Use `comark/plugins/...` for parse-time plugins.
- Use `@comark/react/plugins/...` when the React renderer exposes plugin helpers or companion render components.
- Reuse a configured parser when plugins are expensive to initialize, especially syntax highlighting.

## Built-In Plugin Map

- `alert`
  - built in and enabled by default
  - turns GitHub-style alert blockquotes into alert nodes
- `highlight`
  - use for fenced code highlighting
  - usually configure Shiki themes and optional languages
  - prefer parser reuse because highlighter startup is relatively expensive
- `math`
  - use for `$...$` and `$$...$$`
  - requires `katex`
  - framework integrations usually also export a `Math` component
- `mermaid`
  - use for ```mermaid fences
  - requires `beautiful-mermaid`
  - framework integrations usually also export a `Mermaid` component
- `security`
  - sanitize user-controlled markdown
  - use `blockedTags`, `allowedProtocols`, and `allowDataImages` to tighten input handling
- `summary`
  - extract content before `<!-- more -->`
  - summary nodes land in `tree.meta.summary`
- `toc`
  - generate a heading tree in `tree.meta.toc`
  - `depth` controls included heading levels and `searchDepth` controls traversal depth inside nested structures
- `headings`
  - extract top-of-document title and description into `tree.meta`
  - can remove the source nodes from rendered output
- `emoji`
  - replace shortcode syntax such as `:rocket:` during parsing
- `task-list`
  - convert GitHub task list syntax into checkbox markup
  - useful because it runs before inline parsing and avoids `[ ]` or `[x]` conflicts

## Common Configuration Patterns

```ts
import { createParse } from 'comark'
import emoji from 'comark/plugins/emoji'
import security from 'comark/plugins/security'
import summary from 'comark/plugins/summary'
import toc from 'comark/plugins/toc'

const parseContent = createParse({
  plugins: [
    security({
      blockedTags: ['script', 'iframe', 'object', 'embed'],
      allowedProtocols: ['https', 'mailto'],
      allowDataImages: false,
    }),
    toc({ depth: 3 }),
    summary(),
    emoji(),
  ],
})
```

## Author A Custom Plugin

- Use `defineComarkPlugin` from `comark/parse`.
- Use `pre(state)` to rewrite raw markdown before tokenization.
- Use `post(state)` to inspect or mutate the finished tree and to populate `state.tree.meta`.
- Use `visit` from `comark/utils` when the transform is easier as a tree walk.

```ts
import { defineComarkPlugin } from 'comark/parse'
import { visit } from 'comark/utils'

export const wordCount = defineComarkPlugin(() => ({
  name: 'word-count',
  post(state) {
    let count = 0

    visit(
      state.tree,
      (node) => typeof node === 'string',
      (node) => {
        count += String(node).trim().split(/\s+/).filter(Boolean).length
      },
    )

    state.tree.meta.wordCount = count
  },
}))
```

## Integration Warnings

- Keep plugin setup on the parse side whenever possible.
- If a plugin ships a render component, register it under the same tag name the parser emits.
- Treat docs examples that mix parse and render helpers as patterns, not guarantees that every helper shares the same option shape. When in doubt, keep parsing and rendering as two explicit steps.
