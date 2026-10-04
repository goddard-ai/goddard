import type { ReactNode } from 'octane';

import type { TranscriptRow } from '@/lib/session-presentation';
import type { KeepRowTop } from './transcript-anchor';

/** Per-row hooks the scroll leaf hands to the shared row renderer. */
export interface TranscriptRowScrollHelpers {
  /** Wrap a row-local layout mutation (fold/disclosure) so the row's visual
   * top edge stays put across the change. */
  keepRowTop: KeepRowTop;
}

/** Imperative surface the leaf exposes to TranscriptList. */
export interface TranscriptScrollHandle {
  scrollToLatest: (animated?: boolean) => void;
  /** Follow the next content growth to the bottom (the reader just sent a
   * message). */
  followNextGrowth: () => void;
  /** Scroll a mounted row's visual top to just under the floating header.
   * Rows that mount later (window extension, unfold) are picked up when
   * their layout lands. */
  revealRow: (rowKey: string) => void;
}

export interface TranscriptScrollViewProps {
  /** Mounted window of pipeline rows, oldest → newest. */
  rows: readonly TranscriptRow[];
  renderRow: (row: TranscriptRow, helpers: TranscriptRowScrollHelpers) => ReactNode;
  /** Working strip pinned to the visual bottom (null when idle). */
  strip?: ReactNode;
  hasEarlier: boolean;
  /** Native: fired by scroll-triggered window extension. Web: the "Earlier
   * messages" control. */
  onExtendTail: () => void;
  onScrollHandle?: (handle: TranscriptScrollHandle | null) => void;
  headerInset: number;
}
