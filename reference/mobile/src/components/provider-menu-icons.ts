import type { ProviderKind } from '@waku/client';
import type { ImageSourcePropType } from 'react-native';

/** Raster copies of provider-icons.ts for native menu image sources.
 * The menu tints these marks to stay legible in either system theme. */
export const PROVIDER_MENU_ICONS: Record<ProviderKind, ImageSourcePropType> = {
  antigravity: require('@/assets/images/providers/antigravity.png'),
  amp: require('@/assets/images/providers/amp.png'),
  claude: require('@/assets/images/providers/claude.png'),
  codex: require('@/assets/images/providers/codex.png'),
  copilot: require('@/assets/images/providers/copilot.png'),
  cursor: require('@/assets/images/providers/cursor.png'),
  deepSeek: require('@/assets/images/providers/deepSeek.png'),
  devin: require('@/assets/images/providers/devin.png'),
  droid: require('@/assets/images/providers/droid.png'),
  fx: require('@/assets/images/providers/fx.png'),
  openCode: require('@/assets/images/providers/openCode.png'),
  openCode2: require('@/assets/images/providers/openCode2.png'),
  goose: require('@/assets/images/providers/goose.png'),
  grok: require('@/assets/images/providers/grok.png'),
  kimi: require('@/assets/images/providers/kimi.png'),
  muse: require('@/assets/images/providers/muse.png'),
  ohMyPi: require('@/assets/images/providers/ohMyPi.png'),
  pi: require('@/assets/images/providers/pi.png'),
};
