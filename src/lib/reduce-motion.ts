declare function UIAccessibilityIsReduceMotionEnabled(): boolean;

/** iOS reduce-motion check; Android animator-duration check TODO. */
export function isReduceMotionEnabled(): boolean {
  try {
    return UIAccessibilityIsReduceMotionEnabled();
  } catch {
    return false;
  }
}

/** Live change notifications aren't wired yet — return the current value. */
export function subscribeReduceMotion(_notify: (value: boolean) => void): () => void {
  return () => {};
}
