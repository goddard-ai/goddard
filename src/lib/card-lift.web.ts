/**
 * Web typecheck stand-in — `task-drawer-surface.tsx` is the native-default
 * leaf the web tsconfig still visits; its web counterpart never imports
 * this.
 */
export function liftCard(_card: unknown): void {}
