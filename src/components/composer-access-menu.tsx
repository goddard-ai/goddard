import { useState } from 'octane';
import { Pressable } from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import type { ComposerAccessMenuProps } from './composer-access-menu.types';
import { AccessSheet } from './session-option-sheets';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import { runtimeModeLabel } from '@/lib/session-presentation';

const MODE_ICONS = {
  ask: { ios: 'lock', android: 'lock', web: 'lock' },
  autoAcceptEdits: { ios: 'pencil', android: 'edit', web: 'edit' },
  auto: { ios: 'sparkles', android: 'auto_awesome', web: 'auto_awesome' },
  fullAccess: { ios: 'lock.open', android: 'lock_open', web: 'lock_open' },
} as const;

/** Access-mode trigger + sheet (the iOS reference's anchored menu collapses
 * to a sheet in the shared build). */
export function ComposerAccessMenu({ mode, onApply }: ComposerAccessMenuProps) {
  const theme = useTheme();
  const [open, setOpen] = useState(false);

  return (
    <>
      <Pressable
        accessibilityLabel={`Agent access, ${runtimeModeLabel(mode)}`}
        accessibilityRole="button"
        onPress={() => setOpen(true)}
        style={{
          alignItems: 'center',
          borderRadius: Radius.pill,
          height: 36,
          justifyContent: 'center',
          width: 36,
        }}
      >
        <AppSymbol name={MODE_ICONS[mode]} size={19} tintColor={theme.textSecondary} />
      </Pressable>
      <AccessSheet
        mode={mode}
        onApply={onApply}
        onDismiss={() => setOpen(false)}
        visible={open}
      />
    </>
  );
}
