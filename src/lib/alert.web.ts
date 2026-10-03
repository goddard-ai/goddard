export async function showAlert(title: string, message?: string): Promise<void> {
  window.alert(message ? `${title}\n\n${message}` : title);
}

export async function confirmDestructive(
  title: string,
  message: string,
  _confirmLabel: string,
): Promise<boolean> {
  return window.confirm(message ? `${title}\n\n${message}` : title);
}
