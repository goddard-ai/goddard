/** @jsxImportSource @nativescript-community/octane */
import { registerElement } from '@nativescript-community/octane';
import type { Attributes } from '@nativescript-community/octane/intrinsics';
import { Utils, type EventData } from '@nativescript/core';
import { useEffect, useRef } from 'octane';

import type { LocalAttachmentFile } from '@/lib/attachments';
import { PasteAwareTextView } from '@/lib/paste-text-view';
import type { ComposerTextInputProps } from './composer-text-input.types';

declare module '@nativescript-community/octane/intrinsics' {
  interface NativeScriptElements {
    composertextview: Attributes<typeof PasteAwareTextView>;
  }
}

registerElement('composertextview', PasteAwareTextView as never);

/**
 * The composer's multiline input on iOS/Android — the `TextView` widget,
 * swapped for the paste-aware class from `lib/paste-text-view` so clipboard
 * files and images arrive through `onPasteFiles` and caret moves reach
 * `onSelectionChange`. The autoGrow fit and echo-free text commit mirror
 * the shared TextArea's wiring.
 */
export function ComposerTextInput({
  inputRef,
  onPasteError,
  onPasteFiles,
  selection,
  onSelectionChange,
  style,
  ...props
}: ComposerTextInputProps) {
  const viewRef = useRef<PasteAwareTextView | null>(null);
  const styleObject = typeof style === 'object' && style !== null
    ? style as Record<string, unknown>
    : {};

  /** Collapse the auto-grow height back to the measured text, clamped by the
   * composer's min/max — NativeScript never shrinks a TextView on its own. */
  const fit = () => {
    const view = viewRef.current;
    if (!view || !view.nativeView) {return;}
    view.measure(
      Utils.layout.makeMeasureSpec(0, Utils.layout.UNSPECIFIED),
      Utils.layout.makeMeasureSpec(0, Utils.layout.UNSPECIFIED),
    );

    const padding = metrics(styleObject, 'paddingTop') + metrics(styleObject, 'paddingBottom');
    const border = metrics(styleObject, 'borderWidth') * 2;
    const intrinsic =
      Utils.layout.toDeviceIndependentPixels(view.getMeasuredHeight())
      + padding + border;

    const min = metrics(styleObject, 'minHeight', intrinsic);
    const max = metrics(styleObject, 'maxHeight');
    view.style.height = Math.min(Math.max(intrinsic, min), max || Infinity);
  };

  /** Commit assigned text into the native control only when it differs —
   * writing the same text bounces the caret to the end and echoes back
   * through textChange. */
  const writeText = () => {
    const view = viewRef.current;
    const value = props.value ?? '';
    if (view && view.text !== value) {view.text = value;}
  };

  useEffect(() => {
    writeText();
    fit();
  });

  useEffect(() => {
    if (selection) {viewRef.current?.setSelection(selection.start, selection.end);}
  }, [selection]);

  return (
    <composertextview
      ref={(view: PasteAwareTextView | null) => {
        viewRef.current = view;
        inputRef?.(view
          ? {
              focus: () => view.focus(),
              getSelection: () => view.currentSelection?.() ?? null,
            }
          : null);
      }}
      className={props.className}
      editable={props.editable}
      hint={props.placeholder}
      onLoaded={() => {
        writeText();
        fit();
      }}
      onTextChange={(event: EventData) => {
        props.onChangeText?.((event.object as unknown as PasteAwareTextView).text);
        writeText();
        fit();
      }}
      pasteHandler={onPasteFiles
        ? (files: LocalAttachmentFile[]) => {
            try {
              onPasteFiles(files);
            } catch (error) {
              onPasteError?.(error instanceof Error ? error.message : String(error));
            }
          }
        : null}
      selectionHandler={onSelectionChange
        ? (start: number, end: number) => onSelectionChange({
            nativeEvent: { selection: { start, end } },
          })
        : null}
      style={styleObject}
    />
  );
}

function metrics(
  style: Record<string, unknown>,
  key: string,
  fallback = 0,
): number {
  const value = style[key];
  if (typeof value === 'number') {return value;}
  if (typeof value === 'string') {
    const parsed = parseFloat(value);
    return Number.isFinite(parsed) ? parsed : fallback;
  }

  return fallback;
}
