import { forwardRef, useImperativeHandle } from 'react';
import {
  StyleSheet,
  Text,
  View,
  type StyleProp,
  type ViewStyle,
} from 'react-native';

/** Expo Go stub for expo-libghostty: the ghostty native module is not
 * bundled in Expo Go, so terminal surfaces render a placeholder and the
 * ref methods resolve without doing anything. Selected by metro.config.js
 * when GODDARD_EXPO_GO=1. */
export interface TerminalViewRef {
  write(data: string): Promise<void>;
  finish(code: number): Promise<void>;
}

export const TerminalView = forwardRef<
  TerminalViewRef,
  { style?: StyleProp<ViewStyle> }
>(function TerminalView({ style }, ref) {
  useImperativeHandle(
    ref,
    () => ({
      write: () => Promise.resolve(),
      finish: () => Promise.resolve(),
    }),
    [],
  );
  return (
    <View style={[styles.stub, style]}>
      <Text style={styles.text}>
        Terminal requires a development build
      </Text>
    </View>
  );
});

const styles = StyleSheet.create({
  stub: {
    alignItems: 'center',
    backgroundColor: '#1a1a1a',
    justifyContent: 'center',
  },
  text: {
    color: '#888',
    fontSize: 13,
  },
});
