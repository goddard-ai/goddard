import { useWindowSize } from '@octane-xplat/platform';
import { useSafeAreaInsets } from '@octane-xplat/ui';

import { modelSheetListHeight } from './model-sheet-layout';

/** Sheet list height without live keyboard tracking — KeyboardAvoiding owns
 * avoidance in xplat; the static frame is enough for sheet content. */
export function useModelSheetListHeight(_visible: boolean): number {
  const { height } = useWindowSize();
  const fontScale = 1;
  const insets = useSafeAreaInsets();
  return modelSheetListHeight({
    windowHeight: height,
    keyboardTop: null,
    safeAreaTop: insets.top,
    safeAreaBottom: insets.bottom,
    fontScale,
  });
}
