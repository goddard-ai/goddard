import type { Project, WorkingTreeEntry } from '@waku/client';
import { useEffect, useMemo, useState } from 'octane';
import {
  ActivityIndicator,
  Pressable,
  SafeArea,
  Sheet as XSheet,
  Text,
  TextInput,
  View,
  VirtualList,
} from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import { successHaptic, errorHaptic } from '@/lib/haptics';
import { Radius, Spacing } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import {
  browseDaemonDirectory,
  createProject,
  createProjectlessWorkspace,
  daemonKeys,
  persistProject,
  type TaskState,
} from '@/lib/daemon-api';

import { setDaemonQueryData } from '@/lib/daemon-data';
import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import { randomUUID } from '@/lib/uuid';

/** Full-screen project picker — a page-sheet Dialog in xplat terms. */
export function RemoteProjectPicker({
  visible,
  onDismiss,
  onSelect,
}: {
  visible: boolean;
  onDismiss: () => void;
  onSelect: (project: Project) => void;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const [path, setPath] = useState<string | null>(null);
  const [pathDraft, setPathDraft] = useState('');
  const [submitting, setSubmitting] = useState<'folder' | 'empty' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const directory = useDaemonQuery(
    daemonKeys.directory(profileId, path),
    () => {
      if (!daemon.client) {throw new Error('Goddard daemon is disconnected');}
      return browseDaemonDirectory(daemon.client, path);
    },
    {
      enabled: visible && daemon.phase === 'connected' && Boolean(daemon.client),
      staleTime: 10_000,
    },
  );

  const folders = useMemo(
    () => (directory.data?.entries ?? []).filter((entry) => entry.isDir),
    [directory.data?.entries],
  );

  useEffect(() => {
    setPath(null);
    setPathDraft('');
    setError(null);
  }, [profileId]);

  useEffect(() => {
    if (directory.data?.path) {setPathDraft(directory.data.path);}
  }, [directory.data?.path]);

  function visit(next: string | null) {
    if (submitting) {return;}
    setError(null);
    setPath(next);
  }

  async function saveProject(project: Project) {
    if (!daemon.client) {throw new Error('Goddard daemon is disconnected');}
    const saved = await persistProject(daemon.client, project);
    setDaemonQueryData<TaskState>(daemonKeys.taskState(profileId), saved.taskState);
    onSelect(saved.project);
    onDismiss();
    successHaptic();
  }

  async function addFolder() {
    const target = directory.data?.path;
    if (!target || submitting) {return;}
    setSubmitting('folder');
    setError(null);
    try {
      await saveProject(createProject(target, randomUUID()));
    } catch (cause) {
      setError(errorMessage(cause));
      errorHaptic();
    } finally {
      setSubmitting(null);
    }
  }

  async function addEmptyWorkspace() {
    if (!daemon.client || submitting) {return;}
    setSubmitting('empty');
    setError(null);
    try {
      const cwd = await createProjectlessWorkspace(daemon.client);
      await saveProject({ ...createProject(cwd, randomUUID()), name: 'No project' });
    } catch (cause) {
      setError(errorMessage(cause));
      errorHaptic();
    } finally {
      setSubmitting(null);
    }
  }

  if (!visible) {return null;}

  return (
    <XSheet open onDismiss={onDismiss} detents={[1]}>
      <SafeArea style={{ flex: '1 1', backgroundColor: theme.background }}>
        <View style={{
          alignItems: 'center',
          borderBottomWidth: 0.5,
          borderBottomColor: theme.separator,
          flexDirection: 'row',
          minHeight: 52,
          paddingHorizontal: 8,
        }}>
          <Pressable
            accessibilityRole="button"
            disabled={Boolean(submitting)}
            onPress={onDismiss}
            style={{ justifyContent: 'center', minHeight: 44, minWidth: 64, paddingHorizontal: 8 }}
          >
            <Text style={{ fontSize: 17, fontWeight: '600', color: theme.accent }}>Cancel</Text>
          </Pressable>
          <Text style={{ flex: '1 1', fontSize: 18, fontWeight: '700', textAlign: 'center', color: theme.text }}>
            Choose Project
          </Text>
          <Pressable
            accessibilityLabel="Add current folder"
            accessibilityRole="button"
            disabled={!directory.data?.path || Boolean(submitting)}
            onPress={() => void addFolder()}
            style={{ alignItems: 'flex-end', justifyContent: 'center', minHeight: 44, minWidth: 64, paddingHorizontal: 8 }}
          >
            {submitting === 'folder'
              ? <ActivityIndicator busy />
              : (
                <Text style={{ fontSize: 17, fontWeight: '600', color: directory.data?.path ? theme.accent : theme.textTertiary }}>
                  Add
                </Text>
              )}
          </Pressable>
        </View>

        <View style={{ gap: 9, paddingHorizontal: Spacing.three, paddingTop: 13 }}>
          <View style={{
            alignItems: 'center',
            borderRadius: Radius.medium,
            flexDirection: 'row',
            minHeight: 45,
            paddingLeft: 12,
            paddingRight: 7,
            backgroundColor: theme.surface,
          }}>
            <AppSymbol
              name={{ ios: 'folder', android: 'folder', web: 'folder' }}
              size={16}
              tintColor={theme.textTertiary}
            />
            <TextInput
              returnKeyType="go"
              onChange={setPathDraft}
              onSubmit={() => visit(pathDraft.trim() || null)}
              placeholder="Path on daemon host"
              style={{ flex: '1 1', fontSize: 15, minHeight: 44, paddingHorizontal: 9, paddingVertical: 8, color: theme.text }}
              value={pathDraft}
            />
            <Pressable
              accessibilityLabel="Open entered path"
              accessibilityRole="button"
              disabled={!pathDraft.trim() || Boolean(submitting)}
              onPress={() => visit(pathDraft.trim() || null)}
              style={{ alignItems: 'center', height: 36, justifyContent: 'center', width: 36 }}
            >
              <AppSymbol
                name={{ ios: 'arrow.right.circle.fill', android: 'arrow_circle_right', web: 'arrow_circle_right' }}
                size={22}
                tintColor={pathDraft.trim() ? theme.accent : theme.textTertiary}
              />
            </Pressable>
          </View>
          <View style={{ flexDirection: 'row', gap: 7 }}>
            <LocationButton
              label="Home"
              onPress={() => visit(directory.data?.home ?? null)}
            />
            <LocationButton
              disabled={!directory.data?.parent}
              icon={{ ios: 'arrow.up', android: 'arrow_upward', web: 'arrow_upward' }}
              label="Parent"
              onPress={() => visit(directory.data?.parent ?? null)}
            />
            <LocationButton
              disabled={!directory.data?.filesystem_root}
              icon={{ ios: 'internaldrive', android: 'hard_drive', web: 'hard_drive' }}
              label="File system"
              onPress={() => visit(directory.data?.filesystem_root ?? null)}
            />
          </View>
        </View>

        {error || directory.error ? (
          <View accessibilityLiveRegion="polite" style={{
            borderRadius: Radius.medium,
            marginHorizontal: Spacing.three,
            marginTop: 11,
            padding: 10,
            backgroundColor: theme.accentSoft,
          }}>
            <Text style={{ fontSize: 13.5, fontWeight: '600', lineHeight: '17px', color: theme.danger }}>
              {error ?? errorMessage(directory.error)}
            </Text>
          </View>
        ) : null}

        <VirtualList
          items={folders}
          keyExtractor={(item: WorkingTreeEntry) => item.absolutePath}
          style={{ flex: '1 1', padding: Spacing.three }}
          renderItem={(item: WorkingTreeEntry) => (
            <FolderRow entry={item} onOpen={() => visit(item.absolutePath)} />
          )}
          renderEmpty={() => (
            <DirectoryEmpty loading={directory.isPending || directory.isFetching} />
          )}
        />

        <View style={{
          alignItems: 'center',
          borderTopWidth: 0.5,
          borderTopColor: theme.separator,
          flexDirection: 'row',
          gap: 12,
          minHeight: 58,
          paddingHorizontal: Spacing.three,
        }}>
          <Pressable
            accessibilityHint="Creates a private workspace on the daemon host"
            accessibilityRole="button"
            disabled={Boolean(submitting)}
            onPress={() => void addEmptyWorkspace()}
            style={{ alignItems: 'center', flexDirection: 'row', gap: 6, minHeight: 42 }}
          >
            {submitting === 'empty' && <ActivityIndicator busy />}
            <Text style={{ fontSize: 13.5, fontWeight: '600', color: theme.textSecondary }}>
              New empty workspace
            </Text>
          </Pressable>
          <Text numberOfLines={1} style={{ flex: '1 1', fontSize: 11.5, textAlign: 'right', color: theme.textTertiary }}>
            {directory.data?.path ?? 'Loading folder…'}
          </Text>
        </View>
      </SafeArea>
    </XSheet>
  );
}

function LocationButton({
  label,
  icon = { ios: 'house', android: 'home', web: 'home' },
  disabled = false,
  onPress,
}: {
  label: string;
  icon?: Parameters<typeof AppSymbol>[0]['name'];
  disabled?: boolean;
  onPress: () => void;
}) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityRole="button"
      disabled={disabled}
      onPress={onPress}
      style={{
        alignItems: 'center',
        borderRadius: Radius.pill,
        flexDirection: 'row',
        gap: 5,
        minHeight: 32,
        paddingHorizontal: 10,
        backgroundColor: theme.surfaceMuted,
        opacity: disabled ? 0.35 : 1,
      }}
    >
      <AppSymbol name={icon} size={13} tintColor={theme.textSecondary} />
      <Text style={{ fontSize: 12.5, fontWeight: '600', color: theme.textSecondary }}>{label}</Text>
    </Pressable>
  );
}

function FolderRow({ entry, onOpen }: { entry: WorkingTreeEntry; onOpen: () => void }) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityHint="Opens this folder"
      accessibilityRole="button"
      onPress={onOpen}
      style={{
        alignItems: 'center',
        borderRadius: Radius.medium,
        flexDirection: 'row',
        gap: 10,
        minHeight: 54,
        paddingHorizontal: 11,
        backgroundColor: theme.surface,
      }}
    >
      <View style={{
        alignItems: 'center',
        borderRadius: Radius.small,
        height: 34,
        justifyContent: 'center',
        width: 34,
        backgroundColor: theme.surfaceMuted,
      }}>
        <AppSymbol
          name={{ ios: 'folder.fill', android: 'folder', web: 'folder' }}
          size={18}
          tintColor={theme.textSecondary}
        />
      </View>
      <Text numberOfLines={1} style={{ flex: '1 1', fontSize: 15.5, fontWeight: '600', color: theme.text }}>
        {entry.name}
      </Text>
      <AppSymbol
        name={{ ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
        size={14}
        tintColor={theme.textTertiary}
      />
    </Pressable>
  );
}

function DirectoryEmpty({ loading }: { loading: boolean }) {
  const theme = useTheme();
  return (
    <View style={{ alignItems: 'center', flex: '1 1', justifyContent: 'center', minHeight: 220 }}>
      {loading ? <ActivityIndicator busy /> : (
        <View style={{
          alignItems: 'center',
          borderRadius: 18,
          height: 58,
          justifyContent: 'center',
          width: 58,
          backgroundColor: theme.surfaceMuted,
        }}>
          <AppSymbol
            name={{ ios: 'folder', android: 'folder_open', web: 'folder_open' }}
            size={24}
            tintColor={theme.textTertiary}
          />
        </View>
      )}
      <Text style={{ fontSize: 15, fontWeight: '600', marginTop: 12, color: theme.textSecondary }}>
        {loading ? 'Loading folder…' : 'No folders here'}
      </Text>
    </View>
  );
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error && cause.message.trim() ? cause.message : String(cause);
}
