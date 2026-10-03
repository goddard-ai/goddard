/**
 * Keep a row's top edge fixed on screen across a local layout change.
 *
 * The reference's inverted transcript pointed the native scroll anchor at
 * the tapped row's neighbour for exactly one commit so a disclosure grew
 * downward. xplat ScrollView exposes no content-position anchor, so the
 * context degenerates to apply-immediately — the disclosure still works;
 * the header may drift. Flagged with the transcript-scroll gap.
 */
export type KeepRowTop = (apply: () => void) => void;

export function RowAnchorProvider({ value: _value, children }: { value: KeepRowTop; children?: unknown }) {
  // No-op host: the scroll-anchor handshake it feeds cannot be expressed yet.
  return children;
}

export function useRowAnchor(): KeepRowTop {
  return (apply) => apply();
}
