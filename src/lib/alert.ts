import { Dialogs } from '@nativescript/core';

/** Alert.alert replacement — NativeScript system alert dialog. */
export async function showAlert(title: string, message?: string): Promise<void> {
  await Dialogs.alert({ title, message: message ?? '', okButtonText: 'OK' });
}

/** Alert.alert with a destructive confirm — NativeScript confirm dialog. */
export async function confirmDestructive(
  title: string,
  message: string,
  confirmLabel: string,
): Promise<boolean> {
  return Dialogs.confirm({
    title,
    message,
    okButtonText: confirmLabel,
    cancelButtonText: 'Cancel',
  });
}
