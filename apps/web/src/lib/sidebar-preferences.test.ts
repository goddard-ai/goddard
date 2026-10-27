import { expect, test } from 'bun:test'
import { readSidebarDayStartHour } from './sidebar-preferences'

test('day start defaults to 4 AM for absent or invalid stored preferences', () => {
  expect(readSidebarDayStartHour(null)).toBe(4)
  for (const stored of [null, '', '7', '-1', '1.5', 'invalid']) {
    expect(readSidebarDayStartHour({ getItem: () => stored })).toBe(4)
  }
  for (let hour = 0; hour <= 6; hour++) {
    expect(readSidebarDayStartHour({ getItem: () => String(hour) })).toBe(hour)
  }
})
