import { Pressable, Text, VStack, HStack } from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';

export function DaemonOnboarding({ onAddDaemon }: { onAddDaemon: () => void }) {
  const theme = useTheme();
  return (
    <VStack
      style={{
        alignItems: 'center',
        flex: '1 1',
        justifyContent: 'center',
        paddingBottom: 48,
        paddingLeft: 34,
        paddingRight: 34,
        backgroundColor: theme.background,
      }}
    >
      <Text
        style={{
          fontSize: 29,
          fontWeight: '700',
          letterSpacing: -0.7,
          textAlign: 'center',
          color: theme.text,
        }}
      >
        Talk to your Boss.
      </Text>
      <Text
        style={{
          fontSize: 17,
          lineHeight: '23px',
          marginTop: 10,
          textAlign: 'center',
          color: theme.textSecondary,
        }}
      >
        You talk to the Boss; the Boss runs the rest.
      </Text>
      <Pressable
        accessibilityLabel="Add a daemon"
        accessibilityRole="button"
        onPress={onAddDaemon}
        className="vx-pressable"
        style={{
          alignItems: 'center',
          borderRadius: Radius.pill,
          flexDirection: 'row',
          gap: 8,
          marginTop: 28,
          minHeight: 50,
          paddingLeft: 22,
          paddingRight: 22,
          backgroundColor: theme.inverse,
        }}
      >
        <AppSymbol
          name={{ ios: 'plus', android: 'add', web: 'add' }}
          size={17}
          tintColor={theme.onInverse}
        />
        <Text style={{ fontSize: 17, fontWeight: '700', color: theme.onInverse }}>Connect your Boss</Text>
      </Pressable>
      <HStack
        style={{
          alignItems: 'flex-start',
          gap: 8,
          marginTop: 24,
        }}
      >
        <AppSymbol
          name={{ ios: 'lock.shield', android: 'shield_lock', web: 'lock' }}
          size={15}
          tintColor={theme.textTertiary}
        />
        <Text style={{ flex: '1 1', fontSize: 13, lineHeight: '17px', color: theme.textTertiary }}>
          Tokens stay on this device and go directly to the host you choose.
        </Text>
      </HStack>
    </VStack>
  );
}
