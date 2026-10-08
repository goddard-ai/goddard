import { describe, expect, test } from 'bun:test'

import { atomSegments, atomVisibleText } from './transcript-presentation'
import type { MessageAtom } from './generated'

const OPEN = '￹'
const END = '￺'

/** The display_content encoding: 32 variation selectors = the 16 uuid bytes. */
function encodeSessionId(id: string): string {
  const hex = id.replace(/-/g, '')
  let out = ''
  for (let index = 0; index < hex.length; index += 1) {
    out += String.fromCodePoint(0xfe00 + parseInt(hex[index]!, 16))
  }
  return out
}

describe('atomSegments', () => {
  test('plain text is one text segment', () => {
    expect(atomSegments('hello world')).toEqual([{ kind: 'text', text: 'hello world' }])
    expect(atomSegments('')).toEqual([])
  })

  test('a session atom carries the id decoded from its encoding', () => {
    const id = '123e4567-e89b-42d3-a456-426614174000'
    const text = `look at ${OPEN}${encodeSessionId(id)}spec task${END} please`
    expect(atomSegments(text)).toEqual([
      { kind: 'text', text: 'look at ' },
      { kind: 'atom', label: 'spec task', sessionId: id },
      { kind: 'text', text: ' please' },
    ])
  })

  test('a folded paste has no session; the parallel record fills in when it has one', () => {
    const atoms: MessageAtom[] = [
      { label: 'spec task', payload: 'session:…', session_id: null },
      { label: 'other task', payload: 'session:…', session_id: 'deadbeef-dead-beef-dead-beefdeadbeef' },
    ]
    const text = `x ${OPEN}spec task${END} and ${OPEN}other task${END}`
    expect(atomSegments(text, atoms)).toEqual([
      { kind: 'text', text: 'x ' },
      { kind: 'atom', label: 'spec task', sessionId: null },
      { kind: 'text', text: ' and ' },
      { kind: 'atom', label: 'other task', sessionId: 'deadbeef-dead-beef-dead-beefdeadbeef' },
    ])
  })

  test('label escapes unwrap inside a span', () => {
    const text = `${OPEN}a \\*bold\\* chip${END}`
    expect(atomSegments(text)).toEqual([
      { kind: 'atom', label: 'a *bold* chip', sessionId: null },
    ])
  })

  test('an unterminated span keeps its label as text', () => {
    expect(atomSegments(`before ${OPEN}dangling`)).toEqual([
      { kind: 'text', text: 'before dangling' },
    ])
  })

  test('segments agree with atomVisibleText', () => {
    const id = '123e4567-e89b-42d3-a456-426614174000'
    const text = `a ${OPEN}${encodeSessionId(id)}chip${END} b ${OPEN}paste${END} c`
    const flat = atomSegments(text)
      .map((segment) => (segment.kind === 'text' ? segment.text : segment.label))
      .join('')
    expect(flat).toBe(atomVisibleText(text))
  })
})
