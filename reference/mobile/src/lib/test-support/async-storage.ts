import { mock } from 'bun:test';

export const nativeStorage = new Map<string, string>();
mock.module('@react-native-async-storage/async-storage', () => ({
  default: {
    multiGet: async (keys: string[]) => keys.map((key) => [key, nativeStorage.get(key) ?? null]),
    setItem: async (key: string, value: string) => {
      nativeStorage.set(key, value);
    },
  },
}));
