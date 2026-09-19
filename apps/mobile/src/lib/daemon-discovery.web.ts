/** A daemon found on the local network through Bonjour. */
export interface DiscoveredDaemon {
  /** The daemon's share-endpoint id — correlates with its friend code. */
  id: string;
  name: string;
  /** `ws://host:port` built from the resolved address. */
  address: string;
}

/** Browsers cannot browse mDNS — the web variant discovers nothing. */
export function subscribeDaemonDiscovery(
  _onChange: (daemons: DiscoveredDaemon[]) => void,
): () => void {
  return () => {};
}
