import { Haptics } from '@/lib/haptics';
import { useMemo, useRef, useState } from 'octane';
import {
  ActivityIndicator,
  Row,
  Pressable,
  Text,
  TextInput,
  View,
  pushRoute,
} from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { DaemonList } from '@/components/daemon-list';
import { Sheet, SheetRow } from '@/components/sheet';
import { confirmDestructive } from '@/lib/alert';
import { NativeTint } from '@/lib/theme-platform';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import { useDaemon } from '@/lib/daemon-hooks';
import {
  isPrivateDaemonAddress,
  normalizeDaemonAddress,
  type DaemonProfile,
} from '@/lib/daemon-profile';

type EditorTarget = string | null | undefined;
type ConnectionSecurity = 'secure' | 'private' | 'insecure' | 'invalid' | null;

export function DaemonPickerSheet({
  onDismiss,
  visible,
}: {
  onDismiss: () => void;
  visible: boolean;
}) {
  const daemon = useDaemon();
  const [editorTarget, setEditorTarget] = useState<EditorTarget>(undefined);
  const [editorProfile, setEditorProfile] = useState<DaemonProfile | undefined>(undefined);
  const wasVisible = useRef(false);

  // Reset to the list page each time the sheet opens; first run (no
  // profiles) drops straight into the editor like the reference.
  if (visible !== wasVisible.current) {
    wasVisible.current = visible;
    if (visible) {
      setEditorTarget(daemon.profiles.length === 0 ? null : undefined);
      setEditorProfile(undefined);
    }
  }

  function pushEditor(target: string | null) {
    setEditorTarget(target);
    setEditorProfile(
      typeof target === 'string'
        ? daemon.profiles.find((item) => item.id === target)
        : undefined,
    );
  }

  function popEditor() {
    setEditorTarget(undefined);
    setEditorProfile(undefined);
  }

  function handleDismiss() {
    setEditorTarget(undefined);
    setEditorProfile(undefined);
    onDismiss();
  }

  function select(profileToSelect: DaemonProfile) {
    onDismiss();
    if (profileToSelect.id === daemon.activeProfile?.id) {return;}
    void Haptics.selectionAsync();
    void daemon.selectProfile(profileToSelect.id).then((connected) => {
      if (!connected) {
        void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
      }
    });
  }

  return (
    <Sheet onDismiss={handleDismiss} visible={visible} scrollable>
      {editorTarget === undefined ? (
        <View>
          <SheetPageHeader title="Daemons" />
          <DaemonList
            onEdit={(item) => pushEditor(item.id)}
            onSelect={select}
          />
          <SheetRow
            description="Connect another host"
            label="Add Daemon…"
            leading={(
              <AppSymbol
                name={{ ios: 'plus.circle', android: 'add_circle', web: 'add_circle' }}
                size={22}
                tintColor={String(NativeTint)}
              />
            )}
            onPress={() => pushEditor(null)}
          />
          <SheetRow
            description="The code in Settings → Daemon on the host"
            label="Scan QR Code…"
            leading={(
              <AppSymbol
                name={{
                  ios: 'qrcode.viewfinder',
                  android: 'qr_code_scanner',
                  web: 'qr_code_scanner',
                }}
                size={22}
                tintColor={String(NativeTint)}
              />
            )}
            onPress={() => {
              handleDismiss();
              pushRoute({ stack: 'root', name: 'daemon-scan', params: {} });
            }}
          />
        </View>
      ) : (
        <DaemonEditor
          profile={editorProfile}
          onBack={popEditor}
          onRemoved={popEditor}
          onSaved={popEditor}
        />
      )}
    </Sheet>
  );
}

export function DaemonEditor({
  onBack,
  onRemoved,
  onSaved,
  profile,
}: {
  onBack?: () => void;
  onRemoved?: () => void;
  onSaved?: () => void;
  profile?: DaemonProfile;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const [name, setName] = useState(profile?.name ?? '');
  const [address, setAddress] = useState(profile?.address ?? '');
  const [token, setToken] = useState('');
  const [revealed, setRevealed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const addressInput = useRef<any>(null);
  const tokenInput = useRef<any>(null);
  const security = useMemo<ConnectionSecurity>(() => {
    if (!address.trim()) {return null;}
    try {
      const normalized = normalizeDaemonAddress(address);
      if (normalized.startsWith('wss://')) {return 'secure';}
      return isPrivateDaemonAddress(normalized) ? 'private' : 'insecure';
    } catch {
      return 'invalid';
    }
  }, [address]);

  const canSave = Boolean(
    address.trim()
      && (profile || token.trim())
      && security !== 'invalid'
      && security !== 'insecure'
      && !saving
      && !removing,
  );

  async function save() {
    if (!canSave) {return;}
    setSaving(true);
    setLocalError(null);
    try {
      const result = await daemon.saveProfile({ name, address, token }, profile?.id);
      await Haptics.notificationAsync(
        result.connected
          ? Haptics.NotificationFeedbackType.Success
          : Haptics.NotificationFeedbackType.Warning,
      );

      onSaved?.();
    } catch (cause) {
      setLocalError(cause instanceof Error ? cause.message : String(cause));
      await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
    } finally {
      setSaving(false);
    }
  }

  async function remove() {
    if (!profile || saving || removing) {return;}
    setRemoving(true);
    setLocalError(null);
    try {
      await daemon.removeProfile(profile.id);
      await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      onRemoved?.();
    } catch (cause) {
      setLocalError(cause instanceof Error ? cause.message : String(cause));
      await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
    } finally {
      setRemoving(false);
    }
  }

  async function confirmRemove() {
    if (!profile || saving || removing) {return;}
    const confirmed = await confirmDestructive(
      `Remove ${profile.name}?`,
      'This removes the saved address and token from this device. Tasks remain on the daemon host.',
      'Remove',
    );

    if (confirmed) {void remove();}
  }

  return (
    <View style={{ paddingBottom: 4 }}>
      <SheetPageHeader
        backDisabled={saving || removing}
        title={profile ? 'Edit Daemon' : 'Add Daemon'}
        onBack={onBack}
      />
      <View style={{ borderRadius: Radius.medium, overflow: 'hidden', backgroundColor: theme.overlay }}>
        <Row style={{ alignItems: 'center', minHeight: 54, paddingLeft: 12, paddingRight: 4 }}>
          <Text style={{ fontSize: 16.5, width: 74, color: theme.text }}>Name</Text>
          <TextInput
            editable={!saving && !removing}
            onChange={(value: string) => {
              setName(value);
              setLocalError(null);
            }}
            onSubmit={() => addressInput.current?.focus()}
            placeholder="Optional"
            returnKeyType="next"
            style={{ flex: '1 1', fontSize: 16.5, minHeight: 53, paddingHorizontal: 8, color: theme.text }}
            value={name}
          />
        </Row>
        <View style={{ height: 1, marginLeft: 12, backgroundColor: theme.separator }} />
        <Row style={{ alignItems: 'center', minHeight: 54, paddingLeft: 12, paddingRight: 4 }}>
          <Text style={{ fontSize: 16.5, width: 74, color: theme.text }}>Address</Text>
          <TextInput
            bind={(h: any) => { addressInput.current = h; }}
            editable={!saving && !removing}
            keyboardType="url"
            onChange={(value: string) => {
              setAddress(value);
              setLocalError(null);
            }}
            onSubmit={() => tokenInput.current?.focus()}
            placeholder="wss://host.example"
            returnKeyType="next"
            style={{ flex: '1 1', fontSize: 16.5, minHeight: 53, paddingHorizontal: 8, color: theme.text }}
            value={address}
          />
        </Row>
        <View style={{ height: 1, marginLeft: 12, backgroundColor: theme.separator }} />
        <Row style={{ alignItems: 'center', minHeight: 54, paddingLeft: 12, paddingRight: 4 }}>
          <Text style={{ fontSize: 16.5, width: 74, color: theme.text }}>Token</Text>
          <TextInput
            bind={(h: any) => { tokenInput.current = h; }}
            editable={!saving && !removing}
            onChange={(value: string) => {
              setToken(value);
              setLocalError(null);
            }}
            onSubmit={() => void save()}
            placeholder={profile ? 'Unchanged' : 'Required'}
            returnKeyType="done"
            secure={!revealed}
            style={{ flex: '1 1', fontSize: 16.5, minHeight: 53, paddingHorizontal: 8, color: theme.text }}
            value={token}
          />
          <Pressable
            accessibilityLabel={revealed ? 'Hide token' : 'Reveal token'}
            accessibilityRole="button"
            accessibilityState={{ selected: revealed }}
            disabled={saving || removing}
            onPress={() => setRevealed((value) => !value)}
            style={{ alignItems: 'center', height: 44, justifyContent: 'center', width: 42 }}
          >
            <AppSymbol
              name={revealed
                ? { ios: 'eye.slash', android: 'visibility_off', web: 'visibility_off' }
                : { ios: 'eye', android: 'visibility', web: 'visibility' }}
              size={18}
              tintColor={theme.textSecondary}
            />
          </Pressable>
        </Row>
      </View>

      <ConnectionFootnote editing={Boolean(profile)} security={security} />

      {localError && (
        <Row accessibilityLiveRegion="polite" style={{ alignItems: 'flex-start', gap: 7, marginHorizontal: 8, marginTop: 8 }}>
          <AppSymbol
            name={{ ios: 'exclamationmark.circle.fill', android: 'error', web: 'error' }}
            size={14}
            tintColor={theme.danger}
          />
          <Text style={{ flex: '1 1', fontSize: 13.5, lineHeight: '17px', color: theme.danger }}>{localError}</Text>
        </Row>
      )}

      <Row style={{ gap: 8, marginTop: 16 }}>
        <Pressable
          accessibilityRole="button"
          accessibilityState={{ disabled: !canSave }}
          disabled={!canSave}
          onPress={() => void save()}
          className="vx-pressable"
          style={{
            alignItems: 'center',
            borderRadius: Radius.medium,
            flex: '1 1',
            flexDirection: 'row',
            gap: 7,
            justifyContent: 'center',
            minHeight: 48,
            paddingHorizontal: 14,
            backgroundColor: String(NativeTint),
            opacity: !canSave ? 0.35 : 1,
          }}
        >
          {saving && <ActivityIndicator busy />}
          <Text style={{ fontSize: 17, fontWeight: '600', color: '#ffffff' }}>
            {saving ? 'Saving…' : profile ? 'Save' : 'Add'}
          </Text>
        </Pressable>
      </Row>

      {profile && (
        <Pressable
          accessibilityRole="button"
          disabled={saving || removing}
          onPress={() => void confirmRemove()}
          className="vx-pressable"
          style={{
            alignItems: 'center',
            flexDirection: 'row',
            gap: 7,
            justifyContent: 'center',
            minHeight: 46,
            marginTop: 4,
          }}
        >
          {removing && <ActivityIndicator busy />}
          <Text style={{ fontSize: 16.5, color: theme.danger }}>
            {removing ? 'Removing…' : 'Remove Daemon'}
          </Text>
        </Pressable>
      )}
    </View>
  );
}

function SheetPageHeader({
  backDisabled = false,
  onBack,
  title,
}: {
  backDisabled?: boolean;
  onBack?: () => void;
  title: string;
}) {
  const theme = useTheme();
  return (
    <Row style={{ alignItems: 'center', height: 46, marginBottom: 4 }}>
      {onBack ? (
        <Pressable
          accessibilityLabel="Back to daemons"
          accessibilityRole="button"
          accessibilityState={{ disabled: backDisabled }}
          disabled={backDisabled}
          hitSlop={8}
          onPress={onBack}
          style={{
            alignItems: 'center',
            height: 44,
            justifyContent: 'center',
            width: 44,
            opacity: backDisabled ? 0.3 : 1,
          }}
        >
          <AppSymbol
            name={{ ios: 'chevron.left', android: 'arrow_back', web: 'arrow_back' }}
            size={20}
            tintColor={String(NativeTint)}
          />
        </Pressable>
      ) : (
        <View style={{ width: 44 }} />
      )}
      <Text numberOfLines={1} style={{ flex: '1 1', fontSize: 18, fontWeight: '600', textAlign: 'center', color: theme.text }}>
        {title}
      </Text>
      <View style={{ width: 44 }} />
    </Row>
  );
}

function ConnectionFootnote({
  editing,
  security,
}: {
  editing: boolean;
  security: ConnectionSecurity;
}) {
  const theme = useTheme();
  let color: string = theme.textTertiary;
  let icon: Parameters<typeof AppSymbol>[0]['name'] = {
    ios: 'info.circle',
    android: 'info',
    web: 'info',
  };

  let message = editing
    ? 'Leave the token blank to keep the saved credential.'
    : 'Copy the address and token from Goddard Desktop → Settings → Daemon.';

  if (security === 'invalid') {
    color = theme.danger;
    icon = { ios: 'exclamationmark.circle.fill', android: 'error', web: 'error' };
    message = 'Enter a valid ws:// or wss:// WebSocket address.';
  } else if (security === 'insecure') {
    color = theme.danger;
    icon = { ios: 'exclamationmark.shield.fill', android: 'gpp_bad', web: 'warning' };
    message = 'Public plaintext connections are blocked. Use wss:// for this host.';
  } else if (security === 'private') {
    color = theme.warning;
    icon = { ios: 'wifi', android: 'wifi', web: 'wifi' };
    message = 'Unencrypted connection. Use it only on a LAN or tailnet you trust.';
  } else if (security === 'secure') {
    color = theme.success;
    icon = { ios: 'lock.fill', android: 'lock', web: 'lock' };
    message = 'Encrypted connection. The token is stored in this device’s keychain.';
  }

  return (
    <Row style={{ alignItems: 'flex-start', gap: 7, marginHorizontal: 8, marginTop: 8 }}>
      <AppSymbol name={icon} size={15} tintColor={color} />
      <Text style={{ flex: '1 1', fontSize: 13.5, lineHeight: '17px', color }}>{message}</Text>
    </Row>
  );
}
