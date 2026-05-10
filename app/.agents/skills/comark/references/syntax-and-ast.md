# Syntax And AST

Use this reference when the task changes authored markdown, custom component syntax, native attribute syntax, or tuple-level AST transforms.

## Authoring Primitives

- Block components use `::name{...}` on their own line and close with `::`.
- Inline components use `:name`, `:name[children]`, `:name{props}`, or `:name[children]{props}` inside text.
- Native markdown elements accept trailing `{...}` attributes.
- YAML props inside a block component must appear at the very start of the component body.
- Headings get generated IDs automatically.
- Alerts use normal blockquote syntax with `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, or `[!CAUTION]`; the alert transform is built in.

```md
::callout{type="warning" .compact #ship-notes}
---
title: Breaking change
pinned: true
---
Ship the migration before Friday.
::
```

```md
This release adds :badge[beta]{color="amber"} support for
[external links](https://example.com){target="_blank" rel="noopener"}.
```

## Attribute Rules

- `{bool}` becomes a boolean prop.
- `{#id}` writes an `id`.
- `{.class-one .class-two}` appends classes.
- `{key="value"}` writes a string prop or attribute.
- JSON-like values can be passed in quoted form such as `{config='{"theme":"dark"}'}`.
- Span attributes wrap inline content in a `span`, so `[Warning]{.badge .danger}` is valid even when you do not need a custom component.

## AST Shape

- `ComarkTree` has `nodes`, `frontmatter`, and `meta`.
- A text node is a plain string.
- An element node is `[tag, props, ...children]`.
- Component tags and native tags share the same tuple format, so a custom `alert` node and a native `p` node are handled the same way during traversal.

```ts
import type { ComarkNode, ComarkTree } from 'comark'

const tree: ComarkTree = {
  frontmatter: { title: 'Release Notes' },
  meta: {},
  nodes: [
    ['h1', {}, 'Release Notes'],
    ['p', {}, 'Shipped features and migration notes.'],
    ['alert', { type: 'info' }, 'Upgrade before Friday.'],
  ],
}

function isElement(node: ComarkNode): node is [string, Record<string, unknown>, ...ComarkNode[]] {
  return Array.isArray(node)
}
```

## Tree Transforms

- Parse first, then mutate tuples in place.
- Reach for `visit` from `comark/utils` when the task is easier as a predicate plus visitor callback.
- Keep transforms narrow. Change only the nodes needed for the task, then render to markdown or HTML afterward.

```ts
import { parse } from 'comark'
import { renderMarkdown } from 'comark/render'
import { visit } from 'comark/utils'

const tree = await parse(source)

visit(
  tree,
  (node) => Array.isArray(node) && node[0] === 'a',
  (node) => {
    const link = node as [string, Record<string, unknown>, ...unknown[]]
    const href = typeof link[1].href === 'string' ? link[1].href : ''
    if (href.startsWith('http')) {
      link[1].target = '_blank'
      link[1].rel = 'noopener noreferrer'
    }
  },
)

const normalized = await renderMarkdown(tree)
```

## Frontmatter And HTML

- Frontmatter is parsed into `tree.frontmatter`.
- Plugin-derived metadata such as TOC or summary lives in `tree.meta`.
- Embedded HTML is parsed into AST nodes by default.
- Set `html: false` only when literal HTML text must remain unparsed.
