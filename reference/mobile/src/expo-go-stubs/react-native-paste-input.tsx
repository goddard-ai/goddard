import { forwardRef } from 'react';
import { TextInput, type TextInputProps } from 'react-native';

/** Expo Go stub for @mattermost/react-native-paste-input: plain TextInput
 * with no rich-paste support. Selected by metro.config.js when
 * GODDARD_EXPO_GO=1. */
const PasteInput = forwardRef<
  TextInput,
  TextInputProps & { disableCopyPaste?: boolean; onPaste?: unknown }
>(function PasteInput(
  { disableCopyPaste: _disableCopyPaste, onPaste: _onPaste, ...props },
  ref,
) {
  return <TextInput ref={ref} {...props} />;
});

export default PasteInput;
