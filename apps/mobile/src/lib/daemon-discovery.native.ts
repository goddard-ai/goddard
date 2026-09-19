import Zeroconf from 'react-native-zeroconf';

import type { DiscoveredDaemon } from './daemon-discovery.web';

export type { DiscoveredDaemon };

const SERVICE_TYPE = 'waku';

interface ZeroconfService {
  name: string;
  host?: string;
  port?: number;
  addresses?: string[];
  txt?: Record<string, string>;
}

/** Browse `_waku._tcp` for daemons advertising on the LAN. `onChange`
 * receives the whole current set each time a service resolves or
 * disappears; the returned function stops scanning. */
export function subscribeDaemonDiscovery(
  onChange: (daemons: DiscoveredDaemon[]) => void,
): () => void {
  const zeroconf = new Zeroconf();
  const emit = () => {
    const services = zeroconf.getServices() as Record<string, ZeroconfService>;
    const daemons = Object.values(services)
      .map((service) => {
        const port = service.port;
        const host = service.addresses?.find((address) =>
          /^\d+\.\d+\.\d+\.\d+$/.test(address),
        ) ?? service.addresses?.[0] ?? service.host;
        if (!port || !host) return null;
        return {
          id: service.txt?.id ?? service.name,
          name: service.txt?.name || service.name,
          address: `ws://${host}:${port}`,
        } satisfies DiscoveredDaemon;
      })
      .filter((daemon): daemon is DiscoveredDaemon => daemon !== null);
    onChange(daemons);
  };
  zeroconf.on('resolved', emit);
  zeroconf.on('remove', emit);
  zeroconf.scan(SERVICE_TYPE, 'tcp', 'local.');
  return () => {
    zeroconf.stop();
    zeroconf.removeDeviceListeners();
  };
}
