import Constants from 'expo-constants';
import { View } from 'react-native';
import {
  ScrollViewMarker as NativeScrollViewMarker,
  type ScrollViewMarkerProps,
} from 'react-native-screens/experimental';

/**
 * Expo Go's bundled react-native-screens lacks the RNSScrollViewMarker
 * native component, so mounting it there throws inside createNode. Fall back
 * to a plain View — the marker only configures iOS 26 scroll edge effects,
 * which just revert to the system defaults.
 */
export function ScrollViewMarker({ scrollEdgeEffects, ...rest }: ScrollViewMarkerProps) {
  if (Constants.appOwnership === 'expo') return <View {...rest} />;
  return <NativeScrollViewMarker scrollEdgeEffects={scrollEdgeEffects} {...rest} />;
}
