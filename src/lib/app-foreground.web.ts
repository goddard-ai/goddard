/** Browser foreground = document visibility + page lifecycle. */
export function subscribeAppForeground(
  notify: (foreground: boolean) => void,
): () => void {
  const onVis = () => notify(document.visibilityState === 'visible');
  const onHide = () => notify(false);
  const onShow = () => notify(true);
  document.addEventListener('visibilitychange', onVis);
  window.addEventListener('pagehide', onHide);
  window.addEventListener('pageshow', onShow);
  return () => {
    document.removeEventListener('visibilitychange', onVis);
    window.removeEventListener('pagehide', onHide);
    window.removeEventListener('pageshow', onShow);
  };
}
