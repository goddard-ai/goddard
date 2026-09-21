import type { ReactElement } from 'react';
import type { StyleProp, ViewStyle } from 'react-native';

export interface TaskRowMenuProps {
  accessibilityLabel: string;
  archived?: boolean;
  pinned?: boolean;
  onDelete: () => void;
  onRename: () => void;
  onSelect: () => void;
  onToggleArchive?: () => void;
  onTogglePin?: () => void;
  renderTrigger: (pressed: boolean) => ReactElement;
  selected: boolean;
  style: StyleProp<ViewStyle>;
}
