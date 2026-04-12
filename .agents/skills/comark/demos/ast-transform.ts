import { parse } from 'comark'
import { renderMarkdown } from 'comark/render'
import { visit } from 'comark/utils'

const source = `# Resources

- [Comark](https://comark.dev)
- [Internal wiki](/wiki/comark)
`

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

console.log(normalized)
