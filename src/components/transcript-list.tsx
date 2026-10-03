import type { AgentSession } from '@waku/client';
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'octane';

import { Absolute, Pressable, ScrollView, Text, View } from '@octane-xplat/ui';

import { useMarkdownStyles, VeilRegistry } from '@/components/md-block-row';
import { RowAnchorProvider } from '@/components/transcript-anchor';
import {
  SessionEmpty,
  TranscriptRowView,
  WorkingStrip,
} from '@/components/transcript-rows';

import { useTheme } from '@/lib/use-theme';
import {
  buildTranscriptPipeline,
  expandTranscriptRows,
  stabilizeTranscriptRows,
  type TranscriptRow,
} from '@/lib/session-presentation';

import {
  extendedWindowStart,
  initialWindowStart,
} from '@/lib/transcript-scroll';

import type { MarkdownStyles } from '@/md/render';
import { TranscriptMarkdownCache } from '@/md/transcript-cache';

/** Content column cap (iPad). */
const MAX_CONTENT_WIDTH = 736;

export interface TranscriptListHandle {
  scrollToLatest: (animated?: boolean) => void;
  /** A message the reader just sent is about to land: follow it to the
   * bottom even if they had scrolled up into history. */
  followNextGrowth: () => void;
  /** Scroll a row's visual top to just under the floating header. Expands
   * its turn's fold and mounts enough history for the row to exist.
   * Returns whether the key resolves to a row at all. */
  revealRow: (rowKey: string, turnId?: string | null) => boolean;
}

export interface TranscriptDevSample {
  at: number;
  offset: number;
  contentHeight: number;
  touching: boolean;
  source: 'scroll' | 'size';
}

/**
 * The transcript.
 *
 * FLAGGED FRAMEWORK GAP: the reference is an inverted ScrollView with
 * `maintainVisibleContentPosition` — offset 0 is the visual bottom, so the
 * streaming tail is pinned by layout, not scroll commands. xplat's
 * ScrollView exposes no scroll events, no imperative scrollTo, and no
 * content-position anchor, so none of that engine can be expressed here.
 * This build renders rows in natural order inside a ScrollView and degrades
 * the follow/jump/reveal machinery to no-ops; the "earlier" control replaces
 * scroll-triggered window extension. Restoring the engine needs scroll
 * primitives (offset, size, scrollTo, anchor) in the framework.
 */
export function TranscriptList({
  onHandle,
  session,
  hydrated,
  running,
  headerInset,
  rewindTurns,
  forkTurns,
  onRewindTurn,
  onForkTurn,
}: {
  onHandle?: (handle: TranscriptListHandle) => void;
  session: AgentSession;
  /** False while the session is a task-list skeleton awaiting hydration. */
  hydrated: boolean;
  running: boolean;
  headerInset: number;
  /** Turn ids where the per-turn affordances are eligible (daemon-owned
   * semantics, resolved by the session screen). */
  rewindTurns?: ReadonlySet<string>;
  forkTurns?: ReadonlySet<string>;
  onRewindTurn?: (turnId: string) => void;
  onForkTurn?: (turnId: string) => void;
}) {
  const theme = useTheme();
  const markdownStyles = useMarkdownStyles();

  // Parse + veil state live as long as this list does (keyed per session).
  const [md] = useState(() => new TranscriptMarkdownCache());
  const [veils] = useState(() => new VeilRegistry());

  const [expandedFolds, setExpandedFolds] = useState<ReadonlySet<string>>(() => new Set());

  const runningTurnId = session.turns.find((turn) => turn.status === 'running')?.id ?? null;
  void runningTurnId;
  const pipeline = useMemo(
    () => buildTranscriptPipeline(session, expandedFolds),
    [expandedFolds, session],
  );

  // The mounted window: pipeline rows from `windowStart` to the end. Fixed
  // once the real transcript is known so streaming never shifts it, and only
  // ever grows toward older history.
  const [windowStart, setWindowStart] = useState<number | null>(
    () => hydrated ? initialWindowStart(pipeline.length) : null,
  );

  if (hydrated && windowStart === null) {setWindowStart(initialWindowStart(pipeline.length));}
  const start = Math.min(windowStart ?? initialWindowStart(pipeline.length), pipeline.length);
  const hasEarlier = start > 0;

  const freshRows = useMemo(() => expandTranscriptRows(pipeline, md, start), [md, pipeline, start]);
  const rows = useStableRows(freshRows);

  // Messages already present when this list first rendered (or hydrated)
  // show their markdown at full opacity — only content streamed while
  // watching dissolves in.
  const [seeded, setSeeded] = useState<{ phase: boolean; ids: ReadonlySet<string> }>(() => ({
    phase: hydrated,
    ids: new Set(session.messages.map((message) => message.id)),
  }));

  if (seeded.phase !== hydrated) {
    const ids = new Set(seeded.ids);
    for (const message of session.messages) {ids.add(message.id);}
    setSeeded({ phase: hydrated, ids });
  }

  const toggleFold = useCallback((turnId: string) => {
    setExpandedFolds((current) => {
      const next = new Set(current);
      if (next.has(turnId)) {next.delete(turnId);}
      else {next.add(turnId);}

      return next;
    });
  }, []);

  const revealRow = useCallback((rowKey: string, turnId?: string | null) => {
    // Unfold + mount history so the row exists. Without scroll primitives the
    // reveal can't move the viewport — the row is at least rendered.
    if (turnId) {
      setExpandedFolds((current) => (current.has(turnId) ? current : new Set(current).add(turnId)));
    }

    if (!rows.some((row) => row.key === rowKey)) {
      const pipelineIndex = pipeline.findIndex((row) => {
        if (row.kind !== 'message') {return row.key === rowKey;}
        const id = row.message.id;
        return rowKey === `user:${id}` || rowKey === `system:${id}` || rowKey.startsWith(`md:${id}.`);
      });

      if (pipelineIndex < 0) {
        if (turnId == null) {return false;}
        const foldIndex = pipeline.findIndex((row) => row.turnId === turnId);
        if (foldIndex < 0) {return false;}
        if (foldIndex < start) {
          setWindowStart(foldIndex);
        }

        return true;
      }

      if (pipelineIndex < start) {
        setWindowStart(pipelineIndex);
      }
    }

    return true;
  }, [pipeline, rows, start]);

  const scrollToLatest = useCallback((_animated = true) => {
    // No-op: no imperative scroll on xplat ScrollView (see header comment).
  }, []);

  const followNextGrowth = useCallback(() => {
    // No-op (same gap).
  }, []);

  useEffect(() => {
    onHandle?.({ scrollToLatest, followNextGrowth, revealRow });
  }, [followNextGrowth, onHandle, revealRow, scrollToLatest]);

  const showEmpty = hydrated && !running && rows.length === 0;

  return (
    <View style={{ flex: 1, overflow: 'hidden', paddingTop: headerInset }}>
      <ScrollView style={{ flex: 1 }}>
        <View style={{ paddingHorizontal: 16, paddingBottom: 12 }}>
          {hasEarlier && (
            <Pressable
              accessibilityLabel="Load earlier messages"
              accessibilityRole="button"
              onPress={() => setWindowStart((value) => extendedWindowStart(value ?? start))}
              style={{ alignItems: 'center', paddingVertical: 10 }}>
              <Text style={{ fontSize: 13, fontWeight: '600', color: theme.textTertiary }}>
                Earlier messages…
              </Text>
            </Pressable>
          )}
          {rows.map((row) => (
            <TranscriptRowFrame
              forkTurns={forkTurns}
              keepRowTop={(apply) => apply()}
              markdownStyles={markdownStyles}
              md={md}
              rewindTurns={rewindTurns}
              row={row}
              seeded={row.kind === 'md' && seeded.ids.has(row.messageId)}
              veils={veils}
              onForkTurn={onForkTurn}
              onRewindTurn={onRewindTurn}
              onToggleFold={toggleFold}
            />
          ))}
          {running ? <WorkingStrip session={session} /> : null}
        </View>
      </ScrollView>
      {showEmpty && (
        <Absolute
          style={{ bottom: 0, left: 0, right: 0, top: 0, alignItems: 'center', justifyContent: 'center' }}>
          <SessionEmpty error={null} loading={false} missing={false} />
        </Absolute>
      )}
    </View>
  );
}

/** Reuse the previous render's row objects wherever nothing changed so the
 * memoized row views bail out. */
function useStableRows(fresh: TranscriptRow[]): TranscriptRow[] {
  const previous = useRef<TranscriptRow[]>([]);
  const rows = useMemo(() => stabilizeTranscriptRows(previous.current, fresh), [fresh]);
  useEffect(() => {
    previous.current = rows;
  }, [rows]);

  return rows;
}

/** One transcript row. */
const TranscriptRowFrame = memo(function TranscriptRowFrame({
  row,
  keepRowTop,
  md,
  veils,
  seeded,
  markdownStyles,
  rewindTurns,
  forkTurns,
  onToggleFold,
  onRewindTurn,
  onForkTurn,
}: {
  row: TranscriptRow;
  keepRowTop: (apply: () => void) => void;
  md: TranscriptMarkdownCache;
  veils: VeilRegistry;
  seeded: boolean;
  markdownStyles: MarkdownStyles;
  rewindTurns?: ReadonlySet<string>;
  forkTurns?: ReadonlySet<string>;
  onToggleFold: (turnId: string) => void;
  onRewindTurn?: (turnId: string) => void;
  onForkTurn?: (turnId: string) => void;
}) {
  return (
    <View
      style={{ alignSelf: 'center', maxWidth: MAX_CONTENT_WIDTH, width: '100%', paddingTop: row.topGap }}>
      <RowAnchorProvider value={keepRowTop}>
        <TranscriptRowView
          forkTurns={forkTurns}
          markdownStyles={markdownStyles}
          md={md}
          rewindTurns={rewindTurns}
          row={row}
          seeded={seeded}
          veils={veils}
          onForkTurn={onForkTurn}
          onRewindTurn={onRewindTurn}
          onToggleFold={onToggleFold}
        />
      </RowAnchorProvider>
    </View>
  );
});
