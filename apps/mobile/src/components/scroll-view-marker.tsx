import { UIManager, View } from 'react-native';
import {
  ScrollViewMarker as NativeScrollViewMarker,
  type ScrollViewMarkerProps,
} from 'react-native-screens/experimental';

/**
 * The marker configures iOS 26 scroll edge effects, but it only exists in
 * react-native-screens builds that ship RNSScrollViewMarker — an outdated
 * bundled screens (old Expo Go) lacks the native class and mounting it
 * throws inside createNode. Check the class itself rather than the
 * environment so a current Expo Go, which does bundle it, still gets its
 * edge effects disabled. Where the class is absent the plain-View fallback
 * leaves the system defaults on.
 */
const hasNativeMarker = (() => {
  try {
    return UIManager.hasViewManagerConfig('RNSScrollViewMarker');
  } catch {
    return false;
  }
})();

export function ScrollViewMarker({ scrollEdgeEffects, ...rest }: ScrollViewMarkerProps) {
  if (!hasNativeMarker) return <View {...rest} />;
  return <NativeScrollViewMarker scrollEdgeEffects={scrollEdgeEffects} {...rest} />;
}
