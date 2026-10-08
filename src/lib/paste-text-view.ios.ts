/// <reference types="@nativescript/types" />
import { File, TextView, knownFolders, path as nsPath } from '@nativescript/core';

import { localFileName, type LocalAttachmentFile } from './attachments';

/**
 * A TextView whose UITextView can paste image and file clipboards as
 * attachments — the shared ComposerTextInput's native leaf swaps the stock
 * widget for this one. `pasteHandler` set means image/file paste payloads
 * are delivered as LocalAttachmentFiles instead of inserting into the text;
 * text paste, and clipboards we can't import, keep the default behavior.
 * Selection moves are re-emitted so the composer's context picker inserts at
 * the live caret even when the user only taps to reposition it.
 */
export class PasteAwareTextView extends TextView {
	public pasteHandler: ((files: LocalAttachmentFile[]) => void) | null = null;
	public selectionHandler: ((start: number, end: number) => void) | null = null;

	public createNativeView(): UITextView {
		return PasteAwareUITextView.new();
	}

	public initNativeView(): void {
		super.initNativeView();
		const native = this.nativeTextViewProtected as PasteAwareUITextView;
		native.owner = new WeakRef(this);
		// Caret moves surface through the UITextView delegate — the widget
		// installs its own `UITextViewDelegateImpl` — so patch the optional
		// `textViewDidChangeSelection:` onto it rather than racing the
		// `setSelectedRange:` setter (a property write would recurse).
		const delegate = native.delegate as unknown as
			| { textViewDidChangeSelection?: (textView: UITextView) => void }
			| null;

		if (delegate) {
			const owner = new WeakRef(this);
			const previous = delegate.textViewDidChangeSelection?.bind(delegate);
			delegate.textViewDidChangeSelection = (textView: UITextView) => {
				previous?.(textView);
				const range = textView.selectedRange;
				owner.deref()?.emitSelectionChange(
					range.location,
					range.location + range.length,
				);
			};
		}
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
		const range = this.nativeTextViewProtected?.selectedRange;
		return range
			? { start: range.location, end: range.location + range.length }
			: null;
	}
}

@NativeClass()
class PasteAwareUITextView extends UITextView {
	public owner: WeakRef<PasteAwareTextView> | null = null;

	public static new(): PasteAwareUITextView {
		return super.new() as PasteAwareUITextView;
	}

	// UITextView used to scroll to a content offset when showing the edit
	// menu on iOS 14; core's stock widget guards it the same way.
	public setContentOffsetAnimated(contentOffset: CGPoint, _animated: boolean): void {
		super.setContentOffsetAnimated(contentOffset, false);
	}

	public canPerformActionWithSender(action: string, sender: unknown): boolean {
		if (
			action === 'paste:'
			&& this.owner?.deref()?.pasteHandler
			&& pasteboardHasAttachableContent()
		) {
			return true;
		}

		return super.canPerformActionWithSender(action, sender);
	}

	public paste(sender: unknown): void {
		const owner = this.owner?.deref();
		if (owner?.pasteHandler && pasteboardHasAttachableContent()) {
			const files = clipboardFiles();
			if (files?.length) {
				owner.pasteHandler(files);
				return;
			}
		}

		super.paste(sender);
	}

}

/** Files or images — anything the paste path can turn into an attachment.
 * A text-only clipboard leaves Paste availability to the stock behavior. */
function pasteboardHasAttachableContent(): boolean {
	const pasteboard = UIPasteboard.generalPasteboard;
	if (pasteboard.hasImages) {return true;}
	const urls = pasteboard.URLs;
	return urls != null && urls.count > 0 && urlsAreFiles(urls);
}

function urlsAreFiles(urls: NSArray<NSURL>): boolean {
	if (!urls.count) {return false;}
	for (let index = 0; index < urls.count; index++) {
		const url = urls.objectAtIndex(index);
		if (!url?.isFileURL) {return false;}
	}

	return true;
}

/** The clipboard's attachable payload — PNG-encoded images written to temp,
 * and file URLs as importable paths. Returns null when there is nothing to
 * attach so the caller can fall through to default paste. */
function clipboardFiles(): LocalAttachmentFile[] | null {
	const pasteboard = UIPasteboard.generalPasteboard;
	if (pasteboard.hasImages) {
		const images = pasteboard.images;
		if (!images) {return null;}
		const files: LocalAttachmentFile[] = [];
		for (let index = 0; index < images.count; index++) {
			const data = UIImagePNGRepresentation(images.objectAtIndex(index));
			if (!data) {continue;}
			const name = index === 0
				? 'Pasted image.png'
				: `Pasted image ${index + 1}.png`;

			const target = nsPath.join(knownFolders.temp().path, name);
			data.writeToFileAtomically(target, true);
			const file = File.exists(target) ? File.fromPath(target) : null;
			files.push({
				uri: `file://${target}`,
				name,
				mimeType: 'image/png',
				size: file?.size ?? null,
			});
		}

		return files.length ? files : null;
	}

	const urls = pasteboard.URLs;
	if (urls?.count && urlsAreFiles(urls)) {
		const files: LocalAttachmentFile[] = [];
		for (let index = 0; index < urls.count; index++) {
			const url = urls.objectAtIndex(index);
			const filePath = url.path;
			if (!filePath) {continue;}
			files.push({
				uri: `file://${filePath}`,
				name: url.lastPathComponent ?? localFileName(filePath, 'Attachment'),
				size: File.exists(filePath) ? File.fromPath(filePath).size : null,
			});
		}

		return files.length ? files : null;
	}

	return null;
}
