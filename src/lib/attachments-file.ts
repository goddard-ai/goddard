import { File } from '@nativescript/core';

declare const android: { util: { Base64: { encodeToString(b: unknown, flags: number): string; NO_WRAP: number } } } | undefined;

/** Read a local file URI as base64 (replaces expo-file-system's File.base64). */
export function readFileBase64(uri: string): Promise<string> {
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
