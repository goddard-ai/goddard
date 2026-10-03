import { Pressable, pushRoute, Text, Column } from '@octane-xplat/ui';

import { AppSymbol } from '../components/app-symbol';
import { NativeTint } from '../lib/theme-platform';
import { Radius } from '../lib/theme-colors';
import { useTheme } from '../lib/use-theme';

/** QR scanner for `goddard://connect` codes.
 *
 * TODO(xplat-blocker): live camera scanning. There is no shared camera or
 * barcode-detection widget in @octane-xplat/ui — CameraView exists only for
 * macOS. Needs @nativescript/mlkit-barcode-scanning (or equivalent) wrapped
 * as an app component; until then this screen routes to manual entry. */
export default function DaemonScanScreen() {
  const theme = useTheme();
  return (
    <Column
      style={{
        alignItems: 'center',
        flex: 1,
        gap: 14,
        justifyContent: 'center',
        padding: 32,
        backgroundColor: theme.background,
      }}
    >
      <AppSymbol
        name={{ ios: 'qrcode.viewfinder', android: 'qr_code_scanner', web: 'qr_code_scanner' }}
        size={48}
        tintColor={theme.textTertiary}
      />
      <Text style={{ fontSize: 16, textAlign: 'center', color: theme.textSecondary }}>
        Camera scanning isn’t available in this build yet. Enter the daemon
        address and token by hand instead.
      </Text>
      <Pressable
        accessibilityRole="button"
        onPress={() => pushRoute({ stack: 'root', name: 'daemon-editor', params: {} })}
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
    </Column>
  );
}
