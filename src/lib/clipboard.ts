import { copyToClipboard } from '@nativescript/core/utils';

export async function setStringAsync(value: string): Promise<void> {
  copyToClipboard(value);
}
