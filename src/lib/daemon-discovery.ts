import type { DiscoveredDaemon } from './daemon-discovery.web';

export type { DiscoveredDaemon };

// TODO(xplat-blocker): mDNS/Bonjour discovery. The reference app used
// react-native-zeroconf; the NativeScript equivalents are unmaintained
// (nativescript-zeroconf 1.x predates NS8). Options: write an NSNetServiceBrowser
// / NsdManager wrapper, or contribute a plugin. Until then discovery returns
// nothing and users enter daemon addresses manually.
export function subscribeDaemonDiscovery(
  _onChange: (daemons: DiscoveredDaemon[]) => void,
): () => void {
  return () => {};
}
