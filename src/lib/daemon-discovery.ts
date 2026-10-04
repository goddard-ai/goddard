import type { DiscoveredDaemon } from './daemon-discovery.web';

export type { DiscoveredDaemon };

// mDNS/Bonjour discovery lives in the platform leaves —
// daemon-discovery.ios.ts (NSNetServiceBrowser) and
// daemon-discovery.android.ts (NsdManager) — since no maintained
// NativeScript zeroconf plugin exists. This unsuffixed fallback keeps the
// module importable on any platform without a dedicated leaf.
export function subscribeDaemonDiscovery(
  _onChange: (daemons: DiscoveredDaemon[]) => void,
): () => void {
  return () => {};
}
