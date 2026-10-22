/** @jsxImportSource @nativescript-community/octane */
import { View } from '@octane-xplat/ui';
import type { ReactNode } from 'octane';

import { useTheme } from '@/lib/use-theme';

// TODO(port): real LiquidGlass via @octane-xplat/ui/ios LiquidGlass — it
// exists but takes platform props I haven't matched yet; the overlay
// fallback below is what non-glass iOS already showed.
export const liquidGlass = false;

/** Floating chrome surface: Liquid Glass where the OS supports it, a subtle
 * overlay fill elsewhere. */
export function GlassSurface({
  style,
  children,
  interactive: _interactive = false,
  fallbackColor,
}: {
  style?: any;
  children?: ReactNode;
  interactive?: boolean;
  fallbackColor?: string;
}) {
  const theme = useTheme();
  return (
    <View style={{ backgroundColor: fallbackColor ?? theme.overlayStrong, ...style }}>
      {children}
    </View>
  );
}
