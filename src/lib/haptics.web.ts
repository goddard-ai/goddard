/** Web no-op — browsers have no haptic API outside the deprecated Vibration
 * API, which iOS Safari ignores anyway. */
export const Haptics = {
  async selectionAsync(): Promise<void> {},
  async notificationAsync(_type: unknown): Promise<void> {},
  async impactAsync(_style: unknown): Promise<void> {},
  NotificationFeedbackType: {
    Success: 'success',
    Warning: 'warning',
    Error: 'error',
  },
  ImpactFeedbackStyle: {
    Light: 'light',
    Medium: 'medium',
    Heavy: 'heavy',
  },
} as const;

export function selectionHaptic() {}
export function successHaptic() {}
export function errorHaptic() {}
