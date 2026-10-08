/// <reference types="@nativescript/types" />
import { TextView, Utils } from '@nativescript/core';

import type { LocalAttachmentFile } from './attachments';
import { describeDocument } from './pick-attachment';

/**
 * A TextView whose EditText can paste clipboard URIs — images, files —
 * as attachments, and re-emits caret moves so the composer's context
 * picker inserts at the live selection even when the user only taps to
 * reposition it. Text-only clipboards keep the stock paste behavior.
 */
export class PasteAwareTextView extends TextView {
	public pasteHandler: ((files: LocalAttachmentFile[]) => void) | null = null;
	public selectionHandler: ((start: number, end: number) => void) | null = null;

	// The shared core typings resolve the iOS flavor for every native file
	// (base signature wants UITextView) while the android bundler's check wants
	// EditText — `any` satisfies both.
	public createNativeView(): any {
		return new PasteAwareEditText(this._context);
	}

	public initNativeView(): void {
		super.initNativeView();
		(this.nativeTextViewProtected as unknown as PasteAwareEditText).owner =
			new WeakRef(this);
	}

	// The shared typings declare this abstract; the platform EditableTextBase
	// implements it, so delegate through the parent's prototype.
	public _setInputType(inputType: number): void {
		(
			TextView.prototype as unknown as { _setInputType(inputType: number): void }
		)._setInputType.call(this, inputType);
	}

	public emitSelectionChange(start: number, end: number): void {
		this.selectionHandler?.(start, end);
	}

	/** Live caret for the context picker's insertion point — events alone
	 * can't see a selection the menu made while a sheet was open. */
	public currentSelection(): { start: number; end: number } | null {
		const edit = this.nativeTextViewProtected as unknown as
			| android.widget.EditText
			| undefined;

		if (!edit) {return null;}
		return { start: edit.getSelectionStart(), end: edit.getSelectionEnd() };
	}
}

@JavaProxy('com.goddard.ui.PasteAwareEditText')
class PasteAwareEditText extends android.widget.EditText {
	public owner: WeakRef<PasteAwareTextView> | null = null;

	constructor(context: android.content.Context) {
		super(context);
		return global.__native(this);
	}

	/** Stock Android hides Paste unless the clip claims a text mime; an
	 * image-only clipboard still deserves the action when we can attach it. */
	public canPaste(): boolean {
		try {
			const base = (android.widget.TextView.prototype as unknown as {
				canPaste(): boolean;
			}).canPaste;

			if (base.call(this)) {return true;}
		} catch { /* older runtimes may not expose the super call */ }

		return Boolean(clipboardUriFiles().length);
	}

	public onTextContextMenuItem(id: number): boolean {
		const isPaste = id === android.R.id.paste
			|| id === android.R.id.pasteAsPlainText;

		if (isPaste) {
			const owner = this.owner?.deref();
			const files = clipboardUriFiles();
			if (owner?.pasteHandler && files.length) {
				owner.pasteHandler(files);
				return true;
			}
		}

		return super.onTextContextMenuItem(id);
	}

	public onSelectionChanged(start: number, end: number): void {
		super.onSelectionChanged(start, end);
		this.owner?.deref()?.emitSelectionChange(start, end);
	}
}

/** Clip items carrying a URI become attachments; a clip with any URI wins
 * over plain text — the same either/or the reference's paste input made. */
function clipboardUriFiles(): LocalAttachmentFile[] {
	const context = Utils.android.getApplicationContext();
	const clipboard = context.getSystemService(
		android.content.Context.CLIPBOARD_SERVICE,
	) as android.content.ClipboardManager | null;

	const clip = clipboard?.getPrimaryClip?.();
	if (!clip) {return [];}

	const files: LocalAttachmentFile[] = [];
	for (let index = 0; index < clip.getItemCount(); index++) {
		const uri = clip.getItemAt(index)?.getUri();
		if (!uri) {continue;}
		const file = describeDocument(uri.toString());
		files.push({ ...file, mimeType: contentType(uri) });
	}

	return files;
}

function contentType(uri: android.net.Uri): string | null {
	try {
		return (
			Utils.android.getApplicationContext()
				.getContentResolver()
				.getType(uri) ?? null
		);
	} catch {
		return null;
	}
}
