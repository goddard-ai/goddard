import { create as createImagePicker, ImagePickerMediaType } from '@nativescript/imagepicker';
import {
  requestPermissions,
  takePicture,
} from '@nativescript/camera';

import { File, knownFolders, path as nsPath } from '@nativescript/core';


import type { LocalAttachmentFile } from './attachments';

export type AttachmentSource = 'files' | 'camera' | 'photo';

/** Generic document picking has no maintained NativeScript plugin —
 * flagged blocker. Photo/library attachments work. */
export async function pickAttachments(source: AttachmentSource): Promise<LocalAttachmentFile[] | null> {
  if (source === 'files') {
    throw new Error('File picking is not available on this platform yet — use Photo');
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

async function saveToTmp(asset: unknown, name: string): Promise<LocalAttachmentFile> {
  const source = await (asset as { toImageSource?: () => Promise<unknown> }).toImageSource?.()
    ?? (await import('@nativescript/core/image-source').then((m) => m.ImageSource.fromAsset(asset as never)));

  const target = nsPath.join(knownFolders.temp().path, name);
  (source as { saveToFile: (p: string, f: string) => void }).saveToFile(target, 'jpg');
  const file = File.fromPath(target);
  return { uri: target, name, mimeType: 'image/jpeg', size: file.size };
}
