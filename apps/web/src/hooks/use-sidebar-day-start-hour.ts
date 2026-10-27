import { useSyncExternalStore } from 'react'
import {
  DEFAULT_SIDEBAR_DAY_START_HOUR,
  getSidebarDayStartHour,
  setSidebarDayStartHour,
  subscribeSidebarDayStartHour,
} from '@/lib/sidebar-preferences'

export function useSidebarDayStartHour() {
  const hour = useSyncExternalStore(
    subscribeSidebarDayStartHour,
    getSidebarDayStartHour,
    () => DEFAULT_SIDEBAR_DAY_START_HOUR,
  )
  return [hour, setSidebarDayStartHour] as const
}
