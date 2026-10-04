import type { XtermAssets } from './terminal-assets.web';

// `?raw` imports of node_modules are not reachable on native: the dev
// server's per-module loader strips query suffixes, and vendoring the
// packages eagerly evaluates xterm's DOM globals at bootstrap. Until the
// assets ship inside the app bundle, the native terminal leaf renders an
// explanatory page instead of a working xterm session.
export const xtermAssets: XtermAssets | null = null;
