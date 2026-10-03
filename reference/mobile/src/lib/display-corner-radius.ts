import * as Device from 'expo-device';
import { Platform } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

/**
 * The physical corner radius of the display, in points. iOS has no public API
 * for this before iOS 26's `UICornerConfiguration`, so the values are keyed by
 * internal model id — measured values published via kylebshr/ScreenCorners and
 * community reports for newer releases.
 */
const IPHONE_CORNER_RADIUS: Record<string, number> = {
  // iPhone X, Xs, Xs Max, 11 Pro, 11 Pro Max
  'iPhone10,3': 39, 'iPhone10,6': 39,
  'iPhone11,2': 39, 'iPhone11,4': 39, 'iPhone11,6': 39,
  'iPhone12,3': 39, 'iPhone12,5': 39,
  // iPhone Xr, 11
  'iPhone11,8': 41.5, 'iPhone12,1': 41.5,
  // iPhone 12 mini, 13 mini
  'iPhone13,1': 44, 'iPhone14,4': 44,
  // iPhone 12, 12 Pro, 13, 13 Pro, 14, 16e
  'iPhone13,2': 47.33, 'iPhone13,3': 47.33,
  'iPhone14,2': 47.33, 'iPhone14,5': 47.33, 'iPhone14,7': 47.33,
  'iPhone17,5': 47.33,
  // iPhone 12 Pro Max, 13 Pro Max, 14 Plus
  'iPhone13,4': 53.33, 'iPhone14,3': 53.33, 'iPhone14,8': 53.33,
  // iPhone 14 Pro/Pro Max, 15 series, 16, 16 Plus
  'iPhone15,2': 55, 'iPhone15,3': 55, 'iPhone15,4': 55, 'iPhone15,5': 55,
  'iPhone16,1': 55, 'iPhone16,2': 55, 'iPhone17,3': 55, 'iPhone17,4': 55,
  // iPhone 16 Pro/Pro Max, 17 series, Air
  'iPhone17,1': 62, 'iPhone17,2': 62,
  'iPhone18,1': 62, 'iPhone18,2': 62, 'iPhone18,3': 62, 'iPhone18,4': 62,
};

function iosCornerRadius(topInset: number): number {
  const modelId = Device.modelId ?? '';
  const known = IPHONE_CORNER_RADIUS[modelId];
  if (known != null) return known;

  const model = /^(iPhone|iPad)(\d+),/.exec(modelId);
  if (model?.[1] === 'iPad') {
    // iPads with a rounded display start at iPad13,x (Air 4, iPad 10).
    return Number(model[2]) >= 13 ? 18 : 0;
  }
  if (model?.[1] === 'iPhone' && Number(model[2]) >= 18) return 62;

  // The simulator reports the host Mac's model id, and future models won't be
  // in the table — infer from the display cutout instead. ~59pt top inset
  // means a Dynamic Island-era device, ~44-47pt a notched one.
  if (topInset >= 59) return 55;
  if (topInset >= 44) return 47;
  return 0;
}

/** Corner radius of the physical display, for card surfaces that should match
 * the device when they detach from the screen edge during a gesture. */
export function useDisplayCornerRadius(): number {
  const insets = useSafeAreaInsets();
  switch (Platform.OS) {
    case 'ios':
      return iosCornerRadius(insets.top);
    case 'android':
      // Android exposes no display radius; 28dp matches Material's card and
      // predictive-back treatments.
      return 28;
    default:
      return 12;
  }
}
