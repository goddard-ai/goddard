import { useMemo, useSyncExternalStore } from 'react';

import { useTaskState } from '@/hooks/use-daemon-data';
import { useDaemon } from '@/lib/daemon-context';
import {
  sessionHasUnseenReply,
  subscribeUnseenReplies,
  unseenReplyWatermarks,
  type UnseenReplyWatermarks,
} from '@/lib/unseen-replies';

/** The active daemon's per-session seen watermarks; re-renders when a mark or
 * seed write lands. */
export function useUnseenReplyWatermarks(): UnseenReplyWatermarks {
  const address = useDaemon().activeProfile?.address;
  const snapshot = () => unseenReplyWatermarks(address);
  return useSyncExternalStore(subscribeUnseenReplies, snapshot, snapshot);
}

/** True while any session except the one on screen carries a reply newer
 * than the user's watermark — drives the drawer button's new-content dot. */
export function useHasUnseenReplies(excludedSessionId?: string | null): boolean {
  const sessions = useTaskState().data?.sessions;
  const watermarks = useUnseenReplyWatermarks();
  return useMemo(
    () => Boolean(sessions?.some(
      (session) =>
        session.id !== excludedSessionId &&
        sessionHasUnseenReply(session, watermarks),
    )),
    [excludedSessionId, sessions, watermarks],
  );
}
