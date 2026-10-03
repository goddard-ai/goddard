import { describe, expect, test } from 'bun:test'

import {
  parseSessionMessageSearch,
  sessionMessageSearchIsBlank,
} from './session-search'

describe('parseSessionMessageSearch', () => {
  test('a bare query stays one literal needle', () => {
    const parsed = parseSessionMessageSearch('  retry   logic  ')
    expect(parsed.text).toBe('retry logic')
    expect(parsed.projects).toEqual([])
    expect(parsed.statuses).toEqual([])
    expect(parsed.scope).toBeUndefined()
    expect(parsed.limit).toBeUndefined()
  })

  test('filters lift out of the text', () => {
    const parsed = parseSessionMessageSearch(
      'project:goddard status:idle archived:any limit:5 retry',
    )
    expect(parsed.text).toBe('retry')
    expect(parsed.projects).toEqual(['goddard'])
    expect(parsed.statuses).toEqual(['idle'])
    expect(parsed.scope).toBe('any')
    expect(parsed.limit).toBe(5)
  })

  test('busy expands to the busy status set', () => {
    expect(parseSessionMessageSearch('status:busy').statuses).toEqual([
      'connecting',
      'working',
      'waiting',
      'background',
    ])
  })

  test('repeated filters union and scalars take the last value', () => {
    const parsed = parseSessionMessageSearch(
      'status:idle status:failed status:idle project:a project:b archived:true archived:false',
    )
    expect(parsed.statuses).toEqual(['idle', 'failed'])
    expect(parsed.projects).toEqual(['a', 'b'])
    expect(parsed.scope).toBe('active')
  })

  test('quoted values and phrases hold whitespace', () => {
    const parsed = parseSessionMessageSearch('project:"my app" "the fix" tail')
    expect(parsed.projects).toEqual(['my app'])
    expect(parsed.text).toBe('the fix tail')
  })

  test('unknown fields and bad values fall back to literal text', () => {
    const parsed = parseSessionMessageSearch(
      'status:bogus frobnicate:x limit:nope http://a.b',
    )
    expect(parsed.text).toBe('status:bogus frobnicate:x limit:nope http://a.b')
    expect(parsed.statuses).toEqual([])
    expect(parsed.limit).toBeUndefined()
  })

  test('a filters-only query is not blank', () => {
    expect(sessionMessageSearchIsBlank(parseSessionMessageSearch('status:idle'))).toBe(false)
    expect(sessionMessageSearchIsBlank(parseSessionMessageSearch('  '))).toBe(true)
    expect(sessionMessageSearchIsBlank(parseSessionMessageSearch(''))).toBe(true)
  })
})
