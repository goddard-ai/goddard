export function dismissKeyboard() {
  (document.activeElement as HTMLElement | null)?.blur?.();
}
