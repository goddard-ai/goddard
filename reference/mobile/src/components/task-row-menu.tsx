import { MenuView, type MenuAction } from '@expo/ui/community/menu';
import { Pressable } from 'react-native';

import type { TaskRowMenuProps } from '@/components/task-row-menu.types';

export function TaskRowMenu({
  accessibilityLabel,
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
  const pinAction: MenuAction | null = archived || !onTogglePin ? null : {
    id: 'pin',
    title: pinned ? 'Unpin task' : 'Pin task',
    image: pinned ? 'pin.slash' : 'pin',
  };
  const archiveAction: MenuAction | null = !onToggleArchive ? null : {
    id: 'archive',
    title: archived ? 'Unarchive task' : 'Archive task',
    image: archived ? 'arrow.up.bin' : 'archivebox',
  };
  const actions: MenuAction[] = [
    ...(pinAction ? [pinAction] : []),
    ...(archiveAction ? [archiveAction] : []),
    { id: 'rename', title: 'Rename task', image: 'pencil' },
    {
      id: 'delete',
      title: 'Delete task',
      image: 'trash',
      attributes: { destructive: true },
    },
  ];
  return (
    <MenuView
      actions={actions}
      onPressAction={({ nativeEvent }) => {
        if (nativeEvent.event === 'pin') onTogglePin?.();
        else if (nativeEvent.event === 'archive') onToggleArchive?.();
        else if (nativeEvent.event === 'rename') onRename();
        else if (nativeEvent.event === 'delete') onDelete();
      }}
      shouldOpenOnLongPress
      style={style}>
      <Pressable
        accessibilityHint="Long press for actions"
        accessibilityLabel={accessibilityLabel}
        accessibilityRole="button"
        accessibilityState={{ selected }}
        onPress={onSelect}>
        {({ pressed }) => renderTrigger(pressed)}
      </Pressable>
    </MenuView>
  );
}
