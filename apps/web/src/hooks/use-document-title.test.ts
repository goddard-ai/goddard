import { describe, expect, test } from 'bun:test'
import {
  formatDocumentTitle,
  GODDARD_DOCUMENT_TITLE,
} from './use-document-title'

describe('formatDocumentTitle', () => {
  test('uses the product title without a section', () => {
    expect(formatDocumentTitle()).toBe(GODDARD_DOCUMENT_TITLE)
    expect(formatDocumentTitle('   ')).toBe(GODDARD_DOCUMENT_TITLE)
  })

  test('identifies the current browser surface', () => {
    expect(formatDocumentTitle('New Task')).toBe('New Task — Goddard Web')
    expect(formatDocumentTitle('  General  ')).toBe('General — Goddard Web')
  })

  test('does not duplicate the product title', () => {
    expect(formatDocumentTitle(GODDARD_DOCUMENT_TITLE)).toBe(GODDARD_DOCUMENT_TITLE)
  })
})
