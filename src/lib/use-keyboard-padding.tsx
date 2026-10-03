/**
 * The reference returned reanimated styles driven by live keyboard height.
 * On xplat, `KeyboardAvoiding` owns keyboard avoidance; these return static
 * paddings for layouts that also want the home-indicator inset preserved
 * (bottomInset when the keyboard is hidden — the lifted case is the
 * wrapper's job now).
 */
export function useKeyboardPadding() {
  return { paddingBottom: 0 };
}

/** Bottom padding for an input docked above the keyboard inside a
 * KeyboardAvoiding wrapper: keep the home-indicator inset. */
export function useComposerKeyboardPadding(bottomInset: number) {
  return { paddingBottom: Math.max(bottomInset, 10) };
}
