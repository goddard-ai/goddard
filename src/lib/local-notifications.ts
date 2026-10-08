import { LocalNotifications } from '@nativescript/local-notifications';

import type {
  LocalNotice,
  LocalNoticeTapHandler,
  LocalNoticeTarget,
} from './local-notifications.types';

export type { LocalNotice, LocalNoticeTarget } from './local-notifications.types';

type TapHandler = LocalNoticeTapHandler;

let permissionRequest: Promise<boolean> | null = null;
let tapHandler: TapHandler | null = null;
let tapRegistered = false;

/** iOS asks once; Android reports granted outright. The first qualifying
 * event triggers the prompt rather than boot — a daemon with nothing to
 * report never interrupts setup with a permission sheet. */
export function ensureLocalNotifications(): Promise<boolean> {
  permissionRequest ??= LocalNotifications.hasPermission()
    .then((granted) => granted || LocalNotifications.requestPermission())
    .catch(() => false);

  return permissionRequest;
}

/** Register the deep-link tap handler once — iOS queues notifications
 * delivered before the callback lands, so install at feed setup. */
export function onLocalNotificationTap(handler: TapHandler): void {
  tapHandler = handler;
  if (tapRegistered) {return;}
  tapRegistered = true;
  void LocalNotifications.addOnMessageReceivedCallback((data) => {
    const payload = data.payload as { target?: LocalNoticeTarget } | undefined;
    if (payload?.target) {tapHandler?.(payload.target);}
  }).catch(() => {
    tapRegistered = false;
  });
}

export function postLocalNotification(notice: LocalNotice): void {
  void ensureLocalNotifications().then((granted) => {
    if (!granted) {return;}
    void LocalNotifications.schedule([{
      id: noticeId(notice.key),
      title: notice.title,
      body: notice.body ?? '',
      payload: notice.target ? { target: notice.target } : undefined,
    }]).catch(() => {});
  });
}

/** The plugin's schedule id is a 32-bit int — hash the stable string tag. */
function noticeId(key: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < key.length; index += 1) {
    hash = Math.imul(hash ^ key.charCodeAt(index), 0x01000193);
  }

  return (hash >>> 0) % 2147483647;
}
