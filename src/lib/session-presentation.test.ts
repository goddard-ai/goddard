import { expect, test } from 'bun:test';

import type { AgentSession, Message, ReportTrigger } from '@waku/client';

import { TranscriptMarkdownCache } from '../md/transcript-cache';
import {
  buildTranscriptPipeline,
  buildTranscriptRows,
  type TranscriptRow,
} from './session-presentation';

const TURN = '11111111-1111-4111-8111-111111111111';

function session(overrides: Partial<AgentSession>): AgentSession {
  return {
    id: 'chat',
    messages: [],
    provider: 'codex',
    transcript_blocks: [],
    turns: [],
    ...overrides,
  } as AgentSession;
}

function message(overrides: Partial<Message>): Message {
  return {
    id: Math.random().toString(36).slice(2),
    role: 'assistant',
    content: 'text',
    created_at: 0,
    streaming: false,
    turn_id: null,
    ...overrides,
  } as Message;
}

function trigger(overrides: Partial<ReportTrigger> = {}): ReportTrigger {
  return {
    eventId: Math.random().toString(36).slice(2),
    employee: 'e1',
    employeeName: 'Rhea',
    jobTitle: 'Fix it',
    kind: 'finished',
    boundary: 'opening',
    ...overrides,
  };
}

function kinds(rows: readonly { kind: string }[]) {
  return rows.map((row) => row.kind);
}

test('a hidden report prompt renders a trigger row, not a user bubble', () => {
  const report = message({
    role: 'user',
    hidden: true,
    turn_id: TURN,
    report_trigger: trigger(),
  });

  const answer = message({ role: 'assistant', turn_id: TURN });

  const rows = buildTranscriptRows(
    session({
      messages: [report, answer],
      turns: [{ id: TURN, status: 'done' } as never],
    }),
    new Set(),
    new TranscriptMarkdownCache(),
  );

  expect(rows[0]?.kind).toBe('bossTrigger');
  expect(kinds(rows)).not.toContain('user');
});

test('a turn-opening burst collapses to one expandable marker row', () => {
  const one = message({
    role: 'user',
    hidden: true,
    turn_id: TURN,
    report_trigger: trigger({ employeeName: 'Rhea' }),
  });

  const two = message({
    role: 'user',
    hidden: true,
    turn_id: TURN,
    report_trigger: trigger({ employeeName: 'Pablo', kind: 'blocker' }),
  });

  const answer = message({ role: 'assistant', turn_id: TURN });

  const pipeline = buildTranscriptPipeline(
    session({
      messages: [one, two, answer],
      turns: [{ id: TURN, status: 'done' } as never],
    }),
  );

  const marker = pipeline.find((row) => row.kind === 'bossTrigger');
  expect(marker && 'group' in marker ? marker.group.triggers.length : 0).toBe(2);
  expect(marker && 'expanded' in marker ? marker.expanded : true).toBe(false);

  // The burst toggles on the anchor trigger's event id.
  const expanded = buildTranscriptPipeline(
    session({
      messages: [one, two, answer],
      turns: [{ id: TURN, status: 'done' } as never],
    }),
    new Set(),
    new Set([one.report_trigger!.eventId]),
  );

  const expandedMarker = expanded.find((row) => row.kind === 'bossTrigger');
  expect(expandedMarker && 'expanded' in expandedMarker ? expandedMarker.expanded : false).toBe(true);
});

test('noticed messages render notice rows; unnoticed system text stays a pill', () => {
  const landed = message({
    role: 'system',
    content: 'Landed on main',
    notice: { type: 'landed', base: 'main', commits: [], ahead: 0 },
  });

  const plain = message({ role: 'system', content: 'Context compacted' });
  const status = message({
    role: 'assistant',
    content: 'Interrupted',
    notice: { type: 'status', kind: 'interrupted' },
  });

  // An assistant markdown body stays markdown when the notice is not a
  // status or transfer variant.
  const body = message({
    role: 'assistant',
    content: 'Provider switched',
    notice: { type: 'providerSwitched', from: 'codex', to: 'claude' },
  });

  const rows = buildTranscriptRows(
    session({ messages: [landed, plain, status, body] }),
    new Set(),
    new TranscriptMarkdownCache(),
  );

  expect(kinds(rows)).toEqual(['notice', 'system', 'notice', 'md']);
  expect(rows[0]?.key).toBe(`notice:${landed.id}`);
});

test('opening markers take the turn gap; steer markers take the group gap', () => {
  const firstTurn = message({ role: 'assistant', turn_id: 'turn-0' });
  const opening = message({
    role: 'user',
    hidden: true,
    turn_id: TURN,
    report_trigger: trigger(),
  });

  const answer = message({ role: 'assistant', turn_id: TURN });
  const steer = message({
    role: 'user',
    hidden: true,
    turn_id: TURN,
    report_trigger: trigger({ boundary: 'steer' }),
  });

  const followup = message({ role: 'assistant', turn_id: TURN });

  const rows = buildTranscriptRows(
    session({
      messages: [firstTurn, opening, answer, steer, followup],
      turns: [
        { id: 'turn-0', status: 'done' } as never,
        { id: TURN, status: 'done' } as never,
      ],
    }),
    new Set(),
    new TranscriptMarkdownCache(),
  );

  const openingRow = rows.find((row) => row.kind === 'bossTrigger' && (row as TranscriptRow & { group: { boundary: string } }).group.boundary === 'opening');
  const steerRow = rows.find((row) => row.kind === 'bossTrigger' && (row as TranscriptRow & { group: { boundary: string } }).group.boundary === 'steer');
  expect(openingRow?.topGap).toBeGreaterThanOrEqual(16);
  expect(steerRow?.topGap).toBeLessThan(openingRow!.topGap);
});
