import type { AgentSession, BossBrowseRequest, BossSpeechRequest, TranscriptBlock } from '@waku/client';

/** Live request identities are daemon-scoped. This bounded window survives
 * reconnects, but is never used to replay disconnected speech. */
export class LiveRequestWindow {
  private seen = new Set<string>();
  constructor(private capacity = 512) {}
  accept(profileId: string, requestId: string): boolean {
    const key = JSON.stringify([profileId, requestId]);
    if (this.seen.has(key)) {return false;}
    this.seen.add(key);
    if (this.seen.size > this.capacity) {this.seen.delete(this.seen.values().next().value!);}
    return true;
  }
}

export function safeBrowseRequest(request: BossBrowseRequest): boolean {
  try {
    const url = new URL(request.url);
    return url.protocol === 'https:' || url.protocol === 'http:';
  } catch {return false;}
}

/** Boss home is a chat too; explicit task routes still retain their identity. */
export function bossSurfaceSessionId(routeName: string | undefined, routedSessionId: string | undefined, bossSessionId: string | null): string | null {
  if (routeName === 'index') {return bossSessionId;}
  return routeName === 'session/:id' ? routedSessionId ?? null : null;
}

export function browseReady(
  request: BossBrowseRequest,
  visibleSessionId: string | null,
  bossSessionId: string | null,
  foreground: boolean,
): boolean {
  return foreground && request.sessionId === bossSessionId && request.sessionId === visibleSessionId;
}

export function spokenBlock(session: AgentSession, request: BossSpeechRequest): TranscriptBlock {
  return {
    after_message: session.messages.length,
    turn_id: session.turns.at(-1)?.status === 'running' ? session.turns.at(-1)!.id : null,
    content: { kind: 'activities', data: [{
      id: request.requestId, source_id: null, kind: 'tool', title: 'Spoken aloud',
      detail: request.parts.join(' '), failed: false, complete: true,
    }] },
  };
}
