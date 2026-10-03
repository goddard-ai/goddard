// Learn more: https://docs.expo.dev/guides/customizing-metro/
const { getDefaultConfig } = require('expo/metro-config');
const path = require('path');

const config = getDefaultConfig(__dirname);

// `bun run go` bundles for Expo Go, which lacks the native code for these
// modules. Swap them for JS stubs so their native registration never runs:
// terminals show a placeholder, composer paste falls back to plain TextInput,
// and LAN daemon discovery simply finds nothing.
const expoGo = process.env.GODDARD_EXPO_GO === '1';

// Fold the mode into Metro's cache key so each mode keeps its own warm
// on-disk cache — without this, a cache populated by `bun start` could
// serve the real modules' resolution results to `bun run go`.
config.cacheVersion = `${config.cacheVersion ?? 'default'}-${expoGo ? 'expogo' : 'dev'}`;

const EXPO_GO_STUBS = {
  'expo-libghostty': './src/expo-go-stubs/expo-libghostty.tsx',
  'react-native-zeroconf': './src/expo-go-stubs/react-native-zeroconf.ts',
  '@mattermost/react-native-paste-input':
    './src/expo-go-stubs/react-native-paste-input.tsx',
};

if (expoGo) {
  const defaultResolveRequest = config.resolver.resolveRequest;
  config.resolver.resolveRequest = (context, moduleName, platform) => {
    const stub = EXPO_GO_STUBS[moduleName];
    if (stub) {
      return {
        type: 'sourceFile',
        filePath: path.resolve(__dirname, stub),
      };
    }
    if (defaultResolveRequest) {
      return defaultResolveRequest(context, moduleName, platform);
    }
    return context.resolveRequest(context, moduleName, platform);
  };
}

module.exports = config;
