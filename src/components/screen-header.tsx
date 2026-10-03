import { canGoBack, popRoute, pushRoute, useSafeAreaInsets, View } from '@octane-xplat/ui';

import { useTheme } from '@/lib/use-theme';

/** Pop when there is history; otherwise land on the task list. A screen
 * opened cold (deep link, state restore) is the stack's only entry, and a
 * bare back there has nowhere to go. */
export function navigateBack() {
  if (canGoBack()) {popRoute();}
  else {pushRoute({ stack: 'root', name: '/', params: {}, presentation: 'fade' });}
}

/** Height of the self-drawn top bar: safe-area top + the standard iOS
 * navigation bar the reference measured through useHeaderHeight. */
export function useScreenHeaderInset() {
  const insets = useSafeAreaInsets();
  return insets.top + 44;
}

/** Solid surface behind the header strip: background fill plus a hairline. */
export function ScreenHeaderBackdrop() {
  const theme = useTheme();
  const height = useScreenHeaderInset();
  return (
    <View
      style={{
        backgroundColor: theme.background,
        borderBottomColor: theme.borderStrong,
        borderBottomWidth: 0.5,
        height,
      }}
    />
  );
}
