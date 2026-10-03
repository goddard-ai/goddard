import { useColorScheme } from '@octane-xplat/ui';

import { Colors } from './theme-colors';

/** The reference app's useTheme() — the active scheme's palette object for
 * dynamic style props. */
export function useTheme() {
  const scheme = useColorScheme();
  return Colors[scheme === 'dark' ? 'dark' : 'light'];
}
