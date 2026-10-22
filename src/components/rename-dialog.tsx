import { useState } from 'octane';
import { Pressable, Text, TextInput } from '@octane-xplat/ui';

import { Sheet } from './sheet';
import { showAlert } from '@/lib/alert';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';

export interface RenameDialogProps {
  initialValue: string;
  onDismiss: () => void;
  onSubmit: (title: string) => Promise<void>;
  visible: boolean;
}

/** The iOS reference used a SwiftUI text-field alert; the shared build uses
 * a sheet with a field and a submit button. */
export function RenameDialog({ initialValue, onDismiss, onSubmit, visible }: RenameDialogProps) {
  const theme = useTheme();
  const [title, setTitle] = useState(initialValue);
  const [saving, setSaving] = useState(false);
  const [key, setKey] = useState(0);

  // Reset the field whenever the dialog re-opens.
  if (visible && title !== initialValue && !saving && key === 0) {
    setTitle(initialValue);
    setKey(1);
  }

  if (!visible && key !== 0) {setKey(0);}

  function save() {
    if (saving) {return;}
    setSaving(true);
    void onSubmit(title.trim() || initialValue)
      .then(() => {
        setSaving(false);
        onDismiss();
      })
      .catch((cause) => {
        setSaving(false);
        onDismiss();
        void showAlert(
          'Couldn\u2019t rename task',
          cause instanceof Error ? cause.message : String(cause),
        );
      });
  }

  return (
    <Sheet visible={visible} onDismiss={onDismiss} title="Rename task">
      <TextInput
        placeholder="Task title"
        returnKeyType="done"
        onSubmit={save}
        style={{
          borderRadius: Radius.medium,
          fontSize: 16,
          minHeight: 44,
          paddingLeft: 12,
          paddingRight: 12,
          marginLeft: 6,
          marginRight: 6,
          marginBottom: 8,
          color: theme.text,
          backgroundColor: theme.inset,
        }}
        value={title}
        onChange={setTitle}
      />
      <Pressable
        accessibilityRole="button"
        disabled={saving}
        onPress={save}
        style={{
          alignItems: 'center',
          borderRadius: Radius.medium,
          minHeight: 44,
          justifyContent: 'center',
          marginLeft: 6, marginRight: 6,
          backgroundColor: theme.inverse,
          opacity: saving ? 0.55 : 1,
        }}>
        <Text style={{ fontSize: 15, fontWeight: '700', color: theme.onInverse }}>
          {saving ? 'Renaming…' : 'Rename'}
        </Text>
      </Pressable>
    </Sheet>
  );
}
