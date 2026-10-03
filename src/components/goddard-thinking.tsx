/** @jsxImportSource @nativescript-community/octane */
import { Icon, registerIcons } from '@octane-xplat/ui';

registerIcons({
  'goddard-thinking': {
    // Simplified static mark: the reference replayed SMIL sway/rock/blink
    // tracks per frame on react-native-svg — xplat's Icon takes markup only,
    // so the working indicator is the face at rest. Flagged for revisit.
    markup: `<g><ellipse cx="80" cy="55" rx="58" ry="50" fill="#000"/><circle cx="58" cy="48" r="7" fill="#fff"/><circle cx="102" cy="48" r="7" fill="#fff"/><path d="M70 78 q10 8 20 0" stroke="#fff" stroke-width="5" fill="none" stroke-linecap="round"/></g>`,
    viewBox: '-10 -10 160 125',
  },
});

/** The "still working" mark (static — see note above). */
export function GoddardThinking({ color }: { color?: string }) {
  return <Icon name="goddard-thinking" size={28} color={color} />;
}
