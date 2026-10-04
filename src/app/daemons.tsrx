import { Haptics } from '../lib/haptics';
import { requestPair } from '@waku/client';
import { device } from '@octane-xplat/platform';
import { useEffect, useState } from 'octane';
import {
  ActivityIndicator,
  Row,
  Pressable,
  pushRoute,
  popRoute,
  ScrollView,
  Text,
  View,
  Column,
} from '@octane-xplat/ui';

import { AppSymbol } from '../components/app-symbol';
import { ConnectionBanner } from '../components/connection-banner';
import { DaemonAvatar } from '../components/daemon-avatar';
import { DaemonList } from '../components/daemon-list';
import { showAlert } from '../lib/alert';
import { NativeTint } from '../lib/theme-platform';
import { useTheme } from '../lib/use-theme';
import { createNativeDaemonSocket } from '../lib/socket';
import { useDaemon } from '../lib/daemon-hooks';
import {
  subscribeDaemonDiscovery,
  type DiscoveredDaemon,
} from '../lib/daemon-discovery';

import type { DaemonProfile } from '../lib/daemon-profile';

export default function DaemonsScreen() {
  const theme = useTheme();
  const daemon = useDaemon();
  const [selectingId, setSelectingId] = useState<string | null>(null);
  const [nearby, setNearby] = useState<DiscoveredDaemon[]>([]);
  const [pairingId, setPairingId] = useState<string | null>(null);

  useEffect(() => subscribeDaemonDiscovery(setNearby), []);

  // Daemons already saved as profiles don't need pairing again.
  const discovered = nearby.filter(
    (item) => !daemon.profiles.some((profile) => profile.address === item.address),
  );

  async function pair(found: DiscoveredDaemon) {
    if (pairingId) return;
    setPairingId(found.id);
    try {
      await Haptics.selectionAsync();
      const outcome = await requestPair(found.address, device.model || 'This device', {
        webSocketFactory: createNativeDaemonSocket,
      });

      if (outcome.status === 'granted') {
        const result = await daemon.saveProfile({
          name: outcome.daemonName || found.name,
          address: found.address,
          token: outcome.token,
        });

        await Haptics.notificationAsync(
          result.connected
            ? Haptics.NotificationFeedbackType.Success
            : Haptics.NotificationFeedbackType.Warning,
        );

        popRoute('root');
      } else {
        await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
        void showAlert('Pairing declined', outcome.message);
      }
    } catch (cause) {
      await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
      void showAlert(
        'Pairing failed',
        cause instanceof Error ? cause.message : String(cause),
      );
    } finally {
      setPairingId(null);
    }
  }

  async function select(profile: DaemonProfile) {
    if (selectingId) return;
    if (profile.id === daemon.activeProfile?.id) {
      popRoute('root');
      return;
    }

    setSelectingId(profile.id);
    try {
      await Haptics.selectionAsync();
      const connected = await daemon.selectProfile(profile.id);
      if (connected) popRoute('root');
      else await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
    } finally {
      setSelectingId(null);
    }
  }

  return (
    <Column style={{ flex: '1 1', backgroundColor: theme.background }}>
      <ScrollView style={{ flex: '1 1' }}>
        <Column style={{ paddingBottom: 36, paddingHorizontal: 16 }}>
          <ConnectionBanner />
          {discovered.length ? (
            <>
              <Text style={{
                fontSize: 14,
                fontWeight: '500',
                marginBottom: 7,
                marginLeft: 12,
                marginTop: 14,
                textTransform: 'uppercase',
                color: theme.textTertiary,
              }}>
                On your network
              </Text>
              <Column style={{ borderRadius: 16, overflow: 'hidden', backgroundColor: theme.overlay }}>
                {discovered.map((found, index) => (
                  <View key={found.id}>
                    {index > 0 && (
                      <View style={{ height: 1, marginLeft: 62, backgroundColor: theme.separator }} />
                    )}
                    <Pressable
                      accessibilityHint="Asks that daemon to pair with this device"
                      accessibilityLabel={`Pair with ${found.name}, ${found.address}`}
                      accessibilityRole="button"
                      disabled={pairingId !== null}
                      onPress={() => void pair(found)}
                      className="vx-pressable"
                      style={{
                        alignItems: 'center',
                        flexDirection: 'row',
                        gap: 12,
                        minHeight: 60,
                        paddingHorizontal: 14,
                        paddingVertical: 10,
                      }}
                    >
                      <DaemonAvatar name={found.name} size={36} />
                      <View style={{ flex: '1 1' }}>
                        <Text numberOfLines={1} style={{ fontSize: 17, fontWeight: '500', color: theme.text }}>
                          {found.name}
                        </Text>
                        <Text
                          numberOfLines={1}
                          style={{ fontSize: 14, marginTop: 1, color: theme.textSecondary }}>
                          {found.address}
                        </Text>
                      </View>
                      {pairingId === found.id ? (
                        <ActivityIndicator busy />
                      ) : (
                        <Text style={{ fontSize: 16, fontWeight: '600', color: String(NativeTint) }}>Pair</Text>
                      )}
                    </Pressable>
                  </View>
                ))}
              </Column>
            </>
          ) : null}
          {daemon.profiles.length ? (
            <>
              <Text style={{
                fontSize: 14,
                fontWeight: '500',
                marginBottom: 7,
                marginLeft: 12,
                marginTop: 14,
                textTransform: 'uppercase',
                color: theme.textTertiary,
              }}>Daemons</Text>
              <DaemonList
                selectingId={selectingId}
                onEdit={(profile) => {
                  pushRoute({ stack: 'root', name: 'daemon-editor', params: { id: profile.id } });
                }}
                onSelect={(profile) => void select(profile)}
              />
              <Row style={{
                alignItems: 'flex-start',
                gap: 8,
                marginHorizontal: 12,
                marginTop: 14,
              }}>
                <AppSymbol
                  name={{ ios: 'key.horizontal', android: 'key', web: 'key' }}
                  size={14}
                  tintColor={theme.textTertiary}
                />
                <Text style={{ flex: '1 1', fontSize: 13, lineHeight: '17px', color: theme.textTertiary }}>
                  Only the selected daemon stays connected. Credentials are protected by the device
                  keychain and never pass through a Goddard service.
                </Text>
              </Row>
            </>
          ) : (
            <Column style={{ alignItems: 'center', paddingHorizontal: 40, paddingTop: 100 }}>
              <Text style={{ fontSize: 19, fontWeight: '700', color: theme.text }}>No saved daemons</Text>
              <Text style={{ fontSize: 15, lineHeight: '20px', marginTop: 8, textAlign: 'center', color: theme.textSecondary }}>
                Add the address and token shown in Goddard Desktop’s Daemon settings.
              </Text>
            </Column>
          )}
        </Column>
      </ScrollView>
    </Column>
  );
}
