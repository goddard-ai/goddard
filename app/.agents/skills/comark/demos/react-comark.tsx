import { Comark } from '@comark/react'
import type { PropsWithChildren } from 'react'

function Alert({
  type = 'info',
  children,
}: PropsWithChildren<{ type?: string }>) {
  return (
    <aside className={`alert alert-${type}`} role="note">
      {children}
    </aside>
  )
}

const markdown = `# Product Update

Comark supports regular **Markdown** and custom components.

::alert{type="warning"}
Ship the migration before Friday.
::
`

export function ProductUpdate() {
  return (
    <Comark components={{ alert: Alert }} options={{ autoUnwrap: true }}>
      {markdown}
    </Comark>
  )
}
