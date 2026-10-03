export function isReduceMotionEnabled(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export function subscribeReduceMotion(notify: (value: boolean) => void): () => void {
  const query = window.matchMedia('(prefers-reduced-motion: reduce)');
  const listener = () => notify(query.matches);
  query.addEventListener('change', listener);
  return () => query.removeEventListener('change', listener);
}
