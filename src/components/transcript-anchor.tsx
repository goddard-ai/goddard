import { createContext, useContext } from 'octane';

/**
 * Keep a row's top edge fixed on screen across a local layout change.
 *
 * In the inverted transcript a row's fixed native edge is its visual bottom,
 * so a disclosure that expands in place would grow *upward* and carry the
 * tapped header away. The scroll leaf answers by compensating the native
 * offset for exactly the commit that applies the change, which makes the
 * expansion grow downward the way every iOS disclosure does.
 */
export type KeepRowTop = (apply: () => void) => void;

// Loose typing: the DOM Context signature isn't a JSX component type and the
// universal runtime's is its own callable — both resolve the same context
// object at runtime (same pattern as upstream Tabs/Text leaves).
const RowAnchorContext: any = createContext<KeepRowTop | null>(null);

export function RowAnchorProvider({ value, children }: { value: KeepRowTop; children?: unknown }) {
  return <RowAnchorContext value={value}>{children}</RowAnchorContext>;
}

/** Outside a transcript row the change simply applies immediately. */
export function useRowAnchor(): KeepRowTop {
  return useContext(RowAnchorContext) ?? applyNow;
}

function applyNow(apply: () => void) {
  apply();
}
