import { Application } from '@nativescript/core';

/** Module-scope app-foreground feed — the hook-free half of useAppState so
 * the daemon link can pause/resume retries without a component. */
export function subscribeAppForeground(
  notify: (foreground: boolean) => void,
): () => void {
  const onSuspend = () => notify(false);
  const onResume = () => notify(true);
  const onExit = () => notify(false);
  Application.on(Application.suspendEvent, onSuspend);
  Application.on(Application.resumeEvent, onResume);
  Application.on(Application.exitEvent, onExit);
  return () => {
    Application.off(Application.suspendEvent, onSuspend);
    Application.off(Application.resumeEvent, onResume);
    Application.off(Application.exitEvent, onExit);
  };
}
