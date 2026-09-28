/**
 * Re-attach whitespace CommonMark drops between a quoted line's `>` markers
 * and its text — the web counterpart of the desktop parser's
 * `restore_blockquote_whitespace`. The marker is `>` plus one optional
 * separating space; further indentation is quoted content, so `>   x`
 * keeps two leading spaces. Injected as non-breaking spaces because HTML
 * collapses literal leading whitespace.
 */

export type MdastNode = {
  type: string
  value?: string
  children?: MdastNode[]
  position?: { start: { offset?: number }; end: { offset?: number } }
}

const INDENT = '\u00a0'

/** A line prefix that is only container markers: optional list markers and
 * `>` runs — everything that can precede a quoted line's text. */
const CONTAINER_PREFIX = /^[ \t]*(?:(?:[-*+]|\d+[.)])?[ \t]*>[ \t]*)+$/

/** Whitespace past the marker's one separating space on a marker-only
 * line prefix — `>   ` yields two. Zero when the run after the final `>`
 * is the single allowed space or the prefix carries other content. */
function quoteLeadSpaces(linePrefix: string): number {
  if (!CONTAINER_PREFIX.test(linePrefix)) return 0
  const tail = linePrefix.length - linePrefix.lastIndexOf('>') - 1
  return Math.max(0, tail - 1)
}

/** Recover the child node's own lead plus, for text, every continuation
 * line's lead — micromark keeps soft breaks inside the value, so the
 * whitespace is measured on the matching source lines, markers included. */
function restoreChild(child: MdastNode, source: string): MdastNode[] {
  const start = child.position?.start.offset
  const end = child.position?.end.offset
  if (start === undefined || end === undefined) return [child]
  if (child.type === 'text' && child.value?.includes('\n')) {
    const parts = child.value.split('\n')
    const lines = source.slice(start, end).split('\n')
    for (let index = 1; index < parts.length && index < lines.length; index += 1) {
      const content = parts[index]!
      // The stripped value stays a suffix of its source line unless
      // entities were decoded; either way only a clean suffix is trusted.
      if (!content || !lines[index]!.endsWith(content)) continue
      const lead = quoteLeadSpaces(lines[index]!.slice(0, -content.length))
      if (lead > 0) parts[index] = INDENT.repeat(lead) + content
    }
    child = { ...child, value: parts.join('\n') }
  }
  const lineStart = source.lastIndexOf('\n', start - 1) + 1
  const lead = quoteLeadSpaces(source.slice(lineStart, start))
  if (lead === 0) return [child]
  return [{ type: 'text', value: INDENT.repeat(lead) }, child]
}

/** Remark transform over the parsed tree. Only paragraph children inside a
 * blockquote qualify — code blocks already keep their text literally, and
 * heading/list markers are a different kind of separation. */
export function applyBlockquoteIndent(tree: MdastNode, source: string) {
  const visit = (node: MdastNode, inQuote: boolean) => {
    if (!node.children) return
    const quoted = inQuote || node.type === 'blockquote'
    node.children = node.children.flatMap((child) => {
      visit(child, quoted)
      return quoted && node.type === 'paragraph' ? restoreChild(child, source) : [child]
    })
  }
  visit(tree, false)
}

export function markdownBlockquoteIndentPlugin(source: string) {
  return () => (tree: MdastNode) => applyBlockquoteIndent(tree, source)
}
