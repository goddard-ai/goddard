import { Button, Host, Menu, RNHostView } from '@expo/ui/swift-ui';
import {
  accessibilityAddTraits,
  accessibilityHint,
  accessibilityLabel,
} from '@expo/ui/swift-ui/modifiers';
import { Pressable } from 'react-native';

import type { TaskRowMenuProps } from '@/components/task-row-menu.types';

/**
 * SwiftUI Menu's primary action fires a normal tap directly while reserving
 * long-press for the native menu. ContextMenu delays its hosted React Native
 * child's tap while it arbitrates the long-press gesture.
 *
 * The label's Pressable handles the tap at the RN layer too: Expo Go bundles a
 * fixed ExpoUI native module whose MenuView may predate primary-action
 * support, in which case the event never reaches JS and the tap passes through
 * to the hosted child instead. Where primaryAction does fire natively, the
 * SwiftUI button consumes the tap and the Pressable is cancelled — and if both
 * ever fired, a duplicate same-target navigation is harmless.
 */
export function TaskRowMenu({
  accessibilityLabel: label,
  archived = false,
  pinned = false,
  onDelete,
  onRename,
  onSelect,
  onToggleArchive,
  onTogglePin,
  renderTrigger,
  selected,
  style,
}: TaskRowMenuProps) {
  return (
    <Host ignoreSafeArea="all" matchContents style={style}>
      <Menu
        label={(
          <RNHostView matchContents>
            <Pressable accessible={false} onPress={onSelect}>
              {({ pressed }) => renderTrigger(pressed)}
            </Pressable>
          </RNHostView>
        )}
        modifiers={[
          accessibilityLabel(label),
          accessibilityHint('Long press for actions'),
          accessibilityAddTraits(selected ? ['isButton', 'isSelected'] : ['isButton']),
        ]}
        onPrimaryAction={onSelect}>
        {!archived && onTogglePin && (
          <Button
            label={pinned ? 'Unpin task' : 'Pin task'}
            systemImage={pinned ? 'pin.slash' : 'pin'}
            onPress={onTogglePin}
          />
        )}
        {onToggleArchive && (
          <Button
            label={archived ? 'Unarchive task' : 'Archive task'}
            systemImage={archived ? 'arrow.up.bin' : 'archivebox'}
            onPress={onToggleArchive}
          />
        )}
        <Button label="Rename task" systemImage="pencil" onPress={onRename} />
        <Button
          label="Delete task"
          role="destructive"
          systemImage="trash"
          onPress={onDelete}
        />
      </Menu>
    </Host>
  );
}
