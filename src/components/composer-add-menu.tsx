/** @jsxImportSource @nativescript-community/octane */
import { DropdownMenu, View } from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';

export type ComposerAttachmentSource = 'files' | 'camera' | 'photo';

/** The + menu: commands, file mentions, and attachment pickers. The
 * reference used a native anchored menu; DropdownMenu is the shared
 * equivalent. */
export function ComposerAddMenu({
  disabled = false,
  onChoose,
  onChooseContext,
}: {
  disabled?: boolean;
  onChoose?: (source: ComposerAttachmentSource) => void;
  onChooseContext: (kind: 'command' | 'file') => void;
}) {
  const theme = useTheme();
  const trigger = (
    <View
      accessible
      accessibilityLabel="Add to message"
      accessibilityRole="button"
      accessibilityState={{ disabled }}
      style={{
        alignItems: 'center',
        borderRadius: Radius.pill,
        height: 36,
        justifyContent: 'center',
        width: 36,
        opacity: disabled ? 0.35 : 1,
      }}
    >
      <AppSymbol
        name={{ ios: 'plus', android: 'add', web: 'add' }}
        size={20}
        tintColor={theme.textSecondary}
      />
    </View>
  );

  if (disabled) {return trigger;}

  // Let the menu finish dismissing before presenting a picker.
  const choose = (kind: 'command' | 'file') => setTimeout(() => onChooseContext(kind), 160);
  const pick = (source: ComposerAttachmentSource) => setTimeout(() => onChoose?.(source), 160);

  return (
    <DropdownMenu
      trigger={trigger}
      items={[
        { key: 'command', label: 'Commands', onSelect: () => choose('command') },
        { key: 'mention', label: 'Mention files', onSelect: () => choose('file') },
        ...(onChoose
          ? [
              { key: 'files', label: 'Files', onSelect: () => pick('files') },
              { key: 'camera', label: 'Camera', onSelect: () => pick('camera') },
              { key: 'photo', label: 'Photo', onSelect: () => pick('photo') },
            ]
          : []),
      ]}
    />
  );
}
