import type { AgentSession } from '@waku/client';
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'octane';

import { Absolute, View } from '@octane-xplat/ui';

import { useMarkdownStyles, VeilRegistry } from '@/components/md-block-row';
import { RowAnchorProvider } from '@/components/transcript-anchor';
import { TranscriptScrollView } from '@/components/transcript-scroll-view';
import type { TranscriptScrollHandle } from '@/components/transcript-scroll-view.types';
import {
  SessionEmpty,
  TranscriptRowView,
  WorkingStrip,
} from '@/components/transcript-rows';

import { dismissKeyboard } from '@/lib/keyboard';
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

import type { MarkdownStyles } from '@/md/render.tsrx';
import { TranscriptMarkdownCache } from '@/md/transcript-cache';
import type { KeepRowTop } from '@/components/transcript-anchor';

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
 * The scroll engine lives in the TranscriptScrollView leaf: on native it is
 * the inverted-scroll port of the reference engine (scaleY -1 scrollview,
 * bottom-anchored streaming, jump-to-latest, scroll-triggered history
 * extension); on web it keeps the original natural-order scroller with a
 * manual "Earlier messages" control. This file owns what both share — the
 * pipeline, the mounted window, folds, seeded reveals — plus the imperative
 * handle plumbing.
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
  const markdownStyles = useMarkdownStyles();
  const scrollHandle = useRef<TranscriptScrollHandle | null>(null);

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
  // ever grows toward older history — new rows mount at the native end where
  // they cannot move the offset.
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

  const extendTail = useCallback(() => {
    setWindowStart((value) => extendedWindowStart(value ?? start));
  }, [start]);

  const revealRow = useCallback((rowKey: string, turnId?: string | null) => {
    // Unfold + mount history so the row exists; the leaf then scrolls it to
    // just under the header once its layout lands.
    dismissKeyboard();
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
        // Folded-away rows are absent until the expansion lands; reach the
        // turn's position so a deep match still mounts its history.
        if (turnId == null) {return false;}
        const foldIndex = pipeline.findIndex((row) => row.turnId === turnId);
        if (foldIndex < 0) {return false;}
        if (foldIndex < start) {
          setWindowStart((value) => Math.min(value ?? foldIndex, foldIndex));
        }

        scrollHandle.current?.revealRow(rowKey);
        return true;
      }

      if (pipelineIndex < start) {
        setWindowStart((value) => Math.min(value ?? pipelineIndex, pipelineIndex));
      }
    }

    scrollHandle.current?.revealRow(rowKey);
    return true;
  }, [pipeline, rows, start]);

  const scrollToLatest = useCallback((animated = true) => {
    scrollHandle.current?.scrollToLatest(animated);
  }, []);

  const followNextGrowth = useCallback(() => {
    scrollHandle.current?.followNextGrowth();
  }, []);

  useEffect(() => {
    onHandle?.({ scrollToLatest, followNextGrowth, revealRow });
  }, [followNextGrowth, onHandle, revealRow, scrollToLatest]);

  const showEmpty = hydrated && !running && rows.length === 0;

  return (
    <View style={{ flex: 1, overflow: 'hidden', paddingTop: headerInset }}>
      <TranscriptScrollView
        hasEarlier={hasEarlier}
        headerInset={headerInset}
        rows={rows}
        strip={running ? <WorkingStrip session={session} /> : null}
        renderRow={(row, helpers) => (
          <TranscriptRowFrame
            forkTurns={forkTurns}
            keepRowTop={helpers.keepRowTop}
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
        )}
        onExtendTail={extendTail}
        onScrollHandle={(handle) => { scrollHandle.current = handle; }}
      />
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
  keepRowTop: KeepRowTop;
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
