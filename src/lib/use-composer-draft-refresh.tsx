import { useEffect, useState } from 'octane';

import { subscribeAppForeground } from './app-foreground';

/**
 * Changes whenever the app foregrounds while a composer is mounted. The
 * reference also bumped on route focus; xplat screens remount on navigation,
 * which covers the same refresh boundary — flagged if a screen ever keeps
 * mounted state under a pushed route.
 */
export function useComposerDraftRefreshRevision(): number | null {
  const [revision, setRevision] = useState<number | null>(null);
  const bump = () => setRevision((value) => (value === null ? 0 : value + 1));

  useEffect(() => {
    bump();
    let wasForeground = true;
    return subscribeAppForeground((foreground) => {
      if (foreground && !wasForeground) {bump();}
      wasForeground = foreground;
    });
  }, []);

  return revision;
}
