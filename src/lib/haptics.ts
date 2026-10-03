import {
  Haptics as NSHaptics,
  HapticImpactType,
  HapticNotificationType,
} from '@nativescript/haptics';

/** expo-haptics-shaped shim so ported call sites keep their vocabulary. */
export const Haptics = {
  async selectionAsync(): Promise<void> {
    NSHaptics.selection();
  },
  async notificationAsync(type: 'success' | 'warning' | 'error' | HapticNotificationType): Promise<void> {
    NSHaptics.notification(
      typeof type === 'number'
        ? type
        : type === 'success'
          ? HapticNotificationType.SUCCESS
          : type === 'warning'
            ? HapticNotificationType.WARNING
            : HapticNotificationType.ERROR,
    );
  },
  async impactAsync(style: 'light' | 'medium' | 'heavy' | HapticImpactType): Promise<void> {
    NSHaptics.impact(
      typeof style === 'number'
        ? style
        : style === 'light'
          ? HapticImpactType.LIGHT
          : style === 'medium'
            ? HapticImpactType.MEDIUM
            : HapticImpactType.HEAVY,
    );
  },
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

export function selectionHaptic() {
  NSHaptics.selection();
}
export function successHaptic() {
  NSHaptics.notification(HapticNotificationType.SUCCESS);
}
export function errorHaptic() {
  NSHaptics.notification(HapticNotificationType.ERROR);
}
