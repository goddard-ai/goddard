import { Pressable, Text, VStack } from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import type { AppSymbolName } from './app-symbol.types';
import { NativeTint } from '../lib/theme-platform';
import { Radius } from '../lib/theme-colors';
import { useTheme } from '../lib/use-theme';

/** Manual-entry fallback for the scan screen — shown wherever live scanning
 * can't run (web, denied camera permission). */
export function ScanFallback(props: {
  icon: AppSymbolName;
  message: string;
  onManual: () => void;
}) {
  const theme = useTheme();
  return (
    <VStack
      style={{
        alignItems: 'center',
        flex: '1 1',
        gap: 14,
        justifyContent: 'center',
        padding: 32,
        backgroundColor: theme.background,
      }}
    >
      <AppSymbol name={props.icon} size={48} tintColor={theme.textTertiary} />
      <Text style={{ fontSize: 16, textAlign: 'center', color: theme.textSecondary }}>
        {props.message}
      </Text>
      <Pressable
        accessibilityRole="button"
        onPress={props.onManual}
        className="vx-pressable"
        style={{
          alignItems: 'center',
          borderRadius: Radius.medium,
          justifyContent: 'center',
          minHeight: 44,
          paddingHorizontal: 18,
          backgroundColor: String(NativeTint),
        }}
      >
        <Text style={{ color: '#ffffff', fontSize: 16, fontWeight: '600' }}>Enter manually</Text>
      </Pressable>
    </VStack>
  );
}
