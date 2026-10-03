import { TextArea } from '@octane-xplat/ui';

import type { ComposerTextInputProps } from './composer-text-input.types';

export function ComposerTextInput({
  inputRef,
  onPasteError: _onPasteError,
  onPasteFiles,
  selection: _selection,
  onSelectionChange: _onSelectionChange,
  style,
  ...props
}: ComposerTextInputProps) {
  // Paste files on web arrive through the textarea's paste event.
  return (
    <TextArea
      bind={(h: any) => inputRef?.({ focus: () => h.focus() })}
      autoGrow
      web={{
        onPaste: onPasteFiles
          ? (event: ClipboardEvent) => {
              const files = [...(event.clipboardData?.files ?? [])].map((file) => ({
                uri: URL.createObjectURL(file),
                name: file.name,
                mimeType: file.type,
                size: file.size,
              }));

              if (files.length) {onPasteFiles(files);}
            }
          : undefined,
      }}
      style={style}
      {...props}
    />
  );
}
