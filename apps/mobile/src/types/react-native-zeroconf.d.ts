declare module 'react-native-zeroconf' {
  export interface ZeroconfService {
    name: string;
    fullName?: string;
    host?: string;
    port?: number;
    addresses?: string[];
    txt?: Record<string, string>;
  }

  export default class Zeroconf {
    constructor();
    scan(type?: string, protocol?: string, domain?: string, implType?: string): void;
    stop(implType?: string): void;
    getServices(): Record<string, ZeroconfService>;
    publishService(
      type: string,
      protocol: string,
      domain: string,
      name: string,
      port: number,
      txt?: Record<string, string>,
      implType?: string,
    ): void;
    unpublishService(name: string, implType?: string): void;
    addDeviceListeners(): void;
    removeDeviceListeners(): void;
    on(event: 'resolved' | 'published' | 'unpublished', listener: (service: ZeroconfService) => void): void;
    on(event: 'found' | 'remove', listener: (name: string) => void): void;
    on(event: 'start' | 'stop' | 'update', listener: () => void): void;
    on(event: 'error', listener: (error: Error) => void): void;
  }
}
