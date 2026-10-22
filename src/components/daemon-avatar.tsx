import { Text, View } from '@octane-xplat/ui';

import { Radius } from '@/lib/theme-colors';
import { profileInitials } from '@/lib/daemon-profile';
import { useTheme } from '@/lib/use-theme';

export function DaemonAvatar({ name, size = 42 }: { name: string; size?: number }) {
  const theme = useTheme();
  return (
    <View
      className="vx-avatar"
      style={{
        width: size,
        height: size,
        borderRadius: Math.min(Radius.medium, size / 3.5),
        backgroundColor: theme.accentSoft,
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <Text
        style={{
          color: theme.accent,
          fontSize: Math.round(size * 0.34),
          fontWeight: '700',
          letterSpacing: 0.2,
        }}
      >
        {profileInitials(name)}
      </Text>
    </View>
  );
}
