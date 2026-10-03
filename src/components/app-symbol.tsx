/** @jsxImportSource @nativescript-community/octane */
import { Icon, registerIcons } from '@octane-xplat/ui';
import type { AppSymbolName } from './app-symbol.types';

export type { AppSymbolName };

const registered = new Set<string>();

/** SF Symbols on iOS via the Icon glyph's `sys://` src; the web leaf maps a
 * small inline-SVG set and the android leaf material names. */
export function AppSymbol({
  name,
  size = 18,
  tintColor,
}: {
  name: AppSymbolName | string;
  size?: number;
  tintColor?: string;
}) {
  const symbol = typeof name === 'string' ? name : name.ios;
  const key = `sf-${symbol}`;
  if (!registered.has(key)) {
    registered.add(key);
    registerIcons({ [key]: { src: `sys://${symbol}` } });
  }

  return <Icon name={key} size={size} color={tintColor} />;
}
