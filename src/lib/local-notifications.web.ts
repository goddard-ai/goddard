import type {
  LocalNotice,
  LocalNoticeTapHandler,
  LocalNoticeTarget,
} from './local-notifications.types';

export type { LocalNotice, LocalNoticeTarget } from './local-notifications.types';

type TapHandler = LocalNoticeTapHandler;

let permissionRequest: Promise<boolean> | null = null;
let tapHandler: TapHandler | null = null;

/** Web Notification API — permission persists per origin, so the first
 * request is the only prompt. Unsupported browsers resolve false and the
 * feed simply stays quiet. */
export function ensureLocalNotifications(): Promise<boolean> {
  if (typeof Notification === 'undefined') {
    return Promise.resolve(false);
  }

  permissionRequest ??= (async () => {
    if (Notification.permission === 'granted') {return true;}
    if (Notification.permission === 'denied') {return false;}
    return (await Notification.requestPermission()) === 'granted';
  })().catch(() => false);

  return permissionRequest;
}

export function onLocalNotificationTap(handler: TapHandler): void {
  tapHandler = handler;
}

export function postLocalNotification(notice: LocalNotice): void {
  void ensureLocalNotifications().then((granted) => {
    if (!granted) {return;}
    // `tag` replaces a pending banner for the same subject — the native
    // leaf's keyed id does the same on-device.
    const notification = new Notification(notice.title, {
      body: notice.body,
      tag: notice.key,
      data: notice.target ? { target: notice.target } : undefined,
    });

    notification.onclick = () => {
      window.focus();
      const target = (notification.data as { target?: LocalNoticeTarget } | undefined)?.target;
      if (target) {tapHandler?.(target);}
      notification.close();
    };
  });
}
