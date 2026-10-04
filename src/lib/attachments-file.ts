import { File, Utils } from '@nativescript/core';

declare const android:
  | {
      net: { Uri: { parse(uri: string): unknown } };
      util: { Base64: { encodeToString(b: unknown, flags: number): string; NO_WRAP: number } };
    }
  | undefined;

declare const java:
  | {
      io: {
        ByteArrayOutputStream: new () => {
          write(b: unknown, offset: number, length: number): void;
          toByteArray(): unknown;
        };
      };
    }
  | undefined;

/** Read a local file URI as base64 (replaces expo-file-system's File.base64). */
export function readFileBase64(uri: string): Promise<string> {
  if (uri.startsWith('content://')) {return Promise.resolve(readContentUri(uri));}
  const localPath = uri.startsWith('file://') ? uri.slice('file://'.length) : uri;
  const file = File.fromPath(localPath);
  const data = file.readSync((error: unknown) => {
    throw error instanceof Error ? error : new Error(`Could not read ${uri}`);
  });

  if (!data) {throw new Error(`Could not read ${uri}`);}
  let encoded: string;
  if (typeof data === 'object' && 'base64EncodedStringWithOptions' in data) {
    // iOS NSData
    encoded = (data as { base64EncodedStringWithOptions(o: number): string }).base64EncodedStringWithOptions(0);
  } else {
    // Android byte[]
    const base64 = (android as NonNullable<typeof android>)?.util.Base64;
    encoded = base64.encodeToString(data, base64.NO_WRAP);
  }

  return Promise.resolve(encoded);
}

/** Document-picker URIs on Android API 30+ are `content://` — their bytes
 * come through the content resolver, not the filesystem. */
function readContentUri(uri: string): string {
  const ns = android;
  const io = java?.io;
  if (!ns || !io) {throw new Error(`Could not read ${uri}`);}
  const stream = Utils.android.getApplicationContext().getContentResolver()
    .openInputStream(ns.net.Uri.parse(uri) as never) as unknown as {
      read(buffer: unknown): number;
      close(): void;
    } | null;

  if (!stream) {throw new Error(`Could not read ${uri}`);}
  try {
    const output = new io.ByteArrayOutputStream();
    const buffer = (Array as unknown as { create(type: string, count: number): unknown })
      .create('byte', 8192);

    let read = 0;
    while ((read = stream.read(buffer)) !== -1) {output.write(buffer, 0, read);}
    return ns.util.Base64.encodeToString(output.toByteArray(), ns.util.Base64.NO_WRAP);
  } finally {
    stream.close();
  }
}
