import { Spinner, HStack, Pressable, Text, View, VStack } from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { ConnectionStatus, connectionPhaseLabel } from '@/components/connection-status';
import { DaemonAvatar } from '@/components/daemon-avatar';
import { NativeTint } from '@/lib/theme-platform';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import { useDaemon } from '@/lib/daemon-hooks';
import { displayHost, type DaemonProfile } from '@/lib/daemon-profile';

export function DaemonList({
  onEdit,
  onSelect,
  selectingId = null,
}: {
  onEdit?: (profile: DaemonProfile) => void;
  onSelect: (profile: DaemonProfile) => void;
  selectingId?: string | null;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  return (
    <VStack
      style={{
        backgroundColor: theme.overlay,
        borderRadius: Radius.large,
        overflow: 'hidden',
      }}
    >
      {daemon.profiles.map((profile, index) => {
        const active = profile.id === daemon.activeProfile?.id;
        const selecting = profile.id === selectingId;
        return (
          <View>
            <HStack style={{ alignItems: 'stretch', minHeight: 64 }}>
              <Pressable
                accessibilityHint="Switches to this daemon"
                accessibilityLabel={`${profile.name}, ${displayHost(profile.address)}, ${
                  active ? connectionPhaseLabel(daemon.phase) : 'saved daemon'
                }`}
                accessibilityRole="button"
                accessibilityState={{ selected: active }}
                onPress={() => onSelect(profile)}
                className="vx-pressable"
                style={{
                  alignItems: 'center',
                  flex: '1 1',
                  flexDirection: 'row',
                  gap: 12,
                  paddingLeft: 12,
                  paddingRight: 10,
                  paddingTop: 9,
                  paddingBottom: 9,
                }}
              >
                <DaemonAvatar name={profile.name} size={36} />
                <View style={{ flex: '1 1' }}>
                  <HStack style={{ alignItems: 'center', gap: 8 }}>
                    <Text numberOfLines={1} style={{ fontSize: 17, fontWeight: '500', color: theme.text }}>
                      {profile.name}
                    </Text>
                    {active && <ConnectionStatus compact phase={daemon.phase} />}
                  </HStack>
                  <Text numberOfLines={1} style={{ fontSize: 13.5, marginTop: 3, color: theme.textSecondary }}>
                    {displayHost(profile.address)}
                  </Text>
                </View>
                {selecting ? (
                  <Spinner busy />
                ) : active ? (
                  <AppSymbol
                    name={{ ios: 'checkmark', android: 'check', web: 'check' }}
                    size={16}
                    tintColor={String(NativeTint)}
                  />
                ) : null}
              </Pressable>
              {onEdit && (
                <Pressable
                  accessibilityLabel={`Edit ${profile.name}`}
                  accessibilityRole="button"
                  hitSlop={6}
                  onPress={() => onEdit(profile)}
                  style={{
                    alignItems: 'center',
                    justifyContent: 'center',
                    paddingLeft: 13,
                    paddingRight: 13,
                  }}
                >
                  <AppSymbol
                    name={{ ios: 'info.circle', android: 'info', web: 'info' }}
                    size={20}
                    tintColor={String(NativeTint)}
                  />
                </Pressable>
              )}
            </HStack>
            {index < daemon.profiles.length - 1 && (
              <View style={{ height: 1, marginLeft: 60, backgroundColor: theme.separator }} />
            )}
          </View>
        );
      })}
    </VStack>
  );
}
