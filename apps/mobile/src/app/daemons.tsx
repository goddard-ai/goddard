import * as Haptics from 'expo-haptics';
import Constants from 'expo-constants';
import { router, Stack } from 'expo-router';
import { navigateBack } from '@/components/screen-header';
import { useEffect, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { requestPair } from '@waku/client';

import { AppSymbol } from '@/components/app-symbol';
import { ConnectionBanner } from '@/components/connection-banner';
import { DaemonAvatar } from '@/components/daemon-avatar';
import { DaemonList } from '@/components/daemon-list';
import { NativeTint } from '@/constants/theme';
import { useTheme } from '@/hooks/use-theme';
import { createNativeDaemonSocket, useDaemon } from '@/lib/daemon-context';
import {
  subscribeDaemonDiscovery,
  type DiscoveredDaemon,
} from '@/lib/daemon-discovery';
import type { DaemonProfile } from '@/lib/daemon-profile';

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
      const outcome = await requestPair(found.address, Constants.deviceName ?? 'This device', {
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
        navigateBack();
      } else {
        await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
        Alert.alert('Pairing declined', outcome.message);
      }
    } catch (cause) {
      await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
      Alert.alert(
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
      navigateBack();
      return;
    }
    setSelectingId(profile.id);
    try {
      await Haptics.selectionAsync();
      const connected = await daemon.selectProfile(profile.id);
      if (connected) navigateBack();
      else await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
    } finally {
      setSelectingId(null);
    }
  }

  return (
    <View style={[styles.screen, { backgroundColor: theme.background }]}>
      <Stack.Screen
        options={{
          headerRight: () => (
            <View style={styles.headerActions}>
              <Pressable
                accessibilityLabel="Scan QR code"
                accessibilityRole="button"
                hitSlop={10}
                onPress={() => router.push('/daemon-scan')}>
                <AppSymbol
                  name={{
                    ios: 'qrcode.viewfinder',
                    android: 'qr_code_scanner',
                    web: 'qr_code_scanner',
                  }}
                  size={20}
                  tintColor={NativeTint}
                />
              </Pressable>
              <Pressable
                accessibilityLabel="Add daemon"
                accessibilityRole="button"
                hitSlop={10}
                onPress={() => router.push('/daemon-editor')}>
                <AppSymbol
                  name={{ ios: 'plus', android: 'add', web: 'add' }}
                  size={21}
                  tintColor={NativeTint}
                />
              </Pressable>
            </View>
          ),
          unstable_headerRightItems: () => [{
            type: 'button',
            accessibilityLabel: 'Scan QR code',
            icon: { type: 'sfSymbol', name: 'qrcode.viewfinder' },
            label: 'Scan QR code',
            onPress: () => router.push('/daemon-scan'),
          }, {
            type: 'button',
            accessibilityLabel: 'Add daemon',
            icon: { type: 'sfSymbol', name: 'plus' },
            label: 'Add daemon',
            onPress: () => router.push('/daemon-editor'),
          }],
        }}
      />
      <ScrollView
        contentInsetAdjustmentBehavior="automatic"
        contentContainerStyle={styles.listContent}
        showsVerticalScrollIndicator={false}>
        <ConnectionBanner />
        {discovered.length ? (
          <>
            <Text style={[styles.sectionTitle, { color: theme.textTertiary }]}>
              On your network
            </Text>
            <View style={[styles.group, { backgroundColor: theme.overlay }]}>
              {discovered.map((found, index) => (
                <View key={found.id}>
                  {index > 0 && (
                    <View style={[styles.rowSeparator, { backgroundColor: theme.separator }]} />
                  )}
                  <Pressable
                    accessibilityHint="Asks that daemon to pair with this device"
                    accessibilityLabel={`Pair with ${found.name}, ${found.address}`}
                    accessibilityRole="button"
                    disabled={pairingId !== null}
                    onPress={() => void pair(found)}
                    style={({ pressed }) => [
                      styles.nearbyRow,
                      { backgroundColor: pressed ? theme.overlayStrong : 'transparent' },
                    ]}>
                    <DaemonAvatar name={found.name} size={36} />
                    <View style={styles.copy}>
                      <Text numberOfLines={1} style={[styles.name, { color: theme.text }]}>
                        {found.name}
                      </Text>
                      <Text
                        numberOfLines={1}
                        style={[styles.host, { color: theme.textSecondary }]}>
                        {found.address}
                      </Text>
                    </View>
                    {pairingId === found.id ? (
                      <ActivityIndicator color={NativeTint} size="small" />
                    ) : (
                      <Text style={[styles.pairLabel, { color: NativeTint }]}>Pair</Text>
                    )}
                  </Pressable>
                </View>
              ))}
            </View>
          </>
        ) : null}
        {daemon.profiles.length ? (
          <>
            <Text style={[styles.sectionTitle, { color: theme.textTertiary }]}>Daemons</Text>
            <DaemonList
              selectingId={selectingId}
              onEdit={(profile) => {
                router.push({ pathname: '/daemon-editor', params: { id: profile.id } });
              }}
              onSelect={(profile) => void select(profile)}
            />
            <View style={styles.footer}>
              <AppSymbol
                name={{ ios: 'key.horizontal', android: 'key', web: 'key' }}
                size={14}
                tintColor={theme.textTertiary}
              />
              <Text style={[styles.footerText, { color: theme.textTertiary }]}>
                Only the selected daemon stays connected. Credentials are protected by the device
                keychain and never pass through a Goddard service.
              </Text>
            </View>
          </>
        ) : (
          <View style={styles.empty}>
            <Text style={[styles.emptyTitle, { color: theme.text }]}>No saved daemons</Text>
            <Text style={[styles.emptyBody, { color: theme.textSecondary }]}>
              Add the address and token shown in Goddard Desktop’s Daemon settings.
            </Text>
          </View>
        )}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  headerActions: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 14,
  },
  listContent: { paddingBottom: 36, paddingHorizontal: 16 },
  sectionTitle: {
    fontSize: 14,
    fontWeight: '500',
    marginBottom: 7,
    marginLeft: 12,
    marginTop: 14,
    textTransform: 'uppercase',
  },
  group: { borderRadius: 16, overflow: 'hidden' },
  nearbyRow: {
    alignItems: 'center',
    flexDirection: 'row',
    gap: 12,
    minHeight: 60,
    paddingHorizontal: 14,
    paddingVertical: 10,
  },
  copy: { flex: 1 },
  name: { fontSize: 17, fontWeight: '500' },
  host: { fontSize: 14, marginTop: 1 },
  pairLabel: { fontSize: 16, fontWeight: '600' },
  rowSeparator: { height: StyleSheet.hairlineWidth, marginLeft: 62 },
  footer: {
    alignItems: 'flex-start',
    flexDirection: 'row',
    gap: 8,
    marginHorizontal: 12,
    marginTop: 14,
  },
  footerText: { flex: 1, fontSize: 13, lineHeight: 17 },
  empty: { alignItems: 'center', paddingHorizontal: 40, paddingTop: 100 },
  emptyTitle: { fontSize: 19, fontWeight: '700' },
  emptyBody: { fontSize: 15, lineHeight: 20, marginTop: 8, maxWidth: 320, textAlign: 'center' },
});
