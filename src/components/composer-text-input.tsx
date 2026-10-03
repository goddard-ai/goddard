/** @jsxImportSource @nativescript-community/octane */
import { TextArea } from '@octane-xplat/ui';

import type { ComposerTextInputProps } from './composer-text-input.types';

// TODO(xplat-blocker): image paste inside the composer used
// @mattermost/react-native-paste-input — no NativeScript equivalent; file
// attachments go through the add-menu pickers instead.
export function ComposerTextInput({
  inputRef,
  onPasteError: _onPasteError,
  onPasteFiles: _onPasteFiles,
  selection: _selection,
  onSelectionChange: _onSelectionChange,
  style,
  ...props
}: ComposerTextInputProps) {
  return (
    <TextArea
      bind={(h: any) => inputRef?.({ focus: () => h.focus() })}
      autoGrow
      style={style}
      {...props}
    />
  );
}
