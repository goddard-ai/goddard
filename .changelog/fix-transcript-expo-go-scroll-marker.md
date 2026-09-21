- Fix a crash opening a task in the mobile app under Expo Go: the transcript's
  scroll-edge-effect marker isn't in Expo Go's bundled react-native-screens, so
  mounting it threw inside createNode — it now falls back to a plain wrapper
  there and keeps the native marker in development builds
