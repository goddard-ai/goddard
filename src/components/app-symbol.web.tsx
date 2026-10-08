import { Icon, registerIcons } from '@octane-xplat/ui';
import type { AppSymbolName } from './app-symbol.types';

export type { AppSymbolName };

/** Minimal web icon stand-ins for the SF Symbols the ported screens use so
 * far; extend as more screens port. SF Symbols stay the iOS reference. */
const FALLBACK_TEXT: Record<string, string> = {
  plus: '+',
  checkmark: '✓',
  'info.circle': 'ⓘ',
  'lock.shield': '🔒',
  'qrcode.viewfinder': '▣',
  'chevron.left': '‹',
  'chevron.right': '›',
  'chevron.down': '⌄',
  'chevron.up': '⌃',
  ellipsis: '…',
  magnifyingglass: '⌕',
  'xmark': '×',
  bell: '🔔',
  gearshape: '⚙',
  house: '⌂',
  'square.and.pencil': '✎',
  'sidebar.left': '◧',
  sparkle: '✦',
  sparkles: '✦',
  terminal: '▸_',
  'doc.text': '📄',
  'externaldrive': '▤',
  globe: '🌐',
  target: '◎',
  hourglass: '⧖',
  'exclamationmark.triangle': '⚠',
  pause: '⏸',
  'arrow.triangle.2.circlepath': '⟳',
  circle: '○',
  'circle.inset.filled': '◐',
  'xmark.circle': '⊘',
  'list.bullet.clipboard': '☰',
  'arrow.triangle.branch': '⎇',
  folder: '📁',
};

const registered = new Set<string>();

export function AppSymbol({
  name,
  size = 18,
  tintColor,
}: {
  name: AppSymbolName | string;
  size?: number;
  tintColor?: string;
}) {
  const symbol = typeof name === 'string' ? name : (name.web ?? name.ios);
  const key = `sf-${symbol}`;
  if (!registered.has(key)) {
    registered.add(key);
    registerIcons({ [key]: { text: FALLBACK_TEXT[symbol] ?? '•' } });
  }

  return <Icon name={key} size={size} color={tintColor} />;
}
