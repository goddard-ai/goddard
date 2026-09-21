import * as Haptics from 'expo-haptics';
import { Stack } from 'expo-router';
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query';
import type { NotificationThread } from '@waku/client';
import { useCallback, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  Linking,
  Pressable,
  SectionList,
  StyleSheet,
  Text,
  View,
} from 'react-native';

import { AppSymbol } from '@/components/app-symbol';
import { NativeTint, Radius } from '@/constants/theme';
import { useTheme } from '@/hooks/use-theme';
import {
  daemonKeys,
  listNotifications,
  markAllNotificationsRead,
  markNotificationDone,
  markNotificationRead,
  markRepoNotificationsRead,
} from '@/lib/daemon-api';
import { useDaemon } from '@/lib/daemon-context';
import { relativeSessionTime } from '@/lib/session-presentation';

const SUBJECT_LABELS: Record<NotificationThread['subjectType'], string> = {
  pullRequest: 'PR',
  issue: 'Issue',
  discussion: 'Discussion',
  release: 'Release',
  commit: 'Commit',
  checkSuite: 'Checks',
  workflowRun: 'Workflow',
  repositoryInvitation: 'Invitation',
  repositoryVulnerabilityAlert: 'Security',
  other: 'Update',
};

const REASON_LABELS: Record<NotificationThread['reason'], string> = {
  assign: 'assigned',
  author: 'author',
  ciActivity: 'CI activity',
  comment: 'comment',
  invitation: 'invitation',
  manual: 'manual',
  mention: 'mention',
  reviewRequested: 'review requested',
  securityAlert: 'security alert',
  stateChange: 'state change',
  subscribed: 'subscribed',
  teamMention: 'team mention',
  other: 'update',
};

interface RepoSection {
  title: string;
  data: NotificationThread[];
}

/** Daemon-wide GitHub inbox — mirrors desktop's notification panel. The
 * ops carry no `cwd`; they read the daemon host's `gh` credential. */
export default function NotificationsScreen() {
  const theme = useTheme();
  const daemon = useDaemon();
  const queryClient = useQueryClient();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const [showAll, setShowAll] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  const inbox = useQuery({
    queryKey: daemonKeys.notifications(profileId, showAll),
    queryFn: () => listNotifications(daemon.client!, showAll),
    enabled: Boolean(daemon.client && daemon.phase === 'connected'),
    placeholderData: keepPreviousData,
  });
  const poll = inbox.data;
  const threads = poll?.status === 'changed' ? poll.threads : [];

  const invalidate = useCallback(async () => {
    await queryClient.invalidateQueries({
      queryKey: ['daemon', profileId, 'notifications'],
    });
  }, [profileId, queryClient]);

  const runOp = useCallback(
    (label: string, op: () => Promise<void>) => {
      if (busy) return;
      setBusy(label);
      void op()
        .then(() => Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success))
        .catch((cause) =>
          Alert.alert(
            'Couldn’t update notifications',
            cause instanceof Error ? cause.message : String(cause),
          ),
        )
        .finally(() => {
          setBusy(null);
          void invalidate();
        });
    },
    [busy, invalidate],
  );

  const sections = useMemo<RepoSection[]>(() => {
    const groups = new Map<string, NotificationThread[]>();
    for (const thread of threads) {
      const group = groups.get(thread.repo);
      if (group) group.push(thread);
      else groups.set(thread.repo, [thread]);
    }
    return [...groups.entries()].map(([title, data]) => ({ title, data }));
  }, [threads]);

  const open = useCallback((thread: NotificationThread) => {
    const url = thread.url ?? thread.repoUrl;
    void Linking.openURL(url).catch(() =>
      Alert.alert('Couldn’t open link', url),
    );
  }, []);

  return (
    <View style={[styles.screen, { backgroundColor: theme.background }]}>
      <Stack.Screen
        options={{
          title: 'Notifications',
          contentStyle: { backgroundColor: theme.background },
        }}
      />
      <View style={[styles.controls, { borderBottomColor: theme.border }]}>
        <View style={[styles.scopeToggle, { backgroundColor: theme.overlay }]}>
          {([false, true] as const).map((all) => (
            <Pressable
              accessibilityRole="button"
              accessibilityState={{ selected: showAll === all }}
              key={all ? 'all' : 'unread'}
              onPress={() => setShowAll(all)}
              style={[
                styles.scopeOption,
                showAll === all && { backgroundColor: theme.surface },
              ]}
            >
              <Text
                style={[
                  styles.scopeLabel,
                  { color: showAll === all ? theme.text : theme.textTertiary },
                ]}
              >
                {all ? 'All' : 'Unread'}
              </Text>
            </Pressable>
          ))}
        </View>
        <Pressable
          accessibilityLabel="Mark all notifications read"
          accessibilityRole="button"
          disabled={busy != null}
          hitSlop={6}
          onPress={() =>
            runOp('all', () => markAllNotificationsRead(daemon.client!))
          }
          style={({ pressed }) => [
            styles.textButton,
            { opacity: busy != null || pressed ? 0.5 : 1 },
          ]}
        >
          <Text style={[styles.textButtonLabel, { color: NativeTint }]}>
            Mark all read
          </Text>
        </Pressable>
      </View>
      {!daemon.client || daemon.phase !== 'connected' ? (
        <InboxMessage
          detail="Connect to a daemon to read its GitHub inbox."
          title="Not connected"
        />
      ) : inbox.isPending ? (
        <View accessibilityLabel="Loading" style={styles.message}>
          <ActivityIndicator color={NativeTint} />
        </View>
      ) : inbox.error ? (
        <InboxMessage
          detail={
            inbox.error instanceof Error
              ? inbox.error.message
              : String(inbox.error)
          }
          title="Couldn’t load notifications"
        />
      ) : poll?.status === 'unavailable' ? (
        <InboxMessage
          detail={
            poll.availability === 'missingCli'
              ? 'Install the GitHub CLI on the daemon host.'
              : 'Run `gh auth login` on the daemon host.'
          }
          title="GitHub unavailable"
        />
      ) : (
        <SectionList<NotificationThread, RepoSection>
          contentContainerStyle={styles.list}
          keyExtractor={(thread) => thread.id}
          ListEmptyComponent={
            <InboxMessage
              detail={
                showAll
                  ? 'The daemon’s GitHub inbox is empty.'
                  : 'Nothing is unread.'
              }
              title={showAll ? 'No notifications' : 'All caught up'}
            />
          }
          renderItem={({ item }) => (
            <Pressable
              accessibilityHint="Opens the thread on GitHub"
              accessibilityRole="button"
              onPress={() => open(item)}
              style={({ pressed }) => [
                styles.row,
                {
                  backgroundColor: pressed ? theme.overlayStrong : 'transparent',
                  borderBottomColor: theme.border,
                },
              ]}
            >
              <View
                style={[
                  styles.unreadDot,
                  { backgroundColor: item.unread ? theme.accent : 'transparent' },
                ]}
              />
              <View style={styles.rowCopy}>
                <Text
                  numberOfLines={2}
                  style={[styles.rowTitle, { color: theme.text }]}
                >
                  {item.title}
                </Text>
                <Text style={[styles.rowMeta, { color: theme.textTertiary }]}>
                  {[
                    SUBJECT_LABELS[item.subjectType],
                    item.number != null ? `#${item.number}` : null,
                    REASON_LABELS[item.reason],
                    relativeSessionTime(item.updatedAt),
                  ]
                    .filter(Boolean)
                    .join(' · ')}
                </Text>
              </View>
              {item.unread ? (
                <Pressable
                  accessibilityLabel={`Mark ${item.title} read`}
                  accessibilityRole="button"
                  hitSlop={8}
                  onPress={() =>
                    runOp(`read:${item.id}`, () =>
                      markNotificationRead(daemon.client!, item.id),
                    )
                  }
                  style={({ pressed }) => [
                    styles.iconButton,
                    { opacity: pressed ? 0.4 : 1 },
                  ]}
                >
                  <AppSymbol
                    name={{
                      ios: 'envelope.open',
                      android: 'mark_email_read',
                      web: 'mark_email_read',
                    }}
                    size={17}
                    tintColor={theme.textSecondary}
                  />
                </Pressable>
              ) : null}
              <Pressable
                accessibilityLabel={`Mark ${item.title} done`}
                accessibilityRole="button"
                hitSlop={8}
                onPress={() =>
                  runOp(`done:${item.id}`, () =>
                    markNotificationDone(daemon.client!, item.id),
                  )
                }
                style={({ pressed }) => [
                  styles.iconButton,
                  { opacity: pressed ? 0.4 : 1 },
                ]}
              >
                <AppSymbol
                  name={{
                    ios: 'checkmark.circle',
                    android: 'check_circle',
                    web: 'check_circle',
                  }}
                  size={17}
                  tintColor={theme.textSecondary}
                />
              </Pressable>
            </Pressable>
          )}
          renderSectionHeader={({ section }) => (
            <View
              style={[
                styles.repoHeader,
                {
                  backgroundColor: theme.background,
                  borderBottomColor: theme.border,
                },
              ]}
            >
              <Text
                numberOfLines={1}
                style={[styles.repoTitle, { color: theme.textSecondary }]}
              >
                {section.title}
              </Text>
              <Pressable
                accessibilityLabel={`Mark ${section.title} read`}
                accessibilityRole="button"
                hitSlop={6}
                onPress={() =>
                  runOp(`repo:${section.title}`, () =>
                    markRepoNotificationsRead(daemon.client!, section.title),
                  )
                }
                style={({ pressed }) => [
                  styles.textButton,
                  { opacity: pressed ? 0.5 : 1 },
                ]}
              >
                <Text style={[styles.repoAction, { color: NativeTint }]}>
                  Mark read
                </Text>
              </Pressable>
            </View>
          )}
          sections={sections}
          stickySectionHeadersEnabled
        />
      )}
    </View>
  );
}

function InboxMessage({ title, detail }: { title: string; detail: string }) {
  const theme = useTheme();
  return (
    <View style={styles.message}>
      <Text style={[styles.messageTitle, { color: theme.text }]}>{title}</Text>
      <Text style={[styles.messageDetail, { color: theme.textTertiary }]}>
        {detail}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  controls: {
    alignItems: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: 'row',
    justifyContent: 'space-between',
    minHeight: 48,
    paddingHorizontal: 14,
  },
  scopeToggle: {
    borderRadius: Radius.small,
    flexDirection: 'row',
    padding: 2,
  },
  scopeOption: {
    borderRadius: Radius.small - 2,
    minHeight: 28,
    justifyContent: 'center',
    paddingHorizontal: 12,
  },
  scopeLabel: { fontSize: 13.5, fontWeight: '500' },
  textButton: { justifyContent: 'center', minHeight: 36, paddingHorizontal: 4 },
  textButtonLabel: { fontSize: 14.5, fontWeight: '500' },
  list: { paddingBottom: 24 },
  repoHeader: {
    alignItems: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: 'row',
    justifyContent: 'space-between',
    paddingHorizontal: 14,
    paddingVertical: 7,
  },
  repoTitle: { flex: 1, fontSize: 12.5, fontWeight: '600', marginRight: 10 },
  repoAction: { fontSize: 13, fontWeight: '500' },
  row: {
    alignItems: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: 'row',
    gap: 10,
    minHeight: 54,
    paddingHorizontal: 14,
    paddingVertical: 9,
  },
  unreadDot: { borderRadius: 4, height: 8, width: 8 },
  rowCopy: { flex: 1, minWidth: 0 },
  rowTitle: { fontSize: 14.5 },
  rowMeta: { fontSize: 12, marginTop: 2 },
  iconButton: {
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 36,
    minWidth: 30,
  },
  message: {
    alignItems: 'center',
    flex: 1,
    gap: 8,
    justifyContent: 'center',
    paddingHorizontal: 32,
  },
  messageTitle: { fontSize: 16, fontWeight: '600' },
  messageDetail: { fontSize: 13.5, textAlign: 'center' },
});
