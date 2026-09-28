import { describe, expect, test } from 'bun:test'
import { fromMarkdown } from 'mdast-util-from-markdown'
import {
  applyBlockquoteIndent,
  markdownBlockquoteIndentPlugin,
  type MdastNode,
} from './markdown-blockquote-indent'

const I = ' '

function parse(source: string): MdastNode {
  return fromMarkdown(source) as MdastNode
}

function quoteText(tree: MdastNode, source: string): unknown {
  applyBlockquoteIndent(tree, source)
  return tree.children?.[0]
}

describe('blockquote indent', () => {
  test('restores whitespace past the single marker space', () => {
    const quote = quoteText(parse('>   a\n> b\n>>    c'), '>   a\n> b\n>>    c') as MdastNode
    const first = quote.children?.[0]
    // "  a b": two kept spaces, joined soft break, plain marker line.
    expect(first?.children?.map((n) => n.value ?? '').join('')).toBe(`${I}${I}a\nb`)
    const nested = quote.children?.[1]?.children?.[0]
    expect(nested?.children?.map((n) => n.value ?? '').join('')).toBe(`${I}${I}${I}c`)
  })

  test('leaves indented and fenced code blocks literal', () => {
    const quote = quoteText(parse('>     code'), '>     code') as MdastNode
    expect(quote.children?.[0].type).toBe('code')
    expect(quote.children?.[0].value).toBe('code')
  })

  test('restores the lead before inline spans and inside list items', () => {
    const quote = quoteText(
      parse('>   **b** rest'),
      '>   **b** rest',
    ) as MdastNode
    const para = quote.children?.[0]
    expect(para?.children?.[0].value).toBe(`${I}${I}`)
    expect(para?.children?.[1].type).toBe('strong')

    const listQuote = quoteText(
      parse('> - i\n>   cont'),
      '> - i\n>   cont',
    ) as MdastNode
    const itemPara = listQuote.children?.[0].children?.[0].children?.[0]
    expect(itemPara?.children?.map((n) => n.value ?? '').join('')).toBe(`i\n${I}${I}cont`)
  })

  test('ignores list and heading markers after the quote marker', () => {
    const quote = quoteText(parse('>   - item\n>   # head'), '>   - item\n>   # head') as MdastNode
    expect(quote.children?.[0].type).toBe('list')
    expect(quote.children?.[1].type).toBe('heading')
    const itemText = quote.children?.[0].children?.[0].children?.[0].children
    expect(itemText?.map((n) => n.value ?? '').join('')).toBe('item')
  })

  test('plugin factory applies the transform', () => {
    const source = '>   a'
    const tree = parse(source)
    markdownBlockquoteIndentPlugin(source)()(tree)
    expect(tree.children?.[0].children?.[0].children?.[0].value).toBe(`${I}${I}`)
  })
})
