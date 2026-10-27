export const DEFAULT_SIDEBAR_DAY_START_HOUR = 4
export const SIDEBAR_DAY_START_HOURS = [0, 1, 2, 3, 4, 5, 6] as const

const STORAGE_KEY = 'waku.sidebar-day-start-hour'

export function readSidebarDayStartHour(storage: Pick<Storage, 'getItem'> | null): number {
  const stored = storage?.getItem(STORAGE_KEY)
  if (stored == null || stored.trim() === '') return DEFAULT_SIDEBAR_DAY_START_HOUR
  const hour = Number(stored)
  return Number.isInteger(hour) && hour >= 0 && hour <= 6 ? hour : DEFAULT_SIDEBAR_DAY_START_HOUR
}

function browserStorage(): Storage | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage
  } catch {
    return null
  }
}

let dayStartHour = readSidebarDayStartHour(browserStorage())
const listeners = new Set<() => void>()

export function getSidebarDayStartHour(): number {
  return dayStartHour
}

export function subscribeSidebarDayStartHour(listener: () => void) {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}

export function setSidebarDayStartHour(hour: number) {
  if (!SIDEBAR_DAY_START_HOURS.some((choice) => choice === hour)) return
  browserStorage()?.setItem(STORAGE_KEY, String(hour))
  updateSnapshot(hour)
}

function updateSnapshot(hour: number) {
  if (hour === dayStartHour) return
  dayStartHour = hour
  for (const listener of listeners) listener()
}

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if (event.key === STORAGE_KEY || event.key == null) {
      updateSnapshot(readSidebarDayStartHour(browserStorage()))
    }
  })
}
