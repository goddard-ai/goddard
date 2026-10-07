import { describe, expect, test } from 'bun:test';

import type { BossDeliverable } from '@waku/client';

import {
  DELIVERABLE_RECENT_SECS,
  DELIVERABLE_RECENT_VISIBLE,
  deliverableAttachment,
  deliverableFileName,
  deliverablePreviewKind,
  filterDeliverables,
  recentDeliverables,
} from './deliverables';

const HOUR = 3_600;
const NOW = 1_800_000_000;

let seq = 0;
function deliverable(overrides: Partial<BossDeliverable> = {}): BossDeliverable {
  seq += 1;
  return {
    id: `deliverable-${seq}`,
    name: `Report ${seq}`,
    path: `/var/deliverables/report-${seq}.md`,
    directory: false,
    createdAt: NOW,
    updatedAt: NOW,
    pinnedAt: null,
    dormantAt: null,
    archivedAt: null,
    viewedAt: null,
    ...overrides,
  };
}

describe('recentDeliverables', () => {
  test('live rows keep to twelve hours, newest first', () => {
    const fresh = deliverable({ updatedAt: NOW - HOUR });
    const older = deliverable({ updatedAt: NOW - 2 * HOUR });
    const edge = deliverable({ updatedAt: NOW - DELIVERABLE_RECENT_SECS + 1 });
    const expired = deliverable({ updatedAt: NOW - DELIVERABLE_RECENT_SECS });

    const { live, dormant } = recentDeliverables(
      [expired, older, edge, fresh],
      NOW,
    );

    expect(live.map((item) => item.id)).toEqual([fresh.id, older.id, edge.id]);
    expect(dormant).toEqual([]);
  });

  test('pinned rows lead regardless of age; swept rows fold; archived vanish', () => {
    const pinnedOld = deliverable({
      updatedAt: NOW - 48 * HOUR,
      pinnedAt: NOW - 48 * HOUR,
    });

    const swept = deliverable({ dormantAt: NOW - 30 });
    const archived = deliverable({ archivedAt: NOW });
    const fresh = deliverable({ updatedAt: NOW - 60 });
    // A pin outranks a sweep — a pinned task cannot be dormant either.
    const pinnedSwept = deliverable({
      updatedAt: NOW - 10,
      pinnedAt: NOW,
      dormantAt: NOW - 5,
    });

    const { live, dormant } = recentDeliverables(
      [swept, pinnedOld, archived, pinnedSwept, fresh],
      NOW,
    );

    expect(live.map((item) => item.id)).toEqual([pinnedSwept.id, pinnedOld.id, fresh.id]);
    expect(dormant.map((item) => item.id)).toEqual([swept.id]);
  });
});

describe('filterDeliverables', () => {
  const pinnedOld = () =>
    deliverable({ pinnedAt: NOW - 2 * HOUR, updatedAt: NOW - 2 * HOUR });

  const live = () => deliverable({ updatedAt: NOW - 10 });
  const dormant = () => deliverable({ dormantAt: NOW - 60 });
  const archived = () => deliverable({ archivedAt: NOW - 30 });

  test('all hides dormant and archived; pinned leads', () => {
    const pin = pinnedOld();
    const stale = deliverable({ updatedAt: NOW - 20 * HOUR });
    const fresh = live();
    const rows = filterDeliverables([stale, dormant(), archived(), pin, fresh], 'all');
    expect(rows.map((item) => item.id)).toEqual([pin.id, fresh.id, stale.id]);
  });

  test('pinned shows only pinned live records', () => {
    const pin = pinnedOld();
    const archivedPin = deliverable({ archivedAt: NOW, pinnedAt: NOW });
    const rows = filterDeliverables([live(), dormant(), pin, archivedPin], 'pinned');
    expect(rows.map((item) => item.id)).toEqual([pin.id]);
  });

  test('dormant shows swept-unpinned rows only', () => {
    const swept = dormant();
    const pinnedSwept = deliverable({ dormantAt: NOW, pinnedAt: NOW });
    const rows = filterDeliverables([live(), swept, pinnedSwept, archived()], 'dormant');
    expect(rows.map((item) => item.id)).toEqual([swept.id]);
  });

  test('archived shows only archived records', () => {
    const gone = archived();
    const rows = filterDeliverables([live(), dormant(), gone], 'archived');
    expect(rows.map((item) => item.id)).toEqual([gone.id]);
  });
});

describe('deliverableFileName', () => {
  test('basename for unix and windows separators', () => {
    expect(deliverableFileName(deliverable({ path: '/a/b/report.md' }))).toBe('report.md');
    expect(deliverableFileName(deliverable({ path: 'C:\\work\\out.txt' }))).toBe('out.txt');
    expect(deliverableFileName(deliverable({ path: '/a/b/' }))).toBe('/a/b/');
    expect(deliverableFileName(deliverable({ path: 'solo.md' }))).toBe('solo.md');
  });
});

describe('deliverablePreviewKind', () => {
  test('classifies directory, image, markdown, and plain text', () => {
    expect(deliverablePreviewKind(deliverable({ directory: true }))).toBe('directory');
    expect(deliverablePreviewKind(deliverable({ path: '/a/chart.png' }))).toBe('image');
    expect(deliverablePreviewKind(deliverable({ path: '/a/notes.MD' }))).toBe('markdown');
    expect(deliverablePreviewKind(deliverable({ path: '/a/log.txt' }))).toBe('text');
  });
});

describe('deliverableAttachment', () => {
  test('files carry their absolute path as the mention', () => {
    const attachment = deliverableAttachment(
      deliverable({ path: '/store/reports/q3.md', name: 'Q3 report' }),
    );

    expect(attachment).toMatchObject({
      path: '/store/reports/q3.md',
      mention: '/store/reports/q3.md',
      name: 'Q3 report',
      is_dir: false,
      is_image: false,
      blob_reference: null,
      session_id: null,
    });
  });

  test('directories get a trailing slash and the dir flag', () => {
    const attachment = deliverableAttachment(
      deliverable({ directory: true, path: '/store/site', name: 'Site build' }),
    );

    expect(attachment.mention).toBe('/store/site/');
    expect(attachment.is_dir).toBe(true);
  });

  test('images flag is_image for the chip', () => {
    const attachment = deliverableAttachment(deliverable({ path: '/a/shot.png' }));
    expect(attachment.is_image).toBe(true);
  });
});

test('the recent cap matches the desktop default', () => {
  expect(DELIVERABLE_RECENT_VISIBLE).toBe(5);
});
