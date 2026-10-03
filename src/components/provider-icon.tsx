/** @jsxImportSource @nativescript-community/octane */
import type { ProviderKind } from '@waku/client';
import { useMemo } from 'octane';
import { Icon, registerIcons, View } from '@octane-xplat/ui';

import { PROVIDER_ICON_XML } from './provider-icons';
import { useTheme } from '@/lib/use-theme';

/** Marks with a real brand color; monochrome brands (OpenAI, Cursor, Grok,
 * OpenCode, Fx, Pi) tint with the theme text color, and Oh My Pi's SVG
 * carries its own gradient. */
const PROVIDER_BRAND_COLORS: Partial<Record<ProviderKind, string>> = {
  claude: '#d97757',
  deepSeek: '#4d6bfe',
  amp: '#f34e3f',
};

/** Brand color for a provider mark, when the brand has one. */
export function providerBrandColor(provider: ProviderKind): string | undefined {
  return PROVIDER_BRAND_COLORS[provider];
}

const registered = new Set<ProviderKind>();

function glyphFor(provider: ProviderKind, tint: string) {
  const key = `provider-${provider}`;
  if (!registered.has(provider)) {
    registered.add(provider);
    registerIcons({ [key]: { markup: stripSvgShell(PROVIDER_ICON_XML[provider]) } });
  }
  return key;
}

function stripSvgShell(xml: string): string {
  return xml
    .replace(/^<svg[^>]*>/, '')
    .replace(/<\/svg>\s*$/, '')
    .trim();
}

/** Brand mark for an agent provider. TODO(verify): the shared Icon svgview
 * path may not support the ohMyPi linearGradient — check on device. */
export function ProviderIcon({
  provider,
  size = 18,
  color,
}: {
  provider: ProviderKind;
  size?: number;
  color?: string;
}) {
  const theme = useTheme();
  const tint = color ?? PROVIDER_BRAND_COLORS[provider] ?? theme.text;
  const name = useMemo(() => glyphFor(provider, tint), [provider, tint]);
  return (
    <View style={{ height: size, width: size }}>
      <Icon name={name} size={size} color={tint} />
    </View>
  );
}
