import { Platform } from 'react-native';
import { useAnimatedKeyboard, useAnimatedStyle } from 'react-native-reanimated';

const IS_IOS = Platform.OS === 'ios';

export function useKeyboardPadding() {
  const keyboard = useAnimatedKeyboard();
  return useAnimatedStyle(() => ({
    paddingBottom: IS_IOS ? keyboard.height.value : 0,
  }));
}
