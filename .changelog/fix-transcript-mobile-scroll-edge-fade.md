- Stop the mobile transcript's top-edge fade in Expo Go too: iOS 26's scroll
  edge effect made the transcript look washed out the moment it became
  scrollable — the marker now mounts whenever the bundled react-native-screens
  actually ships it instead of being skipped in Expo Go outright, and the
  session screen also suppresses the effect through its own option
