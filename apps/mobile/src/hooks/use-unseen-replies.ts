import { useSyncExternalStore } from 'react';

import { useDaemon } from '@/lib/daemon-context';
import {
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
