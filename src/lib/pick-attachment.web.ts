import type { LocalAttachmentFile } from './attachments';

export type AttachmentSource = 'files' | 'camera' | 'photo';

export async function pickAttachments(source: AttachmentSource): Promise<LocalAttachmentFile[] | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    input.accept = source === 'files' ? '*/*' : 'image/*';
    if (source === 'camera') {input.setAttribute('capture', 'environment');}
    input.onchange = () => {
      const files = [...(input.files ?? [])].map((file) => ({
        uri: URL.createObjectURL(file),
        name: file.name,
        mimeType: file.type || undefined,
        size: file.size,
      }));

      resolve(files.length ? files : null);
    };

    input.click();
  });
}
