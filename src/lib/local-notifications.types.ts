/** Where a tapped notification lands. `session` opens a task; `deliverable`
 * resolves the record and opens its boss chat armed (the library when no
 * chat exists); `route` names a plain root route. */
export type LocalNoticeTarget =
  | { kind: 'session'; sessionId: string }
  | { kind: 'deliverable'; deliverableId: string }
  | { kind: 'route'; name: 'index' | 'daemons' | 'deliverables' | 'notifications' };

export interface LocalNotice {
  /** Stable tag — re-posting replaces the earlier banner for the same
   * subject instead of stacking, the desktop's per-session notification
   * tag rule. */
  key: string;
  title: string;
  body?: string;
  target?: LocalNoticeTarget;
}

export type LocalNoticeTapHandler = (target: LocalNoticeTarget) => void;
