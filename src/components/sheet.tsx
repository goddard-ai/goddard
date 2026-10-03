import { Pressable, ScrollView, Sheet as XSheet, Text, useSafeAreaInsets, View } from '@octane-xplat/ui';
import { useWindowSize } from '@octane-xplat/platform';
import type { ReactNode } from 'octane';

import { AppSymbol } from '@/components/app-symbol';
import { NativeTint } from '@/lib/theme-platform';
import { Radius, Spacing } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';

/**
 * Bottom sheet used for pickers and action menus — the framework's in-window
 * Sheet (bottom-anchored panel with shade dismiss). The declarative `visible`
 * prop keeps call sites identical to the reference's @expo/ui sheet.
 */
export function Sheet({
  visible,
  onDismiss,
  title,
  children,
  scrollable = true,
}: {
  visible: boolean;
  onDismiss: () => void;
  title?: string;
  children?: ReactNode;
  scrollable?: boolean;
}) {
  const theme = useTheme();
  const insets = useSafeAreaInsets();
  const { height } = useWindowSize();

  const body = (
    <>
      {title ? (
        <Text
          style={{
            fontSize: 13,
            fontWeight: '600',
            letterSpacing: 0.4,
            marginBottom: 6,
            marginHorizontal: 12,
            marginTop: 6,
            textTransform: 'uppercase',
            color: theme.textSecondary,
          }}
        >
          {title}
        </Text>
      ) : null}
      {children}
    </>
  );

  return (
    <XSheet open={visible} onDismiss={onDismiss} detents={[0.62]}>
      <View
        style={{
          paddingHorizontal: Spacing.two,
          paddingTop: 4,
          paddingBottom: Math.max(insets.bottom, 14),
          backgroundColor: theme.surface,
        }}
      >
        {scrollable ? (
          <ScrollView style={{ maxHeight: Math.round(height * 0.62) }}>{body}</ScrollView>
        ) : (
          body
        )}
      </View>
    </XSheet>
  );
}

export function SheetRow({
  label,
  description,
  selected = false,
  destructive = false,
  disabled = false,
  leading,
  onPress,
}: {
  label: string;
  description?: string | null;
  selected?: boolean;
  destructive?: boolean;
  disabled?: boolean;
  leading?: ReactNode;
  onPress: () => void;
}) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected, disabled }}
      disabled={disabled}
      onPress={onPress}
      className="vx-pressable"
      style={{
        alignItems: 'center',
        borderRadius: Radius.medium,
        flexDirection: 'row',
        gap: 11,
        minHeight: 50,
        paddingHorizontal: 12,
        paddingVertical: 9,
        backgroundColor: selected ? theme.overlay : 'transparent',
        opacity: disabled ? 0.4 : 1,
      }}
    >
      {leading}
      <View style={{ flex: 1 }}>
        <Text
          numberOfLines={1}
          style={{
            fontSize: 16.5,
            fontWeight: '500',
            color: destructive ? theme.danger : theme.text,
          }}
        >
          {label}
        </Text>
        {description ? (
          <Text
            numberOfLines={2}
            style={{ fontSize: 13.5, lineHeight: '17px', marginTop: 2, color: theme.textTertiary }}
          >
            {description}
          </Text>
        ) : null}
      </View>
      {selected && (
        <AppSymbol
          name={{ ios: 'checkmark', android: 'check', web: 'check' }}
          size={15}
          tintColor={String(NativeTint)}
        />
      )}
    </Pressable>
  );
}
