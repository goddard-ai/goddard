import { Color } from '@nativescript/core';

declare const UIColor: { systemBlueColor: unknown } | undefined;

/** System tint for interactive affordances (back rows, checkmarks, active
 * toggles, text carets) so controls read native instead of branded. */
export const NativeTint: Color | string =
  typeof UIColor !== 'undefined' ? Color.fromIosColor(UIColor.systemBlueColor as never) : '#3b82f6';

export const Fonts = {
  /** iOS `UIFontDescriptorSystemDesignDefault` */
  sans: 'system-ui',
  /** iOS `UIFontDescriptorSystemDesignSerif` */
  serif: 'ui-serif',
  /** iOS `UIFontDescriptorSystemDesignRounded` */
  rounded: 'ui-rounded',
  /** iOS `UIFontDescriptorSystemDesignMonospaced` */
  mono: 'ui-monospace',
} as const;

export const MonoFont = 'Menlo';
