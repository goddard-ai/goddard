import type {
  AgentSession,
  BossDeliverable,
  BossEmployee,
  BossState,
  MessageAttachment,
} from '@waku/client';

import { deliverableTarget } from '@waku/client/boss';

import { imageMimeForPath } from './task-surfaces';

/** How long a published deliverable keeps its sidebar row after its last
 * publish — pinned and dormant rows never age out. Mirrors the desktop's
 * `SIDEBAR_DELIVERABLE_RECENT_SECS` (12h). */
export const DELIVERABLE_RECENT_SECS = 12 * 60 * 60;

/** Live deliverables shown before the rest fold behind "Show more" — the
 * desktop's `SIDEBAR_DELIVERABLE_DEFAULT_VISIBLE`. Pinned rows lead and
 * count toward the cap. */
export const DELIVERABLE_RECENT_VISIBLE = 5;

export interface RecentDeliverables {
  /** Pinned or still inside the recency window — pinned first, then newest. */
  live: BossDeliverable[];
  /** Swept rows behind the "Show dormant" fold — newest first. */
  dormant: BossDeliverable[];
}

/** The sidebar's deliverable split: a live row is pinned or was published
 * inside the recency window; a swept row (dormant without a pin) sits behind
 * the dormant fold; archived records never list. Mirrors the desktop's
 * `sidebar_recent_deliverables` for the one daemon this app shows at a time.
 * `now` and the record's `updatedAt` are unix seconds. */
export function recentDeliverables(
  deliverables: BossDeliverable[],
  now: number,
): RecentDeliverables {
  const live: BossDeliverable[] = [];
  const dormant: BossDeliverable[] = [];
  for (const deliverable of deliverables) {
    if (deliverable.archivedAt != null) {continue;}
    const pinned = deliverable.pinnedAt != null;
    if (!pinned && deliverable.dormantAt != null) {
      dormant.push(deliverable);
    } else if (pinned || now - deliverable.updatedAt < DELIVERABLE_RECENT_SECS) {
      live.push(deliverable);
    }
  }

  const byNewest = (a: BossDeliverable, b: BossDeliverable) =>
    b.updatedAt - a.updatedAt || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

  live.sort(
    (a, b) =>
      Number(b.pinnedAt != null) - Number(a.pinnedAt != null) || byNewest(a, b),
  );

  dormant.sort(byNewest);
  return { live, dormant };
}

/** The library's record filter — the sidebar's recency window does not
 * apply; every published record lists under one of these. */
export type DeliverablesFilter = 'all' | 'pinned' | 'dormant' | 'archived';

export const DELIVERABLE_FILTERS: readonly DeliverablesFilter[] = [
  'all',
  'pinned',
  'dormant',
  'archived',
];

/** Records visible under a library filter, sorted the way every filter
 * sorts: pinned first, then newest publish. Mirrors the desktop's
 * `BossDeliverablesFilter` gate and sort. */
export function filterDeliverables(
  deliverables: BossDeliverable[],
  filter: DeliverablesFilter,
): BossDeliverable[] {
  const visible = deliverables.filter((deliverable) => {
    const pinned = deliverable.pinnedAt != null;
    const dormant = deliverable.dormantAt != null && !pinned;
    const archived = deliverable.archivedAt != null;
    switch (filter) {
      case 'all':
        return !dormant && !archived;
      case 'pinned':
        return pinned && !archived;
      case 'dormant':
        return dormant && !archived;
      case 'archived':
        return archived;
    }
  });

  return visible.sort(
    (a, b) =>
      Number(b.pinnedAt != null) - Number(a.pinnedAt != null)
      || b.updatedAt - a.updatedAt
      || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
  );
}

/** The deliverable's file or folder name — the detail line's file part. */
export function deliverableFileName(deliverable: BossDeliverable): string {
  const path = deliverable.path;
  const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return separator >= 0 && separator < path.length - 1
    ? path.slice(separator + 1)
    : path;
}

/** What the preview sheet can render: a daemon-readable image, Markdown, or
 * plain text. Directories and files a text read rejects show the record's
 * identity instead — the sheet is never blank. */
export type DeliverablePreviewKind = 'image' | 'markdown' | 'text' | 'directory';

export function deliverablePreviewKind(
  deliverable: BossDeliverable,
): DeliverablePreviewKind {
  if (deliverable.directory) {return 'directory';}
  if (imageMimeForPath(deliverable.path)) {return 'image';}
  const name = deliverableFileName(deliverable).toLowerCase();
  if (name.endsWith('.md') || name.endsWith('.markdown')) {return 'markdown';}
  return 'text';
}

/** The attachment an armed deliverable contributes to a boss submission —
 * its mention token splices into the provider-facing prompt as `@<path>`
 * (trailing slash for a directory), the desktop's
 * `boss_command_attachment` shape. */
export function deliverableAttachment(
  deliverable: BossDeliverable,
): MessageAttachment {
  const name = deliverable.name;
  return {
    path: deliverable.path,
    mention: deliverable.directory ? `${deliverable.path}/` : deliverable.path,
    name,
    is_dir: deliverable.directory,
    is_image:
      !deliverable.directory && imageMimeForPath(deliverable.path) != null,
    blob_reference: null,
    pasted_text_preview: null,
    session_id: null,
  };
}

/** Root + relative path a deliverable's bytes read through — the client's
 * `deliverableTarget`, named for the workspace operations the sheet calls. */
export function deliverableReadTarget(deliverable: BossDeliverable): {
  root: string;
  relativePath: string | null;
} {
  return deliverableTarget(deliverable);
}

/** The employee a deliverable came from, matched on the workspace its
 * source path lived under — `source_path` is the copy's origin, a live
 * reference's `path` names the source itself. `null` when no known session
 * workspace contains it (expired records, boss-published files). */
export function deliverableProducer(
  state: BossState,
  deliverable: BossDeliverable,
  sessions: AgentSession[],
): BossEmployee | null {
  const source = deliverable.sourcePath ?? deliverable.path;
  const workspaceContains = (root: string | undefined | null) =>
    root != null && (source === root || source.startsWith(`${root}/`));

  const candidates = [...state.employees, ...state.retiredEmployees];
  for (const employee of candidates) {
    const session = sessions.find((entry) => entry.id === employee.sessionId);
    const workspace = session?.workspace;
    if (
      workspace?.kind === 'worktree'
      && workspaceContains(workspace.path)
    ) {return employee;}
  }

  return null;
}
