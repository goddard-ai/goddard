import { Haptics } from '../lib/haptics';
import { useEffect, useRef, useState } from 'octane';
import { Spinner, HStack, Pressable, Text, VStack } from '@octane-xplat/ui';
import { popRoute, pushRoute } from '@octane-xplat/ui/routing';

import { AppSymbol } from '../components/app-symbol';
import { NativeTint } from '../lib/theme-platform';
import { Radius } from '../lib/theme-colors';
import { useTheme } from '../lib/use-theme';
import { useDaemon } from '../lib/daemon-hooks';
import { daemonConnectLink } from '../lib/daemon-profile';

/** Handles `goddard://connect?address=…&token=…` — the link inside the
 * desktop daemon settings QR — whether it arrives from the system camera,
 * another app's scanner, or the in-app scanner route. Saves the profile,
 * activates it, and lands in the boss chat. */
export default function ConnectScreen(props: {
  address?: string;
  token?: string;
  name?: string;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const [error, setError] = useState<string | null>(null);
  const attempted = useRef(false);

  useEffect(() => {
    if (attempted.current) return;
    attempted.current = true;
    void (async () => {
      try {
        const link = daemonConnectLink({
          address: props.address,
          token: props.token,
          name: props.name,
        });

        const result = await daemon.saveProfile(link);
        await Haptics.notificationAsync(
          result.connected
            ? Haptics.NotificationFeedbackType.Success
            : Haptics.NotificationFeedbackType.Warning,
        );

        popRoute('root');
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
      }
    })();
  }, []);

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
      {error ? (
        <>
          <HStack style={{ alignItems: 'flex-start', gap: 8 }}>
            <AppSymbol
              name={{
                ios: 'exclamationmark.circle.fill',
                android: 'error',
                web: 'error',
              }}
              size={16}
              tintColor={theme.danger}
            />
            <Text style={{ flex: '1 1', fontSize: 15, lineHeight: '20px', color: theme.danger }}>
              {error}
            </Text>
          </HStack>
          <HStack style={{ gap: 10, marginTop: 8 }}>
            <Pressable
              accessibilityRole="button"
              onPress={() => pushRoute({ stack: 'root', name: 'daemon-editor', params: {} })}
              className="vx-pressable"
              style={{
                alignItems: 'center',
                borderRadius: Radius.medium,
                justifyContent: 'center',
                minHeight: 44,
                paddingLeft: 18,
                paddingRight: 18,
                backgroundColor: String(NativeTint),
              }}
            >
              <Text style={{ color: '#ffffff', fontSize: 16, fontWeight: '600' }}>Enter manually</Text>
            </Pressable>
            <Pressable
              accessibilityRole="button"
              onPress={() => popRoute('root')}
              className="vx-pressable"
              style={{
                alignItems: 'center',
                borderRadius: Radius.medium,
                justifyContent: 'center',
                minHeight: 44,
                paddingLeft: 18,
                paddingRight: 18,
                backgroundColor: theme.overlay,
              }}
            >
              <Text style={{ color: theme.text, fontSize: 16, fontWeight: '600' }}>
                Cancel
              </Text>
            </Pressable>
          </HStack>
        </>
      ) : (
        <>
          <Spinner busy />
          <Text style={{ fontSize: 16, color: theme.textSecondary }}>
            Connecting to daemon…
          </Text>
        </>
      )}
    </VStack>
  );
}
