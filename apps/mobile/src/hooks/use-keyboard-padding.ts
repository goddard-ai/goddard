import { Platform } from 'react-native';
import { useAnimatedKeyboard, useAnimatedStyle } from 'react-native-reanimated';

const IS_IOS = Platform.OS === 'ios';

export function useKeyboardPadding() {
  const keyboard = useAnimatedKeyboard();
  return useAnimatedStyle(() => ({
    paddingBottom: IS_IOS ? keyboard.height.value : 0,
  }));
}

/** Bottom padding for an input docked above the keyboard: keep the
 * home-indicator inset while it is hidden, but once the keyboard lifts the
 * input that inset becomes dead space — collapse it to a small gap. */
export function useComposerKeyboardPadding(bottomInset: number) {
  const keyboard = useAnimatedKeyboard();
  return useAnimatedStyle(() => ({
    paddingBottom: IS_IOS && keyboard.height.value > 0
      ? 8
      : Math.max(bottomInset, 10),
  }));
}
