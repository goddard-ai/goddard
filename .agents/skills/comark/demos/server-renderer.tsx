import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import { createParse } from 'comark'
import type { PropsWithChildren } from 'react'
import { ComarkRenderer } from '@comark/react'
import toc from 'comark/plugins/toc'

const parseDoc = createParse({
  plugins: [toc({ depth: 3 })],
})

function Alert({
  type = 'info',
  children,
}: PropsWithChildren<{ type?: string }>) {
  return <aside className={`alert alert-${type}`}>{children}</aside>
}

export async function DocsPage({ slug }: { slug: string }) {
  const markdown = await readFile(
    join(process.cwd(), 'content', `${slug}.md`),
    'utf-8',
  )

  const tree = await parseDoc(markdown)

  return (
    <ComarkRenderer
      tree={tree}
      components={{ alert: Alert }}
    />
  )
}
