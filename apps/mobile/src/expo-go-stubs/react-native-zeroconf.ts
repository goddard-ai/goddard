/** Expo Go stub for react-native-zeroconf: Expo Go has no mDNS native
 * module, so scanning reports nothing and daemons are added manually.
 * Selected by metro.config.js when GODDARD_EXPO_GO=1. */
export default class Zeroconf {
  on(..._args: unknown[]): void {}
  scan(..._args: unknown[]): void {}
  stop(): void {}
  removeDeviceListeners(): void {}
  getServices(): Record<string, never> {
    return {};
  }
}
