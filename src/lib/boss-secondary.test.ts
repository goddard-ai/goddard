import { describe, expect, test } from 'bun:test';
import type { AgentSession, BossBrowseRequest } from '@waku/client';
import { activityHeaderTitle } from '@waku/client/transcript-presentation';
import { buildTranscriptPipeline } from './session-presentation';
import { bossSurfaceSessionId, browseReady, LiveRequestWindow, safeBrowseRequest, spokenBlock } from './boss-secondary';

const browse: BossBrowseRequest = { requestId: 'r', sessionId: 'boss', url: 'https://example.com', title: 'Docs' };

describe('Boss secondary surface policy', () => {
  test('browser handoff requires the owning boss on screen and foreground', () => {
    expect(browseReady(browse, 'employee', 'boss', true)).toBe(false);
    expect(browseReady(browse, 'boss', 'other-boss', true)).toBe(false);
    expect(browseReady(browse, 'boss', 'boss', false)).toBe(false);
    expect(browseReady(browse, 'boss', 'boss', true)).toBe(true);
  });

  test('browser handoff accepts only HTTP(S)', () => {
    for (const url of ['file:///tmp/private', 'javascript:alert(1)', 'data:text/html,test', 'not a URL']) {
      expect(safeBrowseRequest({ ...browse, url })).toBe(false);
    }

    expect(safeBrowseRequest(browse)).toBe(true);
  });

  test('request deduplication survives reconnect and isolates daemons', () => {
    const seen = new LiveRequestWindow(2);
    expect(seen.accept('host-a', 'r')).toBe(true);
    expect(seen.accept('host-a', 'r')).toBe(false);
    expect(seen.accept('host-b', 'r')).toBe(true);
    expect(seen.accept('host-a', 'next')).toBe(true);
    expect(seen.accept('host-a', 'r')).toBe(true);
  });

  test('spoken receipt retains fragments and a stable request identity even without playback', () => {
    const session = { messages: [{}, {}], turns: [{ id: 'turn', status: 'running' }] } as AgentSession;
    const block = spokenBlock(session, { requestId: 'speech-1', parts: ['Hello', 'Alec'] });
    expect(block.after_message).toBe(2);
    expect(block.turn_id).toBe('turn');
    expect(block.content.kind).toBe('activities');
    if (block.content.kind !== 'activities') {throw new Error('Expected activities');}
    expect(block.content.data[0]).toMatchObject({ id: 'speech-1', title: 'Spoken aloud', detail: 'Hello Alec', complete: true });
    expect(activityHeaderTitle(block.content.data, false)).toBe('Spoken aloud');
  });
});

test('spoken receipt remains visible after its turn settles and work folds', () => {
  const current = {
    messages: [
      { id: 'user', turn_id: 'turn', role: 'user', content: 'Go', created_at: 1, streaming: false },
      { id: 'answer', turn_id: 'turn', role: 'assistant', content: 'Done', created_at: 2, streaming: false },
    ],
    turns: [{ id: 'turn', status: 'completed', started_at: 1, completed_at: 2 }],
    transcript_blocks: [],
  } as unknown as AgentSession;

  const block = spokenBlock(current, { requestId: 'receipt', parts: ['Done'] });
  block.turn_id = 'turn';
  current.transcript_blocks = [block];
  const rows = buildTranscriptPipeline(current);
  expect(rows.some((row) => row.kind === 'activities' && row.block === block)).toBe(true);
});

test('the Boss home route counts as the visible chat while secondary routes do not', () => {
  expect(bossSurfaceSessionId('index', undefined, 'boss')).toBe('boss');
  expect(bossSurfaceSessionId('session/:id', 'employee', 'boss')).toBe('employee');
  expect(bossSurfaceSessionId('new-task', undefined, 'boss')).toBeNull();
  expect(bossSurfaceSessionId('index', undefined, null)).toBeNull();
});
