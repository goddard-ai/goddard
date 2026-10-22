import type { ReactNode } from 'octane';
import { ContextMenu, Pressable, type StyleProp } from '@octane-xplat/ui';

export interface TaskRowMenuProps {
  accessibilityLabel?: string;
  archived?: boolean;
  pinned?: boolean;
  selected?: boolean;
  style?: StyleProp;
  onSelect: () => void;
  onRename: () => void;
  onDelete: () => void;
  onToggleArchive?: () => void;
  onTogglePin?: () => void;
  renderTrigger: (pressed: boolean) => ReactNode;
}

/** iOS long-press context menu → shared ContextMenu (right-click on web). */
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
  return (
    <ContextMenu
      items={[
        ...(archived || !onTogglePin ? [] : [{
          type: 'action' as const,
          key: 'pin',
          label: pinned ? 'Unpin task' : 'Pin task',
          icon: pinned ? 'pin.slash' : 'pin',
          onSelect: onTogglePin,
        }]),
        ...(!onToggleArchive ? [] : [{
          type: 'action' as const,
          key: 'archive',
          label: archived ? 'Unarchive task' : 'Archive task',
          icon: archived ? 'arrow.up.bin' : 'archivebox',
          onSelect: onToggleArchive,
        }]),
        { type: 'action' as const, key: 'rename', label: 'Rename task', icon: 'pencil', onSelect: onRename },
        { type: 'action' as const, key: 'delete', label: 'Delete task', icon: 'trash', onSelect: onDelete },
      ]}>
      <Pressable
        accessibilityHint="Long press for actions"
        accessibilityLabel={accessibilityLabel}
        accessibilityRole="button"
        accessibilityState={{ selected }}
        onPress={onSelect}
        style={style}>
        {renderTrigger(false)}
      </Pressable>
    </ContextMenu>
  );
}
