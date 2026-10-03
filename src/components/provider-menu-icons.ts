import type { ProviderKind } from '@waku/client';

/** Raster copies of provider-icons for menu rows. The reference bundled png
 * assets; here the glyph registry keeps a text fallback. */
export const PROVIDER_MENU_ICONS: Partial<Record<ProviderKind, string>> = {
  claude: 'claude',
  codex: 'codex',
  cursor: 'cursor',
  devin: 'devin',
};
