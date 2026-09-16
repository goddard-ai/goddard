import { useEffect } from 'react'

export const GODDARD_DOCUMENT_TITLE = 'Goddard Web'

export function formatDocumentTitle(section?: string | null): string {
  const normalized = section?.trim()
  if (!normalized || normalized === GODDARD_DOCUMENT_TITLE) return GODDARD_DOCUMENT_TITLE
  return `${normalized} — ${GODDARD_DOCUMENT_TITLE}`
}

export function useDocumentTitle(section?: string | null) {
  const title = formatDocumentTitle(section)
  useEffect(() => {
    document.title = title
  }, [title])
}
