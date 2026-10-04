import { create as createImagePicker, ImagePickerMediaType } from '@nativescript/imagepicker';
import { openFilePicker } from '@nativescript-community/ui-document-picker';
import {
  requestPermissions,
  takePicture,
} from '@nativescript/camera';

import { File, Utils, knownFolders, path as nsPath } from '@nativescript/core';


import { localFileName, type LocalAttachmentFile } from './attachments';

declare const android:
  | {
      net: { Uri: { parse(uri: string): unknown } };
      provider: { OpenableColumns: { DISPLAY_NAME: string; SIZE: string } };
    }
  | undefined;

export type AttachmentSource = 'files' | 'camera' | 'photo';

export async function pickAttachments(source: AttachmentSource): Promise<LocalAttachmentFile[] | null> {
  if (source === 'files') {
    const result = await openFilePicker({ multipleSelection: true });
    const uris = result.files ?? [];
    if (!uris.length) {return null;}
    return uris.map(describeDocument);
  }

  if (source === 'camera') {
    const granted = await requestPermissions();
    if (!granted) {throw new Error('Camera access is required to take a photo');}
    const asset = await takePicture({ saveToGallery: false });
    const file = await saveToTmp(asset, `Photo-${Date.now()}.jpg`);
    return [file];
  }

  const picker = createImagePicker({ mode: 'multiple', mediaType: ImagePickerMediaType.Image, copyToAppFolder: 'pick' });
  await picker.authorize();
  const selection = await picker.present();
  return selection.map((item, index) => ({
    uri: item.path ?? '',
    name: item.filename ?? item.originalFilename ?? `Photo-${Date.now()}-${index + 1}.jpg`,
    mimeType: item.type === 'image' ? 'image/jpeg' : 'video/mp4',
    size: item.filesize,
  }));
}

interface AndroidCursor {
  moveToFirst(): boolean;
  getColumnIndex(name: string): number;
  getString(index: number): string | null;
  getLong(index: number): number;
  close(): void;
}

/** The picker returns plain paths on iOS (import mode copies into the app
 * inbox) and below-30 Android; on newer Android it is a `content://` URI
 * whose display name and size live in the provider's OpenableColumns. */
function describeDocument(uri: string): LocalAttachmentFile {
  const fallback = {
    uri,
    name: localFileName(uri, 'Attachment'),
    size: File.exists(uri) ? File.fromPath(uri).size : null,
  };

  if (!uri.startsWith('content://')) {return fallback;}
  const ns = android;
  if (!ns) {return fallback;}
  try {
    const parsed = ns.net.Uri.parse(uri);
    const resolver = Utils.android.getApplicationContext().getContentResolver() as unknown as {
      query(
        uri: unknown,
        projection: string[] | null,
        selection: string | null,
        selectionArgs: string[] | null,
        sortOrder: string | null,
      ): AndroidCursor | null;
    };

    const cursor = resolver.query(parsed, null, null, null, null);
    if (!cursor?.moveToFirst()) {
      cursor?.close();
      return fallback;
    }

    try {
      const nameIndex = cursor.getColumnIndex(ns.provider.OpenableColumns.DISPLAY_NAME);
      const sizeIndex = cursor.getColumnIndex(ns.provider.OpenableColumns.SIZE);
      return {
        uri,
        name: (nameIndex >= 0 && cursor.getString(nameIndex)) || fallback.name,
        size: sizeIndex >= 0 ? cursor.getLong(sizeIndex) : null,
      };
    } finally {
      cursor.close();
    }
  } catch {
    return fallback;
  }
}

async function saveToTmp(asset: unknown, name: string): Promise<LocalAttachmentFile> {
  const source = await (asset as { toImageSource?: () => Promise<unknown> }).toImageSource?.()
    ?? (await import('@nativescript/core/image-source').then((m) => m.ImageSource.fromAsset(asset as never)));

  const target = nsPath.join(knownFolders.temp().path, name);
  (source as { saveToFile: (p: string, f: string) => void }).saveToFile(target, 'jpg');
  const file = File.fromPath(target);
  return { uri: target, name, mimeType: 'image/jpeg', size: file.size };
}
