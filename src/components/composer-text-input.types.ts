import type { LocalAttachmentFile } from '@/lib/attachments';

export interface ComposerSelection {
  start: number;
  end: number;
}

export interface ComposerTextInputProps {
  value?: string;
  placeholder?: string;
  onChangeText?: (text: string) => void;
  onSelectionChange?: (event: { nativeEvent: { selection: ComposerSelection } }) => void;
  selection?: ComposerSelection;
  editable?: boolean;
  autoFocus?: boolean;
  multiline?: boolean;
  inputRef?: (input: ComposerTextInputHandle | null) => void;
  onPasteFiles?: (files: LocalAttachmentFile[]) => void;
  onPasteError?: (message: string) => void;
  style?: any;
  className?: any;
}

export interface ComposerTextInputHandle {
  focus: () => void;
  /** Live caret — selection events don't fire on every platform, so the
   * picker reads this at the moment it opens. */
  getSelection?: () => ComposerSelection | null;
}
